# rl-net

Real transport for online play (networking spec §2.1, §5.1): WebRTC data
channels over the sans-IO str0m stack, signalling over a WebSocket, LAN
announcements.

- **Layer 5, std; not wasm.** Depends on rl-netcode (`Channel`, `PeerId`,
  `netsim::LinkModel`, `Rng`), rl-proto (`Signal`, `Path`, the STUN codec,
  the limits' sizes), str0m,
  tungstenite (`=0.30.0`, default features: no TLS), getrandom
  (`=0.4.3`, `random_bytes`: the native peers' randomness),
  turn-client-proto (`=0.7.2`, default features: `std`; N52: `Core`'s
  TURN client) and, for our own relay server (ruling TR-2), md-5
  (`=0.11.0`), sha1 (`=0.11.0`), hmac (`=0.13.0`) and crc32fast
  (`=1.5.2`). Real clocks and sockets live only in this crate's socket
  drivers, `endpoint` and `turn::TurnServer` (and the binaries); `link`,
  `core` and `turn::TurnCore` are sans-IO, and `endpoint::poll` takes the
  time.
- **TURN client pins.** `Cargo.lock` holds turn-client-proto's
  transitive crates under two weeks old back (`cargo update -p <crate>
  --precise <ver>`): stun-proto 2.0.2 (2.0.3), stun-types 2.0.2 (2.0.3),
  turn-types 0.7.2 (0.7.3). `cargo tree -p rl-net -e normal` shows them.
  turn-server-proto is not used (ruling TR-2): its auth layer kept a
  nonce per source address forever, its permissions grew without bound,
  it relayed to any peer, and it addressed binding answers to itself.
- **str0m pinned exactly** (`=0.24.0`, decision N15), `default-features =
  false, features = ["rust-crypto"]`: the default features pull the
  `examples` web server and select aws-lc-rs as the crypto backend.
  It needs no system C libraries and cross-builds with mingw
  (str0m-rust-crypto enables dimpl's `rcgen` feature, which builds
  aws-lc-sys from source with `cc`/cmake; the mingw toolchain handles it
  for `x86_64-pc-windows-gnu`). `Cargo.lock` holds transitive crates
  under two weeks old back (dimpl 0.7.4, aws-lc-rs 1.18.1, aws-lc-sys
  0.45.0, zeroize 1.9.0, powerfmt 0.2.0). `=0.24.1` (SCTP send-buffer limit
  configurable) may replace `=0.24.0` from 2026-10-17.
- **`link::Link`**: one peer connection, sans-IO. The offerer makes the SDP
  offer (`Link::offerer`), the answerer answers it (`Link::answerer`), the
  offerer takes the answer (`accept_answer`); the caller carries both
  strings. Host candidates are the `local` UDP addresses (decision N16);
  `offerer_with` / `answerer_with` take `LinkCandidates` (host addresses,
  server-reflexive and relayed `(address, base)` pairs: str0m's
  `Candidate::host`, `server_reflexive`, `relayed`, added before the SDP
  is made). str0m reports its stats every second
  (`set_stats_interval(1 s)`); `selected_pair()` is the latest stats'
  selected candidate pair (local base or relay address, remote address).
  Time is `std::time::Instant`, always an argument (the netcode's `now_us`
  is converted by the endpoint). `accepts` demultiplexes a shared socket;
  `receive` takes a datagram (non-WebRTC bytes are ignored); `poll` feeds
  the time, flushes queued sends and hands out `Transmit`s, `LinkEvent`s
  (`Open` once both channels are open, `Message`, `Closed { why: Ended }`
  once — `Peer`, `Lost` (ICE), `Failed`; nothing after a local `close`)
  and the next timeout. `dtls_fingerprints()` (from `Open`, else `None`):
  the offer side's and the answer side's SHA-256 certificate
  fingerprints, the same at both ends — ours as our SDP announced it, the
  peer's as its DTLS handshake presented it (str0m ends a link whose
  certificate differs from its SDP `a=fingerprint`); the lobby's proofs
  sign them (N37). Every link has its own certificate.
- **Channels** (decision N17), both negotiated out of band, the same config
  on both sides: `INPUTS` = stream id 0, `"inputs"`, unordered,
  `MaxPacketLifetime { lifetime: 1 }` — in effect no retransmits
  (`Channel::Unreliable`); `CONTROL` = stream id 1, `"control"`, reliable,
  ordered (`Channel::Reliable`). Not `MaxRetransmits { retransmits: 0 }`
  (decision N17 as amended): sctp-proto 0.10.5 abandons such chunks as it
  first sends them, so every SACK triggers a FORWARD-TSN and every
  FORWARD-TSN a SACK — under irregular polling a storm of ~45 datagrams
  per message that overflows socket buffers and loses inputs (regression
  test `irregular_polling_does_not_multiply_the_inputs_traffic`). `send`
  refuses messages above `MAX_MESSAGE` (64 KiB, str0m's assumed remote
  maximum) and empty ones (str0m 0.24.0 makes no chunk for them but counts
  them, so the ordered channel stalls behind one).
- **Buffering**: str0m 0.24.0 refuses a write that does not fit its 128 KiB
  SCTP send buffer (shared by the streams), so `Link` queues messages per
  channel and moves them into str0m as room appears: inputs first, control
  leaving 16 KiB free for inputs. `buffered(channel)` = the queue plus
  str0m's buffered amount; big transfers keep it bounded (chunks of 32 KB,
  under 256 KB queued). The link's queue has no limit: callers check
  `buffered` (the endpoint drops sends beyond `MAX_QUEUED`, 4 MiB per peer
  and channel, counted in `PeerStats::sends_dropped`; a reliable message
  it cannot queue, beyond `MAX_QUEUED` or refused by the link, ends the
  link at the next `poll`, `Disconnected { Unsent }` (our failure,
  not the peer's; rl-online tells a client so): the ordered channel
  never loses a message silently. rl-online keeps its own backlog and
  feeds it while `buffered` is below 1 MiB).
- **str0m's contract** — every mutation of the `Rtc` (input, write, SDP or
  direct API change, candidate) is followed by draining `poll_output` until
  it returns a timeout, before the next mutation — is kept inside `Link`:
  every method that mutates drains into the link's own buffers (`fail`
  too, after `rtc.disconnect()`). Keep it so when adding methods.
