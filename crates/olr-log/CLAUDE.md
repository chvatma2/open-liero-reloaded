# olr-log

The game's run log (issue #2): one file per run of `olr` in
`<data>/logs/`, mirrored to the terminal, with crash capture. Read by the
maintainer and attached to players' bug reports, so what `info` holds is
all a stranger's report will ever carry.

- **Layer 5, uses `std`.** Depends on `log` 0.4.34 (the facade; other
  crates log through it with no dependency on this one: `olr-audio`) and
  `time` 0.3.55 (`local-offset`, only for the local UTC offset). Used by
  `olr-game` (the logger) and `olr-server` (`panic_report` for its own
  `server.log`). Simulation crates never log (`no_std`, deterministic).
- **Files** (`run.rs`): `RunLog::open(dir, clock, pid, home, cap, keep)`
  makes `<YYYY-MM-DD_HH-MM-SS.mmm>Z_<pid>.log` (the start in UTC: the
  newest sorts last) with `create_new`, and `<stem>.lock` held with
  `File::lock` until the process ends (`is_live` tries the lock: held,
  or not checkable, is live). Then `previous_unclean` (the newest other
  log not live; its last line lacks `CLEAN_END`: it gets an
  `UNCLEAN_NOTED` line, so it is reported once) and `prune` (all but the
  newest `KEEP` = 10 runs, live ones skipped, their lock files with them;
  only names starting with a digit and ending `.log`).
- **Cap:** `MAX_BYTES` = 2 MiB a file. Past it one `warn log:` line says
  so; then only errors get in (`ERROR_ALLOWANCE`, 256 KiB more), and the
  end line always does.
- **Writes** are buffered (64 KiB `BufWriter`): flushed on `warn` and
  `error`, at most a second after a line otherwise, on a panic and at
  `end()`. Nothing per frame is logged at `info` or above by the game.
- **Lines** (`format.rs`): `YYYY-MM-DD HH:MM:SS.mmm <level> <source>:
  <message>`, the local time from the `Clock` (`SystemClock` reads the
  offset once at its making: `time` refuses it once the process has
  threads on Unix), levels `error`, `warn `, `info `, `debug` (padded),
  the source the record's target with `olr_game::` dropped; a message's
  further lines indented four spaces.
- **Redaction** (`redact.rs`, file only): `home` shows the home
  directory as `~` (a whole directory: not `/home/ann` in `/home/anna`);
  `network`, at `info` and above, replaces IPv4 and IPv6 addresses
  (`<ip>`, a port after it kept), 64-hex-digit keys and `npub1`, `nsec1`,
  `nprofile1` words (`<key>`) and `olr1` invite codes (`<invite>`). A token
  starts after a byte that is no word byte nor a dot, so module paths,
  versions and times stay. `debug` lines keep them (the maintainer's own
  runs).
- **The logger** (`lib.rs`): `init(Options { dir, level, home, header },
  clock)` once: opens the run (an unwritable directory: the terminal
  alone and one warning), writes the header lines to the file only,
  installs the `log` logger and the panic hook, then warns of a previous
  run that ended unexpectedly. Records of targets `olr`, `olr_*` pass at
  the run's level and are mirrored to the terminal as the game printed
  before (`terminal_line`: `info` → stdout `olr: …`; `warn` →
  `warning: …`, `error` → `error: …`, `debug` → `debug: …` on stderr;
  `xtask e2e` fails on stderr lines starting `warning:`/`error:`). Other
  crates' records (wgpu, winit) reach the file from `warn` and never the
  terminal. `to_file` writes a line to the file alone; `end()` writes
  `CLEAN_END` and closes the file (later records reach the terminal
  only). `parse_level`: `error`, `warn`, `info`, `debug`.
- **Panics:** the hook writes `panic_report` (`thread '<name>' panicked
  at <file:line:col>: <message>` and a forced backtrace, at most 80 lines)
  as an `error` from source `panic`, to the file only, flushed, then runs
  the previous hook (the terminal's message). It waits at most 200 ms for
  the file's lock (a panic inside the logger never hangs). Caught panics
  are recorded too.

Test: `cargo nextest run -p olr-log` (`tests/log_files.rs` on a temporary
directory and a `TestClock`; `tests/panic_hook.rs` is its own binary:
the facade and the hook are process-global).
