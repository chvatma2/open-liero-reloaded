# Deferred verification

Changes made by the issue runner (`olr-runner`) that still need heavier
verification than a session may run, such as `cargo xtask bench`, a
Windows 1080p measurement or a playtest of a feel change. Clear an entry
once it is verified, before the release.

| Issue | What to run or measure | Why |
|---|---|---|
| #1 | Playtest Castle Strike (day) on a desktop GPU: the red-roofed building and the rock beside the sky show their OLX art, no brighter; then a lit 1080p frame (`olr-cli gpu-bench --look`) | The renders were checked on lavapipe only; `bench-quick` draws an unlit frame, so the composite's extra solid-map read per texel is unmeasured |
