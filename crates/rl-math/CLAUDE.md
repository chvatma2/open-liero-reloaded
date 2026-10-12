# rl-math

Bit-exact ports of OLX math: `Vec2` (`CVec`), `AbsTime`/`TimeDiff` (whole
milliseconds, `seconds() = ms * 0.001f`), and the macros `SIGN`, `CLAMP_DIRECT`,
`REDUCE_CONST`, `RESET_SMALL`.

- **Layer 0, `#![no_std]` + `alloc`** (`MtWire` only). Builds for `wasm32-unknown-unknown`.
- **`f32` only, in OLX's operation order.** Never reorder an expression, fold
  constants differently, or use `mul_add`: fidelity is checked bit for bit by
  the oracle (`fixtures/oracle`).
- **`sqrt` uses the `libm` crate**, never `std`, so every platform gets the same bits.
- **`sinf` and `cosf` are this crate's own** (`trig.rs`): a port of glibc 2.35's,
  which OLX's oracle uses, checked against it for every `f32` input. Never use
  `libm::sinf`/`cosf`, which are 1 ulp off for about one input in 1300.
- **`atan2f` too** (`atan.rs`, glibc 2.35's fdlibm version, checked against
  the container on 6 million pairs). Keep glibc's decimal constants: the hex
  values in its comments differ in the last bit. `libm::atan2f` differs.
- `normalize_vector` and `fmod` are OLX's `NormalizeVector` and `FMOD<float>`.

- **Randomness** (`rand.rs`): `GlibcRand` is glibc 2.35's `rand()` with OLX's
  float wrappers (`random_num`, `random_pos_num`, `random_int`, `random_vec`),
  `RANDOM_NUMBERS` is OLX's fixed table, and `Mt19937` is boost's `mt19937`
  (`FindSpot`). Each generator counts its draws (`calls()`), which the oracle
  traces record (`rc`, `mt`). Every `random_*` call draws exactly as OLX does,
  including `random_int(0)`. `visit_state` feeds the full state to a checksum.
- **Serde** (world-state transfer): `Vec2`, `AbsTime`, `TimeDiff` and both
  generators derive it (`Mt19937` through `MtWire`, 624 words);
  `is_valid` tells whether a received generator can draw.

Test: `cargo nextest run -p rl-math`.
