# rl-content

Loads the packs in a content directory (`target/content`, written by
`rl-cli import`) into ready-to-run inputs: `Content::open(root)` lists the
packs and loads the core pack `olx-core` once; then `load_mod` (`SimMod` +
decoded images + sound bytes keyed by `SimMod::sounds()` names, `olx:` game
sounds from the core pack), `load_level`, `load_theme` (holes, green ball,
tiles; `stamps()`, `green_mask()`), `load_skin`/`skin_or_default`, `core()`
(sprites by name, game sounds, `colours.cfg`) and `new_world(&SandboxSetup)`.

- **Layer 4, uses `std`.** Depends on `rl-sim`, `rl-pack`, `rl-terrain`,
  `rl-data`, `rl-vfs`, `anyhow`. `rl-scenario` (same layer) uses it.
- **Never panic on content** (spec §19, Review Focus 3): a missing or odd
  asset gets a placeholder (`PLACEHOLDER` colour; core sprites at their
  `CORE_SPRITES` size, so frame arithmetic still works; a 672 × 18 skin;
  silence for sounds) and a warning. Warnings are returned
  (`Content::warnings`, `LoadedMod::warnings`, ...), never printed: the
  caller logs each once. A missing pack or unreadable definition is an `Err`.
- **Two mod loaders.** `sim_mod(id)` is strict, as OLX's client (what the
  oracle replays): an unreadable PNG or an image projectile without its
  frame count is an error (`check_frames`). `load_mod(id)` gives such a
  projectile a one-frame placeholder. On good content both give the same
  `SimMod` (`tests/content.rs`). Frame counts are `width / height` of the
  pack's PNGs (spec §18 D3).
- **Worlds.** `new_world` spawns worms in order; a worm without a position
  draws one from the world's `FindSpot` (`World::find_spot`), so the same
  setup gives the same world. Worms get the mod's first five weapons unless
  the spec names them. The clock starts at `SANDBOX_START`.
- Pack ids of OLX imports: `olx_mod_id("MW 1.0")`, `olx_level_id("CastleStrike.lxl")`.

Test: `cargo nextest run -p rl-content` (`loads_every_manifest_item` and the
real-level part of `new_world_spawns_worms_on_free_spots` need `RL_CONTENT_DIR`).
