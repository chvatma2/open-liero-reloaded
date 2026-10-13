# SDD ledger — plan: docs/superpowers/plans/2026-10-13-plan-12a-finding-games.md
Spec: docs/superpowers/specs/2026-10-12-networking-design.md (approved). The user said 2026-10-13: "Continue with plans 11, 12 and 13". Execution: subagent-driven, on main, no stops between milestones.
Plan commit: 1c87555.
Ruling PL12-1: the spec's plan 12 row is split into 12a (finding games: Nostr list, invites, host-key proof, STUN address), 12b (through routers: Nostr handshakes, ICE with STUN, port mapping, TURN relays, Docker NAT tests), 12c (host migration, net-soak) — too large for one plan; each ends testable — cost if wrong: two extra plan documents.
Ruling PL12-2: event kinds 31470 (addressable announcement) and 21470 (ephemeral handshake, 12b) — unassigned in the NIP kinds table 2026-10-13; Task 2 re-checks the registry — cost if wrong: a kind change before release.
Ruling PL12-3: carried F-B (host-key proof bound to DTLS fingerprints) is Task 1, before invites use expected_host.

## Pre-flight scan
| Rows | Produces / consumes | Finding |
|---|---|---|
| T1→T7/T9/T10 | PeerRole::Client.expected_host, HostProof | consistent |
| T2→T3..T10 | Event, Filter, ToRelay/FromRelay, KIND_* | consistent |
| T3→T4,T8,T9,T10 | TestRelay (feature test-relay) | consistent |
| T4→T5,T9,T10 | Pool, PoolEvent, RelayStatus | consistent |
| T5→T9,T10 | GameSummary, Region, Announcer, GameList, Ping | GameSummary builder shared (T9 says one builder) |
| T6→T9,T10 | public_ip, StunResponder | consistent |
| T7→T8 | defaults.rs placeholders filled by T8 | consistent (T8 edits defaults.rs) |
| T7→T9,T10 | Invite, RelayHint, InviteError | consistent |
| T8 self | publishes to public relays once by hand | outward-facing, authorized by the approved spec's "chosen in plan 12 by measured behaviour"; fresh key, neutral content, 10-min expiry |
| T9 self | Visibility default Public for the server | changes server default from Lan — old configs with explicit visibility unaffected; recorded |
| T10 self | Options Online tab exists (plan 11) | extend it |
BASE T1: 1c87555; implementer dispatched (opus)
Task 1: implementer DONE 891b28a (fingerprints from str0m's verified local/remote, not SDP parsing; LobbyClient::connected(fps); ClientEvent::WrongHost → WRONG_HOST text; host proof aux = challenge nonce). Review dispatched (opus).
Task 1: review → Approved, spec ✅ (security review: proofs sound, fingerprints from str0m's verified certificate, aux reuse safe, ordering correct). Interface deviations accepted: LobbyClient::connected(fps), ClientEvent::WrongHost → OnlineEvent::Refused(WRONG_HOST).
Task 1: complete (commits 1c87555..891b28a). Carried to Task 10: a client-side join deadline in rl-online (a hostile host that never sends Challenge/HostProof/Welcome keeps a client waiting forever: keepalives reset the silence timer). Minors deferred: online test reaches only the Challenge-time refusal; swapped-fingerprint test swaps both.
BASE T2: 891b28a; implementer dispatched (sonnet)
Task 2: implementer DONE 20402af (9/9; kinds 31470/21470 free per registry-of-kinds and NIPs README; 3 real damus events as fixtures; no RED-first, mutation substitute). Review dispatched (sonnet).
Task 2: review → Approved, spec ✅. Minors: a useless assert in canonical_escapes test, fixture lacks \t\r\b\f\"\\ (escapes anchored only by hand-written expectations), no explicit huge-integer decode test (carried to Task 3), thin structural fuzz.
Task 2: complete (commits 891b28a..20402af).
BASE T3: 20402af; implementer dispatched (sonnet)
Task 3: implementer DONE_WITH_CONCERNS 690f27e (17 tests; rustls =0.23.45 aws-lc-rs, rcgen =0.14.10; relay on 127.0.0.1; set_time for NIP-40). Review dispatched (sonnet). BASE T4: 690f27e; implementer dispatched (opus).
Task 3: review → Approved, spec ✅ (NIP-01 fidelity, feature gating, bounded shutdown verified).
Task 3: complete (commits 20402af..690f27e). Carried to Task 4's fix round: test_relay tests' 300/500 ms connect timeouts (flake under load → 5 s connect, short read timeout after handshake); closed-vs-idle assertions; vacuous CLOSE check on `listener`. Minors deferred: port-reuse race in the drop test, untested id tie-break/multi-filter limit/live since, events() doc wording.
Task 4: implementer DONE_WITH_CONCERNS 62ab5ca (29 tests; 12 rulings in report incl. 5 s request-answer timeout, burst-tolerant rate limits, driver verifies events, 8 MiB inbox; webpki-roots =1.0.9; plan 13 will need a native feature). Review dispatched (opus).
Task 4: review → spec ✅, Needs fixes (Important: no liveness/ping so half-open connections stay Connected forever; slow-drip relay defeats deadlines and stop flag; a missing OK tears down healthy connections). Fix round 1 dispatched (resume) incl. Task 3 test minors. Note for Task 11: burst token buckets (5 s burst) recorded as a deviation.
Task 4: fix round 1 done 8642a37 (Timings struct + Pool::start_with; ping/idle liveness; I/O budgets; OK timeout reported not fatal; EOSE 10 s; Task 3 test minors). Concern: Windows send timeouts leave socket state 'indeterminate'. Scoped re-review dispatched (opus).
Task 4: re-review → Important 1–3, minors and Task 3 carries addressed; new Important: retried write timeouts on a blocking socket are indeterminate on Windows (partial sends duplicated/lost).
Task 4: Ruling PF12-4: the relay driver uses non-blocking sockets with explicit short waits (send retry, handshake, idle read) — WouldBlock is well-defined on every platform, SO_SNDTIMEO retries are not on Windows — cost if wrong: ~10 ms polling latency per idle relay. Fix round 2 dispatched (resume).
Task 4: fix round 2 done 0940bab (non-blocking sockets, SlowReads test relay mode, 7.7 MB intact over ws/wss; 36/36). Verified from report.
Task 4: complete (commits 690f27e..0940bab). Minor (deferred): an idle relay connection wakes every 10 ms (~100/s per relay); a longer idle wait would suit laptops.
BASE T5: 0940bab; implementer dispatched (sonnet)
Task 5: implementer DONE cf132e1 (12 tests; filter_since; tombstones; future skew 60 s; 500 cap; loopback/private addr accepted). Review dispatched (sonnet).
Task 5: review → spec ❌, Needs fixes (Important: non-global announced addresses accepted → hostile notes make clients ping/join internal addresses; `seen` full of fresh hostile keys locks out honest hosts for 3 min). Fix round 1 dispatched (resume).
Task 5: Ruling PF12-5: the game list refuses non-global announced addresses (hand-written v4/v6 ranges, v4-mapped unwrapped) unless allow_local is set (local/test relays); no port policy — a remote note must never steer clients at their own network — cost if wrong: a LAN-only relay setup must enable allow_local.
Task 5: fix round 1 done 4c20eca (PF12-5 non-global refusal + allow_local, per-range tests; seen evicts oldest unlisted; announcer gap 12 s; cheap refusals before verify; boundary tests). Verified from report (18/18, check passed).
Task 5: complete (commits 0940bab..4c20eca). Minor (deferred): closing notes recognised pre-verify by the compact text "closed":true.
BASE T6: 4c20eca; implementer dispatched (sonnet)
Task 6: implementer DONE 2ec8960 (6/6; RFC 5769 vector; StunResponder behind test-relay; IPv4 only; no clean RED). Review dispatched (sonnet). BASE T7: 2ec8960; implementer dispatched (sonnet).
Task 6: review → Approved, spec ✅.
Task 6: complete (commits 4c20eca..2ec8960). Carried to Task 8 (it probes STUN): test public_ip's from==target and wrong-transaction rejection with a forging socket; deadline check before DNS/each server; widen the <900 ms timing assertion. Minors deferred: thin fuzz past the header, IPv6 RFC 5769 §2.3 vector, Duration::MAX deadline, recv error ends a try, strict malformed XOR.
Task 7: implementer DONE_WITH_CONCERNS 20d63e6 (8/8; bech32 =0.12.0; 200-char limit enforced on decode). Concern: encode could produce undecodable codes (a 100-byte URL → ~220 chars).
Task 7: Ruling PF12-6: hints are ≤1 URL of ≤64 bytes plus ≤3 defaults, so the worst case fits 200 chars; encode never produces an undecodable code — cost if wrong: a custom relay URL over 64 bytes can't be hinted (defaults still can). Fix dispatched before review (resume).
Task 7: PF12-6 fix 6d98976 (encode truncates hints; worst case 191 chars; proptest encode_always_decodes; 11/11). Review dispatched (sonnet).
Task 7: review → Approved, spec ✅.
Task 7: complete (commits 2ec8960..6d98976). Carried to Task 8 (edits defaults.rs/invite.rs): HRP must equal "rlx" (rlx1foo1… hostile codes), a plain-bech32 checksum test, unknown Default indices are ignored (not Invalid) so old clients read new codes, a test pinning the DEFAULT_RELAYS prefix once chosen. Minors deferred: padding canonicity, URL content validation (callers parse), IPv6 silently dropped (Task 9 passes IPv4 only).
BASE T8: 6d98976; implementer dispatched (opus)
Task 8: implementer DONE 6eae19c (11/15 relays passed; defaults: damus, nos.lol, primal, snort, offchain.pub, oxtr, relay.nostr.net, nostr.bitcoiner.social; STUN: Cloudflare, Google, Twilio, Nextcloud, sipgate; carried items done). Review dispatched (sonnet). BASE T9: 6eae19c; implementer dispatched (opus).
Task 8: review → Approved (STATUS table deferred to Task 11 by ruling — Task 11 MUST copy the measured relay/STUN table from task-8-report.md into STATUS N38). Minors deferred: "fastest first" wording, a long doc line, "restricted:" treated as payment/auth, drop-listener port race, relay.nostr.band dropped on one timeout.
Task 8: complete (commits 6d98976..6eae19c).
Task 9: implementer DONE_WITH_CONCERNS 8057bac (GameSummary::of_lobby + .lan() in rl-discovery, both old builders removed; STUN for every visibility; closing note forced newer; new finding.rs; server default visibility Public). Review dispatched (sonnet). BASE T10: 8057bac; implementer dispatched (opus).
Task 9: review → Approved, spec ✅.
Task 9: complete (commits 6eae19c..8057bac). For Task 11's records: LAN counts now capped at maxima; STUN runs for every visibility (Lan/Invite/old DirectOnly configs contact public STUN at start and every 10 min). Minors deferred: /invite reply doesn't say "(no address)"; a hung DNS lookup delays the first note; a configured non-global public_address isn't warned about; the Invite not-announced test is weak.
Task 10: implementer DONE_WITH_CONCERNS 2098751 (hosting.rs Publisher shared with server, ping.rs, rl-game finding.rs; join deadline 10 s with real RED; game default visibility stays Lan; e2e fixtures updated but not run). Review dispatched (opus); controller runs cargo xtask e2e.
Task 10: controller ran cargo xtask e2e → menus.ron and online.ron passed on Xvfb (29.3 s).
Task 10: review → spec ✅, Needs fixes (Important: invalid relay/STUN entries give no reason). Fix round 1 dispatched (resume) with minors: 30 s pool linger, clear selected_game, GameList::len, Publisher doc, warn on unreachable configured address, test import, safe key slicing.
Task 10: Ruling PF12-7: rl-game takes rl-server as a dev-dependency (same layer 6, tests only; xtask layers skips dev-deps) — the end-to-end finding test needs an in-process server — cost if wrong: one test helper to move.
Task 10: Ruling PF12-8 (implementer's ruling 3, accepted): allow_local switches on automatically when every configured relay is loopback/private/localhost — an office LAN relay setup must list LAN games; the shipped defaults keep it off — cost if wrong: a user with only a LAN relay can be steered at LAN addresses by notes on that relay.
Task 10: minor (deferred): DNS for a typed hostname in Direct still resolves on the UI thread (plan 10 behaviour).
Side request (user, 2026-10-13): `cargo xtask dist` → portable zips. Approved design: target/dist/rust-liero-<ver>-{windows,linux}-x64.zip with one top folder: rl-game, rl-server (dist profile = release + strip), mingw DLLs if imported, content/packs, README/LICENSE/CHANGELOG/docs/online.md/example server.ron; both programs look for content beside the exe first (--content overrides); settings stay in the user profile (user's choice); both platforms in one run (user's choice); xtask verifies by unpacking each zip to a temp dir and running a short headless server match from there (Windows via interop). Dispatch after Task 10's fix round lands.
Task 10: fix round 1 done bedb0e1 (refusal reasons, 30 s relay linger with reuse, GameList::len, unreachable configured address said, minors). Verified from report. Task 10: complete (commits 8057bac..bedb0e1). Side task dist dispatched (opus), BASE bedb0e1.
Side request (user, 2026-10-13): while the change-weapon key is held, show the selected weapon's name above the worm (the right-hand weapon list is too far to look at). Approach: port OLX's own weapon-name-above-worm drawing if it exists (cite source at d1ad953f; keep its post-switch linger if any), at minimum shown while the key is held; presentation only. Queued after the dist task (both touch rl-game; concurrent checks would collide).
Side bug (user playtest, 2026-10-13): bots sometimes die from Mortar/Napalm explosions without visibly being hit. Phase-1 investigator dispatched (opus, worktree, no fix): separate sim bug / faithful OLX / rendering bug.
Side task dist: done bac2193, ba5eff6, c757cad, fb64dae (Ruling D-1: zips in target/release-zips, tidy sweeps target/dist builds). Review dispatched (sonnet). Weapon-name side task dispatched (opus), BASE fb64dae.
Side task dist: review → spec compliant, quality good, no Critical/Important. Minors deferred: Linux rl-game --help check unbounded; symlinked dirs in content fail loudly; exes embed the build path (no --remap-path-prefix); staging left on failure; dist fails entirely without mingw.
Side bug: root cause (hypothesis B, faithful OLX bug): VecTimeRecorder::get(0) returns the OLDEST of 100 positions (~1.1 s back); ~10% of projectile/explosion hit tests use it → phantom hits and misses (8/52 kills in 2-bot runs). Oracle confirms OLX does it (44/112 scenarios diverge without it). Second: ~25% of MW kills uncredited (exact-zero-health death next frame, spec D9).
User decisions (2026-10-13): (1) fix in play — hits test the current position in local and online play; a sim setting keeps OLX's quirk for the oracle only; re-bless match goldens; STATUS deviation, amend D7. (2) credit exact-zero-health deaths to the last damager. Implementation queued after the weapon-name task (sim change alters goldens; concurrent checks would collide).
User (2026-10-13): reaching zero health must always cause the blood explosion and play the death sound; they noticed it sometimes did not. Add to the fix task: every death path (hit, exact-zero-health next frame, any other) emits the death effects and sound; test per path; check OLX for the zero-health path.
Side task weapon name: done 9f8f491 (OLX CWorm.cpp:1304-1315: local human only, white outline font at y-30, while held or 0.75 s after a change; replaces the own name label; ticks so it pauses). Accepted from report. Hit-position + death fix dispatched (opus), BASE 9f8f491.

## Side task: hit-position fix (user request, between T10 and T11)
Hitfix: implementer DONE_WITH_CONCERNS, commits 09d5749..fc6ae29 (BASE 9f8f491). Report hitfix-report.md.
Ruling: hit tests read the newest recorded position instead of get_best with the fixed index — the fixed index alone still tested a position one step old (implementer's evidence) — cost if wrong: hits slightly differ from a corrected OLX.
Ruling: one setting `olx_quirks` gates hit position, zero-health credit and zero-health death effects — the oracle needs all three together — cost if wrong: a later split into separate flags.
Ruling (superseded by user 2026-10-13): in play, raise the 1024-entity cap (E1) rather than evict; 1024 stays only under olx_quirks — user: "We can go past the 1024 limit of effects. Unless there is a performance problem for us, let us push further." — cost if wrong: memory/perf, measured by bench/stress.
Ruling: a death sound that hits the per-sound voice limit steals the oldest voice of that sound instead of being skipped — user: "It should also play the death sound of course" — cost if wrong: an older death sound cut short.
Hitfix review (opus): approve with fixes, 0C/2I/6M (hitfix-review.md). I1 last_damager i32 grows Worm 992→1000 B = whole bench regression; I2 death blood still on cosmetic quota.
Ruling: play entity cap 16384 allocated once at match start, ~1024 slots reserved for death gibs/blood, 1024 only under olx_quirks; rl-sim rng MAX_ENTITIES untouched (emulated counter) — reviewer's measured envelope peak ~9.9k uncapped for 16-bot chaos — cost if wrong: memory/present time, re-measured by envelope/gpu-bench.
Ruling: fix pass also takes minors M1–M6 — standing user instruction "feel free to address the minors" — cost if wrong: a slightly larger diff.
Queued (user request 2026-10-13, after the hitfix fix pass, before T11): choose the followed worm in any watched game (replay, --spectate, online spectator) with Left/Right arrows; PageUp/PageDown speed only; a replay starts on the recorded human if any; HUD follows the chosen worm. No sim change.
Hitfix fix pass: DONE_WITH_CONCERNS, commits fc6ae29..459fd36 (hitfix-fix-report.md); full green on second run. Play cap 16384 kept (envelope peak 7572; heaviest GPU scene +~1 ms CPU/frame, still 144 Hz).
Deferred: bench gate near noise edge on this host (base 4.18→4.97 µs between runs); Worm::spawned #[inline(never)] is a code-layout fix; slower machines unmeasured; one unexplained flaky `check` failure after the M6 bless; envelope results still describe the 1024 pool (bless after the replay-camera task).
Hitfix re-review: approve with one fix. All addressed except M1 partial → N1 Important (erased predicted death suppresses the same life's real death; reset keeps entries). Minors N2 (stolen voice before failed start → silence), N3 (phantom control may pick unrelated kill), N4 (bench headroom on layout fix + upward re-bless).
Queued after the replay-follow task: resume the fix-pass implementer for N1 (tick-windowed dedupe within MAX_PREDICTION, prune in confirmed/reset, erased-then-real test) plus N2, N3.
Replay-follow: DONE_WITH_CONCERNS, f76e7a8 (replay-follow-report.md). Arrows follow in any watched game; PageUp/Down speed only; minimap big dot + hints follow; weapon name stays local-only.
Ruling: record the recording peer's local slot in the replay setup within FORMAT_VERSION 2 (no bump) so a replay starts on its human — v2 is unreleased (introduced 09d5749 today; v0.1.0 zips wrote v1, already refused) and no .rlrp is committed; online replays put the human at any slot — cost if wrong: v2 files written between 09d5749 and this change are refused (dev-only).
Final: minor (deferred): N4 bench headroom rests on Worm::spawned #[inline(never)] layout effect and an upward bench re-bless.
Hitfix N1-N3: DONE_WITH_CONCERNS, ba5d284..6aa5828. full green except bench (tick_32_worms 5.13 µs, +12%, host noise; bench alone passed +7.7%).
Final: minor (deferred): full's bench gate flaky on this host (tick_32_worms ±12% run to run) — wants a noise-robust gate (interleaved base, or wider threshold).
Final: minor (deferred): N1 window — an erased predicted death followed by a real death of the same life within 28 ticks shows once.
Viewer slot: DONE 72d751d (ReplaySetup::viewer in v2; same_match helper for peer replays).
Envelope re-blessed (8754b3f) and summary rewritten (23ad14f): heavy replay tick+frame p99 at 1440p WSL 2.95→4.52, DX12 2.34→3.42, Vulkan 2.18→2.92 ms; 144 Hz holds everywhere.
Flaky test fixed (controller): rl-online a_host_and_two_clients_play_a_short_match asserted viewers [0,1,2] while client slots follow welcome order — now compares with each peer's started slot (commit after 23ad14f). No deterministic RED (a race); 5/5 isolated runs + check green.
Final: minor (deferred): envelope's worms/pool/map/art sweeps still run with effects capped at 1024 (REF_FX); only fx sweep and GPU tables reflect play's 16384.
Task 11: complete (commit 91c44c0, docs: plan 12a's records; check green). report task-11-report.md.
Ruling: Task 11 (docs only) gets no separate task review; the final whole-branch review covers it explicitly — a docs-only diff, and the final reviewer reads it against the code anyway — cost if wrong: a doc error caught later.
Final review dispatched (opus ×2, read-only): A = rl-discovery + probes (final-A.txt → final-review-A.md); B = integration + docs + weapon-name + replay-follow (final-B.txt → final-review-B.md). dist and hitfix excluded (reviewed separately).
Final review A: ready with fixes, 0C/2I/9M (final-review-A.md). I1 closed game relisted from a later relay's older open note; I2 Publisher wall clock read once + Instant → notes born expired after suspend/NTP step.
Final review B: ready with fixes, 0C/4I/11M (final-review-B.md). I1 password proof relayable (not bound to key/fingerprints); I2 relay/STUN/block edits ignored while pool reused; I3 Invite-only host never told its code has no address; I4 arrows don't follow in command-line --spectate/--replay.
Re-grade: all six Important stand (A-I1, A-I2, B-I1..I4). Minors: user's standing "feel free to address the minors" → fix pass takes every minor that needs no design change; the rest get rulings.
Final fix pass dispatched (opus), BASE = HEAD above.
User 2026-10-14: build release zips (cargo xtask dist) after 12a lands and tell the user for a two-PC home playtest.
User 2026-10-14 (revised): no zips after 12a; build them after plan 12b lands for one combined network playtest.
Final fix pass: DONE_WITH_CONCERNS, commits 91c44c0..99d221d (final-fix-report.md). All six Important fixed with RED→GREEN tests (A-I2, A-M6, B-M9, B-I4 RED were compile errors after API changes); minors fixed except rulings FR-2..FR-4.
Ruling FR-1: bind the password proof to the joiner's key and both DTLS fingerprints AND send it in Prove after HostProof verifies — binding stops relaying, moving stops offline guessing by an unproven host; together neither works — cost if wrong: one more field in Prove (protocol 2 unreleased).
Ruling FR-2: a full game list (500) keeps refusing new keys rather than evicting — a flood of fresh keys defeats any eviction order — cost if wrong: honest hosts hidden during a flood (NIP-13 proof of work is a later option).
Ruling FR-3: offer() still re-verifies signatures the relay driver verified — bounded by the pool's rate limits (~0.1 s per hostile burst) and GameList stays safe for callers without a pool — cost if wrong: a UI hitch under a hostile relay.
Ruling FR-4: the Internet list is still laid out in full each frame (≤500 rows); it is only rebuilt when the list changes — virtualising needs fixed column widths — cost if wrong: slow frames under a full hostile list.
Ruling FR-5: unblocking a host shows its game only with its next note (≤1 min) — the list never kept a blocked host's notes — cost if wrong: a short wait after unblocking.
Final: minor (deferred): UNPROVEN_HOST and the Windows STUN reset are pinned by unit tests of the helpers only (no in-process MITM / ICMP reset).
Final: minor (deferred): STUN and public-address edits apply at the next hosting (Options is reachable only from a paused match).
Final: controller verified cargo xtask check green at 99d221d; full green except the known bench noise (tick_32_worms +14 %, rl-sim untouched; bench alone passed twice).
Plan 12a: complete.
