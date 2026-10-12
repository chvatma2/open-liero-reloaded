# Plan 10: Online Play (Direct and LAN) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Play rust-liero online for real: host from the menus or a headless dedicated server, join by address or from the LAN list, gather in a lobby with chat and match settings, play with rollback netcode over WebRTC, spectate, kick and ban, and return to the lobby after each match.

**Architecture:** Plan 9's sans-IO netcode gains real transport (`rl-net`: WebRTC data channels through the sans-IO `str0m` stack, WebSocket signalling, LAN broadcast), a sans-IO lobby (`rl-lobby`), and an orchestration crate (`rl-online`) that turns a lobby's match setup into worlds, drives bots on the host, runs a `rl_netcode::Session` per match and produces the replay. The game (`rl-game`, `rl-ui`) presents online matches correctly under rollback (spec §3.4), and `rl-server` is the headless dedicated server. Everything that decides behaviour stays testable in one process; real sockets are tested on localhost.

**Tech Stack:** Rust 1.99 (edition 2024); `str0m` `=0.24.0` (`default-features = false, features = ["rust-crypto"]`: pure Rust, no C toolchain); `tungstenite` `=0.30.0` (no TLS); `k256` `=0.14.0` (`schnorr` feature: the Nostr key type, spec §5.2); `getrandom` `=0.4.3`; existing workspace crates (serde, postcard, ron, clap, egui 0.36.2, egui_kittest).

**Spec:** `docs/superpowers/specs/2026-10-12-networking-design.md` (approved; plan 9 implemented). This plan covers spec §11's plan 10 row: §2.1 (the transport; the spike chooses `str0m`), §3.4 (presentation during rollback), §3.6 (replays), §4 (match flow), §5.1 direct and LAN rows, §5.2 (identity), §5.4 (security), §6.2 (LAN), §7.1-§7.4 (menus, lobby, in-match, dedicated server without `--relay`), §9.4 (real transport), §9.9 (menus and lobby), §9.10 (`--net-sim`, overlay). Not here: content download (plan 11; a joiner who lacks the content sees what is missing and cannot be ready), Nostr, invite codes, NAT traversal, relays and host migration (plan 12), the browser (plan 13).

## Global Constraints

- The simulation's behaviour must not change: oracle traces, match goldens, render goldens stay byte-identical; `World::checksum` unchanged.
- Layering (`xtask/layers.ron`): `rl-lobby` layer 4 (`no_std` + `alloc`, `wasm`); `rl-net` layer 5 (std; not wasm); `rl-online` layer 5 (std; not wasm); `rl-server` layer 6. `same_layer` pairs: `("rl-lobby", "rl-proto")`, `("rl-online", "rl-net")`; anything else needs a ruling.
- Never panic on network input (spec §5.4): every new decoder is fuzzed with proptest (`cases: 256, failure_persistence: None`); every message type has a size limit and every peer a rate limit; a peer breaking either is dropped.
- Determinism and testability: the lobby and orchestration logic take time as an argument; real clocks and sockets live only in `rl-net`'s socket driver and in the binaries.
- Ports (spec §5.1): the host listens on TCP and UDP port `23400` (configurable); LAN announcements on UDP `23401` every 2 s.
- Spec §3.4: sounds and effects play at once, keyed by tick and source so a re-simulated event never plays twice; an event a correction erases is not undone; the scoreboard, kill and death messages, lives and the match end use confirmed ticks only; rendering draws the newest predicted tick.
- Spec §5.2: each install has a key pair (secp256k1, BIP-340 Schnorr: the Nostr key type); the lobby shows a short fingerprint beside each name; joining proves possession of the key; kick and ban by key (optionally IP).
- Spec §7.2: a lobby holds up to 32 players plus up to 16 spectators; chat lines at most 200 characters.
- Dependencies: only those listed in Tech Stack, pinned exactly (`=x.y.z`, released at least two weeks before use); `str0m` moves to `=0.24.1` only after 2026-10-17 (it fixes SCTP buffering for MB-sized transfers) — Task 1 records whether the 3 MB transfer works on `0.24.0`.
- Every new crate has a `CLAUDE.md`; `cargo xtask check` before every commit; commit on `main`, never push. Commit trailers:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_019821i4KFSbhZ4mHYULrhEV
  ```
- Plan 9's deferred items that touch this plan's code are done in the task that touches that code (Task 7 lists the netcode ones); the rest are recorded as carried in the ledger.

## Decisions this plan makes (recorded as STATUS.md N-rulings in Task 16)

- **N15 – str0m.** The native WebRTC stack is `str0m` (sans-IO, clock-injected, pure Rust with `rust-crypto`), unless Task 1's spike fails a stated criterion; then `rtc` 0.21 is the fallback (a ruling).
- **N16 – host candidates from the signalling connection.** In plan 10 a peer's ICE candidates are its UDP socket's port on the IP address its signalling TCP connection uses (both directions), no STUN yet (plan 12). LAN and direct play (including a forwarded port) need nothing more.
- **N17 – negotiated channels.** Both data channels are pre-negotiated (`inputs` id 0 unreliable unordered, `control` id 1 reliable ordered), so neither side waits for in-band channel setup and browsers (plan 13) use the same ids.
- **N18 – one session per match.** A `rl_netcode::Session` lives for one match: the input log's growth is bounded by the match (plan 9's deferred InputLog item), and the replay is produced after the match on a worker thread by re-simulating the confirmed inputs (the recorder needs the world at each recorded tick, which a rollback session only has for confirmed ticks).
- **N19 – weapons in the lobby.** Each player picks a loadout in the lobby (the existing weapon screen as a lobby sub-page) or keeps "Random"; the host's match setup carries every slot's choice, so every peer builds the identical world.
- **N20 – presentation repair after the advance.** After each `advance`, the game rebuilds the colour image's restored areas from the final terrain, then applies the advance's terrain edits in tick order (plan 9's documented protocol; inexact only inside a corrected area's art, ruling F-B).
- **N21 – keep running when hidden.** While an online lobby or match exists, the game keeps polling the network and ticking when its window is minimised or hidden (no rendering).

## Review Focus

1. **A stranger joining a public host** (spec §5.4): oversized or malformed signalling and lobby messages, floods of chat or joins, a wrong fingerprint, a forged identity (signature over someone else's challenge): refused, counted, the peer dropped, nothing panics. Pinned by Task 2's fuzz proptests, Task 4's `floods_and_oversized_messages_drop_the_peer`, Task 6's `a_forged_signature_is_refused`.
2. **Disconnects at every stage** (signalling, lobby, match start, mid-match, results): the other players carry on, the leaver's slot follows decision N6/`Bye`, and nobody hangs on a dead connection. Pinned by Task 6's `leaving_at_every_stage` and Task 8's `a_client_vanishes_mid_match`.
3. **Two sounds for one event** (rollback re-simulation): never; and the colour image never drifts from the terrain after rollbacks. Pinned by Task 9's `rollbacks_play_each_sound_once_and_keep_the_terrain`.
4. **The local player is not worm 0** (online slot k): HUD, camera, crosshair, own health bar, scoreboard highlight, kill credit and weapon choice follow slot k. Pinned by Task 10's `the_local_slot_drives_everything`.
5. **A minimised window mid-match** (spec §3.2's timeouts): the game keeps ticking and nobody is dropped. Pinned by Task 12's `a_hidden_window_keeps_the_match_alive`.

---

## File structure

| File | Responsibility |
|---|---|
| `crates/rl-net/` (new) | `link.rs` (one WebRTC peer over str0m, sans-IO), `endpoint.rs` (UDP socket + many links + signalling), `signal.rs` (WebSocket signalling), `lan.rs` (LAN announcements), `impair.rs` (network impairment on real sockets), `limits.rs` (per-peer rate and size limits). |
| `crates/rl-proto/src/{signal.rs, lobby.rs, lan.rs}` | Wire messages of signalling, the lobby and LAN discovery. |
| `crates/rl-lobby/` (new) | `identity.rs` (keys, signatures, fingerprints), `host.rs`, `client.rs`, `model.rs` (the lobby's shared state), `setup.rs` (the match setup wire form). |
| `crates/rl-online/` (new) | `matches.rs` (setup → worlds, host bots), `peer.rs` (a host or client: endpoint + lobby + match), `replay.rs` (replay after the match), `events.rs` (what the game and server see). |
| `crates/rl-netcode/src/{session.rs, sync.rs, harness.rs}` | Plan 9's deferred items (Task 8). |
| `crates/rl-present/src/net.rs` (new), `crates/rl-scene`, `crates/rl-audio` | Presentation under rollback (Task 9). |
| `crates/rl-game/src/{session.rs, online.rs, hud.rs, overlay.rs, shell.rs, app.rs, main.rs}` | Local slot, online session, overlay, chat, flags (Tasks 10, 12). |
| `crates/rl-ui/src/screens/{multiplayer.rs, host.rs, lobby.rs}`, `src/online.rs` | Menus and lobby (Task 11). |
| `crates/rl-server/` (new) | The dedicated server binary (Task 13). |
| `crates/rl-cli/src/{net_client_cmd.rs, transport_test.rs}` | Headless bot clients and real-transport tests (Task 14). |
| `fixtures/autoplay/online.ron`, `xtask/src/e2e.rs` | The online end-to-end run (Task 15). |

---
### Task 1: Spike and `rl-net::link`: one WebRTC peer over str0m

**Files:**
- Create: `crates/rl-net/{Cargo.toml, CLAUDE.md, src/lib.rs, src/link.rs}`, `crates/rl-net/tests/link.rs`
- Modify: `Cargo.toml` (workspace deps `str0m = { version = "=0.24.0", default-features = false, features = ["rust-crypto"] }`, `rl-net = { path = "crates/rl-net" }`), `xtask/layers.ron` (`"rl-net": 5`)
- Test: `tests/link.rs` (two links wired in memory on a virtual clock; two links over real localhost UDP sockets)

**Interfaces:**
- Produces (Tasks 4, 14):
  ```rust
  pub const INPUTS: u16 = 0;      // negotiated channel ids (decision N17)
  pub const CONTROL: u16 = 1;
  #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum Side { Offerer, Answerer }
  pub struct Link { .. }          // one peer connection, sans-IO
  #[derive(Debug)] pub enum LinkEvent { Open, Message { channel: rl_netcode::Channel, bytes: Vec<u8> }, Closed { why: String } }
  pub struct Transmit { pub from: SocketAddr, pub to: SocketAddr, pub bytes: Vec<u8> }
  impl Link {
      pub fn offerer(local: &[SocketAddr], now: Instant) -> (Link, String /* SDP offer */);
      pub fn answerer(local: &[SocketAddr], offer: &str, now: Instant) -> Result<(Link, String /* SDP answer */), LinkError>;
      pub fn accept_answer(&mut self, answer: &str) -> Result<(), LinkError>;
      pub fn accepts(&self, from: SocketAddr, to: SocketAddr, bytes: &[u8]) -> bool;   // demux on a shared socket
      pub fn receive(&mut self, from: SocketAddr, to: SocketAddr, bytes: &[u8], now: Instant);
      pub fn send(&mut self, channel: rl_netcode::Channel, bytes: &[u8]) -> Result<(), LinkError>;
      pub fn buffered(&self, channel: rl_netcode::Channel) -> usize;   // bytes queued (backpressure for big transfers)
      pub fn poll(&mut self, now: Instant, out: &mut Vec<Transmit>, events: &mut Vec<LinkEvent>) -> Instant; // next timeout
      pub fn close(&mut self);
      pub fn is_open(&self) -> bool;
  }
  pub enum LinkError { Sdp(String), NotOpen, TooLarge, Closed }
  ```

Context: spec §2.1 and decision N15. `str0m` is a sans-IO WebRTC stack: the caller feeds datagrams and time and drains `poll_output` (transmits, events, timeouts) after every input — its contract: mutate, then drain until it returns a timeout, before the next mutation. `Link` hides str0m behind the game's two channels. Time in `Link` is `std::time::Instant` (str0m's type); the netcode keeps `now_us` (the endpoint converts).

**The spike's pass criteria** (Step 4 records each with numbers; if one fails, stop with BLOCKED and the evidence — the controller rules on the fallback `rtc` 0.21):
1. Two links connect over real localhost UDP in under 1 s.
2. Unreliable channel: 10 000 datagrams of 64 bytes at 1 ms intervals arrive (loopback: no loss expected) unordered-capable (`ordered: false`, `max_retransmits: 0`).
3. Reliable channel: a 3 MB payload sent as 32 KB messages with `buffered` backpressure (keep under 256 KB queued) arrives complete and in order, within 5 s on loopback.
4. In memory on a virtual clock (no sockets; the test passes each `Transmit` to the other link's `receive` with a simulated 20 ms delay): connect, exchange both channels, and the whole test runs in under 1 s of wall time while covering at least 30 s of virtual time (proves no hidden wall-clock use).
5. `cargo build --release --target x86_64-pc-windows-gnu -p rl-net` succeeds (pure Rust; the mingw cross toolchain exists on this machine).

- [ ] **Step 1: Scaffold** `rl-net` (std crate, `[lints] workspace = true`), deps `str0m`, `rl-netcode` (for `Channel`), dev-deps none extra. `lib.rs` docs: "Real transport for online play (networking spec §2.1, §5.1): WebRTC data channels over the sans-IO str0m stack, signalling over a WebSocket, LAN announcements. Layer 5, std."

- [ ] **Step 2: Write the failing tests** (`tests/link.rs`):

```rust
//! One WebRTC peer connection (spec §2.1): both channels, in memory on a
//! virtual clock and over real localhost sockets.

