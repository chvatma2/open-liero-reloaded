# Bot strength: an OLX baseline (2026-10-11)

**Question.** Our bots score few kills (1-7 kills/min in Deathmatch). Is that
how OLX's bots play, or are ours weaker?

**Answer.** It is how OLX's bots play. With the same mod, level, number of
bots and difficulty, OLX's own bots (real-time dedicated server, commit
`d1ad953f`) score the same kills per minute as ours, within run-to-run noise:

- Classic, 4 Medium bots, all three levels: OLX 3.77 kills/min (6 runs), ours
  3.51 (30 seeds).
- OLX bots also mostly fire the Chaingun.
- In Classic, Xtreme is barely better than Medium in OLX too.

Our bots differ from OLX in **how they move and how often they fire**, not in
how many kills they score: they fire 0.5-0.75× as many shots, stand on the
ground 2-3× as long, and move 15-25% slower.

No tracked file was changed. The logs and scripts are in `target/bot-baseline/`.

---

## 1. Method

### OLX (`target/bot-baseline/olx_bots.py`, `run_olx.sh`, `batch.sh`)

- **Server.** Image `olr-olx-oracle:3c714fd09a0b`, run as
  `openlierox -dedicated -script`. The control script never calls `simStep`,
  so OLX runs in real time at 100 frames/s (checked with `getFrame`: 100.0 fps
  in every run). The bots read the wall clock and run their pathfinding
  thread exactly as in normal play.
- **Settings.** Deathmatch, `Lives -2` (unlimited), `KillLimit -1`,
  `TimeLimit 20` (minutes), `Bonuses 0`, `ForceRandomWeapons 0`. Everything
  else is at OLX's defaults: `LoadingTime 100`, `Standard 100lt` restrictions,
  `RespawnTime 2.5`, `SelfInjure` on. No human worm takes part; the dedicated
  host has no worm. Each run lasts 180 s from `Playing`.
- **Difficulty.** `addBots N <diff>` does not work in the lobby:
  `CWorm::setAiDiff` logs "can only set ai-diff for local bots", because the
  bot input handler only exists once the game starts. The bot constructor
  takes its difficulty from the profile instead (`CWormBot.cpp:4105`).
  - Random bots get mixed difficulties. The bundled `cfg/players.dat` holds
    Peter = Easy, N00b and Worm = Medium, Kamikazee! and Dummi = Hard,
    K1ll0r and CPU 5 = Xtreme.
  - So the script adds bots with `addBot "<profile>"`: N00b and Worm for
    Medium, K1ll0r and CPU 5 for Xtreme. The names in the logs confirm it.
- **Measurements.**
  - Kills: `getWormScore` (lives, score, `fDamage`, health), polled every
    0.2 s. Score equals kills, because `SuicideDecreasesScore` is off.
  - Deaths: counted when the polled health turns to -1. The respawn takes
    2.5 s, so a 0.2 s poll misses none.
- **Recorded runs.** Each config also has one recorded run (suffix `-b`), with
  `simRecord` on (no `simLoadInputs`, so the bot AI keeps running). It gives
  per-frame worm state and the shots by weapon.
  - Recording does not change the kill rates. Over the Classic configs the
    unrecorded runs average 3.86 kills/min and the recorded ones 3.95.
  - The trace's `deaths` and `suicides` counters lag, so deaths are counted
    from `alive` turning false.
- **Run counts.** 7 Classic configs × 2 runs. 4 MW configs × 6 runs: MW gives
  bots random loadouts, so it is noisier.

### Ours (`target/bot-baseline/probe`, `run_ours.sh`)

- **Probe.** A throwaway crate outside the workspace, with path dependencies on
  `olr-content`, `olr-bots`, `olr-sim` and `olr-modes`. It builds the match exactly
  as `olr-cli match` does (`Content::build_match`, `SimSettings::lx56()`,
  `DEFAULT_PRESET`, no bonuses), with the kill and time limits turned off.
  - It writes a per-tick log in the shape of the OLX trace, so one analyser
    (`analyze.py`) measures both sides the same way.
  - Check against `olr-cli match ... --seed 1 --coverage`: the same 602 shots
    and 16 kill events.
- **Runs.** 18000 ticks (3 min), seeds 1-10, for every OLX config.
- **Variants** (`ours_var/`): Easy and Hard, `AimModel::OlxDirect`, and a 4×
  work budget, on Classic Dirt Level and Liero Factory.

