# rl-terrain

The simulation's terrain (spec §5.4): one `Material` byte per world pixel.

- **Layer 1, `#![no_std]` + `alloc`.**
- `flags(x, y)` is OLX's `GetPixelFlag`: rock outside the map.
- `carve_hole(stamp, x, y)` ports `CMap::CarveHole` for the material grid,
  including OLX's edge-clipping quirk: the stamp is read from its top-left
  corner even when its left or top part is clipped.
- `material_hash()` is the oracle's `mh`: FNV-1a 64 over the material bytes.
- Colour art is not here. Rendering (M5) applies the same stamps to the images.

Test: `cargo nextest run -p rl-terrain`.
