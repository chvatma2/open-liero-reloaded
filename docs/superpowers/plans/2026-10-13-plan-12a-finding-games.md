# Plan 12a: Finding Games (Nostr Game List and Invite Codes) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Strangers find each other's games without a master server.
- Hosts and dedicated servers announce their games on public Nostr relays.
- The game's **Internet** tab lists them.
- Players share `rlx1…` invite codes.
- A host proves its key to every joiner, so a code or a listed game cannot be impersonated.

**Architecture:**
- **`rl-discovery` (new, layer 5):**
  - the Nostr core (NIP-01 events signed with the install's existing BIP-340 key);
  - a relay pool over `wss://` (tungstenite with rustls);
  - announcements and the game list as sans-IO state machines;
  - a STUN binding client that learns the host's public address;
  - invite codes.
- **Joining:** it stays the plan 10 direct handshake, through the address the announcement or invite carries. Nostr handshakes, hole punching and relays are plan 12b.
- **Host-key proof:** the host signs the joiner's nonce, bound to both DTLS fingerprints, and the joiner refuses a host whose key differs from the one it expected (carried ruling F-B from plan 10).

**Tech Stack:** Rust 1.99 (edition 2024); existing crates (`tungstenite =0.30.0`, `k256 =0.14.0`, `sha2`, `hex`, `serde_json`, `getrandom =0.4.3`, `rcgen` as a dev-dependency, already in the lock). New exact pins, each released at least two weeks before 2026-10-13, checked with `cargo info`:
- `rustls` (0.23.x, default `aws_lc_rs` provider: aws-lc-sys already builds here and for mingw);
- `webpki-roots`;
- tungstenite's `rustls-tls-webpki-roots` feature;
- `bech32` (0.11.x).

**Spec:** `docs/superpowers/specs/2026-10-12-networking-design.md` (approved; plans 9-11 implemented). This plan covers part of spec §11's plan 12 row:
- §5.2 (the key signs announcements; the host's proof);
- §6.1 (Nostr announcements, relays, the browser list);
- §6.3 (invite codes, visibility);
- §7.1 (tabs Internet, LAN, Direct);
- §7.4 (server visibility and relays);
- §5.3's STUN list, used here only to learn the public address.

Not here:
- Nostr handshakes, port mapping, hole punching and TURN relays (plan 12b);
- host migration and `net-soak` (plan 12c);
- the web link `…/#rlx1…` (plan 13).

**Ruling PL12-1 (recorded in Task 11):** the spec's plan 12 row is split into 12a (finding games), 12b (through routers) and 12c (host migration and soak). Each one ends testable, and the row was too large for one plan.

## Global Constraints

- **Spec §6.1:**
  - **Announcement:** a signed, replaceable note per game with name, mode, map, mod (its content hash), players and spectators, maximums, simulation fingerprint, password flag, self-chosen region, handshake hints. Its expiry is 3 minutes ahead and it is re-published every minute.
  - **Relays:** a default list of about 8 relays from different operators, chosen by measured behaviour (accept the kinds, keep notes for minutes, no payment); editable in the options.
  - **Browser list:** hide notes with bad signatures or another fingerprint (another fingerprint shows greyed out, "needs version X", spec §2.2); rate-limit each key; let the user block a key; sink games that never answer; measure ping when a game is selected.
- **Event kinds (ruling PL12-2):**
  - announcements are kind `31470`, an addressable (parameterized replaceable) event with tag `d` = `"rust-liero"`, so one game per key;
  - kind `21470` (ephemeral) is reserved for plan 12b's handshakes.

  Both were checked against the NIP kinds table on 2026-10-13: 31400-31499 and 21400-21499 have no assignments. Task 2 re-checks the registry (https://github.com/nostr-protocol/registry-of-kinds) and records the result.
- **Spec §6.3:**
  - An invite code is `rlx1…`: bech32m with HRP `rlx`, checksummed.
  - It holds the host's key, relay hints and optionally a direct address.
  - A game is public (announced), LAN only or invite only, with or without a password.
- **Spec §5.2.** One key signs announcements and identifies the player: the existing `rl_lobby::Identity` (x-only BIP-340, exactly Nostr's `pubkey`/`sig`).
- **Never panic on network input** (spec §5.4).
  - Every relay frame, event, STUN message and invite code is size-limited and fuzzed with proptest (`cases: 256, failure_persistence: None`).
  - Each relay connection has limits: frame ≤ 128 KiB, ≤ 200 frames/s, ≤ 1 MiB/s.
- **Determinism and testability.**
  - Logic takes time (unix seconds for Nostr's `created_at`, `Instant` for timers) as an argument.
  - Sockets, TLS and threads live only in drivers.
  - `cargo xtask check` never touches the public internet. Every test uses the in-process test relay and STUN responder.
- **Layering.**
  - `rl-discovery` is layer 5 and depends on rl-proto and rl-lobby.
  - rl-game and rl-server (layer 6) wire it.
  - No new `same_layer` pair.
- **Outward-facing measurement.** Task 8's probe publishes to public relays. It uses a fresh random key, the content `"rust-liero relay probe"`, a 10-minute expiry and no personal data. It never publishes the user's identity, name or email.
- **Commits.** Run `cargo xtask check` before every commit; commit on `main`, never push. Commit trailers:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_019821i4KFSbhZ4mHYULrhEV
  ```

## Decisions this plan makes (recorded as STATUS.md N-rulings in Task 11)

- **N37 – the host proves its key.**
  - The joiner sends its own 32-byte nonce in `Hello`.
  - The host answers `ToClient::HostProof { signature }` over `b"rust-liero host v1" ++ client nonce ++ client key ++ offer DTLS fingerprint ++ answer DTLS fingerprint`.
  - The joiner's own proof (`challenge_message`) also covers both fingerprints.
  - A joiner with `expected_host = Some(k)` refuses a host whose key is not `k`, or whose proof fails ("this is not the game you were invited to").
  - Protocol stays 2 (unreleased).
- **N38 – one Nostr core, no Nostr library.**
  - `rl-discovery` implements the NIP-01 subset it needs: event serialization and id, BIP-340 signing through `Identity`, and the relay frames `EVENT`/`REQ`/`CLOSE`/`EOSE`/`OK`/`NOTICE`/`CLOSED`.
  - It also implements NIP-40 expiration.
  - This keeps the dependency tree and the WASM build (plan 13) small.
- **N39 – the announcement.**
  - The content is JSON (`serde_json`, at most 4 KiB): `{v, name, mode, map, map_name, mod, mod_hash, players, max_players, spectators, max_spectators, password, region, playing, addr, port}`.
  - Tags: `["d","rust-liero"]`, `["t","rust-liero"]`, `["f","<fingerprint 16 hex>"]`, `["expiration","<unix+180>"]`.
  - It is re-published every 60 s and when the lobby changes (at most every 10 s).
  - On close, the host publishes a note with `"closed": true` and an expiry 10 s ahead.
- **N40 – the public address.** A host learns its public IP from:
  1. the configured `public_address`, else
  2. a STUN binding request (RFC 5389) to the first answering server of the STUN list.

  The announcement's `addr` is that IP and its `port` is the host's port. Without either, a public game is announced without an address. The list shows it as "cannot be joined yet" (plan 12b adds handshakes) and its invite carries no address.
- **N41 – the game list.** It:
  - verifies every event (id and signature);
  - keeps the newest note per key;
  - drops expired notes and closed games;
  - shows games of another fingerprint greyed out;
  - accepts at most one note per key per 10 s and at most 500 games;
  - hides blocked keys.

  Ping is a TCP connect to `addr:port`, timed, with a 2 s timeout, when a game is selected. A failed ping sinks the game to the bottom for the session.
- **N42 – invite codes.**
  - The format is bech32m with HRP `rlx`. The payload is postcard: `{version: u8 = 1, key: [u8;32], relays: Vec<RelayHint>, addr: Option<(IpAddr, u16)>}`.
  - `RelayHint` is `Default(u8)`, an index into the default relay list (which is append-only), or `Url(String)` (at most 2, each at most 100 bytes).
  - Joining by code uses the address, and expects the code's key (N37).
- **N43 – visibility.** `Visibility { Public, Lan, Invite }`:
  - **Public:** announced on Nostr and broadcast on the LAN.
  - **Lan:** broadcast on the LAN only.
  - **Invite:** neither. The invite code is shown in the lobby and printed by the server.

  The plan 10 `DirectOnly` is read as `Invite` (serde alias).

## Review Focus

1. **A forged or hostile relay feed**:
   - events with bad ids or signatures, a valid signature over other content;
   - huge or deeply nested JSON, a flood from one key or from many keys, notes for another fingerprint;
   - expired notes, a relay that never sends EOSE, a relay that disconnects mid-frame.

   Expected: such notes never show up as joinable games, memory stays bounded, the list stays responsive, and nothing panics. Pinned by:
   - Task 2's fuzz tests;
   - Task 5's `a_hostile_feed_is_filtered_and_bounded`;
   - Task 4's `a_misbehaving_relay_is_dropped_and_retried`.
2. **An impostor host** (someone relays a code's address to their own server, or answers in another's name). Expected: the joiner refuses it with a clear message. Pinned by Task 1's `a_host_with_another_key_is_refused` and `swapped_dtls_fingerprints_fail_the_proof`.
3. **No public address** (STUN unreachable, no config). Expected: the game is still hosted, LAN and direct joins work, the list says the game cannot be joined yet, and the invite has no address and says so. Pinned by Task 6's `no_stun_answer_leaves_no_address` and Task 10's `a_public_game_without_an_address_is_listed_unjoinable`.
4. **Relays unreachable or all down** (offline play). Expected: hosting and LAN play are unaffected, the Internet tab says "no relay reachable", and nothing blocks the UI thread. Pinned by Task 4's `an_unreachable_relay_never_blocks_the_caller` and Task 10's `the_internet_tab_without_relays`.
5. **A mistyped or truncated invite code** (one wrong character, missing characters, a code with whitespace or upper case). Expected: a clear "this invite code is damaged" (the bech32m checksum), with whitespace trimmed and upper case accepted. Pinned by Task 7's `damaged_codes_are_refused_clearly`.

---

## File structure

| File | Responsibility |
|---|---|
| `crates/rl-net/src/{link.rs, endpoint.rs}` | DTLS fingerprints per peer (Task 1). |
| `crates/rl-lobby/src/{identity.rs, host.rs, client.rs}`, `crates/rl-proto/src/lobby.rs` | N37's proof (Task 1). |
| `crates/rl-discovery/` (new) | `nostr.rs` (events, frames), `relay.rs` (one connection's driver), `pool.rs` (sans-IO pool), `announce.rs`, `list.rs`, `stun.rs`, `invite.rs`, `test_relay.rs` (feature `test-relay`), `defaults.rs` (relay and STUN lists). |
| `crates/rl-cli/src/nostr_probe.rs` | Relay and STUN measurement (Task 8). |
| `crates/rl-server/src/{config.rs, lib.rs, admin.rs}` | Visibility, relays, region, public address, invite (Task 9). |
| `crates/rl-ui/src/{online.rs, screens/multiplayer.rs, screens/host.rs, screens/lobby.rs, screens/options.rs}`, `crates/rl-game/src/{net.rs, shell/online.rs}` | Tabs, list, invite, settings (Task 10). |

---

### Task 1: The host proves its key (carried ruling F-B)

**Files:**
- Modify:
  - `crates/rl-net/src/{link.rs, endpoint.rs}`;
  - `crates/rl-proto/src/lobby.rs` (`ToHost::Hello` gains `nonce: [u8; 32]`; `ToClient::HostProof { signature }` appended);
  - `crates/rl-lobby/src/{identity.rs, host.rs, client.rs}`;
  - `crates/rl-online/src/peer.rs` (pass the fingerprints and `expected_host`);
  - the fuzz samples;
  - `crates/rl-lobby/CLAUDE.md` (remove the "proves nothing yet" caveat).
- Test: rl-lobby unit tests, `crates/rl-online/tests/online.rs`.

**Interfaces:**
- Produces (Tasks 7, 10):
  ```rust
  // rl-net
  impl Link { pub fn dtls_fingerprints(&self) -> Option<([u8; 32], [u8; 32])>; } // (offer side's, answer side's) SHA-256 fingerprints from the SDPs
  impl Endpoint { pub fn dtls_fingerprints(&self, peer: PeerId) -> Option<([u8; 32], [u8; 32])>; }
  // rl-lobby
  pub fn challenge_message(nonce: &[u8; 32], host: &PublicKey, fps: &([u8; 32], [u8; 32])) -> Vec<u8>;
  pub fn host_proof_message(client_nonce: &[u8; 32], client: &PublicKey, fps: &([u8; 32], [u8; 32])) -> Vec<u8>; // b"rust-liero host v1" ++ ...
  // LobbyHost::connected(.., fingerprints) and LobbyClient::new(.., expected_host, fingerprints) take the fingerprints
  // LobbyClient: a HostProof that fails, or a host key != expected_host, ends with ClientEvent::Refused("this is not the game you were invited to")
  // rl-online: PeerRole::Client { addr, password, expected_host: Option<PublicKey> }
  ```

Context:
- **Where the fingerprints come from.** str0m puts each side's DTLS certificate fingerprint into its SDP (`a=fingerprint:sha-256 …`). `Link` keeps both from the offer and the answer it saw (parsing the SDP lines is fine), in offer-then-answer order, identical on both sides.
- **What the proofs stop.** A man in the middle who relays signalling must present its own certificate, so the fingerprints the two ends saw differ and both proofs fail.
- **Message order.** The host sends `HostProof` right after `Hello`, before `Welcome`. The client verifies it before sending `Prove`, and refuses otherwise.

- [ ] **Step 1: Write the failing tests.**
  - `a_host_with_another_key_is_refused` (online, real localhost: the client expects key A, the host has key B, giving the refusal text);
  - `swapped_dtls_fingerprints_fail_the_proof` (lobby unit: a client and host given different fingerprints both fail);
  - `the_host_proof_verifies` (lobby unit);
  - fuzz samples for the new messages.
- [ ] **Step 2: Run `cargo test -p rl-lobby -p rl-online`, expect FAIL.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run, expect PASS; then `cargo xtask check`.**
- [ ] **Step 5: Commit** `rl-lobby: the host proves its key, bound to the DTLS fingerprints`.

---

### Task 2: `rl-discovery`: the Nostr core

**Files:**
- Create:
  - `crates/rl-discovery/{Cargo.toml, CLAUDE.md, src/lib.rs, src/nostr.rs}`;
  - `crates/rl-discovery/tests/{integration.rs, nostr.rs}`.
- Modify: `Cargo.toml` (workspace member dependency), `xtask/layers.ron` (`"rl-discovery": 5`).
- Test: `tests/nostr.rs`.

**Interfaces:**
- Produces:
  ```rust
  pub const KIND_ANNOUNCE: u32 = 31470;
  pub const KIND_HANDSHAKE: u32 = 21470;   // plan 12b
  pub const MAX_FRAME: usize = 128 * 1024;
  pub const MAX_CONTENT: usize = 4 * 1024; // our own events' content
  #[derive(Clone, Debug, PartialEq, Eq)]
  pub struct Event { pub id: [u8; 32], pub pubkey: PublicKey, pub created_at: u64, pub kind: u32, pub tags: Vec<Vec<String>>, pub content: String, pub sig: Signature }
  impl Event {
      pub fn sign(identity: &Identity, created_at: u64, kind: u32, tags: Vec<Vec<String>>, content: String, aux: [u8; 32]) -> Event;
      pub fn verify(&self) -> bool;                 // id == sha256(canonical) && BIP-340 sig
      pub fn tag(&self, name: &str) -> Option<&str>; // first value of the first tag named `name`
      pub fn expiration(&self) -> Option<u64>;       // NIP-40
  }
  /// NIP-01 canonical serialization: [0,"<pubkey hex>",created_at,kind,tags,"content"], with exactly NIP-01's escaping.
  pub fn canonical(pubkey: &PublicKey, created_at: u64, kind: u32, tags: &[Vec<String>], content: &str) -> String;
  pub struct Filter { pub kinds: Vec<u32>, pub authors: Vec<PublicKey>, pub tags: Vec<(char, Vec<String>)>, pub since: Option<u64>, pub limit: Option<u32> }
  pub enum ToRelay { Event(Event), Req { sub: String, filters: Vec<Filter> }, Close { sub: String } }
  pub enum FromRelay { Event { sub: String, event: Event }, Eose { sub: String }, Ok { id: [u8; 32], accepted: bool, message: String }, Notice(String), Closed { sub: String, message: String } }
  pub fn encode_to_relay(m: &ToRelay) -> String;
  pub fn decode_from_relay(text: &str) -> Result<FromRelay, NostrError>; // ≤ MAX_FRAME, bounded nesting
  pub fn decode_to_relay(text: &str) -> Result<ToRelay, NostrError>;     // for the test relay
  ```

Context:
- **Canonical form.** NIP-01 escapes `\n`, `\"`, `\\`, `\r`, `\t`, `\b` and `\f`, and writes every other character verbatim. Write the canonical serializer by hand, without `serde_json`'s escaper, and test it against published NIP-01 vectors: a real event from any public relay, with its id and signature, embedded as a fixture.
- **Signing.** `Identity::sign(message, aux)` signs raw bytes, so pass the 32-byte id.
- **Parsing.** Relay frames are JSON arrays; parse them with `serde_json::Value` behind the size check.
- **Kinds.** Record in `CLAUDE.md` the registry check for 31470 and 21470. If either is taken, stop and report: the controller picks another.

- [ ] **Step 1: Write the failing tests:**
  - `a_published_event_verifies` (fixture);
  - `signing_round_trips`;
  - `canonical_escapes_like_nip01`;
  - `tampered_events_fail` (content, tag, created_at, sig);
  - `frames_round_trip`;
  - fuzz `arbitrary_frames_never_panic` and `mutated_frames_never_panic`.
- [ ] **Step 2: Run `cargo test -p rl-discovery`, expect FAIL.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run, expect PASS. Then `cargo xtask check`. Run `cargo xtask layers` too, so the new crate is registered.**
- [ ] **Step 5: Commit** `rl-discovery: the Nostr core`.

---

### Task 3: A test relay

**Files:**
- Create: `crates/rl-discovery/src/test_relay.rs` (feature `test-relay`), `crates/rl-discovery/tests/test_relay.rs`
- Test: `tests/test_relay.rs`

**Interfaces:**
- Produces (Tasks 4, 9, 10):
  ```rust
  pub struct TestRelay { .. }
  impl TestRelay {
      pub fn start() -> TestRelay;                 // ws://127.0.0.1:<port>, a thread, dropped = stopped
      pub fn start_tls() -> (TestRelay, Vec<u8> /* DER root cert */); // wss://localhost:<port>, rcgen self-signed
      pub fn url(&self) -> String;
      pub fn events(&self) -> Vec<Event>;          // stored, for assertions
      pub fn set_misbehaviour(&self, m: Misbehaviour); // NeverEose, Garbage, DropAfter(n), Slow(Duration)
  }
  ```

Context: NIP-01 relay semantics, as far as the plan needs them:
- store and verify events;
- addressable kinds replace by `(pubkey, kind, d)`;
- ephemeral kinds (20000-29999) are forwarded to live subscriptions and never stored;
- honour NIP-40 expiry on read;
- serve `REQ` with `kinds`, `authors`, `#d`/`#t`/`#f`/`#p` tags, `since` and `limit`, then `EOSE`;
- `OK` for every `EVENT`;
- `CLOSE`.

It is a test tool behind a feature. rl-game, rl-server and rl-cli tests enable it in their dev-dependencies, never in normal builds.

- [ ] **Step 1: Write the failing tests:**
  - `stores_replaces_and_expires`;
  - `ephemeral_events_reach_live_subscriptions_only`;
  - `filters_select_by_kind_author_and_tags`;
  - `tls_works_with_the_given_root`.
- [ ] **Step 2: Run, expect FAIL.** **Step 3: Implement.** **Step 4: Run, expect PASS; then `cargo xtask check`.**
- [ ] **Step 5: Commit** `rl-discovery: an in-process test relay`.

---

### Task 4: Relay connections and the pool

**Files:**
- Create: `crates/rl-discovery/src/{relay.rs, pool.rs}`, `crates/rl-discovery/tests/pool.rs`
- Modify: workspace `Cargo.toml` (`rustls`, `webpki-roots`, tungstenite's `rustls-tls-webpki-roots` feature: exact pins)
- Test: `tests/pool.rs`

**Interfaces:**
- Produces (Tasks 5, 9, 10):
  ```rust
  pub struct PoolConfig { pub relays: Vec<String>, pub extra_roots: Vec<Vec<u8>> /* tests */, pub max_frame: usize }
  pub struct Pool { .. }   // owns one driver thread per relay; never blocks the caller
  pub enum PoolEvent { Event { relay: usize, event: Event }, Eose { relay: usize, sub: String }, Ok { relay: usize, id: [u8; 32], accepted: bool, message: String }, Status { relay: usize, status: RelayStatus } }
  pub enum RelayStatus { Connecting, Connected, Failed(String), Dropped(String) }
  impl Pool {
      pub fn start(cfg: PoolConfig) -> Pool;
      pub fn publish(&mut self, event: Event);                       // to every connected relay; queued (bounded, newest wins per (kind, d))
      pub fn subscribe(&mut self, sub: &str, filters: Vec<Filter>);  // re-sent on reconnect
      pub fn unsubscribe(&mut self, sub: &str);
      pub fn poll(&mut self, out: &mut Vec<PoolEvent>);              // drains; dedupes events by id across relays
      pub fn statuses(&self) -> Vec<(String, RelayStatus)>;
  }
  ```

Context:
- **Connections.**
  - Each driver thread connects with `tungstenite::client_tls_with_config`, using a rustls `ClientConfig` with webpki roots plus `extra_roots`. Plain `ws://` is allowed (local relays and tests).
  - Connect and read timeouts are 5 s.
  - Reconnect backoff is 1, 2, 4 … 60 s.
- **Per-relay limits.** Frames above `max_frame`, more than 200 frames/s or more than 1 MiB/s drop the connection (`Dropped`) and back off.
- **Bounded queues.** At most 256 outgoing frames, then the oldest are dropped; at most 4096 incoming events between polls, then the newest are dropped and counted.
- **Shutdown.** Dropping the `Pool` closes every connection, and joins the threads with a 2 s bound.

- [ ] **Step 1: Write the failing tests** against `TestRelay`:
  - `publishes_and_receives_across_two_relays` (dedupe by id);
  - `resubscribes_after_a_reconnect`;
  - `a_misbehaving_relay_is_dropped_and_retried` (Garbage, NeverEose, oversized frame);
  - `an_unreachable_relay_never_blocks_the_caller` (a closed port and an unroutable address: `poll` returns at once, status `Failed`);
  - `wss_with_a_test_root`.
- [ ] **Step 2: Run, expect FAIL.** **Step 3: Implement.** **Step 4: Run, expect PASS. Then `cargo xtask check` and `cargo build --release --target x86_64-pc-windows-gnu -p rl-discovery` (rustls/aws-lc on mingw).**
- [ ] **Step 5: Commit** `rl-discovery: relay connections and the pool`.

---

### Task 5: Announcements and the game list

**Files:**
- Create: `crates/rl-discovery/src/{announce.rs, list.rs}`, `crates/rl-discovery/tests/{announce.rs, list.rs}`
- Test: the above

**Interfaces:**
- Produces (Tasks 9, 10):
  ```rust
  #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
  pub struct GameSummary { pub v: u16, pub name: String, pub mode: String, pub map: String, pub map_name: String, pub mod_id: String, pub mod_hash: String,
      pub players: u8, pub max_players: u8, pub spectators: u8, pub max_spectators: u8, pub password: bool, pub region: Region, pub playing: bool,
      pub addr: Option<IpAddr>, pub port: u16, #[serde(default)] pub closed: bool }
  #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)] pub enum Region { Europe, NorthAmericaEast, NorthAmericaWest, SouthAmerica, Asia, Oceania, Africa, Unknown }
  pub fn announcement(identity: &Identity, summary: &GameSummary, fingerprint: u64, now_unix: u64, aux: [u8; 32]) -> Event;
  pub fn parse_announcement(e: &Event) -> Result<(GameSummary, u64 /* fingerprint */), AnnounceError>;
  pub struct Announcer { .. }   // sans-IO: when to publish
  impl Announcer { pub fn new() -> Announcer; pub fn tick(&mut self, now: Instant, summary: &GameSummary) -> bool /* publish now */; pub fn closing(&mut self) -> bool; }
  pub struct GameList { .. }
  pub struct ListedGame { pub key: PublicKey, pub summary: GameSummary, pub compatible: bool, pub version_hint: String, pub ping: Ping, pub seen_unix: u64 }
  pub enum Ping { Unknown, Measuring, Ms(u32), NoAnswer }
  impl GameList {
      pub fn new(fingerprint: u64) -> GameList;
      pub fn offer(&mut self, e: &Event, now_unix: u64);      // verifies, rate-limits, replaces, bounds
      pub fn expire(&mut self, now_unix: u64);
      pub fn block(&mut self, key: PublicKey); pub fn blocked(&self) -> &[PublicKey];
      pub fn set_ping(&mut self, key: &PublicKey, ping: Ping);
      pub fn games(&self) -> Vec<ListedGame>;                   // compatible first, then by ping, NoAnswer last, then by name
      pub fn filter() -> Filter;                                // kind 31470, #t rust-liero, since now-180
  }
  ```

Context:
- **Announcement content.** It is validated: name ≤ 20 chars by `valid_name`, ids are valid pack ids, the counts are consistent, and the JSON is at most `MAX_CONTENT`.
- **When the announcer publishes:**
  - every 60 s;
  - at a change, at most every 10 s;
  - on close, one `closed: true` note.
- **The version hint** for an incompatible game is the summary's `v` (protocol) plus the fingerprint's first 6 hex digits ("needs version …").

- [ ] **Step 1: Write the failing tests:**
  - `announcements_round_trip`;
  - `the_announcer_republishes_every_minute_and_on_change`;
  - `a_closed_game_leaves_the_list`;
  - `expired_notes_leave_the_list`;
  - `other_fingerprints_are_greyed`;
  - `blocked_keys_are_hidden`;
  - `a_hostile_feed_is_filtered_and_bounded`: bad signatures, 10 000 keys, 100 notes per second from one key, a 4 KiB+ content, nested JSON. The list stays at most 500, and per-key accepted notes are at most 1 per 10 s.
  - `sorting_sinks_no_answer`.
- [ ] **Step 2: Run, expect FAIL.** **Step 3: Implement.** **Step 4: Run, expect PASS; then `cargo xtask check`.**
- [ ] **Step 5: Commit** `rl-discovery: game announcements and the game list`.

---

### Task 6: The public address through STUN

**Files:**
- Create: `crates/rl-discovery/src/stun.rs`, `crates/rl-discovery/tests/stun.rs`
- Test: `tests/stun.rs`

**Interfaces:**
- Produces (Tasks 8, 9, 10):
  ```rust
  pub fn binding_request(transaction: [u8; 12]) -> Vec<u8>;                         // RFC 5389, 20 bytes
  pub fn parse_binding_response(bytes: &[u8], transaction: &[u8; 12]) -> Option<SocketAddr>; // XOR-MAPPED-ADDRESS (or MAPPED-ADDRESS); bounded, never panics
  /// Asks each server in turn (500 ms each, 2 tries), from an ephemeral UDP socket; runs on a thread; returns the first answer.
  pub fn public_ip(servers: &[String], deadline: Duration) -> Option<IpAddr>;
  pub struct StunResponder { .. } // tests: answers binding requests on 127.0.0.1 with a configured mapped address
  ```

Context: N40. The host's game socket is not used for STUN in 12a, because the IP is all it needs. Plan 12b's ICE uses the game socket for server-reflexive candidates. IPv4 only, matching plan 10's IPv4 rule.

- [ ] **Step 1: Write the failing tests:**
  - `parses_a_real_response` (an RFC 5769 test vector);
  - `refuses_wrong_transactions_and_garbage`;
  - fuzz `responses_never_panic`;
  - `asks_the_next_server_when_one_is_silent` (`StunResponder` plus a silent port);
  - `no_stun_answer_leaves_no_address`.
- [ ] **Step 2: Run, expect FAIL.** **Step 3: Implement.** **Step 4: Run, expect PASS; then `cargo xtask check`.**
- [ ] **Step 5: Commit** `rl-discovery: learn the public address through STUN`.

---

### Task 7: Invite codes

**Files:**
- Create: `crates/rl-discovery/src/{invite.rs, defaults.rs}`, `crates/rl-discovery/tests/invite.rs`
- Modify: workspace `Cargo.toml` (`bech32` exact pin)
- Test: `tests/invite.rs`

**Interfaces:**
- Produces (Tasks 9, 10):
  ```rust
  pub const DEFAULT_RELAYS: &[&str];   // append-only (indices are wire format); filled by Task 8
  pub const DEFAULT_STUN: &[&str];     // filled by Task 8
  #[derive(Clone, Debug, PartialEq, Eq)] pub struct Invite { pub key: PublicKey, pub relays: Vec<RelayHint>, pub addr: Option<SocketAddr> }
  #[derive(Clone, Debug, PartialEq, Eq)] pub enum RelayHint { Default(u8), Url(String) }
  impl Invite { pub fn encode(&self) -> String; pub fn decode(text: &str) -> Result<Invite, InviteError>; pub fn relay_urls(&self) -> Vec<String>; }
  #[derive(Debug, thiserror::Error)] pub enum InviteError { #[error("this is not an invite code")] NotInvite, #[error("this invite code is damaged")] Damaged, #[error("this invite code is from a newer version of the game")] Newer, #[error("this invite code is invalid")] Invalid }
  ```

Context: N42.
- **Decode:**
  - trims whitespace;
  - accepts all upper case (bech32 allows it), but not mixed case;
  - allows a length of up to 200 characters (bech32m's 90-character limit is lifted with the crate's `decode` options; document the choice).
- **Relay hints:** at most 2 `Url` hints, each `wss://` or `ws://`, at most 100 bytes. `Default` indices must be in range.
- **The address** is IPv4 only (plan 10's IPv4 rule).
- **Placeholders.** `defaults.rs` starts with the candidate lists below, and Task 8 replaces them with the measured choice.
  - Relays: `wss://relay.damus.io`, `wss://nos.lol`, `wss://relay.primal.net`, `wss://relay.nostr.band`, `wss://nostr.mom`, `wss://relay.snort.social`, `wss://offchain.pub`, `wss://nostr.oxtr.dev`.
  - STUN: `stun.l.google.com:19302`, `stun.cloudflare.com:3478`, `global.stun.twilio.com:3478`, `stun.nextcloud.com:443`, `stun.stunprotocol.org:3478`.

- [ ] **Step 1: Write the failing tests:**
  - `invites_round_trip` (with and without an address, with default and URL hints);
  - `damaged_codes_are_refused_clearly` (each single-character change is `Damaged`; truncation; whitespace trimmed; upper case accepted; mixed case refused);
  - `newer_versions_are_named`;
  - fuzz `arbitrary_text_never_panics`.
- [ ] **Step 2: Run, expect FAIL.** **Step 3: Implement.** **Step 4: Run, expect PASS; then `cargo xtask check`.**
- [ ] **Step 5: Commit** `rl-discovery: invite codes`.

---

### Task 8: Measure relays and STUN servers, choose the defaults

**Files:**
- Create: `crates/rl-cli/src/nostr_probe.rs` (subcommands `rl-cli nostr-probe [urls…] [--wait-min N]` and `rl-cli stun-probe [servers…]`)
- Modify: `crates/rl-discovery/src/defaults.rs`, `docs/porting/STATUS.md` (a measurement table in Task 11's N-ruling text; keep the raw output in the report)
- Test: unit tests of the probe's verdict logic against `TestRelay`, which can simulate dropping notes, refusing the kind, and requiring payment (`OK false "restricted: …"`)

Context:
- **Per relay, the probe:**
  1. connects (wss);
  2. publishes a kind-31470 test note from a fresh random key, with content `"rust-liero relay probe"`, tags `d`, `t` = `"rust-liero-probe"` (not `rust-liero`, so real game lists ignore it) and an expiration 10 minutes ahead;
  3. records `OK`;
  4. reads it back at once through a `#t` filter on a second connection;
  5. reads it back again after `--wait-min` minutes (default 3);
  6. publishes and receives a kind-21470 ephemeral event between two connections (the plan 12b need).
- **Per STUN server:** answer or not, the RTT, and the mapped address family.
- **Verdicts:** accepts announcements, keeps them at least 3 minutes, forwards ephemeral events, and needs no payment or auth.
- **Run it on the candidate lists** (the placeholders, plus any others found with a quick web search for "popular free nostr relays").
- **Choose the defaults:**
  - **Relays:** 8 that pass, from at least 6 different operators.
  - **STUN:** 5 that pass, from at least 4 operators.
  - If fewer pass, take what passes and record it.
- **Records:** commit the measured table in the report and in STATUS N38's text. This step is the only one that touches the public internet, and it runs once by hand, never in `check`.

- [ ] **Step 1: Write the failing verdict tests** (`TestRelay` misbehaviours give the right verdicts).
- [ ] **Step 2: Run, expect FAIL.** **Step 3: Implement.** **Step 4: Run, expect PASS. Run the probes for real, then fill `defaults.rs`.**
- [ ] **Step 5: Commit** `rl-cli: probe Nostr relays and STUN servers; choose the defaults`.

---

### Task 9: The dedicated server announces itself

**Files:**
- Modify: `crates/rl-server/src/{config.rs, lib.rs, admin.rs}`, `fixtures/server/example.ron`, `crates/rl-server/tests/server.rs`
- Test: `crates/rl-server/tests/server.rs`

Context:
- **Config.**
  - `Visibility { Public, Lan, Invite }`, with `#[serde(alias = "DirectOnly")]` on `Invite`. The default becomes `Public` for the server.
  - New fields: `relays: Vec<String>` (default `DEFAULT_RELAYS`), `region: Region` (default `Unknown`), `public_address: Option<String>` (an IP), and `stun: Vec<String>` (default `DEFAULT_STUN`).
- **Startup.** At startup the server learns its address (N40) and logs it, or logs that it has none.
  - **Public:** it starts a `Pool`, announces every minute through an `Announcer` with a `GameSummary` built from its lobby (the same counting as the LAN announcement; share one builder in `rl-discovery` or in rl-online, not two copies), and broadcasts on the LAN.
  - **Shutdown:** a closed note.
- **Invite code.** It always prints its invite code (key, relay hints, address) at startup. The admin chat command `/invite` answers with it.
- **Tests (with `TestRelay`):**
  - `a_public_server_is_listed` (a `GameList` subscribed to the test relay sees it, with the right summary);
  - `the_invite_joins_the_server` (decode the printed code, join through `PeerRole::Client { expected_host: Some(key) }`);
  - `an_invite_server_is_not_announced`;
  - `an_old_config_still_loads` (`DirectOnly`, no new fields).

- [ ] Steps: failing tests → FAIL → implement → PASS → `cargo xtask check` → commit `rl-server: announce on Nostr, print the invite`.

---

### Task 10: The game: Internet, LAN and Direct tabs, invites, settings

**Files:**
- Modify:
  - `crates/rl-ui/src/{online.rs, screens/multiplayer.rs, screens/host.rs, screens/lobby.rs, screens/options.rs, store.rs}`;
  - `crates/rl-game/src/{net.rs, shell/online.rs}`;
  - kittest tests in `crates/rl-ui/tests/`;
  - `crates/rl-game/tests/`.

Context (spec §7.1, N41, N43):
- **Multiplayer tabs.**
  - **Internet:** the `GameList` as a table with columns name, map, mod, players, ping, region and a lock for a password. Selecting a game measures its ping on a thread. Join, and "Block this host". Incompatible games are greyed out with "needs version …", and an addressless game reads "cannot be joined yet".
  - **LAN:** today's list.
  - **Direct:** one field for "address or invite code", plus a password field. Text starting `rlx1` (any case) is decoded as an invite.
  - The Internet tab says "no relay reachable" when no relay is connected, and "searching…" until the first EOSE.
- **Host screen.** "Who can join": Anyone (Internet), This network, Invite only, plus the password.
- **Lobby.** It shows the invite code with a Copy button (egui's clipboard). For a Public game, a line says "Listed on the Internet" or "Not listed: no public address (forward port N or set it in Options)".
- **Options, Online tab.** Region; relays (an editable list with reset to defaults); STUN servers (the same); blocked hosts (unblock).
- **Threads.** The game owns one `Pool` while the Multiplayer screen or a hosted game is open, and closes it otherwise. The `Pool`, the pings and STUN never block the UI thread.
- **Tests:**
  - kittest `tabs_and_direct_invite`;
  - `the_internet_tab_without_relays`;
  - `a_public_game_without_an_address_is_listed_unjoinable`;
  - `options_edit_relays_and_unblock`;
  - an rl-game end-to-end test `a_listed_server_is_joined_from_the_internet_tab`: rl-server announcing to a `TestRelay`, the game's pool pointed at it, select, join, lobby.

- [ ] Steps: failing tests → FAIL → implement → PASS → `cargo xtask check` (bless UI snapshots only if a HUD snapshot changes, with a reason) → commit `rl-game: find games on the Internet tab, join by invite`.

---

### Task 11: Records

**Files:** `docs/porting/STATUS.md` (N37-N43, PL12-1, PL12-2, the measurement table), `CHANGELOG.md`, `docs/online.md` ("Finding games": the Internet tab, invite codes, visibility, relays and STUN in Options, running your own relay, port forwarding still needed until plan 12b), `crates/rl-discovery/CLAUDE.md`, the spec's status line (plan 12a implemented).

- [ ] Write; `cargo xtask check`; commit `docs: plan 12a's records`.
