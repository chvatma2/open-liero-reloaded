# rl-scenario

Plays oracle scenarios (`fixtures/oracle/**/*.ron`, named by their path such as
`weapons/classic/bazooka`) in `rl-sim` and records
traces in `rl-oracle`'s format.

- **Layer 4, uses `std`.** Depends on `rl-content` (same layer).
- The scenario's world comes from the imported packs in a content directory
  (`content/packs`, written by `rl-cli import`, opened with
  `rl_content::Content`) plus OLX's settings from the trace header. The mod
  pack becomes a `SimMod` with `Content::sim_mod` (strict: an image
  projectile without its image's frame count is an error); its weapon and
  projectile names must equal the header's, so ids compare directly.
- A scenario's weapons are set by name and its health applied; without
  weapons, and for lives and kills, the slots are copied from OLX's frame −1.
  The world is a `MatchSetup` played with `rl_sim::OracleMode` (OLX's
  crediting, no end: OLX's oracle server allows empty games with unlimited
  lives, and those scenarios reach neither limit), or, for a scenario with
  `mode: Deathmatch` (`dm-kill-limit`), `rl_modes::Deathmatch` with the
  header's rules; `replay` records the frame and winner of the end as `go`.
- The world starts at the header's `T0` (OLX's restarted `simulationTime`) and is
  seeded with the header's seed; frames record `rc`, `mt` and `ents`, and `st`
  steps by the world's frame time (`World::frame_dt`, GameSpeed-scaled). A
  header with client options `rl-sim` does not model (`Game.Blood` other than 100) is refused.
- `tests/oracle.rs` is the acceptance test: `CHECKED == Fields::ALL`, and
  every scenario must match bit for bit in all of it, except the three D9 rows
  of `EXPECTED_DIVERGENCES` (`weapons/mw-1-0/{assassin-s-shot,flak,plasma-launcher}`),
  which must first diverge exactly at the listed frame and field (so a
  regression elsewhere cannot hide there). Each row's reason says why
  bit-exactness is impossible without porting Gusanos particles. A second
  pass does the same without `rc`, `mt` and `ents`
  (`EXPECTED_DIVERGENCES_WITHOUT_DRAWS`), so a regression in the frame of a
  pinned draw divergence shows up too. A new row needs a deviation in
  `docs/porting/STATUS.md`. On a mismatch the test prints the new row.
- The theme pack's green ball becomes the world's green-dirt mask.
- Natural bonuses hold the weapons the header's `WeaponRestrictionsFile`
  does not ban, in the mod pack's presets (`Content::weapon_rules`; OLX's
  default for a header without it).
- A scenario's bonuses are placed with `World::place_bonus` before the
  frame's tick (weapons by name, ignoring case, as OLX's `FindWeaponId`).
- `tests/smoke.rs` plays 4-worm matches with random seeded keys on every
  level with both mods (3000 ticks) and checks the invariants every tick,
  and that a clone gives the same checksums and events; `tests/no_alloc.rs`
  fires every Classic weapon without allocating. Both skip without content.
- `play(content, sc, expected, each)` runs a scenario and hands each frame's
  world and events to `each` (`replay` records traces with it);
  `color_after` is the colour terrain after a scenario. `tests/render_golden.rs`
  compares it with `fixtures/render/*.png` (CPU goldens, in `check`); with
  `RL_BLESS_RENDER` set it writes them instead, which is what
  `cargo xtask bless-render --reason "<why>"` does (it logs to
  `fixtures/blessings.log`). Never edit a golden by hand. The GPU frame
  goldens (`fixtures/render/frame-*.png`) are `rl-cli render`'s, tested in
  `rl-cli/tests/render_golden.rs` (see `rl-render`'s CLAUDE.md). All goldens
  are drawn from the imported OLX content at test time (scenario, level,
  art scale from the packs in `RL_CONTENT_DIR`); the PNGs are accepted
  fixtures, the packs are never committed.
- For debugging a divergence: `rl-cli scenario <name> --dump <frame>` (`--all`
  compares every field, `--fields checked+projectiles+proj_hidden+clocks` the
  named groups).

Test: `cargo nextest run -p rl-scenario` (with `RL_CONTENT_DIR`; `cargo xtask check` sets it).
