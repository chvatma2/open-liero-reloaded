# rl-discovery

Finding games (networking spec §6, plan 12a), complete: the Nostr core, the
relay pool and its test relay, announcements, the game list, STUN, invite
codes, the default lists, and the pieces rl-server and rl-game share to
host a listed game (`hosting`, `ping`). Plan 12b adds NIP-44 encryption,
the handshake events (`handshake`) and TURN relay notes (`turn_note`).

- **Layer 5, std.** Depends on rl-proto (`PublicKey`, `Signature`), rl-lobby
  (`Identity`, `verify`), serde, serde_json, sha2, hex, thiserror, bech32 and
  postcard (invite codes), k256 (`ecdh`), hkdf, hmac, chacha20 and base64
  (NIP-44), and for the relay drivers rustls, webpki-roots and
  tungstenite. The core (`nostr`, `announce`, `list`, `invite`, `nip44`,
  `handshake`, `turn_note`, the `Announcer`, STUN's codec) is sans-IO (no sockets, no threads, no clock:
  time is an argument) because plan 13 builds parts of this crate for WASM.
  Sockets and threads belong only to drivers (`relay`, `stun::public_ip`,
  `hosting::AddressWatch`, `ping`).
- **Public API** (`lib.rs`): modules `announce`, `defaults`, `handshake`,
  `hosting`, `list`, `nip44`, `nostr`, `ping`, `pool`, `stun`, `turn_note`
  (and `test_relay`, below);
  `invite` and `relay` are private; `invite` is re-exported as `Invite`, `InviteError`,
  `RelayHint`, `MAX_CODE`, `MAX_DEFAULT_HINTS`, `MAX_URL_BYTES`,
  `MAX_URL_HINTS`; `relay` is the pool's driver (its types are `pool`'s).
  Also re-exported: `Event`,
  `Filter`, `ToRelay`, `FromRelay`, `KIND_ANNOUNCE`, `KIND_HANDSHAKE`,
  `Pool`, `PoolConfig`, `PoolEvent`, `RelayStatus`, `Timings`, `GameList`,
  `ListedGame`, `Ping`, `Pinger`, `GameSummary`, `Hosting`, `Region`,
  `Announcer`, `announcement`, `parse_announcement`, `AddressWatch`,
  `Publisher`, `relay_hints`, `invite`, `binding_request`,
  `parse_binding_response`, `public_ip`, `Asker`, `Inbound`, `Inbox`,
  `Reply`, `Request`, `KIND_TURN`, `TurnList`, `TurnNote`, `TurnPick`,
  `parse_turn_note` (the function `turn_note` only by its module path). Read `target/api/rl-discovery.txt`
  (`cargo xtask api`) for signatures.
- **Feature `test-relay`** adds `test_relay` (`TestRelay`, `Misbehaviour`)
  and `StunResponder`; it pulls in `rcgen` and is off in normal builds.
  rl-game, rl-server and rl-cli enable it in dev-dependencies only, and
  this crate's own dev-dependency on itself turns it on for its tests.
