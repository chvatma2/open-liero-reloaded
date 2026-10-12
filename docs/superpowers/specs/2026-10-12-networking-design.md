# Networking: Design

- **Date:** 2026-10-12
- **Status:** Draft, awaiting the user's review
- **Builds on:** `2026-10-10-foundation-and-local-play-design.md` (§12.2, the
  networking constraints), `2026-10-11-performance-envelope-design.md` (the
  rollback headroom).
- **Scope:** sub-project 4 (Networking) in full: the netcode, direct and LAN
  play, the lobby, content download, the serverless game list, NAT
  traversal, host migration and the browser build. Implemented in plans
  9-13 (§11).

## 1. Purpose

The user's words: online play with **strangers, without a single master
server**, if technically feasible. OLX's community died when its master
server went down. It is feasible (§6): games are announced on public Nostr
relays run by many independent operators, found on the LAN by broadcast,
or joined by address or invite code.

### 1.1 What the user decided

| # | Decision |
|---|---|
| U1 | One spec for all four steps (netcode and direct connect, NAT traversal, the serverless game list, the browser), built in several plans. |
| U2 | **Rollback netcode, the host orders inputs** (§3). |
| U3 | Hosting from the game's menus, a headless dedicated server, and host migration. |
| U4 | Missing content is downloaded from the host (§8). |
| U5 | Text chat, kick and ban; spectators; bots in online games. |
| U6 | **No joining a match in progress**: it cannot be fair with the scoring and lives. Players and spectators join in the lobby between matches; a spectator who arrives mid-match watches it and plays from the next one. |
| U7 | A multiplayer lobby with chat and the match settings. |
| U8 | Public game list on **Nostr relays**, plus LAN broadcast, direct address and invite codes (§6). |
| U9 | **Volunteer TURN relays** when no direct path connects (§5.3). |
| U10 | The browser build does **everything**: local play against bots, joining, hosting, spectating (§10). |
| U11 | The design centres on a fast, automated feedback loop for networking, so bugs are found by tests with reproducible seeds, not by playtests (§9). |

### 1.2 Constraints carried over

- No central master server dependency (any single server of ours may
  vanish without ending online play).
- Browser clients must be supported (WebRTC data channels, WebSocket).
- No network compatibility with OLX.
- Movement, physics and shooting feel identical to OLX: your own worm
  reacts with no added delay beyond the chosen input delay (§3.2).
- Up to 32 players (`MAX_WORMS`).
- The simulation stays `no_std`, WASM-clean and deterministic; inputs are
  the only means of change (`World::tick(&[Keys], &mut TickEvents)`).
- Never panic on content or on network input.

## 2. Architecture

The simulation is unchanged in behaviour. A new layer around it exchanges
inputs, rewinds and re-simulates, and connects the peers.

| Crate | Layer | Job | WASM |
|---|---|---|---|
| `rl-proto` | 4, `no_std` | The wire format: versioned messages with size limits, decoding that never panics; the simulation fingerprint (§2.2). | yes |
| `rl-netcode` | 4, `no_std` | The rollback session (§3): tick stamps, input prediction, snapshots, re-simulation, the host's input ordering, checksum comparison, desync repair, host migration. **Sans-IO**: no sockets and no clock; it takes messages and a time and returns messages and ticks to run. | yes |
| `rl-net` | 5 | Transport and connection setup (§5): WebRTC data channels, the handshake over a host's WebSocket, LAN or Nostr, UPnP/NAT-PMP/PCP, STUN and TURN; the network impairment layer (§9.4). | yes (WebRTC, WebSocket and Nostr parts) |
| `rl-discovery` | 5 | Game announcements and lookup on Nostr relays, LAN broadcast, invite codes (§6). | yes (Nostr, invite codes) |
| `rl-server` | 6 | The headless dedicated server (§7.4), optionally a TURN relay. | no |

