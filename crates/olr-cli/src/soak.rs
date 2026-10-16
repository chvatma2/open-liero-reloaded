//! `olr-cli net-soak` (plan 12c Task 11, N77, networking spec §9.10): seeded
//! online cases, one after the other, for as long as it is asked, to find
//! the failures a fixed grid does not. Most are `net-sim` cases (virtual
//! time, a few seconds each); every few a real-transport run on 127.0.0.1.
//!
//! **Bounded.** Each case's report is dropped before the next, and nothing
//! else is kept per run, so an overnight run's memory stays flat
//! (`tests/net_soak_memory.rs` pins it). A failure is a file, at most
//! [`MAX_FAILURES`] of them per invocation (files of earlier runs stay and
//! are not counted); then the soak stops with its summary. Time is checked
//! between cases, so a run ends up to one case (a transport one: about a
//! minute) late. A
//! `net-sim` failure is a `NetCase` (`net-<stem>.ron`, replayed with
//! `olr-cli net-sim --from`; kept as a regression with `cargo xtask
//! bless-regression --from`); a real-transport one a directory
//! `soak-transport-<seed>/` with the spec (RON), the verdict and the
//! server's log. Nothing leaves the machine: net-sim is virtual, transport
//! cases bind 127.0.0.1 only.

use std::any::Any;
use std::io::Write;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::Context as _;
use olr_bots::Difficulty;
use olr_content::Content;
use olr_netcode::Rng;

use crate::netsim::{self, NetCase, NetSpec};
use crate::transport::{ServerExit, ServerExitHow, TransportSpec, run_transport};

/// The most failures a soak keeps; it stops with the one that reaches it.
pub const MAX_FAILURES: usize = 32;

/// What a soak does.
#[derive(Clone, Debug)]
pub struct SoakConfig {
    pub minutes: f64,
    /// The first case's seed; case `n` (counted from 0) is seeded `seed + n`.
    pub seed: u64,
    /// Every this many runs one is a real-transport case (0: none).
    pub transport_every: u32,
    /// Stop after this many runs (tests), else when the time is up.
    pub runs: Option<u32>,
    /// Stop after this many failures ([`MAX_FAILURES`]).
    pub max_failures: usize,
    pub failures: PathBuf,
    pub content_dir: PathBuf,
}

const MODS: [&str; 2] = ["olx-classic", "olx-mw-1-0"];
const LEVELS: [&str; 4] =
    ["olx-level-castlestrike", "olx-level-dirt-level", "olx-level-fossilfacility", "olx-level-liero-factory"];
const PROFILES: [&str; 5] = ["lan", "good", "typical", "bad", "awful"];
const DIFFICULTIES: [&str; 4] = ["easy", "medium", "hard", "xtreme"];

/// The net-sim case of `seed` (N77): its draws, all within `NetSpec`'s
/// bounds. A pure function of the seed.
pub fn case(seed: u64) -> NetSpec {
    generate(seed, 1500, 6000)
}

/// [`case`] with a match of 600 ticks, for tests (everything else drawn
/// the same way, scaled to the match).
#[doc(hidden)]
pub fn case_short(seed: u64) -> NetSpec {
    generate(seed, 600, 600)
}

fn pick<'a, T>(rng: &mut Rng, of: &'a [T]) -> &'a T {
    &of[rng.below(of.len() as u64) as usize]
}