- **`core::Core`**: the endpoint without sockets (the spine of plan 12b's
  ICE work; tested on a virtual wire, `tests/core_wire.rs`). It owns every
  peer's `Link`, the limits, the impairment, peer ids, `SETUP_TIMEOUT` and
  `SILENCE_TIMEOUT`, the stats and `MAX_QUEUED`'s `Unsent`; `admit` makes
  the version, fingerprint, `Full` and `Exhausted` checks of an offer.
  Each connection attempt is a `SetupId` (`offer` / `accept_answer` on a
  client, `answer` on a host, `cancel` when its signalling fails) until
  `CoreEvent::Opened` or `SetupFailed`; `receive(from, bytes, now)` takes a
  datagram, `poll(now, out, events)` hands out `Datagram`s and
  `CoreEvent::Net(NetEvent)`s. A cancelled link's last datagrams leave with
  the next `poll`. `CoreConfig::host()` holds the defaults
  (`EndpointConfig::default` takes them from it). The shared vocabulary
  (`HOST`, `MAX_QUEUED`, `SETUP_TIMEOUT`, `SILENCE_TIMEOUT`, `NetEvent`,
  `Why`, `PeerStats`) is defined in `core`; `endpoint` re-exports it under
  the same paths. The driver reports each datagram its socket took or
  refused (`Core::sent`), counted in the stats of the datagram's setup:
  setup-time datagrams count, as before the split; without the reports the
  stats count no sends.
  - **Signalled setups** (plan 12b, N48-N50; Nostr carries their SDP from
    Task 7; `core/signalled.rs`): `set_base(base, hosts)` (every signalled link's datagrams are
    taken as arriving at `base`; TCP-signalled links keep their own host
    candidate, N16), `stun(servers, now)`, `set_mapped`, `local()`
    (`LocalCandidates`), `nat()`, `gathered(now)`; `offer_ice(hint, now)`
    and `accept_answer_ice(setup, answer, mapped_hint)` on a client
    (the answer is sanitized too; no F-G rewrite), `answer_ice(offer,
    mapped_hint, now) -> Signal` on a host (`admit`, then the pending
    table, then `sanitize` and the answerer link with every local
    candidate). They must open within `SIGNALLED_SETUP_TIMEOUT` (15 s).
    At most `MAX_SIGNALLED_PENDING` (8) connect at once: a new offer then
    evicts the oldest whose peer has sent no ICE connectivity check (a
    STUN binding request reaching its link), or is `Refused { Busy }`
    when every one has (PF12b-2); the eviction's `SetupFailed { Timeout }`
    comes with the next `poll` (calls without an event list queue theirs
    in `Core`, like `outbox`). Offers and answers carry the
    server-reflexive and the mapped address on the base (not when equal
    to it), then as many host candidates (base first) as fit
    `MAX_CANDIDATES` in all, so the far side's `sanitize` never cuts the
    reflexive ones (review I-1); `srflx` follows STUN's latest answer, so a renumbered router's
    new address is offered from its next round.
  - **Paths** (N49): `NetEvent::Connected { path }` and `path(peer)`. A
    TCP-signalled link is `Direct`, reported at `Open`. A signalled link
    is classified (`candidates::classify`) from its selected pair against
    `local()` and the peer's SDP candidates plus its mapped hint, and
    re-classified when the pair changes; its `Connected` waits for
    str0m's first stats naming a pair, at most 1.5 s after `Open`
    (`HolePunched` without one), its `addr` becomes the pair's remote
    address (without a pair, the latest source its link took a datagram
    from: never 0.0.0.0, which the lobby's IP bans key on; it follows the
    pair as ICE moves), and its messages wait until then (`Opened` still
    comes at `Open`: the driver lingers the signalling). `peers()` lists
    reported peers only (when signalled links reach the endpoint,
    rl-online's close-all over `peers()` misses one still waiting; the
    endpoint's drop closes it). Test hook `CoreConfig::suppress_stats`
    (`test-hooks`) ignores str0m's stats, to test the fallback.
  - **STUN on the game socket** (`probe.rs`, N48): each round asks the
    servers (IPv4, at most 4) with binding requests from the core's
    socket, once more after 500 ms if unanswered, at `stun` and every
    `STUN_EVERY` (25 s, which keeps the router's mapping alive);
    `gathered` once the first round is answered or `STUN_WAIT` (1 s)
    passed (at once when `stun` was never called or given no server). `receive` gives a datagram to the probe first when it answers
    an outstanding request from the server asked. `srflx` is the first
    answering server's answer (the next one's if it falls silent for a
    round); two answers of a round give `NatBehaviour` (`Open` when
    `srflx` is the base, `Preserving`, `Symmetric`). Transaction ids come
    from `CoreConfig::stun_seed` (the endpoint seeds it from the system's
    randomness). The probe's datagrams belong to no setup.
  - **TURN allocations** (`relayed.rs`, N48, N52; a joiner's):
    `allocate(servers, now)` asks each server (IPv4, at most 2 live
    allocations) from the base with one `turn_client_proto`
    `TurnClientUdp` each (`TurnConfig::new(TurnCredentials::new(user,
    pass))`); `relays()` lists each server's `RelayState` (`Allocating`,
    `Allocated(relayed)`, `Failed(why)`, `Released`). A relayed address
    equal to one of our own candidates (base, hosts, server-reflexive,
    mapped), or not global unless `CoreConfig::allow_local` (N62; natsim
    sets it), fails and is deleted: a hostile relay cannot capture our
    direct traffic. Permissions and relayed sends go only to peers a
    relay accepts (global, unless `allow_local`). `gathered` also
    waits for every allocation to settle, or `TURN_WAIT` (2 s). Allocated
    addresses are `local().relayed` and relayed candidates `(relayed,
    base)` of links made afterwards, inside the `MAX_CANDIDATES` budget
    with the reflexive ones (before hosts). `receive`: after the probe, a
    datagram from a TURN server goes to its client — a peer's data
    reaches the links as arriving at the relayed address (str0m's
    relayed candidate's base), what the client ignores goes on to the
    links. A link's transmit from a relayed address leaves through its
    client's `send_to` (counted in the link's stats; dropped while no
    permission is installed, as on a lossy path; ICE retries), and binds
    a channel for each peer address sent to (4-byte ChannelData headers
    once bound). The peer's SDP (`accept_answer_ice`, `answer_ice`) asks
    permissions for every candidate IP and the mapped hint's (at most 32
    permissions and channels per allocation). An allocation no open
    link's selected pair uses is deleted `RELEASE_AFTER` (5 s) after a
    `Connected`, while no setup is connecting (kept while a pair is
    unknown); `close_all` deletes every one. A dead relay's link is lost
    by ICE (`Disconnected { Timeout }`). The relays' own datagrams belong
    to no setup.