`rl-netcode` and `rl-proto` are a same-layer pair in `xtask/layers.ron`;
both depend on `rl-sim` and `rl-modes`. `rl-ui` gains the multiplayer
menus and the lobby (§7); `rl-game` the online session; `rl-cli` the
test commands (§9).

### 2.1 One transport: WebRTC

Every peer connection is a WebRTC peer connection with two data channels:

- **inputs:** unreliable, unordered (redundancy replaces retransmission,
  §3.1);
- **control:** reliable, ordered (lobby, chat, settings, checksums,
  snapshots, content).

Reason: a browser can host (U10), so native peers must speak WebRTC
anyway; one transport is one code path to test. WebRTC brings encryption
(DTLS) and NAT traversal (ICE). The native stack is chosen in plan 10
after a short spike, preferring a sans-IO library (such as `str0m`) so the
transport can also run on the virtual clock of §9.1; the browser uses its
own `RTCPeerConnection`.

### 2.2 Version matching

Rollback needs bit-identical simulations. Every build carries a
**simulation fingerprint**: a hash, computed at build time, of the source
of the simulation crates (`rl-math`, `rl-data`, `rl-terrain`, `rl-sim`,
`rl-modes`) and the protocol version. Games announce it; a client only
joins a game with its own fingerprint, and the game list shows other
versions greyed out ("needs version X").

## 3. The netcode

### 3.1 Inputs and ticks

- The game keeps OLX's 100 ticks per second (`FRAME_MS` 10).
- An input is one `Keys` byte per worm per tick.
- Every input packet repeats all of the sender's inputs not yet
  acknowledged (at most the rollback window), so a lost packet costs
  nothing and nothing is resent.
- Budget: at most 24 KB/s received per client at 32 players, packet
  overhead included, independent of projectiles and effects (a packet may
  carry two ticks if overhead requires it).

### 3.2 The host orders inputs

- Topology: a **star**. Every peer connects to the host only; the host
  relays.
- The host forwards each input as it arrives. **What the host forwards for
  (player, tick) is final**: it is the single source of truth.
- **Input delay:** each client applies its own input `d` ticks after it
  was pressed: `d = min(3, floor(RTT / 40 ms))` from its round-trip time
  to the host (30 ms → 0, 80 ms → 2, 150 ms → 3), the formula tuned by
  §9.2's numbers. The user can pin it in the options. Own-worm response is therefore immediate apart from
  `d`, never waiting for the network.
- **Time sync:** the host tells each client, in every packet, how early
  or late its inputs arrive; the client runs up to 1 % faster or slower
  to keep a small positive margin.
- **The lagging-player rule:** the **rollback window** is 12 ticks
  (120 ms). If player P's input for tick T has not reached the host when
  the host's own simulation reaches T + 12, the host fills it with P's
  previous input and forwards that; P's late real input is dropped and P
  is told. P sees their own worm corrected; nobody else ever waits. A
  client whose inputs miss the window for 3 s is shown a "connection
  problem" warning; after 10 s without any packet it is dropped to the
  lobby.
- A tick is **confirmed** on a peer when it holds every player's final
  input for it.

### 3.3 Prediction and rollback

- Other worms are predicted to keep their last final input.
- When a final input differs from the prediction, the peer restores the
  snapshot before it and re-simulates to the present.
