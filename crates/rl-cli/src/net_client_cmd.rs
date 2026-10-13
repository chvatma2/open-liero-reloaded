//! `rl-cli net-client`: a headless bot client over real connections
//! (networking spec §9.4).

use std::net::{SocketAddr, ToSocketAddrs as _};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use anyhow::{Context as _, anyhow, bail};
use rl_cli::transport::{ClientSpec, PeerMatch, Progress, run_bot_client};
use rl_content::Content;
use rl_net::endpoint::DEFAULT_PORT;
use rl_netcode::netsim::Profile;

/// Joins a server (or a hosted game), readies whenever the lobby gathers,
/// plays its matches with a bot, prints each match's confirmed checksums
/// and leaves; exits with status 1 when it was refused, kicked, lost or
/// timed out.
#[derive(clap::Args)]
pub struct Args {
    /// The host's address, `host[:port]` (the port defaults to 23400).
    #[arg(long)]
    join: String,
    /// A bot plays this client's worm (the only kind of client for now).
    #[arg(long, required = true)]
    bot: bool,
    #[arg(long, value_enum, default_value = "medium")]
    difficulty: Level,
    #[arg(long, default_value = "Net bot")]
    name: String,
    /// Matches to play before leaving.
    #[arg(long, default_value_t = 1)]
    matches: u32,
    /// Watch the matches instead of playing.
    #[arg(long)]
    spectate: bool,
    /// Every datagram it sends passes this network: lan, good, typical, bad
    /// or awful.
    #[arg(long)]
    impair: Option<String>,
    #[arg(long, default_value_t = 1)]
    impair_seed: u64,
    /// Gives up after this many seconds.
    #[arg(long, default_value_t = 3600)]
    timeout: u64,
    /// Imported content (packs written by `rl-cli import`).
    #[arg(long, default_value = "content/packs")]
    content: PathBuf,
}

/// A bot's difficulty.
#[derive(Clone, Copy, clap::ValueEnum)]
enum Level {
    Easy,
    Medium,
    Hard,
    Xtreme,
}

impl From<Level> for rl_bots::Difficulty {
    fn from(l: Level) -> Self {
        match l {
            Level::Easy => Self::Easy,
            Level::Medium => Self::Medium,
            Level::Hard => Self::Hard,
            Level::Xtreme => Self::Xtreme,
        }
    }
}

fn address(join: &str) -> anyhow::Result<SocketAddr> {
    // `host:port`, else `host` on the default port.
    let addrs =
        join.to_socket_addrs().or_else(|_| (join.trim_matches(['[', ']']), DEFAULT_PORT).to_socket_addrs());
    addrs.with_context(|| format!("resolving {join}"))?.next().ok_or_else(|| anyhow!("{join} has no address"))
}

/// Prints one finished match: a header line, then `  checksum <tick> <hex>`
/// per confirmed checksum.
fn print_match(n: usize, m: &PeerMatch) {
    let slot = m.slot.map_or_else(|| "watching".into(), |s| format!("slot {s}"));
    let ticks =
        m.record.as_ref().map_or_else(|| "joined mid-match".into(), |r| format!("{} ticks", r.rows.len()));
    let replay =
        m.replay.as_ref().map_or_else(|| "no replay".into(), |b| format!("replay {} bytes", b.len()));
    println!(
        "match {n}: {} on {}, seed {}, {slot}, {ticks}, {} checksums, {replay}",
        m.start.setup.mod_id,
        m.start.setup.level_id,
        m.start.seed,
        m.checksums.len()
    );
    for (t, c) in &m.checksums {
        println!("  checksum {t} {c:016x}");
    }
}

/// Returns `Ok(false)` if the client was dropped before its matches.
pub fn run(args: &Args) -> anyhow::Result<bool> {
    if !args.bot {
        bail!("only bot clients exist (--bot)");
    }
    let impair = match &args.impair {
        Some(p) => Some(Profile::by_name(p).ok_or_else(|| anyhow!("no network profile {p:?}"))?),
        None => None,
    };
    let content =
        Content::open(&args.content).with_context(|| format!("opening {}", args.content.display()))?;
    let spec = ClientSpec {
        addr: address(&args.join)?,
        name: args.name.clone(),
        difficulty: args.difficulty.into(),
        matches: args.matches,
        impair,
        impair_seed: args.impair_seed,
        spectate: args.spectate,
        deadline: Instant::now() + Duration::from_secs(args.timeout),
    };
    let mut finished = 0;
    let run = run_bot_client(content, &spec, &AtomicBool::new(false), &mut |p| match p {
        Progress::Started(start, slot) => {
            let slot = slot.map_or_else(|| "watching".into(), |s| format!("slot {s}"));
            eprintln!("match starts: {} on {}, {slot}", start.setup.mod_id, start.setup.level_id);
        }
        Progress::Finished(m) => {
            finished += 1;
            print_match(finished, m);
        }
    })?;
    if run.desync_bundles > 0 {
        eprintln!("{} desync bundles", run.desync_bundles);
    }
    match run.dropped {
        Some(why) => {
            eprintln!("dropped after {} matches: {why}", run.matches.len());
            Ok(false)
        }
        None => Ok(true),
    }
}
