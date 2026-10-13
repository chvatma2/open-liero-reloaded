# SDD ledger — plan: docs/superpowers/plans/2026-10-13-plan-11-content-download.md
Spec: docs/superpowers/specs/2026-10-12-networking-design.md (approved; §8). The user said 2026-10-13: "Continue with plans 11, 12 and 13" (and approved deps without debug info). Execution: subagent-driven, on main, no stops between milestones.
Plan commit: b2b0c6a.

## Pre-flight scan
| Rows | Produces / consumes | Finding |
|---|---|---|
| T1→T3 | archive::{decode, MAX_ARCHIVE}, ArchiveError (From into InstallError) | consistent |
| T1→T4 | archive::encode for the worker's Archive job | consistent |
| T2→T4/T6 | ContentMsg, Item, ItemKind, CONTENT_CHUNK, MAX_ITEMS, NO_HASH | consistent; rl-proto restates MAX_ARCHIVE as MAX_ARCHIVE_BYTES (no rl-pack dep) — equality pinned by a test in rl-online (carry to T4) |
| T2→T5 | Look.skin_hash, Member.download | consistent |
| T3→T4 | DownloadCache shared Arc<Mutex<>> by Fetcher and Worker; Content::{has, pack_hash, with_packs} | Fetcher::new signature fixed to Arc<Mutex<DownloadCache>> before commit |
| T3 kind | cache uses rl_data::PackKind, wire uses rl_proto::ItemKind | mapping lives in rl-online (T4) |
| T4→T6 | Server/Fetcher/Worker, Download(State), PROMPT_BYTES, rates | consistent; T4 says the split may be adjusted by ruling |
| T5→T6 | set_download, lacking(), Waiting at starts, mid-match join only when complete | consistent |
| T6→T7 | OnlinePeer::{content, downloads, approve_downloads}, OnlineEvent::{Downloads, DownloadPrompt}, PeerContent | consistent |
| T6→T8 | PeerContent.upload_rate ← server upload_kib_s | consistent |
| T1 self | tests use MemFs::files (exists) and MemFs::with | ok |
| T2 self | compile-fix of every Look/Member constructor across crates | broad but mechanical |
| T3 self | uses tempfile (workspace dep) | ok |
| T6 self | test-hooks feature exists in rl-online (plan 10 final) | ok |
| T7 self | replay playback location to be found by implementer | ok |
| T8 self | records N29–N34 | ok |
Ruling PF11-1: rl-proto's MAX_ARCHIVE_BYTES duplicates rl-pack's MAX_ARCHIVE (layering: rl-proto may not depend on rl-pack's std) — a test in rl-online asserts equality — cost if wrong: one constant drift caught by the test.
BASE T1: b2b0c6a; implementer dispatched (sonnet)
Task 1: implementer DONE 754256b (8/8, wasm ok, skin archive ≤64 KiB); review dispatched (sonnet)
Task 1: review → Approved, spec ✅; minors: dotfile makes encode fail, MAX_ARCHIVE overshoot by one file, missing tests (length beyond data, cross-level ordering). Fix round 1 dispatched (resume).
Task 1: Ruling PF11-2: hidden files (any path component starting with '.') are not part of a pack: content_hash and archive::encode skip them, decode refuses them — otherwise a stray .DS_Store makes a pack hashable but unservable — cost if wrong: a pack relying on a dot-named file loses it in transfer (none exist; the loaders never read dot files).
Task 1: complete (commits b2b0c6a..4ac966b; fix round 1 minors-only, verified from report: 11/11 + 37 unit, check passed)
BASE T2: 4ac966b; implementer dispatched (sonnet)
Task 2: implementer DONE_WITH_CONCERNS b2e2c6f (valid_hash lowercase-only now also for setup packs; RED was compile-only); review dispatched
Task 2: review → Approved, spec ✅. Minors carried to Task 4: the MAX_ARCHIVE_BYTES == MAX_ARCHIVE test (PF11-1, doc at content.rs already claims it), Chunk.offset unbounded → receivers use checked arithmetic, rl-proto/CLAUDE.md "64 lowercase hex".
Task 2: complete (commits 4ac966b..b2e2c6f; review approved).
BASE T3: b2e2c6f; implementer dispatched (sonnet)
Task 3: implementer DONE a543391 (7/7); review dispatched
Task 3: review → Needs fixes (Important: packs renamed but not indexed are orphaned forever — open adopts, clear sweeps, save errors propagate; content-hash mismatch path untested). Fix round 1 dispatched (resume).
Task 3: fix round 1 done (6fd7965: reconcile adopts unlisted packs, clear sweeps packs/*, save errors propagate, content-hash mismatch test, minors #2 #8 #3-doc). Verified from report (8/8, check passed). Skipped minors carried to Task 4: partials not counted in the cache budget (#4) and evict's partial removal (#5) — Task 4's fetcher discards partials not in the current list when over budget.
Task 3: complete (commits b2e2c6f..6fd7965).
Task 3: Ruling PF11-3: one cache directory per process, enforced: DownloadCache::open takes an exclusive advisory lock (std File::try_lock on <dir>/lock) and fails with InstallError::Locked when another process holds it; the game then tries downloads-2..downloads-4; within one process (e.g. --net-sim's host and client) the peers share one Arc<Mutex<DownloadCache>> (PeerContent carries the handle, not a path). Done in Task 4 (rl-content part) and Tasks 6/7 (wiring) — two rl-game instances on one machine (LAN tests) otherwise race on index.ron and tmp/ — cost if wrong: a second instance downloads into another directory (duplicate bytes).
BASE T4: 6fd7965; implementer dispatched (opus)
Task 4: implementer DONE_WITH_CONCERNS 149bc61, e278931 (rulings R4-1..R4-4 in report; concerns: Content::has hashes on caller thread, worker panic leaves Verifying, lock untested on Windows); review dispatched (opus)
Task 4: review → spec ✅, Needs fixes (Important: worker panics kill the thread, items stay Verifying forever — catch_unwind). Minors fixed in round 1: load-bearing pacing/QUEUED tests, same-hash-different-size list, lone Begin/Unavailable gating, retry failed items on a new list. Accepted minors: budget overshoot by one in-flight item, adopting a partly deleted pack on reopen (documented), worker outliving its 2 s join keeps the lock. Carried to Task 6: room = QUEUED − outbox.buffered(peer); route every Done::Installed to Fetcher::finished; pre-hash local packs via Job::Hash (no UI-thread hashing).
Task 4: rulings R4-1..R4-4 (interface changes: PackArchive{bytes,sha}, Fetcher::finished, waiting_approval, split install) accepted by review.
Task 4: fix round 1 done 16d7bea; scoped re-review dispatched (sonnet). BASE T5: 16d7bea; implementer dispatched (sonnet)
Task 4: re-review → all addressed, no new breakage. Task 4: complete (commits 6fd7965..16d7bea).
Task 5: implementer DONE_WITH_CONCERNS c303885 (34/34). Carried to Task 6: clients always send Have once their list is processed (even empty), then move mid_match_join from Joined to ContentComplete. Review dispatched (sonnet).
Task 5: review → Approved, spec ✅. Minor (deferred): Waiting counts toward max_players in admit_seat, so a mid-match joiner may be Spectator instead of Waiting (ContentComplete still fires).
Task 5: complete (commits 16d7bea..c303885).
Task 6: Ruling PF11-4 (from PF11-3): PeerContent carries `cache: Result<Arc<Mutex<DownloadCache>>, String>` instead of `cache_dir` — Err(why) disables downloads and the lobby shows why; the game opens the cache once and shares it between its peers — cost if wrong: one field type.
BASE T6: c303885; implementer dispatched (opus) with carried items 1–6
Task 6: implementer DONE_WITH_CONCERNS 6930210, d48bbe9 (R6-1 client offers its skin as a one-item List; R6-2 bot skins listed by the host's own hashes, setup unchanged; host lists after required archives; unarchivable required → size 0/Unavailable; Content::known_hash/remember_hash). Interim: game/server/cli use PeerContent::without_downloads; rl-game still sends its own Have until Task 7. Review dispatched (opus).
Task 6: review → spec ❌, Needs fixes. Critical: a member's skin offer under a required pack's id (e.g. olx-classic, kind Skin) shadows the host's mod via with_packs → every start fails; Important: Waiting members still in host_start's session → dropped after 10 s silence (N33 broken). Minor #1 promoted: host must treat a member as lacking until its Have answers the current list (list revision on List/Have). Fix round 1 dispatched (resume).
Task 6: fix round 1 done 38b07ed, 43165f5, b11fa79, 6b77d7d (list revision on List/Have; rl-game's own Have and missing_packs removed early — Task 7 scope). Scoped re-review dispatched (opus).
Task 6: re-review → Critical #1, Important #1, readiness window fixed; new Important: (A) any list change (even optional-only) resets everyone's readiness — a hostile SetLook reopens it at will; (B) h.sent grows per Want for any valid hash (unbounded). Fix round 2 dispatched (resume) with minor C (rebuild per poll).
Task 6: Ruling PF11-5: a downloaded skin is laid over only when no local pack has its id (local packs win) — otherwise a member's different olx-skin-default redraws everyone's default-skin worms — cost if wrong: that member's custom skin shows as the local pack of the same id.
Task 6: fix round 2 done b67f6a2; scoped re-review 2 dispatched (sonnet). BASE T7: b67f6a2; implementer dispatched (opus).
Task 6: re-review 2 → all addressed (A, B, C, D), no new Critical/Important.
Task 6: complete (commits c303885..b67f6a2). Deferred minors (from review/report): tampered-archive retry not observed in the test, progress counts queued not delivered bytes, failed archive not retried by the host, peer_side.rs 38 KB unsplit, the Waiting-member test runs the real 10 s timeout (~12 s).
Task 7: implementer DONE_WITH_CONCERNS fe58350 (rl-ui 74/74, rl-game 36/36; new Options 'Online' tab; OnlinePeer::download_prompt(); Shell::peer_mut pub for tests; --replay while a game runs gets an empty downloads-2; pre-existing rl-ui flows.rs thumbnail flake under plain cargo test). Review dispatched (sonnet).
Task 7: review → Approved, spec ✅ (8 minors, ⚠️ prompt counts full sizes incl. partial bytes).
Task 7: complete (commits b67f6a2..fe58350).
Task 7: Ruling PF11-6: the prompt counts bytes still to download (item size − bytes already in its partial) — "missing bytes" means what the player will wait for — cost if wrong: a resumed big download skips the prompt it saw before.
Carried to Task 8: lock-free read-only cache lookup for replays (DownloadCache::lookup(dir, hash) without lock/index; check_replay_packs re-hashes anyway) so --replay and downloads-2 sessions find packs in any downloads* dir; the layout test covers the Options "Online" tab; fix the pre-existing rl-ui flows.rs thumbnail flake (shared temp dir under parallel cargo test); PF11-6 in fetch.rs.
BASE T8: fe58350; implementer dispatched (opus) with carried items 1–4
Task 8: implementer DONE_WITH_CONCERNS 018b62c (server cache + upload_kib_s, net-client --cache, real-transport partial-content client, records N29–N36, carried 1–4 done; thumbnail flake fixed 5/5).
Task 8: Ruling R8-1 (accepted): the prompt counts only items not yet approved (minus partial bytes) — otherwise an approved download's count shrinks per chunk and re-prompts — cost if wrong: successive lists each under 50 MB in one session add up past 50 MB without a prompt (each such list would download without asking on a fresh join anyway).
Task 8: review dispatched (sonnet).
Task 8: review → Approved, spec ✅. Minors (deferred): server's Locked log says "another game"; net-client killed by a signal leaks its temp cache; N35 doesn't state that successive sub-50 MB lists can add up.
Task 8: complete (commits fe58350..018b62c).
Final: whole-branch review dispatched (opus), range b2b0c6a..HEAD.
Final: review → "with fixes". Critical C1: on Windows an archive path component like "C:" makes DirFs::native's PathBuf::push escape the cache (VPath accepts ':'), so a hostile host (or a member via the skin offer) writes anywhere — DLL planting. Critical C2: the cache budget counts file bytes, not disk use; a 61 KB valid skin unpacked as 27,218 nested dirs (111.5 MB); SetLook loops let a stranger fill a server's disk.
Final: Ruling F11-A: fix pass covers C1, C2 and minors 1 (list one skin per id), 2 (eviction keeps the union of the process's lists), 3 (SetLook rate limit), 4 (Options reads the disk on a timer), 5 (refuse case-insensitive path collisions), 7 (prompt units) — one dispatch (opus), TDD per item — cost if wrong: one agent round.
Final: fix pass done — 616816f (C1: portable path rules in archive encode/decode incl. case collisions; DirFs::native accepts only Component::Normal; Windows interop check PASS), 3a6fdc6 (C2: decode Limits{bytes,disk,files,depth}, 4 KiB disk charge per file/dir in decode and dir_bytes, skins 64 KiB/16 files/depth 2/256 KiB disk, host fetches ≤1 new skin per member per 5 s, SetLook ≤4/s), db90081 (minors 1, 2 process-wide keep registry, 4, 7 MiB shown as MB). OLX importer: device-name weapon ids get "-2", non-portable asset names skipped with a warning. All 727 committed packs pass the new rules.
Final: residual — over-limit SetLook was dropped (a colour drag could lose the final look); resumed the fixer: latest-wins deferral applied from tick.
Final: minor (deferred): plain `cargo test` of rl-content occasionally reports Locked on reopen (suspected fd inheritance by a spawned mkfifo; nextest unaffected).
Final: minor (deferred): the host still fetches a second member's skin under a taken id (never listed/drawn; bounded by the 5 s limit).
Final: minor (deferred): the disk-charge limit equals MAX_ARCHIVE, so a pack near 512 MiB may fail to archive on the host.
Final: residual fixed 3dc7c7e (latest look held, applied from tick). Scoped final re-review dispatched (opus).
Final: re-review → C1, minors 1/2/4/7, look fix addressed; importer changes leave packs/goldens unchanged (726 packs scanned). Open: R1 (Important) partials unbounded in count and disk (counted by bytes held; a member cycling looks with 1-byte chunks leaves ~576 partials/min from one IP; per-fetch read_dir scans stall the poll thread); R2 (Minor) rejoining bypasses the per-member 5 s skin interval.
Final: Ruling F11-B: one more residual round (R1 + R2) — R1 is the C2 root cause still open on partials, on a server facing strangers — cost if wrong: one agent round. Partials: charged like packs, MAX_PARTIALS 32, member-skin partials discarded with their offer, in-memory partial index; skins: host-wide ≤12 member-skin fetches/min + IP-keyed interval surviving a leave.
Final: minor (deferred): the 4 KiB disk charge underestimates large-cluster file systems (exFAT); COM0/LPT0 treated as portable; HFS+ NFD names fail the hash check (fail-safe).
Final: residual round done ebc90f2 (R1: partial charge + MAX_PARTIALS 32 + in-memory partial index + member-skin partials discarded with their offer; R2: IP-keyed 5 s interval surviving a leave + host-wide 12 member-skin fetches/min; exFAT note in N29). Each RED→GREEN in final-fix-report.md; check green.
Final: complete — plan 11 closed.
