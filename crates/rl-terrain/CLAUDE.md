# rl-terrain

The simulation's terrain (spec §5.4): one `Material` byte per world pixel.

- **Layer 1, `#![no_std]` + `alloc`.**
- `flags(x, y)` is OLX's `GetPixelFlag`: rock outside the map.
- `carve_hole(stamp, x, y)` ports `CMap::CarveHole` for the material grid,
  including OLX's edge-clipping quirk: the stamp is read from its top-left
  corner even when its left or top part is clipped.
- `place_dirt(stamp, x, y)` and `place_green_dirt(mask, x, y, rand)` port
  `CMap::PlaceDirt` and `CMap::PlaceGreenDirt`. Unlike `carve_hole`, they
  offset the stamp by the clipped amount. `place_dirt` counts a background cell
  under a non-magenta colour twice, as OLX does. `place_green_dirt` calls
  `rand` once for each green pixel it places (`GreenMask`, from `greenball.png`).
- Positions are C `int`s: no position, however far outside, may panic.
- `material_hash()` is the oracle's `mh`: FNV-1a 64 over the material bytes.
- Colour art is not here. Rendering (M5) applies the same stamps to the images.

Test: `cargo nextest run -p rl-terrain`.
