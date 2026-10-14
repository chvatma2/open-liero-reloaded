//! `rl-cli nostr-probe` and `rl-cli stun-probe` (plan 12a Task 8): measure
//! public Nostr relays and STUN servers, by hand, to choose the default
//! lists (`rl_discovery::defaults`). No test and nothing in `cargo xtask
//! check` runs them against the internet: the tests use `TestRelay` and
//! `StunResponder`.
//!
//! Every event the relay probe publishes is a probe event: a fresh random key
//! per run (never the player's identity), the content `"rust-liero relay
//! probe"`, tags `d` and `t` = `rust-liero-probe` (not the game's
//! `rust-liero`, so game lists ignore it) and an expiration 10 minutes ahead.
//! Per relay the probe opens two connections and publishes two notes and one
//! ephemeral event:
//! 1. connects both;
//! 2. publishes a kind-31470 note (announcements) and a kind-31471 note
//!    (TURN relays, plan 12b) on the first and records their `OK`s;
//! 3. reads them back at once on the second (`#t` filter);
//! 4. subscribes to kind 21470 on the second, publishes one of
//!    `MAX_HANDSHAKE_CONTENT` (16 KiB of random base64, the largest
//!    handshake) on the first, and waits for it to be forwarded (plan 12b's
//!    handshakes need this);
//! 5. closes the second, waits (`--wait-min`, 3 minutes by default) and
//!    reads both notes back on the first.
//!
//! A relay passes when it accepts both notes, keeps them (both reads),
//! forwards the ephemeral event and refuses nothing for payment or
//! authentication.