use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

use rl_net::link::{Link, LinkEvent, Transmit};
use rl_netcode::Channel;

/// Two links over an in-memory wire with a fixed one-way delay, on a virtual clock.
struct Wire {
    now: Instant,
    a: Link,
    b: Link,
    in_flight: Vec<(Instant, bool /* to b */, Transmit)>,
    got_a: Vec<LinkEvent>,
    got_b: Vec<LinkEvent>,
}

const A: &str = "10.0.0.1:5000";
const B: &str = "10.0.0.2:6000";

fn pair(delay: Duration) -> Wire {
    let now = Instant::now();
    let (a, offer) = Link::offerer(&[A.parse().unwrap()], now);
    let (b, answer) = Link::answerer(&[B.parse().unwrap()], &offer, now).unwrap();
    let mut w = Wire { now, a, b, in_flight: vec![], got_a: vec![], got_b: vec![] };
    w.a.accept_answer(&answer).unwrap();
    let _ = delay;
    w
}

impl Wire {
    fn run(&mut self, until: Instant, delay: Duration) {
        while self.now < until {
            let mut out = Vec::new();
            let ta = self.a.poll(self.now, &mut out, &mut self.got_a);
            for t in out.drain(..) { self.in_flight.push((self.now + delay, true, t)); }
            let tb = self.b.poll(self.now, &mut out, &mut self.got_b);
            for t in out.drain(..) { self.in_flight.push((self.now + delay, false, t)); }
            let next_wire = self.in_flight.iter().map(|f| f.0).min();
            let next = [Some(ta), Some(tb), next_wire, Some(until)].into_iter().flatten().min().unwrap();
            self.now = next.max(self.now + Duration::from_micros(100));
            let (due, rest): (Vec<_>, Vec<_>) = self.in_flight.drain(..).partition(|f| f.0 <= self.now);
            self.in_flight = rest;
            for (_, to_b, t) in due {
                if to_b { self.b.receive(t.from, t.to, &t.bytes, self.now) } else { self.a.receive(t.from, t.to, &t.bytes, self.now) }
            }
        }
    }
}

#[test]
fn links_connect_and_carry_both_channels_on_a_virtual_clock() {
    let wall = Instant::now();
    let delay = Duration::from_millis(20);
    let mut w = pair(delay);
    let t0 = w.now;
    w.run(t0 + Duration::from_secs(2), delay);
    assert!(w.a.is_open() && w.b.is_open(), "connected within 2 s of virtual time");
    for i in 0..1000u32 {
        w.a.send(Channel::Unreliable, &i.to_le_bytes()).unwrap();
        w.a.send(Channel::Reliable, &[i as u8; 1000]).unwrap();
        let t = w.now + Duration::from_millis(10);
        w.run(t, delay);
    }
    let t = w.now + Duration::from_secs(20);
    w.run(t, delay);
    let unreliable = w.got_b.iter().filter(|e| matches!(e, LinkEvent::Message { channel: Channel::Unreliable, .. })).count();
    let reliable: Vec<_> = w.got_b.iter().filter_map(|e| match e { LinkEvent::Message { channel: Channel::Reliable, bytes } => Some(bytes[0]), _ => None }).collect();
    assert_eq!(unreliable, 1000);
    assert_eq!(reliable, (0..1000u32).map(|i| i as u8).collect::<Vec<_>>());
    assert!(wall.elapsed() < Duration::from_secs(2), "virtual time, not wall time");
}

fn sockets() -> (UdpSocket, UdpSocket) {
    let a = UdpSocket::bind("127.0.0.1:0").unwrap();
    let b = UdpSocket::bind("127.0.0.1:0").unwrap();
    a.set_nonblocking(true).unwrap();
    b.set_nonblocking(true).unwrap();
    (a, b)
}

fn pump(sock: &UdpSocket, link: &mut Link, events: &mut Vec<LinkEvent>) {
    let local = sock.local_addr().unwrap();
    let mut buf = [0u8; 2048];
    while let Ok((n, from)) = sock.recv_from(&mut buf) {
        link.receive(from, local, &buf[..n], Instant::now());
    }
    let mut out = Vec::new();
    link.poll(Instant::now(), &mut out, events);
    for t in out { let _ = sock.send_to(&t.bytes, t.to); }
}

