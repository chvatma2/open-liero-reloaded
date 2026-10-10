# rl-scenario

Plays oracle scenarios (`fixtures/oracle/**/*.ron`, named by their path such as
`weapons/classic/bazooka`) in `rl-sim` and records
traces in `rl-oracle`'s format.

- **Layer 4, uses `std`.**
- The scenario's world comes from the imported packs in a content directory
  (`target/content`, written by `rl-cli import`) plus OLX's settings from the
  trace header. The mod pack becomes a `SimMod` (`load_mod`); its weapon and
  projectile names must equal the header's, so ids compare directly.
- A scenario's weapons are set by name and its health applied; without
  weapons, and for lives and kills, the slots are copied from OLX's frame −1.
- The world starts at the header's `T0` (OLX's restarted `simulationTime`) and is
  seeded with the header's seed; frames record `rc`, `mt` and `ents`, and `st`
  steps by the world's frame time (`World::frame_dt`, GameSpeed-scaled). A
  header with client options `rl-sim` does not model (`Game.Blood` other than 100) is refused.
- `tests/oracle.rs` is the acceptance test: every scenario must match bit for
  bit in the fields `CHECKED` lists (every group since bonuses), except those in `EXPECTED_DIVERGENCES`,
  which must first diverge exactly at the listed frame and field (so a
  regression in ported fields cannot hide there). A second pass does the same
  without `rc`, `mt` and `ents` (`EXPECTED_DIVERGENCES_WITHOUT_DRAWS`), so a
  regression in the frame of a pinned draw divergence shows up too. Rows
  owned by a deviation (D9) stay after M4; only those are left.
  On a mismatch the test prints the new row.
- The theme pack's green ball becomes the world's green-dirt mask.
- A scenario's bonuses are placed with `World::place_bonus` before the
  frame's tick (weapons by name, ignoring case, as OLX's `FindWeaponId`).
- `load_mod` refuses an image projectile without its image's frame count.
  `load_map` reads a level pack and its theme (holes, green-dirt mask).
- `tests/smoke.rs` plays 4-worm matches with random seeded keys on every
  level with both mods (3000 ticks) and checks the invariants every tick,
  and that a clone gives the same checksums and events; `tests/no_alloc.rs`
  fires every Classic weapon without allocating. Both skip without content.
- For debugging a divergence: `rl-cli scenario <name> --dump <frame>` (`--all`
  compares every field, `--fields checked+projectiles+proj_hidden+clocks` the
  named groups).

Test: `cargo nextest run -p rl-scenario` (with `RL_CONTENT_DIR`; `cargo xtask check` sets it).