use std::net::{Ipv4Addr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use rl_discovery::defaults::{DEFAULT_RELAYS, DEFAULT_STUN};
use rl_discovery::handshake::MAX_HANDSHAKE_CONTENT;
use rl_discovery::list::is_global;
use rl_discovery::turn_note::KIND_TURN;
use rl_discovery::{
    Event, Filter, KIND_ANNOUNCE, KIND_HANDSHAKE, Pool, PoolConfig, PoolEvent, RelayStatus, binding_request,
    parse_binding_response,
};
use rl_lobby::Identity;

/// The content of every probe event.
pub const PROBE_CONTENT: &str = "rust-liero relay probe";
/// The `d` and `t` tag of every probe event (game lists filter on
/// `rust-liero`, so they never see it).
pub const PROBE_TAG: &str = "rust-liero-probe";
/// Probe events expire this many seconds after their `created_at`.
pub const PROBE_EXPIRY: u64 = 600;
/// Between two polls of a pool.
const POLL: Duration = Duration::from_millis(10);
/// Relay messages in the table are cut to this many characters.
const MESSAGE_CHARS: usize = 80;

/// A fresh random key for one run: never the player's identity.
pub fn probe_key() -> std::io::Result<Identity> {
    for _ in 0..8 {
        if let Some(id) = Identity::from_secret(rl_net::random_bytes::<32>()?) {
            return Ok(id);
        }
    }
    Err(std::io::Error::other("no valid random key"))
}

/// A probe event of `kind`, created at `now_unix`.
pub fn probe_event(key: &Identity, kind: u32, now_unix: u64) -> Event {
    let tags = vec![
        vec!["d".to_owned(), PROBE_TAG.to_owned()],
        vec!["t".to_owned(), PROBE_TAG.to_owned()],
        vec!["expiration".to_owned(), now_unix.saturating_add(PROBE_EXPIRY).to_string()],
    ];
    let aux = rl_net::random_bytes::<32>().unwrap_or([0; 32]);
    Event::sign(key, now_unix, kind, tags, PROBE_CONTENT.to_owned(), aux)
}

/// The probe's ephemeral event: kind 21470, marked as [`probe_event`]
/// marks, with [`MAX_HANDSHAKE_CONTENT`] bytes of random base64 as its
/// content (the largest handshake a relay must forward).
pub fn probe_handshake(key: &Identity, now_unix: u64) -> Event {
    // 3 random bytes make 4 base64 characters, without padding.
    let len = MAX_HANDSHAKE_CONTENT / 4 * 3;
    let mut raw = Vec::with_capacity(len + 32);
    while raw.len() < len {
        raw.extend_from_slice(&rl_net::random_bytes::<32>().unwrap_or([0x5a; 32]));
    }
    raw.truncate(len);
    let content = base64::engine::general_purpose::STANDARD.encode(raw);
    let tags = probe_event(key, KIND_HANDSHAKE, now_unix).tags;
    let aux = rl_net::random_bytes::<32>().unwrap_or([0; 32]);
    Event::sign(key, now_unix, KIND_HANDSHAKE, tags, content, aux)
}

/// Whether a relay's refusal asks for payment or authentication: NIP-01's
/// `restricted:` and NIP-42's `auth-required:` prefixes, or a message
/// naming payment or a whitelist.
pub fn needs_payment_or_auth(message: &str) -> bool {
    let m = message.to_ascii_lowercase();
    m.starts_with("restricted") || m.starts_with("auth-required") || {
        const WORDS: [&str; 6] = ["pay", "paid", "payment", "invoice", "whitelist", "whitelisted"];
        m.split(|c: char| !c.is_ascii_alphanumeric()).any(|w| WORDS.contains(&w))
    }
}

/// The relay probe's deadlines and trust.
#[derive(Clone, Debug)]
pub struct ProbeConfig {
    /// Each step's deadline: connecting, an `OK`, an `EOSE`, the forward.
    pub step: Duration,
    /// Extra DER roots (tests); public relays need none.
    pub extra_roots: Vec<Vec<u8>>,
}

impl Default for ProbeConfig {
    fn default() -> ProbeConfig {
        ProbeConfig { step: Duration::from_secs(15), extra_roots: Vec::new() }
    }
}

/// What the probe saw of one relay.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RelayReport {
    pub url: String,
    /// Why the first connection failed; `None` once it connected.
    pub failure: Option<String>,
    /// From the start to the first connection.
    pub connect_ms: Option<u64>,
    /// The note's `OK` (accepted, message); `None` without an answer.
    pub note: Option<(bool, String)>,
    /// From publishing the note to its `OK`: the relay's round trip.
    pub ok_ms: Option<u64>,
    /// The note read back at once on the second connection.
    pub read_now: bool,
    /// The note read back after the wait on the first.
    pub read_later: bool,
    /// The kind-31471 note's `OK` (accepted, message).
    pub turn: Option<(bool, String)>,
    /// The kind-31471 note read back at once, and after the wait.
    pub turn_read_now: bool,
    pub turn_read_later: bool,
    /// The ephemeral event's `OK`.
    pub ephemeral: Option<(bool, String)>,
    /// The ephemeral event reached the second connection.
    pub forwarded: bool,
    /// Anything else of note (dropped connections, missing `EOSE`s).
    pub notes: Vec<String>,
}

impl RelayReport {
    pub fn connected(&self) -> bool {
        self.connect_ms.is_some()
    }

    /// The relay took the note.
    pub fn accepts(&self) -> bool {
        matches!(self.note, Some((true, _)))
    }

    /// The note was served at once and after the wait.
    pub fn keeps(&self) -> bool {
        self.read_now && self.read_later
    }

    /// The relay took the kind-31471 note.
    pub fn accepts_turn(&self) -> bool {
        matches!(self.turn, Some((true, _)))
    }

    /// The kind-31471 note was served at once and after the wait.
    pub fn keeps_turn(&self) -> bool {
        self.turn_read_now && self.turn_read_later
    }

    /// The (16 KiB) ephemeral event went from one connection to the other.
    pub fn forwards(&self) -> bool {
        self.forwarded
    }

    /// Connected, and refused nothing for payment or authentication.
    pub fn free(&self) -> bool {
        self.connected()
            && [&self.note, &self.turn, &self.ephemeral]
                .into_iter()
                .flatten()
                .all(|(accepted, m)| *accepted || !needs_payment_or_auth(m))
    }