- Snapshots: one per tick for the window (13). `World` and the game mode
  are cloned **except the terrain**, whose edits go into an **undo
  journal** (the changed pixels' old values per tick); restoring replays
  the journal backwards. A snapshot must cost in proportion to what
  changed, not to the map size.
- Budget: restore plus 12 re-simulated ticks within 2 ms p99 at the
  reference configuration (16 players, Castle Strike, Classic) and within
  4 ms at the envelope's 4096 × 4096 point, without bots' thinking (bots
  think on the host only, §3.6). Measured in plan 9 with `cargo xtask
  envelope`'s method and added to the envelope.

### 3.4 Effects and sounds during rollback

- Sounds and cosmetic effects play at once. Every event is keyed by its
  tick and source (worm, projectile slot, kind); a re-simulated event
  with a key already played does not play again.
- An event that a correction erases is not undone; its effect finishes.
- The scoreboard, kill and death messages, lives and the end of the match
  use **confirmed** ticks only (at most about 120 ms later).
- Rendering always draws the newest predicted tick.

### 3.5 Desyncs

A desync can only be a bug of ours (non-deterministic code). Still:

- every 10 confirmed ticks each peer sends the host its checksum
  (`World::checksum`); the host's is the reference;
- on a mismatch the host sends that peer its world state (encoded by
  `rl-sim`, checksum-verified after decoding), and the peer continues from
  it;
- both sides save a **desync bundle**: the match's start state, every
  final input up to the mismatch, both checksum streams, the build
  fingerprint and the platform. `rl-cli net-repro` reads it (§9.6).

### 3.6 Bots, spectators, replays

- **Bots** think on the host only. Their keys are sent as ordinary final
  inputs, so no peer re-simulates their route-finding.
- **Spectators** receive the final inputs and simulate confirmed ticks
  only (no prediction, no rollback). A spectator who arrives mid-match
  first receives the world state.
- Every peer saves the match as an `rl-replay` replay of the final inputs.

### 3.7 Host migration

- The host keeps a **succession order** (the order peers joined; players
  before spectators) and sends it to everyone.
- When the host is silent for 2 s, peers stop advancing past their
  confirmed tick and connect to the first live successor through its
  handshake path (§5.1); the successor becomes the host.
- The new host collects from every peer its furthest confirmed final
  inputs (identical where they overlap, since they came from one source),
  takes the longest, sends each peer what it lacks, and resumes. Bots move
  to the new host and restart their thinking.
- Target: the match continues within 3 s. If no successor is reachable
  within 10 s, peers return to a local "host lost" screen with the replay
  saved.
- A dedicated server that crashes is migrated away from in the same way.

## 4. Match flow

1. **Lobby** (§7.2): players, spectators and bots gather; the host sets
   the match; content downloads (§8); players mark ready.
2. **Start**: the host sends the match setup (mod, level, mode, settings,
   seed, worm slots and skins). Every peer builds the same `World`; the
   start state's checksum is compared before tick 0.
3. **Match**: §3.
4. **End**: decided on a confirmed tick; everyone returns to the lobby
   with the scoreboard; spectators take free slots for the next match in
   join order.

## 5. Connections

### 5.1 The handshake (WebRTC signalling)

| Path | Who | How |
|---|---|---|
| Direct | native → native host | The host listens on TCP and UDP port 23400 (configurable); the handshake runs over a WebSocket on it. Browsers cannot use it (an `https` page may not open an unencrypted socket). |
| LAN | native, same network | Hosts broadcast an announcement (address, key, game summary) on UDP 23401 every 2 s. |
| Nostr | everyone | Short-lived encrypted messages addressed to the host's key through the relays (§6.1). |

### 5.2 Identity

- Each install creates a key pair on first start (the Nostr key type, so
  one key signs announcements and identifies the player).
- The player's name is free; the lobby shows a short fingerprint of the
  key beside it.
- Joining proves possession of the key (the host's challenge, signed).
- Kick and ban by key, optionally by IP. Keys are free to make, so bans
  are soft.

### 5.3 Through routers

In order:

1. **Port mapping:** a native host asks the router to open its port
   (UPnP-IGD, NAT-PMP, PCP).
2. **Hole punching:** ICE with public STUN servers (an editable list from
   several operators).
3. **Volunteer TURN relays:** `rl-server --relay` announces itself on
   Nostr with its region and capacity; peers try the nearest few. Each
   relay limits every user to about 32 KB/s and a few sessions, so it
   cannot serve as a free general proxy.
4. **Failure:** a message naming the step that failed and the fix (a
   dedicated server, port forwarding, another relay).

### 5.4 Security

- WebRTC encrypts every connection.
- Every message type has a size limit; every peer a rate limit; a peer
  breaking either is dropped.
- Network input never panics (fuzzed, §9.3).
- A host cannot make a client run code: mods are data, loaded by the
  existing untrusted-content loaders.
- Optional game password, checked by the host with a challenge.

## 6. Finding games

### 6.1 Nostr

- **Announcement:** a signed, replaceable note per game: name, mode, map,
  mod (its content hash), players and spectators, maximums, simulation
  fingerprint, password flag, self-chosen region, handshake hints. It
  carries an expiry 3 minutes ahead and is re-published every minute, so
  a dead game vanishes.
- **Relays:** a default list of about 8 relays from different operators,
  chosen in plan 12 by measured behaviour (accept the kinds, keep notes
  for minutes, no payment); editable in the options; anyone, the user
  included, can run one.
- **The browser list:** reads the same relays; hides notes with bad
  signatures or another fingerprint; rate-limits each key; lets the user
  block a key; sinks games that never answer. Ping is measured when a game
  is selected.
- The event kinds are chosen in plan 12 from Nostr's unassigned ranges
  (replaceable for announcements, ephemeral for handshakes).

### 6.2 LAN

§5.1: native hosts broadcast; the LAN tab lists them with ping.

### 6.3 Invite codes and links

- A short code (`rlx1…`, checksummed) holds the host's key, relay hints
  and optionally a direct address.
- The same code in a link to the web build (`…/#rlx1…`) opens it and
  joins.
- A game is **public** (announced), **LAN only** or **invite only**, with
  or without a password.

## 7. Menus, lobby, server

### 7.1 Menus

- The main menu gains **Play online**: a game browser with tabs
  **Internet**, **LAN** and **Direct** (address or invite code).
- **Host game**: name, visibility (public, LAN, invite only), password,
  maximum players (up to 32), relays. Then the lobby.
- Every page uses the centred menu frame.

### 7.2 The lobby

- **Players:** name and key fingerprint, skin, team, ping, ready and
  spectator marks. The host adds bots, kicks, bans, moves players to
  spectators and back.
- **Chat:** the typed-line log the HUD uses; it continues into the match
  (T opens it, configurable). Kill messages, joins and leaves appear in
  the HUD log as in OLX.
- **Match settings:** Local Play's Map and Match panels; editable by the
  host, read-only and live for everyone else.
- **Start:** when every player is ready (content complete is a condition
  of ready), or forced by the host.

### 7.3 In the match

- A network overlay (toggled; ping, input delay, rollback depth, loss,
  corrections per second).
- A player who loses the connection sees the warning of §3.2 and is
  returned to the lobby screen if dropped.

### 7.4 The dedicated server

- `rl-server --config server.ron`: name, visibility, password, ports,
  relays and STUN servers, a map rotation (mod, level, match settings),
  bots to fill empty slots, admin keys.
- Admins use chat commands: `/kick`, `/ban`, `/next`, `/settings`.
- Logs to the terminal and to a file.
- `--relay` also runs a volunteer TURN relay (§5.3).
- No window, no GPU; it simulates only to run bots and check checksums.

## 8. Content download

- On joining, the host sends the **content list** of the match: the mod
  pack, the level pack and every player's skin, each by the hash of its
  content.
- The client looks each one up in its own content and its download cache,
  and downloads what is missing from the host over the control channel in
  resumable chunks.
- Every download is verified against its hash, then loaded with the
  existing loaders (untrusted content).
- Limits: a prompt above 50 MB per game; a 1 GB cache dropping the least
  recently used items; a skin at most 64 KB.
- The player is in the lobby while downloads run; Start waits for their
  ready mark.
- Packs are transferred in the native format (`content/packs`), never OLX
  originals.

## 9. Testing: the feedback loop

The aim (U11): nearly every networking bug is a failing test with a seed
and a one-line command that reproduces it, found in the agentic loop
rather than in a playtest. Everything that decides behaviour is sans-IO
(`rl-netcode`, `rl-proto`, ideally the WebRTC stack), so whole online
matches run in one process on a virtual clock, deterministically, in
seconds.

### 9.1 The simulated network (`rl-netcode` tests, `rl-cli net-sim`)

- N peers (host, players, spectators, bots) in one process with a virtual
  clock; every link has its own latency, jitter, loss, reordering,
  duplication and outages.
- **Profiles:** `lan` (1 ms), `good` (30 ms), `typical` (80 ± 20 ms, 1 %
  loss), `bad` (150 ± 50 ms, 5 % loss, reordering), `awful` (300 ms,
  15 % loss, bursts); events: disconnects, reconnects, the host leaving,
  spectators arriving.
- **The verdict** (the oracle): once the network settles, every peer's
  confirmed checksums are identical and equal to an offline replay of the
  final inputs.
- **Invariants checked every tick:** no two peers confirm different final
  inputs; own input applied after exactly `d` ticks; a player within the
  window never stalls; migration keeps the match; spectators equal
  players; message size and rate limits hold.
- `check` runs a few hundred seeds (seconds); `full` thousands. A failure
  writes `target/failures/net-<seed>.ron` (the seed, profile, peers and
  events); `rl-cli net-sim --from <file>` replays it exactly, and
  `cargo xtask bless-regression --from` keeps it as a golden case.

### 9.2 Feel numbers (gated)

Per profile, from §9.1 runs with 16 bots: rollback depth (p50, p99),
other worms' visible correction distance (p99, in pixels), own-worm
corrections per minute, stalls, bytes per second per client, the
re-simulation time of §3.3. Committed baselines in `fixtures/net/`,
gated in `full` against measured noise, re-blessed with a reason like the
benchmarks. Absolute rules: zero stalls for players within the window in
every profile up to `bad`; bandwidth within §3.1.

### 9.3 The wire format

Random and mutated bytes into every decoder (never panic, bounded
memory), and encode-decode round trips: short in `check`, long in `full`.

### 9.4 Real transport on one machine

`rl-server` and 8 headless bot clients (`rl-cli net-client --bot`) over
real WebRTC on localhost, through an impairment layer in `rl-net` using
the profiles of §9.1. Checks the verdict, replays and checksums. In
`full`.

### 9.5 Cross-platform determinism

The golden matches' checksum streams (`fixtures/matches/`) computed on
Linux, in WASM and on Windows (the interop runner of plan 8) must be
identical. WASM in `check`, Windows in `full`. This guards the
assumption rollback rests on (the simulation's `libm` maths).

### 9.6 Desync bundles

`rl-cli net-repro <bundle>` replays a bundle (§3.5) and names the first
diverging tick and the first differing part of the world, as `rl-cli
scenario` does for the oracle. A test plants deliberate nondeterminism
behind a test-only hook and checks the bundle pinpoints it. Bundles from
real games are sent to the developer and become tests.

### 9.7 NAT and discovery in Docker

A Docker topology: two simulated home routers (one port-restricted cone
NAT, one symmetric NAT that defeats hole punching), a router with UPnP, a
local Nostr relay and an `rl-server --relay`. Scenarios: direct, port
mapping, hole punching, relay fallback, announcing and finding a game,
invite codes, a browser host. `cargo xtask net-nat`, in `full`; skipped
with a notice without Docker.

### 9.8 The browser

Headless Chromium in Docker runs the web build: it joins a native server,
hosts a game that native bot clients join, and plays a local match.
`cargo xtask web-e2e`, in `full`.

### 9.9 Menus and lobby

Lobby flows (host, join, chat, settings, ready, kick, ban, spectate,
content download progress) driven by label with `egui_kittest` against an
in-process host, in `check`.

### 9.10 Playtest aids and soak

- The network overlay (§7.3).
- `rl-game --net-sim <profile>` starts a host and a client on one machine
  with a chosen profile.
- Every desync in a real game saves a bundle.
- `cargo xtask net-soak [--minutes N]`: long runs of §9.1 and §9.4 with
  changing profiles, disconnects and host departures, for overnight runs.

## 10. The browser build

- The same game compiled to `wasm32-unknown-unknown`: wgpu on WebGPU with
  a WebGL2 fallback; Web Audio; the Gamepad API; the same menus and HUD.
  Settings, replays and the content cache in the browser's storage.
- **Content:** the packs are served as separate files beside the game and
  fetched on demand (core, the chosen mod and level, skins), then cached.
  Players can import their own OLX folder through the browser's folder
  picker (the importers compile to WASM).
- **Hosting:** handshakes over Nostr only. The host's netcode runs in a
  Web Worker (background tabs slow timers on the main thread); the tab
  warns while it is hidden. No port mapping: hole punching and relays
  only.
- **Distribution:** a folder of static files with no server logic, served
  by any static host and mirrored by anyone. `cargo xtask web` builds it;
  `cargo xtask web --serve` serves it locally. Invite links open it and
  join.
- No dependence on browser threads (they need special server headers);
  the simulation runs on one thread already.
- The performance envelope gains a browser column in plan 13.

## 11. Plans

| Plan | Delivers (each ends playable or testable on its own) |
|---|---|
| 9 | `rl-proto`, `rl-netcode` (§3) with the snapshot undo journal, world encoding, the simulated network (§9.1), feel numbers (§9.2), fuzzing (§9.3), cross-platform determinism (§9.5), desync bundles (§9.6), `rl-cli net-sim` and `net-repro`. |
| 10 | `rl-net` with WebRTC (spike first, §2.1), direct and LAN play, the lobby and menus (§7), chat, spectators, bots, kick and ban, `rl-server` (§7.4), real-transport tests (§9.4), `--net-sim` and the overlay. |
| 11 | Content download (§8). |
| 12 | `rl-discovery`: Nostr announcements and handshakes, invite codes, port mapping, hole punching, volunteer relays (§5-6), host migration (§3.7), the Docker NAT tests (§9.7), `net-soak`. |
| 13 | The browser build (§10), including hosting and local play, and its tests (§9.8). |

## 12. Risks

| Risk | Mitigation |
|---|---|
| The simulation differs across platforms (floats, WASM) | `libm` everywhere already; §9.5 compares checksum streams on Linux, WASM and Windows from plan 9. |
| Snapshots too slow on large maps | The terrain undo journal (§3.3); a measured budget in plan 9. |
| Native WebRTC libraries are young | A spike in plan 10 compares the candidates against §9.4 before committing; the transport sits behind `rl-net`'s interface. |
| Public Nostr relays drop notes, block kinds or ask for payment | About 8 relays from different operators, measured before choosing; an editable list; re-publishing every minute; anyone can run a relay. |
| Some NATs defeat hole punching | Volunteer relays (§5.3), a dedicated server, clear failure messages; §9.7 tests both NAT kinds. |
| Browser background throttling breaks a browser host | The host's netcode in a Web Worker; a warning while hidden; §9.8 tests a browser host. |
| Rollback corrections feel worse than OLX's own-worm authority for laggy players | Adaptive input delay; the feel numbers (§9.2) with gates; the overlay for playtests. |
| Spam or fake games on public relays | Signatures, fingerprint filtering, per-key rate limits, blocking, sinking unanswered games. |

## 13. Out of scope

- Joining a match in progress (U6).
- Voice chat.
- Accounts, rankings, matchmaking by skill.
- Anti-cheat beyond what rollback gives (no aim-bot detection).
- Network compatibility with OLX.
- Mobile and touch controls.
