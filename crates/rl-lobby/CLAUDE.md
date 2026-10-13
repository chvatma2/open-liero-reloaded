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
  is pinned by a test. `DtlsFingerprints` = (offer side's, answer
  side's) SHA-256 certificate fingerprints, the same at both ends of a
  link (rl-net's `dtls_fingerprints`). `challenge_message(nonce, host,
  fps)` is `b"rust-liero join v1" ++ host nonce ++ host key ++ fps.0 ++
  fps.1`, what a joiner signs; `host_proof_message(client_nonce, client,
  fps)` is `b"rust-liero host v1" ++ joiner nonce ++ joiner key ++ fps.0
  ++ fps.1`, what the host signs (N37); `password_proof(nonce, password)`
  is `SHA-256(nonce ++ password)`.
- The secret lives in `<data>/identity.ron` (rl-ui's `Store::load_identity`).
- **Host** (`host.rs`, `LobbyHost`): transport events, messages and time in;
  `outgoing()` (peer, `ToClient`) and `events()` (`HostEvent`) out. State
  changes are coalesced: `state()` is always current, and one `Lobby` per
  `outgoing()` drain goes to every member (also flushed right before a
  `Notice(Joined)`, so the others know the member it names).
  - Join: `connected(peer, ip, now, nonce, fingerprints)` sends `Challenge` (a banned IP:
    `Refused(Banned)`; an IP held off for guessing: `Refused(Throttled)`);
    `Hello` is checked for the guessing hold again (`Throttled`: links
    opened before it get no more guesses), bans (`Banned`), a key already in the lobby or
    the host's (`Busy`), room (`Full`) and the password proof
    (`Password`; `PASSWORD_TRIES` (3) wrong proofs from one IP within
    `PASSWORD_WINDOW_US` (60 s) hold it off for `PASSWORD_LOCKOUT_US`
    (60 s); the empty password's proof is no guess; at most
    `PASSWORD_ADDRESSES` (1024) addresses remembered, the oldest
    forgotten); a `Hello` passing them is answered at once with
    `HostProof`, the host's signature of `host_proof_message(Hello's
    nonce, joiner key, fingerprints)` (BIP-340 aux: the challenge's nonce,
    fresh per link); `Prove` must verify over
    `challenge_message(nonce, host key, fingerprints)` (else `Malformed`); then
    `Welcome` to the joiner, `Lobby` to the others, `Notice(Joined)` to
    all. Anything else before the proof, a second `Hello`, or no proof
    within `PROOF_TIMEOUT_US` (10 s): dropped. Every refusal and drop is
    `HostEvent::Kick` (close the transport); a member sending
    `Hello`/`Prove` again is kicked. `Bye` (before or after the proof) is
    `HostEvent::Close` (close the transport; a member also `Left`).
  - Seats: players take the lowest free slot below `max_players`; when
    full, spectators (members ≤ `max_players + max_spectators` ≤ 48). A
    joiner during a match is `Waiting` while a seat is free for the next
    match (a waiting member holds a seat only if it wants to play, not a
    spectator made to wait for its content). While gathering, members who want to play (not spectating by
    choice) take free seats in join order (`reseat`). Names are sanitized
    and made unique ignoring case with " (n)" (cut to 20 characters).
  - Ready: said ready with nothing `missing`; `Ready(true)` with missing
    content is refused (`MISSING_CONTENT` notice), missing content clears
    it, a setup change clears every member's but the host's (the host
    starts ready) unless only its bots changed (ruling T13-C: a server's
    fill bots, Add/Remove bot), and so does the return to gathering.
    `set_setup` outside gathering is `LobbyError::NotGathering` (the
    running match's setup stays what the lobby shows). `LobbyError` and
    `NotStartable` display as words for players (`Display`), never their
    Rust names.
  - `ip_of(member)`: the address it joined from (the content side counts
    skin fetches by address).
  - `revision()` changes whenever `state()` does (compare it instead of
    cloning the state: the dedicated server's poll).
  - Chat: sanitized and cut to 200 characters; `CHAT_LINES` (5) per
    `CHAT_WINDOW_US` (5 s) per member, beyond: dropped and `SLOW_DOWN` to
    the sender. Relayed as `Chat` to all and `HostEvent::Chat`. A dedicated
    server's `chat` is a `Notice::Message`.
  - Looks: `LOOK_CHANGES` (4) `SetLook`s per `LOOK_WINDOW_US` (1 s) per
    member are taken (each rebuilds the host's content list and may
    start a skin fetch; plan 11 review C2); beyond them only the latest is
    held (`look_held`) and `tick` takes it once the window allows, so the
    member's last look always wins (a dragged colour slider).
  - Kick/ban: the host (`kick`) or an admin key (`AdminCommand::Kick/Ban`);
    the host is never kicked; a ban stores the key for the lobby's life
    (spec §5.2, ruling T13-B: by key); `ban_with_address` (the host's,
    a server's `/banip`) also bans the member's IP (`banned`, `banned_ip`
    for the endpoint's gate; `banned_keys`/`banned_ips` list them,
    `ban_key`/`ban_ip` add a bans file's, `unban_key`/`unban_ip` lift them).
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
    `force_start` (ruling T6-A, spec §7.2): players not ready watch this
    match as spectators and get seats back at the next gathering; nobody
    moves when the ready players alone cannot start.
  - Content (N33): `lacking()` lists the members whose last `Have` named
    something missing, or that have not answered the current content list:
    `set_content_revision(r)` (the online layer's `ContentMsg::List`
    revision) makes every member lacking until its `Have { revision: r }`
    (one for another revision is ignored; without a revision every
    `Have` counts). A lacking member is not ready (a `Ready(true)` while
    only unanswered is kept and counts once answered). At a start (`announce_start`; `force_start` for
    players) a lacking member is `SeatWire::Waiting`: no `Start`, still
    connected, seated again at the next gathering. A member welcomed during
    a match is waiting too. Either way `HostEvent::ContentComplete(id)`
    fires once, on the `Have` that empties its list (so a client sends
    `Have` after the welcome, empty or not); the online layer lets it
    spectate then, not at `Joined`. `set_download(member, percent)`
    publishes `Member::download` (cut to 100), at most once per member per
    `DOWNLOAD_EVERY_US` (250 ms) of the latest time given to the host; a
    held-back value is shown by `tick`; an empty `Have` clears it.
  - `set_ping(peer, ms)`: the transport's round trip in `Member.ping_ms`
    (a change is a `Lobby`: call it every few seconds).
  - Leaving (`Bye`, `disconnected`): removed, `Notice(Left)`,
    `HostEvent::Left`; during a match the slot stays in the match.
  - The state never breaks rl-proto's decoders (looks, names, chat, end
    reasons sanitized; setups validated: `LobbyError::InvalidSetup`).
- **Client** (`client.rs`, `LobbyClient`): `new(.., expected_host)`,
  `connected(fingerprints)` (the link's, as this end saw them); on
  `Challenge` sends `Hello` (with the password proof when asked, and the
  `aux` given with the challenge as its nonce); on `HostProof` that
  verifies under the challenge's key over `host_proof_message(its nonce,
  its key, fingerprints)` sends `Prove`. A challenge naming another key
  than `expected_host` (nothing sent) or a proof that fails (`Hello`
  alone sent) is `WrongHost` (carried ruling F-B, N37): a man in the
  middle presents its own DTLS certificates, so the fingerprints differ
  and both proofs fail; without `expected_host`, an impostor proving its
  own key is not caught (invites pass the key). A `Welcome` before the
  proof is ignored; a `Welcome` or `Lobby` without
  this member closes (`Closed`); `ClientEvent`s for the welcome, refusals, lobby
  changes, chat, notices, start, end, kicks and closing. Actions are sent
  only once joined; `leave()` sends `Bye`.
- **Harness** (`harness.rs`, `Net`; behind the `test-hooks` feature,
  enabled by this crate's own dev-dependency): the host and clients in one process,
  messages tagged and encoded through rl-proto, a fixed one-way delay
  (what arrives at one instant is delivered before the drain: one poll),
  virtual time (`settle`, `run`), raw peers for forged messages
  (`join_raw`, `send_raw`), every link's DTLS fingerprints `FINGERPRINTS`
  (`join_seeing`: a client seeing others, a man in the middle),
  `disconnect`, and the match layer's `start`.

Test: `cargo nextest run -p rl-lobby` (`tests/lobby.rs`: one test per rule
and a proptest of random messages and host actions — a crowd of worst-case
members included — keeping every sent message decodable).