## 2. Kills per minute: OLX against ours

All bots in a run are counted together. "Per-bot" is one bot's kills/min:
the mean, with the range in brackets.

| config | OLX n | OLX kills/min (each run) | OLX mean ± sd | OLX per-bot | OLX deaths/min | ours n | ours mean ± sd | ours range | ours per-bot | ours deaths/min |
|---|---|---|---|---|---|---|---|---|---|---|
| Classic, Castle, 4 Medium | 2 | 3.7, 3.7 | 3.66 ± 0.00 | 0.92 (0.33-1.33) | 4.5 | 10 | 3.23 ± 0.47 | 2.33-3.67 | 0.81 (0.33-1.67) | 4.5 |
| Classic, Dirt, 4 Medium | 2 | 4.3, 4.0 | 4.16 ± 0.24 | 1.04 (0.33-1.66) | 5.2 | 10 | 4.13 ± 0.86 | 3.00-6.00 | 1.03 (0.33-3.00) | 5.5 |
| Classic, Dirt, 4 Xtreme | 2 | 4.0, 5.0 | 4.50 ± 0.71 | 1.12 (0.67-1.67) | 5.7 | 10 | 5.27 ± 0.60 | 4.00-6.33 | 1.32 (0.33-2.33) | 6.6 |
| Classic, Dirt, 5 Medium | 2 | 5.0, 3.0 | 4.00 ± 1.41 | 0.80 (0.00-1.67) | 6.3 | 10 | 5.80 ± 0.76 | 4.33-7.00 | 1.16 (0.33-3.33) | 7.5 |
| Classic, Factory, 4 Medium | 2 | 3.3, 3.7 | 3.50 ± 0.24 | 0.87 (0.33-1.67) | 4.7 | 10 | 3.17 ± 0.84 | 1.67-4.67 | 0.79 (0.00-1.67) | 4.1 |
| Classic, Factory, 4 Xtreme | 2 | 3.0, 4.0 | 3.50 ± 0.71 | 0.87 (0.33-1.33) | 4.2 | 10 | 3.80 ± 0.50 | 3.33-4.67 | 0.95 (0.33-2.00) | 5.0 |
| Classic, Factory, 5 Medium | 2 | 3.7, 4.3 | 4.00 ± 0.47 | 0.80 (0.33-1.33) | 6.2 | 10 | 4.37 ± 1.12 | 1.67-5.33 | 0.87 (0.00-2.00) | 5.9 |
| MW 1.0, Castle, 4 Medium | 6 | 2.7, 5.7, 5.3, 3.7, 2.7, 3.3 | 3.89 ± 1.31 | 0.97 (0.00-2.67) | 9.3 | 10 | 3.73 ± 1.12 | 2.33-6.00 | 0.93 (0.00-2.67) | 8.3 |
| MW 1.0, Dirt, 4 Medium | 6 | 6.0, 4.7, 3.7, 5.7, 3.7, 3.3 | 4.50 ± 1.13 | 1.12 (0.00-1.67) | 16.8 | 10 | 6.10 ± 0.79 | 4.67-7.00 | 1.52 (0.00-3.67) | 14.7 |
| MW 1.0, Dirt, 4 Xtreme | 6 | 7.7, 9.3, 4.3, 7.0, 7.7, 8.0 | 7.33 ± 1.66 | 1.83 (0.33-3.33) | 16.2 | 10 | 6.27 ± 1.43 | 4.00-8.33 | 1.57 (0.33-4.33) | 15.4 |
| MW 1.0, Factory, 4 Medium | 6 | 4.0, 5.3, 4.7, 4.3, 2.3, 3.7 | 4.05 ± 1.02 | 1.01 (0.00-2.67) | 10.0 | 10 | 3.57 ± 1.12 | 2.33-5.67 | 0.89 (0.33-2.33) | 8.5 |

- **Overall.** No config shows our bots clearly weaker.
  - Ours are clearly stronger on MW Dirt Medium (6.10 against 4.50, t ≈ 3).
  - They are slightly weaker on MW Dirt Xtreme (6.27 against 7.33, t ≈ 1.3,
    not significant).
  - Every other config is within one standard deviation.