- **`candidates`** (N48-N50): `LocalCandidates`, `RemoteCandidates`,
  `TurnSpec` (`Core::allocate`), `MAX_CANDIDATES` (8). `sanitize(sdp)` keeps at most
  8 `a=candidate` lines that are UDP, IPv4, unicast (not unspecified,
  multicast or broadcast), with a port and a known `typ` (fields split on
  single spaces, as str0m parses them; no mDNS name, TCP or IPv6), other
  lines as they are; `remote_candidates` reads only those lines, by type
  (`prflx` with `srflx`); `classify(local, remote, ours, theirs)`:
  `Relayed` (either side's address a relay), `MappedPort` (the remote's
  mapped hint), `Direct` (a remote host candidate), else `HolePunched`.
- **`turn`** (N52, spec §5.3, ruling TR-2): `TurnCore` is our own
  sans-IO relay, the UDP subset of RFC 8656 that turn-client-proto and
  browsers use: Binding; Allocate (REQUESTED-TRANSPORT UDP, LIFETIME
  clamped to 600-3600 s, DONT-FRAGMENT accepted and ignored,
  REQUESTED-ADDRESS-FAMILY IPv4); Refresh (0 deletes); CreatePermission
  (at most 8 peers per request); ChannelBind (0x4000-0x7fff); Send and
  Data indications; ChannelData (unpadded). Long-term credentials: realm
  `REALM` `"rust-liero"`, one user, MESSAGE-INTEGRITY HMAC-SHA1 under
  MD5(user:realm:pass) and FINGERPRINT on every authenticated answer; a
  stateless nonce (16 hex digits: the 10-minute period, wrapping, and a
  48-bit HMAC tag of the source address and the period under a
  per-relay random key, `set_secret`, compared in constant time; valid
  in its period and the next), so an unauthenticated source costs no
  memory. Unknown comprehension-required attributes → 420; 437 for a
  5-tuple's wrong allocation; 440, 442, 443; 486 per-IP quota, 508
  capacity. The codec is `turn/msg.rs` (fuzzed).
  - **Reflection (review N2):** answers to sources nothing proves are
    few, small and capped. Only an Allocate naming REQUESTED-TRANSPORT
    gets the first 401; other unsigned requests, and signed ones lacking
    USERNAME/REALM/NONCE, get nothing. A signed request whose nonce is not
    ours for its source (or older than 80 minutes) gets 438 only if its
    integrity holds, else nothing. 401 and 438 are as small as RFC 8489 allows (ERROR-CODE
    without a reason, REALM, NONCE, no FINGERPRINT: 64 B), Binding answers
    XOR-MAPPED-ADDRESS only (32 B): 1.6× a 20 B Binding or
    turn-client-proto's 40 B first Allocate, 2.3× a bare 28 B Allocate.
    All answers to unproven sources share a relay-wide bucket of
    64 KiB/s (holding 2 s), whatever the spread of forged sources.
  - **Proven sources (review N6, N7):** a nonce whose tag is ours for the
    request's exact source (IP and port) and at most `PROVEN_PERIODS` (8,
    80 minutes: past `MAX_LIFETIME`) old proves the source heard our
    answer. Fresh (this period or the last) it is accepted; older, the
    request gets 438 with a fresh nonce, and a proven source's 401 or 438
    is sent outside the unproven budgets, so a blind flood that drains
    them cannot starve a live client's renewals. A proven request takes
    from the IP's proven bucket, any other from its unproven one: the
    IP's `per_ip_requests_per_s` split three to one (default 15 and 5 a
    second, each bucket holding 2 s; review N9), so forged requests in a
    client's name cannot starve its signed ones and the IP's total keeps
    the rate.
  - **Peers (TR-1):** IPv4, global (`rl_proto::is_global`) and not the
    relay's public or listening address, unless `set_allow_local(true)`
    (`TurnServerConfig::allow_local`, N62); CreatePermission and
    ChannelBind to others → 403, Send/ChannelData and peers' datagrams
    from them dropped.
  - **Bounds:** every table is a map: allocations by client 5-tuple and
    by port, expiries indexed by time, at most 16 permissions and 16
    channels per allocation; requests above 1500 B and any datagram (or
    relayed payload) above 4096 B are dropped unread and counted;
    at most 4096 client IPs, an entry only for a request source (never
    for data), the longest idle without an allocation evicted through a
    last-seen index (O(log n)).
  - **Limits** (`TurnLimits`: default 32 allocations, 4 per client IP,
    32 KiB/s relayed payload per IP each way, 20 requests/s; `generous()`
    for tests sharing 127.0.0.1): buckets holding 2 s; a datagram over one
    is dropped and counted (`dropped`), never the client; the down bucket
    is charged only once the peer's permission passed (a stranger cannot
    drain it). Ports come from the driver: `take_allocation_requests` /
    `allocated(req, Some(port) | None)` / `released()`; relayed
    addresses are `public:port`; nothing is sent to the listening address.
  - **`TurnServer::start(cfg)`** (`turn/driver.rs`): IPv4 `listen` and
    `public` only (TR-3: per-IP limits; else `InvalidInput`); port 0:
    ephemeral, `local_addr`. One reader thread per socket (the listening
    one, one per allocation: a free port of `ports`, or OS-assigned for
    `0..=0`) blocks in `recv_from` (200 ms timeout; a timeout or a reset —
    Windows reports ICMP port unreachables so — is read past at once,
    other errors too, sleeping 1 ms only after 16 in a row), drops and
    counts datagrams over 4096 B before copying them, and feeds a channel
    of 1024 (at most 4 MiB; a full one drops and counts); the core's
    thread waits on it (100 ms tick), so an idle relay sleeps.
    `stats()` (allocations, dropped, relayed bytes); dropping it stops
    and joins every thread (within about 200 ms).
