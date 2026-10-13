# rl-discovery

Finding games (networking spec §6, plan 12a): the Nostr core, and on it the
test relay, relay pool, announcements, game list, STUN and invite codes
(later tasks).

- **Layer 5, std.** Depends on rl-proto (`PublicKey`, `Signature`), rl-lobby
  (`Identity`, `verify`), serde_json, sha2, hex, thiserror, and for the
  relay drivers rustls, webpki-roots and tungstenite. The core is
  sans-IO (no sockets, no threads, no clock: time is an argument) because
  plan 13 builds parts of this crate for WASM. Sockets and threads belong
  only to drivers (`relay`).
- **`nostr`** (decision N38: no Nostr library). The NIP-01 subset we use:
  - `Event` with `sign(identity, created_at, kind, tags, content, aux)`
    (signs the 32-byte id with the install's BIP-340 key: Nostr's
    `pubkey`/`sig` are exactly x-only BIP-340), `verify()` (recomputes the
    id from the fields, then checks the signature), `tag(name)` and
    `expiration()` (NIP-40).
  - `canonical(..)` is written by hand, not with serde_json's escaper:
    NIP-01 escapes `\n \" \\ \r \t \b \f` and writes every other character
    verbatim (raw control characters and DEL included), which differs from
    JSON's own escaper. The wire encoding (`encode_*`) uses serde_json and
    may escape differently; ids always come from `canonical`.
  - Frames: `ToRelay` (`EVENT`, `REQ`, `CLOSE`) and `FromRelay` (`EVENT`,
    `EOSE`, `OK`, `NOTICE`, `CLOSED`). `decode_*` refuse text above
    `MAX_FRAME` (128 KiB) and rely on serde_json's recursion limit (128)
    for nesting. Decoding does not verify signatures: callers call
    `Event::verify`. Hex fields must be exact length. `OK` and `CLOSED`
    without a message are accepted (seen in the wild); unknown filter keys
    are ignored; `#x` filters need a single character `x`. Authors in
    filters are full 32-byte keys (no prefixes). `encode_from_relay` and
    `decode_to_relay` exist for the test relay.
  - Kinds: `KIND_ANNOUNCE` 31470 (addressable, tag `d` = `rust-liero`),
    `KIND_HANDSHAKE` 21470 (ephemeral, plan 12b). `MAX_CONTENT` (4 KiB) is
    the limit for our own events' content; callers enforce it.
- **Kinds registry check, 2026-10-13.** `schema.yaml` of
  github.com/nostr-protocol/registry-of-kinds (kinds section) and the NIPs
  README kinds table list nothing in 31400-31499 or 21400-21499 (31470 and
  21470 are free). Re-check before a release.
- **Fixture** `tests/fixtures/relay-events.json`: three real kind-1 events a
  public relay served on 2026-10-13, fetched once by hand (a WebSocket REQ
  from node) and embedded; they verify with our code. No test touches the
  network.

- **`test_relay`** (feature `test-relay`; rl-game, rl-server and rl-cli
  tests enable it in dev-dependencies, never normal builds). `TestRelay`:
  `start()` (`ws://127.0.0.1:<port>`), `start_tls()` (`wss://localhost:<port>`
  plus the DER root; rcgen self-signed, rustls with the aws-lc-rs provider;
  the relay listens on 127.0.0.1 only, so a client connecting to `localhost`
  must fall back from `::1`, or connect to `port()` directly), `url()`,
  `port()`, `events()`, `set_misbehaviour(Honest | NeverEose | Garbage |
  DropAfter(n) | Slow(d))`, `set_time(unix)` (the clock for NIP-40; default
  the system clock). Verifies events; regular events dedupe by id,
  replaceable kinds by (key, kind), addressable by (key, kind, `d`), newest
  wins; ephemeral 20000-29999 only forwarded; expired events are refused on
  publish and not served. REQ: kinds, authors, `#x`, since, limit (per
  filter, newest first), then EOSE; no filters select nothing. Frames are
  limited to `MAX_FRAME`. One thread per connection; drop stops it (joins
  bounded at 2 s). Tests that keep a subscription must say so: the test
  client's `query` CLOSEs after EOSE, `subscribe` does not.
- **`pool`** and **`relay`** (its driver): `Pool::start(PoolConfig {
  relays, extra_roots, max_frame })` runs one driver thread per relay;
  `publish`, `subscribe`, `unsubscribe`, `poll` and `statuses` never block
  (they only touch per-relay mutexes the drivers never hold across IO).
  - **Connect:** the URL's host is resolved, IPv4 addresses first, each
    tried with a 5 s timeout; the TLS and WebSocket handshakes then get 5 s
    in all (`tungstenite::client_tls_with_config` with our rustls
    `ClientConfig`: aws-lc-rs, Mozilla's roots from `webpki-roots` plus
    `extra_roots`, never the platform's). `ws://` is allowed. A bad URL is
    `Failed` for good; anything else retries after 1, 2, 4 … 60 s (a
    connection up 60 s resets the backoff).
  - **Serve:** blocking socket, 50 ms read timeout (`TICK`), 5 s write
    timeout. `Sock` maps `TimedOut` to `WouldBlock` (Windows), on which
    rustls keeps partial records and tungstenite partial frames, and the
    handshake resumes. Subscriptions are reconciled from the pool's set
    (`REQ`/`CLOSE`) and sent again after every reconnect.
  - **Dropped** (then backoff): a frame above `max_frame` (clamped to
    `MAX_FRAME`), a frame that is not a Nostr message (not JSON, or not an
    array headed by a type; unknown types such as `AUTH` and malformed
    single events are skipped), more than 200 frames/s or 1 MiB/s
    sustained (token buckets holding 5 s of burst: a `REQ`'s stored events
    arrive at once), no `EOSE` or `OK` within 5 s of the `REQ`/`EVENT`
    (`CLOSED` cancels the wait), a close or socket error.
  - **Events** are verified in the driver (a forged id never shadows the
    real event in the dedupe) and only for subscriptions the connection
    has; `EOSE` and `OK` only answer what was asked. `poll` appends relay
    by relay in index order, each relay's reports in arrival order, and
    dedupes events by id (an LRU of 4096 ids).
  - **Queues:** outgoing at most 256 per relay (oldest dropped; a queued
    replaceable/addressable event is replaced by a newer one for its
    slot); incoming at most 4096 events or 8 MiB per relay between polls
    (the newest dropped, `dropped_events()`).
  - **Drop** stops every driver: each sends what is queued (a closing
    note), closes, and is joined within 2 s in all (one stuck in a connect
    is detached).
- Deps: `rustls` (default aws-lc-rs) and `webpki-roots` (workspace pins),
  `tungstenite` with `rustls-tls-webpki-roots` (for `Connector::Rustls`;
  its own default roots are never used). `rcgen =0.14.10` (`aws_lc_rs`
  only, no pem) is optional behind `test-relay`. The crate's own
  dev-dependency on itself turns the feature on for its tests.

Test: `cargo nextest run -p rl-discovery` (`tests/nostr.rs`: the published
events verify, signing round-trips, canonical escaping, tampering, frames
round-trip, fuzzed frames never panic; `tests/test_relay.rs`: the relay;
`tests/pool.rs`: the pool against the test relay, a flooding relay and
unreachable addresses: `127.0.0.1`'s closed port and TEST-NET-1
`192.0.2.1`, never routed; unit tests of the backoff, limiter, queues and
dedupe in `src`).