- **Deaths.** Deaths per minute match too, including MW's large number of
  self-kills. Deaths minus kills is about 1.3/min in Classic Dirt and about
  9-12/min in MW Dirt, on both sides.
- **Difficulty in OLX.**
  - In Classic, Xtreme ≈ Medium: Dirt 4.50 against 4.16, Factory 3.50 against
    3.50. This matches notes §0.6: difficulty changes noise, not intelligence.
  - Only MW Dirt shows a clear Xtreme gain in OLX (7.33 against 4.50). Ours is
    flat there (6.27 against 6.10), because our Medium is already higher.
- **Difficulty in ours** (variants, Classic): Easy, Medium, Hard and Xtreme
  score 3.50 / 4.13 / 3.83 / 5.27 on Dirt and 3.17 / 3.17 / 3.83 / 3.80 on
  Factory.
- **Other variants:**
  - A 4× work budget changes nothing: Dirt 4.13, Factory 3.10.
  - `AimModel::OlxDirect` is slightly worse: Dirt 3.80, Factory 2.77.
- **Context.** The in-game figure (4 Medium bots plus a scripted human, 1-7
  kills/min, mean 3.75) is in the same range as OLX.

### Damage

`getWormScore`'s `fDamage` is about 2× ours. OLX counts it twice: on the
client and again from `C2S_REPORTDAMAGE` (`STATUS.md`, "The damage score is
counted once"). It is not comparable. The health lost per bot per minute,
taken from the traces, is: Classic 110-156 in OLX against 111-176 in ours;
MW 262-592 against 214-393.

## 3. Weapons

Share of shots, recorded runs, against ours over 10 seeds:

| config | OLX shots | ours shots | OLX time held | ours time held |
|---|---|---|---|---|
| Classic, Dirt, 4 Medium | Chaingun 59%, Cannon 15%, SSG 15%, Doomsday 10%, Napalm 2% | Chaingun 59%, SSG 18%, Cannon 13%, Doomsday 7%, Napalm 3% | SSG 57%, Cannon 16%, Napalm 11%, Chaingun 9% | SSG 58%, Napalm 13%, Chaingun 12%, Cannon 12% |
| Classic, Factory, 4 Medium | Chaingun 82%, SSG 12%, Cannon 3% | Chaingun 76%, SSG 17%, Cannon 3% | SSG 56%, Chaingun 19%, Napalm 12% | SSG 55%, Chaingun 18%, Napalm 14% |
| Classic, Castle, 4 Medium | Chaingun 83%, SSG 11%, Doomsday 5% | Chaingun 77%, SSG 15%, Napalm 4%, Doomsday 4% | SSG 56%, Chaingun 23% | SSG 50%, Chaingun 19%, Napalm 18% |
| Classic, Factory, 4 Xtreme | Chaingun 84%, SSG 12% | Chaingun 73%, SSG 21% | SSG 54%, Chaingun 25% | SSG 52%, Chaingun 22% |

- **The Chaingun dominates in OLX too.** OLX gives every Classic bot the 100lt
  loadout, and its tactic (notes §1.7) picks the Chaingun first for a visible
  target at 50-300 px.
  - It holds the SSG (slot 0) most of the time: `AI_ReloadWeapons` selects the
    lowest empty slot, and that is the default.
  - Napalm and Cannon are rare on both sides. The Cannon is only used with the
    rope out.
- **MW 1.0.** Bots get random loadouts (no MW preset at `LoadingTime` 100).
  Which weapon dominates depends on the draw: Toxic Waste or Flamer in OLX,
  Flamer or Toxic Waste in ours.

## 4. Behaviour: where we differ (recorded OLX runs against ours, all alive frames)

| config | shots/min OLX / ours | shots/kill | shoot held | move held | on ground | speed px/s | rope attached | nearest enemy px | stuck (moved < 15 px in 3 s) | weapon changes/min* |
|---|---|---|---|---|---|---|---|---|---|---|
| Classic, Castle, 4 Medium | 282 / 173 | 77 / 55 | 0.014 / 0.008 | 0.75 / 0.66 | 0.06 / 0.28 | 71 / 56 | 0.33 / 0.38 | 160 / 147 | 0.14 / 0.13 | 48 / 83 |
| Classic, Dirt, 4 Medium | 252 / 190 | 63 / 48 | 0.071 / 0.037 | 0.78 / 0.69 | 0.12 / 0.24 | 61 / 53 | 0.51 / 0.53 | 65 / 61 | 0.06 / 0.10 | 74 / 85 |
| Classic, Dirt, 4 Xtreme | 209 / 112 | 42 / 22 | 0.043 / 0.017 | 0.73 / 0.64 | 0.13 / 0.28 | 63 / 48 | 0.49 / 0.57 | 66 / 68 | 0.06 / 0.11 | 90 / 154 |
| Classic, Dirt, 5 Medium | 320 / 221 | 107 / 39 | 0.048 / 0.035 | 0.76 / 0.70 | 0.10 / 0.24 | 66 / 53 | 0.50 / 0.52 | 62 / 62 | 0.07 / 0.10 | 61 / 87 |
| Classic, Factory, 4 Medium | 259 / 171 | 71 / 57 | 0.014 / 0.010 | 0.79 / 0.69 | 0.07 / 0.26 | 78 / 60 | 0.50 / 0.49 | 127 / 120 | 0.07 / 0.12 | 59 / 89 |
| Classic, Factory, 4 Xtreme | 241 / 118 | 60 / 31 | 0.013 / 0.006 | 0.71 / 0.60 | 0.09 / 0.23 | 76 / 63 | 0.46 / 0.44 | 114 / 129 | 0.05 / 0.08 | 118 / 200 |
| Classic, Factory, 5 Medium | 341 / 238 | 79 / 58 | 0.014 / 0.011 | 0.77 / 0.68 | 0.10 / 0.23 | 79 / 63 | 0.49 / 0.48 | 98 / 113 | 0.07 / 0.09 | 52 / 89 |
| MW 1.0, Castle, 4 Medium | 397 / 220 | 70 / 65 | 0.020 / 0.011 | 0.71 / 0.63 | 0.07 / 0.31 | 72 / 55 | 0.31 / 0.37 | 185 / 204 | 0.03 / 0.15 | 50 / 103 |
| MW 1.0, Dirt, 4 Medium | 956 / 500 | 205 / 84 | 0.086 / 0.051 | 0.73 / 0.65 | 0.22 / 0.34 | 44 / 39 | 0.51 / 0.59 | 101 / 94 | 0.04 / 0.12 | 78 / 104 |
| MW 1.0, Dirt, 4 Xtreme | 560 / 283 | 60 / 46 | 0.069 / 0.028 | 0.70 / 0.59 | 0.21 / 0.39 | 43 / 35 | 0.55 / 0.63 | 96 / 97 | 0.07 / 0.13 | 137 / 216 |
| MW 1.0, Factory, 4 Medium | 361 / 236 | 68 / 70 | 0.020 / 0.013 | 0.71 / 0.64 | 0.10 / 0.27 | 70 / 59 | 0.39 / 0.45 | 186 / 153 | 0.03 / 0.08 | 62 / 104 |

\* Our motor changes weapons one slot at a time (CHANGE+arrow, 2 ticks a
slot), so a jump of 3 slots counts as 3 changes; OLX sets `iCurrentWeapon`
directly. The column overstates our weapon switching.

Standing still (alive with speed below 1 px/s), seed 1 against the recorded OLX
run: Classic Dirt 6.3% against 2.0%; Classic Factory 4.2% against 0.6%. Most
of it is on the ground without walking: 8.2% against 2.4%, and 7.6% against
1.6% (`ground.py`).

What matches closely: the rope-attached fraction, the distance to the nearest
enemy, the time alive, health lost per minute, weapon choice, and kills.

## 5. Why the kill rate is low (in OLX too): causes, ranked

The task asked for "causes if OLX outperforms ours". It does not, so this
section ranks what holds the kill rate down in both implementations.

1. **The tactics, not the port.** The 100lt rules send the Chaingun at most
   visible targets.
   - On both sides it takes about 50-80 shots per kill in Classic Medium, with
     the Chaingun making 59-84% of the shots.
   - The SSG and Cannon are used little: the SSG only within 50 px, the
     Cannon only with the rope out (notes §1.7).
2. **Difficulty adds no intelligence** (notes §0.6). It only scales aim noise,
   aim tolerance and the weapon-change delay. Kill rates hardly move from
   Easy to Xtreme, in OLX (Classic Xtreme ≈ Medium) and in ours (3.2-5.3
   across all four levels).
3. **Movement crowds out shooting.**
   - Bots hold "move" 65-79% of the time, and the rope is attached 33-63% of
     the time.
   - `AI_Shoot` freezes movement, and the movement code pulls the bot away
     again.
   - On Dirt Level the nearest enemy is ~60 px away on average (54-57% of the
     time within 50 px), yet a bot gets about 1 kill/min. Most of that
     closeness is through dirt: the target is the nearest enemy regardless of
     walls (notes §1.3), and the bot digs towards it.

## 6. Where our port deviates: most likely causes, with evidence

None of these lower the kill rate. They change how the bots look and how
often they fire.

1. **Fewer shots (0.5-0.75× OLX; "shoot held" about half of OLX's).** The
   most likely cause is the motor's aim (`STATUS.md`, "Bots press keys"):
   - the aim takes 0.2 s to speed up and to brake, so 60° takes 0.8 s, where
     OLX turns at 91°/s and snaps within 3°;
   - the aim only counts as good when the worm also faces the target's side;
   - a weapon change takes 2 ticks a slot and stops walking.

   OLX's bot writes `fAngle` and `iCurrentWeapon` directly. Our shots per
   kill are lower, so each shot hits more often and the kill rate comes out
   the same.
2. **2-3× more time on the ground, 3-7× more standing still, 15-25% slower,
   more "stuck" (MW 0.08-0.15 against 0.03-0.07).** Likely causes, all
   recorded in the motor deviations:
   - a jump is never pressed while the rope is out;
   - carving needs walking (OLX carves standing too);
   - turning around takes one step of walking;
   - a weapon change stops walking.

   Also possible: thinking once per 100 Hz tick against OLX's per-11 ms
   sub-step, and the budgeted rope-spot and shooting-spot scans spread over
   ticks (a probe waits a tick when the budget is spent). The 4× budget
   variant did not change kills; its movement metrics were not compared.
3. **The ballistic aim** (default `AimModel::Ballistic`) makes our bots a
   little stronger than `OlxDirect` (Dirt 4.13 against 3.80, Factory 3.17
   against 2.77). It may cover for part of the motor's loss.

## 7. Recommended next steps

1. **Do not tune the kill rate "back to OLX".** It already matches. If
   stronger bots are wanted, that is a deliberate deviation (spec §5.9 allows
   it): for example, a difficulty above Xtreme, a better 100lt weapon choice
   (SSG and Cannon at 50-150 px), or a target choice that prefers enemies in
   line of sight. Record it in `STATUS.md`, and measure it with this harness.
2. **For fidelity of feel, look at the motor.** Before changing anything,
   re-run `run_ours.sh` with variants of the motor (an aim that snaps within
   3° with no acceleration, a direct weapon select, carving and jumping while
   roping). Compare shots/min, on-ground time and stuck time with the OLX
   recorded runs (`olx_agg.json`).
3. **Keep the baseline reproducible.**
   - Consider moving `olx_bots.py` (real-time, profile-based difficulty,
     optional `simRecord`) and `analyze.py` into `tools/` as a bot-baseline
     check.
   - Use health lost (from the trace), not `fDamage`, to compare damage.
4. **More OLX runs** if a 1-kill/min question matters: Classic has only n = 2
   per config. MW needs ≥ 6 per config because of the random loadouts.

## Files (`target/bot-baseline/`)

- **OLX driver:** `olx_bots.py`, `run_olx.sh`, `batch.sh`, `jobs.txt`,
  `jobs2.txt`.
- **OLX runs:** `olx/<cfg>-<a..f>/`, each with `olx.log`, `scores.jsonl`,
  `summary.json` and `meta.json`. The `-b` runs also hold `trace.jsonl.gz`.
- **Our side:** `probe/` (the crate), `run_ours.sh` and `run_var.sh`.
  - `ours/sum/*.json`: per-match metrics.
  - `ours/raw/*-s1.jsonl.gz`: the per-tick logs of seed 1.
  - `ours_var/`: the variants.
- **Analysis:** `analyze.py`, `aggregate.py`, `olx_scores.py`, `ground.py`.
  - Aggregates: `olx_agg.json` and `ours_agg.json`.
  - Tables: `headline.md` and `behaviour.md`.
