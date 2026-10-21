# Deferred verification

Changes made by the issue runner (`olr-runner`) that still need heavier
verification than a session may run, such as `cargo xtask bench`, a
Windows 1080p measurement or a playtest of a feel change. Clear an entry
once it is verified, before the release.

| Issue | What to run or measure | Why |
|---|---|---|
| #2 | Run `olr` in a real window (Wayland and X11, and the Windows zip): the run's `<data>/logs/*.log` has the window system, `GPU …` and `window …; video: …` lines; change the window size and fullscreen in Options and see the `window change`, `settings: video …` lines; kill the game (`kill -9`) and see the next run warn once; `cargo xtask e2e` still passes (it fails on any `warning:`/`error:` on the game's stderr) | The sandbox had no display (Xvfb's socket unreachable), so only runs failing before the window were checked; `e2e` was not run |
| #1 | Playtest Castle Strike (day) on a desktop GPU: the red-roofed building and the rock beside the sky show their OLX art, no brighter; then a lit 1080p frame (`olr-cli gpu-bench --look`) | The renders were checked on lavapipe only; `bench-quick` draws an unlit frame, so the composite's extra solid-map read per texel is unmeasured |
