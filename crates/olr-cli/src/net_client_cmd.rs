//! `olr-cli net-client`: a headless bot client over real connections
//! (networking spec §9.4), joining by address, invite code, host key or
//! listed name (plan 12b, N46).

use std::net::{SocketAddr, ToSocketAddrs as _};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, anyhow, bail};
use clap::ArgGroup;
use olr_cli::transport::{ClientSpec, PeerMatch, Progress, run_bot_client};
use olr_content::Content;
use olr_discovery::{GameList, Invite, ListedGame, Pool, PoolConfig, PoolEvent};
use olr_net::endpoint::{DEFAULT_PORT, EndpointConfig, IceConfig, TurnServerSpec};
use olr_netcode::netsim::Profile;
use olr_online::join::{join_relays, path_tag};
use olr_online::{Join, MigrationConfig, NostrJoin};
use olr_proto::{FINGERPRINT, PublicKey};

/// Joins a server (or a hosted game), readies whenever the lobby gathers,
/// plays its matches with a bot, prints each match's confirmed checksums
/// and leaves; exits with status 1 when it was refused, kicked, lost or
/// timed out. Prints `path: <path>` once its link to the host opened
/// (direct, open port, punched, relayed). Should the host go mid-match it
/// follows the match to another player, or takes it over (spec §3.7):
/// `migrated: <name> hosts` (or `migrated: hosting`); `host lost: <why>`
/// when nobody did (exit 1).
#[derive(clap::Args)]
#[command(group(ArgGroup::new("game").required(true).args(["join", "invite", "key", "listed"])))]
pub struct Args {
    /// The host's address, `host[:port]` (the port defaults to 23400):
    /// joined directly only.
    #[arg(long)]
    join: Option<String>,
    /// An invite code (`olr1…`): its address first (when it has one), then
    /// through its relays and the `--relay`s.
    #[arg(long)]
    invite: Option<String>,
    /// The host's public key (64 hex characters): joined through the
    /// `--relay`s only.
    #[arg(long)]
    key: Option<String>,
    /// The name of a game listed on the `--relay`s (waited for up to 10 s):
    /// joined as the game's Internet tab joins it, its address first.
    #[arg(long)]
    listed: Option<String>,
    /// A Nostr relay (`ws://` or `wss://`) to join through (and to list
    /// games on); none by default.
    #[arg(long = "relay")]
    relays: Vec<String>,
    /// A STUN server (`host:port`) for a join through the relays; none by
    /// default.
    #[arg(long = "stun")]
    stun: Vec<String>,
    /// A TURN server of this player's, `user:pass@host:port` (the first 2
    /// are used).
    #[arg(long = "turn")]
    turn: Vec<String>,
    /// Leaves TURN relays announced on Nostr unused.
    #[arg(long)]
    no_nostr_turn: bool,
    /// The region whose announced TURN relays come first.
    #[arg(long, value_enum, default_value = "unknown")]
    region: Region,
    /// Local, private and documentation addresses count where public ones
    /// are wanted (listed games, TURN notes, relayed addresses: N62), for
    /// LAN setups and the Docker NAT tests.
    #[arg(long)]
    allow_local: bool,
    /// Leaves as soon as it is in the lobby (exit 0): the join alone.
    #[arg(long)]
    lobby_only: bool,
    /// Opens no standby: should the host go mid-match, this client follows
    /// the match to another player but never takes it over (spec §3.7).
    #[arg(long)]
    no_standby: bool,
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
    /// Imported content (packs written by `olr-cli import`), OLR's own packs
    /// (the `reloaded` directory beside it) over it.
    #[arg(long, default_value = "content/packs")]
    content: PathBuf,
    /// The download cache: what `--content` lacks of a match is downloaded
    /// from the host into it. By default a temporary directory, removed
    /// at exit.
    #[arg(long)]
    cache: Option<PathBuf>,
}

/// A bot's difficulty.
#[derive(Clone, Copy, clap::ValueEnum)]
enum Level {
    Easy,
    Medium,
    Hard,
    Xtreme,
}

