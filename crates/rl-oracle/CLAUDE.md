# rl-oracle

The fidelity oracle (spec §6): scenario files, traces recorded from a patched
OpenLieroX, and first-divergence comparison.

- **Layer 2.** No simulation code. In M3 the simulation turns a `Scenario`
  into a `Trace`, and `compare` checks it against the recording.
- **Scenarios** (`fixtures/oracle/<name>.ron`): level, mod, seed, frame count,
  worm spawns, key changes (`"right+jump"`), and optionally weapon slots by
  name, health, and bonuses placed at given frames.
  - Each worm holds its keys until its next change.
  - `Scenario::schedule()` adds a frame-0 entry for every worm, so OLX never runs bot AI.
- **Traces** (`fixtures/oracle/<name>.jsonl.gz`): a header line with every OLX
  feature value (v2 also: weapon and projectile names, options, seed), then
  frame −1 (the state when recording started) and frames 0..n.
  - Fields added in M4 (v2) are optional on parse, so v1 traces still load.
  - Times are integer ms since frame −1 (`tools/olx-oracle/CLAUDE.md`).
  - Floats are OLX's `%.9g`, which round-trips `f32` exactly.
  - `Num` compares bitwise (`-0.0 != 0.0`); all NaNs are equal.
- **Comparison is bit-exact.** A `Tolerance` needs a written `reason` in the scenario file.
  - `compare_fields` checks only some `Fields` groups (what the simulation
    reproduces so far); `compare` checks all of them.
- **Key masks** match `OracleHooks.h` in `tools/olx-oracle/oracle-hooks.patch`.
  Change both together.

Test: `cargo nextest run -p rl-oracle`. Recording needs Docker: `cargo xtask oracle-record`.
