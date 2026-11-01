# tools/sounds

Scripts that synthesise OLR's own sounds, each run with `python3 -I` and
only the standard library (`wave`), from the repository's root. Each is
deterministic (SplitMix64 noise, a fixed seed): running it again writes
the same bytes. The sounds are CC0, credited where they ship as
"synthesised for OLR by `tools/sounds/<script>`"; the maintainer may
replace any.

- `factory_sounds.py` (issue #97): Liero Factory Reloaded's sounds into
  `tools/levels/factory/sounds/`: `hum.wav`, the Crane's `Moving` loop
  (mono, 16-bit, 22050 Hz, a 1.0 s seamless loop of 100 Hz and its
  harmonics under a 4 Hz wobble, about −18 dBFS RMS; seed 97). The
  Crane's README block in `content/assets/README.md` reads it. Issue
  #98: `buzz.wav`, the Strip light's `On` and `Dim` loop (a 1.0 s
  seamless loop of 100 Hz and its 2nd to 12th harmonics, softly clipped,
  about −28 dBFS RMS; seed 98), and `tick.wav`, its `On` entry sound (40
  ms of noise decaying in about 4 ms, about −24 dBFS RMS; seed 981),
  read by the Strip light's block. `write` takes each sound's level.