    pub fn passes(&self) -> bool {
        self.accepts()
            && self.keeps()
            && self.accepts_turn()
            && self.keeps_turn()
            && self.forwards()
            && self.free()
    }
}

fn ms(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

/// One of the probe's two connections.
struct Conn {
    pool: Pool,
    name: &'static str,
}

impl Conn {
    fn new(url: &str, cfg: &ProbeConfig, name: &'static str) -> Conn {
        let pc = PoolConfig {
            relays: vec![url.to_owned()],
            extra_roots: cfg.extra_roots.clone(),
            ..PoolConfig::default()
        };
        Conn { pool: Pool::start(pc), name }
    }

    /// What arrived since the last call; lost connections go to `notes`.
    fn poll(&mut self, notes: &mut Vec<String>) -> Vec<PoolEvent> {
        let mut out = Vec::new();
        self.pool.poll(&mut out);
        for e in &out {
            if let PoolEvent::Status { status: RelayStatus::Dropped(m), .. } = e {
                notes.push(format!("{} connection dropped: {}", self.name, clean(m)));
            }
        }
        out
    }

    /// Waits for the connection; the first failure ends the wait.
    fn connect(&mut self, until: Instant, notes: &mut Vec<String>) -> Result<(), String> {
        while Instant::now() < until {
            for e in self.poll(notes) {
                match e {
                    PoolEvent::Status { status: RelayStatus::Connected, .. } => return Ok(()),
                    PoolEvent::Status { status: RelayStatus::Failed(m), .. } => return Err(clean(&m)),
                    _ => {}
                }
            }
            thread::sleep(POLL);
        }
        Err("no connection in time".to_owned())
    }

    /// The `OK` for event `id`.
    fn ok(&mut self, id: [u8; 32], until: Instant, notes: &mut Vec<String>) -> Option<(bool, String)> {
        while Instant::now() < until {
            for e in self.poll(notes) {
                if let PoolEvent::Ok { id: got, accepted, message, .. } = e
                    && got == id
                {
                    return Some((accepted, message));
                }
            }
            thread::sleep(POLL);
        }
        None
    }

    /// Subscribes `sub` and waits for its `EOSE`; whether event `id` came
    /// before it.
    fn read(
        &mut self,
        sub: &str,
        filter: Filter,
        id: [u8; 32],
        until: Instant,
        notes: &mut Vec<String>,
    ) -> bool {
        self.pool.subscribe(sub, vec![filter]);
        let mut found = false;
        let mut eose = false;
        while !eose && Instant::now() < until {
            for e in self.poll(notes) {
                match e {
                    PoolEvent::Event { event, .. } if event.id == id => found = true,
                    PoolEvent::Eose { sub: s, .. } if s == sub => eose = true,
                    _ => {}
                }
            }
            if !eose {
                thread::sleep(POLL);
            }
        }
        if !eose {
            notes.push(format!("no EOSE for `{sub}` on the {} connection", self.name));
        }
        self.pool.unsubscribe(sub);
        found
    }
}

/// Probes one relay (see the module docs) with `key`, at `now_unix`;
/// `wait` is called between the two reads of the note (the real probe
/// sleeps there).
pub fn probe_relay(
    url: &str,
    key: &Identity,
    now_unix: u64,
    cfg: &ProbeConfig,
    wait: &mut dyn FnMut(),
) -> RelayReport {
    let mut r = RelayReport { url: url.to_owned(), ..RelayReport::default() };
    let notes = &mut r.notes;
    let start = Instant::now();
    let mut a = Conn::new(url, cfg, "first");
    let mut b = Conn::new(url, cfg, "second");
    let until = start + cfg.step;
    match a.connect(until, notes) {
        Ok(()) => r.connect_ms = Some(ms(start.elapsed())),
        Err(e) => {
            r.failure = Some(e);
            return r;
        }
    }
    let b_up = b.connect(until, notes);
    if let Err(e) = &b_up {
        notes.push(format!("second connection: {e}"));
    }
    let filter = |kind| Filter {
        kinds: vec![kind],
        authors: vec![key.public()],
        tags: vec![('t', vec![PROBE_TAG.to_owned()])],
        ..Filter::default()
    };

    // The notes, and the immediate reads on the second connection.
    let note = probe_event(key, KIND_ANNOUNCE, now_unix);
    let sent = Instant::now();
    a.pool.publish(note.clone());
    r.note = a.ok(note.id, sent + cfg.step, notes);
    if r.note.is_some() {
        r.ok_ms = Some(ms(sent.elapsed()));
    }
    let turn = probe_event(key, KIND_TURN, now_unix);
    a.pool.publish(turn.clone());
    r.turn = a.ok(turn.id, Instant::now() + cfg.step, notes);
    if b_up.is_ok() {
        r.read_now = b.read("now", filter(KIND_ANNOUNCE), note.id, Instant::now() + cfg.step, notes);
        r.turn_read_now = b.read("now-turn", filter(KIND_TURN), turn.id, Instant::now() + cfg.step, notes);
    }

    // The ephemeral event, from the first connection to the second.
    if b_up.is_ok() {
        let live = "live";
        b.pool.subscribe(live, vec![filter(KIND_HANDSHAKE)]);
        let until = Instant::now() + cfg.step;
        // The subscription is live once the relay ends its stored events.
        let mut ready = false;
        while !ready && Instant::now() < until {
            ready = b.poll(notes).iter().any(|e| matches!(e, PoolEvent::Eose { sub, .. } if sub == live));
            if !ready {
                thread::sleep(POLL);
            }
        }
        if !ready {
            notes.push("no EOSE for `live` on the second connection".to_owned());
        }
        let eph = probe_handshake(key, now_unix);
        a.pool.publish(eph.clone());
        while Instant::now() < until {
            for e in a.poll(notes) {
                if let PoolEvent::Ok { id, accepted, message, .. } = e
                    && id == eph.id
                {
                    r.ephemeral = Some((accepted, message));
                }
            }
            for e in b.poll(notes) {
                if matches!(&e, PoolEvent::Event { event, .. } if event.id == eph.id) {
                    r.forwarded = true;
                }
            }
            let refused = matches!(r.ephemeral, Some((false, _)));
            if r.ephemeral.is_some() && (r.forwarded || refused) {
                break;
            }
            thread::sleep(POLL);
        }
    }
    drop(b);

    wait();
    r.read_later = a.read("later", filter(KIND_ANNOUNCE), note.id, Instant::now() + cfg.step, notes);
    r.turn_read_later = a.read("later-turn", filter(KIND_TURN), turn.id, Instant::now() + cfg.step, notes);
    r
}

/// Probes `urls` in parallel with one fresh key, `wait` between the reads;
/// the reports in `urls`' order.
pub fn probe_relays(urls: &[String], cfg: &ProbeConfig, wait: Duration) -> std::io::Result<Vec<RelayReport>> {
    let key = probe_key()?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    Ok(thread::scope(|s| {
        let handles: Vec<_> = urls
            .iter()
            .map(|url| {
                let key = &key;
                s.spawn(move || probe_relay(url, key, now, cfg, &mut || thread::sleep(wait)))
            })
            .collect();
        handles
            .into_iter()
            .zip(urls)
            .map(|(h, url)| {
                h.join().unwrap_or_else(|_| RelayReport {
                    url: url.clone(),
                    failure: Some("the probe panicked".to_owned()),
                    ..RelayReport::default()
                })
            })
            .collect()
    }))
}

/// Relay text for the table: one line, no `|`, at most 80 characters.
fn clean(m: &str) -> String {
    m.chars().map(|c| if c.is_control() || c == '|' { ' ' } else { c }).take(MESSAGE_CHARS).collect()
}

fn yes(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

fn opt_ms(v: Option<u64>) -> String {
    v.map_or_else(|| "-".to_owned(), |v| v.to_string())
}

/// The reports as a Markdown table.
pub fn relay_table(reports: &[RelayReport]) -> String {
    let mut out = String::from(
        "| relay | connect ms | OK ms | accepts 31470 | read now/later | accepts 31471 | 31471 now/later | forwards 16 KiB 21470 | free | verdict | notes |\n\
         |---|---|---|---|---|---|---|---|---|---|---|\n",
    );
    for r in reports {
        let mut notes: Vec<String> = Vec::new();
        if let Some(f) = &r.failure {
            notes.push(format!("connect: {f}"));
        }
        for (what, ok) in [("note", &r.note), ("TURN note", &r.turn), ("ephemeral", &r.ephemeral)] {
            match ok {
                Some((_, m)) if !m.is_empty() => notes.push(format!("{what} OK: {}", clean(m))),
                None if r.connected() => notes.push(format!("{what}: no OK")),
                _ => {}
            }
        }
        notes.extend(r.notes.iter().cloned());
        let cells = if r.connected() {
            [
                yes(r.accepts()).to_owned(),
                format!("{}/{}", yes(r.read_now), yes(r.read_later)),
                yes(r.accepts_turn()).to_owned(),
                format!("{}/{}", yes(r.turn_read_now), yes(r.turn_read_later)),
                yes(r.forwards()).to_owned(),
                yes(r.free()).to_owned(),
            ]
        } else {
            std::array::from_fn(|_| "-".to_owned())
        };
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            r.url,
            opt_ms(r.connect_ms),
            opt_ms(r.ok_ms),
            cells[0],
            cells[1],
            cells[2],
            cells[3],
            cells[4],
            cells[5],
            if r.passes() { "pass" } else { "FAIL" },
            notes.join("; "),
        ));
    }
    out
}

