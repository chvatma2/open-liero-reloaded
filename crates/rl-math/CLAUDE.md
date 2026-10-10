# rl-math

Bit-exact ports of OLX math: `Vec2` (`CVec`), `AbsTime`/`TimeDiff` (whole
milliseconds, `seconds() = ms * 0.001f`), and the macros `SIGN`, `CLAMP_DIRECT`,
`REDUCE_CONST`, `RESET_SMALL`.

- **Layer 0, `#![no_std]`.** Builds for `wasm32-unknown-unknown`.
- **`f32` only, in OLX's operation order.** Never reorder an expression, fold
  constants differently, or use `mul_add`: fidelity is checked bit for bit by
  the oracle (`fixtures/oracle`).
- **`sqrt` uses the `libm` crate**, never `std`, so every platform gets the same bits.
- **`sinf` and `cosf` are this crate's own** (`trig.rs`): a port of glibc 2.35's,
  which OLX's oracle uses, checked against it for every `f32` input. Never use
  `libm::sinf`/`cosf`, which are 1 ulp off for about one input in 1300.

Test: `cargo nextest run -p rl-math`.
