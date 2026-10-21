# Deferred verification

Changes made by the issue runner (`olr-runner`) that still need heavier
verification than a session may run, such as `cargo xtask bench`, a
Windows 1080p measurement or a playtest of a feel change. Clear an entry
once it is verified, before the release.

| Issue | What to run or measure | Why |
|---|---|---|
| #3 | Deploy the relay (`tools/bug-relay/README.md`: R2, KV, the GitHub App, secrets, `wrangler deploy`), build with `OLR_REPORT_URL=https://…/report`, and send a report from a real window with a replay attached: the issue has `bug` and `player-report`, the App's `[bot]` author, the GPU/display/settings tables, working replay and log links; a fourth report within the hour shows "Too many reports right now, please try again later." | No Cloudflare account, GitHub App or display in the runner's sandbox: the game's send was tested against a local mock, the relay against fakes (`node --test`); the screen was checked headless (kittest), never in a window |
| #2 | Run `olr` in a real window (Wayland and X11, and the Windows zip): the run's `<data>/logs/*.log` has the window system, `GPU …` and `window …; video: …` lines; change the window size and fullscreen in Options and see the `window change`, `settings: video …` lines; kill the game (`kill -9`) and see the next run warn once; `cargo xtask e2e` still passes (it fails on any `warning:`/`error:` on the game's stderr) | The sandbox had no display (Xvfb's socket unreachable), so only runs failing before the window were checked; `e2e` was not run |
| #1 | Playtest Castle Strike (day) on a desktop GPU: the red-roofed building and the rock beside the sky show their OLX art, no brighter; then a lit 1080p frame (`olr-cli gpu-bench --look`) | The renders were checked on lavapipe only; `bench-quick` draws an unlit frame, so the composite's extra solid-map read per texel is unmeasured |