/// What the probe saw of one STUN server. The mapped address itself (the
/// prober's public IP) is not kept: only its family and whether it is
/// global.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StunReport {
    pub server: String,
    /// The IPv4 address asked.
    pub resolved: Option<SocketAddr>,
    /// From the request that was answered to its answer.
    pub rtt_ms: Option<u64>,
    /// `"IPv4"` or `"IPv6"`: the mapped address's family.
    pub family: Option<&'static str>,
    /// The mapped address is global (`list::is_global`).
    pub global: Option<bool>,
    /// Datagrams that were no answer to us (another sender, another
    /// transaction, not STUN).
    pub stray: u32,
    pub error: Option<String>,
}

impl StunReport {
    pub fn answered(&self) -> bool {
        self.rtt_ms.is_some()
    }
}

/// Asks `server` (`host:port`, IPv4, as `stun::public_ip` does) for a
/// binding, up to 3 tries of `timeout` each.
pub fn probe_stun(server: &str, timeout: Duration) -> StunReport {
    let mut r = StunReport { server: server.to_owned(), ..StunReport::default() };
    let target = match server.to_socket_addrs() {
        Ok(mut addrs) => addrs.find(SocketAddr::is_ipv4),
        Err(e) => {
            r.error = Some(format!("resolve: {e}"));
            return r;
        }
    };
    let Some(target) = target else {
        r.error = Some("no IPv4 address".to_owned());
        return r;
    };
    r.resolved = Some(target);
    let socket = match UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)) {
        Ok(s) => s,
        Err(e) => {
            r.error = Some(format!("bind: {e}"));
            return r;
        }
    };
    let mut buf = [0u8; 1500];
    for _ in 0..3 {
        let Ok(txn) = rl_net::random_bytes::<12>() else {
            r.error = Some("no randomness".to_owned());
            return r;
        };
        let sent = Instant::now();
        if let Err(e) = socket.send_to(&binding_request(txn), target) {
            r.error = Some(format!("send: {e}"));
            return r;
        }
        let until = sent + timeout;
        loop {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() || socket.set_read_timeout(Some(left)).is_err() {
                break;
            }
            match socket.recv_from(&mut buf) {
                Ok((len, from)) => match parse_binding_response(&buf[..len], &txn) {
                    Some(mapped) if from == target => {
                        r.rtt_ms = Some(ms(sent.elapsed()));
                        r.family = Some(if mapped.is_ipv4() { "IPv4" } else { "IPv6" });
                        r.global = Some(is_global(mapped.ip()));
                        return r;
                    }
                    _ => r.stray += 1,
                },
                Err(_) => break,
            }
        }
    }
    r.error = Some("no answer".to_owned());
    r
}

