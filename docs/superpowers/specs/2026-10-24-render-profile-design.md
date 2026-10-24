# Render profile: where a heavy lit frame's time goes

Date: 2026-10-24. Status: design, approved for planning once reviewed.

## Intent

Most of the graphics work (lighting, glow, heat, bloom, distant layers)
is in, and it has made frames expensive. On the maintainer's RTX 5070 Ti,
fullscreen at 3440×1440 on Windows, the frame rate falls into the low 200s
when heat-heavy weapons such as the Big Nuke go off. A 2D game on that
card should be far faster, so weaker hardware will suffer badly.

The next phase is optimization, aiming for the most speed for the least
loss of looks (some loss is acceptable). Before optimizing anything we need
a **quick, repeatable measurement of which parts of the frame cost the
most** in that kind of play, so the work goes to the bottlenecks and the
low-hanging fruit first, and each change shows what it bought.

The maintainer's words: do not get fixated on specific times. The aim is not
to hit targets but to identify bottlenecks and cheap wins.

For context only (not targets this work must meet): the maintainer would
eventually like a steady 60 FPS at 1080p on an office laptop's integrated
GPU, and 120 FPS at 4K on any dedicated GPU of the last ten years.

## What exists today, and why it is not enough

- `olr-cli gpu-bench` draws a replay offscreen at several sizes, with GPU
  timestamps per pass group (`olr_render::PassGroup`: heat, world, emission,
  jump flood, cascades, fluence, occlusion, sum, soak, blur, bloom,
  composite, upscale) and p50/p99 frame times. But with `--look` it draws a
  **synthetic look** (`gpu_bench::bench_look`: one pulsing glow group, no
  materials), so it never runs the heat, the level's real lights, materials,
  sky or distant layers, and it draws the scene over the whole target, not the
  game's layout (on an ultrawide the game has side panels).
- `olr-cli quick-bench` / `cargo xtask bench-quick` draws a real lit frame
  of Castle Strike Reloaded, but with the Classic mod (no Reloaded
  weapon lights, no heat), on the software adapter (llvmpipe). Its pass
  times are relative at best: on llvmpipe the cascades dominate, which may or
  may not be true on a real GPU.
- `cargo xtask bench` is the gated, slow, real benchmark; it is not a tool for
  iterating.