#[test]
fn links_carry_three_megabytes_over_real_sockets() {
    let (sa, sb) = sockets();
    let (mut a, offer) = Link::offerer(&[sa.local_addr().unwrap()], Instant::now());
    let (mut b, answer) = Link::answerer(&[sb.local_addr().unwrap()], &offer, Instant::now()).unwrap();
    a.accept_answer(&answer).unwrap();
    let (mut ea, mut eb) = (Vec::new(), Vec::new());
    let start = Instant::now();
    while !(a.is_open() && b.is_open()) {
        pump(&sa, &mut a, &mut ea);
        pump(&sb, &mut b, &mut eb);
        assert!(start.elapsed() < Duration::from_secs(1), "connects within 1 s");
        std::thread::sleep(Duration::from_millis(1));
    }
    let payload: Vec<u8> = (0..3_000_000u32).map(|i| (i * 7 % 251) as u8).collect();
    let mut sent = 0;
    let mut got = Vec::new();
    let start = Instant::now();
    while got.len() < payload.len() {
        while sent < payload.len() && a.buffered(Channel::Reliable) < 256 * 1024 {
            let end = (sent + 32 * 1024).min(payload.len());
            a.send(Channel::Reliable, &payload[sent..end]).unwrap();
            sent = end;
        }
        pump(&sa, &mut a, &mut ea);
        pump(&sb, &mut b, &mut eb);
        for e in eb.drain(..) {
            if let LinkEvent::Message { channel: Channel::Reliable, bytes } = e { got.extend(bytes); }
        }
        assert!(start.elapsed() < Duration::from_secs(5), "3 MB within 5 s");
    }
    assert_eq!(got, payload);
}
```

(The pair helper's unused `delay` parameter is a reminder that the wire delay is applied in `run`; drop it if clippy objects.)

- [ ] **Step 3: Run the tests to verify they fail** — `cargo nextest run -p rl-net` → compile errors.

- [ ] **Step 4: Implement `link.rs`** on str0m (read `cargo xtask api` → `target/api/str0m.txt` is not generated for external crates: read str0m's docs on docs.rs for 0.24.0 or its source under `~/.cargo/registry`): `RtcConfig::new()` with `rust-crypto`; local host candidates `Candidate::host(addr, "udp")`; the offerer uses the SDP API (`sdp_api()`) to add two negotiated data channels (`ChannelConfig { label: "inputs", ordered: false, reliability: Reliability::MaxRetransmits { retransmits: 0 }, negotiated: Some(0), .. }` and `"control"` reliable ordered `negotiated: Some(1)`) and create the offer; the answerer accepts the offer (negotiated channels need the same config on both sides: add them on the answerer too); map str0m `Output::Transmit` to `Transmit`, `Event::ChannelOpen/ChannelData/ChannelClose/IceConnectionStateChange(Disconnected)` to `LinkEvent`; `send` writes to the channel (`TooLarge` above 64 KB); `buffered` from the channel's buffered amount. Record the five criteria with measured numbers in the report (and the str0m APIs used).

- [ ] **Step 5: Run** `cargo nextest run -p rl-net && cargo build --release --target x86_64-pc-windows-gnu -p rl-net` → pass. Then `cargo xtask check`.

- [ ] **Step 6: Commit** — CLAUDE.md (purpose, layer 5 std, str0m pinned exactly with `rust-crypto` and why, the channel ids, the mutate-then-drain contract), then:

```bash
git add Cargo.toml Cargo.lock xtask/layers.ron crates/rl-net
git commit -m "rl-net: one WebRTC peer connection over str0m, both channels"
```

---

### Task 2: rl-proto: signalling, lobby and LAN messages

**Files:**
- Create: `crates/rl-proto/src/{signal.rs, lobby.rs, lan.rs}`, `crates/rl-proto/tests/fuzz_lobby.rs`
- Modify: `crates/rl-proto/src/lib.rs` (modules, re-exports), `crates/rl-proto/CLAUDE.md`
- Test: round trips and limits inline; `tests/fuzz_lobby.rs`

**Interfaces:**
- Produces (Tasks 4-7, 11, 13):
  ```rust
  // signal.rs — over the host's WebSocket, one postcard message per binary frame
  pub const MAX_SIGNAL: usize = 16 * 1024;
  pub enum Signal {
      Offer { protocol: u16, fingerprint: u64, sdp: String },   // client → host
      Answer { sdp: String },                                     // host → client
      Refused { reason: Refusal },                                // host → client, then close
  }
  pub enum Refusal { Version, Full, Banned, Busy, Malformed }
  pub fn encode_signal(s: &Signal) -> Vec<u8>;
  pub fn decode_signal(b: &[u8]) -> Result<Signal, ProtoError>;   // sdp ≤ 8 KiB, UTF-8 checked by serde

  // lobby.rs — on the reliable channel, before, between and during matches
  pub const MAX_LOBBY: usize = 32 * 1024;
  pub const MAX_NAME: usize = 20;      // characters (rl-ui's profile limit)
  pub const MAX_CHAT: usize = 200;     // characters
  pub const MAX_PLAYERS: usize = 32;
  pub const MAX_SPECTATORS: usize = 16;
  pub type PublicKey = [u8; 32];       // x-only secp256k1 (BIP-340)
  pub type Signature = [u8; 64];
  pub struct Look { pub name: String, pub colour: [u8; 3], pub skin: String /* skin pack id */ }
  pub enum ToHost {
      Hello { key: PublicKey, look: Look, password: Option<[u8; 32]> /* SHA-256 of host nonce + password */ },
      Prove { signature: Signature },                     // over the host's challenge
      SetLook(Look), Ready(bool), Spectate(bool), Loadout(Option<[i32; 5]>),
      Chat(String),
      Have { missing: Vec<String> },                      // pack ids this client lacks (plan 11 downloads them)
      // host-only commands from an admin client are ToHost::Admin (Task 13)
      Admin(AdminCommand),
      Bye,
  }
  pub enum AdminCommand { Kick(PublicKey), Ban(PublicKey), Next, Settings(MatchSetupWire) }
  pub enum ToClient {
      Challenge { nonce: [u8; 32], host_key: PublicKey, needs_password: bool },
      Welcome { you: u8 /* member id */, lobby: LobbyState },
      Refused(Refusal),
      Lobby(LobbyState),                                   // full state on every change (small)
      Chat { from: u8, text: String },
      Notice(Notice),                                      // joins, leaves, kicks, "host started", ...
      Start(MatchStartWire),
      Ended(MatchEndWire),
      Kicked { banned: bool },
      Bye,
  }
  pub struct LobbyState { pub name: String, pub phase: Phase, pub setup: MatchSetupWire, pub members: Vec<Member> }
  pub enum Phase { Gathering, Starting, Playing { tick: u32 }, Results }
  pub struct Member { pub id: u8, pub key: PublicKey, pub look: Look, pub seat: SeatWire, pub ready: bool, pub missing: Vec<String>, pub ping_ms: Option<u16>, pub admin: bool }
  pub enum SeatWire { Player { slot: u8 }, Spectator, Waiting }
  pub struct MatchSetupWire { pub mod_id: String, pub level_id: String, pub packs: Vec<(String, String)> /* id, content hash */,
                              pub rules: RulesWire, pub bonuses: bool, pub loading_time: i32, pub restrictions: String,
                              pub bots: Vec<BotWire> }
  pub struct RulesWire { pub lives: i32, pub kill_limit: i32, pub time_limit_min: f32, pub allow_negative_score: bool,
                         pub death_decreases_score: f32, pub suicide_decreases_score: bool, pub sprees: [i32; 5], pub dying_sprees: [i32; 5] }
  pub struct BotWire { pub difficulty: u8, pub personality: u8, pub loadout: u8, pub look: Look }
  pub struct MatchStartWire { pub seed: u32, pub setup: MatchSetupWire, pub slots: Vec<SlotWire>, pub input_delay: Vec<(u8 /* member */, u8)>, pub start_checksum: u64 }
  pub struct SlotWire { pub member: Option<u8> /* None: a bot */, pub bot: Option<u8> /* index into setup.bots */, pub look: Look, pub loadout: Option<[i32; 5]> }
  pub struct MatchEndWire { pub reason: String, pub rows: Vec<(u8 /* slot */, i32, i32, i32, i32) /* score, kills, deaths, lives */> }
  pub enum Notice { Joined(u8), Left(u8), Kicked(u8), Banned(u8), NotReady(Vec<u8>), Started, Message(String) }
  pub fn encode_to_host(m: &ToHost) -> Vec<u8>;   pub fn decode_to_host(b: &[u8]) -> Result<ToHost, ProtoError>;
  pub fn encode_to_client(m: &ToClient) -> Vec<u8>; pub fn decode_to_client(b: &[u8]) -> Result<ToClient, ProtoError>;

  // lan.rs
  pub const LAN_PORT: u16 = 23401;
  pub const MAX_LAN: usize = 1024;
  pub struct LanAnnounce { pub fingerprint: u64, pub name: String, pub port: u16, pub players: u8, pub max_players: u8,
                           pub spectators: u8, pub mod_id: String, pub level_id: String, pub password: bool, pub playing: bool }
  pub fn encode_lan(a: &LanAnnounce) -> Vec<u8>;  pub fn decode_lan(b: &[u8]) -> Result<LanAnnounce, ProtoError>;
  ```
  The lobby and LAN messages share the same framing as the netcode's control channel (plan 9): the reliable channel carries a one-byte tag first — `0` netcode `Control`, `1` `ToHost`, `2` `ToClient` — so a peer demultiplexes without guessing. Add `pub fn tag(bytes) -> Option<Tagged<'_>>` with `Tagged::{Netcode(&[u8]), ToHost(&[u8]), ToClient(&[u8])}`.

