# rl-net

Real transport for online play (networking spec §2.1, §5.1): WebRTC data
channels over the sans-IO str0m stack, signalling over a WebSocket, LAN
announcements.

- **Layer 5, std; not wasm.** Depends on rl-netcode (`Channel`) and str0m.
  Real clocks and sockets live only in this crate's socket driver (and the
  binaries); `link` itself is sans-IO.
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
  strings. Host candidates are the `local` UDP addresses (decision N16).
  Time is `std::time::Instant`, always an argument (the netcode's `now_us`
  is converted by the endpoint). `accepts` demultiplexes a shared socket;
  `receive` takes a datagram (non-WebRTC bytes are ignored); `poll` feeds
  the time, flushes queued sends and hands out `Transmit`s, `LinkEvent`s
  (`Open` once both channels are open, `Message`, `Closed` once; nothing
  after a local `close`) and the next timeout.
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
  maximum).
- **Buffering**: str0m 0.24.0 refuses a write that does not fit its 128 KiB
  SCTP send buffer (shared by the streams), so `Link` queues messages per
  channel and moves them into str0m as room appears: inputs first, control
  leaving 16 KiB free for inputs. `buffered(channel)` = the queue plus
  str0m's buffered amount; big transfers keep it bounded (chunks of 32 KB,
  under 256 KB queued).
- **str0m's contract** — every mutation of the `Rtc` (input, write, SDP or
  direct API change, candidate) is followed by draining `poll_output` until
  it returns a timeout, before the next mutation — is kept inside `Link`:
  every method that mutates drains into the link's own buffers. Keep it so
  when adding methods.
- **Never panics on network input**: offers, answers and datagrams are
  fuzzed (`tests/fuzz.rs`); a forged datagram does not end an open link.

Test: `cargo nextest run -p rl-net` (`tests/link.rs`: in memory on a
virtual clock, 32 s in well under 1 s of wall time, and irregular
polling without a FORWARD-TSN/SACK storm; over localhost UDP:
connect, 3 MB reliable, 2 000 unreliable datagrams at 1 ms — 10 000, about
10 s, with `RL_FULL_TESTS` in `cargo xtask full`).
Cross-build: `cargo build --release --target x86_64-pc-windows-gnu -p rl-net`.