/// A region (N52's choice of announced TURN relays).
#[derive(Clone, Copy, clap::ValueEnum)]
enum Region {
    Europe,
    NorthAmericaEast,
    NorthAmericaWest,
    SouthAmerica,
    Asia,
    Oceania,
    Africa,
    Unknown,
}

impl From<Region> for olr_discovery::Region {
    fn from(r: Region) -> Self {
        match r {
            Region::Europe => Self::Europe,
            Region::NorthAmericaEast => Self::NorthAmericaEast,
            Region::NorthAmericaWest => Self::NorthAmericaWest,
            Region::SouthAmerica => Self::SouthAmerica,
            Region::Asia => Self::Asia,
            Region::Oceania => Self::Oceania,
            Region::Africa => Self::Africa,
            Region::Unknown => Self::Unknown,
        }
    }
}

impl From<Level> for olr_bots::Difficulty {
    fn from(l: Level) -> Self {
        match l {
            Level::Easy => Self::Easy,
            Level::Medium => Self::Medium,
            Level::Hard => Self::Hard,
            Level::Xtreme => Self::Xtreme,
        }
    }
}

/// A listed game is waited for this long at most (and never past
/// `--timeout`).
const LISTED_WAIT: Duration = Duration::from_secs(10);

/// What the arguments join.
#[derive(Debug)]
enum Target {
    /// `--join`: an address to resolve, joined directly only.
    Address(String),
    /// `--invite` or `--key`: the join and the host key expected.
    Join { join: Join, expected_host: Option<PublicKey> },
    /// `--listed`: the game of this name on the `--relay`s.
    Listed(String),
}

/// The arguments' target (N46), checked: an invite decoded (its hints
/// first, then the `--relay`s), a key parsed; a key or a listed name
/// needs a `--relay`.
fn target(args: &Args) -> anyhow::Result<Target> {
    if let Some(addr) = &args.join {
        return Ok(Target::Address(addr.clone()));
    }
    if let Some(code) = &args.invite {
        let invite = Invite::decode(code).map_err(|e| anyhow!("--invite: {e}"))?;
        let relays = join_relays(&invite.relay_urls(), &args.relays);
        let join = Join { direct: invite.addr, nostr: nostr_join(args, invite.key, relays) };
        if join.direct.is_none() && join.nostr.is_none() {
            bail!("--invite: this code has no address and no relays: name one with --relay");
        }
        return Ok(Target::Join { join, expected_host: Some(invite.key) });
    }
    if let Some(hex) = &args.key {
        let key = parse_key(hex).ok_or_else(|| anyhow!("--key: not 64 hex characters"))?;
        let nostr = nostr_join(args, key, join_relays(&[], &args.relays))
            .ok_or_else(|| anyhow!("--key joins through the relays: name one with --relay"))?;
        return Ok(Target::Join {
            join: Join { direct: None, nostr: Some(nostr) },
            expected_host: Some(key),
        });
    }
    if let Some(name) = &args.listed {
        if args.relays.is_empty() {
            bail!("--listed finds the game on the relays: name one with --relay");
        }
        return Ok(Target::Listed(name.clone()));
    }
    bail!("one of --join, --invite, --key or --listed")
}

/// A join through Nostr to `host` on `relays` (`None` without relays),
/// with this client's region, announced TURN relays and `allow_local`.
fn nostr_join(args: &Args, host: PublicKey, relays: Vec<String>) -> Option<NostrJoin> {
    (!relays.is_empty()).then(|| NostrJoin {
        host,
        pool: PoolConfig { relays, ..PoolConfig::default() },
        region: args.region.into(),
        turn_from_nostr: !args.no_nostr_turn,
        allow_local: args.allow_local,
    })
}

/// A listed game's join, as the game's Internet tab makes it (N46): its
/// address first (when it announces one), then through the `--relay`s.
fn listed_join(args: &Args, game: &ListedGame) -> Join {
    let direct = game.summary.addr.map(|ip| SocketAddr::new(ip, game.summary.port));
    Join { direct, nostr: nostr_join(args, game.key, join_relays(&[], &args.relays)) }
}

