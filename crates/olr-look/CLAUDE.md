# olr-look

The look's runtime data (spec `docs/superpowers/specs/2026-10-18-look-system-design.md`,
plan 15a): what a level's and a mod's look compile to, animation sampling,
the per-frame `LookFrame` the renderer's look passes read, and the engine's
own look. The simulation never sees it.

- **Layer 1, `#![no_std]` + `alloc`, wasm.** Depends only on `libm`.
- **Types** (`lib.rs`): `Light` (colour, intensity percent, `Reach`,
  `Animation`, `phase_ms`), `GlowGroup`, `PlacedLight`, `LevelLook`,
  `ProjectileLook`, `WeaponLook`, `ModLook` (indexed like `SimMod`), `Grade`,
  `Ambient`. `olr-format` compiles the authoring files into them.
- **Animation** (`anim.rs`): `Animation::sample(t_ms, phase_ms, identity)`
  gives a scale (1.0 = 100 %) and maybe a colour. `Pulse` starts at its
  midpoint rising; `Flicker` is smooth value noise over buckets of
  `1000 / speed` ms seeded by the identity; `Keys` are linear, and a
  segment's colour is interpolated when both keys have one, else the
  earlier key's. A clock that is not finite samples as `Steady`. A curve
  that does not loop `ended` past its last key. The default phase
  (`default_phase`, a hash of the identity) applies only to looping
  animations: a flash starts at its start.
- **`LookFrame`** (`frame.rs`): `begin(level, t_ms)` fills ambient, grade,
  settings, groups (`FrameGroup`: cast colour, own colour, self-light scale,
  reach) and the level's lights; `push` adds the dynamic lights in plan 15a
  R9's order; `cull(view)` keeps those whose reach plus radius touches the
  view, then the first `MAX_LIGHTS` (256). Colours are linear
  (`srgb_to_linear`). `fade(d, reach)` is `(1 - (d/r)²)²`.
- **Identities:** a group's is `identity_of(id)` (FNV-1a 64), a level
  light's its index.
- **Engine** (`engine.rs`, R13): `FLOOR` (50 %), `explosion_light(size)`,
  `muzzle_light()`.

Test: `cargo nextest run -p olr-look`.
