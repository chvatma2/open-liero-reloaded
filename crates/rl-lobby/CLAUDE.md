# rl-lobby

Online lobby logic (networking spec §4, §5.2, §5.4, §7.2), sans-IO.

- **Layer 4, `#![no_std]` + `alloc`, wasm32.** Depends on `rl-proto` (same
  layer: the lobby messages, their validators), `rl-netcode` (same layer:
  `PeerId`, `input_delay`), `k256` (BIP-340 Schnorr, exact pin) and `sha2`
  (crate-local, no default features). Time and randomness come from the
  caller (`now_us`, nonces, signing `aux`); there is no clock and no
  `getrandom` here. `BTreeMap`/`BTreeSet` only.
- **Identity** (`identity.rs`): `Identity::from_secret(secret)` (`None` for
  an invalid scalar), `public()` (x-only, 32 bytes), `sign(message, aux)`
  (BIP-340 with the caller's 32 aux bytes), `verify(key, message, sig)`
  (false for any bad input, never a panic). `fingerprint(key)` is 8
  characters: lowercase unpadded base32 of `SHA-256(key)[..5]`; the format
  is pinned by a test. `challenge_message(nonce, host)` is
  `b"rust-liero join v1" ++ nonce ++ host`, what a joiner signs;
  `password_proof(nonce, password)` is `SHA-256(nonce ++ password)`.
- The secret lives in `<data>/identity.ron` (rl-ui's `Store::load_identity`).
- **Host** (`host.rs`, `LobbyHost`): transport events, messages and time in;
  `outgoing()` (peer, `ToClient`) and `events()` (`HostEvent`) out. State
  changes are coalesced: `state()` is always current, and one `Lobby` per
  `outgoing()` drain goes to every member (also flushed right before a
  `Notice(Joined)`, so the others know the member it names).
  - Join: `connected(peer, ip, now, nonce)` sends `Challenge` (a banned IP:
    `Refused(Banned)`); `Hello` is checked for bans (`Banned`), a key
    already in the lobby or the host's (`Busy`), room (`Full`) and the
    password proof (`Password`); `Prove` must verify over
    `challenge_message(nonce, host key)` (else `Malformed`); then
    `Welcome` to the joiner, `Lobby` to the others, `Notice(Joined)` to
    all. Anything else before the proof, a second `Hello`, or no proof
    within `PROOF_TIMEOUT_US` (10 s): dropped. Every refusal and drop is
    `HostEvent::Kick` (close the transport); a member sending
    `Hello`/`Prove` again is kicked. `Bye` (before or after the proof) is
    `HostEvent::Close` (close the transport; a member also `Left`).
  - Seats: players take the lowest free slot below `max_players`; when
    full, spectators (members ≤ `max_players + max_spectators` ≤ 48). A
    joiner during a match is `Waiting` while a seat is free for the next
    match. While gathering, members who want to play (not spectating by
    choice) take free seats in join order (`reseat`). Names are sanitized
    and made unique ignoring case with " (n)" (cut to 20 characters).
  - Ready: said ready with nothing `missing`; `Ready(true)` with missing
    content is refused (`MISSING_CONTENT` notice), missing content clears
    it, a setup change clears every member's but the host's (the host
    starts ready), and so does the return to gathering.
  - Chat: sanitized and cut to 200 characters; `CHAT_LINES` (5) per
    `CHAT_WINDOW_US` (5 s) per member, beyond: dropped and `SLOW_DOWN` to
    the sender. Relayed as `Chat` to all and `HostEvent::Chat`. A dedicated
    server's `chat` is a `Notice::Message`.
  - Kick/ban: the host (`kick`) or an admin key (`AdminCommand::Kick/Ban`);
    the host is never kicked; a ban stores the key and the member's IP for
    the lobby's life (`banned`, `banned_ip` for the endpoint's gate;
    `banned_keys`/`banned_ips` list them, `ban_key`/`ban_ip` add a bans
    file's).
    `Next`/`Settings` from an admin are `HostEvent::Admin`; non-admins get
    `NOT_ADMIN`.
  - Start (ruling PF1): `can_start` (gathering, at least one player —
    a member or the host's, `NoPlayers` — 2..=32 worms with bots, every
    player ready; `StartRequested` when it turns `Ok`) → `start(seed,
    rtts)` returns `MatchStartWire` (players by slot, then `setup.bots` in
    order; `input_delay(rtt)` per player, the host 0, an unmeasured one
    `MAX_INPUT_DELAY`; `start_checksum: 0`) and sends nothing but a
    `Notice::NotReady` on failure → the match layer fills the checksum →
    `announce_start(wire)` (`Stale` if anything a start depends on changed
    since `start`: a generation counter; pings do not count): `Start` to
    everyone, `Phase::Starting` →
    `playing(tick)`: `Phase::Playing`. `match_ended(end, now)`: `Ended` to
    all, `Phase::Results` for `RESULTS_US` (5 s, in `tick`), then gathering.
    Ids of the running match's members are not reused until it ends.
    `force_start` (ruling T6-A, spec §7.2): players not ready (or lacking
    content) watch this match as spectators and get seats back at the next
    gathering; nobody moves when the ready players alone cannot start.
  - `set_ping(peer, ms)`: the transport's round trip in `Member.ping_ms`
    (a change is a `Lobby`: call it every few seconds).
  - Leaving (`Bye`, `disconnected`): removed, `Notice(Left)`,
    `HostEvent::Left`; during a match the slot stays in the match.
  - The state never breaks rl-proto's decoders (looks, names, chat, end
    reasons sanitized; setups validated: `LobbyError::InvalidSetup`).
- **Client** (`client.rs`, `LobbyClient`): `new(.., expected_host)`: a
  challenge naming another key is not answered (`WrongHost`);
  `connected()`, then on `Challenge` sends `Hello` (with the password
  proof when asked) and `Prove` at once; a `Welcome` or `Lobby` without
  this member closes (`Closed`); `ClientEvent`s for the welcome, refusals, lobby
  changes, chat, notices, start, end, kicks and closing. Actions are sent
  only once joined; `leave()` sends `Bye`.
- **Harness** (`harness.rs`, `Net`): the host and clients in one process,
  messages tagged and encoded through rl-proto, a fixed one-way delay
  (what arrives at one instant is delivered before the drain: one poll),
  virtual time (`settle`, `run`), raw peers for forged messages
  (`join_raw`, `send_raw`), `disconnect`, and the match layer's `start`.

Test: `cargo nextest run -p rl-lobby` (`tests/lobby.rs`: one test per rule
and a proptest of random messages and host actions — a crowd of worst-case
members included — keeping every sent message decodable).
