# rl-lobby

Online lobby logic (networking spec §5.2, §7.2), sans-IO.

- **Layer 4, `#![no_std]` + `alloc`, wasm32.** Depends on `rl-proto` (same
  layer: `PublicKey`, `Signature`), `k256` (BIP-340 Schnorr, exact pin) and
  `sha2` (crate-local, no default features). Time and randomness come from
  the caller; there is no clock and no `getrandom` here.
- **Identity** (`identity.rs`): `Identity::from_secret(secret)` (`None` for
  an invalid scalar), `public()` (x-only, 32 bytes), `sign(message, aux)`
  (BIP-340 with the caller's 32 aux bytes), `verify(key, message, sig)`
  (false for any bad input, never a panic). `fingerprint(key)` is 8
  characters: lowercase unpadded base32 of `SHA-256(key)[..5]`; the format
  is pinned by a test. `challenge_message(nonce, host)` is
  `b"rust-liero join v1" ++ nonce ++ host`, what a joiner signs;
  `password_proof(nonce, password)` is `SHA-256(nonce ++ password)`.
- The secret lives in `<data>/identity.ron` (rl-ui's `Store::load_identity`).
