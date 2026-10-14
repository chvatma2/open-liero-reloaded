# Plan 12c: Host Migration and Net-Soak Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When the host of an online match crashes, leaves or freezes, the match goes on under the next player within seconds, bit-exact on every peer; if nobody can take over, every peer lands on a "Host lost" screen with its replay saved. A long soak run (`cargo xtask net-soak`) hunts migration and netcode bugs overnight.
- The host keeps a **succession order** and sends it to everyone (players in join order, then spectators).
- Every client keeps a **standby endpoint** (a host endpoint, closed until it takes over) so it can be reached when its turn comes.
- When the host is silent for 2 s (or says goodbye), peers stop advancing and join the first live successor, directly or through the Nostr relays.
- The successor collects every peer's final inputs, takes the longest, sends each peer what it lacks, restarts the bots and resumes.
- A host whose own game stalled long enough for its peers to give up on it steps down, so a match never splits in two.
- `rl-cli net-soak` and `cargo xtask net-soak [--minutes N]` run seeded net-sim cases (profiles changing mid-run, outages, host crashes, departures and freezes, spectators arriving) and real-transport cases until the time is up.

**Architecture:**
- **`rl-proto`**: the migration wire: `Control::{Collect, Report, Resume}`, `ToHost::Standby`, `ToClient::Succession`, `Refusal::{NotHost, Closed}`.
- **`rl-netcode`** (sans-IO, `no_std`; spec §2: "host migration" lives here):
  - the session's side: host silence, suspension, `take_over`, `rejoined`, the reports' merge, `resume`, `rehost`;
  - `migration.rs`: the timing constants and the `Migration` state machine that says, at any instant, which successor a peer tries, whether it hosts, or whether it gives up;
  - the harness plays host exits, freezes and migrations in virtual time, so every migration rule is a seeded test in `check` (spec §9.1, U11).
