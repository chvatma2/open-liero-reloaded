# Plan 12b: Through Routers (Nostr Handshakes, ICE, Port Mapping, TURN Relays) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Two players behind ordinary home routers play each other with no manual port forwarding, and each side can see which path connected them.
- The host asks its router to open its port (UPnP-IGD, NAT-PMP, PCP).
- A joiner reaches a game it found on the Internet tab, or through an invite code without an address, by an encrypted Nostr handshake (kind 21470).
- ICE then tries host, server-reflexive (STUN) and relayed (TURN) candidates.
- Volunteer TURN relays (`rl-server --relay`) announce themselves on Nostr.
- The lobby and the server log say which path was used: direct, mapped port, hole-punched or relayed. A failure names the step that failed and the fix.

**Architecture:**
- **`rl-net`** (spec §2: "UPnP/NAT-PMP/PCP, STUN and TURN" live here):
  - `Endpoint` is split into a sans-IO `Core` (links, limits, candidates, STUN on the game socket, TURN client, path classification) and its socket driver.
  - A virtual NAT network, `natsim` (feature `test-hooks`), runs `Core`s behind simulated routers on a virtual clock, so hole punching and relaying are seeded tests in `check` (spec U11).
  - `portmap` maps ports; `turn` is the TURN server core and its driver.
- **`rl-discovery`** gains:
  - NIP-44 v2 encryption;
  - the handshake events and their sans-IO state machines (`Inbox` for the host, `Asker` for the joiner);
  - TURN relay notes (kind 31471) and their list.
- **`rl-online`** (new same-layer pair with `rl-discovery`) owns:
  - the join order (direct, then Nostr);
  - the host's Nostr listener;
  - the reachability report and the path and failure texts.
- **`rl-server`** gains: port mapping, Nostr handshakes, `--relay`, `--nat-check`.
- **`rl-game`/`rl-ui`** gain: the lobby lines, path tags, Options and the joins without an address.
- **`cargo xtask net-nat`** runs the spec §9.7 Docker topology in `full`.

**Tech Stack:** Rust 1.99 (edition 2024); existing crates (str0m `=0.24.0`, tungstenite `=0.30.0`, k256 `=0.14.0`, sha2 0.11, serde_json, rustls, postcard). New exact pins, each released at least two weeks before 2026-10-14 (checked with crates.io on 2026-10-14):

| Crate | Pin | Released | Why |
|---|---|---|---|
| `turn-client-proto` | `=0.7.2` | 2026-08-05 | sans-IO TURN client (RFC 5766/8656), pure Rust, `no_std` |
| `turn-server-proto` | `=0.7.2` | 2026-08-05 | sans-IO TURN server, pure Rust |
| `igd-next` | `=0.17.1` | 2026-06-01 | UPnP-IGD client: its sync API and attohttpc (without TLS) are unconditional; default features are empty, and only `aio_tokio` is optional (not used) |
| `netdev` | `=0.46.3` | 2026-09-13 | default interface's IPv4 and default gateway (netlink / windows-sys); `default-features = false, features = ["gateway"]` (`get_default_interface` and `Interface::gateway` need `gateway`) |
| `hkdf` | `=0.13.0` | 2026-03-30 | NIP-44 (digest 0.11, matches sha2 0.11); not yet in the lock or the local registry (only 0.12.x): fetched once |
| `hmac` | `=0.13.0` | 2026-03-29 | NIP-44; not yet in the lock or the local registry: fetched once |
| `chacha20` | `=0.10.2` | 2026-08-27 | NIP-44 (already in the lock) |
| `base64` | `=0.22.1` | 2024-04-30 | NIP-44 payload; not in the lock today: it enters with igd-next (attohttpc) and stun-proto in Tasks 3-4, so all three share 0.22.1 |

- `k256` gains feature `ecdh`.
- Transitive crates newer than two weeks are pinned back in `Cargo.lock` with `cargo update -p <crate> --precise <older>`, as plan 10 did for str0m. Known on 2026-10-14:
  - `stun-proto` 2.0.3 → `2.0.2`;
  - `stun-types` 2.0.3 → `2.0.2`;
  - `turn-types` 0.7.3 → `0.7.2`.

  Record every pin-back in the crate's `CLAUDE.md`.
- `stun-types` 2.0.2, `hkdf` 0.13.0 and `hmac` 0.13.0 are not in the local registry: the first build of Tasks 3 and 6 needs the network once (never `--offline` there). Everything else above is cached.

**Spec:** `docs/superpowers/specs/2026-10-12-networking-design.md` (approved; plans 9-11 and 12a implemented). This plan covers spec §11's plan 12 row as split by ruling PL12-1:
- §5.1's Nostr handshake row;
- §5.3 (port mapping, hole punching, volunteer TURN relays, failure messages);
- §5.4 (limits on the new inputs);
- §6.1's ephemeral handshake kind;
- §7.4's `--relay`;
- §9.7 (the Docker NAT tests).

Not here:
- host migration and `net-soak` (plan 12c);
- the browser and §9.7's "a browser host" scenario (plan 13, ruling PL12b-3).

## Global Constraints

- **Spec §5.3, in order:**
  1. **Port mapping:** a native host asks the router to open its port (UPnP-IGD, NAT-PMP, PCP).
  2. **Hole punching:** ICE with public STUN servers (an editable list from several operators).
  3. **Volunteer TURN relays:** `rl-server --relay` announces itself on Nostr with its region and capacity; peers try the nearest few. Each relay limits every user to about 32 KB/s and a few sessions, so it cannot serve as a free general proxy.
  4. **Failure:** a message naming the step that failed and the fix (a dedicated server, port forwarding, another relay).
- **Spec §5.1:** the direct path stays as is: TCP and UDP port 23400, configurable, with a WebSocket handshake.
  - Nostr: "Short-lived encrypted messages addressed to the host's key through the relays."
  - Kind `21470` is ephemeral (PL12-2; re-checked in Task 6).
- **Spec §1.2:** no central master server dependency. "Any single server of ours may vanish without ending online play." The project ships no TURN server list of its own (ruling N52).
- **Spec §5.4:**
  - Every message type has a size limit and every peer a rate limit.
  - Network input never panics. Every new decoder is fuzzed with proptest (`cases: 256, failure_persistence: None`): handshake events, NIP-44 payloads, TURN notes, NAT-PMP/PCP responses, UPnP-facing code paths, TURN server input.