/// The reports as a Markdown table.
pub fn stun_table(reports: &[StunReport]) -> String {
    let mut out = String::from(
        "| server | address | RTT ms | mapped family | global | verdict | notes |\n|---|---|---|---|---|---|---|\n",
    );
    for r in reports {
        let mut notes: Vec<String> = r.error.iter().cloned().collect();
        if r.stray > 0 {
            notes.push(format!("{} stray datagrams", r.stray));
        }
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} |\n",
            r.server,
            r.resolved.map_or_else(|| "-".to_owned(), |a| a.to_string()),
            opt_ms(r.rtt_ms),
            r.family.unwrap_or("-"),
            r.global.map_or("-", yes),
            if r.answered() { "pass" } else { "FAIL" },
            notes.join("; "),
        ));
    }
    out
}

/// `rl-cli nostr-probe`: measures Nostr relays for the default list. It
/// publishes probe events (a fresh random key, 10-minute expiry) to the
/// relays named: run it by hand only.
#[derive(clap::Args)]
pub struct RelayArgs {
    /// Relay URLs (`wss://…`); the default list when none are given.
    pub urls: Vec<String>,
    /// Minutes between publishing the note and reading it back again.
    #[arg(long, default_value_t = 3)]
    pub wait_min: u64,
}

/// Prints the table; `Ok(false)` unless every relay passes.
pub fn run_relays(args: &RelayArgs) -> anyhow::Result<bool> {
    let urls: Vec<String> = if args.urls.is_empty() {
        DEFAULT_RELAYS.iter().map(|s| (*s).to_owned()).collect()
    } else {
        args.urls.clone()
    };
    eprintln!(
        "probing {} relays (two connections, two notes and one 16 KiB ephemeral event each; reading back after {} min)",
        urls.len(),
        args.wait_min
    );
    let wait = Duration::from_secs(args.wait_min.saturating_mul(60));
    let reports = probe_relays(&urls, &ProbeConfig::default(), wait)?;
    print!("{}", relay_table(&reports));
    Ok(reports.iter().all(RelayReport::passes))
}

/// `rl-cli stun-probe`: measures STUN servers for the default list.
#[derive(clap::Args)]
pub struct StunArgs {
    /// `host:port` servers; the default list when none are given.
    pub servers: Vec<String>,
}

/// Prints the table; `Ok(false)` unless every server answers.
pub fn run_stun(args: &StunArgs) -> anyhow::Result<bool> {
    let servers: Vec<String> = if args.servers.is_empty() {
        DEFAULT_STUN.iter().map(|s| (*s).to_owned()).collect()
    } else {
        args.servers.clone()
    };
    let reports: Vec<StunReport> = thread::scope(|s| {
        let handles: Vec<_> =
            servers.iter().map(|srv| s.spawn(move || probe_stun(srv, Duration::from_secs(1)))).collect();
        handles
            .into_iter()
            .zip(&servers)
            .map(|(h, srv)| {
                h.join().unwrap_or_else(|_| StunReport {
                    server: srv.clone(),
                    error: Some("the probe panicked".to_owned()),
                    ..StunReport::default()
                })
            })
            .collect()
    });
    print!("{}", stun_table(&reports));
    Ok(reports.iter().all(StunReport::answered))
}
