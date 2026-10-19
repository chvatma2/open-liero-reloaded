# Evolving the lighting model: brainstorm

**Status:** a draft for discussion, 2026-10-19. This is not a design yet:
it lays out directions, what each costs, and the questions only the user
can answer. A design follows once the user has chosen a direction.

## Where we are

The look system (plan 15a), plus the worm lighting of plan 15b, draws a lit
frame in these passes:

- **Placed and engine lights** (lamps, explosions, muzzle flashes, shot
  lights). Each light has a shadow map of 512 rays. A ray is walked pixel
  by pixel until it enters rock, dirt or another worm. The shadow edge is
  hard, anti-aliased or soft; the soft penumbra grows with the light's
  radius.
- **Glow light.** 2D global illumination with radiance cascades, over the
  whole level's grid of cells (1 world pixel on most levels, up to 8 on
  4096² ones). Only glowing pixels emit, so a glowing sky already gives
  skylight from every open direction (the night Castle Strike uses this).
  It runs every lit frame.
- **The light grid.** Ambient plus glow light plus placed lights, soaked
  into solids, blurred by the softness, capped and banded. The composite
  applies floors (the solid floor, the worm floor, self-light), the lit
  side of worms and the grade.

**What it costs** (`docs/perf/envelope.md`, plan 15a):

| GPU | Lit frame (GPU) | Notes |
|---|---|---|
| RTX 5070 Ti | 0.33–0.40 ms p50 | fine |
| Integrated AMD Radeon (laptop proxy) | 10–12 ms p50 | misses 60 Hz |

On the integrated GPU, 13.3 ms splits into glow light ≈6.5 ms, soft shadow
filter ≈3.6 ms, 20 placed lights ≈2.4 ms, and the rest ≈4.5 ms. The CPU
cost of encoding the passes adds 1–5 ms, depending on the backend. The
user kept those numbers for now; laptop performance is later work.

**What looks wrong** (the user, 2026-10-19):

- **Worm shadows** are long, sharp wedges from small point lights.
- **The moon's single direction** looks unnatural. The sky-glow version
  is better.

**What's missing:**

- **No bounce.** Light never reflects: a red-lit wall does not tint the
  tunnel beside it, and a lamp lights only what it sees directly.
- **No ambient occlusion.** Corners and tunnels are not darker than open
  air.
- **No glare.** Bright light just clips: there is no bloom and no tone
  mapping.

## What "maximum effect" could mean

Ranked by how much each changes the picture, in my estimate:

1. **Bounced, coloured light.** Lava tints the cave. An explosion briefly
   lights the whole tunnel, not only what it sees. Light fills shadows
   softly.
2. **Soft, physically plausible shadows.** Penumbrae grow with distance
   and the light's size, and a worm's shadow is short and soft near the
   ground.
3. **Glare and HDR.** Bloom on flames, lamps and explosions, with tone
   mapping instead of clipping, so a flash reads as bright.
4. **Ambient occlusion and contact darkening.** Narrow tunnels and
   corners read as deep.
5. **Terrain relief.** Rock edges catch light on the side facing a
   light. The worms already do this; the terrain does not.
6. **Atmosphere.** Light shafts through openings and a dusty haze where
   light passes through air.

## Directions

### A. Bounce in the existing glow light (temporal feedback)

When a glow-light ray hits a solid cell, it returns that cell's emission
plus its albedo × the light that reached it last frame. Bounces build up
over a few frames into many.

- **Gives:** item 1 (and fills shadows) for every light the light grid
  knows, if direct light is also written into the cells (see B).
- **Cost:** one extra texture read per ray hit, and a per-cell albedo
  texture (the art averaged per cell, made with the glow textures). It
  adds almost nothing to the 6.5 ms; it's the same passes.
- **Risk:** light settles over 3–6 frames (30–60 ms at 100 Hz). A flash
  may leave a short afterglow, which may well read as natural.
  Feedback can blow up if albedo × gain exceeds 1, so cap it.

### B. All lights through the radiance cascades (one light model)

Lamps, explosions, muzzle flashes and shot lights are drawn into the
emission grid as glowing discs of their radius. The shadow maps go.

- **Gives:** item 2 for free: soft shadows of the right width for every
  light, and worms cast soft shadows naturally (they already block the
  glow light). Together with A, every light bounces. There is one model
  to tune.
- **Cost:** removes the 3.6 ms soft filter and the 2.4 ms per-light work,
  and adds a little emission work.
- **Risk 1, small lights:** a radius-1 muzzle flash is one cell, so its
  light is faint and blocky on coarse grids. Bigger engine-light radii or
  a finer grid near the view help (C).
- **Risk 2, hard shadows:** the style needs a decision. Radiance
  cascades give soft shadows only, while levels can ask for hard
  shadows today. Keep shadow maps only for levels that want hard
  shadows?

### C. Two-scale cascades: fine near the view, coarse over the level

Today's grid covers the whole level, so big levels get coarse cells
everywhere. Instead:

