# olr-oracle

The fidelity oracle (spec §6): scenario files, traces recorded from a patched
OpenLieroX, and first-divergence comparison.

- **Layer 2.** No simulation code. In M3 the simulation turns a `Scenario`
  into a `Trace`, and `compare` checks it against the recording.
- **Scenarios** (`fixtures/oracle/<name>.ron`): level, mod, seed, frame count,
  worm spawns, key changes (`"right+jump"`), and optionally weapon slots by
  name (exactly `WEAPON_SLOTS` = 5 per worm), health (> 0), bonuses placed
  at given frames, and the `mode` the simulation replays it with
  (`ScenarioMode`: `Oracle` by default, or `Deathmatch`, whose match can
  end; OLX always plays its own Deathmatch).
  - Names may contain `/`: `weapons/<mod>/<weapon>.ron` holds one generated
    scenario per weapon (`cargo xtask oracle-gen-weapons`; do not edit them).
  - Each worm holds its keys until its next change.
  - `Scenario::schedule()` adds a frame-0 entry for every worm, so OLX never runs bot AI.
- **Traces** (`fixtures/oracle/<name>.jsonl.gz`): a header line with every OLX
  feature value (v2 also: weapon and projectile names, options, seed), then
  frame −1 (the state when recording started) and frames 0..n.
  - Fields added in M4 (v2) are optional on parse, so v1 traces still load.
  - `go` (plan 6) is written only from the frame the match ended in; the
    `GAME_OVER` group compares it (`null` before).
  - Times are integer ms since frame −1 (`tools/olx-oracle/CLAUDE.md`).
  - Floats are OLX's `%.9g`, which round-trips `f32` exactly.
  - `Num` compares bitwise (`-0.0 != 0.0`); all NaNs are equal.
- **Comparison is bit-exact.** A `Tolerance` needs a written `reason` in the scenario file.
  - `compare_fields` checks only some `Fields` groups (what the simulation
    reproduces so far); `compare` checks all of them.
- **Key masks** match `OracleHooks.h` in `tools/olx-oracle/oracle-hooks.patch`.
  Change both together. `Keys` is a `u16`: the classic 8, then `strafe`,
  `prev`, `next` (issue #23), then `weapon1` to `weapon5` (issue #48).
  `fixtures/oracle-pending/` holds scenarios
  waiting for their trace (no Docker when written; its README, and
  `docs/deferred-verification.md`); `pending_scenarios_parse` only parses them.

`tests/fixtures.rs` checks that every committed trace is complete and shows
what its scenario's description claims.

Test: `cargo nextest run -p olr-oracle`. Recording needs Docker: `cargo xtask oracle-record`.
