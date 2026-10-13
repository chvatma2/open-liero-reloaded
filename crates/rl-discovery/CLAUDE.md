# rl-discovery

Finding games (networking spec §6, plan 12a): the Nostr core, and on it the
test relay, relay pool, announcements, game list, STUN and invite codes
(later tasks).

- **Layer 5, std.** Depends on rl-proto (`PublicKey`, `Signature`), rl-lobby
  (`Identity`, `verify`), serde_json, sha2, hex, thiserror. The core is
  sans-IO (no sockets, no threads, no clock: time is an argument) because
  plan 13 builds parts of this crate for WASM. Sockets and threads belong
  only to drivers added later.
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
- Deps: `rustls =0.23.45` (default aws-lc-rs), `rcgen =0.14.10`
  (`aws_lc_rs` only, no pem) and `tungstenite`, all optional behind
  `test-relay`. The crate's own dev-dependency on itself turns the feature
  on for its tests.

Test: `cargo nextest run -p rl-discovery` (`tests/nostr.rs`: the published
events verify, signing round-trips, canonical escaping, tampering, frames
round-trip, fuzzed frames never panic; `tests/test_relay.rs`: the relay).