- **Nothing in `cargo xtask check` or any test touches the public internet, or the machine's real router.**
  - Tests use in-process relays (`TestRelay`), STUN (`StunResponder`, the TURN server's own binding answers), TURN servers (`rl_net::turn`), fake gateways (`portmap::FakeGateway`) and `natsim`.
  - `IceConfig::default()` asks no STUN or TURN server and maps no port.
  - **No test maps a real router port or sends SSDP to 239.255.255.250.** Every test host either sets `port_mapping: false`, or aims at a `FakeGateway` through the test-hooks fields (`IceConfig::{gateway, ssdp}`, `MapRequest::{pmp_port, ssdp}`, rl-server's `Config::test_gateway`). That covers:
    - `rl-server`'s test configs;
    - `fixtures/server/e2e.ron` and `xtask dist`'s `CHECK_CONFIG`;
    - the in-process servers of `crates/rl-cli/src/transport.rs` (`server_config`) and `crates/rl-game/tests/finding.rs` (`server_config`);
    - the hosted games' settings in `crates/rl-game/tests/{finding.rs, online.rs}` and `fixtures/autoplay/settings.ron`.

    Every `portmap` test aims `ssdp` at the fake gateway or a closed 127.0.0.1 port.
  - **TURN servers in tests** listen on port 0, take OS-assigned ephemeral relay ports (`TurnServerConfig::ports = 0..=0`) and generous `TurnLimits` (`TurnLimits::generous()`). Only `turn_limits_hold_per_ip` and `a_full_relay_refuses_new_allocations` use the real limits. Parallel tests never share a port or a per-IP budget.
  - The Docker tests (`cargo xtask net-nat`) belong to `full`. Their image build is the only network use, and it is cached. Without Docker they print `skip net-nat (<why>)`, like `olx_timing::docker_missing`.
- **Personal data never leaves:**
  - no User-Agent or description naming the user (UPnP mapping description `"rust-liero"`);
  - a joiner signs handshakes with a fresh key per join (never the player's identity);
  - no personal data in any test event or Docker scenario.

  The project's commit trailers are no exception: they are tooling metadata.
- **Layers (`xtask/layers.ron`).**
  - New same-layer pair `("rl-online", "rl-discovery")` (ruling N55).
  - No new crate.
  - The simulation-layer crates stay `#![no_std]` and wasm. `rl-proto` (no_std) gains only `core`-based code: the STUN codec, `Path` and `is_global` (moved from `rl_discovery::list`, which re-exports it, so rl-net can judge addresses without a new pair).
  - One integration-test binary per crate (`autotests = false`): new test files are `mod`s of `tests/integration.rs`.
- **Dependencies:**
  - Exact pins (`=x.y.z`), pure Rust, no system packages for the build.
  - The Docker tests need Docker only (`sudo apt install docker.io`, then `sudo usermod -aG docker $USER`).
  - `cargo xtask api` summarises **workspace** crates only. For `str0m`, `turn-client-proto`, `turn-server-proto`, `igd-next` and `netdev`, read the public items in `~/.cargo/registry/src/*/<crate>-<version>/src/` (`lib.rs`, `api.rs`). Never guess a signature.
- **Sans-IO first:**
  - Logic takes time as an argument (`Instant`, unix seconds) and randomness from the caller.
  - Sockets, DNS, threads and the router live only in drivers (`Endpoint`, `PortMapper`, `TurnServer`, `Pool`).
  - Nothing blocks the game's UI thread.
- **Determinism:** no `HashMap` iteration in output (BTreeMap/BTreeSet).
- **Protocol stays 2 (unreleased since plan 10; 0.1.0 had no online play).** Wire changes are appended (`Member::path`).
- **Commits.** Run `cargo xtask check` before every commit, and `cargo xtask full` before the last commit of Tasks 5, 7 and 11 (network-sensitive). Commit on `main`, never push. Trailers:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_019821i4KFSbhZ4mHYULrhEV
  ```

## Decisions this plan makes (recorded as STATUS.md rulings in Task 12)

The spec leaves these open; each is decided here.

- **N44 – handshake encryption.**
  - **Content:** NIP-44 v2 (secp256k1 ECDH x-coordinate → HKDF-SHA256 extract with salt `"nip44-v2"` → per-message HKDF expand → ChaCha20 → HMAC-SHA256), checked against the official test vectors.
  - **Joiner:** signs and encrypts with a **fresh random key per join**, so relays cannot link joins to players. The lobby's proofs (N37) still prove the player's real key over the data channel.
  - **Host:** answers with its own key.
- **N45 – handshake wire.**
  - **Event:** kind 21470 with tags `["p", <recipient hex>]` and `["expiration", <now+60>]`.
  - **Content:** NIP-44 of the JSON `{"v":1,"session":"<32 hex>","signal":<rl_proto::Signal>,"mapped":"<ip:port>"|null}`, at most 16 KiB of content.
  - **Joiner:** publishes its offer, re-sends it every 2 s, and gives up after `ANSWER_DEADLINE` (10 s).
  - **Host:**
    - caches each session's answer for 2 minutes, and answers a repeat with the same event;
    - takes at most 10 new offers a second (burst 20) and refuses anything older than 60 s or more than 60 s ahead;
    - keeps at most `MAX_SIGNALLED_PENDING` (8) signalled links connecting (PF12b-2 says what happens when it is full).
  - **Over-limit input:** an over-limit or hostile event is dropped and counted. The sender cannot be "dropped" (its key is ephemeral), so this deviates from spec §5.4's "a peer breaking a limit is dropped".
  - **No trickle ICE:** the joiner gathers for at most `GATHER_DEADLINE` (2 s), then offers.
  - **Cost if wrong:** up to 2 s more per Nostr join.
- **PF12b-2 – a full pending table evicts a silent setup** (controller ruling).
  - **When:** the signalled-pending table (`MAX_SIGNALLED_PENDING`, 8) is full and a new valid offer arrives.
  - **What:** the oldest pending signalled setup that has received **no ICE connectivity check from its peer** is closed, and the new offer takes its place. Honest joiners send checks within about 1 s of the answer. Only when every pending setup has heard a check is the new offer refused `Busy`.
  - **Effect:** a flood of valid-but-silent offers from fresh keys cannot lock an honest joiner out.
  - **Cost if wrong:** a slow honest joiner may be evicted during a flood and retries.
- **N46 – join order.**
  - A typed address or a LAN game: direct only (plan 10).
  - A listed game or an invite **with** an address: direct first, then Nostr if the link never opened.
  - **Without** an address: Nostr only. The joiner's Nostr relays are the invite's hints first, then its own list, deduplicated and cut to 8. The host listens on all of its relays, so any relay that carried its note reaches it, and no "handshake hint" field is added to notes.
  - **Cost if wrong:** a stale mapped address costs at most `CONNECT_TIMEOUT` (5 s) before Nostr is tried.
- **N47 – what an address in a note or invite means.** It is present only when the host is directly reachable:
  - a router mapping whose external IP is global (TCP and UDP on the same external port, ruling F-G), or
  - a configured `public_address`.

  The port is then the external port. A STUN-learned address is only an ICE candidate, never advertised: this replaces N40's use of STUN, and `AddressWatch` retires.

  **Cost if wrong:** a manual forward without `public_address` joins through Nostr and ICE (relays needed).
- **N48 – ICE candidates.**
  - **Host candidates:** the IPv4 addresses (at most 2) of the default interface (`netdev`; else the source address the OS picks toward the first STUN server, else 127.0.0.1). All candidates share the one UDP socket, and datagrams are handed to str0m as arriving at the first (the "base").
  - **Server-reflexive:** STUN binding requests **from the game socket** to the first two answering STUN servers, at the start and every 25 s (this keeps the router's mapping alive). Two servers seeing different ports means a symmetric NAT (`NatBehaviour`).
  - **Mapped:** a server-reflexive candidate at the router mapping's external address.
  - **Relayed:** the joiner only, at most 2 TURN allocations. The host never allocates: a relay's per-user limit (32 KiB/s) fits one client's traffic (spec §3.1: ≤ 24 KB/s), not a host's. Allocations are released 5 s after a link opens on another path.
  - **Cost if wrong:** a host behind a symmetric NAT whose joiner reaches no TURN relay fails; friends on a shared VPN connect over public paths (other interfaces are not offered).
- **N49 – path names.** `rl_proto::Path { Direct, MappedPort, HolePunched, Relayed }`.
  - **Source:** str0m's selected candidate pair (`RtcConfig::set_stats_interval(Some(1 s))`, `Event::PeerStats::selected_candidate_pair`), classified by `candidates::classify` against our candidates and the remote SDP's (`typ host`/`srflx`/`prflx`/`relay`) plus the remote's mapped hint.
  - **Relayed:** either side's selected address is a TURN relay.
  - **MappedPort:** the remote selected address is the remote's mapped hint.
  - **Direct:** a remote host candidate.
  - **HolePunched:** otherwise.
  - **Direct TCP path:** `Direct` for the joiner; for the host, `MappedPort` when it holds a mapping and the joiner's address is global.
  - **Cost if wrong:** the host may label a mapped-port ICE join "hole-punched".
- **N50 – signalled offers' candidates.**
  - With no TCP connection, T4-A (keep only the TCP peer's IP) cannot apply.
  - A signalled offer keeps at most `MAX_CANDIDATES` (8) `a=candidate` lines that are UDP, IPv4, unicast and not unspecified (`candidates::sanitize`).
  - The host's connectivity checks are ICE-paced and bounded by `MAX_SIGNALLED_PENDING`, as for any WebRTC peer.
  - **Cost if wrong:** a hostile joiner can aim a few STUN checks at a third party.
- **N51 – port mapping.**
  - **Protocols:** NAT-PMP (RFC 6886) and PCP (RFC 6887) are hand-written (two small binary protocols, fuzzed, as N38 did for Nostr). UPnP-IGD goes through `igd-next =0.17.1`'s sync API on the mapper's thread (it handles router quirks: chunked HTTP, control URLs, schemas).
  - **Order:** PCP, falling back to NAT-PMP after any PCP failure (refusal, `UNSUPP_VERSION` or silence) on port 5351 of the default gateway, runs on its own thread in parallel with UPnP's (SSDP search, then HTTP) on another. Each has its own deadline; the first success wins.
  - **Ports:**
    - The game's port: TCP and UDP on the same external port (F-G). The local port is tried first, then +1 … +7.
    - A relay's ports (`MapRequest::exact`): the external port must equal the local one, with no shift. A port that cannot be mapped is reported, not moved.
  - **Lifetimes:**
    - PCP and NAT-PMP: 2 h, renewed at half;
    - UPnP: 3600 s, renewed at half. A router answering 725 (`OnlyPermanentLeasesSupported`) gets lease 0.
  - **Exit:** mappings are deleted within 1 s.
  - **Useless mapping:** an external IP that is not global (`rl_proto::is_global`), unless `allow_local`, means a double NAT or CGNAT. It is reported (`MappingState::Mapped { usable: false }`) and not advertised.
  - **Cost if wrong:** a router quirk igd-next misses leaves hole punching and relays.
- **N52 – TURN relays.**
  - **Library:** `turn-client-proto`/`turn-server-proto` `=0.7.2`, sans-IO.
  - **Who runs them:** the project runs **no** TURN server and ships **no** TURN list (public TURN services need paid credentials). Volunteers run `rl-server --relay`, which announces a kind-31471 note `{v, addr, port, region, capacity, load, user, pass}`. The long-term credentials are public, realm `"rust-liero"`. Players may also add their own TURN servers (`user:pass@host:port`) in Options, for joining. The server config has no TURN list: a host never allocates (N48).
  - **Limits per client IP:** 32 KiB/s relayed each way (token bucket holding 2 s), 4 allocations, 20 requests/s. Over a limit, the datagram or request is dropped and counted, not the client: a relay has no session to drop, which deviates from spec §5.4's "a peer breaking a limit is dropped".
  - **Capacity and ports:** 32 allocations by default, on relay ports 49160-49191, each mapped with an exact external port by the server's port mapping when it is on.
  - **Choice ("the nearest few"):** the player's own servers first (at most 2 used), then announced relays in the joiner's region, then the others, least loaded first, at most 2 allocations in all. No latency probe.
  - **Cost if wrong:** until volunteers run relays, symmetric-NAT pairs cannot connect. Content downloads through a relay run at about 32 KiB/s.
- **N53 – kind 31471** (addressable, `d` = `"rust-liero-turn"`) for TURN relay notes. It is in the 31400-31499 range found free (PL12-2) and re-checked in Task 6.
- **N54 – the host listens on Nostr.**
  - Public and Invite games (and servers) listen on their relays, with a pool of their own. LAN games do not listen.
  - **Cost if wrong:** a public host keeps two connections per relay (the announcing pool and the listening pool).
- **N55 – same-layer pair** `("rl-online", "rl-discovery")`: rl-online drives the handshake pool, so rl-game, rl-server and rl-cli share one join and one listener. **Cost if wrong:** a pair to unwind.
- **N56 – `Endpoint` splits into a sans-IO `Core` and its socket driver**, and `natsim` runs `Core`s behind virtual NATs in `check`.
  - **NAT models:** RFC 4787 terms. Full cone (EIM, EIF), address-restricted (EIM, ADF), port-restricted (EIM, APDF) and symmetric (APDM, APDF).
  - **Other rules:** a 120 s idle mapping lifetime, no hairpinning, 10 ms per hop, seeded port choice.
  - **Cost if wrong:** test infrastructure to maintain.
- **N57 – the STUN binding codec and `is_global` move to `rl-proto`** (`rl_proto::stun`, `rl_proto::is_global`, `core::net`). rl-net's probe and mapper and rl-discovery (re-exports: `rl_discovery::{binding_request, parse_binding_response}`, `rl_discovery::list::is_global`) share them. rl-net has no pair with rl-discovery, and copying the code would duplicate it.
- **N58 – the Docker NAT tests.**
  - **Image:** built `FROM <ID>:<VERSION_ID>` of the host's `/etc/os-release`, so host-built release binaries (libc only) run unchanged. A host without an `/etc/os-release` ID and version, or whose image cannot be built, skips with a notice.
  - **Networks:** isolated `--internal` Docker networks with documentation addresses (WAN 203.0.113.0/24).
  - **Routers:** nftables `masquerade` (port-restricted cone) and `masquerade fully-random` (symmetric), `miniupnpd` (UPnP, NAT-PMP, PCP) with `ext_allow_private_ipv4=yes`.
  - **Our processes:** run with `allow_local` (N62).
  - **Cost if wrong:** a host whose distribution lacks an official image skips the step.
- **N59 – a relayed member's IP ban is a key ban.** Its address is the relay's: banning it would ban the relay for everyone. The admin who asked (`ban_with_address(member, by: Option<u8>)`: the acting admin's member id, `None` for the host's own or a server command) is told "reached through a TURN relay: banned by key only". The server logs it.
- **N60 – `rl-server --nat-check`.**
  - Asks the configured STUN servers, the router (a mapping made and deleted), and the configured Nostr relays (each one's connection and the TURN notes on it).
  - Prints a report and exits 0.
  - It ships in the release zips, so playtesters can run it. It uses public servers only when run by hand.
- **N61 – protocol stays 2:** `Member` gains `path: Option<Path>` (appended); `Signal` is unchanged.
- **N62 – `allow_local`.**
  - **Where:** `rl-server` config `allow_local: bool` and `rl-cli --allow-local`.
  - **What:** lists, TURN notes, mappings and the server's own advertised address accept non-global addresses (PF12-5's switch, extended to everything that judges an address).
  - **Who:** office or LAN setups, and the Docker tests. The game keeps PF12-8's automatic rule.

**Plan rulings:**
- **PL12b-1:** 12 tasks. The `rl-net` refactor (Task 1) comes first, so every later transport feature lands in sans-IO code with natsim tests.
- **PL12b-2:** relay URLs are not added to notes. Any relay that carried a note reaches its host (N46). This deviates from spec §6.1's "handshake hints"; Task 12 adds a pointer there.
- **PL12b-3:** spec §9.7's "a browser host" scenario moves to plan 13 (no browser build yet).

## Review Focus

1. **A hostile handshake feed on the host's relays:**
   - events addressed to the host that are undecryptable, signed by the wrong key, too large, too old or in the future;
   - replays of a real offer;
   - thousands of sessions from fresh keys;
   - an offer naming 100 candidates or a victim's address.

   Expected: no panic, bounded memory and CPU, at most 10 new offers a second, at most 8 signalled links pending (a silent one evicted for a new offer, PF12b-2), at most 8 candidates kept, and honest joiners still answered and connected. Pinned by:
   - Task 6's `a_hostile_handshake_feed_is_bounded`;
   - Task 2's `signalled_offers_keep_at_most_eight_unicast_candidates`;
   - Task 7's `a_flood_of_offers_leaves_room_for_an_honest_joiner`.
2. **A router that lies, stalls or forgets:**
   - a UPnP device whose HTTP never answers;
   - NAT-PMP granting another external port for UDP than for TCP;
   - PCP refusing;
   - a mapping with a private external IP (double NAT);
   - a renewal refused.

   Expected: hosting goes on, the lobby and log say why, nothing blocks the UI thread, and mappings are deleted at exit. Pinned by Task 4's `fake_gateway_misbehaviours_are_reported` and `mappings_are_deleted_on_drop`.
3. **Relays losing or delaying the ephemeral handshake:**
   - a relay that never forwards kind 21470 (`NoForward`);
   - one that drops the first offers;
   - duplicates across relays.

   Expected: re-sends every 2 s, the cached answer for a repeated session, one link per session, and `NO_REPLY` after 10 s if nothing answers. Pinned by Task 7's `a_lost_offer_is_sent_again` and `a_relay_that_never_forwards_is_survived`.
4. **A TURN relay abused or failing:**
   - a client over 32 KiB/s;
   - five allocations from one IP;
   - a full relay;
   - a relay that dies mid-match.

   Expected: the limits hold, other users are unaffected, the joiner tries the next relay, and a dead relay ends the link with "the connection to the host was lost". Pinned by Task 3's `turn_limits_hold_per_ip` and `a_full_relay_refuses_new_allocations`, and natsim's `a_dead_relay_ends_the_link`.
5. **Paths that look right but are not:**
   - a joiner behind the same router as the host, using the host's public address (no hairpinning);
   - a relayed member being IP-banned;
   - the host's public address changing mid-session.

   Expected: the same-LAN joiner falls back to Nostr and connects directly; the ban is by key only; new answers carry the new candidates. Pinned by:
   - natsim's `the_same_lan_connects_directly`;
   - Task 7's `a_same_lan_joiner_without_hairpinning_falls_back_and_connects_directly`;
   - Task 7's `a_relayed_member_is_banned_by_key_only`;
   - Task 2's `a_changed_reflexive_address_is_offered`.

---

## File structure

| File | Responsibility |
|---|---|
| `crates/rl-proto/src/stun.rs` and `addr.rs` (moved from rl-discovery), `crates/rl-proto/src/lobby.rs` | The STUN binding codec and `is_global` (N57); `Path`, `Member::path` (N49, N61). |
| `crates/rl-net/src/core.rs` | The sans-IO endpoint core: links, limits, impairment, setups, demultiplexing, events (Task 1); candidates and STUN (Task 2); TURN client (Task 3). |
| `crates/rl-net/src/candidates.rs` | `LocalCandidates`, `RemoteCandidates`, `sanitize`, `classify` (Task 2). |
| `crates/rl-net/src/probe.rs` | `StunProbe`: binding requests on the game socket, `NatBehaviour` (Task 2). |
| `crates/rl-net/src/relayed.rs` | The joiner's TURN allocations inside `Core` (Task 3). |
| `crates/rl-net/src/turn.rs` | `TurnCore` (server, limits) and `TurnServer` (its socket driver) (Task 3). |
| `crates/rl-net/src/natsim.rs` | The virtual NAT network (feature `test-hooks`) (Tasks 2, 3). |
| `crates/rl-net/src/portmap/{mod.rs, pmp.rs, upnp.rs, fake.rs}` | `PortMapper`, the NAT-PMP/PCP codec, UPnP through igd-next, `FakeGateway` (test-hooks) (Task 4). |
| `crates/rl-net/src/local.rs` | The base address and gateway (`netdev`) (Task 5). |
| `crates/rl-net/src/endpoint.rs` | The socket driver: UDP, TCP signalling, DNS and mapping threads, ICE wiring (Tasks 1, 5). |
| `crates/rl-discovery/src/{nip44.rs, handshake.rs, turn_note.rs}` | NIP-44, `Inbox`/`Asker`, `TurnNote`/`TurnList` (Task 6). |
| `crates/rl-online/src/{join.rs, reach.rs}` | The join order and its texts; the host's listener and `Reachability` (Task 7). |
| `crates/rl-server/src/{config.rs, lib.rs, relay.rs, nat_check.rs, main.rs}` | Mapping, listening, `--relay`, `--nat-check` (Task 8). |
| `crates/rl-ui/src/{online.rs, state.rs, screens/lobby.rs, screens/multiplayer.rs, screens/options.rs, store.rs}`, `crates/rl-game/src/{finding.rs, net.rs, shell/online.rs}` | Lobby lines, tags, Options, joins (Task 9). |
| `crates/rl-cli/src/{net_client_cmd.rs, transport.rs, nat_tools.rs}` | Joins by invite, key or listing; `nostr-relay` and `stun-server` (Task 10). |
| `xtask/src/nat.rs`, `tools/net-nat/{Dockerfile, router.sh, node.sh, CLAUDE.md}` | `cargo xtask net-nat` (Task 11). |

---

### Task 1: `rl-net`: split `Endpoint` into a sans-IO `Core` and its socket driver

**Files:**
- Create: `crates/rl-net/src/core.rs`, `crates/rl-net/tests/core_wire.rs` (a `mod core_wire;` of `tests/integration.rs`)
- Modify: `crates/rl-net/src/{endpoint.rs, lib.rs}`, `crates/rl-net/tests/integration.rs`, `crates/rl-net/CLAUDE.md`
- Test: `crates/rl-net/tests/core_wire.rs`; every existing `rl-net` test unchanged

**Interfaces:**
- Consumes: `link::Link` (unchanged), `limits::PeerLimits`, `impair::Impair`, `rl_proto::{Signal, Refusal}`.
- Produces (Tasks 2, 3, 5):
  ```rust
  // rl_net::core
  pub type SetupId = u64;
  #[derive(Clone, Debug)]
  pub struct CoreConfig {
      pub host: bool,
      pub max_peers: usize,
      pub limits: Limits,
      pub impair: Option<Profile>,
      pub impair_seed: u64,
      #[cfg(feature = "test-hooks")] pub fingerprint_override: Option<u64>,
      #[cfg(feature = "test-hooks")] pub first_peer_id: Option<PeerId>,
  }
  impl CoreConfig { pub fn host() -> CoreConfig; pub fn client() -> CoreConfig; } // EndpointConfig's defaults
  /// One datagram to send from the core's socket.
  #[derive(Clone, Debug, PartialEq, Eq)] pub struct Datagram { pub to: SocketAddr, pub bytes: Vec<u8> }
  #[derive(Clone, Debug, PartialEq, Eq)]
  pub enum CoreEvent {
      /// What the endpoint reports (`Connected`, `Message`, `Disconnected`).
      Net(NetEvent),
      /// Setup `setup` opened as `peer` (the driver lingers its signalling connection).
      Opened { setup: SetupId, peer: PeerId },
      /// Setup `setup` did not open (`Timeout` past `SETUP_TIMEOUT`, `Closed` by the link).
      SetupFailed { setup: SetupId, why: Why },
  }
  pub struct Core { .. }
  impl Core {
      pub fn new(cfg: CoreConfig, now: Instant) -> Core;
      /// The version, fingerprint, capacity and id checks of an offer (not the gate).
      pub fn admit(&self, offer: &Signal) -> Result<(), Refusal>;
      /// Client: an offerer link from host candidates `local` to the peer at `addr`.
      pub fn offer(&mut self, local: &[SocketAddr], addr: SocketAddr, now: Instant) -> (SetupId, String);
      pub fn accept_answer(&mut self, setup: SetupId, answer: &str) -> Result<(), LinkError>;
      /// Host: an answerer link for an admitted offer's SDP.
      pub fn answer(&mut self, local: &[SocketAddr], offer_sdp: &str, addr: SocketAddr, now: Instant) -> Result<(SetupId, String), Refusal>;
      /// The signalling of `setup` failed: drop it (a client reports `Disconnected { HOST, why }`).
      pub fn cancel(&mut self, setup: SetupId, why: Why, events: &mut Vec<CoreEvent>);
      pub fn receive(&mut self, from: SocketAddr, bytes: &[u8], now: Instant);
      pub fn poll(&mut self, now: Instant, out: &mut Vec<Datagram>, events: &mut Vec<CoreEvent>);
      pub fn send(&mut self, peer: PeerId, channel: Channel, bytes: &[u8]);
      pub fn buffered(&self, peer: PeerId, channel: Channel) -> usize;
      pub fn allow_burst(&mut self, peer: PeerId, bytes: usize);
      pub fn close(&mut self, peer: PeerId, out: &mut Vec<Datagram>);
      pub fn peers(&self) -> impl Iterator<Item = (PeerId, SocketAddr)> + '_;
      pub fn dtls_fingerprints(&self, peer: PeerId) -> Option<([u8; 32], [u8; 32])>;
      pub fn stats(&self, peer: PeerId) -> Option<PeerStats>;
      pub fn stray_datagrams(&self) -> u64;
      pub fn pending(&self) -> usize;
  }
  ```
  `Endpoint`'s public API stays exactly as `target/api/rl_net.txt` lists it. `Endpoint` keeps the UDP socket, TCP listener, `SignalConn`s, the connect thread and the lingering closes. It maps `SetupId` to its `SignalConn`, feeds `Core::receive`, sends `Core::poll`'s datagrams (`send_to`, counting `send_failures` as today) and turns `CoreEvent::Net` into `NetEvent`.

Context:
- **A pure refactor.** Every behaviour in `crates/rl-net/CLAUDE.md` stays, and the 839 lines of `tests/endpoint.rs` plus `tests/link.rs`, `tests/fuzz.rs`, rl-online's and rl-cli's transport tests pin it. That covers:
  - limits and timeouts;
  - T4-A and F-G;
  - the per-IP caps (these stay in the driver: they count signalling connections);
  - ids never reused;
  - impairment per link;
  - `Unsent` on a reliable overflow.
- **Ownership.** T4-A's `keep_candidates_of` and F-G's `rewrite_candidates` stay in the driver: they are about the TCP connection. `Core` takes the SDP the driver hands it.
- **Send failures.** `PeerStats::send_failures` is counted by the driver (the socket refused). `Core::poll` returns datagrams with the peer id they came from; give `Datagram` a `pub(crate) peer: Option<PeerId>` field the driver uses, so the stats stay per peer.

- [ ] **Step 1: Write the failing test** `crates/rl-net/tests/core_wire.rs`:

```rust
//! `Core` without sockets: two cores on an in-memory wire, a virtual clock.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use rl_net::core::{Core, CoreConfig, CoreEvent, Datagram};
use rl_net::endpoint::{HOST, NetEvent};
use rl_netcode::Channel;

/// Delivers what each core sends to the other at once.
fn exchange(a: &mut Core, a_at: SocketAddr, b: &mut Core, b_at: SocketAddr, now: Instant, ea: &mut Vec<CoreEvent>, eb: &mut Vec<CoreEvent>) {
    let mut out: Vec<Datagram> = Vec::new();
    a.poll(now, &mut out, ea);
    for d in out.drain(..) {
        if d.to == b_at {
            b.receive(a_at, &d.bytes, now);
        }
    }
    b.poll(now, &mut out, eb);
    for d in out.drain(..) {
        if d.to == a_at {
            a.receive(b_at, &d.bytes, now);
        }
    }
}

fn connected(events: &[CoreEvent]) -> bool {
    events.iter().any(|e| matches!(e, CoreEvent::Net(NetEvent::Connected { .. })))
}

#[test]
fn two_cores_connect_and_talk_on_a_virtual_wire() {
    let t0 = Instant::now();
    let host_at: SocketAddr = "10.0.0.1:23400".parse().unwrap();
    let client_at: SocketAddr = "10.0.0.2:50000".parse().unwrap();
    let mut host = Core::new(CoreConfig::host(), t0);
    let mut client = Core::new(CoreConfig::client(), t0);
    let (setup, offer) = client.offer(&[client_at], host_at, t0);
    let (_, answer) = host.answer(&[host_at], &offer, client_at, t0).expect("answered");
    client.accept_answer(setup, &answer).expect("accepted");
    let (mut eh, mut ec) = (Vec::new(), Vec::new());
    let mut now = t0;
    while !(connected(&eh) && connected(&ec)) {
        now += Duration::from_millis(5);
        assert!(now < t0 + Duration::from_secs(10), "no connection; host {eh:?} client {ec:?}");
        exchange(&mut client, client_at, &mut host, host_at, now, &mut ec, &mut eh);
    }
    assert!(ec.iter().any(|e| matches!(e, CoreEvent::Opened { setup: s, peer: HOST } if *s == setup)));
    client.send(HOST, Channel::Reliable, b"hello");
    let mut got = None;
    for _ in 0..200 {
        now += Duration::from_millis(5);
        eh.clear();
        exchange(&mut client, client_at, &mut host, host_at, now, &mut ec, &mut eh);
        got = eh.iter().find_map(|e| match e {
            CoreEvent::Net(NetEvent::Message { bytes, .. }) => Some(bytes.clone()),
            _ => None,
        });
        if got.is_some() {
            break;
        }
    }
    assert_eq!(got.as_deref(), Some(&b"hello"[..]));
}

#[test]
fn a_setup_that_never_answers_times_out() {
    let t0 = Instant::now();
    let mut client = Core::new(CoreConfig::client(), t0);
    let (setup, _) = client.offer(&["10.0.0.2:50000".parse().unwrap()], "10.0.0.1:23400".parse().unwrap(), t0);
    let mut events = Vec::new();
    client.poll(t0 + Duration::from_secs(11), &mut Vec::new(), &mut events);
    assert!(events.contains(&CoreEvent::SetupFailed { setup, why: rl_net::endpoint::Why::Timeout }), "{events:?}");
    assert!(events.contains(&CoreEvent::Net(NetEvent::Disconnected { peer: HOST, why: rl_net::endpoint::Why::Timeout })));
}

#[test]
fn admit_checks_version_fingerprint_and_capacity() {
    let core = Core::new(CoreConfig { max_peers: 0, ..CoreConfig::host() }, Instant::now());
    let offer = rl_proto::Signal::Offer { protocol: rl_proto::PROTOCOL_VERSION, fingerprint: rl_proto::FINGERPRINT, sdp: String::new() };
    assert_eq!(core.admit(&offer), Err(rl_proto::Refusal::Full));
    let old = rl_proto::Signal::Offer { protocol: 1, fingerprint: rl_proto::FINGERPRINT, sdp: String::new() };
    assert_eq!(core.admit(&old), Err(rl_proto::Refusal::Version));
}
```

- [ ] **Step 2: Run `cargo nextest run -p rl-net core_wire`, expect FAIL** (no `rl_net::core`).
- [ ] **Step 3: Implement.** Move the per-peer logic (`Peer`, `step`, `receive`'s demultiplexing, `pump`, `finish`, limits, impairment, id assignment, deadlines) from `endpoint.rs` into `core.rs`. Leave `endpoint.rs` as the socket and TCP driver. Keep every doc comment that still applies. `Endpoint::poll`'s order stays: signalling, receive, step peers, lingering.
- [ ] **Step 4: Run `cargo nextest run -p rl-net -p rl-online -p rl-cli`, expect PASS, existing tests unchanged; then `cargo xtask check`.**
- [ ] **Step 5: Update `crates/rl-net/CLAUDE.md`** (the `Core`/driver split, which part owns what). **Commit** `rl-net: split the endpoint into a sans-IO core and its socket driver`.

---

### Task 2: Candidates, STUN on the game socket, paths, and `natsim`

**Files:**
- Create:
  - `crates/rl-proto/src/stun.rs` (moved from `crates/rl-discovery/src/stun.rs`'s codec, `core::net`);
  - `crates/rl-proto/src/addr.rs` (`is_global`, moved from `crates/rl-discovery/src/list.rs`);
  - `crates/rl-net/src/{candidates.rs, probe.rs, natsim.rs}`;
  - `crates/rl-net/tests/{natsim.rs, candidates.rs}`.
- Modify:
  - `crates/rl-proto/src/{lib.rs, lobby.rs}` (`Path`, `Member::path`, the re-exports);
  - `crates/rl-proto/tests/fuzz_lobby.rs` (`Member` samples);
  - `crates/rl-discovery/src/stun.rs` (re-exports the codec; `public_ip` and `StunResponder` stay);
  - `crates/rl-discovery/src/list.rs` (`pub use rl_proto::is_global`; its range tests move to rl-proto);
  - `crates/rl-discovery/tests/announce.rs` (builds `Member`);
  - `crates/rl-lobby/src/{host.rs, harness.rs}` and `crates/rl-lobby/tests/lobby.rs` (`connected(.., path, ..)`, `Member::path`);
  - `crates/rl-online/src/{peer.rs, content/peer_side.rs}` (the `connected` call, `Member` literals);
  - `crates/rl-net/src/{core.rs, link.rs, endpoint.rs, lib.rs}`;
  - `crates/rl-net/Cargo.toml` (`natsim` under `test-hooks`);
  - the CLAUDE.md of rl-net, rl-proto, rl-lobby and rl-discovery.
- Test: `crates/rl-net/tests/{natsim.rs, candidates.rs}`, the moved STUN codec tests in rl-proto, rl-discovery's `tests/stun.rs` still passing.

**Interfaces:**
- Consumes: Task 1's `Core`, `CoreEvent`, `Datagram`.
- Produces (Tasks 3, 5, 7, 9):
  ```rust
  // rl_proto::lobby (re-exported at the crate root)
  #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
  pub enum Path { Direct, MappedPort, HolePunched, Relayed }
  // rl_proto::stun (moved; rl_discovery::{binding_request, parse_binding_response} re-export these)
  pub fn binding_request(transaction: [u8; 12]) -> Vec<u8>;
  pub fn parse_binding_response(bytes: &[u8], transaction: &[u8; 12]) -> Option<SocketAddr>;
  pub fn is_stun(bytes: &[u8]) -> bool; // first two bits 0, magic cookie at 4..8, length matches
  // rl_proto::addr (re-exported at the crate root; rl_discovery::list::is_global re-exports it)
  pub fn is_global(a: IpAddr) -> bool; // moved unchanged (v4-mapped unwrapped, hand-written v4/v6 ranges)

  // rl_net::candidates
  pub const MAX_CANDIDATES: usize = 8;
  #[derive(Clone, Debug, Default, PartialEq, Eq)]
  pub struct LocalCandidates { pub base: Option<SocketAddr>, pub host: Vec<SocketAddr>, pub srflx: Option<SocketAddr>, pub mapped: Option<SocketAddr>, pub relayed: Vec<SocketAddr> }
  #[derive(Clone, Debug, Default, PartialEq, Eq)]
  pub struct RemoteCandidates { pub host: Vec<SocketAddr>, pub srflx: Vec<SocketAddr>, pub relay: Vec<SocketAddr>, pub mapped: Option<SocketAddr> }
  /// Only the candidate lines `sanitize` keeps (UDP, IPv4, unicast, not unspecified; no mDNS name, TCP, IPv6 or multicast), by `typ`.
  pub fn remote_candidates(sdp: &str, mapped_hint: Option<SocketAddr>) -> RemoteCandidates;
  /// A signalled offer's SDP with at most MAX_CANDIDATES UDP IPv4 unicast candidate lines (N50).
  pub fn sanitize(sdp: &str) -> String;
  pub fn classify(local: SocketAddr, remote: SocketAddr, ours: &LocalCandidates, theirs: &RemoteCandidates) -> Path;
  /// A TURN server's address and long-term credentials (allocations come in Task 3).
  #[derive(Clone, Debug, PartialEq, Eq)] pub struct TurnSpec { pub addr: SocketAddr, pub user: String, pub pass: String }

  // rl_net::probe
  #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
  pub enum NatBehaviour { #[default] Unknown, /// srflx == base: no NAT
      Open, /// the same mapped port toward two servers
      Preserving, /// different mapped ports: symmetric NAT
      Symmetric }
  pub const STUN_EVERY: Duration = Duration::from_secs(25);
  pub const STUN_WAIT: Duration = Duration::from_secs(1);

  // rl_net::core (additions)
  /// A signalled setup must open within this (TCP-signalled ones keep SETUP_TIMEOUT, 10 s).
  pub const SIGNALLED_SETUP_TIMEOUT: Duration = Duration::from_secs(15);
  /// At most this many signalled setups connect at once; when full, PF12b-2 evicts a silent one.
  pub const MAX_SIGNALLED_PENDING: usize = 8;
  impl Core {
      /// The socket's candidates: `base` is where every datagram is taken as arriving; `hosts` (base first) are offered.
      pub fn set_base(&mut self, base: SocketAddr, hosts: Vec<SocketAddr>);
      /// STUN servers asked from this socket now and every STUN_EVERY (two answers give NatBehaviour).
      pub fn stun(&mut self, servers: Vec<SocketAddr>, now: Instant);
      pub fn set_mapped(&mut self, mapped: Option<SocketAddr>);
      pub fn local(&self) -> LocalCandidates;
      pub fn nat(&self) -> NatBehaviour;
      /// STUN answered (or STUN_WAIT passed) — and, from Task 3, TURN too.
      pub fn gathered(&self, now: Instant) -> bool;
      /// Client, signalled: an offerer link with every local candidate.
      pub fn offer_ice(&mut self, peer_hint: SocketAddr, now: Instant) -> (SetupId, String);
      /// Client: the host's answer and its mapped hint (no F-G rewrite).
      pub fn accept_answer_ice(&mut self, setup: SetupId, answer: &str, mapped_hint: Option<SocketAddr>) -> Result<(), LinkError>;
      /// Host, signalled: admit, sanitize, an answerer link with every local candidate; Answer or Refused.
      /// With MAX_SIGNALLED_PENDING setups pending, the oldest one that has received no ICE connectivity
      /// check (STUN binding request) from its peer is closed to make room (PF12b-2); if every one has, Refused { Busy }.
      pub fn answer_ice(&mut self, offer: &Signal, mapped_hint: Option<SocketAddr>, now: Instant) -> Signal;
      pub fn path(&self, peer: PeerId) -> Option<Path>;
  }
  // NetEvent::Connected gains `path: Path` (the first classification; Direct for TCP-signalled links).

  // rl_net::link (additions)
  pub struct LinkCandidates { pub host: Vec<SocketAddr>, pub srflx: Vec<(SocketAddr, SocketAddr)>, pub relayed: Vec<(SocketAddr, SocketAddr)> } // (address, base)
  impl Link {
      pub fn offerer_with(c: &LinkCandidates, now: Instant) -> (Link, String);
      pub fn answerer_with(c: &LinkCandidates, offer: &str, now: Instant) -> Result<(Link, String), LinkError>;
      /// str0m's selected pair (local, remote) from its stats, every second once open.
      pub fn selected_pair(&self) -> Option<(SocketAddr, SocketAddr)>;
  }

  // rl_net::natsim (feature test-hooks)
  #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum NatKind { FullCone, AddressRestricted, PortRestricted, Symmetric }
  #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub struct NatId(pub usize);
  #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub struct NodeId(pub usize);
  pub struct Net { .. }
  impl Net {
      pub fn new(seed: u64) -> Net;
      pub fn nat(&mut self, kind: NatKind, public: Ipv4Addr) -> NatId;
      /// A router mapping (what a successful UPnP gives): `external_port` on the NAT to `to`.
      pub fn forward(&mut self, nat: NatId, external_port: u16, to: SocketAddr);
      pub fn stun_server(&mut self, at: SocketAddr) -> SocketAddr;
      /// A host core (CoreConfig::host()) at `at`, behind `nat` (or on the public network).
      /// Nodes are numbered in creation order from 0.
      pub fn host(&mut self, behind: Option<NatId>, at: SocketAddr) -> NodeId;
      pub fn client(&mut self, behind: Option<NatId>, at: SocketAddr) -> NodeId;
      pub fn core(&mut self, n: NodeId) -> &mut Core;
      pub fn now(&self) -> Instant;
      /// core.stun(servers) (and, from Task 3, core.allocate(turn)).
      pub fn gather(&mut self, n: NodeId, stun: &[SocketAddr], turn: &[TurnSpec]);
      pub fn gathered(&self, n: NodeId) -> bool;
      /// offer_ice on the client, answer_ice on the host, accept_answer_ice: the signalling Nostr would carry.
      pub fn signal(&mut self, client: NodeId, host: NodeId) -> Result<(), Refusal>;
      /// Advances the clock in 1 ms steps, delivering datagrams, until `done` or `limit`; true when `done`.
      pub fn run_until(&mut self, limit: Duration, done: impl FnMut(&mut Net) -> bool) -> bool;
      /// The path of the client's link to its host once Connected (None before, or after a failure).
      pub fn connected(&self, client: NodeId) -> Option<Path>;
      /// Every NetEvent the node reported so far (kept; not drained).
      pub fn events(&mut self, n: NodeId) -> Vec<NetEvent>;
      /// The ISP gives the NAT a new public IP: its mappings are dropped.
      pub fn renumber(&mut self, nat: NatId, public: Ipv4Addr);
  }
  ```

Context:
- **`natsim`'s NATs** (N56).
  - A datagram from a node behind a NAT gets an external port:
    - an endpoint-independent mapping keeps the internal port if free, else the next free one;
    - a symmetric NAT makes a new mapping per destination, its port from the seeded RNG in 1024..65535.
  - Inbound datagrams to an external port pass the filter:
    - full cone: anyone;
    - address-restricted: an IP the mapping sent to;
    - port-restricted and symmetric: the exact ip:port the mapping sent to.
  - A `forward` passes everything. Mappings idle 120 s expire. A datagram from inside to the NAT's own public IP is dropped (no hairpin).
  - Delivery takes 10 ms per hop in (time, sequence) order. A STUN server answers a binding request with the observed source. Nodes on the same NAT reach each other's private addresses directly.
- **`Core` and STUN.**
  - `Core::receive` gives a datagram to `StunProbe` first when it comes from a server it asked and parses as a binding response to an outstanding transaction. Otherwise it goes to the links, as arriving at `base` (or at the link's own `local` for TCP-signalled links, N16 unchanged).
  - `srflx` is the first answer. When it differs from a later one (the router's address changed), `local()` returns the new one and later offers and answers carry it.
- **`classify`** follows N49; unit-test every row.
- **Selected pair.**
  - `Link` builds its `Rtc` with `RtcConfig::new().set_stats_interval(Some(Duration::from_secs(1)))`. In `on_event`, `Event::PeerStats(s)` keeps `s.selected_candidate_pair`'s `(local.addr, remote.addr)`.
  - `Core` re-classifies at each change.
  - `Connected` is reported when the link opens, with the path of the selected pair at that moment. A link whose stats have not arrived yet keeps `Connected` until the first stats event, at most 1.5 s after `Open`, and is classified then. Bound it: if no stats come, use `HolePunched` for signalled links and `Direct` for TCP-signalled ones.
- **str0m's candidates** are `Candidate::host`, `Candidate::server_reflexive(addr, base, "udp")` and `Candidate::relayed(addr, base, "udp")` (Task 3), added before the offer or answer is made so they appear in the SDP.

- [ ] **Step 1: Write the failing tests.** `crates/rl-net/tests/natsim.rs`:

```rust
//! Hole punching through simulated routers (N56), on a virtual clock.

use std::net::SocketAddr;
use std::time::Duration;

use rl_net::natsim::{NatKind, Net, NodeId};
use rl_net::probe::NatBehaviour;
use rl_proto::Path;

fn a(s: &str) -> SocketAddr {
    s.parse().unwrap()
}

/// A host behind `host_nat` and a client behind `client_nat` (None: public), gathered and signalled.
fn join(host_nat: Option<NatKind>, client_nat: Option<NatKind>) -> (Net, NodeId, Option<Path>) {
    let mut net = Net::new(7);
    let s1 = net.stun_server(a("198.51.100.1:3478"));
    let s2 = net.stun_server(a("198.51.100.2:3478"));
    let nh = host_nat.map(|k| net.nat(k, "203.0.113.10".parse().unwrap()));
    let nc = client_nat.map(|k| net.nat(k, "203.0.113.20".parse().unwrap()));
    let host = net.host(nh, if nh.is_some() { a("192.168.1.10:23400") } else { a("203.0.113.10:23400") });
    let client = net.client(nc, if nc.is_some() { a("192.168.7.20:50000") } else { a("203.0.113.20:50000") });
    net.gather(host, &[s1, s2], &[]);
    net.gather(client, &[s1, s2], &[]);
    assert!(net.run_until(Duration::from_secs(3), |n| n.gathered(host) && n.gathered(client)));
    net.signal(client, host).expect("answered");
    // Past SIGNALLED_SETUP_TIMEOUT (15 s), so a failed setup has reported Disconnected.
    net.run_until(Duration::from_secs(20), |n| n.connected(client).is_some());
    let path = net.connected(client);
    (net, client, path)
}

#[test]
fn public_peers_connect_directly() {
    assert_eq!(join(None, None).2, Some(Path::Direct));
}

#[test]
fn cone_nats_hole_punch() {
    assert_eq!(join(Some(NatKind::FullCone), Some(NatKind::FullCone)).2, Some(Path::HolePunched));
}

#[test]
fn port_restricted_nats_hole_punch() {
    assert_eq!(join(Some(NatKind::PortRestricted), Some(NatKind::PortRestricted)).2, Some(Path::HolePunched));
}

#[test]
fn a_full_cone_host_takes_a_symmetric_client() {
    assert_eq!(join(Some(NatKind::FullCone), Some(NatKind::Symmetric)).2, Some(Path::HolePunched));
}

#[test]
fn symmetric_against_port_restricted_fails_without_a_relay() {
    let (mut net, client, path) = join(Some(NatKind::PortRestricted), Some(NatKind::Symmetric));
    assert_eq!(path, None);
    assert!(net.events(client).iter().any(|e| matches!(e, rl_net::endpoint::NetEvent::Disconnected { .. })));
}

#[test]
fn a_mapped_port_is_used_and_named() {
    let mut net = Net::new(7);
    let s1 = net.stun_server(a("198.51.100.1:3478"));
    let nh = net.nat(NatKind::PortRestricted, "203.0.113.10".parse().unwrap());
    let nc = net.nat(NatKind::Symmetric, "203.0.113.20".parse().unwrap());
    let host = net.host(Some(nh), a("192.168.1.10:23400"));
    net.forward(nh, 23400, a("192.168.1.10:23400"));
    net.core(host).set_mapped(Some(a("203.0.113.10:23400")));
    let client = net.client(Some(nc), a("192.168.7.20:50000"));
    net.gather(host, &[s1], &[]);
    net.gather(client, &[s1], &[]);
    assert!(net.run_until(Duration::from_secs(3), |n| n.gathered(host) && n.gathered(client)));
    net.signal(client, host).expect("answered");
    assert!(net.run_until(Duration::from_secs(15), |n| n.connected(client).is_some()));
    assert_eq!(net.connected(client), Some(Path::MappedPort));
}

#[test]
fn the_same_lan_connects_directly() {
    let mut net = Net::new(7);
    let s1 = net.stun_server(a("198.51.100.1:3478"));
    let n = net.nat(NatKind::PortRestricted, "203.0.113.10".parse().unwrap());
    let host = net.host(Some(n), a("192.168.1.10:23400"));
    let client = net.client(Some(n), a("192.168.1.11:50000"));
    net.gather(host, &[s1], &[]);
    net.gather(client, &[s1], &[]);
    assert!(net.run_until(Duration::from_secs(3), |n| n.gathered(host) && n.gathered(client)));
    net.signal(client, host).expect("answered");
    assert!(net.run_until(Duration::from_secs(15), |n| n.connected(client).is_some()));
    assert_eq!(net.connected(client), Some(Path::Direct));
}

#[test]
fn nat_behaviour_is_detected() {
    for (kind, expected) in [
        (None, NatBehaviour::Open),
        (Some(NatKind::PortRestricted), NatBehaviour::Preserving),
        (Some(NatKind::Symmetric), NatBehaviour::Symmetric),
    ] {
        let mut net = Net::new(3);
        let s1 = net.stun_server(a("198.51.100.1:3478"));
        let s2 = net.stun_server(a("198.51.100.2:3478"));
        let nat = kind.map(|k| net.nat(k, "203.0.113.20".parse().unwrap()));
        let at = if nat.is_some() { a("192.168.7.20:50000") } else { a("203.0.113.20:50000") };
        let c = net.client(nat, at);
        net.gather(c, &[s1, s2], &[]);
        assert!(net.run_until(Duration::from_secs(3), |n| n.gathered(c)));
        assert_eq!(net.core(c).nat(), expected, "{kind:?}");
    }
}

#[test]
fn stun_keeps_the_mapping_alive() {
    let mut net = Net::new(5);
    let s1 = net.stun_server(a("198.51.100.1:3478"));
    let nh = net.nat(NatKind::Symmetric, "203.0.113.10".parse().unwrap());
    let host = net.host(Some(nh), a("192.168.1.10:23400"));
    net.gather(host, &[s1], &[]);
    assert!(net.run_until(Duration::from_secs(3), |n| n.gathered(host)));
    let before = net.core(host).local().srflx;
    assert!(before.is_some(), "STUN answered");
    // Past the NAT's 120 s idle lifetime: STUN_EVERY (25 s) keeps the mapping, so the
    // (seeded, random) symmetric port does not change.
    net.run_until(Duration::from_secs(130), |_| false);
    assert_eq!(net.core(host).local().srflx, before);
}

#[test]
fn a_changed_reflexive_address_is_offered() {
    let mut net = Net::new(5);
    let s1 = net.stun_server(a("198.51.100.1:3478"));
    let nh = net.nat(NatKind::PortRestricted, "203.0.113.10".parse().unwrap());
    let nc = net.nat(NatKind::PortRestricted, "203.0.113.20".parse().unwrap());
    let host = net.host(Some(nh), a("192.168.1.10:23400"));
    net.gather(host, &[s1], &[]);
    assert!(net.run_until(Duration::from_secs(3), |n| n.gathered(host)));
    net.renumber(nh, "203.0.113.11".parse().unwrap());
    // The next STUN round (at most STUN_EVERY later) learns the new address.
    assert!(net.run_until(Duration::from_secs(30), |n| n
        .core(host)
        .local()
        .srflx
        .is_some_and(|s| s.ip() == "203.0.113.11".parse::<std::net::IpAddr>().unwrap())));
    let client = net.client(Some(nc), a("192.168.7.20:50000"));
    net.gather(client, &[s1], &[]);
    assert!(net.run_until(Duration::from_secs(3), |n| n.gathered(client)));
    net.signal(client, host).expect("answered");
    assert!(net.run_until(Duration::from_secs(15), |n| n.connected(client).is_some()));
    assert_eq!(net.connected(client), Some(Path::HolePunched));
}
```

`crates/rl-net/tests/candidates.rs`:

```rust
use std::net::SocketAddr;

use rl_net::candidates::{LocalCandidates, MAX_CANDIDATES, RemoteCandidates, classify, remote_candidates, sanitize};
use rl_proto::Path;

fn a(s: &str) -> SocketAddr {
    s.parse().unwrap()
}

const SDP: &str = "v=0\r\na=ice-ufrag:x\r\n\
    a=candidate:1 1 udp 2130706431 192.168.1.5 50000 typ host\r\n\
    a=candidate:2 1 udp 1694498815 198.51.100.7 61000 typ srflx raddr 0.0.0.0 rport 0\r\n\
    a=candidate:3 1 udp 16777215 203.0.113.9 49160 typ relay raddr 0.0.0.0 rport 0\r\n\
    a=candidate:4 1 tcp 2130706431 192.168.1.5 9 typ host tcptype active\r\n\
    a=candidate:5 1 udp 2130706431 x.local 50000 typ host\r\n\
    a=candidate:6 1 udp 2130706431 0.0.0.0 50000 typ host\r\n\
    a=candidate:7 1 udp 2130706431 224.0.0.1 50000 typ host\r\n\
    a=candidate:8 1 udp 2130706431 fe80::1 50000 typ host\r\n";

#[test]
fn remote_candidates_are_read_by_type() {
    let r = remote_candidates(SDP, Some(a("198.51.100.7:23400")));
    assert_eq!(r.host, vec![a("192.168.1.5:50000")]);
    assert_eq!(r.srflx, vec![a("198.51.100.7:61000")]);
    assert_eq!(r.relay, vec![a("203.0.113.9:49160")]);
    assert_eq!(r.mapped, Some(a("198.51.100.7:23400")));
}

#[test]
fn signalled_offers_keep_at_most_eight_unicast_candidates() {
    let kept = sanitize(SDP);
    assert_eq!(kept.matches("a=candidate:").count(), 3, "{kept}");
    let mut many = String::from("v=0\r\n");
    for i in 0..100 {
        many.push_str(&format!("a=candidate:{i} 1 udp 2130706431 10.0.{}.{} 5000 typ host\r\n", i / 200, i % 200 + 1));
    }
    assert_eq!(sanitize(&many).matches("a=candidate:").count(), MAX_CANDIDATES);
    assert!(sanitize(&many).starts_with("v=0\r\n"));
}

#[test]
fn paths_are_classified() {
    let ours = LocalCandidates {
        base: Some(a("192.168.7.20:50000")),
        host: vec![a("192.168.7.20:50000")],
        relayed: vec![a("203.0.113.50:49170")],
        ..LocalCandidates::default()
    };
    let theirs = RemoteCandidates {
        host: vec![a("192.168.1.5:23400")],
        srflx: vec![a("198.51.100.7:23400")],
        relay: vec![a("203.0.113.9:49160")],
        mapped: Some(a("198.51.100.7:23401")),
    };
    let base = a("192.168.7.20:50000");
    assert_eq!(classify(base, a("192.168.1.5:23400"), &ours, &theirs), Path::Direct);
    assert_eq!(classify(base, a("198.51.100.7:23400"), &ours, &theirs), Path::HolePunched);
    assert_eq!(classify(base, a("198.51.100.7:40000"), &ours, &theirs), Path::HolePunched); // peer-reflexive
    assert_eq!(classify(base, a("198.51.100.7:23401"), &ours, &theirs), Path::MappedPort);
    assert_eq!(classify(base, a("203.0.113.9:49160"), &ours, &theirs), Path::Relayed);
    assert_eq!(classify(a("203.0.113.50:49170"), a("198.51.100.7:23400"), &ours, &theirs), Path::Relayed);
}
```

`crates/rl-net/tests/core_wire.rs` gains `a_full_pending_table_evicts_a_silent_setup` (PF12b-2):
- eight signalled offers come from spare client `Core`s that are never polled again (silent);
- a ninth from a live client `Core`, pumped on the wire, connects;
- then, with eight live offerers pumped but every host datagram to them dropped, so each has sent checks, a tenth gets `Signal::Refused { reason: Refusal::Busy }`.

Also:
- Move the STUN codec's tests into rl-proto (RFC 5769 vector, fuzz `responses_never_panic`). Add `is_stun` unit tests.
- Move `is_global`'s range tests into rl-proto; rl-discovery's list tests keep passing through the re-export.
- Fuzz `sanitize_and_remote_candidates_never_panic` (proptest: arbitrary strings, and valid SDPs with mutated candidate lines). `sanitize`'s output never has more than `MAX_CANDIDATES` candidate lines, and `remote_candidates` never returns an address `sanitize` would drop.
- Add `natsim` unit tests in `src/natsim.rs`: a port-restricted NAT drops an unsolicited datagram, a full cone passes it, a symmetric NAT gives two destinations two ports.

- [ ] **Step 2: Run `cargo nextest run -p rl-net -p rl-proto -p rl-discovery`, expect FAIL** (missing modules and items).
- [ ] **Step 3: Implement.**
  - **The codec move:** move the binding codec to `rl_proto::stun` with `core::net` and re-export it from `rl_discovery::stun` (its API unchanged). Move `is_global` to `rl_proto::addr` the same way.
  - **rl-net:** add `candidates`, `probe`, `Link::{offerer_with, answerer_with, selected_pair}`, `Core`'s additions, `NetEvent::Connected::path` (TCP-signalled links: `Direct`), and `natsim`.
  - **`Path` in the lobby:** `Member` gains `path: Option<Path>`, appended as a plain field (postcard has no defaults; the protocol is unreleased, N61). rl-lobby's `LobbyHost::connected` gains `path: Path` after `addr`, stored per member. rl-online passes the `path` of `NetEvent::Connected { path, .. }`. Update every file listed above that constructs `Member` or calls `connected`.
- [ ] **Step 4: Run `cargo nextest run -p rl-net -p rl-proto -p rl-discovery -p rl-lobby -p rl-online`, expect PASS; then `cargo xtask check`.**
- [ ] **Step 5: Update the CLAUDE.md of rl-net, rl-proto, rl-lobby and rl-discovery** (the codec moved). **Commit** `rl-net: candidates, STUN on the game socket, paths and a virtual NAT network`.

---

### Task 3: TURN: the relay core and server, and relayed candidates in `Core`

**Files:**
- Create:
  - `crates/rl-net/src/{turn.rs, relayed.rs}`;
  - `crates/rl-net/tests/turn.rs`.
- Modify:
  - `Cargo.toml` (workspace pins `turn-client-proto = "=0.7.2"`, `turn-server-proto = "=0.7.2"`, with `default-features` kept so `std` is on);
  - `crates/rl-net/{Cargo.toml, CLAUDE.md}`, `crates/rl-net/src/{core.rs, natsim.rs, lib.rs}`;
  - `crates/rl-net/tests/natsim.rs`;
  - `Cargo.lock` (pin-backs: `cargo update -p stun-proto --precise 2.0.2`, `-p stun-types --precise 2.0.2`, `-p turn-types --precise 0.7.2`).
- Test: `crates/rl-net/tests/{turn.rs, natsim.rs}`, unit tests in `turn.rs`

**Interfaces:**
- Consumes: Task 2's `Core`, `LinkCandidates`, `natsim::Net`.
- Produces (Tasks 5, 7, 8, 10, 11):
  ```rust
  // rl_net::turn
  pub const REALM: &str = "rust-liero";
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub struct TurnLimits { pub capacity: usize, pub per_ip_allocations: usize, pub per_ip_bytes_per_s: u32, pub per_ip_requests_per_s: u32 }
  impl Default for TurnLimits { /* 32, 4, 32 * 1024, 20 */ }
  impl TurnLimits { /// For tests that share 127.0.0.1: 256 allocations, 64 per IP, 64 MiB/s, 10 000 requests/s.
      pub fn generous() -> TurnLimits; }
  #[derive(Clone, Debug, PartialEq, Eq)] pub struct TurnOut { pub from_port: u16, pub to: SocketAddr, pub bytes: Vec<u8> }
  /// The library asked for a relay socket for `client`: the driver picks the port and calls `allocated`.
  #[derive(Clone, Debug, PartialEq, Eq)] pub struct AllocationRequest { pub client: SocketAddr, pub(crate) id: u64 }
  pub struct TurnCore { .. }
  impl TurnCore {
      /// Listens on `listen`; relayed addresses are `public` with the port the driver allocated.
      pub fn new(listen: SocketAddr, public: IpAddr, user: &str, pass: &str, limits: TurnLimits, now: Instant) -> TurnCore;
      pub fn receive(&mut self, from: SocketAddr, bytes: &[u8], now: Instant);            // to the listening port
      pub fn receive_relayed(&mut self, port: u16, from: SocketAddr, bytes: &[u8], now: Instant); // to a relay port
      pub fn poll(&mut self, now: Instant, out: &mut Vec<TurnOut>);
      /// Allocations the limits admitted, waiting for a port (refused ones never appear: TurnCore answers them).
      pub fn take_allocation_requests(&mut self) -> Vec<AllocationRequest>;
      /// The driver's answer: `Some(port)` bound, `None` no port free (the client gets 508).
      pub fn allocated(&mut self, req: AllocationRequest, port: Option<u16>, now: Instant);
      /// Relay ports whose allocation ended since the last call (the driver closes them).
      pub fn released(&mut self) -> Vec<u16>;
      pub fn allocations(&self) -> usize;
      pub fn dropped(&self) -> u64; // datagrams over a limit
  }
  #[derive(Clone, Debug)]
  pub struct TurnServerConfig {
      pub listen: SocketAddr,           // port 0: an ephemeral port (`local_addr` tells)
      pub public: IpAddr,
      /// Relay ports; `0..=0`: an OS-assigned ephemeral port per allocation (tests).
      pub ports: RangeInclusive<u16>,
      pub user: String, pub pass: String, pub limits: TurnLimits,
  }
  pub struct TurnServer { .. } // a thread: the listening socket and one UDP socket per allocation
  impl TurnServer {
      pub fn start(cfg: TurnServerConfig) -> io::Result<TurnServer>;
      pub fn local_addr(&self) -> SocketAddr;
      pub fn stats(&self) -> TurnStats; // allocations, dropped, bytes relayed
  }
  impl Drop for TurnServer {} // stops and joins within 1 s
  #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)] pub struct TurnStats { pub allocations: usize, pub dropped: u64, pub relayed_bytes: u64 }

  // rl_net::relayed (used by Core; TurnSpec is Task 2's rl_net::candidates::TurnSpec)
  #[derive(Clone, Debug, PartialEq, Eq)] pub enum RelayState { Allocating, Allocated(SocketAddr), Failed(String), Released }
  pub const TURN_WAIT: Duration = Duration::from_secs(2);
  // Core additions
  impl Core {
      /// Allocates on each server (at most 2), from this socket.
      pub fn allocate(&mut self, servers: &[TurnSpec], now: Instant);
      pub fn relays(&self) -> Vec<(SocketAddr, RelayState)>;
  }
  // natsim additions
  impl Net { /// Relay ports from at.port() + 1 upward on the same IP; TurnLimits::default().
      pub fn turn_server(&mut self, at: SocketAddr) -> TurnSpec; pub fn turn_dropped(&self, at: SocketAddr) -> u64; pub fn kill(&mut self, at: SocketAddr); }
  ```

Context:
- **The server core** (`TurnCore`) wraps `turn_server_proto::server::TurnServer` (UDP) with `add_user(user, pass)`.
  - **Allocations.** On `TurnServerPollRet::AllocateSocket` it checks `capacity` and the client IP's `per_ip_allocations`, and refuses with `SocketAllocateError` when either is reached (the library maps that to a 486/508 error). Otherwise it hands an `AllocationRequest` to its driver, which picks the port:
    - `TurnServer` binds a free port of `ports`, or an OS-assigned one for `0..=0`;
    - `natsim` takes the next port of its range.

    The driver reports the port with `allocated`, and `TurnCore` calls the library's `allocated_socket` with `public:port`.
  - **Limits per client IP:**
    - a token bucket of `per_ip_bytes_per_s` holding 2 s, counting relayed payload bytes in each direction separately;
    - one of `per_ip_requests_per_s` (burst 2 s) for STUN/TURN requests.

    Over either, the datagram is dropped and counted; nothing is fatal.
  - **Bounded tables:** at most 4096 client IPs (the oldest idle evicted). Binding requests are answered (the library does): a relay is also a STUN server.
  - The library's time type is `stun_proto::Instant` (`sans_io_time::Instant`). Convert with `Instant::from_nanos(now.duration_since(epoch).as_nanos() as i64)` from the core's creation instant.
- **The client** (`relayed.rs`, inside `Core`): one `turn_client_proto::udp::TurnClientUdp` per server (`TurnConfig::new(TurnCredentials::new(user, pass))`).
  - **Demultiplexing:** a datagram from a TURN server address goes to its client first. `TurnRecvRet::PeerData` goes to the links as arriving at that allocation's relayed address. `Ignored` falls through.
  - **Candidates:** `AllocationCreated` adds `Candidate::relayed(relayed, base, "udp")` to links made afterwards.
  - **Sending:** a link's `Transmit` whose `from` is a relayed address is sent through that client's `send_to(peer)`. Bind a channel for each peer address sent to (`bind_channel`), so data uses 4-byte ChannelData headers once bound.
  - **Permissions:** when an answer arrives, permissions are created for every remote candidate IP.
  - **Release:** an allocation unused by the open link is `delete`d 5 s after `Connected` on another path. A link whose relay dies is lost by ICE (`Disconnected { Timeout }`).
- **`gathered`** now also waits for every allocation to settle (allocated or failed), or `TURN_WAIT`.

- [ ] **Step 1: Write the failing tests.** Append to `crates/rl-net/tests/natsim.rs`:

```rust
fn join_relayed(host_nat: NatKind, client_nat: NatKind) -> (Net, NodeId, Option<Path>) {
    let mut net = Net::new(11);
    let s1 = net.stun_server(a("198.51.100.1:3478"));
    let turn = net.turn_server(a("198.51.100.9:3478"));
    let nh = net.nat(host_nat, "203.0.113.10".parse().unwrap());
    let nc = net.nat(client_nat, "203.0.113.20".parse().unwrap());
    let host = net.host(Some(nh), a("192.168.1.10:23400"));
    let client = net.client(Some(nc), a("192.168.7.20:50000"));
    net.gather(host, &[s1], &[]);
    net.gather(client, &[s1], &[turn]);
    assert!(net.run_until(Duration::from_secs(3), |n| n.gathered(host) && n.gathered(client)));
    net.signal(client, host).expect("answered");
    net.run_until(Duration::from_secs(15), |n| n.connected(client).is_some());
    let path = net.connected(client);
    (net, client, path)
}

#[test]
fn symmetric_against_port_restricted_goes_through_the_relay() {
    assert_eq!(join_relayed(NatKind::PortRestricted, NatKind::Symmetric).2, Some(Path::Relayed));
}

#[test]
fn symmetric_against_symmetric_goes_through_the_relay() {
    assert_eq!(join_relayed(NatKind::Symmetric, NatKind::Symmetric).2, Some(Path::Relayed));
}

#[test]
fn a_hole_punched_link_releases_its_allocation() {
    let (mut net, client, path) = join_relayed(NatKind::PortRestricted, NatKind::PortRestricted);
    assert_eq!(path, Some(Path::HolePunched));
    net.run_until(Duration::from_secs(6), |_| false);
    assert!(!net.core(client).relays().is_empty(), "the client allocated");
    assert!(net.core(client).relays().iter().all(|(_, s)| *s == rl_net::relayed::RelayState::Released));
}

#[test]
fn a_dead_relay_ends_the_link() {
    let (mut net, client, path) = join_relayed(NatKind::PortRestricted, NatKind::Symmetric);
    assert_eq!(path, Some(Path::Relayed));
    net.kill(a("198.51.100.9:3478"));
    assert!(net.run_until(Duration::from_secs(30), |n| n
        .events(client)
        .iter()
        .any(|e| matches!(e, rl_net::endpoint::NetEvent::Disconnected { .. }))));
}
```

`crates/rl-net/tests/turn.rs` (real sockets on 127.0.0.1, a `TurnServer` on port 0 with ephemeral relay ports (`ports: 0..=0`) and `TurnLimits::generous()` unless named, and two plain UDP sockets acting through `turn_client_proto` directly):
- `a_client_allocates_and_relays_both_ways`;
- `turn_limits_hold_per_ip` (`TurnLimits::default()`): a fifth allocation from one IP is refused; 200 KiB pushed in 1 s through one allocation delivers at most `2 * 32 KiB + 32 KiB` and `dropped() > 0`;
- `a_full_relay_refuses_new_allocations` (`TurnLimits { capacity: 2, ..TurnLimits::default() }`, a third client refused while the first two keep relaying);
- `binding_requests_are_answered` (with `rl_proto::stun`);
- fuzz `arbitrary_datagrams_never_panic_the_server` (proptest into `TurnCore::receive` and `receive_relayed`).

- [ ] **Step 2: Run `cargo nextest run -p rl-net natsim turn`, expect FAIL.**
- [ ] **Step 3: Implement**, then apply the `Cargo.lock` pin-backs and check that `cargo tree -p rl-net -e normal | grep -E "stun-proto|stun-types|turn-types"` shows 2.0.2/2.0.2/0.7.2.
- [ ] **Step 4: Run `cargo nextest run -p rl-net`, expect PASS. Then `cargo build --release --target x86_64-pc-windows-gnu -p rl-net`, then `cargo xtask check`.**
- [ ] **Step 5: Update `crates/rl-net/CLAUDE.md`** (TURN core, client, limits, pins). **Commit** `rl-net: TURN relays: the relay core and server, relayed candidates`.

---

### Task 4: `rl-net`: port mapping (PCP, NAT-PMP, UPnP-IGD)

**Files:**
- Create: `crates/rl-net/src/portmap/{mod.rs, pmp.rs, upnp.rs, fake.rs}`, `crates/rl-net/tests/portmap.rs`
- Modify: `Cargo.toml` (workspace `igd-next = "=0.17.1"`), `crates/rl-net/{Cargo.toml, CLAUDE.md}`, `crates/rl-net/src/lib.rs`
- Test: `crates/rl-net/tests/portmap.rs`, unit tests in `pmp.rs`

**Interfaces:**
- Produces (Tasks 5, 8):
  ```rust
  // rl_net::portmap
  #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum Method { Upnp, NatPmp, Pcp }
  impl std::fmt::Display for Method {} // "UPnP", "NAT-PMP", "PCP"
  #[derive(Clone, Debug, PartialEq, Eq)]
  pub enum MappingState {
      Off,
      Trying,
      /// UDP on `external`'s port, and TCP on the same port when `tcp`; `usable`: its IP is global
      /// (`rl_proto::is_global`) or `allow_local`.
      Mapped { method: Method, external: SocketAddrV4, tcp: bool, usable: bool },
      /// Every method failed: why, per method, in words.
      Failed(String),
  }
  #[derive(Clone, Debug)]
  pub struct MapRequest {
      pub local: SocketAddrV4,          // the base address and the game port
      pub tcp: bool,                    // also TCP on the same external port (F-G)
      pub gateway: Option<Ipv4Addr>,    // the default gateway (Task 5's `local::gateway`)
      pub allow_local: bool,
      pub description: &'static str,    // "rust-liero"
      /// The external port must equal the local one (a relay's ports: no +1 … +7 shift).
      pub exact: bool,
      /// None: 2 h for PCP/NAT-PMP, 3600 s for UPnP. Tests set seconds.
      pub lifetime: Option<Duration>,
      pub pmp_port: u16,                // 5351; tests: a FakeGateway's port
      pub ssdp: SocketAddr,             // 239.255.255.250:1900; tests: a FakeGateway's (never the real group)
  }
  impl MapRequest { pub fn new(local: SocketAddrV4, tcp: bool, gateway: Option<Ipv4Addr>) -> MapRequest; }
  pub struct PortMapper { .. }       // a thread per method (PCP/NAT-PMP, UPnP), each with its own deadline; never blocks the caller
  impl PortMapper {
      pub fn start(req: MapRequest) -> PortMapper;
      /// The state when it changed since the last poll.
      pub fn poll(&mut self) -> Option<MappingState>;
      pub fn state(&self) -> &MappingState;
  }
  impl Drop for PortMapper {} // deletes the mappings, bounded at 1 s
  // rl_net::portmap::pmp (codec, sans-IO, fuzzed)
  pub fn pmp_external_request() -> [u8; 2];
  pub fn pmp_map_request(udp: bool, internal: u16, external: u16, lifetime: u32) -> [u8; 12];
  #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum PmpResponse { External { result: u16, ip: Ipv4Addr }, Map { udp: bool, result: u16, internal: u16, external: u16, lifetime: u32 } }
  pub fn parse_pmp(bytes: &[u8]) -> Option<PmpResponse>;
  pub fn pcp_map_request(nonce: [u8; 12], client: Ipv4Addr, protocol: u8, internal: u16, suggested: SocketAddrV4, lifetime: u32) -> Vec<u8>;
  #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub struct PcpResponse { pub version: u8, pub result: u8, pub lifetime: u32, pub protocol: u8, pub internal: u16, pub external: SocketAddrV4 }
  pub fn parse_pcp(bytes: &[u8], nonce: &[u8; 12]) -> Option<PcpResponse>;
  // rl_net::portmap::fake (feature test-hooks)
  #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum Misbehaviour { Honest, HttpSilent, OtherUdpPort, PcpRefuses, PrivateExternal, RenewRefused, /// garbage in every SSDP, HTTP and PCP/NAT-PMP answer
      Garbage }
  pub struct FakeGateway { .. }
  impl FakeGateway {
      pub fn start(upnp: bool, pmp: bool, pcp: bool, external: Ipv4Addr, m: Misbehaviour) -> FakeGateway; // on 127.0.0.1
      pub fn ssdp(&self) -> SocketAddr; pub fn pmp(&self) -> SocketAddr;
      pub fn mappings(&self) -> Vec<(Method, bool /* udp */, u16 /* external */, SocketAddrV4)>;
      /// Fuzzing: SSDP and HTTP answer every request with `answer`; PCP/NAT-PMP stay silent.
      pub fn serving(answer: Vec<u8>) -> FakeGateway;
  }
  ```

Context (N51):
- **NAT-PMP** (RFC 6886 §3): version 0, ops 0 (external address), 1 (UDP), 2 (TCP). Responses add 128 to the op, carry a 16-bit result code and seconds since epoch. Map response layout: `vers, op, result(2), epoch(4), internal(2), external(2), lifetime(4)`.
- **PCP** (RFC 6887 §7, §11):
  - Version 2, opcode MAP (1). The request header has the client IP as an IPv4-mapped IPv6 address; the MAP payload is nonce(12), protocol (6/17), reserved(3), internal port, suggested external port, suggested external address (v4-mapped).
  - Any PCP failure falls back to NAT-PMP: a refusal (a nonzero result, as `PcpRefuses` gives), a NAT-PMP-only gateway's version 0 with `UNSUPP_VERSION` (1), or silence after the retries.
  - The response must echo our nonce.
- **Retries:** 250, 500 and 1000 ms per request.
- **Gateway port:** `pmp_port` exists so tests aim at the fake gateway on 127.0.0.1. When `gateway` is `None`, PCP and NAT-PMP are skipped, and so said.
- **UPnP:** `igd_next::search_gateway(SearchOptions { broadcast_address: req.ssdp, timeout: Some(2 s), single_search_timeout: Some(2 s), ..Default::default() })`, then `get_external_ip`, `add_port(UDP, ext, local, 3600, "rust-liero")` (and TCP). Retry with lease 0 on `AddPortError::OnlyPermanentLeasesSupported`. Port conflicts try `ext + 1 … + 7`, except with `exact`.
- **TCP and UDP must share one external port.** If NAT-PMP grants another UDP port than TCP's, ask once more for UDP at TCP's port. If that fails, delete both and try the next port. If no pair matches, keep the UDP mapping alone, reported as `Mapped { tcp: false, .. }` (the direct TCP path then cannot use it, N47).
- **The UI thread never waits:**
  - `PortMapper::start` spawns one thread per method: `rl-net pcp` (PCP, then NAT-PMP) and `rl-net upnp`. Each has its own deadline (3 s for PCP/NAT-PMP with retries, 4 s for UPnP's search and HTTP).
  - The first `Mapped` wins; the other thread deletes anything it mapped and ends.
  - `poll` reads a channel. `Drop` signals, deletes, and joins within 1 s, detaching a thread stuck in a router's HTTP.
- **Renewal** happens at half the lifetime. A refused renewal becomes `Failed("the router dropped the mapping: …")`.
- **The fake gateway** runs on threads:
  - SSDP replies with `LOCATION: http://127.0.0.1:<port>/desc.xml`;
  - a tiny HTTP server serves a `WANIPConnection:1` description and SCPD, and answers SOAP `GetExternalIPAddress`, `AddPortMapping` and `DeletePortMapping`;
  - one UDP socket answers PCP and NAT-PMP.

  `Misbehaviour` covers Review Focus 2.

- [ ] **Step 1: Write the failing tests.** Unit tests in `pmp.rs`:

```rust
#[test]
fn nat_pmp_requests_and_responses_have_rfc_6886_layouts() {
    assert_eq!(pmp_external_request(), [0, 0]);
    assert_eq!(pmp_map_request(true, 23400, 23400, 7200), [0, 1, 0, 0, 0x5b, 0x68, 0x5b, 0x68, 0, 0, 0x1c, 0x20]);
    let ext = [0, 128, 0, 0, 0, 0, 0, 9, 203, 0, 113, 7];
    assert_eq!(parse_pmp(&ext), Some(PmpResponse::External { result: 0, ip: "203.0.113.7".parse().unwrap() }));
    let map = [0, 129, 0, 0, 0, 0, 0, 9, 0x5b, 0x68, 0x5b, 0x69, 0, 0, 0x1c, 0x20];
    assert_eq!(parse_pmp(&map), Some(PmpResponse::Map { udp: true, result: 0, internal: 23400, external: 23401, lifetime: 7200 }));
    assert_eq!(parse_pmp(&map[..15]), None);
}

#[test]
fn pcp_map_round_trips_its_nonce() {
    let nonce = [7u8; 12];
    let req = pcp_map_request(nonce, "192.168.1.10".parse().unwrap(), 17, 23400, "0.0.0.0:23400".parse().unwrap(), 7200);
    assert_eq!(req.len(), 60);
    assert_eq!((req[0], req[1] & 0x7f), (2, 1));
    // A response: version 2, R|opcode, reserved, result 0, lifetime, epoch, reserved(12), then the MAP payload.
    let mut resp = vec![2, 0x81, 0, 0, 0, 0, 0x1c, 0x20, 0, 0, 0, 9];
    resp.extend([0u8; 12]);
    resp.extend(nonce);
    resp.extend([17, 0, 0, 0, 0x5b, 0x68, 0x5b, 0x68]);
    resp.extend([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff, 203, 0, 113, 7]);
    assert_eq!(
        parse_pcp(&resp, &nonce),
        Some(PcpResponse { version: 2, result: 0, lifetime: 7200, protocol: 17, internal: 23400, external: "203.0.113.7:23400".parse().unwrap() })
    );
    assert_eq!(parse_pcp(&resp, &[8u8; 12]), None, "another nonce");
}
```

`crates/rl-net/tests/portmap.rs`:
Every test below builds its `MapRequest` with `ssdp` set to the fake gateway's `ssdp()` (or to a closed 127.0.0.1 port when the fake has no UPnP), `pmp_port` set to the fake's, and `gateway: Some(127.0.0.1)` (or `None` where named). The fake's external address 203.0.113.7 is TEST-NET, not global, so every case expecting `usable: true` sets `allow_local: true`. No test sends to 239.255.255.250 or to a real gateway.
- `upnp_maps_tcp_and_udp_on_one_port` (`FakeGateway::start(true, false, false, …)`, `gateway: None`, `allow_local: true`: `Mapped { Upnp, 203.0.113.7:<local port>, tcp: true, usable: true }`, and both protocols are in `mappings()`);
- `pcp_maps_first_and_nat_pmp_is_the_fallback` (pcp-only gateway: `Pcp`; pmp-only: `NatPmp`);
- `fake_gateway_misbehaviours_are_reported`:
  - `HttpSilent` (UPnP only) → `Failed` mentioning UPnP within 5 s, and `poll` never blocks over 5 ms;
  - `OtherUdpPort` → still one external port for both;
  - `PcpRefuses` → NAT-PMP;
  - `PrivateExternal` (external 10.0.0.1, `allow_local: false`) → `usable: false`;
  - `RenewRefused` with `lifetime: Some(2 s)` → `Failed("the router dropped the mapping…")`;
  - `Garbage` → `Failed`, no panic;
- `an_exact_request_never_shifts` (the fake holds the local port already: `exact: true` → `Failed`, not another port);
- `mappings_are_deleted_on_drop` (`mappings()` empty within 1.5 s of dropping the mapper);
- `no_gateway_and_no_upnp_fails_quickly` (`gateway: None`, `ssdp` a closed 127.0.0.1 port: `Failed` naming all three methods within 5 s);
- fuzz `pmp_and_pcp_responses_never_panic`;
- fuzz `upnp_answers_never_panic`: proptest bytes served by a `FakeGateway` in a mode that answers SSDP and HTTP with the given bytes; the mapper ends in `Failed`.
- [ ] **Step 2: Run `cargo nextest run -p rl-net portmap`, expect FAIL.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run, expect PASS; `cargo build --release --target x86_64-pc-windows-gnu -p rl-net`; `cargo xtask check`.**
- [ ] **Step 5: Update `crates/rl-net/CLAUDE.md`. Commit** `rl-net: port mapping through PCP, NAT-PMP and UPnP-IGD`.

---

### Task 5: The endpoint drives ICE on real sockets

**Files:**
- Create: `crates/rl-net/src/local.rs`, `crates/rl-net/tests/ice.rs`
- Modify: `Cargo.toml` (workspace `netdev = { version = "=0.46.3", default-features = false, features = ["gateway"] }`: `get_default_interface` and `Interface::gateway` exist only with `gateway`), `crates/rl-net/{Cargo.toml, CLAUDE.md}`, `crates/rl-net/src/{endpoint.rs, lib.rs}`
- Test: `crates/rl-net/tests/ice.rs`

**Interfaces:**
- Consumes: Tasks 2-4 (`Core` ICE API, `TurnSpec`, `TurnServer`, `PortMapper`, `MappingState`, `NatBehaviour`).
- Produces (Tasks 7, 8, 10):
  ```rust
  // rl_net::local
  /// The default interface's IPv4 addresses (≤ 2) and gateway; else the OS's source address toward `toward`; else 127.0.0.1.
  pub fn base_addresses(toward: Option<SocketAddr>) -> (Vec<Ipv4Addr>, Option<Ipv4Addr> /* gateway */);
  // rl_net::endpoint (additions)
  #[derive(Clone, Debug, Default, PartialEq, Eq)]
  pub struct TurnServerSpec { pub addr: String /* host:port */, pub user: String, pub pass: String }
  #[derive(Clone, Debug, Default)]
  pub struct IceConfig {
      pub stun: Vec<String>,            // host:port; resolved on a thread; [] asks none
      pub turn: Vec<TurnServerSpec>,    // a signalled client's; at most 2 used
      pub port_mapping: bool,           // a host's
      pub allow_local: bool,
      #[cfg(feature = "test-hooks")] pub base_override: Option<Ipv4Addr>,
      #[cfg(feature = "test-hooks")] pub relay_only: bool,      // a client's links get only relayed candidates
      #[cfg(feature = "test-hooks")] pub gateway: Option<SocketAddr>, // a FakeGateway's PCP/NAT-PMP port
      #[cfg(feature = "test-hooks")] pub ssdp: Option<SocketAddr>,
  }
  // EndpointConfig gains `pub ice: IceConfig` (default: nothing asked)
  #[derive(Clone, Debug, PartialEq)]
  pub struct Reach { pub base: Option<SocketAddr>, pub srflx: Option<SocketAddr>, pub nat: NatBehaviour, pub mapping: MappingState, pub relays: Vec<(SocketAddr, RelayState)> }
  pub const GATHER_DEADLINE: Duration = Duration::from_secs(2);
  pub use crate::core::{MAX_SIGNALLED_PENDING, SIGNALLED_SETUP_TIMEOUT}; // Task 2's, re-exported
  impl Endpoint {
      /// A client for a signalled (Nostr) join: binds UDP on cfg.port, gathers per cfg.ice.
      pub fn signalled(cfg: EndpointConfig) -> io::Result<Endpoint>;
      /// Gathering is done (or GATHER_DEADLINE passed).
      pub fn gathered(&self) -> bool;
      /// The offer, once gathered: `Signal::Offer` with every candidate (a client sends no mapped hint).
      pub fn offer(&mut self, now: Instant) -> Option<Signal>;
      /// The host's reply: an Answer starts ICE; a Refused ends with NetEvent::Refused.
      pub fn take_reply(&mut self, reply: &Signal, mapped_hint: Option<SocketAddr>, now: Instant);
      /// Host: a signalled offer (admitted, sanitized; PF12b-2 when the pending table is full) → Answer or Refused.
      pub fn answer_signalled(&mut self, offer: &Signal, mapped_hint: Option<SocketAddr>, now: Instant) -> Signal;
      /// Host: our router mapping's external address, when usable (for the hint and N47).
      pub fn mapped(&self) -> Option<SocketAddr>;
      pub fn reach(&self) -> Reach;
      pub fn path(&self, peer: PeerId) -> Option<Path>;
  }
  ```

Context:
- **Startup.**
  - **`Endpoint::host`:** besides today's work:
    - `set_base` from `local::base_addresses` (the first STUN server resolved as `toward`);
    - `Core::stun` once the names resolve (a `rl-net dns` thread);
    - with `port_mapping`, a `PortMapper` for the UDP and TCP port on the base address and the gateway (or the test hooks' fake). Its state goes into `reach()`. Its usable external address goes to `Core::set_mapped`.
  - **`Endpoint::signalled`:** a UDP socket, base and STUN as the host's, and `Core::allocate` for `cfg.ice.turn` (resolved).
- **Limits.** `answer_signalled` is `Core::answer_ice`: with `MAX_SIGNALLED_PENDING` signalled setups pending, the oldest one that has heard no ICE connectivity check from its peer is evicted (PF12b-2); `Refusal::Busy` only when every pending one has. Signalled setups get `SIGNALLED_SETUP_TIMEOUT`.
- **`Connected { addr }`** for a signalled link is the selected remote address; the lobby's IP checks use it.
- **`relay_only`** (test hook): the client's links are built with only relayed candidates in their `LinkCandidates` (no host or server-reflexive candidate is added to str0m, not merely filtered out of the SDP). str0m then cannot pair a host candidate peer-reflexively, so on 127.0.0.1 the path really is `Relayed`.
- **No default STUN or TURN.** `IceConfig::default()` asks nothing: every existing caller and test is unchanged until Task 7 or 8 passes settings.

- [ ] **Step 1: Write the failing tests** `crates/rl-net/tests/ice.rs` (127.0.0.1, port 0, `base_override: Some(127.0.0.1)`, `TurnServer`s on port 0 with `ports: 0..=0` and `TurnLimits::generous()`, every wait bounded):
  - `a_signalled_client_connects_to_a_host`: `Endpoint::host` + `Endpoint::signalled`; offer → `answer_signalled` → `take_reply`; both `Connected`, the path `Direct`, a reliable message both ways.
  - `a_signalled_client_connects_through_a_local_turn_server`: `TurnServer` on 127.0.0.1, client `relay_only: true`; path `Relayed`, 1000 reliable messages arrive in order, and `TurnServer::stats().relayed_bytes > 0`.
  - `stun_on_the_game_socket_learns_the_reflexive_address`: the `TurnServer` answers bindings; `reach().srflx == Some(127.0.0.1:<the endpoint's port>)` and `nat == Open`.
  - `a_host_maps_its_port_on_a_fake_gateway`: `FakeGateway` UPnP through the hooks `ssdp`/`gateway`, `allow_local: true` (203.0.113.7 is TEST-NET); `reach().mapping` is `Mapped { usable: true, .. }`, `mapped()` is the fake's external address.
  - `the_signalled_pending_table_is_bounded` (eight silent offers pending, from offerer `Link`s never pumped; a ninth from a live `Endpoint::signalled` still connects: PF12b-2 on real sockets).
  - `the_direct_path_is_unchanged`: `Endpoint::connect`, path `Direct`.
  - `gathering_ends_by_the_deadline` (a STUN server on a silent port: `gathered()` within `GATHER_DEADLINE` + 100 ms).
- [ ] **Step 2: Run `cargo nextest run -p rl-net ice`, expect FAIL.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run `cargo nextest run -p rl-net`, expect PASS; `cargo build --release --target x86_64-pc-windows-gnu -p rl-net`; `cargo xtask check`; `cargo xtask full`** (network-sensitive).
- [ ] **Step 5: Update `crates/rl-net/CLAUDE.md`. Commit** `rl-net: the endpoint gathers ICE candidates, maps its port and answers signalled offers`.

---

### Task 6: `rl-discovery`: NIP-44, handshake events, TURN relay notes

**Files:**
- Create:
  - `crates/rl-discovery/src/{nip44.rs, handshake.rs, turn_note.rs}`;
  - `crates/rl-discovery/tests/{nip44.rs, handshake.rs, turn_note.rs}`;
  - `crates/rl-discovery/tests/fixtures/nip44.vectors.json` (fetched once by hand, below).
- Modify:
  - `Cargo.toml` (workspace pins `hkdf =0.13.0`, `hmac =0.13.0`, `chacha20 =0.10.2`, `base64 =0.22.1`; `k256` features gain `"ecdh"`);
  - `crates/rl-discovery/{Cargo.toml, CLAUDE.md}`, `crates/rl-discovery/src/{lib.rs, nostr.rs, test_relay.rs}`;
  - `crates/rl-cli/src/nostr_probe.rs` (kind 31471 and a `MAX_HANDSHAKE_CONTENT` (16 KiB) kind-21470 event).
- Test: the files above

**Interfaces:**
- Consumes: `Event`, `Filter`, `Identity`, `rl_proto::{Signal, PublicKey}`, `Region`, `list::is_global`.
- Produces (Tasks 7, 8, 10):
  ```rust
  // rl_discovery::nip44
  #[derive(Debug, PartialEq, Eq, thiserror::Error)] pub enum Nip44Error { #[error("unknown version")] Version, #[error("bad payload")] Payload, #[error("bad MAC")] Mac, #[error("bad padding")] Padding, #[error("bad length")] Length }
  pub fn conversation_key(secret: &[u8; 32], public: &PublicKey) -> Option<[u8; 32]>;
  pub fn message_keys(conversation_key: &[u8; 32], nonce: &[u8; 32]) -> ([u8; 32], [u8; 12], [u8; 32]);
  pub fn padded_len(len: usize) -> usize;
  pub fn encrypt(conversation_key: &[u8; 32], plaintext: &str, nonce: [u8; 32]) -> Result<String, Nip44Error>;
  pub fn decrypt(conversation_key: &[u8; 32], payload: &str) -> Result<String, Nip44Error>;
  // rl_discovery::handshake
  pub const MAX_HANDSHAKE_CONTENT: usize = 16 * 1024;
  pub const RESEND: Duration = Duration::from_secs(2);
  pub const ANSWER_DEADLINE: Duration = Duration::from_secs(10);
  pub const MAX_SKEW: u64 = 60;
  pub const SESSION_MEMORY: Duration = Duration::from_secs(120);
  #[derive(Clone, Debug, PartialEq, Eq)]
  pub struct Request { pub from: PublicKey, pub session: [u8; 16], pub signal: Signal, pub mapped: Option<SocketAddr> }
  #[derive(Clone, Debug, PartialEq, Eq)]
  pub enum Inbound { Offer(Request), /// a repeated session: publish this cached reply again
      Repeat(Event), Ignored(&'static str) }
  pub struct Inbox { .. }
  impl Inbox {
      pub fn new(me: Identity) -> Inbox;
      pub fn filter(&self, now_unix: u64) -> Filter;          // kinds [21470], #p [me], since now-60
      pub fn offer(&mut self, e: &Event, now: Instant, now_unix: u64) -> Inbound;
      pub fn reply(&mut self, req: &Request, signal: Signal, mapped: Option<SocketAddr>, now: Instant, now_unix: u64, rand: &mut dyn FnMut() -> [u8; 32]) -> Event;
      pub fn sessions(&self) -> usize;
  }
  #[derive(Clone, Debug, PartialEq, Eq)] pub struct Reply { pub signal: Signal, pub mapped: Option<SocketAddr> }
  pub struct Asker { .. }
  impl Asker {
      /// A fresh ephemeral key from `rand` (a draw that is no valid secret, such as all zeros or ≥ n, is drawn again); `offer` is a Signal::Offer.
      pub fn new(host: PublicKey, offer: Signal, now: Instant, now_unix: u64, rand: &mut dyn FnMut() -> [u8; 32]) -> Asker;
      pub fn key(&self) -> PublicKey;
      pub fn filter(&self, now_unix: u64) -> Filter;          // kinds [21470], #p [ephemeral], authors [host]
      /// The offer event to publish now: at once, then every RESEND (a new event, the same session).
      pub fn poll(&mut self, now: Instant, now_unix: u64, rand: &mut dyn FnMut() -> [u8; 32]) -> Option<Event>;
      pub fn take(&mut self, e: &Event) -> Option<Reply>;    // the host's reply to our session, decrypted
      pub fn expired(&self, now: Instant) -> bool;            // ANSWER_DEADLINE passed without a reply
  }
  // rl_discovery::turn_note
  pub const KIND_TURN: u32 = 31471;
  pub const TURN_TAG: &str = "rust-liero-turn";
  #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
  pub struct TurnNote { pub v: u16, pub addr: Ipv4Addr, pub port: u16, pub region: Region, pub capacity: u16, pub load: u16, pub user: String, pub pass: String }
  /// Tags d, t = TURN_TAG, expiration now + ttl (ANNOUNCE_TTL, 180, for a live relay; CLOSE_TTL, 10, with capacity 0 for its closing note).
  pub fn turn_note(identity: &Identity, note: &TurnNote, now_unix: u64, ttl: u64, aux: [u8; 32]) -> Event;
  pub fn parse_turn_note(e: &Event) -> Result<TurnNote, AnnounceError>;
  #[derive(Clone, Debug, PartialEq, Eq)] pub struct TurnPick { pub key: PublicKey, pub addr: SocketAddr, pub user: String, pub pass: String }
  pub struct TurnList { .. }
  impl TurnList {
      pub fn new() -> TurnList;
      pub fn allow_local(&mut self, allow: bool);
      pub fn filter(now_unix: u64) -> Filter;  // kind 31471, #t TURN_TAG, since now-180
      /// A closing note (capacity 0) is exempt from the 10 s per-key gap, as GameList exempts closes, and removes the relay.
      pub fn offer(&mut self, e: &Event, now_unix: u64) -> bool;
      pub fn expire(&mut self, now_unix: u64);
      /// Region first, then load/capacity ascending, then key: at most `n`; full relays (load >= capacity) never.
      pub fn pick(&self, region: Region, n: usize) -> Vec<TurnPick>;
      pub fn len(&self) -> usize;
  }
  // test_relay additions
  impl TestRelay { pub fn start_on(addr: SocketAddr) -> TestRelay; } // any address (Docker); start() = 127.0.0.1:0
  // Misbehaviour gains DropEphemeral(u32): drops the first n ephemeral events, then forwards.
  ```

Context:
- **The vectors.** Fetch the NIP-44 test vectors once by hand:

  ```bash
  curl -sSfo crates/rl-discovery/tests/fixtures/nip44.vectors.json https://raw.githubusercontent.com/paulmillr/nip44/main/nip44.vectors.json
  sha256sum crates/rl-discovery/tests/fixtures/nip44.vectors.json
  ```

  The hash must equal the one NIP-44 states in its "Tests" section (https://github.com/nostr-protocol/nips/blob/master/44.md). Record the hash in `CLAUDE.md`. If it differs, stop and report. No test fetches anything.
- **NIP-44 v2 as the NIP specifies** (follow its pseudocode exactly):
  - **Conversation key:** `conversation_key = HKDF-extract(salt = b"nip44-v2", ikm = shared_x)`, where `shared_x` is the x coordinate of `secret · lift_x(public)`. Use k256's `ecdh` with the secret scalar **as given** (`Identity::secret()`), and the public key as the even-y point `0x02 || x`.
  - **Message keys:** `message_keys = HKDF-expand(prk = conversation_key, info = nonce, L = 76)` → `chacha_key[0..32]`, `chacha_nonce[32..44]`, `hmac_key[44..76]`.
  - **Padding:** `u16` big-endian length, the plaintext, then zeros to `padded_len`. `padded_len(n)` is 32 for `n <= 32`. Otherwise `next = 1 << (floor(log2(n - 1)) + 1)`, `chunk = 32 if next <= 256 else next / 8`, and the result is `chunk * (floor((n - 1) / chunk) + 1)`. The plaintext is 1..=65535 bytes.
  - **Ciphertext and MAC:** the ciphertext is ChaCha20 (IETF, counter 0) of the padded plaintext. `mac = HMAC-SHA256(hmac_key, nonce || ciphertext)`, and the payload is `base64(0x02 || nonce || ciphertext || mac)`.
  - **Decrypt checks:** first `#` (unknown version), the base64 length 132..=87472, the decoded length 99..=65603, version 2, the MAC in constant time, then the padding.
- **Handshake events** (N45) use the existing `nostr::KIND_HANDSHAKE` (21470); no new constant.
  - **Content:** the encrypted JSON `{"v":1,"session":"<32 hex>","signal":…,"mapped":…}` with serde_json; `signal` is `rl_proto::Signal`'s serde form.
  - **Inbox checks:** the kind, the `p` tag equal to us, the size (`MAX_HANDSHAKE_CONTENT`), `created_at` within `MAX_SKEW`, the signature, decryption, the JSON, `v == 1`, the signal being an `Offer` with its SDP within `rl_proto::MAX_SDP`, and `mapped` an IPv4 socket address.
  - **Cheap checks first:** size, kind, tag and skew come before the signature and decryption, and the rate bucket (10 a second, burst 20) before decryption.
  - **Sessions:** at most 4096 remembered, the oldest dropped; each for `SESSION_MEMORY`.
  - **Asker:** `take` checks that the author is the host, then decrypts, the session, and that the signal is `Answer` or `Refused`.
- **TURN notes** (N52, N53) are validated like announcements:
  - the address global unless `allow_local`;
  - port nonzero;
  - user and pass 1..=64 printable ASCII;
  - `capacity` at most 4096;
  - one note per key per 10 s;
  - newest per key;
  - at most 64 relays.
- **Re-check the registry for 31471** (and 21470), as Task 2 of plan 12a did, and record it in `CLAUDE.md`. If either is taken, stop and report.
- **The probe:** extend `rl-cli nostr-probe` with a kind-31471 note (`t` = `rust-liero-probe`) and a kind-21470 event of `MAX_HANDSHAKE_CONTENT` (16 KiB; content is random base64). A maximal offer, an 8 KiB SDP, is about 13.7 KiB once encrypted. Run it **by hand once**. Record which default relays store 31471 for 3 minutes and forward a 16 KiB ephemeral event; the table goes into Task 12's STATUS text. A default relay failing either is noted, not removed (the list is append-only).

- [ ] **Step 1: Write the failing tests.** `crates/rl-discovery/tests/nip44.rs`:

```rust
use rl_discovery::nip44::{conversation_key, decrypt, encrypt, message_keys, padded_len};
use serde_json::Value;

fn vectors() -> Value {
    serde_json::from_str(include_str!("fixtures/nip44.vectors.json")).unwrap()
}

fn h32(s: &str) -> [u8; 32] {
    hex::decode(s).unwrap().try_into().unwrap()
}

#[test]
fn conversation_keys_match_the_vectors() {
    let v = vectors();
    for case in v["v2"]["valid"]["get_conversation_key"].as_array().unwrap() {
        let key = conversation_key(&h32(case["sec1"].as_str().unwrap()), &h32(case["pub2"].as_str().unwrap()));
        assert_eq!(key, Some(h32(case["conversation_key"].as_str().unwrap())), "{case}");
    }
    for case in v["v2"]["invalid"]["get_conversation_key"].as_array().unwrap() {
        let sec: Option<[u8; 32]> = hex::decode(case["sec1"].as_str().unwrap()).ok().and_then(|b| b.try_into().ok());
        let public: Option<[u8; 32]> = hex::decode(case["pub2"].as_str().unwrap()).ok().and_then(|b| b.try_into().ok());
        if let (Some(s), Some(p)) = (sec, public) {
            assert_eq!(conversation_key(&s, &p), None, "{}", case["note"]);
        }
    }
}

#[test]
fn message_keys_match_the_vectors() {
    let v = vectors();
    let mk = &v["v2"]["valid"]["get_message_keys"];
    let ck = h32(mk["conversation_key"].as_str().unwrap());
    for k in mk["keys"].as_array().unwrap() {
        let (chacha_key, chacha_nonce, hmac_key) = message_keys(&ck, &h32(k["nonce"].as_str().unwrap()));
        assert_eq!(hex::encode(chacha_key), k["chacha_key"].as_str().unwrap());
        assert_eq!(hex::encode(chacha_nonce), k["chacha_nonce"].as_str().unwrap());
        assert_eq!(hex::encode(hmac_key), k["hmac_key"].as_str().unwrap());
    }
}

#[test]
fn padding_matches_the_vectors() {
    for pair in vectors()["v2"]["valid"]["calc_padded_len"].as_array().unwrap() {
        assert_eq!(padded_len(pair[0].as_u64().unwrap() as usize), pair[1].as_u64().unwrap() as usize);
    }
}

#[test]
fn encryption_matches_the_vectors_both_ways() {
    for case in vectors()["v2"]["valid"]["encrypt_decrypt"].as_array().unwrap() {
        let ck = h32(case["conversation_key"].as_str().unwrap());
        let payload = encrypt(&ck, case["plaintext"].as_str().unwrap(), h32(case["nonce"].as_str().unwrap())).unwrap();
        assert_eq!(payload, case["payload"].as_str().unwrap());
        assert_eq!(decrypt(&ck, &payload).unwrap(), case["plaintext"].as_str().unwrap());
    }
}

#[test]
fn invalid_payloads_are_refused() {
    for case in vectors()["v2"]["invalid"]["decrypt"].as_array().unwrap() {
        let ck = h32(case["conversation_key"].as_str().unwrap());
        assert!(decrypt(&ck, case["payload"].as_str().unwrap()).is_err(), "{}", case["note"]);
    }
}
```

Plus fuzz `arbitrary_payloads_never_panic` (proptest strings and mutated valid payloads into `decrypt`).

`crates/rl-discovery/tests/handshake.rs`:

```rust
use std::time::{Duration, Instant};

use rl_discovery::handshake::{ANSWER_DEADLINE, Asker, Inbound, Inbox, RESEND};
use rl_lobby::Identity;
use rl_proto::{FINGERPRINT, PROTOCOL_VERSION, Refusal, Signal};

/// Distinct draws: SHA-256 of (seed, counter), never a repeated or all-zero value.
fn rand_from(seed: u64) -> impl FnMut() -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut n = 0u64;
    move || {
        n += 1;
        let mut h = Sha256::new();
        h.update(seed.to_le_bytes());
        h.update(n.to_le_bytes());
        h.finalize().into()
    }
}

fn offer() -> Signal {
    Signal::Offer { protocol: PROTOCOL_VERSION, fingerprint: FINGERPRINT, sdp: "v=0\r\n".into() }
}

#[test]
fn an_offer_and_its_answer_round_trip() {
    let (t0, unix) = (Instant::now(), 1_800_000_000);
    let host = Identity::from_secret([5; 32]).unwrap();
    let mut inbox = Inbox::new(host.clone());
    let mut rand = rand_from(1);
    let mut asker = Asker::new(host.public(), offer(), t0, unix, &mut rand);
    assert_ne!(asker.key(), host.public(), "a fresh key, never the player's");
    let sent = asker.poll(t0, unix, &mut rand).expect("sent at once");
    assert!(sent.tags.iter().any(|t| t[0] == "p" && t[1] == hex::encode(host.public())));
    let Inbound::Offer(req) = inbox.offer(&sent, t0, unix) else { panic!("not an offer") };
    assert_eq!(req.signal, offer());
    let mapped = Some("203.0.113.7:23400".parse().unwrap());
    let reply = inbox.reply(&req, Signal::Answer { sdp: "v=0 answer\r\n".into() }, mapped, t0, unix, &mut rand);
    let got = asker.take(&reply).expect("our reply");
    assert_eq!(got.signal, Signal::Answer { sdp: "v=0 answer\r\n".into() });
    assert_eq!(got.mapped, mapped);
}

#[test]
fn a_repeated_offer_gets_the_same_reply_and_resends_are_spaced() {
    let (t0, unix) = (Instant::now(), 1_800_000_000);
    let host = Identity::from_secret([5; 32]).unwrap();
    let mut inbox = Inbox::new(host.clone());
    let mut rand = rand_from(2);
    let mut asker = Asker::new(host.public(), offer(), t0, unix, &mut rand);
    let first = asker.poll(t0, unix, &mut rand).unwrap();
    assert!(asker.poll(t0 + RESEND / 2, unix, &mut rand).is_none());
    let second = asker.poll(t0 + RESEND, unix + 2, &mut rand).expect("re-sent");
    assert_ne!(first.id, second.id);
    let Inbound::Offer(req) = inbox.offer(&first, t0, unix) else { panic!() };
    let reply = inbox.reply(&req, Signal::Refused { reason: Refusal::Full }, None, t0, unix, &mut rand);
    assert_eq!(inbox.offer(&second, t0, unix + 2), Inbound::Repeat(reply));
    assert!(asker.expired(t0 + ANSWER_DEADLINE));
}

#[test]
fn a_hostile_handshake_feed_is_bounded() {
    let (t0, unix) = (Instant::now(), 1_800_000_000);
    let host = Identity::from_secret([5; 32]).unwrap();
    let other = Identity::from_secret([6; 32]).unwrap();
    let mut inbox = Inbox::new(host.clone());
    let mut rand = rand_from(3);
    // Addressed to someone else, too old, in the future: ignored before any decryption.
    let mut wrong = Asker::new(other.public(), offer(), t0, unix, &mut rand);
    assert!(matches!(inbox.offer(&wrong.poll(t0, unix, &mut rand).unwrap(), t0, unix), Inbound::Ignored(_)));
    let mut old = Asker::new(host.public(), offer(), t0, unix - 3600, &mut rand);
    assert!(matches!(inbox.offer(&old.poll(t0, unix - 3600, &mut rand).unwrap(), t0, unix), Inbound::Ignored(_)));
    let mut future = Asker::new(host.public(), offer(), t0, unix + 3600, &mut rand);
    assert!(matches!(inbox.offer(&future.poll(t0, unix + 3600, &mut rand).unwrap(), t0, unix), Inbound::Ignored(_)));
    // 2 000 fresh sessions (each its own key) within 2 ms: at most the burst (20) gets through.
    let mut taken = 0;
    for i in 0..2_000u64 {
        let mut a = Asker::new(host.public(), offer(), t0, unix, &mut rand_from(1_000 + i));
        let e = a.poll(t0, unix, &mut rand).unwrap();
        if matches!(inbox.offer(&e, t0 + Duration::from_micros(i), unix), Inbound::Offer(_)) {
            taken += 1;
        }
    }
    assert!(taken <= 20, "{taken}");
    // A steady 10 a second for 200 s of virtual time: every offer is taken, and memory is
    // bounded by SESSION_MEMORY (120 s × 10/s), far below the 4096 cap.
    let start = t0 + Duration::from_secs(10);
    for i in 0..2_000u64 {
        let (at, at_unix) = (start + Duration::from_millis(100 * i), unix + 10 + i / 10);
        let mut a = Asker::new(host.public(), offer(), at, at_unix, &mut rand_from(10_000 + i));
        let e = a.poll(at, at_unix, &mut rand).unwrap();
        assert!(matches!(inbox.offer(&e, at, at_unix), Inbound::Offer(_)), "offer {i}");
    }
    assert!((1_190..=1_222).contains(&inbox.sessions()), "{}", inbox.sessions());
    // A tampered content or signature is ignored, never a panic.
    let late = start + Duration::from_secs(300);
    let mut honest = Asker::new(host.public(), offer(), late, unix + 310, &mut rand);
    let mut e = honest.poll(late, unix + 310, &mut rand).unwrap();
    let flipped = if &e.content[10..11] == "A" { "B" } else { "A" };
    e.content.replace_range(10..11, flipped);
    assert!(matches!(inbox.offer(&e, late, unix + 310), Inbound::Ignored(_)));
}
```

Also fuzz `arbitrary_handshake_events_never_panic`.

`crates/rl-discovery/tests/turn_note.rs`:
- `turn_notes_round_trip`;
- `turn_notes_with_private_addresses_need_allow_local`;
- `pick_prefers_the_region_then_the_least_loaded_and_skips_full_relays` (four notes: Europe 10/32, Europe 30/32, Asia 0/32, Europe 32/32; `pick(Europe, 3)` = the first, the second, Asia; the full one never);
- `a_flood_of_turn_notes_is_bounded` (64 relays at most, 1 per key per 10 s);
- `a_closing_turn_note_removes_the_relay_at_once` (a note, then a `capacity: 0` note with `ttl` 10 two seconds later: accepted despite the gap, and `pick` no longer returns it);
- fuzz.
- [ ] **Step 2: Run `cargo nextest run -p rl-discovery`, expect FAIL.**
- [ ] **Step 3: Implement.** Then run the probe by hand (Context) and keep its output for Task 12.
- [ ] **Step 4: Run, expect PASS; `cargo xtask check`; `cargo build --release --target x86_64-pc-windows-gnu -p rl-discovery`.**
- [ ] **Step 5: Update `crates/rl-discovery/CLAUDE.md`** (NIP-44 with the vectors' hash, handshakes, TURN notes, the registry check, `start_on`, `DropEphemeral`). **Commit** `rl-discovery: NIP-44, handshake events and TURN relay notes`.

---

### Task 7: `rl-online`: joining through the relays, the host's listener, paths and reachability

**Files:**
- Create: `crates/rl-online/src/{join.rs, reach.rs}`, `crates/rl-online/tests/through.rs` (a `mod` of `tests/integration.rs`)
- Modify:
  - `xtask/layers.ron` (`("rl-online", "rl-discovery")` with a comment);
  - `crates/rl-online/{Cargo.toml, CLAUDE.md}` (`rl-discovery` dependency; dev-dependencies: `rl-discovery` with `test-relay`, `rl-net` with `test-hooks` for `base_override`, `relay_only` and `FakeGateway`);
  - `crates/rl-online/src/{peer.rs, events.rs, lib.rs}`;
  - `crates/rl-online/tests/online.rs` (it builds `PeerRole::Host(cfg)` and `Client { addr, .. }`);
  - `crates/rl-lobby/src/host.rs` (N59), `crates/rl-lobby/tests/lobby.rs`, `crates/rl-lobby/CLAUDE.md`;
  - callers of `PeerRole` (the minimal compile changes, `Join::direct(addr)` and `HostListen::off()`): `crates/rl-game/src/shell/online.rs`, `crates/rl-game/tests/online.rs`, `crates/rl-server/src/lib.rs`, `crates/rl-cli/src/transport.rs`.
- Test: `crates/rl-online/tests/through.rs`, `crates/rl-lobby/tests/lobby.rs`

**Interfaces:**
- Consumes:
  - Task 5's `Endpoint::{signalled, gathered, offer, take_reply, answer_signalled, mapped, reach, path}`, `IceConfig`, `TurnServerSpec`;
  - Task 6's `Inbox`, `Asker`, `TurnList`;
  - `Pool`, `PoolConfig`.
- Produces (Tasks 8, 9, 10):
  ```rust
  // rl_online::join
  #[derive(Clone, Debug)]
  pub struct NostrJoin { pub host: PublicKey, pub pool: PoolConfig, pub region: Region, pub turn_from_nostr: bool, pub allow_local: bool }
  #[derive(Clone, Debug)]
  pub struct Join { pub direct: Option<SocketAddr>, pub nostr: Option<NostrJoin> }
  impl Join { pub fn direct(addr: SocketAddr) -> Join; }
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum JoinStep { Direct, Relays, Gathering, Asking, Connecting }
  impl std::fmt::Display for JoinStep {} // "Reaching the game directly…", "Connecting to the relays…", "Looking for paths through your router…", "Asking the game through the relays…", "Connecting…"
  pub const NO_RELAYS: &str = "could not reach any Nostr relay: check your Internet connection, or the relays in Options → Online";
  pub const NO_REPLY: &str = "the game did not answer through the relays (it may have closed, or its relays differ from yours)";
  pub fn no_path(port: u16, relayed: Option<SocketAddr>) -> String;
  // without a relay: "no network path to the game: hole punching failed and no TURN relay could be used. The host can open port {port} (TCP and UDP) on their router (UPnP, or port forwarding) or use a dedicated server; anyone can run a TURN relay (rl-server --relay)"
  // with one: "no network path to the game, not even through the TURN relay {addr}: try again, or ask the host to open port {port} on their router"
  pub fn path_text(path: Path, remote: SocketAddr) -> String;
  // Direct: "connected directly to {remote}"; MappedPort: "connected through the host's open port {remote}";
  // HolePunched: "connected by hole punching through both routers"; Relayed: "connected through the TURN relay {remote}"
  pub fn path_tag(path: Path) -> &'static str; // "direct", "open port", "punched", "relayed"
  /// The NAT behaviour in words (the reach line's and `rl-server --nat-check`'s): "no NAT", "a NAT that keeps ports (hole punching works)", "symmetric NAT", "not known yet".
  pub fn nat_text(nat: NatBehaviour) -> &'static str;
  // rl_online::reach
  #[derive(Clone, Debug)]
  pub struct HostListen {
      pub pool: Option<PoolConfig>,
      /// The configured public address (rl-server's `public_address`, the game's setting): `Reachability.configured`.
      pub public_address: Option<IpAddr>,
  }
  impl HostListen { pub fn off() -> HostListen; } // no pool, no address
  #[derive(Clone, Debug, PartialEq, Eq)]
  pub enum Listening { Off, Connecting, On { relays: usize, of: usize }, NoRelay }
  #[derive(Clone, Debug, PartialEq)]
  pub struct Reachability { pub port: u16, pub configured: Option<IpAddr>, pub reach: Reach, pub listening: Listening }
  impl Reachability {
      /// N47: the usable mapping's external address (UDP and TCP, `tcp: true`), else the configured address on `port`.
      pub fn direct(&self) -> Option<SocketAddr>;
      /// The mapping attempt finished (Mapped, Failed or Off) and the first STUN round settled. Callers wait at most 5 s for it.
      pub fn settled(&self) -> bool;
      pub fn lines(&self) -> Vec<String>;
  }
  // rl_online::peer changes
  pub enum PeerRole {
      Host { lobby: HostConfig, listen: HostListen },
      Server { lobby: HostConfig, listen: HostListen },
      Client { join: Join, password: Option<String>, expected_host: Option<PublicKey> },
  }
  impl OnlinePeer {
      pub fn reachability(&self) -> Option<Reachability>;            // host and server
      pub fn connection(&self) -> Option<(Path, SocketAddr)>;          // client, once connected
  }
  // rl_online::events
  // OnlineEvent gains: Joining(JoinStep), Reachability(Reachability) (on change), Connected { path: Path, remote: SocketAddr }
  ```
  `Reachability::lines()` exact texts (tested):
  - `Mapped { usable: true }`: `"Port {p} is open on your router ({method}): players join directly at {external}"`.
  - `Mapped { usable: false }`: `"Your router opened port {p} ({method}), but its own address {ip} is not public (double NAT or carrier-grade NAT): players connect by hole punching or a TURN relay"`.
  - `Trying`: `"Asking your router to open port {p}…"`.
  - `Failed(why)`: `"Your router did not open port {p} ({why}): players connect by hole punching or a TURN relay"`.
  - `Off` with no configured address: `"Port mapping is off: players connect by hole punching or a TURN relay"`.
  - A configured address: `"Players join directly at {ip}:{p} (your public address setting)"`.
  - `NatBehaviour::Symmetric`: `"Your router changes ports for every destination (symmetric NAT): hole punching will fail; players need the open port or a TURN relay"`.
  - `Listening::On`: `"Taking joins through {n} of {of} relays"`.
  - `NoRelay`: `"No relay reachable: only LAN and direct joins work"`.
  - `Connecting`: `"Connecting to the relays…"`.

Context:
- **The joiner** (`join.rs`, N46):
  - **Direct first.** With `direct`, start `Endpoint::connect`. On `Disconnected { HOST }` before `Connected` (not `Refused`), with `nostr`, drop that endpoint and go on through Nostr. Otherwise the existing message.
  - **Through Nostr** (`JoinStep`s in order):
    1. **Relays:** a `Pool` on `nostr.pool`. The caller passes the relays: the invite's hints first, then its own, deduplicated and cut to 8. With `turn_from_nostr`, it subscribes `TurnList::filter` and collects kind-31471 notes until 1 s after the first `EOSE` (at most 4 s after the start).
    2. **Gathering:** `Endpoint::signalled(cfg)`. Its `ice.turn` is the caller's own TURN servers (the first 2) plus `TurnList::pick(region, 2usize.saturating_sub(own.len()))` (resolved to `TurnServerSpec`s), so at most 2 in all, whatever Options hold.
    3. **Asking:** once `gathered()`, an `Asker` with `offer()`. Subscribe the asker's filter, and publish its first event once an `Eose` for that subscription arrived from any relay (or 2 s passed), so the reply cannot outrun the subscription. Then publish each `Asker::poll`.
    4. **Connecting:** the first `take` reply goes to `Endpoint::take_reply`.

    The port in `no_path` is the port of the host's first candidate in its answer.
  - **Failures:**
    - every relay `Failed` before an answer: `NO_RELAYS`;
    - `Asker::expired`: `NO_REPLY`;
    - the link's setup timing out after an answer: `no_path(port_of_host_answer, relayed)`.
  - **On `Connected`:** the pool is dropped on a thread (R10-10), unused allocations release by themselves (Task 3), and `OnlineEvent::Connected { path, remote }` is reported once.
  - `expected_host` is the Nostr key for Nostr joins: the encrypted answer and N37's `HostProof` both bind it.
- **The host's listener** (`reach.rs`, N54): with `listen.pool`:
  - a `Pool` subscribing `Inbox::filter(now_unix)`, re-subscribed every 30 s with a fresh `since`;
  - every `Inbound::Offer` → `Endpoint::answer_signalled(&req.signal, req.mapped, now)` → `Inbox::reply(.., endpoint.mapped(), ..)` → `publish`;
  - every `Repeat(e)` → `publish(e)`.

  `Reachability` is reported when its lines change.
- **Paths in the lobby.** `LobbyHost::connected(peer, addr, path, ..)` gets the `path` of `NetEvent::Connected { peer, addr, path }` (always a `Path`; `Endpoint::path`, an `Option`, is for later queries).
- **N59.** rl-lobby's `ban_with_address(member, by: Option<u8>)`, where `by` is the acting admin's member id: an admin's command from a client gives `Some(admin)`; the host's own Ban button and a server command give `None`.
  - On a member whose path is `Relayed`, it bans the key only and says `"reached through a TURN relay: banned by key only"`.
  - The notice goes to `by` as a `Notice`, or with `None` as a `HostEvent` that rl-online turns into `OnlineEvent::Notice`, which the game shows and `rl-server` logs.
  - `LobbyHandle::ban_with_address(member)` (the host's own) passes `None`.
- **`rl_net::random_bytes::<32>()`** is the randomness for `Asker` and `Inbox`.

- [ ] **Step 1: Write the failing tests** `crates/rl-online/tests/through.rs`. They run on 127.0.0.1 with `TestRelay` and `TurnServer` (port 0, `ports: 0..=0`, `TurnLimits::generous()`), `base_override: 127.0.0.1`, Classic on Dirt Level from `RL_CONTENT_DIR` (skipped with a notice without it), every wait bounded, each under 20 s:
  - `a_client_joins_through_nostr_without_an_address`: host `HostListen { pool: Some(relay) }`; client `Join { direct: None, nostr: Some(..) }` with `expected_host`. The client sees `Joining(Relays)`, then `Joining(Asking)`, then `Connected { path: Direct, .. }`, and the lobby with both members. The host's member for it has `path: Some(Path::Direct)`.
  - `a_client_joins_through_a_turn_relay_found_on_nostr`: a `TurnServer` on 127.0.0.1 announced with `turn_note` to the `TestRelay` (`allow_local`); client `relay_only`. Path `Relayed`, a 6 s match played to equal confirmed checksums.
  - `a_lost_offer_is_sent_again` (`Misbehaviour::DropEphemeral(2)`: connected within 8 s).
  - `a_relay_that_never_forwards_is_survived` (two relays, one `NoForward`).
  - `a_game_that_never_answers_says_so` (no host listening: `Disconnected(NO_REPLY)` 10 s ± 1 s after the client's `Joining(Asking)`).
  - `no_relay_reachable_says_so` (a closed port: `NO_RELAYS` within 6 s).
  - `direct_failure_falls_back_to_nostr` (`direct` = a closed port, `nostr` valid: connected, and `Joining(Direct)` came first).
  - `a_same_lan_joiner_without_hairpinning_falls_back_and_connects_directly`: `direct` = `192.0.2.1:9`, TEST-NET-1, never routed, as the public address the host's router does not hairpin. Connected via Nostr with path `Direct`, after the direct attempt's `CONNECT_TIMEOUT`.
  - `a_flood_of_offers_leaves_room_for_an_honest_joiner` (PF12b-2). In 1 s, through the relay, the junk is:
    - 150 forged or undecryptable kind-21470 events addressed to the host (bad signatures, tampered content, content encrypted to another key);
    - 50 **valid-but-silent** offers from fresh keys, enough to fill `MAX_SIGNALLED_PENDING` many times over, whose SDPs come from offerer `Link`s that are never pumped.

    Then an honest client joins and **connects** within 10 s: silent setups are evicted for it.
  - `the_host_reports_its_reachability` (`FakeGateway` UPnP through the `IceConfig` hooks, `allow_local: true` since 203.0.113.7 is TEST-NET: `Reachability` lines contain `"Port {p} is open on your router (UPnP)"` and `"Taking joins through 1 of 1 relays"`).
  - A unit test in `reach.rs`: every `lines()` text exactly as listed above.

  In `crates/rl-lobby/tests/lobby.rs`: `a_relayed_member_is_banned_by_key_only` (`connected(.., Path::Relayed, ..)`, then `ban_with_address(member, Some(admin))`: `banned_ip(addr)` false, `banned(key)` true, the admin's notice; with `None`, the `HostEvent` notice).
- [ ] **Step 2: Run `cargo nextest run -p rl-online -p rl-lobby`, expect FAIL.**
- [ ] **Step 3: Implement**, including the pair in `xtask/layers.ron` and the callers' minimal changes (`cargo build --workspace --all-targets` clean).
- [ ] **Step 4: Run `cargo nextest run -p rl-online -p rl-lobby -p rl-server -p rl-cli -p rl-game`, expect PASS; `cargo xtask check`; `cargo xtask full`.**
- [ ] **Step 5: Update the CLAUDE.md of rl-online and rl-lobby. Commit** `rl-online: join through Nostr relays, listen as a host, report paths and reachability`.

---

### Task 8: `rl-server`: port mapping, Nostr handshakes, `--relay`, `--nat-check`

**Files:**
- Create: `crates/rl-server/src/{relay.rs, nat_check.rs}`
- Modify:
  - `crates/rl-server/Cargo.toml` (feature `test-hooks = ["rl-net/test-hooks"]`, enabled by its own dev-dependency on itself, rl-discovery's pattern);
  - `crates/rl-server/src/{config.rs, lib.rs, main.rs}`, `crates/rl-server/tests/server.rs`;
  - `fixtures/server/{example.ron, e2e.ron}`, `xtask/src/dist.rs` (`CHECK_CONFIG` gains `port_mapping: false`);
  - `crates/rl-cli/src/transport.rs` (`server_config` gains `port_mapping: false`) and `crates/rl-game/tests/finding.rs` (its `server_config` gains `port_mapping: false`): in-process servers that would otherwise talk to the real router during `check`;
  - `crates/rl-server/CLAUDE.md`;
  - `crates/rl-discovery/src/announce.rs` (`Hosting::public_port`). `AddressWatch` stays in rl-discovery: this task removes only rl-server's use; Task 9 removes the game's, then the type.
- Test: `crates/rl-server/tests/server.rs`, unit tests in `config.rs`, `relay.rs` and `nat_check.rs`

**Interfaces:**
- Consumes: Task 7's `PeerRole::Server { lobby, listen }`, `OnlinePeer::reachability`, `Reachability`, `HostListen`, `path_tag`; Task 3's `TurnServer`; Task 6's `turn_note`, `TurnNote`; Task 4's `PortMapper`.
- Produces (Tasks 9, 10, 11):
  ```rust
  // rl_server::config::Config gains (all defaulted, so older configs load)
  pub port_mapping: bool,                 // default true
  pub allow_local: bool,                  // default false (N62)
  pub relay: Option<RelayConfig>,         // None: no relay; `--relay` sets Some(default) when absent
  #[derive(Clone, Debug, PartialEq, Deserialize)] #[serde(deny_unknown_fields, default)]
  /// `port: 0` and `ports: (0, 0)` take ephemeral ports (tests).
  pub struct RelayConfig { pub port: u16 /* 3478 */, pub ports: (u16, u16) /* (49160, 49191) */, pub capacity: u16 /* 32 */, pub user_kib_s: u32 /* 32 */, pub allocations_per_ip: u8 /* 4 */ }
  /// Tests only (feature `test-hooks`, `#[serde(skip)]`): aim the port mapping at a FakeGateway and pin the base address.
  #[cfg(feature = "test-hooks")] pub test_gateway: Option<TestGateway>,
  #[cfg(feature = "test-hooks")]
  #[derive(Clone, Debug, PartialEq)] pub struct TestGateway { pub pmp: SocketAddr, pub ssdp: SocketAddr, pub base: Ipv4Addr }
  // rl_server::Server (Server::start, run and run_with keep their signatures; the hooks travel in Config)
  impl Server {
      pub fn relay_addr(&self) -> Option<SocketAddr>; // the TURN server's listening address (tests)
  }
  // rl_server::nat_check
  pub fn run(config: &Config, deadline: Duration, out: &mut dyn Write) -> anyhow::Result<()>; // = run_with(config, None, ..)
  #[cfg(feature = "test-hooks")]
  pub fn run_with(config: &Config, hooks: Option<&TestGateway>, deadline: Duration, out: &mut dyn Write) -> anyhow::Result<()>;
  // rl_discovery::announce::Hosting gains `pub public_port: Option<u16>`: the note's `port` is public_port.unwrap_or(port); `lan()` keeps `port`.
  ```
  The binary gains `--relay` (sets `config.relay` to `Some(RelayConfig::default())` when the config has none) and `--nat-check`.

Context:
- **Listening and announcing** (N47, N54):
  - Public and Invite servers listen on Nostr (`HostListen { pool: Some(PoolConfig { relays: config.relays, .. }), public_address: config.public_ip() }`). Lan servers do not.
  - `EndpointConfig.ice = IceConfig { stun: config.stun, port_mapping: config.port_mapping && visibility != Lan, allow_local, .. }`, plus `test_gateway`'s `gateway`, `ssdp` and `base_override` under `test-hooks`.
  - The server has no TURN list: a host never allocates (N48, N52).
  - **Address:**
    - The note's `addr`/`port` and the invite's address come from `Reachability::direct()`. `AddressWatch` goes.
    - The first note and the printed invite wait for `Reachability::settled()`, at most 5 s (the mapper's UPnP deadline is 4 s), as R9-3 did for the address.
    - The invite is printed again when `direct()` changes.
    - An invite without an address now says `"invite code (joins through the relays): rlx1…"`, else `"invite code: rlx1…"`.
- **Log lines:**
  - every `Reachability::lines()` line once, as it changes, prefixed `"reach: "`;
  - each join's path: `"join: {name} ({fp}) from {addr} via {path_tag}"`;
  - N59's notice.
- **`--relay`** (N52):
  - **Server:** a fresh random user and password each start (16 lowercase hex characters each, from `random_bytes`).
    - **When it starts:** only once its public IP is known (`TurnServerConfig::public` is fixed at start). The IP is, in order: `public_address`; else the usable mapping's external IP once the relay's listening port is mapped; else the srflx IP once STUN settles (logged `"relay: no port mapping: relying on {ip} being reachable"`).
    - **How:** `TurnServer::start` on `0.0.0.0:relay.port` with `ports`, and the limits from `RelayConfig`.
    - Before it starts, the log says `"relay: waiting for the public address"`.
  - **Mapping:** with `port_mapping`, a `PortMapper` per relay port and for the listening port (UDP only), each with `exact: true`. The external port must equal the local one, because the relay reports `public:<local port>`.
    - A port that cannot be mapped exactly is logged (`"relay: port {p} not mapped: {why}"`) and never shifted.
    - The relay is announced only when its listening port is mapped exactly, or `public_address` is set, or the host is not behind a NAT (`NatBehaviour::Open`).
  - **Announcing:** a `turn_note` every 60 s on the server's relays (also for Lan and Invite servers: a relay is public by nature), with `load` = allocations. It waits until the public IP is known. Without one: `"relay: not announced: no public address (set public_address, or enable port mapping)"`.
  - **Shutdown:** a closing TURN note (`turn_note(.., ttl: CLOSE_TTL)`, `capacity: 0`), which `TurnList` takes despite the 10 s gap.
  - **Log:** `"relay: TURN on port 3478, relay ports 49160-49191, announced as {ip}:{port}"`.
- **`--nat-check`** (N60): prints
  - the base address and gateway;
  - STUN from the config's servers (each answer, and the NAT behaviour as `"NAT: {nat_text}"`, so a symmetric NAT prints `"NAT: symmetric NAT"`);
  - the router (a `PortMapper` on a random high port, its result, then dropped, deleting it);
  - the relays (each one's status after 5 s, the TURN notes seen).

  It exits 0. Unit-test its report format with in-process `TestRelay`, `TurnServer` (as STUN) and `FakeGateway` through `nat_check::run_with(config, Some(&hooks), ..)`.
- **Every test config not aimed at a `FakeGateway` (through `test_gateway`) sets `port_mapping: false`.** No test reaches the real router or sends SSDP to 239.255.255.250.

- [ ] **Step 1: Write the failing tests** in `crates/rl-server/tests/server.rs` (in-process `TestRelay` and `TurnServer`, `stun: []` unless named):
  - `an_invite_server_without_an_address_is_joined_through_nostr` (the printed code has no address; a client through `Join { nostr }` reaches the lobby; the log says `via direct`);
  - `a_mapped_server_announces_its_mapped_address` (`test_gateway` aimed at a `FakeGateway`, `port_mapping: true`, `allow_local: true` since 203.0.113.7 is TEST-NET; a `GameList` with `allow_local(true)` sees `addr = 203.0.113.7` and the mapped port; the log has `"reach: Port … is open on your router (UPnP)"`);
  - `a_relay_server_announces_itself_and_relays` (`relay: Some(RelayConfig { port: 0, ports: (0, 0), user_kib_s: 65536, allocations_per_ip: 64, capacity: 256 })`, `public_address: "127.0.0.1"`, `allow_local`, `port_mapping: false`; a `TurnList` subscribed to the test relay picks it; a `relay_only` client joins through it; `stats().relayed_bytes > 0`);
  - `the_relay_closes_its_note_at_shutdown`;
  - `an_old_config_still_loads` extended (no `port_mapping`, `relay` or `allow_local` field);
  - `nat_check_reports_every_step` (unit, in-process fakes).
- [ ] **Step 2: Run `cargo nextest run -p rl-server`, expect FAIL.**
- [ ] **Step 3: Implement**; document every new field in `fixtures/server/example.ron`.
- [ ] **Step 4: Run `cargo nextest run -p rl-server -p rl-discovery -p rl-cli`, expect PASS; `cargo xtask check`; `cargo build --release --target x86_64-pc-windows-gnu -p rl-server`.**
- [ ] **Step 5: Update `crates/rl-server/CLAUDE.md` and `crates/rl-discovery/CLAUDE.md`. Commit** `rl-server: port mapping, Nostr handshakes, --relay and --nat-check`.

---

### Task 9: The game: joins without an address, lobby lines, path tags, Options

**Files:**
- Modify:
  - `crates/rl-ui/src/{lib.rs, online.rs, state.rs, screens/lobby.rs, screens/multiplayer.rs, screens/options.rs}` (`lib.rs` re-exports `CANNOT_JOIN_YET`, which goes; `FindingSettings` lives in `online.rs`);
  - `crates/rl-ui/tests/{online.rs, options.rs}` (kittest);
  - `crates/rl-game/src/{finding.rs, net.rs, shell/online.rs}`;
  - `crates/rl-game/tests/{finding.rs, online.rs}`: their hosted games' settings set `port_mapping: false` (they host Public/Invite games, which would otherwise ask the real router);
  - `crates/rl-discovery/src/hosting.rs` and its tests (`AddressWatch` removed, now unused), `crates/rl-discovery/CLAUDE.md`;
  - `fixtures/autoplay/settings.ron` (`port_mapping: false`);
  - the CLAUDE.md of rl-ui and rl-game.
- Test: `crates/rl-ui/tests/{online.rs, options.rs}`, `crates/rl-game/tests/finding.rs`

**Interfaces:**
- Consumes: Task 7 (`Join`, `NostrJoin`, `HostListen`, `JoinStep`, `OnlineEvent::{Joining, Reachability, Connected}`, `path_text`, `path_tag`); Task 8's `Hosting::public_port`.
- Produces:
  ```rust
  // rl_ui::online
  // InternetEntry.addr stays Option<SocketAddr>; CANNOT_JOIN_YET is removed: every compatible game is joinable.
  // LobbyView gains:
  pub connection: Option<String>,   // a client's path_text, or the JoinStep while joining
  pub reach: Vec<String>,           // a host's Reachability::lines()
  // MemberView gains:
  pub path: Option<String>,         // path_tag of the host's view ("direct", "open port", "punched", "relayed")
  // rl_ui::state
  // UiAction::JoinListed { addr: Option<SocketAddr>, key: [u8; 32], password }
  // AppCommand::JoinGame { addr: Option<String>, password, expected_host: Option<[u8; 32]>, relays: Vec<String> /* invite hints */ }
  // rl_ui::online::FindingSettings gains (serde-defaulted, so older settings load):
  pub port_mapping: bool,                 // default true
  pub turn: Vec<String>,                  // the player's own TURN servers for joining, "user:pass@host:port"; default empty (no default list, N52)
  ```

Context:
- **Joining.**
  - **Internet tab:** a listed game joins with `Join { direct: summary.addr, nostr: Some(NostrJoin { host: key, pool: the game's relays, region: settings region, turn_from_nostr: true, allow_local: PF12-8's rule }) }`.
  - **Direct tab, invite code:** `direct: invite.addr`, `nostr` with the relays being the invite's `relay_urls()` first, then the settings' relays, deduplicated and cut to 8 (N46).
  - **Typed address and LAN:** `Join::direct`.
- **Joining progress.** The Multiplayer screen's waiting text shows the current `JoinStep`.
- **Lobby:**
  - a client's line is `connection` (`path_text`);
  - a host's lines are `reach`, under its existing `listing` line (R10-5's texts stay for listing status);
  - each member row shows its `path` tag beside the ping.
- **Hosting.**
  - Public and Invite games: `HostListen { pool: Some(relays), public_address: the setting }` and `IceConfig { stun, port_mapping: settings.port_mapping, .. }`. Lan games: neither.
  - A joiner's `IceConfig.turn` is the settings' `turn` list (Task 7 uses the first 2).
  - The note's `addr`/`port` come from `Reachability::direct()` (N47).
  - The R10-5 line "Not listed: no public address (forward port N or set it in Options)" goes. A Public game is listed once relays accept its note, address or not.
  - An Invite game's line "This code has no address…" becomes `"This code joins through the relays"` when `direct()` is `None`.
- **Options → Online gains:**
  - "Open the game's port on my router (UPnP/NAT-PMP/PCP)" (checkbox, default on);
  - "TURN relays" (an editable list like Relays; an entry must parse as `user:pass@host:port`, else "needs user:pass@host:port"; at most 8).
- **Removal.** Remove the game's `AddressWatch` use; with rl-server's gone (Task 8), remove `hosting::AddressWatch` and its tests from rl-discovery. Keep `stun::public_ip` while `rl-cli stun-probe` uses it.

- [ ] **Step 1: Write the failing tests.**
  - **kittest:**
    - `an_addressless_internet_game_is_joinable` (Join is enabled, and its action carries `addr: None`);
    - `the_lobby_shows_reach_lines_and_path_tags` (a `LobbyView` with `reach` lines and members with tags: the labels are visible);
    - `options_edit_turn_relays_and_port_mapping` (a bad entry shows "needs user:pass@host:port"; the checkbox saves).
  - **rl-game end-to-end** (`tests/finding.rs`): `a_listed_server_without_an_address_is_joined_through_nostr` (rl-server Public with `port_mapping: false` and `stun: []` on a `TestRelay`; the game's pool on it; select, Join, lobby; `connection` reads "connected directly to 127.0.0.1:…").
- [ ] **Step 2: Run `cargo nextest run -p rl-ui -p rl-game`, expect FAIL.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run, expect PASS; `cargo xtask check`. The HUD snapshots are untouched; bless nothing unless a HUD image changed, then `cargo xtask bless-render --reason "<why>"`. `cargo xtask e2e`** (the autoplay online run still passes).
- [ ] **Step 5: Commit** `rl-game: join games without an address, show paths and reachability`.

---

### Task 10: `rl-cli`: joins by invite, key or listing, and the NAT tools

**Files:**
- Create: `crates/rl-cli/src/nat_tools.rs`
- Modify:
  - `crates/rl-cli/{Cargo.toml, CLAUDE.md}`:
    - feature `nat-tools = ["rl-discovery/test-relay"]`;
    - a dev-dependency on itself, `rl-cli = { path = ".", features = ["nat-tools"] }` (rl-discovery's pattern), so `check`'s plain `nextest --workspace` builds the feature and runs its test;
  - `crates/rl-cli/src/{main.rs, lib.rs, net_client_cmd.rs, transport.rs}`;
  - `crates/rl-cli/tests/{cli.rs, transport.rs}` (`ClientSpec` callers);
  - `crates/rl-discovery/src/stun.rs` (`StunResponder::reflecting`) and `crates/rl-discovery/CLAUDE.md`.
- Test: `crates/rl-cli/tests/cli.rs`, unit tests in `net_client_cmd.rs`

**Interfaces:**
- Consumes: Task 7's `Join`, `NostrJoin`, `OnlineEvent::Connected`; Task 6's `TestRelay::start_on`; plan 12a's `Invite`, `GameList` and `StunResponder`.
- Produces (Task 11):
  ```text
  rl-cli net-client --bot [--join host[:port] | --invite rlx1… | --key <64 hex> | --listed "<name>"]
                    [--relay <url>]... [--stun host:port]... [--turn user:pass@host:port]... [--no-nostr-turn]
                    [--region <name>] [--allow-local] [--lobby-only] … (existing flags)
    prints "path: <path_tag>" once connected; --lobby-only exits 0 once in the lobby
  rl-cli nostr-relay --listen <ip:port>      (feature nat-tools: TestRelay::start_on until killed)
  rl-cli stun-server --listen <ip:port>      (feature nat-tools: StunResponder answering with the observed source)
  ```
  `transport::ClientSpec.addr: SocketAddr` becomes `join: Join`.

Context:
- **`--listed "<name>"`** subscribes a `GameList` on `--relay`s (`allow_local` with `--allow-local`). It waits up to 10 s for a compatible game with that name and joins it as the game's Internet tab does.
- **`StunResponder` reflecting mode.** Today it answers with a configured address. Add `StunResponder::reflecting(bind: SocketAddr)` in rl-discovery behind `test-relay`, answering with the request's observed source.
- **The feature never ships.** `xtask dist` does not build `rl-cli`.

- [ ] **Step 1: Write the failing tests:**
  - unit tests of argument resolution: `--invite` without an address gives `Join { direct: None, nostr: Some }`; `--invite` with one gives both; `--key` needs a `--relay`; `--listed` needs a `--relay`; the flags conflict;
  - `tests/cli.rs`: `net_client_joins_by_invite_through_a_test_relay` (rl-server in-process Invite with `port_mapping: false`, `--lobby-only`, stdout has `path: direct`);
  - `nostr_relay_and_stun_server_run_until_killed` (the test build has `nat-tools` through the self dev-dependency: a child process answers one REQ and one binding, then is killed).
- [ ] **Step 2: Run `cargo nextest run -p rl-cli cli`, expect FAIL.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run `cargo nextest run -p rl-cli`, expect PASS (the `nat-tools` test included); `cargo build -p rl-cli` (without the feature) builds; `cargo xtask check`.**
- [ ] **Step 5: Commit** `rl-cli: join by invite, key or listing; a relay and STUN server for NAT tests`.

---

### Task 11: The Docker NAT tests (`cargo xtask net-nat`, in `full`)

**Files:**
- Create:
  - `xtask/src/nat.rs`;
  - `tools/net-nat/Dockerfile`, `tools/net-nat/router.sh`, `tools/net-nat/node.sh`;
  - `tools/net-nat/miniupnpd.conf.in`, `tools/net-nat/CLAUDE.md`.
- Modify: `xtask/src/{main.rs, check.rs}`, `xtask/CLAUDE.md`
- Test: unit tests in `xtask/src/nat.rs`; the step itself

**Interfaces:**
- Consumes:
  - `rl-server` (`--relay`, `port_mapping`, `allow_local`, its log lines);
  - `rl-cli net-client` (Task 10's flags, `path:` line);
  - `rl-cli nostr-relay`, `rl-cli stun-server`;
  - `olx_timing::docker_missing`.
- Produces: `cargo xtask net-nat [--keep] [--only <scenario>]...`; `full`'s step `net-nat`.

Context (spec §9.7, N58):
- **Prerequisites** (each missing one prints `skip net-nat (<why and the fix>)` and passes):
  - `olx_timing::docker_missing()`;
  - `/etc/os-release` with `ID` and `VERSION_ID`;
  - the content directory.
- **Build.** `cargo build --release -p rl-server -p rl-cli --features rl-cli/nat-tools` on the host. The image `rl-nat:<first 12 hex of SHA-256(Dockerfile ++ scripts ++ base)>` is built from `tools/net-nat/Dockerfile`:

  ```dockerfile
  ARG BASE
  FROM ${BASE}
  ENV DEBIAN_FRONTEND=noninteractive
  RUN apt-get update && apt-get install -y --no-install-recommends iproute2 nftables miniupnpd-nftables \
      && rm -rf /var/lib/apt/lists/*
  COPY router.sh node.sh miniupnpd.conf.in /rl-nat/
  RUN chmod 755 /rl-nat/*.sh
  ```

  `--build-arg BASE=<ID>:<VERSION_ID>`. The binaries and `content/packs` are mounted read-only at `/rl/bin` and `/rl/content`. Old `rl-nat:*` images are pruned like the oracle's.
- **Networks** (all `docker network create --internal`, label `rl-nat`, removed at the end unless `--keep`):
  - `rl-nat-wan` 203.0.113.0/24;
  - `rl-nat-a` 10.1.0.0/24 (router A: port-restricted cone, `masquerade`);
  - `rl-nat-b` 10.2.0.0/24 (router B: symmetric, `masquerade fully-random`);
  - `rl-nat-c` 10.3.0.0/24 (router C: `masquerade` plus miniupnpd with UPnP, NAT-PMP and PCP, `ext_allow_private_ipv4=yes`, `secure_mode=yes`, `allow 1024-65535 10.3.0.0/24 1024-65535`);
  - `rl-nat-d` 10.4.0.0/24 (router D: as C with `enable_upnp=no`, NAT-PMP and PCP only).
- **Routers.** Each router has `--cap-add NET_ADMIN --sysctl net.ipv4.ip_forward=1`, WAN .101 to .104, LAN .2. `router.sh <kind> <wan ip> <lan ip>` finds the interfaces by address, then sets nftables NAT and, for C and D, starts miniupnpd.
- **Nodes.** Nodes run `node.sh <router lan ip> <command…>` (`--cap-add NET_ADMIN`): `ip route replace default via <router>`, then exec.
- **Infrastructure on the WAN:**
  - `rl-cli nostr-relay --listen 0.0.0.0:7777` at 203.0.113.10;
  - `rl-cli stun-server --listen 0.0.0.0:3479`, same container;
  - a relay server at 203.0.113.20: `rl-server --relay`, Invite, `allow_local`, `port_mapping: false`, `public_address: "203.0.113.20"`, `stun: ["203.0.113.10:3479"]`, relays `["ws://203.0.113.10:7777"]`;
  - a public game server at 203.0.113.30.
- **Configs.** Every config of ours uses `relays: ["ws://203.0.113.10:7777"]`, `stun: ["203.0.113.10:3479", "203.0.113.20:3478"]` and `allow_local: true` (rl-cli: `--allow-local`). They are written by `nat.rs` into `target/net-nat/`.
- **Scenarios** (`SCENARIOS`, each bounded at 60 s, each its own fresh containers, named in failures):

  | name | host | joiner | expect |
  |---|---|---|---|
  | `direct` | public server 203.0.113.30 | net-client in LAN A, `--join 203.0.113.30:23400 --lobby-only` | exit 0, `path: direct` |
  | `upnp` | rl-server Invite in LAN C, `port_mapping: true` | LAN A, `--invite <code from the host's log>` | log `reach: Port 23400 is open on your router (UPnP)`, the code has an address, `path: direct` |
  | `pcp` | rl-server Invite in LAN D | LAN A, `--invite` | log `(PCP)` or `(NAT-PMP)`, `path: direct` |
  | `hole-punching` | rl-server Invite in LAN A (no mapping) | LAN C, `--invite` (no address), `--no-nostr-turn` | `path: punched` |
  | `relay` | rl-server Invite in LAN A | LAN B (symmetric), `--invite`, one 0.2-minute match (`--matches 1`) | `path: relayed`, both exit 0, the relay's log shows an allocation |
  | `no-relay` | rl-server Invite in LAN A | LAN B, `--invite --no-nostr-turn` | exit 1, stderr contains `no network path to the game` |
  | `finding` | rl-server **Public** in LAN A | LAN B, `--listed "<name>" --lobby-only` | `path: relayed` |
  | `nat-check` | `rl-server --nat-check` in LAN B | — | stdout contains `NAT: symmetric NAT` |

  A browser host is plan 13 (PL12b-3).
- **Bookkeeping.** Each scenario's container logs go to `target/net-nat/<scenario>/`, and a failure prints the last 40 lines of each. Containers are removed at the end of each scenario (`docker rm -f` by label). A `full` run prints `ok   net-nat (8 scenarios, <s>s)`.
- **Unit tests in `nat.rs`:**
  - `base_image_from_os_release` (Ubuntu, Debian, a missing `VERSION_ID` → skip reason);
  - `invite_code_is_read_from_the_log` (`"invite code: rlx1…"` and `"invite code (joins through the relays): rlx1…"`);
  - `scenario_names_are_unique_and_selectable` (`--only`);
  - `image_tag_changes_with_the_scripts`.

- [ ] **Step 1: Write the failing unit tests**; run `cargo nextest run -p xtask nat`, expect FAIL.
- [ ] **Step 2: Implement `nat.rs`, the Dockerfile and the scripts.** Run `cargo xtask net-nat` here (Docker is installed): every scenario passes. Fix what fails; a failure is a real finding about Tasks 1-10 and is fixed there, with a natsim regression test when it is a traversal bug.
- [ ] **Step 3: Add `net-nat` to `full` after `e2e`** (`check.rs`); `cargo xtask full`.
- [ ] **Step 4: Update `xtask/CLAUDE.md` and `tools/net-nat/CLAUDE.md`** (topology, scenarios, timings measured here). `cargo xtask check`.
- [ ] **Step 5: Commit** `xtask: Docker NAT tests: direct, port mapping, hole punching, relays, finding`.

---

### Task 12: Records

**Files:**
- `docs/porting/STATUS.md`: a new section "Through routers decisions (plan 12b)" holding:
  - N44-N62, PF12b-2 and PL12b-1 to PL12b-3, as decided here and as amended by rulings during execution (N45 and N52 say that over-limit input is dropped, not the sender, a deviation from spec §5.4);
  - Task 6's probe table (kind 31471 kept 3 minutes, 16 KiB ephemeral forwarded, per default relay);
  - the NIP-44 vectors' hash;
  - the kinds registry check;
  - N40, R9-3 and R10-5, amended where N47 replaced them (by a pointer, not by rewriting history).
- `CHANGELOG.md` (Unreleased, "Added — through routers": port mapping, joins without an address, TURN relays and `--relay`, `--nat-check`, the lobby's path and reachability lines, Options' port mapping and TURN relays).
- `docs/online.md`, a section "Through routers":
  - what the lobby lines mean;
  - each path;
  - what to do when "no network path" appears;
  - running `rl-server --relay` (ports to open: 3478/udp and the relay range);
  - `rl-server --nat-check`;
  - the Windows Firewall prompt (allow rl-game on private and public networks; hole punching works without it, an open port needs it);
  - **"Two PCs across home routers"**: the playtest recipe:
    1. host "Invite only";
    2. copy the code;
    3. the other PC pastes it in Direct;
    4. read both lobby lines;
    5. if it fails, run `rl-server --nat-check` on both and keep the output.
- `crates/*/CLAUDE.md` reconciled with the code: rl-proto, rl-net, rl-lobby, rl-discovery, rl-online, rl-server, rl-game, rl-ui, rl-cli, xtask.
- The spec's status line (plan 12b implemented), and a pointer in spec §6.1 after "handshake hints": announcements carry no handshake hints; any relay that carries a note reaches its host (STATUS PL12b-2).

- [ ] Write; `cargo xtask check`; commit `docs: plan 12b's records`.