- **`natsim`** (feature `test-hooks`, N56): `Net` runs `Core`s behind
  virtual NATs (full cone, address-restricted, port-restricted:
  endpoint-independent mapping keeping the internal port if free;
  symmetric: a seeded random port per destination; their filters), with
  router `forward`s, 120 s idle expiry, no hairpinning (nodes behind one
  NAT reach each other's private addresses directly), STUN servers, 10 ms
  per hop and a 1 ms virtual clock; each node's `impair_seed` and
  `stun_seed` come from `Net::new`'s seed; `gather` (STUN, and TURN
  allocations when given servers), `signal` (the signalling
  Nostr will carry), `run_until`, `connected` (the client's path),
  `events`, `renumber`. `turn_server(at)` runs a `TurnCore` (default
  limits, relay ports from `at.port() + 1` up on its IP) and returns its
  `TurnSpec` (local peers allowed, a nonce key from the net's seed);
  `kill(at)` silences it; `turn_dropped(at)`. Nodes' cores have
  `allow_local` (documentation and private addresses).
- **`endpoint::Endpoint`**: the socket driver of a `Core`: the UDP socket,
  the TCP listener and `SignalConn`s (mapped by `SetupId`), the per-IP
  caps, the gate, the connect thread, lingering closes, and the SDP
  rewrites T4-A (`keep_candidates_of`) and F-G (`rewrite_candidates`),
  which are about the TCP connection. `poll`'s order: signalling (accepts,
  offers, the connect), the socket's datagrams into `Core::receive`, each
  setup's signalling, `Core::poll` (its datagrams `send_to`), lingering
  closes. One non-blocking UDP socket
  for every link (a datagram goes to the link that `accepts` it, given the
  link's own host candidate as the destination; others are counted,
  `stray_datagrams`). `poll(now, events)` never blocks.
  - **Host** (`Endpoint::host(cfg, gate)`): TCP and UDP on the same port
    (`0.0.0.0`; port 0 picks one free for both, `local_port`). Each TCP
    connection runs a non-blocking tungstenite server handshake, then
    sends one binary frame, a `Signal::Offer` (frames above
    `limits.max_signal_bytes` close it). Checks in order: decodes as an
    offer (else `Refused { Malformed }`), `PROTOCOL_VERSION` and the
    fingerprint (`Version`), `max_peers` counting connecting links
    (`Full`), peer ids left (`Exhausted`: ids are `u32`, unreachable in
    practice), the `gate` (its reason), the SDP
    (`Malformed`). The offer's `a=candidate` lines whose IP is not the
    TCP peer's are dropped first (ruling T4-A): the host never sends checks
    to an address the joiner may not hold (reflection), and a joiner behind
    NAT (its private candidate dropped) still connects through the
    peer-reflexive candidate its checks reveal; str0m takes an offer with
    no candidate left. Test hooks (`EndpointConfig`'s `fingerprint_override`,
    `offer_ip_override`, `answer_addr_override`, `first_peer_id`; the first
    and last are `CoreConfig`'s too) exist only
    with the `test-hooks` feature, enabled by this crate's own
    dev-dependency (never in a shipped build). Accepted:
    `Link::answerer` with the candidate (the TCP connection's local IP, the
    UDP port; decision N16), `Signal::Answer`, the WebSocket kept until the
    link opens; any further frame drops the joiner. At most 64 connections
    wait for their offer, each `OFFER_TIMEOUT` (3 s); at most `MAX_PER_IP`
    (8) not-yet-open connections (waiting or linking) per source IP —
    more are closed as they come. A refused or finished connection gets a
    WebSocket close and 1 s to go (at most 64 linger; beyond, a refusal may
    be lost). Nothing is reported for a connection that never opens.
  - **Signalling never holds `poll`**: `SignalConn::poll` reads at most 4
    messages; a ping, pong, text or close frame closes the connection (our
    peers send none; tungstenite answers every ping). The reader takes one
    frame of at most `max_signal_bytes` (+ 14) at a time; tungstenite's
    handshake reader stops at 64 KiB.
  - **Client** (`Endpoint::connect(cfg, host)`): the TCP connect runs on
    its own thread (`connect_timeout`, 5 s), then the client handshake, the
    offer (candidate: the TCP local IP, the UDP port), the answer or
    `NetEvent::Refused`. Every `a=candidate` line of the answer is
    rewritten to the address the client dialled, IP and port (ruling F-G):
    a host behind NAT (a port forward, Docker, a cloud VM) answers with its
    private address, and its TCP and UDP share the port, so a forward of
    both on the same external port maps alike; a host cannot aim its
    joiners' checks at a third party (T4-A). A NAT'd client then reaches
    a NAT'd host: the host learns the client peer-reflexive from its checks.
    A failure before the link opens is
    `Disconnected { peer: HOST }` without `Connected`.
  - **Peers**: host side 1, 2, 3, … in the order links open, never reused
    (`PeerId` is `u32` and never goes on the wire);
    the client's peer is `HOST` (0). `Connected { addr, path }`: the
    remote end of the signalling connection, `Path::Direct`. `dtls_fingerprints(peer)`: an open
    peer's link's. A local `close` (and dropping the
    endpoint) tells the peer at once and reports nothing.
  - **Timeouts**: a link must open within `SETUP_TIMEOUT` (10 s) of its
    offer; an open link silent for `SILENCE_TIMEOUT` (10 s) or lost by ICE
    is `Disconnected { Timeout }`.
- **Limits** (`limits.rs`, spec §5.4), per peer, on what it sends us: a
  token bucket holding one second, `max_datagrams_per_s` (400) messages on
  the unreliable channel, `max_reliable_bytes_per_s` (256 KiB) reliable
  bytes, each message costing `MESSAGE_COST` (64) more. Beyond either →
  `Disconnected { Limits }`; an unreliable message above
  `rl_proto::MAX_DATAGRAM` or a reliable one above `limits::MAX_RELIABLE`
  (the larger of `MAX_CONTROL` and `MAX_LOBBY`, plus the tag byte) →
  `Malformed`. The link is closed. `allow_burst(peer, bytes)` exempts an
  expected state transfer from the byte limit (receiver side; the sender
  is not limited). Raw datagrams from the peer's address, whatever they
  carry (STUN, undecryptable DTLS, SCTP control), meet generous raw
  buckets too: `max_raw_datagrams_per_s` (10 000) and
  `max_raw_bytes_per_s` (12 MiB), about 4× a 3 MB transfer per second;
  `allow_burst` lifts them by twice the transfer (call it on the sending
  side too for states much larger than 3 MB: their SACKs count). Beyond →
  `Limits`. At most 1024 datagrams and 64 TCP connections per `poll`.
  `PeerStats` counts both ways, plus `sends_dropped` and `send_failures`
  (datagrams the local socket refused).
- **Impairment** (`impair.rs`, spec §9.4): with `cfg.impair`, every
  outgoing datagram (handshakes included) passes `rl_netcode::netsim`'s
  `LinkModel` with the profile — loss, delay with jitter, duplication —
  before `send_to`; each link's model forks from `impair_seed`. Each end
  impairs its own sending, so impairing both ends gives the profile's
  ping. SCTP retransmits what it loses.
- **LAN announcements** (`lan.rs`): `Announcer::new(target)` +
  `tick(now, &LanAnnounce)` sends one datagram at most every 2 s (send
  errors ignored); `Listener::new(port)` (0 = free port, `local_port`) and
  `poll(now)` (never blocks, at most 256 datagrams) return `LanGame`s
  (source IP + announced port, info, seen) sorted by name then address,
  dropping games unseen for 6 s. Receiving limits: undecodable datagrams
  ignored; one entry per source address, at most 4 per source IP and 64
  in all (a full list evicts the oldest entry of the IP holding most
  games, never rejects a newcomer); a source IP over 10 datagrams/s is
  ignored for 10 s (rate table keyed by IP, oldest evicted when full). std cannot set `SO_REUSEADDR`
  before `bind`, so two listeners on one machine conflict (the game opens
  one). `broadcast_targets()` is `255.255.255.255:LAN_PORT` only: directed
  per-interface broadcasts need interface enumeration (deferred). WSL2's
  default NAT networking receives no LAN broadcasts (only
  `networkingMode=mirrored`), and Windows Firewall may prompt on the
  first bind: the Direct tab is the fallback; no workaround in code.
- **Never panics on network input**: offers, answers and datagrams are
  fuzzed (`tests/fuzz.rs`, also strangers' datagrams, datagrams to a
  connecting link, empty sends); a forged datagram does not end an open
  link. Signalling garbage, oversized frames and wrong messages close the
  connection quietly (`tests/endpoint.rs`).

Test: `cargo nextest run -p rl-net` (`tests/core_wire.rs`: two `Core`s
on an in-memory wire and a virtual clock, a setup timing out, `admit`,
setup-time datagrams and a refused one in the stats, a cancelled setup's
last datagrams closing the host's open link, a full signalled pending
table evicting a silent setup and refusing `Busy` once every one has
heard checks, reflexive candidates surviving ten host addresses, the
1.5 s `Connected` fallback with a real address, `gathered` without STUN; `tests/natsim.rs`: hole punching through every NAT pair of
interest, a mapped port, the same LAN, NAT detection, STUN keeping a
mapping alive, a renumbered router, symmetric NATs through a relay, a
hole-punched link releasing its allocation, a dead relay ending the
link (kept relayed past `RELEASE_AFTER` first), gathering bounded by
`TURN_WAIT`; `tests/turn.rs`: a `TurnServer` on 127.0.0.1 with plain
sockets through `turn_client_proto` (relaying both ways, unbound and
through a channel; the per-IP limits; a full relay; binding answers;
IPv4 only), and in memory against a sans-IO client: local peers refused,
a stranger not draining a player's down budget, 16 permissions, spoofed
sources and oversized requests costing nothing, a `Core` refusing a
relay that names our base or a private address, `TurnCore` fuzzed with
requests, Send and ChannelData; only an Allocate getting an
unauthenticated answer, the amplification pinned, the relay-wide cap on
unauthenticated answers, forged requests not starving a client's signed
ones, a live client keeping its allocation, permission and channel for
32 virtual minutes past nonce expiry against a spoofer draining both
unproven budgets, oversized datagrams dropped; unit tests in `turn.rs`
(the bucket, the IP table, nonces, a stale tag proven only for its own
source, a proven source answered outside the unproven budget, an IP's
two buckets keeping its rate, a hostile nonce, any nonce on a signed
request, fuzzed) and `turn/msg.rs` (the codec, fuzzed); `tests/candidates.rs`: reading,
sanitizing (fuzzed) and classifying candidates;
`tests/link.rs`: in memory on a
virtual clock, 32 s in well under 1 s of wall time, and irregular
polling without a FORWARD-TSN/SACK storm; over localhost UDP:
connect, 3 MB reliable, 2 000 unreliable datagrams at 1 ms — 10 000, about
10 s, with `RL_FULL_TESTS` in `cargo xtask full`. `tests/endpoint.rs`, on
127.0.0.1 with port 0, every wait bounded: a host and three clients,
refusals, floods, bad signalling, closes, a reliable overflow ending the
link, 1000 reliable messages through
`bad`, the 10 s timeouts — the longest test, about 10 s — and hostile
peers: pings in a tight loop, 20 offers from one IP (the second address
is the machine's non-loopback IP, skipped with a notice without one), an
offer naming another IP, an answer naming a decoy address the client never
sends to, a client "behind NAT" connecting peer-reflexive, a host "behind
NAT" (its answer naming 192.0.2.1:9) joined by a client behind NAT or not,
a host's ids past
65 535 and exhausted,
raw junk from a connected peer's address).
Cross-build: `cargo build --release --target x86_64-pc-windows-gnu -p rl-net`.