fn generate(seed: u64, min_ticks: u32, max_ticks: u32) -> NetSpec {
    let mut rng = Rng::new(seed);
    let r = &mut rng;
    let mod_id = (*pick(r, &MODS)).to_string();
    let level_id = (*pick(r, &LEVELS)).to_string();
    let slots = 2 + r.below(15) as u8;
    let clients = 1 + r.below(u64::from(slots.min(7))) as u8;
    let spectators = r.below(3) as u8;
    let ticks = min_ticks + r.below(u64::from(max_ticks - min_ticks) + 1) as u32;
    let profile = (*pick(r, &PROFILES)).to_string();
    let match_seed = r.next_u64() as u32;
    let net_seed = r.next_u64();
    let drift_ppm = r.below(5) as i32 * 500;
    let difficulty = match r.below(5) {
        0 => None,
        n => Some(DIFFICULTIES[n as usize - 1].to_string()),
    };
    // One spectator in two arrives mid-match (all of them at once).
    let late = r.below(2) == 1;
    let join = ticks / 10 + r.below(u64::from(ticks) * 6 / 10) as u32;
    let join_tick = (spectators > 0 && late).then_some(join);
    // Clients are peers 1.., spectators 100..: any but the host.
    let peers: Vec<u32> = (1..=u32::from(clients)).chain(100..100 + u32::from(spectators)).collect();
    let mut profile_changes = Vec::new();
    for _ in 0..r.below(4) {
        let tick = ticks / 10 + r.below(u64::from(ticks) * 8 / 10) as u32;
        let peer = *pick(r, &peers);
        profile_changes.push((tick, peer, (*pick(r, &PROFILES)).to_string()));
    }
    profile_changes.sort();
    let mut outages = Vec::new();
    for _ in 0..r.below(3) {
        let len = (50 + r.below(551) as u32).min(ticks / 2);
        let from = 100 + r.below(u64::from(ticks.saturating_sub(len + 200)).max(1)) as u32;
        outages.push((*pick(r, &peers), from, from + len));
    }
    outages.sort();
    // In half of the cases the host goes: crash, departure or freeze, at a
    // random tick in the middle 60 % of the match.
    let (mut exits, mut host_freeze) = (Vec::new(), None);
    if r.below(2) == 1 {
        let kind = r.below(3);
        let tick = ticks / 5 + r.below(u64::from(ticks) * 6 / 10 + 1) as u32;
        let freeze_ticks = 160 + r.below(91) as u32; // 1.6 to 2.5 s
        match kind {
            0 => exits.push((0, tick, "crash".to_string())),
            1 => exits.push((0, tick, "leave".to_string())),
            _ => host_freeze = Some((tick, tick + freeze_ticks)),
        }
    }
    // Plan 12d: in one case in four with two clients or more, a client
    // departs (a leave, its `Bye`: withdrawn at once; or a crash: withdrawn
    // at its timeout, net-sim's crash window excusing the fills' stalls,
    // ruling PF12d-5) at a random tick of the middle 60 %. Drawn last, so
    // every draw above stays as it was for a seed.
    if clients >= 2 && r.below(4) == 0 {
        let peer = 1 + r.below(u64::from(clients)) as u32;
        let tick = ticks / 5 + r.below(u64::from(ticks) * 6 / 10 + 1) as u32;
        let how = if r.below(2) == 0 { "leave" } else { "crash" };
        exits.push((peer, tick, how.to_string()));
    }
    NetSpec {
        mod_id,
        level_id,
        seed: match_seed,
        slots,
        clients,
        spectators,
        join_tick,
        profile,
        drift_ppm,
        ticks,
        outages,
        net_seed,
        difficulty,
        faults: Vec::new(),
        exits,
        host_freeze,
        profile_changes,
        migration: false,
    }
}

/// The real-transport case of `seed`: 2-4 clients through good, typical or
/// bad, a 0.3-0.5 minute match, and in half of them the server freezes or
/// leaves at a random time of its first part (migration on, the server's own
/// bots playing on); the others are clean control runs (migration off).
/// [`run_cases`] gives each its own `data_dir`.
pub fn transport_case(seed: u64, data_dir: &Path) -> TransportSpec {
    let mut rng = Rng::new(seed);
    let r = &mut rng;
    let players = 2 + r.below(3) as u8;
    let profile = *pick(r, &["good", "typical", "bad"]);
    let minutes = [0.3_f32, 0.4, 0.5][r.below(3) as usize];
    let mut spec = TransportSpec::new(players, minutes, Some(profile), data_dir);
    spec.net_seed = r.next_u64();
    spec.difficulty = match r.below(5) {
        0 => None,
        n => {
            Some([Difficulty::Easy, Difficulty::Medium, Difficulty::Hard, Difficulty::Xtreme][n as usize - 1])
        }
    };
    spec.min_checksums = 3;
    let exit = r.below(2) == 1;
    let how = if r.below(2) == 1 { ServerExitHow::Freeze } else { ServerExitHow::Leave };
    let at = r.unit();
    let bots = r.below(3) as u8;
    if exit {
        // 25 % to 70 % into the match.
        let secs = f64::from(minutes) * 60.0;
        spec.server_exit = Some(ServerExit { after_s: (secs * (0.25 + 0.45 * at)) as f32, how });
        spec.migration = true;
        spec.fill_bots = players + 1 + bots;
    }
    spec
}

