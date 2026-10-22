//! `olr-cli net-sim`: an online match of bots in one process over the
//! network model (networking spec §9.1).

use std::ops::Range;
use std::path::PathBuf;

use anyhow::{Context as _, bail};
use olr_cli::netsim::{NetCase, NetSpec, run_with, write_failure};

/// Plays an online match in one process (a host, clients and spectators with
/// bots on every peer) over a simulated network and checks that every peer
/// agrees. `--from` replays a case; a failure writes its case to `--failures`.
#[derive(clap::Args)]
pub struct Args {
    #[command(flatten)]
    spec: NetSpecArgs,
    /// A net case (`target/failures/net-*.ron`, `fixtures/net/cases/*.ron`)
    /// to play instead of the options above.
    #[arg(long)]
    from: Option<PathBuf>,
    /// Play the match once for each seed in `a..b` (or `a..=b`), as the match
    /// seed and the network seed.
    #[arg(long, value_parser = parse_seeds)]
    seeds: Option<Range<u32>>,
    /// Imported content (packs written by `olr-cli import`), OLR's own packs
    /// (the `reloaded` directory beside it) over it.
    #[arg(long, default_value = "content/packs")]
    content: PathBuf,
    /// Where a failing run writes its case.
    #[arg(long, default_value = "target/failures")]
    failures: PathBuf,
    /// Measure the feel numbers instead (every profile × seeds 1..=2 and the
    /// 32-worm bandwidth run, `olr_cli::feel`; the options above are not
    /// used) and write them to this RON file.
    #[arg(long, conflicts_with_all = ["from", "seeds"])]
    feel: Option<PathBuf>,
}

#[derive(clap::Args)]
struct NetSpecArgs {
    #[arg(long = "mod", default_value = "olx-classic")]
    mod_id: String,
    #[arg(long = "level", default_value = "olx-level-castlestrike")]
    level_id: String,
    #[arg(long, default_value_t = 1)]
    seed: u32,
    /// Worms; clients take slots 0.., the host's bots the rest.
    #[arg(long, default_value_t = 8)]
    slots: u8,
    #[arg(long, default_value_t = 3)]
    clients: u8,
    #[arg(long, default_value_t = 1)]
    spectators: u8,
    /// Spectators arrive at this tick.
    #[arg(long)]
    join_tick: Option<u32>,
    /// lan, good, typical, bad or awful.
    #[arg(long, default_value = "typical")]
    profile: String,
    /// Clients' clock drift spread, in ppm.
    #[arg(long, default_value_t = 0)]
    drift_ppm: i32,
    #[arg(long, default_value_t = 1000)]
    ticks: u32,
    /// `peer:from_tick:until_tick`: nothing passes between host and peer (repeatable).
    #[arg(long = "outage", value_parser = parse_outage)]
    outages: Vec<(u32, u32, u32)>,
    #[arg(long, default_value_t = 0)]
    net_seed: u64,
    /// easy, medium, hard, xtreme or mixed (default).
    #[arg(long)]
    difficulty: Option<String>,
    /// `peer:tick:crash|leave`: the peer (0 the host) crashes or leaves when
    /// the current host's tick reaches `tick` (repeatable). An exit of the
    /// host turns host migration on.
    #[arg(long = "exit", value_parser = parse_exit)]
    exits: Vec<(u32, u32, String)>,
    /// `from:until`: the host frozen from its tick `from` for `until - from`
    /// ticks' worth of time (it steps down past 1.5 s). Turns host
    /// migration on.
    #[arg(long, value_parser = parse_freeze)]
    host_freeze: Option<(u32, u32)>,
    /// `tick:peer:profile`: from the host's tick `tick`, the peer's link to
    /// whoever hosts takes the profile (repeatable).
    #[arg(long = "profile-change", value_parser = parse_profile_change)]
    profile_changes: Vec<(u32, u32, String)>,
    /// Host migration on (spec §3.7) even without a host exit or freeze.
    #[arg(long)]
    migration: bool,
    /// Every bot also presses Strafe and Previous/Next weapon (issue #23).
    #[arg(long)]
    strafe: bool,
    /// Every bot also holds Weapon 1-5 (issue #48).
    #[arg(long)]
    weapon_keys: bool,
}

impl From<NetSpecArgs> for NetSpec {
    fn from(a: NetSpecArgs) -> NetSpec {
        NetSpec {
            mod_id: a.mod_id,
            level_id: a.level_id,
            seed: a.seed,
            slots: a.slots,
            clients: a.clients,
            spectators: a.spectators,
            join_tick: a.join_tick,
            profile: a.profile,
            drift_ppm: a.drift_ppm,
            ticks: a.ticks,
            outages: a.outages,
            net_seed: a.net_seed,
            difficulty: a.difficulty,
            faults: Vec::new(),
            exits: a.exits,
            host_freeze: a.host_freeze,
            profile_changes: a.profile_changes,
            migration: a.migration,
            strafe: a.strafe,
            weapon_keys: a.weapon_keys,
        }
    }
}

