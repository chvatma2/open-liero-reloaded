# Measuring graphics (issue #34)

Three tools for judging the renderer's cost and proving an optimisation
leaves the picture alone. The standing rule holds: graphics performance
is judged on Windows at 1080p with a representative benchmark (the
envelope, `envelope.md`); these tools find where the time goes and keep a
change honest, on any machine.

## Per-pass GPU times: `olr-cli gpu-bench`

With timestamp queries (`Offscreen` with `timestamps`, which `gpu-bench`
asks for), every pass of a frame writes its own pair of timestamps and is
counted in one of twelve groups, in the GPU's order:

| Group | Passes |
|---|---|
| world | each view's world pass (albedo, and lit the shade) and occluder map |
| emission | the glow light's occluder map, play's discs, the emission |
| jump flood | the flood's seed and its passes |
| cascades | the radiance cascades, top down |
| fluence | the fluence (and its clear on a fresh start) |
| occlusion | the ambient occlusion's rows (only a level with occlusion) |
| light sum | the light grid's sum |
| soak | the soak, across and down |
| blur | the softness blur, across and down (toned, banded) |
| bloom | the bloom, across and down |
| composite | the composite |
| upscale | the upscale into the output |

`gpu-bench` prints, per size, each group's p50 and p99 over the measured
frames, then their p50s' sum against the frame's: the rest is copies (the
occluder maps' copy; the glow light's history copy until issue #35) and gaps between
passes. Without timestamp queries it says once that they were not
measured. The report (`--out`) holds them as `SizeReport::passes`.

The lit frame on the representative scene:

```
olr-cli stress --mod olx-classic --level olr-level-castlestrike-reloaded \
    --worms 8 --seconds 30 --record cs.olrp
olr-cli gpu-bench --replay cs.olrp --look --sizes 1920x1080 --out cs.ron
```

(`--adapter integrated` for the laptop proxy.) For orientation only, the
software adapter (llvmpipe, 2026-10-22, `--adapter software --look`, an
8-bot chaos match on Castle Strike Reloaded, 1080p, 292 ms a frame):
cascades 211 ms, jump flood 37, soak 12, emission 6, bloom 6, world 3,
composite 2.5, blur 2.4, fluence 2.4, upscale 2.2, light sum 1.3; 7.6 ms
of copies and gaps. A CPU rasteriser weighs passes otherwise than a GPU
(bandwidth, caches, parallelism), so rank the next issue's work by the
Windows numbers (`2026-10-0.2-windows-1080p.md`), not these.
Issue #35's optimisations and the bigger ideas they left are in
`2026-10-graphics-candidates.md` (lavapipe per pass before and after).

## The lit frame in `bench-quick`

`cargo xtask bench-quick` now also draws a lit Castle Strike Reloaded
frame (`olr_cli::quick_bench::warm_lit`: 8 chaos bots in Classic after
2000 ticks; the level's look, its flames' and sky's glow groups, a fight's
glowing discs, 32 of them) at 1080p on the software adapter, after a
first frame that makes the targets:

- `lit_frame_instructions`: the fewest instructions, of 5 frames, of the
  uploads and `render_lit`'s encoding and submit (about 1.36 M here,
  steady to 0.1 %), gated at +15 % like the other instruction counts;
- `lit_frame_ms_min`: the fastest wall time with the device waited for
  (about 190 ms here, nearly all of it the cascades), gated at +50 % like
  the unlit frame: a relative signal of the look passes' shader work
  (their invocations and arithmetic), never real milliseconds.

Issue #35 added, said but not gated: the bytes a lit frame uploads
(`lit_frame_upload_bytes`, `Renderer::uploads`), its passes
(`lit_passes`) and each pass group's fastest time on the software
adapter (`lit_pass_ms`, from GPU timestamps on frames timed apart) and
the frame's time less its passes' (`lit_between_ms`: copies and gaps), and
the terrain's uploads over 100 more ticks of the lit match
(`terrain_upload_instructions`, gated at +15 %, with its texture writes
and bytes): the signals an optimisation of passes or uploads shows.

The baseline is `fixtures/bench/quick.ron`, re-blessed with
`cargo xtask bench-quick --bless --reason "<why>"`.

## Byte identity: `olr-cli frame-hashes`

The render goldens compare pictures with a perceptual tolerance;
an optimisation meant to change nothing should change no byte.
`olr-cli frame-hashes <match flags> --frames N --out f.ron` plays a bot
match and draws each of its last N ticks lit on the software adapter,
writing per frame the world's checksum and an FNV-1a hash of the exact
bytes of the light grid (`Rgba16Float`) and of the lit target (the
composite, before the upscale). `fixtures/render/lit-hashes.ron` is one
such recording (4 chaos bots in Classic on Castle Strike Reloaded, seed 1,
the last 30 of 330 ticks), checked by `cargo xtask check --render`
(`olr-cli`'s `render_golden` test `lit_frames_keep_their_bytes`, about
7 s). A failure names the first tick and the texture that differs; a
different world checksum means the match itself changed (bots or the
simulation), not the picture. Hashes recorded on another adapter are not
compared. An intended picture change re-blesses it with
`cargo xtask bless-render --reason "<why>"`.