Context: spec §5.1, §5.2, §5.4, §6.2, §7.2. All encodings are postcard (like plan 9's messages), strict about trailing bytes, with every `String`/`Vec` bounded after decoding (names ≤ `MAX_NAME` characters and no control characters, chat ≤ `MAX_CHAT` characters, members ≤ `MAX_PLAYERS + MAX_SPECTATORS`, slots ≤ 32, packs ≤ 8, bots ≤ 31, `missing` ≤ 8 ids of ≤ 64 bytes, pack ids ASCII `[a-z0-9-]`, hashes 64 hex characters); a violation is `ProtoError::Invalid`.

- [ ] **Step 1: Write the failing tests**: per module, a round trip of a full message, each bound refused (one test per bound: a 21-character name, a 201-character chat line, a name with `\n`, 49 members, a 33-slot start, a pack id with `/`), trailing bytes refused, and the tag demultiplexer. `tests/fuzz_lobby.rs`: proptests (arbitrary bytes and mutated valid encodings) for `decode_signal`, `decode_to_host`, `decode_to_client`, `decode_lan`, `tag` — never panic.

- [ ] **Step 2: Run** `cargo nextest run -p rl-proto` → fail (compile).

- [ ] **Step 3: Implement**; the fingerprint (plan 9) already hashes `rl-proto/src`, so new messages change it automatically.

- [ ] **Step 4: Run** `cargo nextest run -p rl-proto && cargo build --target wasm32-unknown-unknown -p rl-proto` → pass; `cargo xtask check`.

- [ ] **Step 5: Commit** — CLAUDE.md (the three message families, the tag byte, the bounds), then `git commit -m "rl-proto: signalling, lobby and LAN messages with their bounds"`.

---

### Task 3: rl-lobby identity: keys, signatures, fingerprints

**Files:**
- Create: `crates/rl-lobby/{Cargo.toml, CLAUDE.md, src/lib.rs, src/identity.rs}`
- Modify: `Cargo.toml` (`k256 = { version = "=0.14.0", default-features = false, features = ["schnorr"] }`, `rl-lobby` path dep), `xtask/layers.ron` (`"rl-lobby": 4`, `no_std`, `wasm`, same_layer `("rl-lobby", "rl-proto")`), `crates/rl-ui/src/store.rs` (identity file), `crates/rl-ui/CLAUDE.md`
- Test: inline in `identity.rs`; `crates/rl-ui/tests/store.rs` (identity file)

**Interfaces:**
- Produces:
  ```rust
  pub struct Identity { /* secret key */ }
  impl Identity {
      pub fn from_secret(secret: [u8; 32]) -> Option<Identity>;      // None for an invalid scalar
      pub fn secret(&self) -> [u8; 32];
      pub fn public(&self) -> rl_proto::PublicKey;                    // x-only, 32 bytes
      pub fn sign(&self, message: &[u8], aux: [u8; 32]) -> rl_proto::Signature;   // BIP-340 with caller randomness
  }
  pub fn verify(key: &rl_proto::PublicKey, message: &[u8], sig: &rl_proto::Signature) -> bool;
  pub fn fingerprint(key: &rl_proto::PublicKey) -> String;            // 8 characters: base32 (lowercase, no padding) of SHA-256(key)[..5]
  pub fn challenge_message(nonce: &[u8; 32], host: &rl_proto::PublicKey) -> Vec<u8>;  // b"rust-liero join v1" ++ nonce ++ host
  pub fn password_proof(nonce: &[u8; 32], password: &str) -> [u8; 32];             // SHA-256(nonce ++ password)
  // rl-ui store.rs
  impl Store { pub fn load_identity(&self, fresh: impl FnOnce() -> [u8; 32]) -> Loaded<[u8; 32]>; }  // creates identity.ron on first use
  ```

Context: spec §5.2. `rl-lobby` is `no_std` (wasm for plan 13): randomness comes from the caller (`rl-net` and the game use `getrandom`). SHA-256 via `sha2` (already in the workspace; check it is `no_std` with `default-features = false`). The identity file (`<data>/identity.ron`, `(version: 1, secret: "<64 hex>")`) is written atomically like `settings.ron`; a damaged file is backed up and replaced by a new identity with a warning (the player's fingerprint changes; bans by key are soft by design).

- [ ] **Step 1: Write the failing tests**: sign → verify true; a changed message, key or signature → false; `from_secret([0; 32])` is None; the fingerprint of a fixed key equals a committed constant (pin the format) and is 8 characters; `challenge_message` layout; `password_proof` differs by nonce. In `rl-ui/tests/store.rs`: `load_identity` creates the file once and returns the same secret on the next load; a garbage file → new secret + warning + `.bak`.
- [ ] **Step 2: Run** → fail. **Step 3: Implement.** **Step 4: Run** `cargo nextest run -p rl-lobby -p rl-ui && cargo build --target wasm32-unknown-unknown -p rl-lobby` → pass; `cargo xtask check`.
- [ ] **Step 5: Commit** `rl-lobby: player identity keys, signatures and fingerprints` (CLAUDE.md for rl-lobby: purpose, layer 4 no_std, identity).

---

### Task 4: rl-net endpoint: sockets, signalling, many peers, limits, impairment

**Files:**
- Create: `crates/rl-net/src/{endpoint.rs, signal.rs, limits.rs, impair.rs}`, `crates/rl-net/tests/endpoint.rs`
- Modify: `Cargo.toml` (`tungstenite = "=0.30.0"` default features; `getrandom = "=0.4.3"`), `crates/rl-net/{Cargo.toml, src/lib.rs, CLAUDE.md}`
- Test: `tests/endpoint.rs` (localhost)

**Interfaces:**
- Consumes: Task 1 `Link`, Task 2 `Signal`, `rl_proto::{FINGERPRINT, PROTOCOL_VERSION, tag}`, `rl_netcode::{PeerId, Channel, netsim::Profile}`.
- Produces (Tasks 7, 13, 14):
  ```rust
  pub struct EndpointConfig { pub port: u16 /* 0 = any */, pub max_peers: usize, pub limits: Limits, pub impair: Option<rl_netcode::netsim::Profile>, pub impair_seed: u64 }
  pub struct Limits { pub max_datagrams_per_s: u32 /* 400 */, pub max_reliable_bytes_per_s: u32 /* 256 KiB, states excepted below */, pub max_signal_bytes: usize }
  pub enum NetEvent {
      Connected { peer: PeerId, addr: SocketAddr },
      Message { peer: PeerId, channel: Channel, bytes: Vec<u8> },
      Disconnected { peer: PeerId, why: Why },
      Refused { why: rl_proto::Refusal },            // client side: the host refused us
  }
  pub enum Why { Closed, Timeout, Limits, Malformed, Refused }
  pub struct Endpoint { .. }
  impl Endpoint {
      pub fn host(cfg: EndpointConfig, gate: Box<dyn FnMut(&SocketAddr, &rl_proto::Signal) -> Result<(), rl_proto::Refusal>>) -> io::Result<Endpoint>;
      pub fn connect(cfg: EndpointConfig, host: SocketAddr) -> io::Result<Endpoint>;    // the host's TCP signalling address
      pub fn local_port(&self) -> u16;
      pub fn poll(&mut self, now: Instant, events: &mut Vec<NetEvent>);   // never blocks
      pub fn send(&mut self, peer: PeerId, channel: Channel, bytes: &[u8]);
      pub fn buffered(&self, peer: PeerId, channel: Channel) -> usize;
      pub fn close(&mut self, peer: PeerId);
      pub fn peers(&self) -> impl Iterator<Item = (PeerId, SocketAddr)> + '_;
      pub fn stats(&self, peer: PeerId) -> Option<PeerStats>;           // bytes and datagrams both ways
  }
  pub const HOST: PeerId = 0;   // a client's only peer
  ```

Behaviour:
- The host binds UDP and TCP on `port` (both `0.0.0.0`; tests use port 0 and read `local_port`). The TCP listener is non-blocking; each accepted connection runs a non-blocking `tungstenite` server handshake, then reads one binary frame: `Signal::Offer` (≤ `MAX_SIGNAL`, else `Why::Malformed`, close). Checks: `protocol == PROTOCOL_VERSION` and `fingerprint == FINGERPRINT` else `Refused { Version }`; `max_peers` reached → `Full`; then the caller's `gate` (bans by IP, Task 6/13). Accept: `Link::answerer` with local candidates (decision N16: the IP of the TCP connection's local address with the UDP port; enough for LAN and direct play, and the loopback address in tests), send `Signal::Answer`, keep the WebSocket open until the link opens (then close it; a link that does not open within 10 s → close both, no event).
- The client connects TCP (non-blocking connect with a 5 s timeout), WebSocket handshake, `Link::offerer` with the candidate `(TCP local IP, UDP port)`, sends `Offer`, waits for `Answer` or `Refused` (→ `NetEvent::Refused`).
- One UDP socket for all links; incoming datagrams go to the link that `accepts` them; a datagram no link accepts is dropped (counted).
- Peer ids: host side 1, 2, 3, … never reused within an endpoint's life; client side the host is `HOST`.
- **Limits** (`limits.rs`, spec §5.4): per peer, a token bucket over 1 s for datagrams and reliable bytes; exceeding either → `Disconnected { Limits }` and the link is closed; reliable messages larger than `rl_proto::MAX_CONTROL` → `Malformed`. A peer that the session marks as receiving a state is exempt from the byte limit for that transfer (`Endpoint::allow_burst(peer, bytes)` for the receiver side; the sender side is not limited).
- **Impairment** (`impair.rs`, spec §9.4): when `cfg.impair` is set, every outgoing datagram passes through a delay/loss/duplication stage with the profile's numbers (reuse `rl_netcode::netsim`'s link model: factor the per-link loss/delay state into a pub `LinkModel` there if needed — small refactor, its tests must stay green) before `send_to`; reliable-channel retransmissions happen naturally (SCTP) since loss applies below WebRTC.
- Timeouts: a link with no traffic for 10 s → `Disconnected { Timeout }` (str0m's ICE disconnect also maps there).

- [ ] **Step 1: Write the failing tests** (`tests/endpoint.rs`): (1) a host and three clients on localhost connect, each client sends 100 unreliable and 100 reliable messages and the host echoes them; all arrive (reliable in order); (2) a client with a different fingerprint (test hook: `EndpointConfig` carries `fingerprint_override: Option<u64>` used only by tests, `#[doc(hidden)]`) gets `Refused { Version }`; (3) `max_peers: 1` → the second client gets `Refused { Full }`; (4) `floods_and_oversized_messages_drop_the_peer`: a client sending 2000 datagrams in 1 s, and another sending a 100 KB reliable message, are disconnected with `Why::Limits` / `Why::Malformed` while a third, well-behaved client stays connected; (5) a raw TCP connection that sends garbage instead of a WebSocket handshake, and one that sends a 20 KB frame, are closed without events and without panic; (6) a client closing its endpoint → host sees `Disconnected { Closed }` within 1 s; (7) with `impair: Some(Profile::BAD)` 1000 reliable messages still arrive in order and complete.
- [ ] **Step 2: Run** → fail. **Step 3: Implement.** **Step 4: Run** `cargo nextest run -p rl-net` (must finish under 30 s) and `cargo build --release --target x86_64-pc-windows-gnu -p rl-net`; `cargo xtask check`.
- [ ] **Step 5: Commit** `rl-net: endpoints with WebSocket signalling, limits and impairment`.

---
### Task 5: rl-net LAN announcements

**Files:**
- Create: `crates/rl-net/src/lan.rs`, `crates/rl-net/tests/lan.rs`
- Modify: `crates/rl-net/{src/lib.rs, CLAUDE.md}`

**Interfaces:**
- Consumes: Task 2 `LanAnnounce`, `encode_lan`, `decode_lan`, `LAN_PORT`.
- Produces (Tasks 7, 11, 13):
  ```rust
  pub struct Announcer { .. }
  impl Announcer {
      pub fn new(target: SocketAddr /* broadcast address:LAN_PORT */) -> io::Result<Announcer>;
      pub fn tick(&mut self, now: Instant, what: &LanAnnounce);   // sends at most every 2 s
  }
  pub struct LanGame { pub addr: SocketAddr /* host IP + announced port */, pub info: LanAnnounce, pub seen: Instant }
  pub struct Listener { .. }
  impl Listener {
      pub fn new(port: u16) -> io::Result<Listener>;   // binds 0.0.0.0:port with SO_REUSEADDR semantics where allowed
      pub fn poll(&mut self, now: Instant) -> &[LanGame];   // drains the socket, drops games unseen for 6 s, sorted by name then address
  }
  pub fn broadcast_targets() -> Vec<SocketAddr>;   // 255.255.255.255:LAN_PORT plus each IPv4 interface's directed broadcast when known
  ```

Behaviour: announcements are rate-limited on the receiving side too (one entry per source address; at most 64 games; a source sending more than 10 datagrams/s is ignored for 10 s). Undecodable datagrams are ignored silently. The LAN tab shows the host's announced name, players and map; the address is the datagram's source IP with the announced port.

Notes for the report (from the research): WSL2's default NAT networking does not receive LAN broadcasts from other machines (only `networkingMode=mirrored` does), and Windows Firewall may prompt on the first bind; the Direct tab is the fallback. Do not try to work around these in code.

- [ ] **Step 1: Write the failing tests** (`tests/lan.rs`): an `Announcer` targeting `127.0.0.1:<listener port>` and a `Listener` on an ephemeral port: the game appears after the first tick with the announced fields and the address `127.0.0.1:<announced port>`; a second announcement within 2 s is not sent (count datagrams with a raw socket listener); the game disappears 6 s after the last announcement (drive `now` forward: the listener takes `now` as an argument); garbage datagrams are ignored; 65 distinct fake sources (raw sockets bound to `127.0.0.1:0`) leave at most 64 entries.
- [ ] **Step 2: Run** → fail. **Step 3: Implement.** **Step 4: Run** `cargo nextest run -p rl-net --test lan`; `cargo xtask check`.
- [ ] **Step 5: Commit** `rl-net: LAN game announcements`.

---

### Task 6: rl-lobby: the lobby, host and client (sans-IO)

**Files:**
- Create: `crates/rl-lobby/src/{model.rs, host.rs, client.rs, harness.rs}`, `crates/rl-lobby/tests/lobby.rs`
- Modify: `crates/rl-lobby/{src/lib.rs, CLAUDE.md, Cargo.toml}`
- Test: `tests/lobby.rs` (in-process: hosts and clients exchange messages through a queue with optional delay; no sockets)

**Interfaces:**
- Consumes: Task 2 messages, Task 3 identity; `rl_netcode::PeerId` (rl-lobby may depend on rl-netcode: both layer 4 — add the `same_layer` pair `("rl-lobby", "rl-netcode")` with a comment "peer ids and input delay").
- Produces (Tasks 7, 11, 13):
  ```rust
  pub struct HostConfig { pub name: String, pub password: Option<String>, pub max_players: u8, pub max_spectators: u8, pub admins: Vec<PublicKey> }
  pub struct LobbyHost { .. }
  impl LobbyHost {
      pub fn new(cfg: HostConfig, me: Option<(Identity, Look)> /* None: dedicated server */, setup: MatchSetupWire, now_us: u64) -> LobbyHost;
      pub fn connected(&mut self, peer: PeerId, now_us: u64, nonce: [u8; 32]);   // the transport opened a peer: send Challenge
      pub fn receive(&mut self, peer: PeerId, msg: ToHost, now_us: u64);
      pub fn disconnected(&mut self, peer: PeerId);
      pub fn tick(&mut self, now_us: u64);                     // timeouts (unproven peers after 10 s), chat rate windows
      pub fn outgoing(&mut self) -> Vec<(PeerId, ToClient)>;   // drain
      pub fn events(&mut self) -> Vec<HostEvent>;              // drain: Kick(peer) means close the transport
      // the host player's own actions (a dedicated server has none)
      pub fn set_setup(&mut self, setup: MatchSetupWire);
      pub fn set_my_look(&mut self, look: Look); pub fn set_my_ready(&mut self, ready: bool); pub fn set_my_loadout(&mut self, l: Option<[i32; 5]>);
      pub fn chat(&mut self, text: &str);
      pub fn kick(&mut self, member: u8, ban: bool);
      pub fn move_seat(&mut self, member: u8, spectate: bool);
      pub fn can_start(&self) -> Result<(), NotStartable>;     // every player ready and complete; ≥ 2 worms incl. bots
      pub fn start(&mut self, seed: u32, rtts_ms: &BTreeMap<PeerId, u32>, start_checksum: u64) -> Result<MatchStartWire, NotStartable>;
      pub fn match_ended(&mut self, end: MatchEndWire);         // back to the lobby; spectators fill free seats in join order
      pub fn state(&self) -> &LobbyState;
      pub fn peer_of(&self, member: u8) -> Option<PeerId>;  pub fn member_of(&self, peer: PeerId) -> Option<u8>;
      pub fn banned(&self, key: &PublicKey) -> bool;  pub fn banned_ip(&self, ip: IpAddr) -> bool;   // for the endpoint's gate
  }
  pub enum HostEvent { Joined(u8), Left(u8), Kick { peer: PeerId, ban: bool }, Chat { from: u8, text: String }, StartRequested, Admin { from: u8, cmd: AdminCommand } }
  pub struct LobbyClient { .. }
  impl LobbyClient {
      pub fn new(me: Identity, look: Look, password: Option<String>) -> LobbyClient;
      pub fn connected(&mut self, now_us: u64);                // sends Hello
      pub fn receive(&mut self, msg: ToClient, now_us: u64, aux: [u8; 32]);
      pub fn outgoing(&mut self) -> Vec<ToHost>;
      pub fn events(&mut self) -> Vec<ClientEvent>;            // Welcome, Refused, Chat, Notice, Start(MatchStartWire), Ended, Kicked
      pub fn set_look/set_ready/set_spectate/set_loadout/chat/have(...);
      pub fn state(&self) -> Option<&LobbyState>;  pub fn me(&self) -> Option<u8>;
  }
  ```

Rules (spec §4, §5.2, §5.4, §7.2), each pinned by a test:
- **Join**: transport opens → host sends `Challenge { nonce, host_key, needs_password }` (a dedicated server has a host key too: its own `Identity`) → client sends `Hello` (with `password_proof` when needed) → host checks name rules (Task 2 bounds; a name equal ignoring case to another member's gets a suffix " (2)"), the password proof, bans (`Refused(Banned)`), capacity (players full → seat as spectator; spectators full too → `Refused(Full)`), then client sends `Prove { signature over challenge_message(nonce, host_key) }` → host verifies (`a_forged_signature_is_refused`: a signature by another key or over another nonce → `Refused(Malformed)` and `HostEvent::Kick`) → `Welcome` to the joiner, `Lobby` to everyone, `Notice(Joined)`. Messages other than Hello/Prove before the proof → kick. No proof within 10 s → kick.
- **Seats**: players take the lowest free slot (0..max_players); bots take slots after the players, in `setup.bots` order (re-numbered at start); spectators have no slot. A member who joins while `Playing` is `Waiting` (watches; Task 7 adds them as a netcode spectator) and gets a slot at the next `Gathering` in join order.
- **Ready**: a member is ready only when it says so and its `missing` list is empty; any change of the setup by the host clears every member's ready (players must re-confirm what they agreed to); the host's own ready is implicit.
- **Chat**: at most 200 characters (longer is cut, not refused), at most 5 lines per 5 s per member (beyond: dropped and a `Notice(Message("slow down"))` to that member); the host relays to everyone as `Chat { from, text }`.
- **Kick/ban**: only the host or an admin key; ban stores the key and the IP (the endpoint's address for the peer, given to `LobbyHost` via `connected`'s caller — add `addr: IpAddr` to `connected`) for the lobby's life; a kicked peer gets `Kicked`, then the transport closes.
- **Start**: `can_start` → `start(seed, rtts, start_checksum)` builds `MatchStartWire`: slots in order (players by slot, then bots), each member's input delay `rl_netcode::input_delay(rtt)` (host 0), phase `Starting` → `Playing` when the match layer reports its first tick (Task 7).
- **Leave**: `Bye` or a transport disconnect → member removed, `Notice(Left)`; during a match the slot stays in the match (the netcode's `peer_lost`, Task 7) and the member is gone from the lobby.
- **End**: `match_ended` → phase `Results` (shown 5 s by clients) then `Gathering`; spectators/waiting members fill free player seats in join order.

`harness.rs`: an in-process lobby harness (host + clients exchanging encoded messages through `encode_to_host`/`decode_to_host` etc., so encoding is exercised, with a configurable delay) used by the tests and by Task 7.

- [ ] **Step 1: Write the failing tests** (`tests/lobby.rs`), one per rule, including: `three_players_and_a_spectator_gather_ready_and_start`, `a_forged_signature_is_refused`, `a_wrong_password_is_refused`, `banned_keys_and_ips_are_refused`, `duplicate_names_get_a_suffix`, `setup_changes_clear_ready`, `chat_is_cut_and_rate_limited`, `only_admins_kick`, `missing_content_prevents_ready`, `joiners_during_a_match_wait_and_get_seats_after`, `leaving_at_every_stage` (before proof, after welcome, while starting, while playing, during results: the lobby stays consistent and others are notified), `the_start_message_assigns_slots_and_delays`.
- [ ] **Step 2: Run** → fail. **Step 3: Implement** (`no_std` + `alloc`; `BTreeMap` only). **Step 4: Run** `cargo nextest run -p rl-lobby && cargo build --target wasm32-unknown-unknown -p rl-lobby`; `cargo xtask check`.
- [ ] **Step 5: Commit** `rl-lobby: the lobby's host and client`.

---

### Task 7: rl-netcode: plan 9's deferred items for real play

**Files:**
- Modify: `crates/rl-netcode/src/{session.rs, sync.rs, harness.rs, inputs.rs}`, `crates/rl-proto/src/lib.rs` (new control variants), tests in `crates/rl-netcode/tests/`, `crates/rl-netcode/CLAUDE.md`
- Test: new tests per item

Items (each with a failing test first):
1. `pub fn confirmed(&self) -> u32` (the private `confirmed()`: `max(frontier, floor)`), documented as "ticks below it are final (the HUD's ticks, spec §3.4)".
2. **Leaving cleanly**: `Control::Bye` — a client's `Session::leave()` sends it; the host maps it to `peer_lost` at once (no 10 s timeout); the client stops sending. Test: a client leaves at tick 300 → the host's slot becomes `Keys(0)` filled from the next tick, no `PeerTimedOut`.
3. **Late coalesced**: one `Control::Late { from, to }` (a tick range) per received datagram instead of one per tick; `Event::Late { from, to }`. Update every user. Test: a client whose uplink is cut for 1 s gets at most one `Late` per datagram the host received afterwards.
4. **Lagging on the client**: the client raises `Event::Lagging(true)` when it received `Late` for at least 3 s worth of ticks within the last 4 s, and `Lagging(false)` after 2 s without `Late` (spec §3.2's warning on the lagging player's screen).
5. **Stall episodes**: `stats.stalls` counts episodes (a run of stalled advances counts once); `Event::Stalled` once per episode. Update the feel numbers' definition (`stalls` = episodes) and re-bless `fixtures/net/feel.ron` with the reason "stalls count episodes"; the absolute rule (0 up to `bad`) is unaffected.
6. **Repair timing from the client's load**: the client sends `Control::StateLoaded { tick }` after loading a state; the host's re-arm window (`REPAIR_MARGIN`) starts when that arrives (not at send time), so a multi-MB state over a slow link is not re-sent while still downloading. Until it arrives, mismatches are ignored, bounded by 60 s (then `RepairFailed`).
7. **The spurious `Malformed` at a spectator joined before any confirmed tick**: a frame datagram older than the state floor is ignored, not counted as malformed.
8. `sim_mut` is `#[doc(hidden)]` and documented as test/harness only (like `change_confirmed`).
9. **A host-freeze scenario** in the harness (`PeerPlan.freeze` for the host): the host stalls 2 s; clients stall at the prediction limit and recover; verdict holds.
10. **Tick records for the game** (`rl_netcode::world_sim`): `TickedTick`, `ConfirmedTick`, `TickRecorder` exactly as Task 9's Interfaces block lists them (Task 8 fills them during matches, Task 9 consumes them). `ConfirmedTick` copies `World::scores()`, the deathmatch time left (through `mode_state()` downcast: rl-netcode may depend on rl-modes, layer 3) and `World::result()` when the tick is simulated, keeps them until the tick is confirmed, and is replaced when the tick is re-simulated. Test: records come out once per tick, in order, only below `confirmed()`, and equal the final timeline's after rollbacks.
11. **A structure-aware datagram proptest**: well-formed `ClientInputs`/`HostInputs` with edge values (`first`, `ack`, `tick`, `margin`, stamps at 0, `u32::MAX`, around the current tick) into `receive` + `advance` for client, spectator, joining spectator and host; no panic; final inputs never change.

- [ ] **Steps**: write the tests (one per item), see them fail, implement, `cargo nextest run -p rl-netcode -p rl-proto -p rl-cli --test feel --test net_sim`, `cargo xtask net-feel --bless --reason "stalls count episodes"`, `cargo xtask check`, commit `rl-netcode: confirmed tick, clean leaves, coalesced lateness, repair timing from the load`.

---
### Task 8: rl-online: from lobby to match and back

**Files:**
- Create: `crates/rl-online/{Cargo.toml, CLAUDE.md, src/lib.rs, src/matches.rs, src/peer.rs, src/replay.rs, src/events.rs}`, `crates/rl-online/tests/online.rs`
- Modify: `Cargo.toml` (`rl-online` path dep), `xtask/layers.ron` (`"rl-online": 5`, same_layer `("rl-online", "rl-net")`), `crates/rl-cli/src/netsim.rs` (move its `BotDriver`/world building onto `rl_online::matches`, so there is one implementation)
- Test: `tests/online.rs` (localhost endpoints: a host peer and clients run full flows)

**Interfaces:**
- Consumes: Tasks 4-7 (Task 7's `TickRecorder`, `TickedTick`, `ConfirmedTick`, `Session::confirmed`, `leave`, `Event::Lagging`); `rl_netcode::{Session, Config, Role, Seat, WorldSim, Driver, Event}`; `rl_content::{Content, MatchOptions, Player}`; `rl_bots`; `rl_replay`.
- Produces (Tasks 10-14):
  ```rust
  // matches.rs
  pub fn options(start: &MatchStartWire) -> anyhow::Result<MatchOptions>;   // slots → players (bots as Player::Bot, members as Human/HumanWith)
  pub struct MatchWorld { pub sim: WorldSim, pub bots: BTreeMap<u8 /* slot */, rl_bots::Bot> }
  pub fn build(content: &Content, start: &MatchStartWire) -> anyhow::Result<MatchWorld>;   // checks packs' hashes; its own AreaCache
  pub struct HostBots { .. }   // Driver<WorldSim>::local_inputs for the host's bot slots (+ the host player's keys via a callback)
  // peer.rs
  pub enum PeerRole { Host(HostConfig), Client { addr: SocketAddr, password: Option<String> } }
  pub struct OnlinePeer { .. }
  impl OnlinePeer {
      pub fn start(role: PeerRole, endpoint: EndpointConfig, me: Identity, look: Look, content: Content, setup: Option<MatchSetupWire>) -> io::Result<OnlinePeer>;
      pub fn poll(&mut self, now: Instant, local_keys: &mut dyn FnMut() -> Keys, on_tick: &mut dyn FnMut(&World, &TickEvents, TickInfo)) -> Vec<OnlineEvent>;  // network, lobby, match advance
      pub fn lobby(&mut self) -> LobbyHandle<'_>;     // the host's or client's lobby actions (ready, chat, look, setup, kick, start)
      pub fn match_session(&self) -> Option<&rl_netcode::Session<WorldSim>>;
      pub fn match_session_mut(&mut self) -> Option<&mut rl_netcode::Session<WorldSim>>;
      pub fn my_slot(&self) -> Option<u8>;
      pub fn stats(&self) -> NetStats;               // rtt, input delay, rollback depth, loss, corrections/s (for the overlay)
      pub fn leave(&mut self);                       // Bye to everyone, close
  }
  pub enum OnlineEvent {
      Lobby(LobbyState), Chat { from: String, text: String }, Notice(String),
      MatchStarting { start: MatchStartWire, my_slot: Option<u8> },
      Ticked(Vec<TickedTick>),        // every tick simulated during this poll, in order, with its events (Task 9 consumes)
      Restored(Vec<rl_terrain::Area>),// areas a rollback or a loaded state put back (drained after each advance)
      Confirmed(Vec<ConfirmedTick>),  // newly confirmed ticks (HUD, log, results; spec §3.4)
      MatchEnded(MatchEndWire),
      Lagging(bool),                  // this client's inputs miss the window (3 s), spec §3.2
      Left(String), Kicked { banned: bool }, Refused(String), Disconnected(String),
      ReplayReady(Vec<u8>),           // encoded .rlrp, produced after the match (decision N18)
  }
  ```

Behaviour:
- **Host**: the endpoint's `gate` asks the lobby (`banned_ip`); `Connected` → `LobbyHost::connected` with a fresh `getrandom` nonce; reliable messages are demultiplexed by `rl_proto::tag` (netcode `Control` → session, `ToHost` → lobby); the lobby's outgoing go out tagged; `Kick` → close the peer.
- **Start** (spec §4): the host computes `MatchStartWire` from the lobby (`start`), builds its own world (`build`), puts `world.checksum()` of the start state into the message, sends it, and creates `Session::new(Config::new(slots, 0), Role::Host { peers }, sim, now_us)` where each player member's peer gets `Seat::Player(slot)`, spectators and waiting members `Seat::Spectator`. A client receiving `Start` builds the world, compares the start checksum (mismatch → `OnlineEvent::Refused("your simulation differs from the host's: version or content")` and leaves), creates `Role::Client { host: HOST, slot }` (or `Role::Spectator`) with `Config::new(slots, delay)`.
- **Mid-match joiners** become netcode spectators via `Session::add_spectator` and `Role::Joining`.
- **Advance**: each `poll`: receive datagrams into the session, `advance(now_us, driver)` where the driver gives local keys (host: its player's slot from `local_keys`, bots from `HostBots`; client: its slot from `local_keys`) and, in `simulated`, feeds Task 7's `TickRecorder` and calls the caller's `on_tick(&World, &TickEvents, TickInfo)` callback (the game's scene snapshots need the world as of each tick, Task 9); transmit; then emit `Ticked` (the recorder's ticks), `Restored` (`take_restored_areas`) and `Confirmed` (the recorder's records below `Session::confirmed()`).
- **Session events**: `PeerTimedOut`/`RepairFailed` (host) → `peer_lost` + lobby `disconnected`; `JoinRefused` → close; a client's `Lagging` (Task 7) → `OnlineEvent::Lagging`; leaving sends the netcode's `Bye` (Task 7) and the lobby's `Bye`.
- **End**: the host sees `GameOver` on a confirmed tick → waits 300 confirmed ticks (results shown in-game, like local play's `RECORD_AFTER_END`) → `MatchEndWire` to the lobby (`match_ended`) and `Ended` to everyone; the session is dropped (decision N18); every peer then produces the replay on a worker thread (`replay.rs`: `rl_replay::Recorder` driven by re-simulating the confirmed input rows from the start world; a mismatch against the netcode's confirmed checksums is a desync bundle-worthy error: log it and still return the replay).
- `rl-cli`'s `netsim.rs` keeps its tests green after moving `BotDriver` onto `rl_online::matches::HostBots`.

- [ ] **Step 1: Write the failing tests** (`tests/online.rs`, localhost, content from `RL_CONTENT_DIR`, Classic on Dirt Level): (1) `a_host_and_two_clients_play_a_short_match`: lobby, ready, start (time limit 0.1 min), every peer's confirmed checksums equal at the end, `MatchEnded` everywhere, `ReplayReady` everywhere with equal bytes; (2) `a_late_joiner_watches_then_plays_the_next_match`; (3) `a_client_with_other_content_is_refused_at_start` (start checksum mismatch: give one client a world built from a different seed through a test hook); (4) `a_client_vanishes_mid_match` (drop its endpoint: the host and the other client finish the match; the vanished slot is filled with `Keys(0)`); (5) `kicked_players_cannot_rejoin`. Bound the run time: each test under 20 s.
- [ ] **Step 2: Run** → fail. **Step 3: Implement.** **Step 4: Run** `cargo nextest run -p rl-online -p rl-cli`; `cargo xtask check`.
- [ ] **Step 5: Commit** `rl-online: lobbies, matches and replays over real connections`.

---

### Task 9: Presentation under rollback (spec §3.4)

**Files:**
- Create: `crates/rl-present/src/net.rs`, `crates/rl-present/tests/net.rs`
- Modify: `crates/rl-present/src/lib.rs`, `crates/rl-scene/src/{lib.rs, camera.rs}` (a shake-free `after_tick` for re-simulated ticks), `crates/rl-present/CLAUDE.md`, `crates/rl-scene/CLAUDE.md`
- Test: `tests/net.rs` (a real world under `rl_netcode::WorldSim` with scripted rollbacks; no window, `Audio::silent()`-style counting)

**Interfaces:**
- Consumes: Task 7's `rl_netcode::world_sim::{TickedTick, ConfirmedTick, TickRecorder}` (plain data, layer 4, seen by rl-present, rl-online and rl-game):
  ```rust
  // rl_netcode::world_sim (layer 4) — defined by Task 7
  pub struct TickedTick { pub tick: u32, pub resim: bool, pub substeps: u8, pub lost: Option<rl_sim::LostArea>, pub events: Vec<rl_sim::Event> }
  pub struct ConfirmedTick { pub tick: u32, pub events: Vec<rl_sim::Event> /* gameplay kinds only */, pub scores: Vec<rl_sim::Score>, pub time_left_ms: Option<u64>, pub result: Option<rl_sim::MatchResult>, pub positions: Vec<(i32, rl_math::Vec2)> /* worm id → pos, for Damage popups */ }
  pub struct TickRecorder { .. }   // fed from Driver::simulated; records TickedTick for every simulated tick, keeps ConfirmedTick candidates until confirmed
  impl TickRecorder { pub fn simulated(&mut self, sim: &WorldSim, info: TickInfo); pub fn take_ticked(&mut self) -> Vec<TickedTick>; pub fn take_confirmed(&mut self, confirmed: u32) -> Vec<ConfirmedTick>; }
  // rl_present::net
  pub struct EventKey { pub tick: u32, pub substep: u8, pub class: u64, pub ordinal: u16 }   // class excludes floats
  pub fn event_keys(tick: u32, events: &[rl_sim::Event]) -> Vec<Option<EventKey>>;          // None for kinds never deduped (terrain edits)
  pub struct NetPresenter { .. }   // what has been played/spawned per key, pruned below the confirmed tick
  impl NetPresenter {
      pub fn new() -> Self;
      /// Applies one advance's restored areas then its ticks to the presentation; returns the sounds to play now.
      pub fn apply(&mut self, present: &mut Presentation, world: &rl_sim::World, restored: &[rl_terrain::Area], ticks: &[TickedTick], sounds: &mut Vec<rl_sim::Event>);
      pub fn confirmed(&mut self, tick: u32);    // prune
      pub fn reset(&mut self);                   // a loaded state: everything restarts
  }
  ```

Behaviour (the research's design, decision N20):
1. **Keys**: `class` = a hash of the kind's discriminant and its integer identity fields (Sound id; WeaponFired worm+weapon; Damage worm+by; Kill victim+killer; Spawned worm; SpawnCosmetic kind incl. its worm; Explosion shake; BonusPickup worm+kind; Message text id+args); positions, velocities and amounts are excluded (a corrected re-simulation moves them). `ordinal` counts equal (tick, substep, class) within the tick.
2. **Order per advance**: first rebuild every restored area (`present.color_mut().rebuild_area(area, world.terrain())`), then for each `TickedTick` in order:
   - a tick **not seen before** (above the highest tick already shown): run today's step with all its events (`Fx::step_tick` through a hand-built `FxTick` from the recorded events/substeps/lost), record every key as played, play every `Sound`;
   - a **re-simulated** tick (seen before): `FxTick { events: all terrain edits + only SpawnCosmetic whose key is new, substeps: 0, .. }` (no entity passes: entities do not age twice), play only `Sound`s whose key is new, record the new keys.
   The scene's snapshot and marks are taken inside the advance, where the world is as of each tick: Task 8's `OnlinePeer::poll` calls an `on_tick(&World, &TickEvents, TickInfo)` callback from the driver's `simulated`, and the game calls `Scene::after_tick` for new ticks and `Scene::after_tick_resimulated` for re-simulated ones there. `NetPresenter::apply` then does colour, effects and sounds after the advance.
3. After the advance, in debug and test builds, `present.terrain_drift(world)` must be false.
4. A loaded state (`StateLoaded`): `present.restart`-like reset of fx and scene, `NetPresenter::reset`, and the whole-map restored area rebuilds the colour image.

- [ ] **Step 1: Write the failing tests** (`tests/net.rs`): build a Dirt Level match world under `WorldSim` with 4 scripted-key worms, a `TickRecorder`, a `Presentation` and a `NetPresenter`; drive a `rl_netcode::Session` pair over the netsim (`typical` profile) or, simpler and deterministic, script rollbacks by hand: tick 50 ticks predicted, then replace inputs for ticks 40..50 and re-simulate (through `Session` with a second scripted peer, or through `WorldSim::restore` + re-tick calling the recorder with `resim: true`). Assert `rollbacks_play_each_sound_once_and_keep_the_terrain`: (a) the multiset of played sound keys equals the keys of the final timeline's events plus the erased ones (never a key twice); (b) `terrain_drift` is false after every advance; (c) the fx entity count never exceeds what the final timeline spawned plus the erased spawns; (d) confirmed records arrive in tick order, once each, with the final timeline's scores.
- [ ] **Step 2: Run** → fail. **Step 3: Implement.** **Step 4: Run** `cargo nextest run -p rl-present -p rl-scene -p rl-netcode`, the render goldens (`cargo nextest run -p rl-cli --test render_golden` with the GPU tier env if available, else `cargo xtask full`'s GPU step later) must be unchanged; `cargo xtask check`.
- [ ] **Step 5: Commit** `rl-present: sounds and effects once per event across rollbacks`.

---

### Task 10: rl-game: the local slot, per-slot looks, and the online session

**Files:**
- Create: `crates/rl-game/src/online.rs`, `crates/rl-game/tests/online.rs`
- Modify: `crates/rl-game/src/{session.rs, hud.rs, overlay.rs, drive.rs, setup.rs}`, `crates/rl-ui/src/hud/{view.rs, paint.rs}`, `crates/rl-present/src/lib.rs` (camera target), `crates/rl-game/Cargo.toml` (`rl-online`, `rl-netcode`, `rl-proto`, `rl-lobby`, `rl-net`), `crates/rl-game/CLAUDE.md`
- Test: `tests/online.rs`, existing `tests/{deathmatch,ingame,menu}.rs` stay green

**Interfaces:**
- Consumes: Task 8 `OnlinePeer`, `OnlineEvent`; Task 9 `NetPresenter`, `TickedTick`, `ConfirmedTick`.
- Produces (Tasks 11, 12):
  ```rust
  // session.rs
  pub struct Session { .., local: Option<u8> /* the slot this player controls; None: watching */, looks: Vec<WormLook> }   // replaces `human: bool` + worm-0 logic
  pub struct WormLook { pub name: String, pub colour: [u8; 3], pub skin: LoadedSkin }
  pub enum Driver { Sandbox, Match(Box<MatchDriver>), Replay(ReplayDriver), Online(Box<OnlineDriver>) }
  impl Session { pub fn local(&self) -> Option<u8>; pub fn follow(&mut self, slot: u8); pub fn follow_next(&mut self, step: i32); }
  // online.rs
  pub struct OnlineDriver { peer: OnlinePeer, presenter: NetPresenter, confirmed_scores: Vec<Score>, .. }
  ```

Behaviour:
- **Local slot**: every place that assumed worm 0 (research list: session.rs names, `show_new_worms`, keys, kill credit, `Spawned{worm:0}`; hud.rs `WormHud::of(.., 0, ..)` and the minimap's big dot; rl-ui paint.rs's yellow scoreboard row; rl-present's `Camera::new(.., 0, ..)`; shell.rs `arm`) uses `Session::local()`; local play sets `local = Some(0)` (behaviour unchanged: the existing tests and goldens prove it), replays and spectating `None` (the HUD follows the camera's target).
- **Per-slot looks**: `Session` loads one `LoadedSkin` per distinct skin id (cache by id) and registers each worm with its own look; local play keeps today's colours (the player's profile colour, `bot_colours` for bots), online uses the lobby's `Look`s. A skin the joiner lacks falls back to the default skin (plan 11 downloads it).
- **Online driver** (`Driver::Online`): `Session::tick` is not used; instead `Session::frame(now)` (new) polls the `OnlinePeer` once per frame with `local_keys` reading the input latch (`InputState::latch_tick()` is called by the netcode's `local_inputs`, i.e. once per new tick), applies `Ticked`/`Restored` through `NetPresenter` (sounds via `audio.play(&sounds, listener)`), feeds `Confirmed` ticks to the existing after-tick logic (log, tally, results, time left, popups) — refactor `after_tick` to take a `ConfirmedTick`-like view so local play and online share it — and draws the newest predicted world (`peer.match_session().sim().world()`).
- **Spectating**: `local == None`; Tab/PageDown/PageUp cycle the followed worm (`follow_next`), the HUD shows the followed worm.

- [ ] **Step 1: Write the failing tests**: `the_local_slot_drives_everything` (a local match where the player is slot 2: build `MatchOptions` with bots in slots 0-1 and the human in 2 — add a `MatchPrefs`-independent test constructor — and assert the HUD's worm, the camera target, the crosshair's `local` flag, the scoreboard highlight row, kill credit and `player_deaths` all use slot 2); `an_online_match_runs_headless` (rl-game's `Session` with `Driver::Online` against a localhost host from `rl_online` with bots: 300 frames at a fake 10 ms clock, no panic, `terrain_drift` false, the HUD's scores equal the confirmed records); existing tests unchanged.
- [ ] **Step 2: Run** → fail. **Step 3: Implement.** **Step 4: Run** `cargo nextest run -p rl-game -p rl-ui -p rl-present` and the GPU-tier HUD snapshots if available (`cargo xtask full` later); `cargo xtask check`.
- [ ] **Step 5: Commit** `rl-game: the local slot, per-worm looks and the online session`.

---

### Task 11: rl-ui: Multiplayer, Host and Lobby screens

**Files:**
- Create: `crates/rl-ui/src/screens/{multiplayer.rs, host.rs, lobby.rs}`, `crates/rl-ui/src/online.rs`, `crates/rl-ui/tests/online.rs`
- Modify: `crates/rl-ui/src/{state.rs, screens/mod.rs, screens/main.rs, screens/local_play.rs (make rules/level_grid/mod_picker/bots pub(super)), settings.rs (OnlineSettings)}`, `crates/rl-ui/tests/layout.rs`, `crates/rl-game/src/autoplay.rs` (SCREENS), `crates/rl-ui/CLAUDE.md`
- Test: `tests/online.rs` (egui_kittest flows), `tests/layout.rs` (the new screens are centred columns)

**Interfaces:**
- Produces (Task 12):
  ```rust
  pub enum Screen { .., Multiplayer, Host, Lobby }
  pub enum UiAction { .., Host(HostPrefs), Join { addr: String, password: Option<String> }, JoinLan(SocketAddr), LobbyReady(bool), LobbySpectate(bool),
                      LobbyChat(String), LobbySetup(MatchPrefs), LobbyKick { member: u8, ban: bool }, LobbyStart, LobbyLoadout(Option<[i32; 5]>),
                      LeaveLobby, OnlineFailed(String), LobbyJoined }
  pub enum AppCommand { .., HostGame(HostPrefs), JoinGame { addr: String, password: Option<String> }, Lobby(LobbyCommand), LeaveOnline }
  pub struct HostPrefs { pub name: String, pub visibility: Visibility /* Lan | DirectOnly */, pub password: String, pub max_players: u8, pub port: u16 }
  pub struct OnlineSettings { pub last_address: String, pub host: HostPrefs, pub port: u16 }   // in Settings, #[serde(default)]
  // MenuData gains: pub lan_games: Vec<LanEntry>, pub lobby: Option<LobbyView>, pub chat: Vec<LogLine>, pub online_status: Option<String>
  pub struct LobbyView { pub name: String, pub me: u8, pub host: bool, pub phase: String, pub members: Vec<MemberView>, pub setup: MatchPrefs, pub can_start: Result<(), String> }
  pub struct MemberView { pub id: u8, pub name: String, pub fingerprint: String, pub colour: [u8; 3], pub seat: String, pub ready: bool, pub missing: Vec<String>, pub ping_ms: Option<u16>, pub admin: bool }
  ```

Screens (spec §7.1, §7.2; every page uses the centred `page()` frame):
- **Main menu** gains "Multiplayer" (after Local Play).
- **Multiplayer**: two sections, "On this network" (the LAN list: name, map, players, a lock for a password; clicking joins; "Searching…" when empty) and "Direct" (address field `host[:port]`, password field, Join button, the last address remembered); footer "Host a game" → Host.
- **Host**: name, who can join (On this network / Only by address), password, maximum players (2..=32), port; Start hosting → the Lobby.
- **Lobby**: left: the match panels from Local Play (editable for the host; read-only, greyed for others) plus the weapons choice ("Random" or the weapon screen); right: members (name, fingerprint in grey, colour swatch, seat, ready tick, ping, missing content in red, admin badge; for the host: kick and ban buttons, move to spectators/players, "Add bot"/"Remove bot" in the bots panel), chat (scrolling log, a single-line input, Enter sends), Ready/Spectate toggles; footer: Start (host, enabled by `can_start`, its reason as the tooltip) or "Waiting for the host"; Back = leave the lobby (with a confirmation if a match is running).
- Errors (`OnlineFailed`) show as the screen's red message line.

- [ ] **Step 1: Write the failing tests** (`tests/online.rs`, kittest, hand-built `MenuData`): open Multiplayer from Main; type an address and Join emits `JoinGame`; a LAN entry click emits `JoinLan`; Host → Start hosting emits `HostGame`; in a Lobby view as host: changing the time limit emits `LobbySetup`, Start disabled until ready with the reason as tooltip, kick emits `LobbyKick`; as a client the settings are read-only (clicks change nothing); chat Enter emits `LobbyChat` and clears the field; Back from the Lobby emits `LeaveLobby`. `layout.rs`: the three screens are centred columns at 1280×720 and 3440×1440.
- [ ] **Step 2: Run** → fail. **Step 3: Implement.** **Step 4: Run** `cargo nextest run -p rl-ui -p rl-game`; `cargo xtask check`.
- [ ] **Step 5: Commit** `rl-ui: Multiplayer, Host and Lobby screens`.

---

### Task 12: rl-game: online play from the menus

**Files:**
- Modify: `crates/rl-game/src/{shell.rs, app.rs, app/handler.rs, app/menus.rs, overlay.rs, main.rs}`, `crates/rl-game/tests/online.rs`, `crates/rl-game/CLAUDE.md`
- Test: `tests/online.rs` (Shell-level flows with a localhost host)

**Interfaces:**
- Consumes: Tasks 8, 10, 11.
- Produces: `Launch::Online`, the network overlay, in-game chat, flags `--host [name]`, `--join <addr>`, `--net-sim <profile>`, `--port <n>`.

Behaviour:
- **Shell**: `AppCommand::HostGame/JoinGame` create the `OnlinePeer` (identity from `Store::load_identity`, look from the selected profile, content from the shell's roots); the shell polls it every frame (also while no match runs), turns `OnlineEvent`s into `MenuData` (lobby view, chat lines, LAN list from a `Listener` while the Multiplayer screen is open) and `UiAction`s (`LobbyJoined`, `OnlineFailed`, `MatchStarted`); `MatchStarting` → load assets for the match (mod, level, theme, every slot's skin) and start a `Session` with `Driver::Online`; `MatchEnded` → Results screen, then back to the Lobby (not Main); `ReplayReady` → `replays::auto_save`.
- **Hidden window** (decision N21): `about_to_wait` keeps `ControlFlow::Poll` and calls `App::frame` without rendering while `shell.online_active()`; ticks and network continue.
- **In-game chat**: `T` opens a one-line text field over the playfield (keys go to the field, not the game, while it is open; Enter sends, Esc cancels); chat lines appear in the HUD log as `LineKind::Chat` ("name: text").
- **Overlay** (`F3` toggles; spec §7.3): ping, input delay, rollback depth (last and p99 over 5 s), loss, corrections per second, the frontier lag; the `Lagging` warning ("Connection problem") shows at the top while true.
- **Flags**: `--host [name]` hosts with the profile's look and the saved match settings and opens the Lobby directly; `--join <addr>` joins; `--net-sim <profile>` (spec §9.10) starts an in-process host with 3 bots and joins it as a client over an impaired loopback link (`EndpointConfig::impair`), so a single player feels a profile; `--port <n>`.

- [ ] **Step 1: Write the failing tests** (`tests/online.rs`): `hosting_and_joining_from_the_shell` (two `Shell`s in one process: one hosts on an ephemeral port, the other joins `127.0.0.1:<port>`; both reach the Lobby screen; the host adds a bot, both ready, the host starts; both reach InGame; 200 frames later both show the same confirmed scores; leaving returns the client to Multiplayer and the host's lobby shows it left); `a_hidden_window_keeps_the_match_alive` (the app-level frame path with `drawable == false` still advances the online session: test through the function that `about_to_wait` calls); `chat_reaches_the_other_player_in_game`; `net_sim_flag_starts_a_local_match` (headless: the flag's setup function builds a host and a client and they connect).
- [ ] **Step 2: Run** → fail. **Step 3: Implement.** **Step 4: Run** `cargo nextest run -p rl-game`; then a manual run `cargo run -p rl-game -- --net-sim typical --autoquit-after 10` (on WSLg) must play 10 s without errors (note the result in the report); `cargo xtask check`.
- [ ] **Step 5: Commit** `rl-game: host, join and play online from the menus`.

---
### Task 13: rl-server: the dedicated server

**Files:**
- Create: `crates/rl-server/{Cargo.toml, CLAUDE.md, src/main.rs, src/config.rs, src/admin.rs, src/rotation.rs}`, `crates/rl-server/tests/server.rs`, `fixtures/server/example.ron`
- Modify: `Cargo.toml`, `xtask/layers.ron` (`"rl-server": 6`)
- Test: `tests/server.rs` (the server's library part driven in-process against `rl_online` clients on localhost)

**Interfaces:**
- Consumes: Task 8 `OnlinePeer` (host role without a local player), Task 5 `Announcer`.
- Produces (Tasks 14, 15): `rl-server --config <file.ron> [--content <dir>] [--data-dir <dir>]`, a library entry `rl_server::run(config, content, stop: &AtomicBool, log: &mut dyn Write) -> anyhow::Result<()>` for tests.

Config (spec §7.4), RON, `#[serde(deny_unknown_fields)]`, every field documented in `fixtures/server/example.ron`:
```ron
(
    name: "Office Liero",
    visibility: Lan,            // Lan | DirectOnly (Nostr arrives in plan 12)
    password: None,
    port: 23400,
    max_players: 16,
    max_spectators: 8,
    admins: ["<64 hex public key>"],
    fill_bots: 4,               // bots added while fewer players than this
    rotation: [
        (mod: "olx-classic", level: "olx-level-dirt-level", time_limit: 5.0, kill_limit: 15, lives: -2, bonuses: true),
        (mod: "olx-mw-1-0", level: "olx-level-castlestrike", time_limit: 5.0, kill_limit: 20, lives: -2, bonuses: true),
    ],
    start_when_ready: true,     // start as soon as every player is ready (at least `min_players`)
    min_players: 2,
    bans_file: Some("bans.ron"),
)
```

Behaviour: a server identity (`<data>/server-identity.ron`, created on first start); hosts with `OnlinePeer` (no local player); fills bots; starts the next rotation entry when `start_when_ready` and every player is ready (or 60 s after the previous match with at least `min_players`); admins' chat commands: `/kick <name>`, `/ban <name>`, `/next` (end the match, next map), `/settings <field> <value>` (time_limit, kill_limit, lives, bonuses), `/help`; anyone: `/players`; bans persist to `bans_file` (keys and IPs, RON); logs one line per event to the log writer (and stdout in the binary): joins/leaves with fingerprint and IP, chat, kicks, match start/end with the winner, desyncs and repairs. `SIGINT`/Ctrl+C stops cleanly (`Bye` to everyone). No window, no GPU, no audio.

- [ ] **Step 1: Write the failing tests** (`tests/server.rs`, localhost, Classic/Dirt Level, a 0.1-minute time limit): `a_server_runs_a_rotation` (two bot-driven `rl_online` clients join, ready, the server starts entry 0, the match ends, the lobby returns, entry 1 starts automatically after ready); `admins_kick_and_ban_by_chat` (an admin key's `/ban Name` bans; the banned key cannot rejoin; the ban is in the bans file after restart); `non_admins_cannot_use_admin_commands`; `the_example_config_parses`.
- [ ] **Step 2: Run** → fail. **Step 3: Implement.** **Step 4: Run** `cargo nextest run -p rl-server`; `cargo build --release --target x86_64-pc-windows-gnu -p rl-server`; `cargo xtask check`.
- [ ] **Step 5: Commit** `rl-server: the dedicated server with rotation, bots and admin commands`.

---

### Task 14: Headless bot clients and real-transport tests (spec §9.4)

**Files:**
- Create: `crates/rl-cli/src/{net_client_cmd.rs, transport.rs}`, `crates/rl-cli/tests/transport.rs`
- Modify: `crates/rl-cli/src/{main.rs, lib.rs}`, `crates/rl-cli/Cargo.toml` (`rl-online`, `rl-net`, `rl-lobby`), `xtask/src/check.rs` (`FULL_TESTS` gains `("rl-cli", "transport")`), `crates/rl-cli/CLAUDE.md`, `xtask/CLAUDE.md`

**Interfaces:**
- Produces: `rl-cli net-client --join <addr> --bot [--difficulty ..] [--name ..] [--matches N] [--impair <profile>]` (a bot that joins, readies, plays N matches, prints its confirmed checksums per match and exits); `rl_cli::transport::{TransportSpec, run_transport(content, spec) -> anyhow::Result<TransportRun>}` running `rl_server::run` in a thread plus N bot clients (threads) over localhost with an impairment profile, collecting every peer's confirmed checksums and replays.

Verdict (spec §9.4): every peer's confirmed checksum list equals the server's (players and spectators; a peer that was dropped is reported, and fails the test unless the spec expects it), every peer's replay bytes are equal, and a replay played back reproduces the server's checksums.

- [ ] **Step 1: Write the failing tests** (`tests/transport.rs`): check tier: `two_bots_play_a_match_over_real_connections` (server + 2 clients, `typical` impairment on the clients, 0.2-minute match, under 30 s); full tier (`if !full_tier() { return; }`): `eight_bots_per_profile` (server + 8 clients for `good`, `typical`, `bad`, each a 0.5-minute match) and `a_spectator_joins_mid_match_over_real_connections`.
- [ ] **Step 2: Run** → fail. **Step 3: Implement.** **Step 4: Run** `cargo nextest run -p rl-cli --test transport` and `RL_FULL_TESTS=1 cargo nextest run -p rl-cli --test transport` (record times); `cargo xtask check`.
- [ ] **Step 5: Commit** `rl-cli: headless bot clients and real-transport matches`.

---

### Task 15: The online end-to-end run

**Files:**
- Create: `fixtures/autoplay/online.ron`, `fixtures/server/e2e.ron`
- Modify: `xtask/src/e2e.rs` (a second run: start `rl-server --config fixtures/server/e2e.ron` and a bot client, then `rl-game --autoplay fixtures/autoplay/online.ron` without `--fake-clock`), `crates/rl-game/src/autoplay.rs` (`Do::Type(text)` for the address and chat fields, if missing; `Check::Lobby { members }`), `xtask/CLAUDE.md`

Script (`online.ron`): Main → Multiplayer → type `127.0.0.1:<port from the e2e config>` → Join → `WaitFor("Lobby")` → assert 3 members (the server's bot client counts as one member; the server itself is not a member) → type a chat line and Enter → Ready → `WaitFor("InGame")` (the server starts when all are ready) → press keys for 3 s → `Screenshot("online")` → `WaitFor("Results")` (0.2-minute limit) → `WaitFor("Lobby")` → Back (leave) → `WaitFor("Multiplayer")` → Quit. The e2e passes iff both runs pass; the server and bot client are killed at the end; their logs are checked for `panicked`/`error:` like the game's.

- [ ] **Steps**: write the script and the xtask change; run `cargo xtask e2e` (record the time; it must stay under 4 minutes for both runs); `cargo xtask check`; commit `e2e: an online match through the menus against the dedicated server`.

---

### Task 16: Records

**Files:** `docs/porting/STATUS.md` (`## Online play decisions (plan 10)`: N15-N21 and the execution rulings the controller supplies), `CHANGELOG.md` (`### Added`: online play from the menus (host, LAN list, direct address, lobby with chat, ready, spectators, kick and ban); the dedicated server `rl-server`; `rl-game --net-sim <profile>`; the network overlay (F3); online replays), `docs/superpowers/specs/2026-10-12-networking-design.md` (status: "plan 10 implemented"), `README` or a new `docs/online.md` (how to host, join, run a server, open the port; WSL2's LAN limitation and the mirrored-networking setting).

- [ ] **Steps**: write; `cargo xtask check`; commit `docs: plan 10's decisions, changelog and how to play online`.