- **`nostr`** (decision N38: no Nostr library). The NIP-01 subset we use:
  - `Event` with `sign(identity, created_at, kind, tags, content, aux)`
    (signs the 32-byte id with the install's BIP-340 key: Nostr's
    `pubkey`/`sig` are exactly x-only BIP-340), `verify()` (recomputes the
    id from the fields, then checks the signature), `tag(name)` and
    `expiration()` (NIP-40). `Filter::matches(event)` (kinds, authors,
    since, `#x` tags; the driver's and the test relay's).
  - `canonical(..)` is written by hand, not with serde_json's escaper:
    NIP-01 escapes `\n \" \\ \r \t \b \f` and writes every other character
    verbatim (raw control characters and DEL included), which differs from
    JSON's own escaper. The wire encoding (`encode_*`) uses serde_json and
    may escape differently; ids always come from `canonical`.
  - Frames: `ToRelay` (`EVENT`, `REQ`, `CLOSE`) and `FromRelay` (`EVENT`,
    `EOSE`, `OK`, `NOTICE`, `CLOSED`). `decode_*` refuse text above
    `MAX_FRAME` (128 KiB) and rely on serde_json's recursion limit (128)
    for nesting. Decoding does not verify signatures: callers call
    `Event::verify`. `NostrError::is_frame_error()` tells text that is no
    relay frame at all (`Json`, `TooLarge`, `NotAFrame`) from a frame we
    skip (`Unknown` type, `Malformed` field). Hex fields must be exact
    length. `OK` and `CLOSED`
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
- **Kinds registry re-check, 2026-10-14** (plan 12b Task 6, for 31471 and
  21470): registry-of-kinds `schema.yaml` at d18576cc (2026-10-02) and the
  NIPs README kinds table at 6631b3eb (2026-09-27). Neither lists 31470,
  31471 or 21470; in 21000-21999 only 21000-21003 and 21059 are taken. The
  31400-31499 range is not empty after all: 31436 ("Gopherhole Document")
  is listed (already in the 2026-10-02 schema, so the 2026-10-13 note above
  missed it); it does not touch our kinds.
- **`nip44`** (N44): NIP-44 v2 exactly as the NIP's pseudocode.
  `conversation_key(secret, public)` (k256 `ecdh::diffie_hellman` with the
  scalar as given, `Identity::secret()`, and the x-only key lifted to
  `0x02 || x`; HKDF-SHA256 extract, salt `nip44-v2`; `None` for a secret of
  zero or ≥ n, or an x on no curve point), `message_keys` (HKDF expand of
  the nonce to 76 bytes: ChaCha20 key, nonce, HMAC key), `padded_len`,
  `encrypt(ck, plaintext, nonce)` (1..=65535 bytes, else `Length`) and
  `decrypt(ck, payload)` (`#` or empty: `Version`; base64 length
  132..=87472 and decoded 99..=65603: `Payload`; version byte 2; the MAC
  through `Mac::verify_slice`, constant time: `Mac`; then the padding:
  `Padding`; non-UTF-8 plaintext: `Payload`). Only the 2-byte length
  prefix: the NIP's later 6-byte extended prefix (64 KiB and more) is left
  out, as the NIP allows; handshakes are at most 16 KiB. Never panics
  (fuzzed, including arbitrary padded plaintexts behind a correct MAC).
- **Fixture** `tests/fixtures/nip44.vectors.json`: the official NIP-44
  vectors, unmodified, fetched once by hand on 2026-10-14 from
  `https://raw.githubusercontent.com/paulmillr/nip44/main/nip44.vectors.json`
  (repository HEAD 671a1f04, the file last changed in 1f8dba17). SHA-256
  `269ed0f69e4c192512cc779e78c555090cebc7c785b609e338a62afc3ce25040`, the
  checksum NIP-44's "Tests" section states (nostr-protocol/nips 44.md at
  733a0471). All of `v2.valid` and `v2.invalid` are tested, the long
  messages included.