/// One case of the soak.
pub enum SoakCase {
    Net(NetSpec),
    Transport(TransportSpec),
}

/// What a soak did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SoakSummary {
    pub runs: u32,
    pub transport_runs: u32,
    /// The files (a directory for a transport case) kept, one per failure.
    pub failures: Vec<PathBuf>,
}

/// A case's end.
enum Outcome {
    Ok,
    /// What went wrong, in one line, and where it was kept.
    Failed(String, PathBuf),
}

/// Runs cases until the time or `runs` is up, one line each to `out`:
/// `run <n> seed <s> <stem>: ok (<secs>s)` or `FAIL <what> at tick <t>
/// (<file>)` (a transport or error case has no tick); then the summary line. A case that
/// cannot even run (an error, or a panic) is a failure too, kept like the
/// others. `run` with the cases from [`case`] and [`transport_case`].
pub fn run(cfg: &SoakConfig, out: &mut dyn Write) -> anyhow::Result<SoakSummary> {
    let every = cfg.transport_every;
    run_cases(
        cfg,
        &mut |n, seed| {
            if every > 0 && (n + 1) % every == 0 {
                SoakCase::Transport(transport_case(seed, Path::new("")))
            } else {
                SoakCase::Net(case(seed))
            }
        },
        out,
    )
}

/// [`run`] with the cases from `cases(n, seed)` (`n` counted from 0, `seed`
/// the config's plus `n`); a transport case's `data_dir` is replaced by a
/// temporary directory of its own, removed after it.
pub fn run_cases(
    cfg: &SoakConfig,
    cases: &mut dyn FnMut(u32, u64) -> SoakCase,
    out: &mut dyn Write,
) -> anyhow::Result<SoakSummary> {
    let content = Content::open(&cfg.content_dir)
        .with_context(|| format!("opening the content in {}", cfg.content_dir.display()))?;
    let begun = Instant::now();
    let budget = cfg.minutes * 60.0;
    let mut summary = SoakSummary::default();
    loop {
        let more = match cfg.runs {
            Some(runs) => summary.runs < runs,
            None => begun.elapsed().as_secs_f64() < budget,
        };
        if !more {
            break;
        }
        let n = summary.runs;
        let seed = cfg.seed.wrapping_add(u64::from(n));
        let started = Instant::now();
        let (stem, outcome) = match cases(n, seed) {
            SoakCase::Net(spec) => (spec.stem(), run_net(cfg, &content, &spec)),
            SoakCase::Transport(spec) => {
                summary.transport_runs += 1;
                (format!("soak-transport-{seed}"), run_transport_case(cfg, &content, spec, seed))
            }
        };
        summary.runs += 1;
        match outcome {
            Outcome::Ok => writeln!(
                out,
                "run {} seed {seed} {stem}: ok ({:.1}s)",
                n + 1,
                started.elapsed().as_secs_f64()
            )?,
            Outcome::Failed(what, file) => {
                writeln!(out, "FAIL {what} ({})", file.display())?;
                summary.failures.push(file);
            }
        }
        if summary.failures.len() >= cfg.max_failures {
            writeln!(
                out,
                "stopping: {} failures kept in {}",
                summary.failures.len(),
                cfg.failures.display()
            )?;
            break;
        }
    }
    writeln!(
        out,
        "soak: {} runs ({} transport), {} failures in {:.1} min",
        summary.runs,
        summary.transport_runs,
        summary.failures.len(),
        begun.elapsed().as_secs_f64() / 60.0,
    )?;
    Ok(summary)
}

/// A panic's message.
fn panic_text(p: &(dyn Any + Send)) -> String {
    p.downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| p.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic".into())
}

