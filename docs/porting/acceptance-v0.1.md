# v0.1 acceptance runs (plan 7 task 9, 2026-10-11)

Spec §1.3 criterion 1: a local Deathmatch against bots, playable on each of
the 4 levels with either mod. The runs follow plan 5's ruling R3: the real
game in a real window, driven by a script, with the counters and
screenshots read afterwards.

## Method

- **Build:** release `olr-game` at commit `c578b33` (plan 7 task 9's
  tick-percentile and autoplay label fixes, before the results-panel fix
  `20c03dd`).
- **Window:** a real WSLg window under Wayland (the default), 1920 × 1080
  (the `W1920x1080` preset), VSync (FIFO, the compositor's 60 Hz), sharp
  filter, interpolation on. There was no sound device (`--no-sound`), to
  keep the runs off the speakers.
- **Adapter:** every run reported `olr-game: GPU D3D12 (NVIDIA GeForce RTX
  5070 Ti) (Gl, Other), Fifo, Bgra8Unorm`. That is Mesa's d3d12 driver on
  the discrete card, chosen by `MESA_D3D12_DEFAULT_ADAPTER_NAME=NVIDIA`
  (19b152e).
- **Machine:** AMD Ryzen 5 7600X (12 threads), WSL2.
- **Command:** `olr-game --autoplay <script> --data-dir <fresh dir> --no-sound
  --stats`, on the real clock (no `--fake-clock`). Each data directory's
  `settings.ron` sets 4 bots at Medium and a 1-minute time limit. Every
  other option is the default: kill limit 15, unlimited lives, Standard
  100lt restrictions, OLX's bot loadouts, bonuses off.
- **The script**, one per run, goes through the menu flow:
  1. Main;
  2. click **Local Play**;
  3. for MW 1.0, open **Mod** and click "Modern Warfare1.0";
  4. click the level's card ("Castle Strike", ...);
  5. **Start**;
  6. the weapon selection: **Random**, then **Done**;
  7. in game, 2 s idle, then about 50 s of scripted play: Shoot held,
     right and left 1.2 s each, jumps, aiming up and rope throws;
  8. the match ends by its time limit (60 s), then OLX's 3 s of
     "Game Over", then the **Results**;
  9. quit on the results, so the exit summary still has the match's bots.

  Screenshots: main, local play, weapons, 2 s in, about 22 s and 40 s
  left, results.
- **Numbers:**
  - **FPS p50 / p1:** from the exit summary's frame-time p50 and p99, over
    the whole run (menus included, the first 60 frames skipped).
  - **Worst 1-s FPS:** from `--stats`, over the match's seconds without a
    screenshot; the worst second with one is in brackets.
  - **Tick** and **bot think** p50 / p99 / max: per 100 Hz tick, from the
    exit summary. Think is all 4 bots together.
- **Log:** stderr held no warning, error or panic in any run; the only
  lines are Mesa's `libEGL warning` lines at start, which WSL always
  prints. Each `autoplay.jsonl` ends in `pass`.

## Results

All 8 runs passed: menus → match → results, 60.0 s of match time,
6362–6364 ticks.

| Mod | Level | FPS p50 / p1 | Worst 1-s FPS | CPU busy ms/frame | Tick ms p50 / p99 / max | Bot think ms p50 / p99 / max | Peak projectiles | Bot kills / suicides | Catch-up ticks dropped |
|---|---|---|---|---|---|---|---|---|---|
| Classic | Liero Factory | 59.5 / 57.9 | 59 (52) | 2.86 | 0.045 / 0.323 / 0.965 | 0.024 / 0.102 / 0.304 | 127 | 3 / 0 | 17 |
| Classic | Dirt Level | 60.0 / 58.8 | 60 (53) | 2.74 | 0.041 / 0.163 / 0.290 | 0.020 / 0.052 / 0.242 | 97 | 5 / 1 | 20 |
| Classic | Fossil Facility | 59.5 / 58.0 | 57 (51) | 2.79 | 0.045 / 0.259 / 0.562 | 0.023 / 0.103 / 0.531 | 89 | 3 / 1 | 48 |
| Classic | Castle Strike | 59.6 / 51.0 | 52 (52) | 2.79 | 0.062 / 0.426 / 1.028 | 0.036 / 0.136 / 0.284 | 123 | 3 / 0 | 4 |
| MW 1.0 | Liero Factory | 60.0 / 58.9 | 59 (53) | 2.85 | 0.077 / 0.519 / 1.382 | 0.044 / 0.334 / 0.681 | 355 | 1 / 4 | 13 |
| MW 1.0 | Dirt Level | 60.1 / 59.0 | 58 (53) | 2.69 | 0.044 / 0.179 / 0.688 | 0.017 / 0.049 / 0.648 | 361 | 10 / 5 | 16 |
| MW 1.0 | Fossil Facility | 60.0 / 58.9 | 59 (51) | 2.78 | 0.054 / 0.317 / 0.960 | 0.022 / 0.120 / 0.466 | 454 | 2 / 10 | 29 |
| MW 1.0 | Castle Strike | 59.5 / 57.9 | 58 (55) | 2.98 | 0.064 / 0.535 / 1.300 | 0.036 / 0.213 / 0.393 | 284 | 2 / 2 | 9 |

Every run hit the 1024-entity cap (blood and gibs) at its peak. Input
latency was not measured: no run used `--latency-probe`, and only one
scripted press was timed (9.9 ms).

### Reading the numbers

- **Frame rate.** The game holds the compositor's 60 Hz. Each frame costs
  the CPU under 3 ms, and the rest of the 16.7 ms is the VSync wait.
- **Frame spikes are the screenshots.** Each screenshot is a GPU readback
  of 100–230 ms. A run's 3 in-match screenshots are exactly its 3 seconds
  with a frame over 60 ms, and the catch-up ticks it drops (at most 10
  ticks a frame) come from those stalls. Without screenshots the worst
  second is 52–60 FPS.
- **Ticks** take 0.04–0.08 ms at the median and at most 1.4 ms, under
  1.5 % of the 10 ms tick. **Bots** think in 0.02–0.04 ms at the median
  and under 0.7 ms at worst, never over their budget.
- **Kills.** Bots score few kills in a minute (1–10 for 4 bots), as
  plan 6 measured for OLX's bots (`bot-baseline-2026-10-11.md`). The
  human's scripted play kills little.

### Retries and one unexplained stall

- **Lost clicks.** 3 of the last 14 attempts failed because one scripted
  click had no effect (the two logs kept show Local Play and Start), and
  those runs were run again. An earlier batch lost the very first click, made 1
  frame after the window opened, until the scripts waited 60 frames. Runs
  on Xvfb (`cargo xtask e2e`) never lose a click. The likely cause is the
  real window's own pointer events (the desktop cursor), which egui
  receives in the same frames as the injected press and release; this was
  not proven.
- **A stall that did not come back.** The first MW 1.0 / Liero Factory
  run had 9 s (seconds 6–14 of the match) at 39–55 FPS. Its frames took
  two refreshes (25–32 ms) while the CPU was busy only about 3 ms: the
  wait was in presentation, not in the game. A second run with the
  machine idle (the one in the table) showed no stall.

## What the screenshots show

These were checked by eye on the PNGs (1920 × 1080).

- **Correct:**
  - **Menus.** Main and local play; the level thumbnails letterboxed with
    their names and sizes; the chosen card outlined; the mod shown.
  - **Weapon selection** over the level, with the mod's names (Classic's
    Mini Rockets, Cannon, ...; MW's Cluster Grenade, Shockrifle, ...).
  - **The HUD:** health; five slots with reload bars; the clock (red at
    0:00) and kill limit; the kill feed in OLX's texts, including OLX's
    own "commited suicide" typo; the minimap with worm dots and the view
    rectangle; the key hints.
  - **The world:** worms with names and ropes, blood and gibs, all 4
    levels' art in the right colours.
  - **Results:** the winner, the reason ("by time limit") and OLX's order.
- **Wrong (findings):**
  1. **Results table clipped** (fixed in `20c03dd`). At 1920 × 1080 the
     results panel was narrower than its table, so Suicides was cut and
     Damage hidden behind a horizontal scroll. The panel is now as wide as
     the table (`results_show_every_column_on_a_large_screen`). A run
     after the fix shows every column.
  2. **The HUD shows stale weapons during the selection.** While the
     weapon dialog is open, the HUD's slots list the placeholder world's
     loadout, not the dialog's choice. Done rebuilds the world, and from
     then on the HUD is right.
  3. **HUD panels cover the playfield's corners.** The minimap (top right)
     and the health and weapons panel (bottom left) sit over the world.
     When the camera clamps at a level edge, a worm can stand under them.
     On Dirt Level the human stood under the minimap, its name visible
     through it, and a bot under the weapons panel. OLX keeps its HUD in a
     bar below the viewport.
  4. **World names draw over the HUD and dialogs.** Worm names show over
     the minimap, and through the translucent weapon dialog and results
     panel. A name at the screen edge is cut off ("enLieroXor").
- **Not seen:** missing art, placeholder sprites, wrong colours, a HUD
  overlapping itself, egui errors.