fn parse_outage(s: &str) -> Result<(u32, u32, u32), String> {
    let parts: Vec<&str> = s.split(':').collect();
    let [p, a, b] = parts[..] else { return Err("expected peer:from_tick:until_tick".into()) };
    let num = |x: &str| x.parse::<u32>().map_err(|e| e.to_string());
    Ok((p.parse::<u32>().map_err(|e| e.to_string())?, num(a)?, num(b)?))
}

fn parse_exit(s: &str) -> Result<(u32, u32, String), String> {
    let parts: Vec<&str> = s.split(':').collect();
    let [p, t, how] = parts[..] else { return Err("expected peer:tick:crash|leave".into()) };
    if !matches!(how, "crash" | "leave") {
        return Err(format!("{how:?} is neither crash nor leave"));
    }
    let num = |x: &str| x.parse::<u32>().map_err(|e| e.to_string());
    Ok((num(p)?, num(t)?, how.to_string()))
}

fn parse_freeze(s: &str) -> Result<(u32, u32), String> {
    let (a, b) = s.split_once(':').ok_or("expected from_tick:until_tick")?;
    let num = |x: &str| x.parse::<u32>().map_err(|e| e.to_string());
    let (from, until) = (num(a)?, num(b)?);
    if until <= from {
        return Err(format!("the freeze ends at tick {until}, not after it begins at {from}"));
    }
    Ok((from, until))
}

fn parse_profile_change(s: &str) -> Result<(u32, u32, String), String> {
    let parts: Vec<&str> = s.split(':').collect();
    let [t, p, profile] = parts[..] else { return Err("expected tick:peer:profile".into()) };
    if olr_netcode::netsim::Profile::by_name(profile).is_none() {
        return Err(format!("no network profile {profile:?} (lan, good, typical, bad, awful)"));
    }
    let num = |x: &str| x.parse::<u32>().map_err(|e| e.to_string());
    Ok((num(t)?, num(p)?, profile.to_string()))
}

fn parse_seeds(s: &str) -> Result<Range<u32>, String> {
    let (a, b, inclusive) = match s.split_once("..=") {
        Some((a, b)) => (a, b, true),
        None => {
            let (a, b) = s.split_once("..").ok_or("expected a..b or a..=b")?;
            (a, b, false)
        }
    };
    let a: u32 = a.parse().map_err(|e: std::num::ParseIntError| e.to_string())?;
    let b: u32 = b.parse().map_err(|e: std::num::ParseIntError| e.to_string())?;
    let end = if inclusive { b.checked_add(1).ok_or("seed out of range")? } else { b };
    if end <= a {
        return Err("no seeds in the range".into());
    }
    Ok(a..end)
}

/// Returns `Ok(false)` if any run failed.
pub fn run_cmd(args: Args) -> anyhow::Result<bool> {
    let content = olr_content::Content::open_bundled(&args.content)?;
    if let Some(out) = &args.feel {
        return feel(&content, out);
    }
    let base: NetSpec = match &args.from {
        Some(path) => {
            let case = NetCase::load(path)?;
            if let Some(f) = &case.failure {
                println!("recorded failure: {f}");
            }
            case.spec
        }
        None => args.spec.into(),
    };
    let specs: Vec<NetSpec> = match &args.seeds {
        Some(range) => {
            range.clone().map(|seed| NetSpec { seed, net_seed: u64::from(seed), ..base.clone() }).collect()
        }
        None => vec![base],
    };
    if specs.is_empty() {
        bail!("nothing to play");
    }
    let mut ok = true;
    for spec in &specs {
        let r = run_with(&content, spec, Some(&args.failures))
            .with_context(|| format!("running {}", spec.stem()))?;
        println!("{}", r.summary());
        for b in &r.bundles {
            println!("wrote {}; replay with: olr-cli net-repro {}", b.display(), b.display());
        }
        match &r.verdict {
            Ok(()) => println!("ok"),
            Err(f) => {
                ok = false;
                println!("FAIL {} at tick {}", f.what, f.tick);
                let path = write_failure(&args.failures, &r)?;
                println!("wrote {}; replay with: olr-cli net-sim --from {}", path.display(), path.display());
            }
        }
    }
    Ok(ok)
}

/// `--feel`: measures, prints one line per profile, writes the report.
fn feel(content: &olr_content::Content, out: &std::path::Path) -> anyhow::Result<bool> {
    let report = olr_cli::feel::measure(content)?;
    for p in &report.profiles {
        println!("{}", p.line());
    }
    println!("bandwidth32 {}", report.bandwidth32.line());
    if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let text = ron::ser::to_string_pretty(&report, ron::ser::PrettyConfig::default())? + "\n";
    std::fs::write(out, text).with_context(|| format!("writing {}", out.display()))?;
    println!("wrote {}", out.display());
    Ok(true)
}