/// 64 hex characters as a key.
fn parse_key(hex: &str) -> Option<PublicKey> {
    let hex = hex.trim().as_bytes();
    if hex.len() != 64 {
        return None;
    }
    let mut key = [0u8; 32];
    for (k, pair) in key.iter_mut().zip(hex.as_chunks::<2>().0) {
        *k = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(key)
}

/// A TURN server, `user:pass@host:port` (the password may hold `:`).
fn turn_server(entry: &str) -> anyhow::Result<TurnServerSpec> {
    let bad = || anyhow!("--turn {entry:?}: not user:pass@host:port");
    let (credentials, addr) = entry.rsplit_once('@').ok_or_else(bad)?;
    let (user, pass) = credentials.split_once(':').ok_or_else(bad)?;
    let (host, port) = addr.rsplit_once(':').ok_or_else(bad)?;
    if user.is_empty() || pass.is_empty() || host.is_empty() || port.parse::<u16>().is_err() {
        return Err(bad());
    }
    Ok(TurnServerSpec { addr: addr.into(), user: user.into(), pass: pass.into() })
}

/// The wall-clock time in unix seconds (notes are judged by it).
fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// The compatible game named `name` listed on the `--relay`s, waited for
/// up to [`LISTED_WAIT`] (and never past `deadline`).
fn find_listed(args: &Args, name: &str, deadline: Instant) -> anyhow::Result<ListedGame> {
    let mut list = GameList::new(FINGERPRINT);
    list.allow_local(args.allow_local);
    let mut pool = Pool::start(PoolConfig { relays: args.relays.clone(), ..PoolConfig::default() });
    pool.subscribe("games", vec![GameList::filter_since(now_unix())]);
    let begun = Instant::now();
    let until = deadline.min(begun + LISTED_WAIT);
    let mut events = Vec::new();
    loop {
        pool.poll(&mut events);
        for e in events.drain(..) {
            if let PoolEvent::Event { event, .. } = e {
                list.offer(&event, now_unix());
            }
        }
        let games = list.games();
        if let Some(g) = games.iter().find(|g| g.compatible && g.summary.name == name) {
            return Ok(g.clone());
        }
        if Instant::now() >= until {
            let other = games
                .iter()
                .find(|g| g.summary.name == name)
                .map_or_else(String::new, |g| format!(" (one of another version is: {})", g.version_hint));
            bail!(
                "no game named {name:?} is listed on the relays (waited {} s){other}",
                begun.elapsed().as_secs()
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// `join` resolved (`host:port`, else `host` on the default port), given up
/// at `deadline`: name resolution (the system's resolver) runs on its own
/// thread, so a resolver that never answers cannot hold the client past
/// `--timeout`.
fn address(join: &str, deadline: Instant) -> anyhow::Result<SocketAddr> {
    let (tx, rx) = std::sync::mpsc::channel();
    let name = join.to_string();
    std::thread::Builder::new().name("olr-cli resolve".into()).spawn(move || {
        let addrs = name
            .to_socket_addrs()
            .or_else(|_| (name.trim_matches(['[', ']']), DEFAULT_PORT).to_socket_addrs())
            .map(|mut a| a.next());
        let _ = tx.send(addrs);
    })?;
    let wait = deadline.saturating_duration_since(Instant::now());
    let resolved =
        rx.recv_timeout(wait).map_err(|_| anyhow!("resolving {join}: no answer within --timeout"))?;
    resolved.with_context(|| format!("resolving {join}"))?.ok_or_else(|| anyhow!("{join} has no address"))
}

/// This client's part in a host migration (spec §3.7), on by default: a
/// standby on port 0 (its datagrams through `--impair`) that asks the
/// `--stun` servers, the `--relay`s to listen on once it hosts (none: no
/// Nostr), and for its joins to a successor through the relays the same
/// ICE, region, TURN notes and `--allow-local` as its first join;
/// `--no-standby`: a follower only.
fn migration_config(args: &Args, impair: Option<Profile>, turn: &[TurnServerSpec]) -> MigrationConfig {
    let ice = IceConfig {
        stun: args.stun.clone(),
        turn: turn.to_vec(),
        allow_local: args.allow_local,
        ..IceConfig::default()
    };
    let standby = (!args.no_standby).then(|| EndpointConfig {
        port: 0,
        impair,
        impair_seed: args.impair_seed,
        ice: IceConfig { stun: args.stun.clone(), allow_local: args.allow_local, ..IceConfig::default() },
        ..EndpointConfig::default()
    });
    MigrationConfig {
        standby,
        relays: (!args.relays.is_empty())
            .then(|| PoolConfig { relays: args.relays.clone(), ..PoolConfig::default() }),
        ice,
        region: args.region.into(),
        turn_from_nostr: !args.no_nostr_turn,
        allow_local: args.allow_local,
        enabled: true,
    }
}

/// The stdout line a migration or a host lost prints (`None` for other
/// progress).
fn migration_line(p: &Progress<'_>) -> Option<String> {
    match *p {
        Progress::Migrated { me: true, .. } => Some("migrated: hosting".into()),
        Progress::Migrated { host, me: false } => Some(format!("migrated: {host} hosts")),
        Progress::HostLost(why) => Some(format!("host lost: {why}")),
        _ => None,
    }
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
    let turn = args.turn.iter().map(|t| turn_server(t)).collect::<anyhow::Result<Vec<_>>>()?;
    // Every wait is bounded by `--timeout`: the name's resolution or the
    // listing here, then `run_bot_client`'s loop. Joining itself never
    // blocks: olr-net's TCP connect runs on its own thread
    // (`CONNECT_TIMEOUT`, 5 s) and the Nostr join and signalling are
    // polled by the loop.
    let deadline = Instant::now() + Duration::from_secs(args.timeout);
    let (join, expected_host) = match target(args)? {
        Target::Address(addr) => (Join::direct(address(&addr, deadline)?), None),
        Target::Join { join, expected_host } => (join, expected_host),
        Target::Listed(name) => {
            let game = find_listed(args, &name, deadline)?;
            let join = listed_join(args, &game);
            let at = join.direct.map_or_else(|| "no address".into(), |a| a.to_string());
            eprintln!("listed: {name:?} ({at})");
            (join, Some(game.key))
        }
    };
    // A join through the relays gathers candidates (N48): STUN from the
    // game's socket, this player's TURN servers; a direct one needs none.
    let ice = match &join.nostr {
        Some(_) => IceConfig {
            stun: args.stun.clone(),
            turn: turn.clone(),
            allow_local: args.allow_local,
            ..IceConfig::default()
        },
        None => IceConfig::default(),
    };
    let content = Content::open_bundled(&args.content)
        .with_context(|| format!("opening {}", args.content.display()))?;
    // Removed when this returns (dropped), unless `--cache` named one.
    let temporary = match &args.cache {
        Some(_) => None,
        None => Some(
            tempfile::Builder::new()
                .prefix("olr-cli-downloads-")
                .tempdir()
                .context("a temporary download cache")?,
        ),
    };
    let cache = args.cache.clone().or_else(|| temporary.as_ref().map(|t| t.path().to_path_buf()));
    let spec = ClientSpec {
        join,
        expected_host,
        ice,
        lobby_only: args.lobby_only,
        name: args.name.clone(),
        difficulty: args.difficulty.into(),
        matches: args.matches,
        impair,
        impair_seed: args.impair_seed,
        spectate: args.spectate,
        deadline,
        cache,
        migration: migration_config(args, impair, &turn),
    };
    let mut finished = 0;
    let run = run_bot_client(content, &spec, &AtomicBool::new(false), &mut |p| {
        if let Some(line) = migration_line(&p) {
            println!("{line}");
        }
        match p {
            Progress::Connected(path, remote) => {
                println!("path: {}", path_tag(path));
                eprintln!("connected to {remote}");
            }
            Progress::Started(start, slot) => {
                let slot = slot.map_or_else(|| "watching".into(), |s| format!("slot {s}"));
                eprintln!("match starts: {} on {}, {slot}", start.setup.mod_id, start.setup.level_id);
            }
            Progress::Finished(m) => {
                finished += 1;
                print_match(finished, m);
            }
            Progress::Migrated { .. } | Progress::HostLost(_) => {}
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

#[cfg(test)]
mod tests {
    use clap::{Args as _, FromArgMatches as _};

    use super::*;

    const RELAY: &str = "ws://127.0.0.1:7000";

    fn parse(args: &[&str]) -> Result<Args, clap::Error> {
        let cmd = Args::augment_args(clap::Command::new("net-client"));
        let m = cmd.try_get_matches_from(std::iter::once("net-client").chain(args.iter().copied()))?;
        Args::from_arg_matches(&m)
    }

    fn resolved(args: &[&str]) -> anyhow::Result<Target> {
        target(&parse(args).unwrap())
    }

    fn code(addr: Option<&str>, relays: &[&str]) -> String {
        let relays: Vec<String> = relays.iter().map(|r| r.to_string()).collect();
        olr_discovery::invite([7; 32], &relays, addr.map(|a| a.parse().unwrap()), 23401).encode()
    }

    /// N46: an invite without an address joins through the relays only
    /// (its hints first, then `--relay`s), its key expected.
    #[test]
    fn an_invite_without_an_address_joins_through_the_relays() {
        let c = code(None, &[RELAY]);
        let Target::Join { join, expected_host } =
            resolved(&["--bot", "--invite", &c, "--relay", "ws://127.0.0.1:7001", "--allow-local"]).unwrap()
        else {
            panic!("a join")
        };
        assert_eq!(join.direct, None);
        let nostr = join.nostr.expect("through the relays");
        assert_eq!(nostr.host, [7; 32]);
        assert_eq!(nostr.pool.relays, [RELAY, "ws://127.0.0.1:7001"], "the hints first");
        assert!(nostr.allow_local && nostr.turn_from_nostr);
        assert_eq!(expected_host, Some([7; 32]));
        // Nothing to join by: no address, no relays.
        let err = resolved(&["--bot", "--invite", &code(None, &[])]).unwrap_err();
        assert!(err.to_string().contains("no address and no relays"), "{err}");
    }

    /// N46: an invite with an address joins it directly first, then
    /// through the relays.
    #[test]
    fn an_invite_with_an_address_joins_it_then_through_the_relays() {
        let c = code(Some("127.0.0.1"), &[RELAY]);
        let Target::Join { join, expected_host } =
            resolved(&["--bot", "--invite", &c, "--no-nostr-turn", "--region", "europe"]).unwrap()
        else {
            panic!("a join")
        };
        assert_eq!(join.direct, Some("127.0.0.1:23401".parse().unwrap()));
        let nostr = join.nostr.expect("then through the relays");
        assert_eq!(nostr.pool.relays, [RELAY]);
        assert!(!nostr.turn_from_nostr && !nostr.allow_local);
        assert_eq!(nostr.region, olr_discovery::Region::Europe);
        assert_eq!(expected_host, Some([7; 32]));
        // Without relays: directly only.
        let Target::Join { join, .. } =
            resolved(&["--bot", "--invite", &code(Some("127.0.0.1"), &[])]).unwrap()
        else {
            panic!("a join")
        };
        assert!(join.direct.is_some() && join.nostr.is_none());
        assert!(resolved(&["--bot", "--invite", "olr1nonsense"]).is_err());
    }

    #[test]
    fn a_key_needs_a_relay() {
        let key = "ab".repeat(32);
        let err = resolved(&["--bot", "--key", &key]).unwrap_err();
        assert!(err.to_string().contains("--relay"), "{err}");
        let Target::Join { join, expected_host } =
            resolved(&["--bot", "--key", &key, "--relay", RELAY]).unwrap()
        else {
            panic!("a join")
        };
        assert_eq!(join.direct, None);
        assert_eq!(join.nostr.expect("through the relay").pool.relays, [RELAY]);
        assert_eq!(expected_host, Some([0xab; 32]));
        for bad in ["ab".repeat(31), "zz".repeat(32), "ab".repeat(33)] {
            assert!(resolved(&["--bot", "--key", &bad, "--relay", RELAY]).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_listed_game_needs_a_relay() {
        let err = resolved(&["--bot", "--listed", "A game"]).unwrap_err();
        assert!(err.to_string().contains("--relay"), "{err}");
        let Target::Listed(name) = resolved(&["--bot", "--listed", "A game", "--relay", RELAY]).unwrap()
        else {
            panic!("a listed game")
        };
        assert_eq!(name, "A game");
    }

    /// Exactly one way to the game.
    #[test]
    fn the_join_flags_conflict() {
        let key = "ab".repeat(32);
        let c = code(Some("127.0.0.1"), &[RELAY]);
        let ways: [&[&str]; 4] =
            [&["--join", "127.0.0.1"], &["--invite", &c], &["--key", &key], &["--listed", "A game"]];
        for (i, a) in ways.iter().enumerate() {
            for b in &ways[i + 1..] {
                let args: Vec<&str> = ["--bot"].iter().chain(a.iter()).chain(b.iter()).copied().collect();
                assert!(parse(&args).is_err(), "{args:?}");
            }
        }
        assert!(parse(&["--bot"]).is_err(), "one is required");
        assert!(matches!(resolved(&["--bot", "--join", "127.0.0.1"]).unwrap(), Target::Address(_)));
    }

    #[test]
    fn turn_servers_are_user_pass_at_host_port() {
        let t = turn_server("u:p:w@198.51.100.1:3478").unwrap();
        assert_eq!((t.user.as_str(), t.pass.as_str(), t.addr.as_str()), ("u", "p:w", "198.51.100.1:3478"));
        for bad in ["u@h:1", ":p@h:1", "u:@h:1", "u:p@h", "u:p@"] {
            assert!(turn_server(bad).is_err(), "{bad}");
        }
    }

    /// Plan 12c: migration is on by default, a standby asking the `--stun`
    /// servers and listening on the `--relay`s once it hosts; `--no-standby`
    /// follows only.
    #[test]
    fn a_standby_unless_no_standby() {
        let args = parse(&[
            "--bot",
            "--join",
            "127.0.0.1",
            "--stun",
            "127.0.0.1:3478",
            "--relay",
            RELAY,
            "--allow-local",
            "--region",
            "europe",
            "--impair",
            "bad",
            "--impair-seed",
            "9",
        ])
        .unwrap();
        let m = migration_config(&args, Profile::by_name("bad"), &[]);
        assert!(m.enabled);
        let standby = m.standby.expect("a standby");
        assert_eq!(standby.port, 0);
        assert_eq!(standby.ice.stun, ["127.0.0.1:3478"]);
        assert!(standby.ice.allow_local);
        assert_eq!((standby.impair.map(|p| p.name), standby.impair_seed), (Some("bad"), 9));
        assert_eq!(m.relays.expect("its relays").relays, [RELAY]);
        assert_eq!(m.ice.stun, ["127.0.0.1:3478"]);
        assert_eq!(m.region, olr_discovery::Region::Europe);
        assert!(m.allow_local && m.turn_from_nostr);
        let args = parse(&["--bot", "--join", "127.0.0.1", "--no-standby", "--no-nostr-turn"]).unwrap();
        let m = migration_config(&args, None, &[]);
        assert!(m.enabled && m.standby.is_none(), "a follower");
        assert!(m.relays.is_none() && !m.turn_from_nostr);
    }

    #[test]
    fn migrations_print_their_lines() {
        assert_eq!(
            migration_line(&Progress::Migrated { host: "Anna", me: false }).unwrap(),
            "migrated: Anna hosts"
        );
        assert_eq!(
            migration_line(&Progress::Migrated { host: "Cli", me: true }).unwrap(),
            "migrated: hosting"
        );
        assert_eq!(
            migration_line(&Progress::HostLost(olr_online::HOST_LOST)).unwrap(),
            format!("host lost: {}", olr_online::HOST_LOST)
        );
        let connected = Progress::Connected(olr_proto::Path::Direct, "127.0.0.1:23400".parse().unwrap());
        assert_eq!(migration_line(&connected), None);
    }
}