fn run_net(cfg: &SoakConfig, content: &Content, spec: &NetSpec) -> Outcome {
    let run = catch_unwind(AssertUnwindSafe(|| netsim::run_with(content, spec, Some(&cfg.failures))))
        .unwrap_or_else(|p| Err(anyhow::anyhow!("panic: {}", panic_text(&*p))));
    let intended = cfg.failures.join(format!("net-{}.ron", spec.stem()));
    let (mut what, kept) = match run {
        Ok(run) => match &run.verdict {
            Ok(()) => return Outcome::Ok,
            Err(f) => (format!("{} at tick {}", f.what, f.tick), netsim::write_failure(&cfg.failures, &run)),
        },
        Err(e) => (format!("error: {e:#}"), write_error_case(&cfg.failures, spec, &e)),
    };
    // A failure that cannot be kept (a full disk) is still reported and counted.
    let file = kept.unwrap_or_else(|e| {
        what += &format!("; not kept: {e:#}");
        intended
    });
    Outcome::Failed(what, file)
}

/// Keeps a case that could not run, as `<dir>/net-<stem>.ron`: a `NetCase`
/// whose note names the error (`netsim::write_failure` keeps only a failed
/// verdict).
pub fn write_error_case(dir: &Path, spec: &NetSpec, error: &anyhow::Error) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let case = NetCase { spec: spec.clone(), failure: Some(format!("error: {error:#}")) };
    let text = ron::ser::to_string_pretty(&case, ron::ser::PrettyConfig::default())?;
    let path = dir.join(format!("net-{}.ron", spec.stem()));
    std::fs::write(&path, text + "\n").with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

fn run_transport_case(cfg: &SoakConfig, content: &Content, mut spec: TransportSpec, seed: u64) -> Outcome {
    let data = match tempfile::tempdir() {
        Ok(d) => d,
        Err(e) => {
            let what = format!("error: no data directory for the transport case: {e}");
            return Outcome::Failed(what, cfg.failures.join(format!("soak-transport-{seed}")));
        }
    };
    spec.data_dir = data.path().to_path_buf();
    let run = catch_unwind(AssertUnwindSafe(|| run_transport(&cfg.content_dir, &spec)))
        .unwrap_or_else(|p| Err(anyhow::anyhow!("panic: {}", panic_text(&*p))));
    let (what, log) = match run {
        Ok(run) => match run.verdict(content, &spec) {
            Ok(()) => return Outcome::Ok,
            Err(why) => (why, run.server_log),
        },
        Err(e) => (format!("error: {e:#}"), String::new()),
    };
    let dir = cfg.failures.join(format!("soak-transport-{seed}"));
    let mut first = what.lines().next().unwrap_or("").to_string();
    if let Err(e) = keep_transport(&dir, &mut spec, &what, &log, data.path()) {
        first += &format!("; not kept: {e:#}");
    }
    Outcome::Failed(first, dir)
}

/// Writes a failed transport case's files to `dir`.
fn keep_transport(
    dir: &Path,
    spec: &mut TransportSpec,
    what: &str,
    log: &str,
    data: &Path,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    // The spec as it ran, its data directory a placeholder (a temporary one, gone).
    spec.data_dir = PathBuf::from("data");
    let ron = ron::ser::to_string_pretty(spec, ron::ser::PrettyConfig::default())?;
    std::fs::write(dir.join("spec.ron"), ron + "\n")?;
    std::fs::write(dir.join("failure.txt"), format!("{what}\n"))?;
    std::fs::write(dir.join("server.log"), log)?;
    for (i, bundle) in desync_bundles(data).iter().enumerate().take(8) {
        let _ = std::fs::copy(bundle, dir.join(format!("desync-{i}.rlds")));
    }
    Ok(())
}

/// The desync bundles under `dir` (a few levels down), in path order.
fn desync_bundles(dir: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, depth: u32, found: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
        paths.sort();
        for p in paths {
            if p.is_dir() && depth < 4 {
                walk(&p, depth + 1, found);
            } else if p.extension().is_some_and(|e| e == "rlds") {
                found.push(p);
            }
        }
    }
    let mut found = Vec::new();
    walk(dir, 0, &mut found);
    found
}