- a **fine grid** of 1-pixel cells over the view plus a pad (about
  500 × 250 cells);
- a **coarse grid** over the level for far light and the sky, merged
  in as the fine grid's outer cascades.

- **Gives:** sharp, correct light where the player looks, on any level
  size; small lights work (helps B).
- **Cost:** the fine grid is about half today's cell budget, at full
  resolution. The coarse one is cheap.
- **Risk:** the light near the view's edge depends on the camera. Today
  the light is camera-independent, which a test pins; the pad has to
  hide the seam.

### D. Static and dynamic light, split and amortised

Most light is static: level lights, glow, sky, bounce. Only explosions,
shots, flashes and worms move. Static light is recomputed rarely:

- when the terrain or the glow changes, the dirty region at low priority;
- or a slice of the cascades each frame.

Only dynamic light runs every frame, at a lower cascade count or over
the view only.

- **Gives:** the integrated GPU's budget back. This is the plan 15a
  spec's own lever.
- **Risk:** Liero digs every tick, so terrain is rarely still, and dirty
  regions must stay local. Static light after a big explosion lags a
  few frames.

### E. Ambient occlusion from the distance field

The glow light already computes, for every cell, the distance to the
nearest solid (the jump flood). Darkening air by that distance gives
contact shadows in tunnels and corners.

- **Cost:** about one texture read per light-grid texel.
- **Gives:** item 4. It suits the art and is cheap. With B and A it is
  partly redundant, but still adds crispness.

### F. Glare: bloom and tone mapping

The light is kept above 1 (already up to `max_light` 2.0). Bright
pixels get a few blurred downsamples, and a filmic curve replaces the
clip.

- **Cost:** a handful of small passes. The envelope's blur passes cost
  0.03–0.1 ms each on the desktop; on the integrated GPU it's unknown but
  small.
- **Gives:** item 3; flashes and lamps read as bright.
- **Note:** this changes every lit level's look. The grade stays for
  levels to tune.

### G. Terrain relief (normals from the material)

A surface normal per rock or dirt pixel, from the gradient of the
distance to air (or of the material's soft edge). Edges facing a light
brighten, as worms do.

- **Cost:** one pass over the world target.
- **Gives:** item 5. Risk: it is noisy on dithered art; normals would
  need smoothing over a few pixels.

### H. Atmosphere (scattering in air)

Light passing through air adds a faint glow by a per-level haze density
(`look.haze`). Light shafts through openings then show even on a black
background.

- **Cost:** one term in the composite.
- **Gives:** item 6. It is cheap, but easy to overdo; it needs taste.

### Worm shadows meanwhile (small fixes)

These help whatever direction is chosen:

- engine lights with a larger radius;
- a worm's shadow fading with distance behind it (a few world pixels);
- or worms blocking only the glow light, not placed lights.

## Combinations

I would recommend phase 1, then phase 2, and phase 3 only as needed.

1. **Phase 1, cheap wins:** E (ambient occlusion), F (bloom and tone
   mapping), sky light in content (done for the night Castle Strike), and
   the worm-shadow fixes. Days of work. It changes the look a lot at
   little cost, keeps today's model, and helps the laptop not at all.
2. **Phase 2, the light model:** A + B on today's whole-level grid:
   bounce and soft shadows for everything, shadow maps gone or kept for
   hard-shadow levels. About a plan. This removes about 6 ms of shadow
   work on the integrated GPU and adds bounce.
3. **Phase 3, scale and laptops:** C (two-scale) and D (static and
   dynamic split). About a plan. It makes big levels sharp and brings the
   integrated GPU towards 60 Hz.
4. **Later, by taste:** G (relief) and H (atmosphere).

**Alternative:** phase 3 before phase 2, if laptop play matters
sooner than looks.

## Measuring

Every phase is measured with `olr-cli gpu-bench --look` on the desktop
GPU and on the integrated one, with the user's consent (the PC is
shared). The current budgets from the look system's spec:

- ≤ 0.25 ms GPU on the desktop at art scale 2;
- 4 ms on the integrated GPU to hold 60 Hz.

Plan 15a already missed both. A phase should not make them worse unless
the user accepts that.

## Questions for the user

1. **The picture.** Which game's lighting is the reference? (Noita is
   the obvious one for falling-pixel worlds with bounce and glow;
   others?)
2. **Laptops.** Must a lit level hold 60 Hz on an integrated GPU, and
   when: before 0.2.0, or later?
3. **Quality settings.** Today there are no per-player visual settings
   that change information. Would a quality setting be acceptable (fewer
   bounces, coarser grid, no bloom) if the floors still guarantee every
   worm stays visible? Or must every player see the same frame?
4. **Hard shadows.** Are they a style to keep, beside soft and bounced
   light?
5. **Latency.** Is light that settles over a few frames (bounce, and
   amortised static light) acceptable?
6. **The look changing.** Phase 1's bloom and tone mapping change every
   lit level, the goldens included. Is that fine at this stage?