- The game already logs a session's frame times when it quits (`olr: N
  frames … frame ms p50 p99 max; busy ms …; acquire …`). The maintainer's log
  of today's Castle Strike Reloaded session (Liero v1.0 Reloaded, one player,
  3 Medium bots; Vulkan, Mailbox) shows the CPU nearly idle (busy mean
  0.25 ms, ticks 0.013 ms mean) and most of the frame spent waiting to acquire
  the next image (mean 1.17 ms): the frame is very likely GPU-bound. The
  match's replay is `match-000076.olrp` in the game's Windows data folder
  (`%APPDATA%\open-liero-reloaded\data\replays\`), and the game plays a
  replay with `olr --replay f.olrp`.
- The look's passes draw into a world target of the view's world pixels times
  the art scale (`olr-render`'s `WorldFrame::target_size`), not the window's
  pixels; only the upscale grows with the output size. So the output size is
  likely not what most passes scale with.
- The session can measure on the real GPU: WSL sees the RTX 5070 Ti
  (`/dev/dxg`), and the cross-compiled `target/x86_64-pc-windows-gnu/release/olr-cli.exe`
  runs from WSL through Windows interop, so the real card (Vulkan or DX12) is
  reachable from the session without copying files to Windows.

## Goals

1. **A heavy, representative scene drawn the way the game draws it.** Liero
   v1.0 Reloaded on a lit Reloaded level, with a normal handful of bots
   (not the overkill 32-chaos-bot stress replay), in which several nukes
   (Big Nuke, Mini Nuke) go off. It goes through the game's real drawing path
   (`olr_present::Presentation` with the level's own look, materials,
   heat, sky, distant layers and the mod's lights), and the views are laid
   out as the game lays them out at that window size (side panels on an
   ultrawide). It uses the maintainer's video settings
   (`settings.ron`: blood 100 %, the Sharp filter, interpolation on, one
   view), and draws frames at the cadence of uncapped play, several frames per
   10 ms tick, so heat steps and catch-up passes land in frames as they do in
   the game (a fixed simulated frame rate, a few hundred a second, is fine).
2. **Phases of the scene, reported apart:** calm play before the blast, the
   blast itself, and the hot aftermath while craters glow and cool. We do not
   yet know which phase is worst.
3. **Where the time goes, ranked.** Per output size, per phase:
   - each pass group's GPU time, with the **heat group split into its own
     steps** (deposit, the spread/cool step, the copy back, the layer reset,
     whatever the heat code actually runs), and any other group that turns
     out to dominate split the same way;
   - the frame's GPU time not covered by any pass (copies, gaps);
   - the CPU side of a frame: the tick and presentation, building the draw
     lists, uploads (with their bytes) and encoding/submitting;
   - the mean, the 99th percentile and the worst frame;
   - a ranked list of the biggest costs, so the first candidates are obvious.
4. **What each cost scales with.** Enough to tell work that grows with
   output pixels (fix by resolution, scaling or sampling) from work that grows
   with the level, the heat's active area or the number of lights (fix by
   doing less of it). At least: the three output sizes 1920×1080, 3440×1440 and
   3840×2160, the world target's size at each, plus workload counts per frame
   such as the heat's active box area, the number of lights/discs, the passes
   run and the bytes uploaded.
5. **Fast to iterate.** One command, about a minute or less for a run
   on the real GPU, printing a short ranked table. It saves its result, and the
   next run prints the change against the saved one, so an optimization shows
   its gain or loss per stage.
6. **A check against the game.** The tool saves the staged fight as a replay
   the game can play. The maintainer plays it once in the game (`olr --replay
   f.olrp`, fullscreen 3440×1440; the session does not take over their screen
   itself), and the first report sets the frame times the game logs when it
   quits (read from its Windows log folder) beside the tool's. If the game is much slower than the tool says, the
   gap (the HUD's egui drawing, presenting, the game loop) is itself a finding,
   and timing it inside the game is a follow-up, not part of this work.
7. **One first report.** Once the tool exists, it is run on the RTX 5070 Ti,
   and the findings,
   the ranked costs and the obvious candidates for cheap wins, are written up
   for the maintainer. That report is the deliverable that starts the
   optimization phase.

## Non-goals

- Optimizing anything. This work only measures.
- Pass/fail budgets or gates. The numbers are for finding bottlenecks, not for
  meeting targets; nothing fails because a frame is slow.
- Quality tiers or a "low" graphics setting.
- Measuring on a laptop or other hardware now (later, by the maintainer).
- Replacing `cargo xtask bench`, `bench-quick` or the envelope.

## Anti-goals (it would be a failure even if it works)

- A scene that does not represent play: the synthetic `bench_look`, the
  Classic mod, the overkill stress replay, or a layout the game never draws.
  The maintainer judges graphics performance on representative play
  (memory: "perf: measure reasonably").
- Precise-looking numbers that mean nothing: software-adapter times presented
  as real milliseconds, or GPU times without timestamps passed off as pass
  costs. If timestamps are missing, say so.
- A tool that takes many minutes, so nobody runs it between changes.
- Running other benchmarks without consent. The maintainer has allowed this
  profiler's runs (about a minute each, while it is built and through the
  optimization work) without asking; `cargo xtask bench`, `stress`, the
  envelope and `full` still need their OK, since the maintainer shares the PC.

## Constraints

- The project's rules (root `CLAUDE.md`): each crate's `CLAUDE.md` read
  before editing it, layers respected, no panics on content, deterministic
  output (the scene must be the same every run: seeded match, fixed ticks).
  The simulation's crates stay untouched by profiling needs, apart from
  read-only counters if any are needed.
- Timing instrumentation must not change what is drawn: render goldens and
  `lit-hashes.ron` stay as they are. Instrumentation that costs time
  (extra timestamp queries) must be off in the game unless asked for.
- The Windows run uses the existing cross-compile target
  (`x86_64-pc-windows-gnu`) and WSL interop, on Vulkan (what the game picked on
  this card) and, if cheap, DX12 beside it. The same tool must also
  run natively (Linux on a hardware adapter, or the software adapter, where it
  says loudly that its times are relative only).
- wgpu timestamps inside passes depend on device features; the split of a
  pass group into finer steps must work with what Vulkan and DX12 on this card offer,
  and degrade to the group's time where a finer split is not available.

## Decided

- The scene is a **staged nuke fight** (the maintainer's choice over their own
  replay): Liero v1.0 Reloaded on a Reloaded level, a normal bot count (about
  3, as in the maintainer's matches), Big Nukes and Mini Nukes going off, made
  deterministically so every run is the same.
- Sizes 1920×1080, 3440×1440 (the maintainer's display) and 3840×2160.
- Measured on the RTX 5070 Ti from the session (Windows, Vulkan first). The
  profiler may be run without asking each time.
- The game itself gets no new instrumentation in this work; its existing
  frame log is the cross-check (my call, from the builder check).
- Mean, p99 and worst frame are all reported.
- The result is a ranking of costs, not a comparison against budgets.

## Left to the builder

- How the nuke fight is staged: for example a match generated at run time with
  weapon restrictions leaving only the nukes, short loading times, or scripted
  locals, or such a match recorded once and committed as a replay. Which Reloaded level
  (Castle Strike Reloaded or Liero Factory Reloaded; both if cheap). How long
  each phase is and where it starts.
- Whether this extends `olr-cli gpu-bench` (a "real look" mode, the game's
  layout) or is a new command, and what the `xtask` wrapper that builds the
  Windows binary and runs it is called.
- Where the saved result for comparison lives (local under `target/` is
  enough; it is per machine and not a gate).
- How the heat group and any other dominant group are split into finer
  timestamps.
- Whether the HUD is drawn; if not, say so in the report.
- The report's form (a Markdown note under `docs/` with the ranked table and
  the candidates for cheap wins is the expected shape).