- **`handshake`** (N45): kind-21470 events, tags `p` (the recipient) and
  `expiration` (+60 s), content the NIP-44 encryption of
  `{"v":1,"session":"<32 hex>","signal":<rl_proto::Signal>,"mapped":"<ip:port>"|null}`.
  `MAX_HANDSHAKE_CONTENT` 16 KiB, `RESEND` 2 s, `ANSWER_DEADLINE` 10 s,
  `MAX_SKEW` 60 s, `SESSION_MEMORY` 120 s, `MAX_SESSIONS` 4096.
  - `Asker` (the joiner): `new` draws a fresh key from `rand` (an invalid
    secret is drawn again, up to 16 draws; a source that gives none is
    broken: the asker then has no key (`key()` all zeros), sends nothing and
    expires, rather than derive a key every join would share) and a 16-byte
    session; `poll` gives the offer at once and every `RESEND` (a new
    event, the same session) until answered or expired, never an offer
    whose content would exceed `MAX_HANDSHAKE_CONTENT` (an SDP of
    `MAX_SDP` bytes in CRLF lines fits; one JSON escapes mostly does not).
    The joiner sends `mapped: null` (only the host's mapped address helps); `take` checks the author is the
    host, kind, size, our `p` tag, the signature, decrypts, the session,
    and an `Answer` (SDP within `MAX_SDP`) or `Refused`; taken once.
    `filter`: kind 21470, author the host, `#p` our key, since now - 60.
  - `Inbox` (the host): `offer` checks size, kind, `p` = us and the date
    (±60 s) first. A key with a live session: an event id seen before is a
    `duplicate` (no signature check); otherwise signature, decryption with
    the remembered conversation key, the same session, then `Repeat(the
    cached reply)` or `being answered` before the reply (at most 5 signed
    resends per session, so a forgery naming the key uses none). A new key: signature, ECDH,
    decryption, JSON, `v == 1`, an `Offer` with SDP within `MAX_SDP`, and
    `mapped` an IPv4 socket address, then the rate bucket (10 a second,
    burst 20; integer microseconds, so a steady 10/s always passes): only
    offers that decrypt spend tokens, so undecryptable junk cannot starve
    honest joiners (Task 7 review, Important 1; a flood of valid offers
    from fresh keys still shares the bucket). One session per key; sessions are
    forgotten `SESSION_MEMORY` after the offer or the reply, the oldest
    dropped at 4096. `reply` encrypts and signs with the host's key and
    caches the event for repeats; a signal that would not fit
    `MAX_HANDSHAKE_CONTENT` is sent as `Refused(Malformed)`. `ignored()`
    counts hostile or over-limit events, not `duplicate` (each relay's
    copy) nor `being answered` (a resend before the reply).
    `filter(now)`: kind 21470, `#p` us, since now - 60.
  - **Cost:** `Inbox::offer` re-verifies signatures the pool's drivers
    already checked (a forgery never arrives through a `Pool`) and decrypts
    every event addressed to us before the bucket, so its cost is bounded
    by the pool's per-relay limits (200 frames/s): call it from the host's
    network thread (rl-online's listener thread), never the UI thread.
- **`turn_note`** (N52, N53): `KIND_TURN` 31471 (addressable), `TURN_TAG`
  `rust-liero-turn` (tags `d` and `t`), `TurnNote { v (TURN_NOTE_V = 1),
  addr (IPv4), port, region, capacity, load, user, pass }` as JSON.
  `turn_note(identity, note, now, ttl, aux)` (expiration now + ttl:
  `ANNOUNCE_TTL` for a live relay, `CLOSE_TTL` with capacity 0 when it
  closes); `parse_turn_note` (kind, `d`, `t`, `MAX_CONTENT`, JSON; `v` must
  be `TURN_NOTE_V` (another format is never read as ours); port nonzero,
  user and pass 1..=64 printable ASCII (space included), capacity at most
  4096, the address neither unspecified, broadcast nor multicast;
  `AnnounceError`s). `TurnList` checks like `GameList`: cheap checks
  first (kind, size, age 180 s, future 60 s, expiration, the per-key 10 s
  gap by arrival or date, staleness, a full list of `MAX_RELAYS` 64), then
  the signature, the note and the address (global unless `allow_local`).
  A closing note (capacity 0, recognised before the signature by
  `"capacity":0,`, our own serde encoding: only rust-liero writes notes;
  the parsed capacity decides) is exempt from the gap, removes the relay
  and leaves a tombstone. At 4096 remembered keys, stale ones go, then the
  oldest never listed (a flood of closing notes from fresh keys), then the
  oldest unlisted: a closed relay's tombstone outlives such a flood. `pick(region, n)`: the region first, then load/capacity
  (compared exactly), then key; full relays (load >= capacity) never.
  `filter(now)`: kind 31471, `#t` `TURN_TAG`, since now - 180.
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
  `start_on(addr)` (`ws://` on any address, port 0 for a free one; `url()`
  names the bound address, loopback for an unspecified one: `rl-cli
  nostr-relay` in Docker),
  `port()`, `events()`, `set_misbehaviour(Honest | NeverEose | Garbage |
  DropAfter(n) | Slow(d) | Silent | Drip | SlowReads | Refuse(msg) |
  RefuseKind(k) | KeepFor(secs) | NoForward | HalfFrame |
  DropEphemeral(n))` (DropEphemeral: the first n ephemeral events after it
  is set are accepted and forwarded to nobody, then they are forwarded; HalfFrame: at
  the first frame read, half a 256-byte text frame, then the connection
  closes; Refuse: every event gets
  `OK false msg`, such as a paid relay's `restricted: …`; RefuseKind: that
  kind gets `blocked: …`; KeepFor: a stored event is served only while the
  relay's clock is under `created_at + secs`; NoForward: ephemeral events
  accepted, forwarded to nobody; SlowReads: reads
  of at most 1 KiB, a 1 ms pause every 8, under any TLS; Silent: no reads,
  no writes, new connections held without a handshake; Drip: one byte per 40 ms,
  forever: an endless handshake response on new connections (on the TCP
  stream, under any TLS), an endless 64 KiB frame on established ones),
  `set_time(unix)` (the clock for NIP-40; default the system clock).
  Verifies events; regular events dedupe by id,
  replaceable kinds by (key, kind), addressable by (key, kind, `d`), newest
  wins; ephemeral 20000-29999 only forwarded; expired events are refused on
  publish and not served. REQ: kinds, authors, `#x`, since, limit (per
  filter, newest first), then EOSE; no filters select nothing. Frames are
  limited to `MAX_FRAME`. One thread per connection; drop stops it (joins
  bounded at 2 s). Tests that keep a subscription must say so: the test
  client's `query` CLOSEs after EOSE, `subscribe` does not.
- **`pool`** and **`relay`** (its driver): `Pool::start(PoolConfig {
  relays, extra_roots, max_frame })` runs one driver thread per relay
  (`Pool::start_with(cfg, Timings)` shortens the deadlines for tests);
  `publish`, `subscribe`, `unsubscribe`, `poll` and `statuses` never block
  (they only touch per-relay mutexes the drivers never hold across IO).
  - **Connect:** the URL's host is resolved, IPv4 addresses first, each
    tried with a 5 s timeout; the TLS and WebSocket handshakes then get 5 s
    in all (tungstenite's `AttackCheck` also caps the response head at
    64 KiB and 512 reads, and fails many tiny reads);
    `client_tls_with_config` with our rustls `ClientConfig`: aws-lc-rs,
    Mozilla's roots from `webpki-roots` plus
    `extra_roots`, never the platform's. `ws://` is allowed. A bad URL is
    `Failed` for good; anything else retries after 1, 2, 4 … 60 s, each
    ±20 % (`RandomState`, so one build's clients do not reconnect to a
    recovering relay at once; final review A-M7; a connection up 60 s
    resets the backoff).
  - **Serve:** a non-blocking socket (ruling PF12-4: no `SO_RCVTIMEO`/
    `SO_SNDTIMEO`, whose expiry leaves a Windows socket undefined) behind
    an I/O budget in `Sock`. Each turn of the loop first sends (the
    turn's sends share a cumulative 5 s deadline, then
    `Dropped("send timed out")`; a full send buffer waits `POLL`, 10 ms,
    between flushes), then reads for at most one `TICK` (50 ms), waiting
    `POLL` only when nothing is left to read, so bursts drain at full
    speed. Each handshake round reads for one `TICK` and waits `POLL`.
    Reads also stop once the pool is stopping. A relay streaming or
    dripping bytes thus cannot hold the driver past a deadline or a stop.
    On `WouldBlock`, rustls keeps partial records and unsent output,
    tungstenite partial frames and unflushed output, and the handshake
    resumes. Subscriptions are reconciled from the pool's set
    (`REQ`/`CLOSE`) and sent again after every reconnect.
  - **Liveness:** after 30 s with nothing received, a WebSocket ping;
    nothing at all within 10 s of it is `Dropped("no answer to ping")`.
  - **Dropped** (then backoff): a frame above `max_frame` (clamped to
    `MAX_FRAME`), a frame that is not a Nostr message (not JSON, or not an
    array headed by a type; unknown types such as `AUTH` and malformed
    single events are skipped), more than 200 frames/s or 1 MiB/s
    sustained (token buckets holding 5 s of burst: a `REQ`'s stored events
    arrive at once), no `EOSE` within 10 s of a `REQ` (`CLOSED` cancels the
    wait), the liveness and send deadlines, a close or socket error.
  - **No `OK` within 5 s** of an `EVENT` is reported as `PoolEvent::Ok {
    accepted: false, message: "no answer within 5 s" }`; the connection
    stays (a later `OK` is ignored).
  - **Events** are verified in the driver (a forged id never shadows the
    real event in the dedupe), only for subscriptions the connection has
    and only when one of its filters selects them (`Filter::matches`: a
    relay may push any kind under our subscription; final review A-M5); `EOSE` and `OK` only answer what was asked. `poll` appends relay
    by relay in index order, each relay's reports in arrival order, and
    dedupes events by id (an LRU of 4096 ids).
  - **Queues:** outgoing at most 256 per relay (oldest dropped; a queued
    replaceable/addressable event is replaced by a newer one for its
    slot); incoming at most 4096 events or 8 MiB per relay between polls,
    each event counted by its wire bytes or its memory, whichever is more
    (`footprint`: a 5-byte `[""]` tag takes tens of bytes in memory; the
    newest dropped, `dropped_events()`). Queued events leave the
    queue before they are sent: those of a turn whose send fails, or sent
    on a connection lost before its `OK`, are lost (not retried after the
    reconnect; announcements are re-published every minute).
  - **Drop** stops every driver: each notices within about a `TICK`
    (even mid-read or mid-send), sends what is queued (a closing note)
    and the Close frame within 1 s, and is joined within 2 s in all (one
    stuck in a TCP connect or DNS lookup is detached).
- **`stun`** (N40): `binding_request(txn)`, `parse_binding_response(bytes, &txn)`
  re-export `rl_proto::stun`'s codec (moved there, N57, with its tests:
  XOR-MAPPED-ADDRESS, MAPPED-ADDRESS only when no XOR one; v4 and v6; the
  address as given, no filtering: callers use `list::is_global`, itself a
  re-export of `rl_proto::is_global`; bounded, integrity and fingerprint
  unchecked), and `public_ip(servers, deadline)`:
  `host:port` strings, IPv4 only, ephemeral `0.0.0.0:0` socket, 500 ms x 2
  tries per server, no lookup started past `deadline` (one lookup may
  block longer): run it on a thread. Only a reply from the server's own
  address with our transaction counts; a read failing with
  `ConnectionReset`/`ConnectionRefused` (Windows' `WSAECONNRESET` from an
  earlier server's ICMP unreachable) keeps waiting (final review A-M8). `StunResponder` (feature `test-relay`;
  `start`, `start_plain`, `server()`, `answered()`) is the test server;
  `stun::response` (same feature) is `rl_proto::stun::binding_response`.
- **`invite`** (N42) and **`defaults`**: `Invite::{encode, decode, relay_urls}`,
  `rlx1…` bech32m of a postcard `Wire` (version byte 1, key, `RelayHint`s,
  IPv4 addr as 4 bytes + port). `decode` trims, takes all upper case (not
  mixed), refuses text over `MAX_CODE` (200) as `Invalid` (the `bech32`
  0.12 crate's own Bech32m limit is 1023, so the 90 limit is simply not
  applied), text without the `rlx1` prefix is `NotInvite`, any checksum or
  case failure, a plain-bech32 checksum or an HRP other than exactly `rlx`
  (`rlx1foo1…` is HRP `rlx1foo`) `Damaged`, version above 1 `Newer` (checked before parsing),
  version 0, bad postcard, trailing bytes or bad hints `Invalid`. Hints (PF12-6): at most
  3 `Default` and 1 `Url` (`wss://`/`ws://`, at most 64 bytes); a
  `Default` index beyond this build's list (a newer build's code) is
  skipped, not `Invalid`, so the other hints serve; the worst case is 191 characters, within `MAX_CODE`. `encode` is
  infallible and truncates: it drops bad hints, then those beyond the
  limits (first win), so its code always decodes. `DEFAULT_RELAYS` is append-only (indices are wire format;
  `defaults`' test pins the released prefix). Both lists were chosen by
  `rl-cli nostr-probe`/`stun-probe` on 2026-10-13 (plan 12a Task 8; the
  table is in STATUS.md N38).
- Deps: `rustls` (default aws-lc-rs) and `webpki-roots` (workspace pins),
  `tungstenite` with `rustls-tls-webpki-roots` (for `Connector::Rustls`;
  its own default roots are never used). `rcgen =0.14.10` (`aws_lc_rs`
  only, no pem) is optional behind `test-relay`. The crate's own
  dev-dependency on itself turns the feature on for its tests.
- **`announce`** (N39): `GameSummary` (JSON, mod id under `mod`; `mode`
  free, at most 16 chars; unknown `region` reads as `Unknown`);
  `GameSummary::of_lobby(&LobbyState, &Hosting)`, the one summary of a
  hosted lobby for rl-server and rl-game (plan 12a Task 9: here, since
  `LobbyState` and `LanAnnounce` are rl-proto's, with no new same-layer
  pair): players = seated members + bots, spectators = the other members,
  each held to its maximum, `playing` from `Starting` to `Playing`, mode
  "deathmatch", the mod's hash from the setup's packs (else `NO_HASH`),
  `map_name` cut to 40 characters (blank or invalid: the level id);
  `lan()` is the same game's `LanAnnounce`. `announcement(..)`
  (tags `d`, `t`, `f` = 16 lowercase hex, `expiration` +180 s, +10 s when
  `closed`), `parse_announcement` (validates names, ids and counts, no address policy; refuses
  a missing or malformed `f`; does not verify the signature) and the sans-IO
  `Announcer` (every 60 s, at a change after 12 s, `closing()` once, only
  after a first publish).
- **`list`** (N41): `GameList::offer` checks cheaply first (kind, size, age
  at most 180 s, not in the future by over 60 s, expiration, blocked, rate
  gap, staleness, a full list), then verifies and parses. One note per key
  per 10 s of `now_unix`, or a note dated at least 10 s after the key's
  last (final review A-M2: a subscription or a relay delivering an older
  version first no longer holds the newest back; a closing note of a
  listed game is exempt); only newer `created_at` than the key's last
  replaces it; every accepted closing note leaves a tombstone, listed or
  not (final review A-I1: a relay that missed the close cannot bring the
  game back with its older open note); at most 500 games (new keys
  refused when full, before the signature check) and 4096 remembered keys
  (when full the oldest unlisted one is evicted). Announced addresses must be global
  (`list::is_global`, v4-mapped unwrapped) unless `allow_local(true)`. Refusals
  that need no signature check (stale, inside the gap) come first. `len()`/`is_empty()`; `games()`
  sorts compatible first, then ping (measured, unknown/measuring, no answer),
  name, key. `offer` returns whether the list changed (the game rebuilds
  its view only then); `block`/`unblock` (an unblocked key is listed from
  its next note). `filter()` has no `since` (no clock); `filter_since(now)` does.

- **`hosting`** (plan 12a Task 10: shared by rl-server and rl-game, moved
  from rl-server's `finding.rs`): `AddressWatch` (N40, a driver: the
  configured address wins; else `stun::public_ip(stun, STUN_DEADLINE 3 s)`
  on a thread at the start and every `ADDRESS_EVERY` (10 minutes), an
  answer kept only when `is_global`; a later round without an answer
  keeps the address, one answering a non-public address drops it; `poll`
  returns a log line when the address is first settled or changes),
  `relay_hints` (default relays by index, plus the first other URL of at
  most `MAX_URL_BYTES`), `local_relays(relays)` (every relay `localhost`
  or a loopback/private IP literal: lists may take local addresses
  (PF12-8) and a host may announce a private configured address; a host
  name other than `localhost` counts as public), `invite(key, relays, addr, port)` (IPv4 only),
  and `Publisher` (sans-IO: the `Announcer`'s timing, notes signed by the
  host's identity; `new(identity, after)`, the first note dated `after` at
  the earliest (a game hosted again right after its closing note, final
  review A-M6); `tick(now, now_unix, summary, aux)` → the note when due,
  `closing(now_unix, ..)` once, at least a second newer than the last
  note. `created_at` is the wall-clock `now_unix` the caller reads at each
  note, never a monotonic clock anchored once, which falls behind after a
  suspend or an NTP step and dates every note expired at birth (final
  review A-I2)).
- **`ping`** (N41, a driver): `Pinger::ping(key, addr)` times a TCP
  connect on a thread (`PING_TIMEOUT` 2 s → `Ping::NoAnswer`), one per key
  and at most `MAX_PINGS` (4) at once; `poll` collects the answers.

Test: `cargo nextest run -p rl-discovery` (`tests/announce.rs` and `tests/list.rs`: announcements and the game list; `tests/nostr.rs`: the published
events verify, signing round-trips, canonical escaping, tampering, frames
round-trip, fuzzed frames never panic; `tests/test_relay.rs`: the relay;
`tests/stun.rs`: `public_ip` and the codec's re-exports; `tests/nip44.rs`:
the official vectors, padding behind a good MAC, fuzzed payloads;
`tests/handshake.rs`: round trip, repeats and resends, replays, malformed
offers, a host-only reply, filters, secret redraws, a maximal offer and
an unsendable one, honest redundancy not counted as ignored, the hostile feed
(`a_hostile_handshake_feed_is_bounded`: wrong recipient, too old, in the
future, 2000 fresh sessions in 2 ms take at most 20, a steady 10/s for
200 s keeps about 1200 sessions, tampering), 200 undecryptable events
from fresh keys leaving the whole burst of 20 to honest offers
(`undecryptable_events_spend_no_tokens`), fuzzed events;
`tests/turn_note.rs`: notes, validation, local addresses, the pick order,
the flood bound, closing notes, a closing-note flood keeping a closed
relay's tombstone, fuzzed notes; `tests/invite.rs`: invite codes (round trips, hints, damage, proptest `encode_always_decodes`); unit tests of `hosting` (hints, IPv4-only
invites, the address watch's answers, the publisher's notes and closing)
and `ping` (an open and a closed port, the bound); `tests/pool.rs`: the pool against the test relay (silent, dripping,
slow, slow-reading: every frame arrives intact through partial writes,
over `ws://` and `wss://`; a relay dying mid-frame), a flooding relay and unreachable relays: a
listener on `127.0.0.1` that never accepts, and TEST-NET-1 `192.0.2.1`, never
routed; unit tests of the backoff, limiter, queues, dedupe and default
timings in `src`).