- **`rl-lobby`**: the succession list (from the members' `Standby` reports), and the **migrated lobby** a successor builds from the old lobby's state: it takes back the match's members by key and nobody else.
- **`rl-online`** drives it over real connections: the standby endpoint, the joins to successors (direct, then Nostr), the promotion to host (lobby, listener, content side, bots), the host-lost replay and the stalled host's step-down.
- **`rl-cli`**: net-sim migration cases (the real simulation), the real-transport migration matches, `net-client`'s migration, and `net-soak`.
- **`rl-game`/`rl-ui`**: the in-match migration lines, "You are now hosting", the new host's lobby, the Host lost screen.
- **`xtask`**: `cargo xtask net-soak`; a `migration` scenario in the Docker NAT tests.

**Tech Stack:** Rust 1.99 (edition 2024); existing crates only (str0m `=0.24.0`, tungstenite, k256, postcard, serde, proptest, egui_kittest). **No new dependency** and no new system package.

**Spec:** `docs/superpowers/specs/2026-10-12-networking-design.md` (approved; plans 9-11, 12a and 12b implemented). This plan covers spec §11's plan 12 row as split by ruling PL12-1 into its last part, 12c:
- §3.7 (host migration), with §4's match flow around it;
- §9.1's events "the host leaving" and invariant "migration keeps the match";
- §9.10's `cargo xtask net-soak [--minutes N]`.

Not here: the browser (plan 13).

## Global Constraints

- **Spec §3.7, verbatim:**
  - "The host keeps a **succession order** (the order peers joined; players before spectators) and sends it to everyone."
  - "When the host is silent for 2 s, peers stop advancing past their confirmed tick and connect to the first live successor through its handshake path (§5.1); the successor becomes the host."
  - "The new host collects from every peer its furthest confirmed final inputs (identical where they overlap, since they came from one source), takes the longest, sends each peer what it lacks, and resumes. Bots move to the new host and restart their thinking."
  - "Target: the match continues within 3 s. If no successor is reachable within 10 s, peers return to a local "host lost" screen with the replay saved."
  - "A dedicated server that crashes is migrated away from in the same way."
- **Spec §3.2:** "What the host forwards for (player, tick) is final: it is the single source of truth." After a migration the new host is that source; nothing it confirms may contradict a value any peer confirmed before.
- **Spec §9.1:** "once the network settles, every peer's confirmed checksums are identical and equal to an offline replay of the final inputs"; invariants "no two peers confirm different final inputs" and "migration keeps the match". "`check` runs a few hundred seeds (seconds); `full` thousands." A failure writes `target/failures/net-<seed>.ron`, replayed by `rl-cli net-sim --from <file>`.
- **Spec §9.10:** "`cargo xtask net-soak [--minutes N]`: long runs of §9.1 and §9.4 with changing profiles, disconnects and host departures, for overnight runs."
- **Determinism:** the simulation is untouched (match goldens, oracle and render goldens do not move). Every peer's confirmed inputs and checksums agree across a migration, and the old host's confirmed inputs are a prefix of the new host's. `BTreeMap`/`BTreeSet` only in anything that orders output.
- **Nothing in `cargo xtask check` or any test touches the public internet or the machine's real router (ruling NT-1: tests send only to 127.0.0.0/8).**
  - Standby endpoints never map a port (`IceConfig::port_mapping` is forced off for them), and every test's standby has `stun: []`.
  - Tests use 127.0.0.1 successors, in-process `TestRelay`s and the netcode's virtual network. The Docker NAT tests (`cargo xtask net-nat`) belong to `full` and stay inside their `--internal` networks.
  - Labels such as `10.0.0.n` or `93.184.216.34` appear only in sans-IO tests that send nothing.
- **Personal data never leaves:** no email, name or other personal detail in any message, test event or log beyond what the lobby already shows. A member's LAN address and standby port reach only the other members of its match, and only when the host saw it on a private network (N64).
- **Layers (`xtask/layers.ron`) unchanged:** no new crate and no new pair. `rl-proto`, `rl-netcode` and `rl-lobby` stay `#![no_std]` + `alloc` and build for `wasm32-unknown-unknown`. One integration-test binary per crate (`autotests = false`): new test files are `mod`s of `tests/integration.rs`; rl-lobby's single `tests/lobby.rs` takes its new tests in place.
- **Dependencies:** none added. The Docker step needs Docker only (`sudo apt install docker.io`, then `sudo usermod -aG docker $USER`). `cargo xtask api` summarises workspace crates: read `target/api/<crate>.txt`, never guess a signature.
- **Sans-IO first:** every migration rule (timing, successor choice, the merge) is in `rl-netcode` and takes time as an argument. Sockets, threads and relays stay in rl-net, rl-discovery and rl-online's drivers. **Nothing blocks the game's UI thread:** endpoints, joins and listeners start without waiting; old endpoints shut down on threads (ruling NT-2's pattern).
- **Bounded state:** every list a peer receives is capped on the wire (succession ≤ `MAX_MEMBERS`, reports ≤ `MAX_REPORT_FRAMES`); every retry has a period and a deadline; at most `MAX_MIGRATIONS` per match.
- **Protocol stays 2:** new messages and refusals are appended variants. The simulation fingerprint changes with rl-netcode's and rl-proto's sources, as always.
- **Never panic on network input** (spec §5.4): every new decoder and handler is fuzzed with proptest (`cases: 256, failure_persistence: None`).
- **Commits:** run `cargo xtask check` before every commit (`--offline` without network). Run `cargo xtask e2e` in Task 8 and `cargo xtask net-nat --only migration` in Task 9, the tasks that touch those areas. **`cargo xtask full` runs once, at the end of the plan (Task 11), never mid-plan (user rule, 2026-10-15).** Commit on `main`, never push. Trailers:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_019821i4KFSbhZ4mHYULrhEV
  ```

## Decisions this plan makes (recorded as STATUS.md rulings in Task 11)

The spec leaves these open; each is decided here.

- **N63 – the standby endpoint.** A successor must be reachable when its turn comes, but a client has no listening socket.
  - Every client (player or spectator) opens a **standby endpoint** when it is welcomed: `Endpoint::host` on port 0 (TCP and UDP), port mapping forced off, its STUN servers as configured (so its server-reflexive candidate is ready). Its gate refuses every offer with `Refusal::NotHost` until this peer takes over.
  - It reports `ToHost::Standby { port, relays }` after each `Welcome` (relays: its first 2 Nostr relays, the ones it listens on once it hosts).
  - A client whose standby cannot bind (or that has none: `MigrationConfig::standby = None`) cannot host, but still follows a migration (N79).
  - **Cost if wrong:** one more listening socket per client while it is online.
- **N64 – the succession list.**
  - **Order:** the match's players in join order, then its spectators in join order, then members waiting for the next match (followers only).
  - **`can_host`:** a `Standby` arrived and the member holds a world (a player, or a spectator from the start).
  - **Direct address:** only when the host saw the member at a non-global IPv4 address (`!rl_proto::is_global`: the same LAN), with the standby port, and only for ports ≥ 1024. Anyone else is reached through Nostr.
  - **Relays:** at most 2 per entry, each a `ws://` or `wss://` URL of at most 128 bytes.
  - **When:** `ToClient::Succession` goes out with the match's `Start` and whenever it changes during a match, coalesced like `Lobby`.
  - **Cost if wrong:** a successor on another network is never tried directly (its join takes the relays' few seconds).
- **N65 – timing.**
  - `HOST_SILENT_US` = 2 s (spec): nothing from the host for 2 s while the peer itself kept advancing.
  - Each `can_host` successor has a **window** of `WINDOW_US` = 5 s, in order, from the moment the migration began.
  - `GIVE_UP_US` = 10 s after the migration began: host lost. The spec's "within 10 s" counts from the start of the migration, so a host is lost at most 12 s after its last message.
  - A `NotHost` refusal is tried again after `RETRY_US` (250 ms) within the window; a successor that answered as a host (**contact**) is kept past its window until `GIVE_UP_US`.
  - "Peers stop advancing" means they are **suspended where they are** (at most `MAX_PREDICTION` past their confirmed tick): no tick runs and nothing is rolled back until the resume, and the last predicted frame stays on screen.
  - **Cost if wrong:** only two successors fit in 10 s; when both die with the host, the third is never tried.
- **N66 – "the match continues within 3 s".** Read as: from the moment the peers find the host gone to the new host's resume, on LAN, good and typical links, when every other peer survives. It is gated in the harness (≤ 3 s), and in rl-online's loopback test (≤ 3.5 s, real clocks). A departed peer the new host still waits for costs up to its window (5 s). **Cost if wrong:** a stricter reading (3 s from the host's last message) cannot be met with the spec's own 2 s detection on anything but LAN.
- **N67 – the merge.**
  - **Final inputs:** the new host takes the union, per slot, of every report's final inputs; all came from the old host, so they cannot differ. A value that does differ is ignored and counted (`Stats::report_conflicts`); its reporter is repaired by the desync path (spec §3.5).
  - **Provisional inputs:** a reporter's own provisional inputs are taken only where they extend its slot's known prefix, after every report's finals.
  - **Nothing is filled before the reports are in:** filling at the take-over could contradict a final value a later report carries. At the resume:
    - each bot slot's missing ticks below `R` get the slot's previous final value (the lagging rule's fill, what every peer predicted);
    - the new host's own slot gets the keys it pressed;
    - **abandoned slots** (the old host's own player, players not back by the resume) get `Keys(0)` from their first unknown tick (decision N6).
  - **The resume tick** `R` is the new host's current tick. Peers restart their pace at `max(their current tick, R)`: the pause is not caught up.
  - **Report size:** at most `MAX_REPORT_FRAMES` (3000 ticks) and `MAX_CONTROL`; a longer tail is cut. A cut value the new host decides otherwise is repaired by the desync path.
  - **Cost if wrong:** a peer with a cut report shows a correction (a repair) after the resume.
- **N68 – collection.**
  - The successor **commits** (drops its old host and becomes the host) at the first proven rejoin, or at once when it expects nobody.
  - It **resumes** when every expected peer (the match's players and spectators but itself) reported, or at the end of its window.
  - A successor that nobody rejoined by `GIVE_UP_US`, while it expected someone, gives up (host lost).
  - **Cost if wrong:** a 1-vs-host match continues for the lone survivor against bots (spec: "the match continues").
- **N69 – the old host speaks again.** A peer that has not committed returns to its old host (`HostHeard`), unless its attempt has contact with a hosting successor; a successor that has not committed closes its standby again. **Cost if wrong:** a peer whose own link blipped for 2-5 s rejoins its host instead of the match moving; a peer in contact with a successor follows it.
- **N70 – a stalled host steps down.** A host whose own polls stop for `STEP_DOWN_US` (1.5 s, less than `HOST_SILENT_US`, so jitter cannot make its peers think it silent while it thinks itself fine) leaves the match: `Bye` to every peer, so they migrate at once. This applies when its succession has a `can_host` entry.
  - The gap is measured on both the monotonic and the wall clock (a suspended machine's monotonic clock stands still on Linux).
  - A game host shows the Host lost screen ("Your game stopped for N s…"). A dedicated server logs it and opens its lobby again.
  - It does not rejoin the match.
  - **Cost if wrong:** a host whose window was dragged for 1.5 s (Windows) loses that match.
- **N71 – migration happens only during a match** (from `Start` until the host's `Ended`). A host lost in the lobby or during the results ends the game as before ("the connection to the host was lost"). **Cost if wrong:** a group whose host quits between matches must host again.
- **N72 – the migrated game is closed.**
  - It takes back the succession's members by key, with their member ids, seats and admin marks, and nobody else (`Refusal::Closed`).
  - It has no password, is not announced (Nostr or LAN) and maps no port.
  - A member rejoining after the resume is welcomed as a mid-match joiner: waiting, then watching from a state, then playing the next match. Its slot stays `Keys(0)` for the rest of the match.
  - **Cost if wrong:** new players need a fresh hosted game.
- **N73 – the joins to a successor.**
  - **Direct first:** when the entry has a direct address, with `MIGRATION_CONNECT_TIMEOUT` (1 s). A failed direct connect is not tried again within the window.
  - **Then through Nostr:** the entry's relays first, then the peer's own, cut to 8 (`join_relays`), with the successor's key expected (N37's proofs).
  - **The successor** listens on its own relays from the moment it opens its standby; one without relays (a LAN game) listens on none.
  - **Cost if wrong:** up to 1 s more on a stale direct address.
- **N74 – repeated migrations.** Former hosts are skipped in a later migration. At most `MAX_MIGRATIONS` (4) per match; past that, host lost. **Cost if wrong:** a fifth migration in one match ends it.
- **N75 – bots restart** at the new host: fresh `Bot`s for the bot slots of the `MatchStartWire`, on the world as it stands, with a new (empty, lazy) area cache. Only the host thinks for bots, so this changes nobody's simulation. **Cost if wrong:** bots forget their routes once per migration.
- **N76 – the host's `Bye`, or its link closing, during a match starts the migration at once** (no 2 s wait). **Cost if wrong:** none known.
- **N77 – net-soak.**
  - **Command:** `cargo xtask net-soak [--minutes N] [--seed S]` (default 10 minutes) runs `rl-cli net-soak` in release.
  - **Cases:** seeded random net-sim cases on the real simulation: both mods, the 4 bundled levels, 2-16 worms, 1-7 clients, 0-2 spectators, 0-3 profile changes, 0-2 outages, a host crash, departure or freeze in half of them, a spectator arriving. Every 5th run is a real-transport case on 127.0.0.1 with a server freeze or departure.
  - **Failures:** to `target/failures/` (net-sim cases as `NetCase`s, replayable with `rl-cli net-sim --from`), at most 32, then it stops.
  - **Not in `full`.** `full` gains the migration cases of `net_sim`'s and `transport`'s full tiers (about 2 minutes, measured in Task 11).
  - **Cost if wrong:** soak-only bugs surface overnight, not in `full`.
- **N78 – protocol stays 2** (appended variants; plan 12b's reasoning: unreleased online play). **No new dependency.**
- **N79 – a peer that cannot host** (no standby) follows a migration but is never a successor.
- **N80 – privacy of the succession.** A member's LAN address and standby port reach only the other members, and only when the host saw it on a private network (N64). A successor's Nostr listener answers with its own identity key, as any host's does.
- **N81 – what the players see.**
  - **During a migration:** the HUD's top line reads "The host is gone: moving the game to <name>…" (or "…: you are taking over the game…").
  - **After it:** the log says "<name> is now hosting" (or "You are now hosting"); the new host's lobby shows host controls and "Taken over from the host: only this match's players can join".
  - **Host lost:** a "Host lost" screen with the reason and the Replays and Back buttons; the replay is the peer's confirmed inputs, saved like any online match's.

**Plan rulings:**
- **PL12c-1:** 11 tasks. The wire (Task 1) and the sans-IO netcode with its harness (Tasks 2-3) come first, so every rule has a seeded test before any socket is involved.
- **PL12c-2:** spec §9.7's scenarios gain `migration` (Task 9), the only test of a successor reached across routers.

## Review Focus

1. **A host that freezes for 1-3 s:** a dragged window on Windows, a laptop lid, a Wi-Fi roam, a loaded VPS.
   - **Expected:** the match never splits. Below `STEP_DOWN_US` nothing happens. Past it the host steps down, its peers migrate, and its player is told why.
   - **Pinned by:**
     - Task 3's `a_host_frozen_past_the_step_down_threshold_hands_over` and `a_host_frozen_briefly_keeps_hosting`;
     - Task 5's `a_stalled_host_steps_down_and_the_match_moves`.
2. **One client's, or the successor's, own link blips for 2-5 s while the host is fine.**
   - **Expected:** the blipped peer returns to its old host (`HostHeard`). The successor never takes the match alone, and nobody else moves.
   - **Pinned by** Task 3's `a_client_outage_does_not_move_the_match` and `a_successor_outage_does_not_take_the_match`, and Task 5's `a_successors_own_outage_does_not_take_the_match` (real links).
3. **A hostile or broken host or peer abusing the migration wire:**
   - a succession naming LAN victims, privileged ports or 48 junk relays;
   - a report with conflicting, gapped or oversize frames;
   - `Collect`/`Resume` from a peer that is not the host;
   - `Standby` floods.

   - **Expected:** no panic and bounded memory. At most one direct connect per successor window, to ports ≥ 1024. Conflicts are ignored and counted; their reporter is repaired.
   - **Pinned by:**
     - Task 1's fuzzers;
     - Task 2's `a_conflicting_report_is_ignored_and_counted`, `migration_messages_from_a_non_host_are_ignored` and `migration_controls_never_panic`;
     - Task 4's `standby_reports_are_bounded`;
     - Task 6's `a_hostile_succession_is_bounded`.
4. **The successor is dead too, or unreachable** (crashed with the host, or behind a symmetric NAT with no TURN).
   - **Expected:** the next successor's window, then host lost 10 s after the migration began, with the replay saved; nobody waits for ever.
   - **Pinned by:**
     - Task 3's `the_second_successor_takes_over_when_the_first_is_dead` and `nobody_reachable_is_host_lost_after_ten_seconds`;
     - Task 5's `nobody_to_take_over_is_host_lost_with_a_replay`;
     - Task 8's `the_host_lost_screen_shows_and_the_replay_is_saved`.
5. **A late peer** (slow relays, an outage during the collection), **or a second migration in the same match.**
   - **Expected:** the late peer watches from a state and plays the next match; a second migration skips the former host; checksums agree throughout.
   - **Pinned by:**
     - Task 3's `a_late_peer_watches_from_a_state` and `a_second_migration_skips_the_former_host`;
     - Task 6's `a_late_rejoiner_watches_from_a_state` and `a_second_migration_moves_the_match_again`.

---

## File structure

| File | Responsibility |
|---|---|
| `crates/rl-proto/src/{lib.rs, lobby.rs, signal.rs}` | `Control::{Collect, Report, Resume}`, `InputsReport`, `ToHost::Standby`, `ToClient::Succession`, `SuccessorWire`, `Refusal::{NotHost, Closed}`, their bounds (Task 1). |
| `crates/rl-netcode/src/session.rs`, `src/session/migrate.rs` (new) | Host silence, suspension, take-over, rejoins, the merge, resume, rehost (Task 2). |
| `crates/rl-netcode/src/migration.rs` (new), `src/harness.rs`, `src/toy.rs` | The timing constants and the `Migration` state machine; the harness's exits, freezes, migrations and profile changes (Task 3). |
| `crates/rl-lobby/src/{host.rs, client.rs, model.rs, harness.rs}`, `src/host/migrated.rs` (new) | Standby records, the succession list, the migrated lobby (Task 4). |
| `crates/rl-net/src/{endpoint.rs, core.rs}` | `EndpointConfig::connect_timeout`, `NetEvent::Answered` (Task 5). |
| `crates/rl-online/src/{migration.rs (new), peer.rs, peer/migrate.rs (new), matches.rs, events.rs, join.rs}` | `MigrationConfig`, the standby, the joins to successors, the promotion, host lost, step-down (Tasks 5, 6). |
| `crates/rl-cli/src/{netsim.rs, net_sim_cmd.rs, transport.rs, net_client_cmd.rs, soak.rs (new), main.rs}`, `crates/rl-server/src/lib.rs` | net-sim migration, real-transport migration, `net-client`, `net-soak`, the server's log lines (Tasks 7, 10). |
| `crates/rl-ui/src/{state.rs, screens/host_lost.rs (new), screens/mod.rs, online.rs}`, `crates/rl-game/src/{shell/online.rs, net.rs, overlay.rs}` | The migration lines, the new host's lobby, the Host lost screen (Task 8). |
| `xtask/src/{net.rs, main.rs, nat.rs}`, `tools/net-nat/CLAUDE.md` | `cargo xtask net-soak`, the `migration` NAT scenario (Tasks 9, 10). |

---

### Task 1: `rl-proto`: the migration wire

**Files:**
- Modify: `crates/rl-proto/src/lib.rs` (the `Control` variants, `InputsReport`, `MAX_REPORT_FRAMES`, `MAX_REPORT_OWN`, their validation in `decode_control`), `crates/rl-proto/src/lobby.rs` (`ToHost::Standby`, `ToClient::Succession`, `SuccessorWire`, `valid_relay_url`, `valid_succession`, their bounds), `crates/rl-proto/src/signal.rs` (`Refusal::NotHost`, `Refusal::Closed`), `crates/rl-proto/tests/{fuzz.rs, fuzz_lobby.rs}`, `crates/rl-proto/CLAUDE.md`
- Create: `crates/rl-proto/tests/migration.rs` (a `mod migration;` of `tests/integration.rs`)

**Interfaces:**
- Consumes: `Frame`, `encode_frames`/`decode_frames`, `PublicKey`, `MAX_CONTROL`, `MAX_LOBBY`, `MAX_MEMBERS`, `MAX_SLOTS`, `is_global`.
- Produces (Tasks 2-8):
  ```rust
  // rl_proto (lib.rs)
  /// Most ticks of final inputs one `Control::Report` carries (30 s).
  pub const MAX_REPORT_FRAMES: usize = 3_000;
  /// Most provisional inputs of its own slot a report carries.
  pub const MAX_REPORT_OWN: usize = 256;
  /// What a peer knows, for the new host after a migration (spec §3.7).
  #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
  pub struct InputsReport {
      /// The reporter's frontier: the new host's frames to it start there.
      pub ack: u32,
      /// `frames[k]` holds tick `first + k`'s final inputs as the reporter knows them (the frame delta rule; `base` is the new host's value at `first - 1`).
      pub first: u32,
      pub frames: Vec<Frame>,
      /// The reporter's own provisional inputs for its slot, ticks `own_first..` (empty for a spectator).
      pub own_first: u32,
      pub own: Vec<u8>,
  }
  pub enum Control {
      // … the existing variants, unchanged and in order, then (appended):
      /// New host to a rejoined peer: per slot, the first tick it lacks (`known.len()` = the match's slots, at most `MAX_SLOTS`).
      Collect { known: Vec<u32> },
      /// A rejoined peer to its new host: the answer to `Collect`.
      Report(InputsReport),
      /// New host to its peers: the match resumes at `tick`.
      Resume { tick: u32 },
  }
  // rl_proto::lobby
  pub const MAX_STANDBY_RELAYS: usize = 2;
  pub const MAX_RELAY_URL: usize = 128;
  /// Standby ports below this are no successor's (N64: a hostile host cannot aim peers at a privileged service).
  pub const MIN_STANDBY_PORT: u16 = 1024;
  pub enum ToHost { /* …existing…, then appended: */
      /// The member's standby endpoint's port and the relays it listens on once it hosts (N63).
      Standby { port: u16, relays: Vec<String> },
  }
  pub enum ToClient { /* …existing…, then appended: */
      /// The succession order (spec §3.7), whole, on every change during a match.
      Succession(Vec<SuccessorWire>),
  }
  #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
  pub struct SuccessorWire {
      pub member: u8,
      pub key: PublicKey,
      /// Its standby endpoint, IPv4 octets and port, when the host saw it on its own network (N64).
      pub direct: Option<([u8; 4], u16)>,
      /// At most `MAX_STANDBY_RELAYS` relays it listens on once it hosts.
      pub relays: Vec<String>,
      /// It may host (a standby and a world); `false`: it follows only.
      pub can_host: bool,
  }
  /// `ws://` or `wss://`, printable ASCII without spaces, at most `MAX_RELAY_URL` bytes.
  pub fn valid_relay_url(s: &str) -> bool;
  /// At most `MAX_MEMBERS` entries, unique member ids and keys, relays valid and at most 2, direct ports ≥ `MIN_STANDBY_PORT`.
  pub fn valid_succession(list: &[SuccessorWire]) -> bool;
  // rl_proto::signal: Refusal gains (appended) NotHost (a standby before its peer took over) and Closed (a migrated lobby refuses strangers, N72).
  ```

Context:
- **Decoder bounds** (each violation is `ProtoError::Invalid`):
  - `Collect`: `known.len()` 1..=`MAX_SLOTS`.
  - `Report`: `frames.len()` ≤ `MAX_REPORT_FRAMES`; `own.len()` ≤ `MAX_REPORT_OWN`; `first + frames.len()` and `own_first + own.len()` do not overflow; each frame's `changed ⊆ known` and `keys.len() == changed.count_ones()` (as `decode_frames` checks).
  - `Standby`: port ≥ `MIN_STANDBY_PORT`; relays per `valid_relay_url`, at most 2.
  - `Succession`: `valid_succession`.
- **Size:** a worst-case `Succession` (48 entries, each with a direct address and 2 relays of 128 bytes) must fit `MAX_LOBBY`; a worst-case `Report` encoded by Task 2's trimming must fit `MAX_CONTROL`. Pin both with tests, as `worst_case_messages_fit` does.
- `ToHost::Standby` is a lobby message like `Ready`: rl-lobby's per-member rate limits apply (Task 4).

- [ ] **Step 1: Write the failing tests** `crates/rl-proto/tests/migration.rs`:

```rust
//! The migration wire (spec §3.7, plan 12c): round trips, bounds, worst cases.

use rl_proto::*;

fn report(frames: usize, own: usize) -> InputsReport {
    let f = encode_frames(100, frames as u32, 32, &|t, s| Some(((t as usize * 7 + s) % 251) as u8), &|_| Some(0));
    InputsReport { ack: 90, first: 100, frames: f, own_first: 120, own: vec![3; own] }
}

#[test]
fn migration_controls_round_trip() {
    for c in [
        Control::Collect { known: vec![0, 5, u32::MAX - 1] },
        Control::Report(report(64, 10)),
        Control::Resume { tick: 12_345 },
    ] {
        assert_eq!(decode_control(&encode_control(&c)), Ok(c));
    }
}

#[test]
fn migration_controls_out_of_bounds_are_refused() {
    let bad = [
        Control::Collect { known: vec![] },
        Control::Collect { known: vec![0; MAX_SLOTS + 1] },
        Control::Report(InputsReport { own: vec![0; MAX_REPORT_OWN + 1], ..report(1, 0) }),
        Control::Report(InputsReport { first: u32::MAX, ..report(2, 0) }),
        Control::Report(InputsReport { own_first: u32::MAX, ..report(1, 2) }),
        Control::Report(InputsReport {
            frames: vec![Frame { known: 1, changed: 2, keys: vec![1] }],
            ..report(0, 0)
        }),
    ];
    for c in bad {
        assert_eq!(decode_control(&encode_control(&c)), Err(ProtoError::Invalid), "{c:?}");
    }
    let mut long = report(1, 0);
    long.frames = vec![Frame { known: 0, changed: 0, keys: vec![] }; MAX_REPORT_FRAMES + 1];
    assert_eq!(decode_control(&encode_control(&Control::Report(long))), Err(ProtoError::Invalid));
}

fn successor(n: u8) -> SuccessorWire {
    SuccessorWire {
        member: n,
        key: [n; 32],
        direct: Some(([10, 0, 0, n], 40_000 + n as u16)),
        relays: vec![format!("wss://{}.example/{}", n, "r".repeat(100)), "ws://relay.example".into()],
        can_host: true,
    }
}

#[test]
fn a_worst_case_succession_fits_and_round_trips() {
    let list: Vec<SuccessorWire> = (0..MAX_MEMBERS as u8).map(successor).collect();
    for s in &list {
        assert!(s.relays.iter().all(|r| r.len() <= MAX_RELAY_URL && valid_relay_url(r)));
    }
    let bytes = encode_to_client(&ToClient::Succession(list.clone()));
    assert!(bytes.len() <= MAX_LOBBY, "{} bytes", bytes.len());
    assert_eq!(decode_to_client(&bytes), Ok(ToClient::Succession(list)));
}

#[test]
fn successions_and_standbys_out_of_bounds_are_refused() {
    let mut twice = vec![successor(1), successor(1)];
    assert!(!valid_succession(&twice));
    twice[1].member = 2; // the same key under two ids
    assert!(!valid_succession(&twice));
    let low_port = SuccessorWire { direct: Some(([10, 0, 0, 1], 80)), ..successor(1) };
    let three = SuccessorWire { relays: vec!["wss://a.example".into(); 3], ..successor(1) };
    let not_ws = SuccessorWire { relays: vec!["https://a.example".into()], ..successor(1) };
    let spaced = SuccessorWire { relays: vec!["wss://a b.example".into()], ..successor(1) };
    let long = SuccessorWire { relays: vec![format!("wss://{}", "a".repeat(MAX_RELAY_URL))], ..successor(1) };
    for s in [low_port, three, not_ws, spaced, long] {
        let bytes = encode_to_client(&ToClient::Succession(vec![s.clone()]));
        assert_eq!(decode_to_client(&bytes), Err(ProtoError::Invalid), "{s:?}");
    }
    let too_many: Vec<SuccessorWire> = (0..=MAX_MEMBERS as u8).map(successor).collect();
    assert_eq!(decode_to_client(&encode_to_client(&ToClient::Succession(too_many))), Err(ProtoError::Invalid));
    for bad in [
        ToHost::Standby { port: 1023, relays: vec![] },
        ToHost::Standby { port: 40_000, relays: vec!["wss://a.example".into(); 3] },
        ToHost::Standby { port: 40_000, relays: vec!["file:///etc/passwd".into()] },
    ] {
        assert_eq!(decode_to_host(&encode_to_host(&bad)), Err(ProtoError::Invalid), "{bad:?}");
    }
    let ok = ToHost::Standby { port: 40_000, relays: vec!["wss://a.example".into()] };
    assert_eq!(decode_to_host(&encode_to_host(&ok)), Ok(ok));
}

#[test]
fn the_new_refusals_round_trip() {
    for r in [Refusal::NotHost, Refusal::Closed] {
        let s = Signal::Refused { reason: r };
        assert_eq!(decode_signal(&encode_signal(&s)), Ok(s));
        let c = ToClient::Refused(r);
        assert_eq!(decode_to_client(&encode_to_client(&c)), Ok(c));
    }
}
```

  Extend `tests/fuzz.rs`: the existing random-bytes and mutation fuzzers already cover `decode_control`. Add a proptest strategy producing `Collect`/`Report`/`Resume` with edge values (`u32::MAX`, 0, `MAX_REPORT_FRAMES ± 1` frames, random `known`/`changed` masks), mutated after encoding, asserting no panic and that whatever decodes re-encodes to the same value. Extend `tests/fuzz_lobby.rs` the same way for `Standby` and `Succession`.
- [ ] **Step 2: Run `cargo nextest run -p rl-proto`, expect FAIL** (the types do not exist).
- [ ] **Step 3: Implement.** Append the variants (never reorder: postcard encodes the variant index), validate in the decoders, and add the `Refusal` variants to every exhaustive `match` in the workspace with the plain text: `NotHost` "this player is not hosting the game"; `Closed` "this game takes back its own players only".
- [ ] **Step 4: Run `cargo nextest run -p rl-proto`, then `cargo build --target wasm32-unknown-unknown -p rl-proto`; `cargo xtask check`.**
- [ ] **Step 5: Update `crates/rl-proto/CLAUDE.md`** (the migration messages, their bounds, the refusals). **Commit** `rl-proto: the host migration wire`.

---

### Task 2: `rl-netcode`: the session's side of a migration

**Files:**
- Modify: `crates/rl-netcode/src/session.rs` (events, stats, the silence check, `advance` while suspended, `on_control`, `is_host` made public), `crates/rl-netcode/src/lib.rs`, `crates/rl-netcode/CLAUDE.md`
- Create: `crates/rl-netcode/src/session/migrate.rs` (take-over, rejoins, collection, the merge, resume, rehost: kept out of the 2000-line `session.rs`), `crates/rl-netcode/src/migration.rs` (constants only in this task; Task 3 adds the state machine)
- Test: `crates/rl-netcode/tests/migration.rs` (a `mod migration;` of `tests/integration.rs`)

**Interfaces:**
- Consumes: Task 1's `Control::{Collect, Report, Resume}`, `InputsReport`, `MAX_REPORT_FRAMES`, `MAX_REPORT_OWN`.
- Produces (Tasks 3, 5, 6, 7):
  ```rust
  // rl_netcode::migration (this task: the constants)
  /// Spec §3.7: a client or spectator whose host sent nothing for this long, while it kept advancing, finds it gone.
  pub const HOST_SILENT_US: u64 = 2_000_000;
  /// N70: a host whose own polls stopped this long steps down (below HOST_SILENT_US, so jitter cannot split the match).
  pub const STEP_DOWN_US: u64 = 1_500_000;
  // rl_netcode::Event gains:
  /// Client or spectator: nothing from its host for `HOST_SILENT_US` while this peer kept advancing (once per silence).
  HostSilent,
  /// Client or spectator: its host spoke again after `HostSilent` (before any `rehost`).
  HostHeard,
  /// The timeline resumed after a migration at `tick` (the new host's `resume`; a client's `Control::Resume`).
  Resumed { tick: u32 },
  // rl_netcode::Stats gains:
  pub migrations: u64,        // resumes after a take-over or a rehost
  pub report_conflicts: u64,  // reported final inputs that differed from ours (ignored)
  impl<S: Simulation> Session<S> {
      pub fn is_host(&self) -> bool;
      /// Stops advancing: no new tick, no datagram (the migration began). Rollbacks owed and states arriving are still taken.
      pub fn suspend(&mut self);
      pub fn suspended(&self) -> bool;
      /// The old host is back before any rehost (N69): advancing again, the pace restarting at `now_us` (the pause is not caught up).
      pub fn unsuspend(&mut self, now_us: u64);
      /// Client or spectator: its host is now `host` (a new link). The host's state (acks, round trip, silence) starts afresh; the session stays suspended until `Control::Resume`, answers `Control::Collect` with `Control::Report`, and takes nothing from any other peer.
      pub fn rehost(&mut self, host: PeerId, now_us: u64);
      /// This peer becomes the host (spec §3.7). Its local slots are its own slot (if a player) and `bots`; every other slot is awaited from a rejoining peer or abandoned at the resume. Suspended until `resume`. Nothing is filled here (N67: a later report may carry final values for any slot).
      pub fn take_over(&mut self, bots: &[u8], now_us: u64);
      /// New host: `peer` rejoined with `seat` (its key matched the succession). Sends it `Control::Collect` with our known prefixes. `false` (nothing done) after the resume, for a seat out of range, for a slot already held, or when not a suspended host.
      pub fn rejoined(&mut self, peer: PeerId, seat: Seat, now_us: u64) -> bool;
      /// New host: rejoined peers whose `Control::Report` has not arrived.
      pub fn awaiting_reports(&self) -> Vec<PeerId>;
      /// New host: the merge ends. Reported own inputs extend their slots; bot slots' missing ticks below R get their previous final value, its own slot its pressed keys; awaited slots nobody took become abandoned (`Keys(0)`, decision N6). `Control::Resume { tick }` to every rejoined peer; the pace restarts at the current tick and `now_us`. Returns that tick (R).
      pub fn resume(&mut self, now_us: u64) -> u32;
  }
  ```

Context (decisions N67, N69, N76):
- **Host silence.** A client or spectator pushes `HostSilent` once when `now - heard_us(host) ≥ HOST_SILENT_US`, measured only over time the peer itself advanced: a gap of more than `HOST_SILENT_US / 2` between two of its own `advance` calls restarts the silence clock at the later one. A frozen client that wakes up does not blame its host for its own sleep. A datagram from the host after `HostSilent` (and before `rehost`) pushes `HostHeard` once. A `Control::Bye` from the host stays `PeerLeft { peer: host }`; the driver starts the migration on either (N76).
- **Suspended:** `advance` runs no new tick and queues no datagram, and pushes no `Stalled` (a migration is no stall: the feel gates count stalls). It still runs an owed rollback or a loaded state's re-simulation, and still confirms checksums.
- **Collect / Report** (the new host's `known[s]` = its `known_upto(s)`):
  - The reporter answers with the frames from `first = min(known)` to its own horizon. Each slot carries its final values (`None` where it lacks them; the encoder's `base` is its value at `first - 1`).
  - It also carries its own provisional inputs from its slot's `known_upto` (at most `MAX_REPORT_OWN`).
  - When the encoded control exceeds `MAX_CONTROL` or `MAX_REPORT_FRAMES`, it halves the span from the end until it fits (the earliest ticks matter: N67).
  - At the new host, frames decode with `base(s)` = its own value at `first - 1` (known for every slot, since `first` is the least prefix) and are taken in tick order: `New` extends, `Same` is fine, `Conflict` is ignored and counted.
  - Provisional own inputs wait until `resume`, then extend that slot's prefix only.
  - A `Report` from a peer that never rejoined, or a second one from the same peer, changes nothing.
  - A `Report` after the resume is taken the same way.
- **Resume** (the order matters: reports first, then the fills, N67):
  - Each rejoined player's reported own inputs extend its slot's prefix.
  - Each bot slot's missing ticks below `R` get its previous final value; the new host's own slot gets its pressed keys. A difference from what the new host simulated is an ordinary rollback at its next advance.
  - The awaited player slots whose peer did not rejoin, and the old host's own player's slot, become abandoned: recorded as lost players, so the existing `fill` makes them `Keys(0)` from their first unknown tick (N6).
  - Every rejoined peer's acknowledged frontier is its report's `ack`, so the host's datagrams start there ("sends each peer what it lacks").
  - Each rejoined peer gets `Resume { tick: R }`; `Event::Resumed { tick: R }` and `Stats::migrations += 1` here.
  - At a client, `Resume` from its host unsuspends it: the pace restarts at `max(current, R)` and `now`; its time sync starts as after a jump (margins stale until fresh samples); `Event::Resumed`. `Collect`/`Resume` from anyone but its current host are ignored.
- **The new host's own slot** keeps the match's input delay for the rest of the match (decision N2: a delay changed mid-match leaves a tick with two inputs or none). Its keys still go to its provisional buffer `d` ticks ahead and become final when simulated.
- **Spectators** take part: they report (no own inputs), and a spectator may take over (its local slots are the bots alone).
- **Never panic on any of these messages** (spec §5.4); unknown senders are ignored, not `Malformed`.

- [ ] **Step 1: Write the failing tests** `crates/rl-netcode/tests/migration.rs`:

```rust
//! Host migration in the session (spec §3.7): three sessions over a wire the
//! test controls, in 1 ms steps. Who takes over and who rejoins whom is the
//! drivers' part (the harness, rl-online); here the test does it by hand.

use std::collections::{BTreeMap, BTreeSet};

use proptest::prelude::*;
use rl_netcode::harness::Factory;
use rl_netcode::migration::HOST_SILENT_US;
use rl_netcode::toy::ToyFactory;
use rl_netcode::{Channel, Config, Driver, Event, PeerId, Role, Seat, Session, ToySim, input_delay};
use rl_proto::{Control, Frame, InputsReport, encode_control, encode_frames};
use rl_sim::Keys;

const H: PeerId = 0; // the first host: slot 0 its player, slot 1 its bot
const A: PeerId = 1; // slot 2: the first successor
const B: PeerId = 2; // slot 3
const SLOTS: u8 = 4;

struct Mesh {
    s: BTreeMap<PeerId, Session<ToySim>>,
    d: BTreeMap<PeerId, Box<dyn Driver<ToySim>>>,
    /// Not alive: no advance, nothing received (a crash).
    alive: BTreeMap<PeerId, bool>,
    /// Paused: no advance; what arrives waits (a frozen game whose socket fills).
    paused: BTreeSet<PeerId>,
    inbox: BTreeMap<PeerId, Vec<(PeerId, Channel, Vec<u8>)>>,
    one_way_ms: u64,
    /// `(from, to)` → from this ms on, that direction loses everything.
    cut: BTreeMap<(PeerId, PeerId), u64>,
    now_ms: u64,
    wire: Vec<(u64, PeerId, PeerId, Channel, Vec<u8>)>,
    events: BTreeMap<PeerId, Vec<(u64, Event)>>,
}

impl Mesh {
    fn new(one_way_ms: u64, seed: u64) -> Mesh {
        let mut f = ToyFactory { slots: SLOTS, seed };
        let delay = input_delay(2 * one_way_ms as u32);
        let sim = || ToySim::new(SLOTS as usize);
        let host = Role::Host { peers: vec![(A, Seat::Player(2)), (B, Seat::Player(3))] };
        let mut s = BTreeMap::new();
        s.insert(H, Session::new(Config::new(SLOTS, 0), host, sim(), 0));
        s.insert(A, Session::new(Config::new(SLOTS, delay), Role::Client { host: H, slot: 2 }, sim(), 0));
        s.insert(B, Session::new(Config::new(SLOTS, delay), Role::Client { host: H, slot: 3 }, sim(), 0));
        let mut d = BTreeMap::new();
        d.insert(H, f.driver(H, None));
        d.insert(A, f.driver(A, Some(Seat::Player(2))));
        d.insert(B, f.driver(B, Some(Seat::Player(3))));
        Mesh {
            s,
            d,
            alive: [(H, true), (A, true), (B, true)].into(),
            paused: BTreeSet::new(),
            inbox: BTreeMap::new(),
            one_way_ms,
            cut: BTreeMap::new(),
            now_ms: 0,
            wire: Vec::new(),
            events: BTreeMap::new(),
        }
    }

    fn alive(&self, id: PeerId) -> bool {
        self.alive.get(&id).copied().unwrap_or(false)
    }

    fn step(&mut self) {
        self.now_ms += 1;
        let (now, us) = (self.now_ms, self.now_ms * 1000);
        let (due, later): (Vec<_>, Vec<_>) =
            std::mem::take(&mut self.wire).into_iter().partition(|m| m.0 <= now);
        self.wire = later;
        for (_, from, to, ch, bytes) in due {
            if self.paused.contains(&to) {
                self.inbox.entry(to).or_default().push((from, ch, bytes));
            } else if self.alive(to) {
                self.s.get_mut(&to).unwrap().receive(from, ch, &bytes, us);
            }
        }
        let ids: Vec<PeerId> = self.s.keys().copied().collect();
        for id in ids {
            if !self.alive(id) || self.paused.contains(&id) {
                continue;
            }
            for (from, ch, bytes) in self.inbox.remove(&id).unwrap_or_default() {
                self.s.get_mut(&id).unwrap().receive(from, ch, &bytes, us);
            }
            let s = self.s.get_mut(&id).unwrap();
            s.advance(us, self.d.get_mut(&id).unwrap().as_mut());
            let mut out = Vec::new();
            s.transmit(&mut |to, ch, b| out.push((to, ch, b)));
            for e in s.drain_events() {
                self.events.entry(id).or_default().push((now, e));
            }
            for (to, ch, b) in out {
                if self.cut.get(&(id, to)).is_some_and(|&from| now >= from) {
                    continue;
                }
                self.wire.push((now + self.one_way_ms, id, to, ch, b));
            }
        }
    }

    fn run_ms(&mut self, ms: u64) {
        for _ in 0..ms {
            self.step();
        }
    }

    fn until(&mut self, limit_ms: u64, what: &str, done: impl Fn(&Mesh) -> bool) {
        let end = self.now_ms + limit_ms;
        while !done(self) {
            assert!(self.now_ms < end, "{what} within {limit_ms} ms");
            self.step();
        }
    }

    fn first(&self, id: PeerId, f: impl Fn(&Event) -> bool) -> Option<u64> {
        self.events.get(&id)?.iter().find(|(_, e)| f(e)).map(|(t, _)| *t)
    }

    fn count(&self, id: PeerId, f: impl Fn(&Event) -> bool) -> usize {
        self.events.get(&id).map_or(0, |v| v.iter().filter(|(_, e)| f(e)).count())
    }

    fn known(&self, id: PeerId) -> Vec<u32> {
        (0..SLOTS).map(|s| self.s[&id].input_log().known_upto(s)).collect()
    }

    /// A takes over (slot 1 is the bot it now plays) and B rejoins it, as the drivers do.
    fn migrate_to_a(&mut self) {
        let us = self.now_ms * 1000;
        for id in [A, B] {
            self.s.get_mut(&id).unwrap().suspend();
        }
        self.s.get_mut(&A).unwrap().take_over(&[1], us);
        self.s.get_mut(&B).unwrap().rehost(A, us);
        assert!(self.s.get_mut(&A).unwrap().rejoined(B, Seat::Player(3), us));
    }
}

#[test]
fn a_silent_host_is_reported_after_two_seconds_and_heard_again() {
    let mut m = Mesh::new(20, 1);
    m.run_ms(3_000);
    m.alive.insert(H, false);
    let gone = m.now_ms;
    m.run_ms(2_500);
    let silent_ms = HOST_SILENT_US / 1000;
    for id in [A, B] {
        let at = m.first(id, |e| *e == Event::HostSilent).expect("HostSilent");
        // Its last datagram left at `gone` at the latest and arrived one way later.
        assert!(at >= gone + silent_ms && at <= gone + m.one_way_ms + silent_ms + 20, "peer {id} at {at}");
        assert_eq!(m.count(id, |e| *e == Event::HostSilent), 1, "once per silence");
        assert_eq!(m.count(id, |e| matches!(e, Event::PeerTimedOut { .. })), 0, "not yet 10 s");
    }
    m.alive.insert(H, true); // a frozen host whose clock ran: it simply sends again
    m.run_ms(500);
    for id in [A, B] {
        assert_eq!(m.count(id, |e| *e == Event::HostHeard), 1, "peer {id} heard it again");
    }
}

#[test]
fn a_peers_own_sleep_is_not_its_hosts_silence() {
    let mut m = Mesh::new(20, 2);
    m.run_ms(2_000);
    // B's game stops for 3 s and everything the host sent meanwhile is lost (its socket overflowed).
    m.paused.insert(B);
    m.cut.insert((H, B), m.now_ms);
    m.run_ms(3_000);
    m.cut.remove(&(H, B));
    m.inbox.remove(&B);
    m.paused.remove(&B);
    m.run_ms(1_000);
    assert_eq!(m.count(B, |e| *e == Event::HostSilent), 0, "the host was never silent while B ran");
}

#[test]
fn a_suspended_session_runs_no_tick_sends_nothing_and_never_stalls() {
    let mut m = Mesh::new(20, 3);
    m.run_ms(1_000);
    m.s.get_mut(&B).unwrap().suspend();
    assert!(m.s[&B].suspended());
    let (tick, sent) = (m.s[&B].current_tick(), m.s[&B].stats().datagrams_out);
    m.run_ms(1_000);
    assert_eq!(m.s[&B].current_tick(), tick);
    assert_eq!(m.s[&B].stats().datagrams_out, sent);
    assert_eq!(m.count(B, |e| matches!(e, Event::Stalled { .. })), 0);
    m.s.get_mut(&B).unwrap().unsuspend(m.now_ms * 1000);
    m.run_ms(200);
    assert!(m.s[&B].current_tick() > tick, "it runs again");
}

#[test]
fn the_successor_takes_what_a_peer_knows_and_both_converge() {
    let mut m = Mesh::new(10, 4);
    m.run_ms(3_800);
    // The host's last 200 ms reach B only: B knows more than A.
    m.cut.insert((H, A), m.now_ms);
    m.run_ms(200);
    m.alive.insert(H, false);
    m.until(2_500, "A and B find the host silent", |m| {
        [A, B].iter().all(|&id| m.first(id, |e| *e == Event::HostSilent).is_some())
    });
    let (a_knew, b_knew) = (m.known(A), m.known(B));
    assert!((0..SLOTS as usize).any(|s| b_knew[s] > a_knew[s] + 10), "B knows more: {b_knew:?}, A {a_knew:?}");
    let b_values: Vec<(u32, u8, Option<Keys>)> = (0..SLOTS)
        .flat_map(|s| (a_knew[s as usize]..b_knew[s as usize]).map(move |t| (t, s)))
        .map(|(t, s)| (t, s, m.s[&B].final_input(t, s)))
        .collect();
    m.migrate_to_a();
    m.until(1_000, "B's report", |m| m.s[&A].awaiting_reports().is_empty());
    for &(t, s, v) in &b_values {
        assert_eq!(m.s[&A].final_input(t, s), v, "slot {s} tick {t}: B's final value taken");
    }
    let r = m.s.get_mut(&A).unwrap().resume(m.now_ms * 1000);
    m.until(1_000, "B resumes", |m| m.first(B, |e| matches!(e, Event::Resumed { .. })).is_some());
    assert!(m.first(B, |e| *e == Event::Resumed { tick: r }).is_some(), "B resumes at the new host's tick");
    m.run_ms(5_000);
    let (a, b) = (&m.s[&A], &m.s[&B]);
    let upto = a.frontier().min(b.frontier());
    assert!(upto > r + 400, "the match went on: {upto} past {r}");
    for t in 0..upto {
        assert_eq!(a.input_log().row(t), b.input_log().row(t), "tick {t}");
    }
    // The old host's player stands still from its known prefix (N6); its bot plays on at A.
    let from = a_knew[0].max(b_knew[0]);
    assert!((from..upto).all(|t| a.final_input(t, 0) == Some(Keys(0))));
    assert!((r..upto).any(|t| a.final_input(t, 1) != Some(Keys(0))), "the bot plays at the new host");
    // Confirmed checksums agree, and nobody saw a desync, a conflict or a malformed message.
    let sums: BTreeMap<u32, u64> = a.checksums().iter().copied().collect();
    let common: Vec<&(u32, u64)> = b.checksums().iter().filter(|(t, _)| sums.contains_key(t)).collect();
    assert!(common.len() > 40, "{} common checksums", common.len());
    for (t, h) in common {
        assert_eq!(sums[t], *h, "checksum of tick {t}");
    }
    for id in [A, B] {
        let bad = |e: &Event| matches!(e, Event::Desync { .. } | Event::Inconsistent { .. } | Event::Malformed { .. });
        assert_eq!(m.count(id, bad), 0, "peer {id}");
    }
    assert_eq!(a.stats().migrations, 1);
    assert_eq!(b.stats().migrations, 1);
    assert_eq!(a.stats().report_conflicts, 0);
}

#[test]
fn a_rejoined_player_is_not_corrected_for_inputs_it_reported() {
    let mut m = Mesh::new(30, 5);
    m.run_ms(4_000);
    m.alive.insert(H, false);
    m.until(2_500, "silence", |m| m.first(B, |e| *e == Event::HostSilent).is_some());
    let before = m.s[&B].stats().own_corrections;
    m.migrate_to_a();
    m.until(1_000, "B's report", |m| m.s[&A].awaiting_reports().is_empty());
    m.s.get_mut(&A).unwrap().resume(m.now_ms * 1000);
    m.run_ms(3_000);
    assert_eq!(m.s[&B].stats().own_corrections, before, "its pressed keys became final as pressed");
}

#[test]
fn a_conflicting_report_is_ignored_and_counted() {
    let mut m = Mesh::new(10, 6);
    m.run_ms(3_000);
    m.alive.insert(H, false);
    m.until(2_500, "silence", |m| m.first(A, |e| *e == Event::HostSilent).is_some());
    m.migrate_to_a();
    let us = m.now_ms * 1000;
    // A forged spectator rejoins and claims another value for slot 0 at tick 100.
    assert!(m.s.get_mut(&A).unwrap().rejoined(9, Seat::Spectator, us));
    let real = m.s[&A].final_input(100, 0).unwrap().0;
    let base: Vec<Option<u8>> = (0..SLOTS).map(|s| m.s[&A].final_input(99, s).map(|k| k.0)).collect();
    let frames = encode_frames(100, 1, SLOTS as usize, &|_, s| (s == 0).then_some(real ^ 0x10), &|s| base[s]);
    let report = InputsReport { ack: 0, first: 100, frames, own_first: 0, own: vec![] };
    m.s.get_mut(&A).unwrap().receive(9, Channel::Reliable, &encode_control(&Control::Report(report)), us);
    assert_eq!(m.s[&A].final_input(100, 0), Some(Keys(real)), "the first value stays");
    assert_eq!(m.s[&A].stats().report_conflicts, 1);
    assert_eq!(m.count(A, |e| matches!(e, Event::Malformed { .. })), 0);
}

#[test]
fn migration_messages_from_a_non_host_are_ignored() {
    let mut m = Mesh::new(10, 7);
    m.run_ms(1_000);
    let us = m.now_ms * 1000;
    // B, a client of a live host: A's Collect and Resume are not its host's.
    let sent = m.s[&B].stats().datagrams_out;
    for c in [Control::Collect { known: vec![0; SLOTS as usize] }, Control::Resume { tick: 5 }] {
        m.s.get_mut(&B).unwrap().receive(A, Channel::Reliable, &encode_control(&c), us);
    }
    let mut out = Vec::new();
    m.s.get_mut(&B).unwrap().transmit(&mut |to, ch, b| out.push((to, ch, b)));
    assert!(out.iter().all(|(to, ..)| *to == H), "nothing to A");
    assert_eq!(m.s[&B].stats().datagrams_out, sent);
    assert!(!m.s[&B].suspended());
    // The host: a report from a peer that never rejoined changes nothing.
    let row = m.s[&H].input_log().row(50);
    let frames = encode_frames(50, 1, SLOTS as usize, &|_, _| Some(0x7f), &|_| Some(0));
    let report = InputsReport { ack: 0, first: 50, frames, own_first: 0, own: vec![] };
    m.s.get_mut(&H).unwrap().receive(B, Channel::Reliable, &encode_control(&Control::Report(report)), us);
    assert_eq!(m.s[&H].input_log().row(50), row);
    assert_eq!(m.s[&H].stats().report_conflicts, 0);
    for id in [H, B] {
        assert_eq!(m.count(id, |e| matches!(e, Event::Malformed { .. })), 0);
    }
}

fn control() -> impl Strategy<Value = Control> {
    let edge = prop_oneof![Just(0u32), Just(1), Just(99), Just(u32::MAX - 1), Just(u32::MAX), 0u32..5_000];
    let frame = (any::<u32>(), any::<u32>(), prop::collection::vec(any::<u8>(), 0..33))
        .prop_map(|(known, changed, keys)| Frame { known, changed, keys });
    prop_oneof![
        prop::collection::vec(edge.clone(), 0..40).prop_map(|known| Control::Collect { known }),
        (edge.clone(), edge.clone(), prop::collection::vec(frame, 0..80), edge.clone(), prop::collection::vec(any::<u8>(), 0..300))
            .prop_map(|(ack, first, frames, own_first, own)| Control::Report(InputsReport { ack, first, frames, own_first, own })),
        edge.prop_map(|tick| Control::Resume { tick }),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn migration_controls_never_panic_or_change_confirmed_inputs(
        to_host in prop::collection::vec(control(), 1..12),
        to_client in prop::collection::vec(control(), 1..12),
    ) {
        let mut m = Mesh::new(10, 8);
        m.run_ms(600);
        m.alive.insert(H, false);
        m.run_ms(2_100);
        m.migrate_to_a();
        let us = m.now_ms * 1000;
        let confirmed = m.s[&A].frontier();
        let rows: Vec<_> = (0..confirmed).map(|t| m.s[&A].input_log().row(t)).collect();
        for c in &to_host {
            m.s.get_mut(&A).unwrap().receive(B, Channel::Reliable, &encode_control(c), us);
        }
        for c in &to_client {
            m.s.get_mut(&B).unwrap().receive(A, Channel::Reliable, &encode_control(c), us);
        }
        m.run_ms(300);
        for t in 0..confirmed {
            prop_assert_eq!(m.s[&A].input_log().row(t), rows[t as usize], "tick {}", t);
        }
    }
}
```

- [ ] **Step 2: Run `cargo nextest run -p rl-netcode migration`, expect FAIL** (no such API).
- [ ] **Step 3: Implement** in `session/migrate.rs` (the session's fields it needs are `pub(crate)`). The existing tests must pass unchanged: `HostSilent` is a new event, and the harness and drivers ignore it until Tasks 3 and 5.
- [ ] **Step 4: Run `cargo nextest run -p rl-netcode`, expect PASS; `cargo build --target wasm32-unknown-unknown -p rl-netcode`; `cargo xtask check`.**
- [ ] **Step 5: Update `crates/rl-netcode/CLAUDE.md`** (a "Migration (session)" paragraph: silence, suspension, take-over, the merge rules N67, resume, rehost). **Commit** `rl-netcode: the session's host migration (silence, take-over, merge, resume)`.

---

### Task 3: `rl-netcode`: the successor choice and the harness's migrations

**Files:**
- Modify: `crates/rl-netcode/src/migration.rs` (the state machine), `crates/rl-netcode/src/harness.rs` (exits, migration, profile changes, the report, the verdict), `crates/rl-netcode/src/toy.rs`, every `Scenario { .. }` literal in `crates/rl-netcode/tests/*.rs` and `crates/rl-cli/src/netsim.rs` (the new fields, empty), `crates/rl-netcode/CLAUDE.md`
- Test: unit tests in `migration.rs`; `crates/rl-netcode/tests/migration.rs` (appended)

**Interfaces:**
- Consumes: Task 2's session API and constants.
- Produces (Tasks 5-7, 10):
  ```rust
  // rl_netcode::migration
  pub const WINDOW_US: u64 = 5_000_000;
  pub const GIVE_UP_US: u64 = 10_000_000;
  pub const RETRY_US: u64 = 250_000;
  pub const MAX_MIGRATIONS: u32 = 4;
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum Step {
      /// Keep an attempt to join successor `i` (an index into the order) going.
      Try(usize),
      /// This peer's turn: it is the successor (stays so until the driver drops the migration).
      Host,
      /// No successor's turn now (the windows ran out): wait for `GiveUp`.
      Wait,
      /// No successor was reachable within `GIVE_UP_US`: the host is lost.
      GiveUp,
  }
  /// One peer's view of a migration (spec §3.7, N65): which successor it tries, from the moment it found its host gone.
  #[derive(Clone, Debug)]
  pub struct Migration { /* can_host, me, began_us, contact: Option<usize>, hosting: bool */ }
  impl Migration {
      /// `can_host[i]`: entry `i` of the succession may host now (the driver passes `false` for former hosts and followers); `me`: this peer's entry, if any.
      pub fn new(can_host: Vec<bool>, me: Option<usize>, now_us: u64) -> Migration;
      /// The `k`-th entry that may host has the window [began + k·WINDOW_US, began + (k+1)·WINDOW_US). A contacted successor is kept until GIVE_UP_US; once `Host`, always `Host`.
      pub fn step(&mut self, now_us: u64) -> Step;
      /// Successor `i` answered as a host (N65's contact).
      pub fn contact(&mut self, i: usize);
      /// The attempt to `i` failed after contact: back to the windows.
      pub fn lost_contact(&mut self, i: usize);
      pub fn began_us(&self) -> u64;
      /// When the current window ends (the successor's collection deadline, N68).
      pub fn window_end(&self, now_us: u64) -> u64;
  }
  // rl_netcode::harness
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum ExitHow { Crash, Leave }
  /// `peer` (the host, `HOST`, or any other) leaves at the current host's tick `tick`: `Crash` (no frame, nothing sent or received from then on) or `Leave` (`Session::leave`, transmitted, then gone).
  #[derive(Clone, Debug)] pub struct Exit { pub peer: PeerId, pub tick: u32, pub how: ExitHow }
  /// Host migration on (spec §3.7).
  #[derive(Clone, Debug, Default)] pub struct MigrationPlan {
      /// The first host's own player's slot (abandoned once it is gone); its other local slots are bots, which a new host plays.
      pub host_human: Option<u8>,
      /// `(peer, µs)`: this peer's joins to a successor take this long after contact (a slow path through relays), instead of three pings.
      pub slow_joins: Vec<(PeerId, u64)>,
  }
  /// From the current host's tick `tick`, `peer`'s link to whoever hosts takes `profile`.
  #[derive(Clone, Debug)] pub struct ProfileChange { pub tick: u32, pub peer: PeerId, pub profile: Profile }
  // Scenario gains:
  pub exits: Vec<Exit>,
  pub migration: Option<MigrationPlan>,
  pub profile_changes: Vec<ProfileChange>,
  // Report gains:
  /// Who hosted, in order (`HOST` first).
  pub hosts: Vec<PeerId>,
  pub migrations: Vec<Migrated>,
  /// Peers that gave up (spec §3.7's host lost).
  pub host_lost: BTreeSet<PeerId>,
  /// Peers that crashed, left or stepped down.
  pub exited: BTreeSet<PeerId>,
  #[derive(Clone, Debug, PartialEq, Eq)]
  pub struct Migrated {
      pub from: PeerId,
      pub to: PeerId,
      /// Virtual time the first peer found the host gone (`HostSilent`, `PeerLeft`).
      pub silent_us: u64,
      /// Virtual time the new host resumed, and at which tick.
      pub resumed_us: u64,
      pub tick: u32,
  }
  // Factory gains (a default method; ToyFactory keeps it):
  /// The driver of a peer that took over: its own slot as before (`own`), and every bot slot (spec §3.7: bots restart).
  fn host_driver(&mut self, peer: PeerId, own: Option<u8>, sim: &S) -> Box<dyn Driver<S>> { self.driver(peer, None) }
  ```

Context:
- **`Migration` rules** (N65, N68, N74):
  - Windows count only `can_host` entries, in order.
  - `step` returns `GiveUp` from `began + GIVE_UP_US`, unless `Host`.
  - An entry whose window comes and is `me` gives `Host`.
  - A contacted successor stays `Try(i)` until `GIVE_UP_US`, or until `lost_contact(i)`.
  - Past the last window comes `Wait`.
- **The harness drives migrations as rl-online will** (when `Scenario::migration` is `Some`; with `None`, everything stays as today, so the plan-9 tests keep their meaning):
  - **The succession:** players in `peers` order, then spectators present from the start (`can_host`), then joined spectators (followers). Former hosts are skipped.
  - **The start:** a peer begins its `Migration` at its `HostSilent` or `PeerLeft { peer: host }` and `suspend`s. When the host was the first host and `host_human` is set, that slot is abandoned.
  - **Attempts** (virtual time, deterministic, using the peer's own `profile`):
    - a successor that is `Host` and alive: contact after one ping, rejoined after three pings (or `slow_joins`' time);
    - a successor alive but not hosting: refused after one ping, tried again `RETRY_US` later;
    - a dead successor, or one across a link in an outage: no answer, until the window moves on.
  - **A rejoin:** `Network::connect(peer, successor, profile)`, the peer's `rehost(successor)`, the successor's `rejoined(peer, seat)` (its commit and `take_over` with the bot slots on the first one, and `host_driver` replacing its driver).
  - **The new host** resumes when `awaiting_reports()` is empty and every expected peer rejoined, or at `window_end`.
  - **Expected peers** are every other succession entry with a world (players and spectators from the start). The new host cannot know who died with the old host, so the harness does not tell it: a dead one costs the wait until `window_end`, as in real play.
  - **Late peers:** a peer reaching the new host after its resume gets `rejoined() == false`; the harness then gives it a new `Role::Joining` session and the host's `add_spectator` (N72), as a spectator joining mid-match.
  - **The old host coming back:** if its datagrams reach a peer that has not committed and has no contact, that peer `unsuspend`s and drops its migration (N69).
  - **Step-down:** a host whose frame gap (virtual time between two of its frames, a `host_freeze` included) reaches `STEP_DOWN_US`, and whose succession has a `can_host` entry, `leave`s and is gone (N70).
  - **Give-up:** `GiveUp` puts the peer in `host_lost`; it runs no more frames.
- **Profile changes and outages** apply to the link between the peer and whoever hosts at that moment; an outage covers links made during it.
- **The run ends** when the current host is past `ticks + settle_ticks` and every remaining peer is `done`, or when no host remains and every peer is lost or exited (then `run` still returns `Ok`).
- **The verdict** compares every surviving peer with the **last** host's log:
  - every exited peer's confirmed rows are a prefix of it (no two peers confirm different inputs);
  - no `Inconsistent`, `Malformed` or `Desync` but corrupted peers' (as today);
  - a late peer is checked from its loaded state (as a joined spectator is).
- `HOST` stays the first host's id. `Report::host_log` is the last host's.

- [ ] **Step 1: Write the failing tests.** Unit tests in `migration.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const S: u64 = 1_000_000;

    #[test]
    fn each_successor_that_can_host_has_its_window() {
        // Entry 1 cannot host (a follower); this peer is entry 3.
        let mut m = Migration::new(vec![true, false, true, true], Some(3), 100);
        assert_eq!(m.step(100), Step::Try(0));
        assert_eq!(m.step(100 + 5 * S - 1), Step::Try(0));
        assert_eq!(m.step(100 + 5 * S), Step::Try(2));
        // Its own window would open at 10 s, when the migration gives up.
        assert_eq!(m.step(100 + 10 * S), Step::GiveUp);
    }

    #[test]
    fn its_own_window_makes_a_peer_the_host_for_good() {
        let mut m = Migration::new(vec![true, true], Some(1), 0);
        assert_eq!(m.step(4 * S), Step::Try(0));
        assert_eq!(m.step(5 * S), Step::Host);
        assert_eq!(m.step(60 * S), Step::Host);
        assert_eq!(m.window_end(5 * S), 10 * S);
    }

    #[test]
    fn a_contacted_successor_is_kept_past_its_window() {
        let mut m = Migration::new(vec![true, true, true], Some(2), 0);
        m.contact(0);
        assert_eq!(m.step(7 * S), Step::Try(0));
        m.lost_contact(0);
        assert_eq!(m.step(7 * S), Step::Try(1));
        assert_eq!(m.step(10 * S), Step::GiveUp);
    }

    #[test]
    fn a_follower_waits_once_the_windows_run_out() {
        let mut m = Migration::new(vec![true, false], Some(1), 0);
        assert_eq!(m.step(0), Step::Try(0));
        assert_eq!(m.step(5 * S), Step::Wait);
        assert_eq!(m.step(10 * S), Step::GiveUp);
        let mut none = Migration::new(vec![], None, 0);
        assert_eq!(none.step(0), Step::Wait);
    }
}
```

  Appended to `tests/migration.rs` (harness scenarios on `ToySim`; slots: 0 the host's player, 1 its bot, then one per client):

```rust
use rl_netcode::harness::{self, Exit, ExitHow, HOST, MigrationPlan, Outage, PeerPlan, ProfileChange, Report, Scenario};
use rl_netcode::migration::{GIVE_UP_US, STEP_DOWN_US, WINDOW_US};
use rl_netcode::netsim::Profile;

fn plan(id: u32, seat: Seat, profile: Profile) -> PeerPlan {
    PeerPlan {
        id,
        seat,
        profile,
        clock_ppm: 0,
        input_delay: if matches!(seat, Seat::Player(_)) { input_delay(profile.rtt_ms()) } else { 0 },
        frame_us: 6_944,
        join_tick: None,
        freeze: None,
    }
}

/// A host (slot 0 its player, slot 1 its bot), `clients` players and one spectator, migration on.
fn mscenario(seed: u64, profile: Profile, clients: u8) -> Scenario {
    let mut peers: Vec<PeerPlan> = (0..clients).map(|c| plan(1 + c as u32, Seat::Player(2 + c), profile)).collect();
    peers.push(plan(100, Seat::Spectator, profile));
    Scenario {
        seed,
        slots: clients + 2,
        peers,
        ticks: 1_500,
        settle_ticks: 200,
        outages: vec![],
        expect_no_stalls: false,
        corrupt: vec![],
        host_freeze: None,
        exits: vec![],
        migration: Some(MigrationPlan { host_human: Some(0), slow_joins: vec![] }),
        profile_changes: vec![],
    }
}

fn run_checked(sc: &Scenario) -> Report {
    let mut f = ToyFactory { slots: sc.slots, seed: sc.seed };
    let r = harness::run(sc, &mut f).unwrap_or_else(|e| panic!("seed {}: {e:?}", sc.seed));
    harness::verdict(sc, &r, &mut f).unwrap_or_else(|e| panic!("seed {} {:?}: {e:?}", sc.seed, sc.peers[0].profile.name));
    r
}

#[test]
fn a_crashed_or_leaving_host_is_replaced_in_every_profile() {
    for profile in Profile::ALL {
        for seed in 1..=12 {
            for how in [ExitHow::Crash, ExitHow::Leave] {
                let mut sc = mscenario(seed, profile, 3);
                sc.exits.push(Exit { peer: HOST, tick: 300 + (seed as u32 * 37) % 400, how });
                let r = run_checked(&sc);
                assert_eq!(r.hosts, vec![HOST, 1], "seed {seed} {how:?}");
                assert_eq!(r.migrations.len(), 1);
                let m = &r.migrations[0];
                let took = m.resumed_us - m.silent_us;
                let fast = matches!(profile.name, "lan" | "good" | "typical");
                assert!(took <= if fast { 3_000_000 } else { WINDOW_US }, "seed {seed} {how:?} {}: {took} µs", profile.name);
                assert!(r.host_lost.is_empty() && !r.exited.contains(&1));
            }
        }
    }
}

#[test]
fn a_leaving_host_is_followed_without_the_two_second_wait() {
    let mut sc = mscenario(3, Profile::GOOD, 2);
    sc.exits.push(Exit { peer: HOST, tick: 500, how: ExitHow::Leave });
    let r = run_checked(&sc);
    let left_us = 500 * 10_000;
    assert!(r.migrations[0].silent_us < left_us + 200_000, "{:?}", r.migrations[0]);
}

#[test]
fn the_second_successor_takes_over_when_the_first_is_dead() {
    let mut sc = mscenario(5, Profile::TYPICAL, 3);
    sc.exits.push(Exit { peer: HOST, tick: 400, how: ExitHow::Crash });
    sc.exits.push(Exit { peer: 1, tick: 400, how: ExitHow::Crash });
    let r = run_checked(&sc);
    assert_eq!(r.hosts, vec![HOST, 2]);
    let m = &r.migrations[0];
    assert!(m.resumed_us >= m.silent_us + WINDOW_US, "the first window passed first");
}

#[test]
fn nobody_reachable_is_host_lost_after_ten_seconds() {
    // One client and a spectator that joined mid-match (a follower): the client dies with the host.
    let mut sc = mscenario(6, Profile::GOOD, 1);
    sc.peers[1].join_tick = Some(100);
    sc.exits.push(Exit { peer: HOST, tick: 400, how: ExitHow::Crash });
    sc.exits.push(Exit { peer: 1, tick: 400, how: ExitHow::Crash });
    let mut f = ToyFactory { slots: sc.slots, seed: sc.seed };
    let r = harness::run(&sc, &mut f).unwrap();
    assert_eq!(r.host_lost, [100].into());
    assert!(r.events[&100].contains(&Event::HostSilent));
    assert!(r.migrations.is_empty());
    // It gave up GIVE_UP_US after its migration began: about 4 s + 2 s + 10 s into the run.
    assert!(r.duration_us >= 4_000_000 + 2_000_000 + GIVE_UP_US && r.duration_us < 4_000_000 + 2_000_000 + GIVE_UP_US + 500_000);
}

#[test]
fn a_host_frozen_past_the_step_down_threshold_hands_over() {
    let mut sc = mscenario(7, Profile::GOOD, 2);
    sc.host_freeze = Some((600, 600 + (STEP_DOWN_US / 10_000) as u32 + 50));
    let r = run_checked(&sc);
    assert!(r.exited.contains(&HOST), "the host stepped down");
    assert_eq!(r.hosts, vec![HOST, 1]);
}

#[test]
fn a_host_frozen_briefly_keeps_hosting() {
    let mut sc = mscenario(8, Profile::GOOD, 2);
    sc.host_freeze = Some((600, 600 + (STEP_DOWN_US / 10_000) as u32 - 40));
    let r = run_checked(&sc);
    assert_eq!(r.hosts, vec![HOST]);
    assert!(r.exited.is_empty() && r.migrations.is_empty());
}

#[test]
fn a_client_outage_does_not_move_the_match() {
    let mut sc = mscenario(9, Profile::TYPICAL, 3);
    sc.outages.push(Outage { peer: 3, from_tick: 500, until_tick: 800 }); // 3 s
    let r = run_checked(&sc);
    assert_eq!(r.hosts, vec![HOST]);
    assert!(r.events[&3].contains(&Event::HostSilent) && r.events[&3].contains(&Event::HostHeard));
}

#[test]
fn a_successor_outage_does_not_take_the_match() {
    let mut sc = mscenario(10, Profile::TYPICAL, 3);
    sc.outages.push(Outage { peer: 1, from_tick: 500, until_tick: 800 });
    let r = run_checked(&sc);
    assert_eq!(r.hosts, vec![HOST]);
    assert!(r.events.values().flatten().all(|e| !matches!(e, Event::Resumed { .. })));
}

#[test]
fn a_late_peer_watches_from_a_state() {
    let mut sc = mscenario(11, Profile::GOOD, 3);
    sc.exits.push(Exit { peer: HOST, tick: 400, how: ExitHow::Crash });
    // Peer 3's join to the successor takes 7 s after contact (slow relays):
    // the successor resumes without it at its window's end.
    sc.migration.as_mut().unwrap().slow_joins.push((3, 7_000_000));
    let r = run_checked(&sc);
    assert_eq!(r.hosts, vec![HOST, 1]);
    let m = &r.migrations[0];
    assert!(m.resumed_us >= m.silent_us + WINDOW_US - 100_000, "it waited for peer 3 until its window's end");
    assert!(r.events[&3].iter().any(|e| matches!(e, Event::StateLoaded { .. })), "it watches from a state");
    assert!(!r.host_lost.contains(&3));
    // Its slot (4) stays Keys(0) from the resume on.
    assert!(r.host_log[m.tick as usize + 50..].iter().all(|row| row[4] == 0));
}

#[test]
fn a_second_migration_skips_the_former_host() {
    let mut sc = mscenario(12, Profile::TYPICAL, 3);
    sc.ticks = 2_500;
    sc.exits.push(Exit { peer: HOST, tick: 400, how: ExitHow::Crash });
    sc.exits.push(Exit { peer: 1, tick: 1_500, how: ExitHow::Crash });
    let r = run_checked(&sc);
    assert_eq!(r.hosts, vec![HOST, 1, 2]);
    assert_eq!(r.migrations.len(), 2);
}

#[test]
fn a_spectator_takes_over_when_no_player_can() {
    let mut sc = mscenario(13, Profile::LAN, 0); // the host and a spectator only
    sc.exits.push(Exit { peer: HOST, tick: 400, how: ExitHow::Crash });
    let r = run_checked(&sc);
    assert_eq!(r.hosts, vec![HOST, 100]);
    let after = &r.host_log[600..900];
    assert!(after.iter().any(|row| row[1] != 0), "the bot plays at the spectator");
}

#[test]
fn profiles_change_mid_run() {
    let mut sc = mscenario(14, Profile::GOOD, 3);
    sc.profile_changes.push(ProfileChange { tick: 300, peer: 2, profile: Profile::AWFUL });
    sc.profile_changes.push(ProfileChange { tick: 900, peer: 2, profile: Profile::GOOD });
    sc.exits.push(Exit { peer: HOST, tick: 600, how: ExitHow::Crash });
    let r = run_checked(&sc);
    assert_eq!(r.hosts, vec![HOST, 1]);
}

#[test]
fn migration_runs_are_deterministic() {
    let mut sc = mscenario(15, Profile::BAD, 3);
    sc.exits.push(Exit { peer: HOST, tick: 500, how: ExitHow::Crash });
    let (a, b) = (run_checked(&sc), run_checked(&sc));
    assert_eq!(a.host_log, b.host_log);
    assert_eq!(a.migrations, b.migrations);
    assert_eq!(a.checksums, b.checksums);
}
```

- [ ] **Step 2: Run `cargo nextest run -p rl-netcode`, expect FAIL** (compile errors: the new fields and API).
- [ ] **Step 3: Implement.** Add the three new `Scenario` fields (`vec![]`, `None`, `vec![]`) to every existing literal (rl-netcode's tests and `crates/rl-cli/src/netsim.rs`): their meaning does not change. If `a_crashed_or_leaving_host_is_replaced_in_every_profile` misses the 3 s bound for `typical`, measure and report; do not loosen it without a ruling (N66).
- [ ] **Step 4: Run `cargo nextest run -p rl-netcode`, expect PASS** (note the added wall time of the 120-run grid: it must stay a few seconds); `cargo build --target wasm32-unknown-unknown -p rl-netcode`; `cargo xtask check`.
- [ ] **Step 5: Update `crates/rl-netcode/CLAUDE.md`** (the `migration` module; the harness's exits, migrations, step-down, profile changes, report fields and verdict). **Commit** `rl-netcode: successor windows and host migration in the harness`.

---

### Task 4: `rl-lobby`: the succession list and the migrated lobby

**Files:**
- Modify: `crates/rl-lobby/src/{host.rs, client.rs, model.rs, harness.rs}`, `crates/rl-lobby/CLAUDE.md`
- Create: `crates/rl-lobby/src/host/migrated.rs` (the migrated lobby's construction and rejoins)
- Test: `crates/rl-lobby/tests/lobby.rs` (appended: rl-lobby's single test binary)

**Interfaces:**
- Consumes: Task 1's `ToHost::Standby`, `ToClient::Succession`, `SuccessorWire`, `valid_succession`, `Refusal::Closed`, `MIN_STANDBY_PORT`, `is_global`.
- Produces (Tasks 5, 6):
  ```rust
  impl LobbyClient {
      /// Reports this member's standby endpoint (sent now if joined, else after the welcome; again after every welcome).
      pub fn standby(&mut self, port: u16, relays: Vec<String>);
      /// The host's latest succession order (empty before the first).
      pub fn succession(&self) -> &[SuccessorWire];
  }
  impl LobbyHost {
      /// The succession order now (N64): players of the match in join order, then its spectators, then waiting members.
      pub fn succession(&self) -> Vec<SuccessorWire>;
      /// A successor's lobby (N72), built from the old lobby's `state` and `succession`: `key` must be a member's in both (else `LobbyError::NotASuccessor`). That member is the host's own (ready, admin); the old host's member is gone; every other succession member is kept with its id, look, seat and admin mark, away until it rejoins; the phase is the state's. No password; strangers are refused `Closed`.
      pub fn migrated(key: Identity, state: &LobbyState, succession: &[SuccessorWire]) -> Result<LobbyHost, LobbyError>;
      /// Members of a migrated lobby that have not rejoined.
      pub fn away(&self) -> Vec<u8>;
      /// The resume (N68): away members leave (`Notice(Left)`, `HostEvent::Left`); their keys may still join, as mid-match joiners.
      pub fn drop_away(&mut self);
  }
  // HostEvent gains: Rejoined(u8): an away member proved its key on a new link (its slot is the match's still).
  // LobbyError gains: NotASuccessor.
  ```

Context:
- **Standby records:** one per member, the latest wins (rl-net's per-peer rate limits bound how many arrive, spec §5.4; the host stores no more than one each, and a change sends one `Succession` per drain at most). Waiting members and members without a world are followers (`can_host: false`).
- **`direct`** (N64): `Some((ip, port))` when the member's address is IPv4, `!is_global(ip)` and `port ≥ MIN_STANDBY_PORT`.
- **Sending:** `Succession` goes to every member with the `Start` (in `announce_start`'s drain, after `Start`), and whenever `succession()` changes while `Starting` or `Playing` (coalesced into the next `outgoing()` drain, like `Lobby`). Never while gathering.
- **Rejoins** (N72): a `Hello` whose key is an away member's goes through the same proofs (challenge, `HostProof`, `Prove`; a migrated lobby has no password). It is welcomed with the member's old id and the current state, then `HostEvent::Rejoined(id)` and `Notice(Joined(id))`. A key in the succession but already dropped by `drop_away` joins as a new member (waiting during the match). A key not in the succession is `Refused(Closed)`. The migrated lobby keeps ids unique: `next_id` starts past the highest kept id, and the running match's ids stay reserved (`in_match`).
- **Never panic** on hostile `Standby` (the decoder bounds it; the host stores at most one per member).

- [ ] **Step 1: Write the failing tests**, appended to `crates/rl-lobby/tests/lobby.rs`:

```rust
/// Starts a match of the host, Ann and Bob (players) and Cid (spectator):
/// Ann and Cid report standbys, Bob none.
fn started_with_standbys() -> (Net, [usize; 3]) {
    let mut net = lobby_with(HostConfig { max_players: 3, max_spectators: 2, ..cfg() }, 1);
    let a = join(&mut net, 1, "Ann");
    let b = join(&mut net, 2, "Bob");
    let c = join(&mut net, 3, "Cid");
    net.client(a).standby(40_001, vec!["wss://a.example".into()]);
    net.client(c).standby(40_003, vec![]);
    for x in [a, b] {
        net.client(x).set_ready(true);
    }
    net.settle();
    net.start(7, 0).unwrap();
    net.settle();
    (net, [a, b, c])
}

#[test]
fn the_succession_lists_players_then_spectators_in_join_order() {
    let (net, [a, b, c]) = started_with_standbys();
    let list = net.host.succession();
    let ids: Vec<u8> = list.iter().map(|s| s.member).collect();
    assert_eq!(ids, vec![id_of(&net, a), id_of(&net, b), id_of(&net, c)]);
    assert_eq!(list[0].direct, Some(([10, 0, 0, 1], 40_001)), "a private address: direct");
    assert_eq!(list[0].relays, vec!["wss://a.example".to_string()]);
    assert!(list[0].can_host && !list[1].can_host && list[2].can_host, "Bob reported no standby");
    assert!(list.iter().all(|s| s.key != key(HOST)), "the host is no successor");
    for x in [a, b, c] {
        assert_eq!(net.peers[x].client.as_ref().unwrap().succession(), &list[..], "client {x}");
    }
}

#[test]
fn the_succession_is_sent_with_the_start_and_on_change_only() {
    let mut net = lobby_with(HostConfig { max_players: 3, ..cfg() }, 1);
    let a = join(&mut net, 1, "Ann");
    net.client(a).standby(40_001, vec![]);
    net.settle();
    let sent = |net: &Net| net.peers[a].received.iter().filter(|m| matches!(m, ToClient::Succession(_))).count();
    assert_eq!(sent(&net), 0, "nothing while gathering");
    net.client(a).set_ready(true);
    net.settle();
    net.start(7, 0).unwrap();
    net.settle();
    assert_eq!(sent(&net), 1);
    net.run(2_000_000);
    assert_eq!(sent(&net), 1, "unchanged: not sent again");
    net.client(a).standby(40_002, vec![]);
    net.settle();
    assert_eq!(sent(&net), 2);
}

#[test]
fn standby_reports_are_bounded() {
    let mut net = lobby_with(HostConfig { max_players: 3, ..cfg() }, 1);
    // A member seen at a global address gets no direct entry.
    let g = net.join(LobbyClient::new(ident(7), look("Gus"), None, None), IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34)));
    net.settle();
    net.client(g).standby(40_007, vec![]);
    net.client(g).set_ready(true);
    net.settle();
    for _ in 0..200 {
        net.send_raw_from(g, ToHost::Standby { port: 40_008, relays: vec!["wss://b.example".into()] });
    }
    net.settle();
    net.start(7, 0).unwrap();
    net.settle();
    let list = net.host.succession();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].direct, None);
    assert!(valid_succession(&list));
    assert!(net.errors.is_empty(), "{:?}", net.errors);
}

#[test]
fn a_migrated_lobby_takes_back_its_members_by_key() {
    let (net, [a, b, c]) = started_with_standbys();
    let state = net.host.state().clone();
    let list = net.host.succession();
    let (ann, bob, cid) = (id_of(&net, a), id_of(&net, b), id_of(&net, c));
    // Ann takes over.
    let mut next = Net::new(LobbyHost::migrated(ident(1), &state, &list).unwrap(), DELAY_US);
    assert_eq!(next.host.state().phase, state.phase);
    assert!(next.host.state().members.iter().all(|m| m.key != key(HOST)), "the old host is gone");
    assert_eq!(next.host.away(), vec![bob, cid]);
    let nb = next.join(LobbyClient::new(ident(2), look("Bob"), None, Some(key(1))), ip(2));
    next.settle();
    assert!(next.host_events.contains(&HostEvent::Rejoined(bob)));
    assert!(got(&next, nb, &ClientEvent::Welcome { you: bob }), "its old id");
    assert_eq!(member_by_id(&next, bob).seat, member_by_id_in(&state, bob).seat, "its seat");
    // A stranger is refused.
    let x = next.join(LobbyClient::new(ident(9), look("Xan"), None, Some(key(1))), ip(9));
    next.settle();
    assert!(got(&next, x, &ClientEvent::Refused(Refusal::Closed)));
    // The resume: Cid did not come back; later it joins as a mid-match joiner.
    next.host.drop_away();
    next.settle();
    assert!(got(&next, nb, &ClientEvent::Notice(Notice::Left(cid))));
    let nc = next.join(LobbyClient::new(ident(3), look("Cid"), None, Some(key(1))), ip(3));
    next.settle();
    assert_eq!(member(&next, nc).seat, SeatWire::Waiting);
    assert!(!next.host_events.contains(&HostEvent::Rejoined(cid)));
    assert!(LobbyHost::migrated(ident(9), &state, &list).is_err(), "not a successor");
    consistent(&next);
}
```

  Add the helpers these tests use: `Net::send_raw_from(c, msg)` in `harness.rs` (a member's own message after its welcome, encoded and delivered like its client's) and `member_by_id_in(state, id)` in the test file. Extend the module's proptest (random messages and host actions) with `Standby` messages, and with `drop_away`/rejoins on a migrated lobby, keeping every sent message decodable.
- [ ] **Step 2: Run `cargo nextest run -p rl-lobby`, expect FAIL.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run `cargo nextest run -p rl-lobby`, expect PASS; `cargo build --target wasm32-unknown-unknown -p rl-lobby`; `cargo xtask check`.**
- [ ] **Step 5: Update `crates/rl-lobby/CLAUDE.md`** (standby records, the succession, the migrated lobby). **Commit** `rl-lobby: the succession order and the migrated lobby`.

---

### Task 5: `rl-online`: migration over direct links

**Files:**
- Modify: `crates/rl-net/src/{endpoint.rs, core.rs}` (`EndpointConfig::connect_timeout`, `NetEvent::Answered`) and its CLAUDE.md; `crates/rl-online/src/{lib.rs, peer.rs, events.rs, matches.rs, join.rs}`; every `PeerRole::Client { .. }` in the workspace (rl-online's tests, `crates/rl-cli/src/transport.rs`, `crates/rl-cli/src/net_client_cmd.rs`, `crates/rl-game/src/shell/online.rs` and rl-game's tests) gains `migration`
- Create: `crates/rl-online/src/migration.rs` (`MigrationConfig`, `MigrationStep`, the texts), `crates/rl-online/src/peer/migrate.rs` (the client's migration driver and the promotion)
- Test: `crates/rl-online/tests/migration.rs` (a `mod migration;` of `tests/integration.rs`), `crates/rl-net` unit tests

**Interfaces:**
- Consumes: Tasks 2-4 (session API, `Migration`, constants, lobby API); `Joiner`, `Outbox`, `HostReach`, `ContentSide`, `HostBots`.
- Produces (Tasks 6-8):
  ```rust
  // rl_net::endpoint
  pub struct EndpointConfig { /* … */ pub connect_timeout: Duration /* default CONNECT_TIMEOUT */ }
  pub enum NetEvent { /* … */ /// Client side: the host answered our offer (direct or signalled): it is there. Answered }
  // rl_online::migration
  /// A direct join to a successor gives up after this long (N73).
  pub const MIGRATION_CONNECT_TIMEOUT: Duration = Duration::from_secs(1);
  pub const HOST_LOST: &str = "The host left and no other player could take over the game.";
  pub fn stepped_down(secs: f32) -> String; // "Your game stopped for {secs:.1} s, so the match moved on without you."
  /// How a client takes part in a host migration (spec §3.7).
  #[derive(Clone, Debug)]
  pub struct MigrationConfig {
      /// Its standby endpoint (N63): `port` is forced to 0 and `ice.port_mapping` off. `None`: it never hosts (N79).
      pub standby: Option<EndpointConfig>,
      /// The relays it listens on once it hosts, and the first 2 it reports; `None`: no Nostr (direct joins only).
      pub relays: Option<PoolConfig>,
      /// Its joins to a successor through Nostr: their ICE (STUN, TURN), region and TURN notes (as a `NostrJoin`'s).
      pub ice: IceConfig,
      pub region: Region,
      pub turn_from_nostr: bool,
      pub allow_local: bool,
      /// Off: the host's loss ends this peer as before (tests that need the old ending).
      pub enabled: bool,
  }
  impl MigrationConfig {
      /// No migration (the old ending).
      pub fn off() -> MigrationConfig;
      /// A standby on port 0 without STUN, no relays: LAN and loopback games, tests.
      pub fn direct() -> MigrationConfig;
  }
  // `MigrationConfig`, `MigrationStep`, `HOST_LOST`, `stepped_down` and `MIGRATION_CONNECT_TIMEOUT` are re-exported at the crate root, as `Join` is.
  #[derive(Clone, Debug, PartialEq, Eq)]
  pub enum MigrationStep { Finding, Moving(String /* the successor's name */), TakingOver }
  impl std::fmt::Display for MigrationStep {} // "The host is gone: finding who takes over…", "The host is gone: moving the game to {name}…", "The host is gone: you are taking over the game…"
  // rl_online::PeerRole::Client gains `migration: MigrationConfig`.
  // rl_online::OnlineEvent gains:
  Migrating(MigrationStep),
  /// The match resumed under `host` (a name; `me`: this peer hosts now).
  Migrated { host: String, me: bool },
  /// No successor took over (`HOST_LOST`), or this host stepped down (`stepped_down`): the match is over for this peer; its replay follows (`ReplayReady`).
  HostLost(String),
  // rl_online::matches
  /// The bots of `start` restarted on `world` (spec §3.7, N75): `Bot::with_cache` per bot slot, a new lazy area cache, the match's seed.
  pub fn restart_bots(start: &MatchStartWire, world: &World) -> anyhow::Result<HostBots>;
  // rl_online::OnlinePeer gains:
  pub fn standby_port(&self) -> Option<u16>;
  ```

Context (N63-N76; this task: direct links only, Task 6 adds Nostr):
- **Standby** (N63): on `Welcome`, a client with `migration.enabled` and `standby: Some` opens `Endpoint::host` (port 0, port mapping off, `max_peers` `MAX_MEMBERS`), whose gate refuses `NotHost` until it hosts (a shared `AtomicBool`; the lobby's banned set behind it once it hosts). It then sends `LobbyClient::standby(port, first 2 relays)`. A bind failure is a log line and no standby (N79). The standby is polled every poll and closed at `shutdown`.
- **Start** (client, during a match only, N71): the session's `HostSilent`, its `PeerLeft { peer: HOST }`, or the endpoint's `Disconnected { HOST }`.
  - The session is suspended, and the old endpoint is kept (still polled, its messages still fed to the session: `HostHeard`).
  - A `Migration` starts from `LobbyClient::succession()`: `can_host` as listed, `false` for former hosts; `me` = this member's entry; `MAX_MIGRATIONS` counted per match.
  - `OnlineEvent::Migrating(Finding)`.
  - With `migration.enabled` false, or no succession, it ends as before (`Disconnected`).
- **`Try(i)`:** one attempt at a time (a `Joiner` with `Join { direct: entry.direct, nostr: None }` and `connect_timeout: MIGRATION_CONNECT_TIMEOUT`, `expected_host: entry.key`, a fresh `LobbyClient`).
  - `NetEvent::Answered` is contact.
  - A `Refused(NotHost)` is tried again after `RETRY_US`.
  - A failed direct connect is not tried again within the window (N73).
  - When the step moves on, the attempt is dropped (its endpoint shut down off the thread).
  - `Migrating(Moving(name))`.
- **Rejoin (commit):** at the attempt's `Welcome` (the new link's lobby):
  - The old endpoint is shut down on a thread, and the attempt's endpoint becomes the outbox's.
  - The `LobbyClient` is replaced; `session.rehost(HOST, now)` follows.
  - The content side takes the new host as its source (`Fetcher::source_left`, then its lists).
  - The standby is sent again (`standby()` after every welcome).
  - At `Resumed`: `OnlineEvent::Migrated { host, me: false }` and `Connected { path, remote }` for the new link.
- **`Host`** (this peer's turn): its standby's gate opens and `Migrating(TakingOver)` is reported. It commits at the first `HostEvent::Rejoined` of a `LobbyHost::migrated` built from its client lobby's state and succession, or at once when it expects nobody (N68). The commit:
  - shuts the old endpoint down and makes the standby the outbox's endpoint;
  - turns the side into a `HostSide` (the migrated lobby, an empty ban set, `HostReach` with `HostListen { pool: migration.relays, public_address: None }`, which Task 6 completes);
  - makes the content side a host's (same content and cache), whose list goes to each rejoined member after its welcome;
  - calls `session.take_over(bot slots)` and sets `Game.bots = restart_bots(..)`;
  - calls `session.rejoined(peer, seat)` for each rejoined member, with `seat` from the `MatchStartWire` (the member's slot, else `Spectator`), after its welcome is drained (so `Collect` follows `Welcome` on the ordered channel).
- **Resume:** when `awaiting_reports()` is empty and `away()` is empty, or at `window_end`, it calls `drop_away()`, `session.resume`, `lobby.playing(tick)`, then `OnlineEvent::Migrated { host: my name, me: true }`. From then on it is an ordinary host: it sends the succession and the end, and makes its replay.
- **Cancel** (N69): `HostHeard` before the commit, with no contact, closes the attempt (or the standby's gate and the listener), `unsuspend`s, keeps the old endpoint, and reports no event beyond a log line.
- **Host lost:**
  - `GiveUp`, or a host that nobody rejoined by `GIVE_UP_US` while expecting someone, ends the peer. Its replay comes from its confirmed rows (`record_of(session, start, frontier)`, the existing `make_replay`), then `OnlineEvent::HostLost(HOST_LOST)` and `ReplayReady` later.
  - A spectator that joined mid-match has no rows from tick 0 and makes none (as today).
- **Step-down** (N70):
  - **When:** each poll on a host during a match measures the gap since its previous poll on the monotonic clock and on `SystemTime` (the larger counts). A gap ≥ `STEP_DOWN_US`, with a `can_host` entry in its succession, steps it down. The check comes first in `poll`, before anything is received or sent, so its peers never hear it again but for the `Bye`.
  - **What:** `session.leave()` (Bye to every peer, transmitted at once), every member's link closed after `CLOSE_LINGER`, and the lobby back to gathering without them.
  - **Game host:** `HostLost(stepped_down(secs))` and its replay.
  - **Server role:** the same event (rl-server logs it), and its lobby stays open.
- **Never block:** endpoints start without waiting (rl-net's connect thread), old ones shut down on threads; `restart_bots` builds an empty area cache.

- [ ] **Step 1: Write the failing tests.**
  - **rl-net:** `answered_comes_before_connected` (a direct join's client sees `Answered`, then `Connected`); `the_connect_timeout_is_the_configs` (a connect to a held, silent loopback port with `connect_timeout: 300 ms` ends within 300-800 ms; rl-net's `connect_stall` hook).
  - **rl-online, `crates/rl-online/tests/migration.rs`:** reuse `tests/online.rs`'s `Net`/`Peer` (move them to `tests/common/net.rs`, a `pub mod` of `common`, so both modules share them). Add a `frozen: bool` to `Peer` (`Net::poll` skips a frozen peer: alive, sockets open, silent), `Net::join_with_migration(name, MigrationConfig)`, and `Net::host_index()` (the peer whose `is_host()` holds now). Clients use `MigrationConfig::direct()` (port 0, no STUN: NT-1). Each test under 20 s, every wait bounded:

```rust
//! Host migration over real connections on 127.0.0.1 (spec §3.7): the
//! successor takes over, the others rejoin it, the match ends and the
//! records agree.

use std::time::{Duration, Instant};

use rl_online::{HOST_LOST, MigrationConfig, MigrationStep, OnlineEvent, StartError};

use crate::common::net::{Net, content, ended, replay_ready};

fn migrated(e: &OnlineEvent) -> bool {
    matches!(e, OnlineEvent::Migrated { .. })
}

/// A host and two clients (Anna joins first: the first successor) start a 0.15-minute match.
fn started(content: rl_content::Content) -> (Net, [usize; 2]) {
    let mut net = Net::new(content, 0.15);
    let a = net.join_with_migration("Anna", MigrationConfig::direct());
    let b = net.join_with_migration("Bert", MigrationConfig::direct());
    net.run_until(Duration::from_secs(10), "both joined", |n| n.members() == 3);
    net.ready(&[a, b]);
    net.host().peer().lobby().start(7).unwrap();
    net.run_until(Duration::from_secs(10), "the match runs", |n| n.host_tick() > 300);
    (net, [a, b])
}

#[test]
fn a_frozen_host_hands_the_match_to_the_first_player() {
    let Some(content) = content() else { return };
    let (mut net, [a, b]) = started(content);
    let host_rows = net.host().peer().match_session().unwrap().frontier();
    net.peers[0].frozen = true;
    let froze = Instant::now();
    net.run_until(Duration::from_secs(8), "both resume under Anna", |n| {
        n.peers[a].has(|e| matches!(e, OnlineEvent::Migrated { me: true, .. }))
            && n.peers[b].has(|e| matches!(e, OnlineEvent::Migrated { host, me: false } if host == "Anna"))
    });
    // N66: 2 s of silence, then at most 3.5 s on loopback with real clocks.
    assert!(froze.elapsed() < Duration::from_millis(2_000 + 3_500), "{:?}", froze.elapsed());
    assert!(net.peers[b].has(|e| matches!(e, OnlineEvent::Migrating(MigrationStep::Moving(n)) if n == "Anna")));
    assert!(net.peers[a].peer().is_host());
    net.run_until(Duration::from_secs(20), "the match ends and both make replays", |n| {
        [a, b].iter().all(|&i| n.peers[i].has(ended) && n.peers[i].has(replay_ready))
    });
    let ra = net.peers[a].peer().last_record().cloned().unwrap();
    let rb = net.peers[b].peer().last_record().cloned().unwrap();
    assert_eq!(ra.rows, rb.rows);
    assert_eq!(ra.checksums, rb.checksums);
    // The frozen host's confirmed inputs are a prefix of the match (spec §3.2: final is final).
    let old = net.peers[0].p.as_ref().unwrap().match_session().unwrap();
    for t in 0..host_rows {
        assert_eq!(Some(ra.rows[t as usize]), old.input_log().row(t), "tick {t}");
    }
    // The old host's player stands still after the migration; the bot (slot 3) still plays.
    let after = &ra.rows[host_rows as usize + 300..];
    assert!(after.iter().all(|row| row[0] == 0));
    assert!(after.iter().any(|row| row[3] != 0), "the bot moved to Anna");
    assert!(!net.peers[a].has(|e| matches!(e, OnlineEvent::DesyncBundle(_))));
    assert!(!net.peers[b].has(|e| matches!(e, OnlineEvent::DesyncBundle(_))));
    // Back in Anna's lobby: Anna is its host, the old host is no member.
    net.run_until(Duration::from_secs(10), "the results, then gathering", |n| {
        n.peers[b].peer().lobby().state().is_some_and(|s| s.phase == rl_proto::Phase::Gathering)
    });
    let host_key = net.peers[0].key;
    let state = net.peers[b].peer().lobby().state().cloned().unwrap();
    assert!(state.members.iter().all(|m| m.key != host_key));
    assert!(!matches!(net.peers[a].peer().lobby().can_start(), Err(StartError::NotHost)));
    assert!(matches!(net.peers[b].peer().lobby().can_start(), Err(StartError::NotHost)));
}

#[test]
fn a_leaving_host_is_followed_at_once() {
    let Some(content) = content() else { return };
    let (mut net, [a, b]) = started(content);
    net.host().peer().leave(); // its Byes go out with leave's own endpoint poll
    let left = Instant::now();
    net.peers[0].frozen = true;
    net.run_until(Duration::from_secs(5), "both resume", |n| n.peers[a].has(migrated) && n.peers[b].has(migrated));
    assert!(left.elapsed() < Duration::from_millis(1_500), "no 2 s wait (N76): {:?}", left.elapsed());
}

#[test]
fn a_stalled_host_steps_down_and_the_match_moves() {
    let Some(content) = content() else { return };
    let (mut net, [a, b]) = started(content);
    net.peers[0].frozen = true;
    std::thread::sleep(Duration::from_millis(1_700)); // nobody polls: the host's own stall
    net.peers[0].frozen = false;
    net.run_until(Duration::from_secs(8), "the host steps down and the match moves", |n| {
        n.peers[0].has(|e| matches!(e, OnlineEvent::HostLost(t) if t.starts_with("Your game stopped")))
            && n.peers[a].has(migrated)
            && n.peers[b].has(migrated)
    });
    assert!(net.peers[0].peer().match_session().is_none(), "no match left on the old host");
    // Its replay of what it confirmed is made, as at any end.
    net.run_until(Duration::from_secs(5), "the old host's replay", |n| n.peers[0].has(replay_ready));
}

#[test]
fn nobody_to_take_over_is_host_lost_with_a_replay() {
    let Some(content) = content() else { return };
    let mut net = Net::new(content, 0.5);
    let a = net.join_with_migration("Anna", MigrationConfig::direct());
    let b = net.join_with_migration("Bert", MigrationConfig { standby: None, ..MigrationConfig::direct() });
    net.run_until(Duration::from_secs(10), "both joined", |n| n.members() == 3);
    net.ready(&[a, b]);
    net.host().peer().lobby().start(7).unwrap();
    net.run_until(Duration::from_secs(10), "the match runs", |n| n.host_tick() > 200);
    net.peers[0].frozen = true;
    net.peers[a].frozen = true; // Anna, the only successor, is gone too
    let froze = Instant::now();
    net.run_until(Duration::from_secs(14), "Bert gives up", |n| n.peers[b].has(|e| *e == OnlineEvent::HostLost(HOST_LOST.into())));
    let took = froze.elapsed();
    assert!(took >= Duration::from_secs(12) && took < Duration::from_millis(13_000), "{took:?}");
    net.run_until(Duration::from_secs(5), "its replay", |n| n.peers[b].has(replay_ready));
}

#[test]
fn a_successors_own_outage_does_not_take_the_match() {
    // N69: Anna's own link is cut for 3 s while the host is fine. Anna finds
    // the host silent, opens its standby (its turn: the first successor),
    // nobody comes, the link returns: Anna cancels and stays a client.
    let Some(content) = content() else { return };
    let (mut net, [a, b]) = started(content);
    net.outage(a, Duration::from_secs(3)); // test hook: drop everything on Anna's endpoint both ways
    net.run_until(Duration::from_secs(8), "Anna is back with the host", |n| {
        n.peers[a].has(|e| matches!(e, OnlineEvent::Migrating(_))) && n.peers[a].peer().match_session().is_some_and(|s| !s.suspended())
    });
    assert!(!net.peers[a].has(migrated) && !net.peers[b].has(migrated));
    assert!(net.host().peer().is_host());
}
```

  `Net::outage(i, d)` uses a test hook `OnlinePeer::test_outage(until: Instant)` (feature `test-hooks`): the peer's endpoints drop every datagram both ways until then (rl-net's impairment stage already sits on the socket path: extend `Impair` with a timed blackout). The last test must fail if `HostHeard` does not cancel: without it, Anna stays suspended.

- [ ] **Step 2: Run `cargo nextest run -p rl-online migration -p rl-net`, expect FAIL.**
- [ ] **Step 3: Implement.** Update every `PeerRole::Client` construction:
  - `MigrationConfig::direct()` in rl-online's existing tests;
  - `off()` in any test that expects a host leaving mid-match to end its clients (find them by running the suite);
  - rl-cli and rl-game get their real configs in Tasks 7 and 8: until then `MigrationConfig::direct()`.
- [ ] **Step 4: Run `cargo nextest run -p rl-online -p rl-net -p rl-cli -p rl-game`, expect PASS; `cargo xtask check`.**
- [ ] **Step 5: Update `crates/rl-online/CLAUDE.md` and `crates/rl-net/CLAUDE.md`. Commit** `rl-online: host migration over direct links`.

---

### Task 6: `rl-online`: migration through the relays, late rejoiners, repeated migrations

**Files:**
- Modify: `crates/rl-online/src/{peer/migrate.rs, reach.rs, join.rs, migration.rs}`, `crates/rl-online/CLAUDE.md`
- Test: `crates/rl-online/tests/migration.rs` (appended)

**Interfaces:**
- Consumes: Task 5; plan 12b's `NostrJoin`, `join_relays`, `HostReach`, `TestRelay` (`rl-discovery/test-relay`, already a dev-dependency).
- Produces: `OnlinePeer::test_ignore_direct()` (feature `test-hooks`: this peer's joins to successors skip direct addresses, so a loopback test exercises the relays).

Context (N73, N72, N74):
- **Relays:** a client whose `migration.relays` is `Some` listens on them from the moment its standby's gate opens (`HostReach::start` with `HostListen { pool: relays, public_address: None }`; its answers through `Endpoint::answer_signalled` on the standby), and stops at a cancel.
- **The joins:**
  - An attempt's `Join` is `{ direct: entry.direct, nostr: Some(NostrJoin { host: entry.key, pool: join_relays(&entry.relays, own relays), region, turn_from_nostr, allow_local }) }`, its endpoint config `migration.ice` with `connect_timeout: MIGRATION_CONNECT_TIMEOUT`.
  - Without relays on either side: direct only.
  - The joiner's answer (JoinStep `Connecting`) is contact.
- **Late rejoiners** (N72): a client whose attempt is welcomed after the new host's resume is welcomed as `Waiting`. It drops its old `Game` (its session) and becomes a mid-match joiner: `ContentComplete` → `Start` → `Role::Joining`, then it watches from a state. Its old slot stays `Keys(0)`.
- **Repeated migrations** (N74): former hosts get `can_host: false`. The peer counts migrations per match; past `MAX_MIGRATIONS` it is host lost.
- **Hostile successions** (Review Focus 3): direct ports below `MIN_STANDBY_PORT` cannot arrive (the decoder refuses them). A failed direct connect is not retried in its window, and each window has at most one attempt at a time. Relays per entry are capped at 2, and a join's pool at 8 (`join_relays`).

- [ ] **Step 1: Write the failing tests** (appended to `tests/migration.rs`; a `TestRelay` on 127.0.0.1; `allow_local: true`; no STUN):
  - `a_successor_is_reached_through_the_relays`:
    - setup: Anna's and Bert's `MigrationConfig { relays: Some(pool on the test relay), allow_local: true, ..direct() }`; Bert has `test_ignore_direct()`;
    - expected: after the host freezes, Bert's `Joining` steps go `Relays`, then `Asking`, and it is welcomed by Anna; both resume within 2 s + 6 s; the records match at the end; Anna reports `ListenRelay { status: "connected" }` only after the freeze (she listens only once her turn comes), and Bert's path line names the new link.
  - `a_late_rejoiner_watches_from_a_state`:
    - setup: Bert's endpoint is blacked out from 0.2 s before the freeze until 7 s after it (`test_outage`); Anna resumes without Bert at her window's end;
    - expected: Bert later rejoins as `Waiting`, then watches (a `StateLoaded`), and the next match's start gives it a seat; Anna's record shows Bert's slot `0` from the resume.
  - `a_second_migration_moves_the_match_again`:
    - setup: three clients; the host freezes, then Anna (the new host) freezes 4 s after the resume;
    - expected: Bert hosts; the third client follows; the records of Bert and the third client agree; Anna was skipped as a former host in the second succession (`Migrating(Moving("Bert"))` first, never "Anna").
  - `a_hostile_succession_is_bounded` (a unit test of `peer/migrate.rs`'s attempt planner with a fake clock):
    - setup: a succession of 48 entries with direct addresses on a loopback port where a TCP listener refuses every connection, and 2 relays each pointing to a closed loopback port;
    - expected over 10 s: at most one direct connect per window per entry, at most 8 relays in a pool, no attempt past `GIVE_UP_US`, and `HostLost`.
- [ ] **Step 2: Run `cargo nextest run -p rl-online migration`, expect FAIL.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run `cargo nextest run -p rl-online`, expect PASS; `cargo xtask check`.**
- [ ] **Step 5: Update `crates/rl-online/CLAUDE.md`** (a "Migration" section: standby, start, attempts, commit, promotion, resume, cancel, host lost, step-down, relays, late rejoiners, limits). **Commit** `rl-online: migration through the relays; late rejoiners; repeated migrations`.

---

### Task 7: `rl-cli` and `rl-server`: migration in net-sim, real transport and `net-client`

**Files:**
- Modify:
  - `crates/rl-cli/src/{netsim.rs, net_sim_cmd.rs, transport.rs, net_client_cmd.rs, main.rs}`;
  - `crates/rl-cli/tests/{net_sim.rs, transport.rs, cli.rs}`;
  - `crates/rl-server/src/lib.rs`, `crates/rl-server/tests/server.rs`;
  - the CLAUDE.md of rl-cli and rl-server.
- Test: as listed

**Interfaces:**
- Consumes: Tasks 3, 5, 6 (`Scenario`'s new fields, `Factory::host_driver`, `MigrationConfig`, `OnlineEvent::{Migrating, Migrated, HostLost}`, `matches::restart_bots`).
- Produces (Task 10):
  ```rust
  // rl_cli::netsim::NetSpec gains (all #[serde(default)], so kept cases load):
  /// `(peer, host tick, "crash" | "leave")`: peers leaving (peer 0 the host).
  pub exits: Vec<(u32, u32, String)>,
  /// The host frozen from tick `.0` for `.1 - .0` ticks' worth of time.
  pub host_freeze: Option<(u32, u32)>,
  /// `(host tick, peer, profile)`: from then, that peer's link to whoever hosts takes the profile.
  pub profile_changes: Vec<(u32, u32, String)>,
  /// Host migration on (spec §3.7); `exits` naming the host and `host_freeze` turn it on too.
  pub migration: bool,
  // rl_cli::transport
  #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
  pub enum ServerExitHow { Freeze, Leave }
  #[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
  pub struct ServerExit { pub after_s: f32 /* after the first match started */, pub how: ServerExitHow }
  // TransportSpec gains `pub server_exit: Option<ServerExit>` (default None); ClientRun gains
  pub migrated: Vec<String>,          // "Anna hosts", in order
  pub host_lost: Option<String>,
  ```
  `net-sim` gains `--exit peer:tick:crash|leave` (repeatable), `--host-freeze from:until`, `--profile-change tick:peer:profile` (repeatable) and `--migration`. `net-client` gains `--no-standby`; its standby asks the `--stun` servers, and it listens on the `--relay`s once it hosts. It prints `migrated: <name> hosts` (or `migrated: hosting`) and `host lost: <why>` (exit 1).

Context:
- **net-sim** (`netsim.rs`): `WorldFactory::host_driver` keeps the peer's own bot and restarts the others with `matches::restart_bots` semantics (a fresh area cache on the current world). In net-sim every worm is a bot, so `host_human` is `None`. The scenario's succession is the clients in order, then the spectators. A failure still writes `target/failures/net-<stem>.ron`.
- **Transport** (`transport.rs`): the server thread's observer freezes it (`ServerExit::Freeze`: it stops polling and stays alive until the run ends, holding its sockets: a hung host) or sets `stop` (`Leave`: `Server::shutdown` says `Bye`), `after_s` after the first match started.
  - Clients use `MigrationConfig::direct()` (loopback).
  - The verdict, with a server exit:
    - every client finished its match under the successor;
    - their confirmed rows, checksums and replays agree;
    - the server's confirmed rows before the exit are a prefix (taken from the observer's last view);
    - no desync bundle.
- **rl-server:** it logs `succession: <n> players can take over` once per match start (from `LobbyHandle`'s succession). On `HostLost(stepped_down)` it logs `stalled for <s> s: the match moved to the next player; the lobby is open again`. The server sends succession lists and steps down through rl-online; nothing else changes.

- [ ] **Step 1: Write the failing tests:**
  - `tests/net_sim.rs`:
    - `a_host_crash_migrates_a_real_match` (check: Classic on Castle Strike, 8 slots, 3 clients, 1 spectator, `typical`, the host crashes at tick 400 of 1000; `ok`, `hosts` = [0, 1]);
    - `a_frozen_host_steps_down_in_a_real_match` (check, `good`, a 1.7 s host freeze);
    - `net_migration_grid` (full: 5 profiles × crash, leave and a 2 s freeze, seed 1, 2000 ticks; MW 1.0 for the freezes).
  - `tests/transport.rs` (full tier only, `RL_FULL_TESTS`):
    - `a_frozen_server_hands_its_match_to_a_client` (3 clients, `good`, 0.3-minute match, freeze 8 s in);
    - `a_leaving_server_hands_its_match_to_a_client` (`bad`);
    - each about 25 s.
  - `tests/cli.rs`: `net_sim_flags_parse_and_write_a_replayable_case` (`--exit 0:300:crash` with a planted fault writes a case that `--from` replays to the same failure; a malformed `--exit` is an error). `net_client`'s unit tests: `--no-standby` and the printed lines.
  - `crates/rl-server/tests/server.rs`: `the_server_logs_its_succession` (two loopback clients, `MigrationConfig::direct()`: the log line counts 2).
- [ ] **Step 2: Run `cargo nextest run -p rl-cli --test net_sim --test integration -p rl-server`, expect FAIL.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run `cargo nextest run -p rl-cli -p rl-server`, expect PASS. Also run the full-tier tests once by hand, timed: `RL_FULL_TESTS=1 cargo nextest run -p rl-cli --test net_sim net_migration_grid` and `RL_FULL_TESTS=1 cargo nextest run -p rl-cli --test transport a_` (record the times for Task 11). Then `cargo xtask check`.**
- [ ] **Step 5: Update the CLAUDE.md of rl-cli and rl-server. Commit** `rl-cli: migration in net-sim, real transport and net-client`.

---

### Task 8: The game: migration lines, the new host's lobby, the Host lost screen

**Files:**
- Modify:
  - `crates/rl-ui/src/{state.rs, screens/mod.rs, online.rs}`, `crates/rl-ui/tests/online.rs`;
  - `crates/rl-game/src/{shell/online.rs, net.rs, overlay.rs}`, `crates/rl-game/tests/online.rs`;
  - the CLAUDE.md of rl-ui and rl-game.
- Create: `crates/rl-ui/src/screens/host_lost.rs`
- Test: `crates/rl-ui/tests/online.rs` (kittest), `crates/rl-game/tests/online.rs`

**Interfaces:**
- Consumes: Task 5's events and `MigrationConfig`; the settings' relays, STUN, TURN and region (`relays_of`, `turn_of`, `local_relays`, `Shell::test_ice`).
- Produces:
  ```rust
  // rl_ui
  pub enum Screen { /* … */ HostLost }
  pub enum UiAction { /* … */ HostLost(String) /* the app reports it: the screen opens with the reason */ }
  // MenuData gains `pub host_lost: Option<String>`; the screen shows "Host lost", the reason, "The match's replay is saved in Replays.", buttons "Replays" (the Replays screen) and "Back" (Multiplayer).
  pub const MIGRATED_LISTING: &str = "Taken over from the host: only this match's players can join";
  // rl_game::Shell (test-hooks): pub fn test_no_standby(&mut self) — its next client never hosts (N79).
  ```

Context (N81):
- **The client's config:** `MigrationConfig { standby: Some(EndpointConfig { port: 0, ice: IceConfig { stun: settings', port_mapping: false, allow_local, .. }, .. }), relays: the settings' relays (none: `None`), ice: the join's (STUN, the player's TURN), region, turn_from_nostr: true, allow_local: local_relays, enabled: true }`, built like the joins' (`shell/joining.rs`). Tests: `online::menus` sets no relay, no STUN and no port mapping, so the standby is loopback-only (NT-1).
- **In the match:**
  - `Migrating(step)` sets the HUD's top line (where "Connection problem" shows), cleared at `Migrated`.
  - `Migrated { host, me }` logs "<host> is now hosting" or "You are now hosting" in the HUD log and on the console (`rl-game: migration: …`).
  - When `me`: `Net::host = true`, no announcer, no `Hosted` (the migrated game is closed: N72), `LobbyView::listing = MIGRATED_LISTING`.
- **Host lost:** `HostLost(why)` leaves the match view at once (as `lost` does), releases the peer (its replay goes to `Shell::replays` and is saved by `auto_save`), and reports `UiAction::HostLost(why)`, not `OnlineFailed`.
- The new host's lobby is the ordinary host's: Start, Force start, kick and the setup.

- [ ] **Step 1: Write the failing tests.**
  - **kittest** (`rl-ui/tests/online.rs`):
    - `the_host_lost_screen_says_why_and_goes_back` (the labels "Host lost" and the reason are visible; Replays opens the Replays screen; Back opens Multiplayer);
    - `a_migrated_lobby_shows_host_controls` (a `LobbyView { host: true, listing: Some(MIGRATED_LISTING) }`: Start is there, the listing text is visible).
  - **rl-game** (`tests/online.rs`, through `Shell::net_frame` and `App::network_frame` as the app runs them):
    - `a_frozen_server_hands_the_match_to_the_first_player`:
      - setup: an in-process `rl-server` (port 0, `port_mapping: false`, no STUN, one bot) and two `Shell`s joining it; the server's poll thread stops (frozen) 3 s into the match;
      - expected: Shell A's HUD line reads "The host is gone: you are taking over the game…", then its log says "You are now hosting"; Shell B's log says "<A's name> is now hosting"; both reach the results with equal confirmed scores; both replays are saved and equal; A's lobby view is the host's;
      - bounded: under 25 s.
    - `the_host_lost_screen_shows_and_the_replay_is_saved`:
      - setup: the server and Shell A frozen; Shell B with `test_no_standby()`;
      - expected: B's screen becomes `Screen::HostLost` within 2 s + 10 s + 1 s, with `HOST_LOST`; a replay file appears in its data directory's `replays/`.
  - **Then `cargo xtask e2e`** (the autoplay online run must still pass: the game's client now opens a standby; nothing else changes).
- [ ] **Step 2: Run `cargo nextest run -p rl-ui -p rl-game`, expect FAIL.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run, expect PASS; `cargo xtask check`; `cargo xtask e2e`.** Bless nothing unless a HUD image changed (then `cargo xtask bless-render --reason "<why>"`).
- [ ] **Step 5: Update the CLAUDE.md of rl-ui and rl-game. Commit** `rl-game: migration lines, the new host's lobby and the Host lost screen`.

---

### Task 9: The Docker NAT tests: a migration across routers

**Files:**
- Modify: `xtask/src/nat.rs` (a `migration` scenario; the scenario count), `tools/net-nat/CLAUDE.md`, `xtask/CLAUDE.md`
- Test: `nat.rs`'s unit tests; the scenario itself

**Interfaces:**
- Consumes: Task 7's `net-client` (`migrated:` lines, its standby asking `--stun`, listening on `--relay` once it hosts), plan 12b's topology (`rl-nat-wan`, LANs A-D, the WAN relay and STUN at 203.0.113.10, the TURN relay at 203.0.113.20).
- Produces: `cargo xtask net-nat --only migration`; `full`'s `net-nat` step runs it with the others.

Context (PL12c-2):
- **The scenario `migration`:**
  - an `rl-server` Invite in LAN A (one fill bot, a 0.5-minute match that starts when 2 players are ready);
  - `net-client --bot --name succ` in LAN C;
  - `net-client --bot --name other` in LAN B (symmetric NAT; TURN notes used, no `--no-nostr-turn`), both `--invite <code> --relay ws://203.0.113.10:7777 --stun 203.0.113.10:3479 --allow-local --matches 1`.
  - Once both print a start, the runner waits 8 s, then `docker kill`s the server's container: a crash, silence.
- **Expect:**
  - `succ` prints `migrated: hosting`; `other` prints `migrated: succ hosts`;
  - both exit 0, and their printed `checksum` lines are equal;
  - `other`'s path after the migration is `relayed` (it is behind the symmetric NAT);
  - the relay's log shows an allocation;
  - bounded at 90 s.
- A failure prints the last 40 lines of each container's log, as the others do.

- [ ] **Step 1: Write the failing unit tests** in `nat.rs`: `the_migration_scenario_is_listed_and_selectable`; `checksum_lines_are_compared` (the parser that collects `checksum <tick> <hex>` lines from two outputs and names the first difference).
- [ ] **Step 2: Run `cargo nextest run -p xtask nat`, expect FAIL.**
- [ ] **Step 3: Implement**, then run `cargo xtask net-nat --only migration` (Docker is installed here). A failure is a real finding about Tasks 5-7: fix it there, with a test in rl-online or the harness that reproduces it.
- [ ] **Step 4: `cargo xtask net-nat --only migration` passes; `cargo xtask check`.**
- [ ] **Step 5: Update `tools/net-nat/CLAUDE.md` (the scenario, its timing) and `xtask/CLAUDE.md`. Commit** `xtask: a host migration across routers in the Docker NAT tests`.

---

### Task 10: `net-soak`

**Files:**
- Create: `crates/rl-cli/src/soak.rs`, `crates/rl-cli/tests/net_soak.rs` (a `mod net_soak;` of `tests/integration.rs`)
- Modify: `crates/rl-cli/src/{lib.rs, main.rs}`, `xtask/src/{net.rs, main.rs}`, the CLAUDE.md of rl-cli and xtask, `README.md` (the commands list, if it lists xtask's)

**Interfaces:**
- Consumes: Task 7's `NetSpec` fields, `netsim::{run, write_failure}`, `transport::{TransportSpec, ServerExit, run_transport}`; `rl_netcode::Rng`.
- Produces:
  ```rust
  // rl_cli::soak
  #[derive(Clone, Debug)]
  pub struct SoakConfig {
      pub minutes: f64,
      /// The first case's seed; case `n` is seeded `seed + n`.
      pub seed: u64,
      /// Every this many runs one is a real-transport case (0: none).
      pub transport_every: u32,
      /// Stop after this many runs (tests), else when the time is up.
      pub runs: Option<u32>,
      /// Stop after this many failures (`MAX_FAILURES`, 32).
      pub max_failures: usize,
      pub failures: PathBuf,
      pub content_dir: PathBuf,
  }
  pub const MAX_FAILURES: usize = 32;
  /// The net-sim case of `seed` (N77): its draws, all within NetSpec's bounds.
  pub fn case(seed: u64) -> NetSpec;
  /// The real-transport case of `seed`: 2-4 clients, good/typical/bad, a 0.3-0.5-minute match, a server freeze or departure at a random time in half of them.
  pub fn transport_case(seed: u64, data_dir: &Path) -> TransportSpec;
  #[derive(Clone, Debug, Default, PartialEq)]
  pub struct SoakSummary { pub runs: u32, pub transport_runs: u32, pub failures: Vec<PathBuf> }
  /// One case of the soak.
  pub enum SoakCase { Net(NetSpec), Transport(TransportSpec) }
  /// Runs cases until the time or `runs` is up, one line each to `out`: `run <n> seed <s> <stem>: ok (<secs>s)` or `FAIL <what> at tick <t> (<file>)`; then the summary line. A case that cannot even run (an error) is a failure too, kept like the others.
  pub fn run(cfg: &SoakConfig, out: &mut dyn Write) -> anyhow::Result<SoakSummary>; // = run_cases with `case` and `transport_case`
  /// `run` with the cases from `cases(n, seed)` (tests).
  pub fn run_cases(cfg: &SoakConfig, cases: &mut dyn FnMut(u32, u64) -> SoakCase, out: &mut dyn Write) -> anyhow::Result<SoakSummary>;
  ```
  `TransportSpec` and `ServerExit` derive `Serialize`/`Deserialize` (if they do not yet), so a transport failure keeps its spec as RON.
  `rl-cli net-soak [--minutes 10] [--seed 1] [--transport-every 5] [--runs N] [--content DIR] [--failures DIR]` exits 1 when a case failed. `cargo xtask net-soak [--minutes N] [--seed S] [--offline]` builds rl-cli in release and runs it on `content/packs` with `--failures target/failures`, streaming its output; it is **not** a `full` step (N77).

Context:
- **Cases** (N77, spec §9.10), all draws from `rl_netcode::Rng::new(seed)`:
  - the mod and level (both mods, the 4 bundled levels);
  - 2-16 slots; 1-min(slots, 7) clients; 0-2 spectators (one may join mid-match);
  - a base profile; 0-3 profile changes; 0-2 outages of 0.5-6 s;
  - in half of the cases one of a host crash, a host departure or a 1.6-2.5 s host freeze, at a random tick in the middle 60 % of the match;
  - 1500-6000 ticks.

  `case(seed)` is a pure function: the same seed, the same case.
- **Bounded:** each case's report is dropped before the next. Failures are files (at most `MAX_FAILURES`, then the soak stops with a summary). A real-transport failure keeps its spec (RON) and the server's log in `target/failures/soak-transport-<seed>/`. Nothing else accumulates (no per-run state kept), so an overnight run's memory stays flat. Pin it: `tests/net_soak.rs` runs the same short case 8 times and compares the process's `VmRSS` after the 2nd and the 8th (Linux; skipped elsewhere), within 16 MiB.
- **Nothing leaves the machine:** net-sim is virtual; transport cases run on 127.0.0.1 with `MigrationConfig::direct()`.

- [ ] **Step 1: Write the failing tests** `crates/rl-cli/tests/net_soak.rs`:
  - `cases_are_deterministic_and_within_bounds` (`case(s) == case(s)` for 200 seeds, each `NetSpec::scenario()` is `Ok`, at least one of each exit kind among them, every profile named);
  - `a_short_soak_runs_and_reports` (`runs: Some(3)`, `transport_every: 0`, cases shortened to 600 ticks through a `#[doc(hidden)] pub fn case_short(seed)`, every line printed, the summary `3 runs`, no failure);
  - `a_failing_case_is_kept_and_counted` (`run_cases` with `runs: Some(4)` and a case function whose third case names a mod that does not exist: the soak goes on with the fourth; the failures directory holds that case as a `NetCase` naming the error; the summary counts 1 failure; with `max_failures: 1` it stops right after it, `runs` = 3);
  - `memory_stays_flat_over_cases` (as above).
  - `xtask`'s unit test: `net_soak_args_are_parsed` (`--minutes 0.5 --seed 9` reach rl-cli's command line).
- [ ] **Step 2: Run `cargo nextest run -p rl-cli --test integration net_soak -p xtask`, expect FAIL.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run, expect PASS. Then run `cargo xtask net-soak --minutes 3` once: every case passes, or each failure is a finding fixed in its task with a kept case (`cargo xtask bless-regression --from target/failures/net-*.ron --reason "<why>"`). Then `cargo xtask check`.**
- [ ] **Step 5: Update the CLAUDE.md of rl-cli and xtask. Commit** `rl-cli: net-soak; xtask: cargo xtask net-soak`.

---

### Task 11: Records, and the plan's one `full` run

**Files:**
- `docs/porting/STATUS.md`: a new section "Host migration decisions (plan 12c)" holding:
  - N63-N81 and PL12c-1, PL12c-2, as decided here and as amended by rulings during execution;
  - the measured times: a migration's resume after detection per profile (harness), the loopback rl-online test's, and the added `full` time.
- `CHANGELOG.md` (Unreleased, "Added — host migration"):
  - a match goes on when its host crashes, leaves or freezes;
  - "Host lost" with the replay;
  - "You are now hosting";
  - `rl-cli net-sim --exit/--host-freeze/--profile-change`, `net-client --no-standby`, `cargo xtask net-soak`.
- `docs/online.md`, a section "When the host leaves":
  - what the players see;
  - how long it takes (LAN about 2.5 s, through the relays up to about 8 s);
  - who takes over (the first player who joined, then the next);
  - why a host whose game froze for 1.5 s loses that match (N70);
  - that the migrated game takes back only its players (N72);
  - "Host lost" and where its replay is;
  - the playtest recipe:
    1. three PCs: host, join, join;
    2. start a match;
    3. close the host's game window (or pull its network cable) mid-match;
    4. both others should see "moving the game to …" and play on;
    5. at the results, both replays should be equal;
    6. if not, keep both PCs' `desyncs/` folders.
- `crates/*/CLAUDE.md` reconciled with the code: rl-proto, rl-netcode, rl-lobby, rl-net, rl-online, rl-server, rl-cli, rl-game, rl-ui, xtask, `tools/net-nat`.
- The spec's status line: "plan 12c implemented".

- [ ] **Step 1: Write the records.** Run `cargo xtask check`.
- [ ] **Step 2: Run `cargo xtask full` (the plan's only full run).** Every step must pass. Record its duration and the new steps' share in STATUS. A failure is fixed in the task that owns the code (with a test that reproduces it), then `full` runs again.
- [ ] **Step 3: Commit** `docs: plan 12c's records`.
