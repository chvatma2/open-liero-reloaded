# Render Profile Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A one-minute profiler (`cargo xtask profile`) that plays a staged nuke fight drawn the way the game draws it on the maintainer's RTX 5070 Ti and prints a ranked table of where each frame's time goes, per phase (calm, blast, hot), with the heat split into its steps. A first report comes from it.

**Architecture:** `olr-render` gets finer timing: the heat's own groups, its copies timed, each pass's span kept, and a workload reading. `olr-cli` gets `render-profile`. It stages the fight as a match with a nuke-only loadout and records it as a replay. Then it draws every tick of that replay at a few frames per tick through `Presentation` (the level's real look, materials and heat; no synthetic `bench_look`), and reports by phase. It reuses `gpu_bench`'s helpers. `xtask profile` cross-builds and stages `olr-cli.exe` with `winbuild` (as the envelope does), runs it on Vulkan, compares against a saved base, and with `--game` plays the same replay in `olr.exe` fullscreen for the cross-check.

**Tech Stack:** Rust 2024, wgpu (timestamp queries), clap, serde/RON, the existing `winbuild` WSL interop.

**Spec:** `docs/superpowers/specs/2026-10-24-render-profile-design.md`

## Global Constraints

- Read each crate's `CLAUDE.md` before editing it; respect layers (`cargo xtask layers`). `olr-cli` (6) may newly depend on `olr-ui` (5).
- Timing must not change any drawn byte: the render goldens and `fixtures/render/lit-hashes.ron` stay unchanged. Timing stays off unless `enable_gpu_timing` was called (the game never calls it).
- No panics on content; deterministic scene (seeded match, fixed ticks, a replay checked against its checksums).
- Integration tests are modules of `tests/integration.rs` (`autotests = false`). GPU-tier tests skip without `OLR_GPU_TESTS`.
- No pass/fail budgets: nothing fails because a frame is slow.
- Software-adapter times are printed with the warning `software adapter: times are relative only`.
- The profiler may be run on the real GPU without asking (the maintainer's consent, 2026-10-24); `cargo xtask bench`, `stress`, `envelope` and `full` still need consent.
- `cargo xtask check` before every commit; commits `<crate>: <imperative subject>`.

## Review Focus

1. **A device without timestamps:** the profile still runs and prints CPU times, and says once `GPU times: not measured (no timestamps)`. It shows no 0 ms pass rows. Test in Task 3.
2. **A device without `TIMESTAMP_QUERY_INSIDE_ENCODERS`:** the heat's copies go untimed and land in `between`. The report names the group `heat copy` as `not timed`, never 0 ms. Test in Task 1.
3. **More passes than query slots:** a catch-up frame with up to `MAX_HEAT_STEPS` (250) steps would overrun `PASS_SLOTS`. Untimed passes are counted in `GpuTimes::untimed`, and the report prints `N passes untimed` for that phase. The frame is never dropped and never panics. Test in Task 1.
4. **A fight where no nuke goes off** (a bot or mod change): the profile fails loudly with `the staged fight set off no nukes` instead of reporting a calm scene as the nuke case. Test in Task 2.
5. **The Windows run stopping halfway** (timeout, crash, interop missing): `xtask profile` reports the stderr tail and leaves the saved base untouched, and its processes are stopped by stage path (`winbuild::run` already does this). Test in Task 5 (the base is only written after a parsed report).

---

### Task 1: `olr-render`: heat groups, copy timing, per-pass spans and the workload

**Files:**
- Modify: `crates/olr-render/src/renderer/timing.rs` (groups, `Timer`, `GpuTimes`)
- Modify: `crates/olr-render/src/renderer/heat.rs:1175-1260` (`encode`: a group per op, timed copies)
- Modify: `crates/olr-render/src/renderer/pass.rs:221-237` (the heat stamp closure takes the op's group)
- Modify: `crates/olr-render/src/offscreen.rs:54,129` (ask for `TIMESTAMP_QUERY_INSIDE_ENCODERS` when the adapter has it, with `timestamps`)
- Modify: `crates/olr-render/src/renderer.rs` (`last_passes`, `workload`)
- Modify: `crates/olr-render/CLAUDE.md`
- Test: `crates/olr-render/src/renderer/timing.rs` (unit), `crates/olr-render/tests/gpu/heat.rs` (GPU tier)

**Interfaces:**
- Produces:
  - `PassGroup::Heat` is replaced by `HeatClear`, `HeatDeposit`, `HeatStep`, `HeatZero`, `HeatLayer` and `HeatCopy`, in that order at the front of `PassGroup::ALL`. Their names are `"heat clear"`, `"heat deposit"`, `"heat step"`, `"heat zero"`, `"heat layer"` and `"heat copy"`. `PASS_GROUPS` becomes 18.
  - `GpuTimes { …, pub untimed: u32 }`: the passes this frame that found no slot left.
  - `GpuTimes::copies_timed: bool`: `HeatCopy` was measurable on this device.
  - `Renderer::last_passes(&self) -> &[(PassGroup, f64)]`: the last read frame's timed passes in GPU order, in ms. It is filled by `gpu_times()`.
  - `pub struct Workload { pub heat_area: u64, pub heat_boxes: u32, pub heat_steps: u64, pub heat_passes: u64, pub world_targets: Vec<(u32, u32)>, pub glow_grid: Option<(u32, u32)>, pub cascades: u32 }`, re-exported at the crate root.
  - `Renderer::workload(&self) -> Workload`:
    - `heat_area` is the active boxes' pixels, the level's and the distant layers' summed;
    - `heat_steps` and `heat_passes` are the cumulative `heat_steps()` and `heat_passes()`;
    - `world_targets` is each view target's `target_size()`;
    - `glow_grid` and `cascades` are the glow light's cell grid and cascade count, `None`/0 unlit.

- [ ] **Step 1: Write the failing unit tests in `timing.rs`**

```rust
#[test]
fn heat_ops_have_groups_of_their_own() {
    let names: Vec<&str> = PassGroup::ALL[..6].iter().map(|g| g.name()).collect();
    assert_eq!(names, ["heat clear", "heat deposit", "heat step", "heat zero", "heat layer", "heat copy"]);
    assert_eq!(PassGroup::ALL.len(), PASS_GROUPS);
    assert_eq!(PassGroup::ALL[6], PassGroup::World);
}

#[test]
fn passes_past_the_slots_are_counted_untimed() {
    // A frame of PASS_SLOTS / 2 + 3 passes: the first PASS_SLOTS / 2 timed, 3 untimed.
    let spans: Vec<PassSpan> = (0..PASS_SLOTS / 2 + 3).map(|i| span_for_test(PassGroup::HeatStep, i)).collect();
    let t = gpu_times(&ticks_for_test(&spans), 1.0, false, &spans);
    assert_eq!(t.untimed, 3);
}
```

`span_for_test` and `ticks_for_test` are small helpers beside the existing `passes_add_up_in_their_groups`. They follow its slot layout, and a span past the slots has no slots. Model "untimed" exactly as `Timer::pass` sees it: a pass whose `slot()` returned `None`.

- [ ] **Step 2: Run them; they fail**

Run: `cargo nextest run -p olr-render timing::`
Expected: FAIL. `HeatClear` and `untimed` are not defined.

- [ ] **Step 3: Implement the groups, `untimed`, and the op's group in `heat.rs`'s `encode`**

`encode`'s stamp closure becomes `FnMut(PassGroup, bool) -> Option<RenderPassTimestampWrites>`, with the group of the op: `Clear` → `HeatClear`, `Deposit` → `HeatDeposit`, `Step` → `HeatStep`, `Zero` → `HeatZero`, `Layer` → `HeatLayer`. `pass.rs` passes the group through. The first heat pass still starts the frame's slot 0. `run_heat` (untimed) passes `|_, _| None`. `Timer` counts a pass that found no free slot in a `Cell<u32>` and copies it into `GpuTimes::untimed` on read.

- [ ] **Step 4: Time the heat's copies**

After each `Deposit`/`Step`/`Layer` pass, `encode` copies the scratch back. It brackets those copies with `encoder.write_timestamp` from two fresh slots in `HeatCopy`, only when the device has `Features::TIMESTAMP_QUERY_INSIDE_ENCODERS`. The capability goes to `Timer::new` as `inside_encoders: bool`, from `device.features()`. `Offscreen::open` requests the feature beside `TIMESTAMP_QUERY` when `timestamps` is set and the adapter has it. `GpuTimes::copies_timed` is that flag.

- [ ] **Step 5: Keep each pass's span; add `last_passes` and `workload`**

`Timer::read` keeps the frame's `(group, ms)` per timed pass in a `Vec`, reused between frames. `Renderer::gpu_times` refreshes it, and `last_passes` returns it. For `workload`, sum the `HeatSim` boxes' `w*h` over the level's heat and the distant layers' heat. Add `pub(super) fn area(&self) -> u64` and `fn boxes(&self) -> u32` to `HeatSim`. Read the glow light's grid and cascade count from the `gi` state already in `self.look`.

- [ ] **Step 6: Write the GPU-tier test in `tests/gpu/heat.rs`**

```rust
#[test]
fn heat_ops_are_timed_in_their_groups_and_counted_in_the_workload() {
    let Some(gpu) = timed_offscreen() else { return }; // OLR_GPU_TESTS and timestamps, else skip
    let mut r = heated(&gpu);                            // the module's helper: a lit renderer whose rock heats
    if !r.enable_gpu_timing() { return; }
    r.deposit_heat(/* one deposit in the middle of `maps()`, as `heated_rock` does */);
    r.advance_heat(HEAT_STEP_MS * 3);
    draw_one_frame(&gpu, &mut r);                        // the module's existing frame helper
    let t = r.gpu_times().expect("timed");
    assert!(t.passes[PassGroup::HeatDeposit as usize] > 0.0);
    assert!(t.passes[PassGroup::HeatStep as usize] > 0.0);
    assert_eq!(t.passes[PassGroup::HeatCopy as usize] > 0.0, t.copies_timed);
    assert!(r.last_passes().iter().any(|(g, _)| *g == PassGroup::HeatStep));
    let w = r.workload();
    assert!(w.heat_area > 0 && w.heat_boxes >= 1 && w.heat_steps >= 3);
    assert!(!w.world_targets.is_empty() && w.glow_grid.is_some());
}
```

Use the module's own helpers (`heated`, the frame helper the existing tests use). Add `timed_offscreen()` if none exists: an `Offscreen::open` with `timestamps: true` on the software adapter under `OLR_GPU_TESTS`.

- [ ] **Step 7: Run the unit tests, the GPU tier and the render check**

Run: `cargo nextest run -p olr-render timing:: && OLR_GPU_TESTS=1 cargo nextest run -p olr-render --test gpu heat_ops_are_timed && cargo xtask check --render --offline`
Expected: PASS, and `check passed` with no golden or lit-hash change. If `olr-cli`'s `gpu_bench` tests assert a `"heat"` name, update them to the new names.

- [ ] **Step 8: Update `crates/olr-render/CLAUDE.md`, then commit**

Add a line on the heat's groups, `untimed`, `last_passes` and `Workload`.

```bash
git commit -am "olr-render: time the heat's steps and copies apart, read a frame's workload"
```

---

### Task 2: `olr-cli`: the staged nuke fight

**Files:**
- Modify: `crates/olr-cli/src/runner.rs:207-246` (`Match::with_match_options`)
- Create: `crates/olr-cli/src/render_profile.rs` (the scene part; Task 3 adds the run)
- Modify: `crates/olr-cli/src/lib.rs` (`pub mod render_profile;`)
- Create: `crates/olr-cli/tests/render_profile.rs` (`mod render_profile;` in `tests/integration.rs`)

**Interfaces:**
- Consumes: `Match`, `MatchSpec`, `LevelAssets`, `olr_content::{MatchOptions, WeaponChoice}`, `olr_data::loadouts::MatchLoadout`; the replay recording `stress --record` uses (`stress.rs:434`).
- Produces:
  - `Match::with_match_options(assets: &LevelAssets, spec: &MatchSpec, options: MatchOptions, opts: &Options) -> anyhow::Result<Match>`. `with_assets` becomes `with_match_options(assets, spec, spec.match_options(), opts)`.
  - `pub const FIGHT_MOD: &str = "olr-mod-liero-v1-reloaded"`, `pub const FIGHT_LEVEL: &str = "olr-level-castlestrike-reloaded"`, `pub const FIGHT_BOTS: usize = 3`, `pub const FIGHT_SEED: u32 = 7`, `pub const FIGHT_TICKS: u32 = 2000` (20 s), `pub const NUKES: [&str; 2] = ["Big Nuke", "Mini Nuke"]`, `pub const NUKE_LOADING_TIME: i32 = 20`.
  - `pub fn fight_spec() -> MatchSpec`: `MatchSpec::new(FIGHT_MOD, FIGHT_LEVEL, FIGHT_BOTS, FIGHT_SEED, FIGHT_TICKS)` with `difficulty: Medium`, no time or kill limit.
  - `pub fn fight_options(spec: &MatchSpec) -> MatchOptions`: `spec.match_options()` with `weapons: WeaponChoice::Loadout(MatchLoadout { name: "Nukes".into(), weapons: NUKES.map(String::from).to_vec(), loading_time: NUKE_LOADING_TIME, unique: false })`.
  - `pub struct Fight { pub replay: olr_replay::Replay, pub explosions: u32, pub first_explosion: Option<u32> }`.
  - `pub fn stage_fight(content: &Content) -> anyhow::Result<Fight>`: plays the fight headless and records it. It counts the ticks with an explosion event. It fails with `the staged fight set off no nukes` when `explosions == 0`.
  - `pub fn has_explosion(events: &TickEvents) -> bool`: the tick has an explosion. Use the event class `olr-present` turns into its explosions' heat (`look.rs:19-26`).

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn the_nuke_fight_sets_off_nukes_and_is_the_same_every_time() {
    let content = content(); // the module helper other tests use: Content::open_bundled("content/packs")
    let a = stage_fight(&content).unwrap();
    assert!(a.explosions >= 20, "{}", a.explosions);
    assert!(a.first_explosion.unwrap() < FIGHT_TICKS / 4, "nukes early enough for a calm phase and a hot one");
    let b = stage_fight(&content).unwrap();
    assert_eq!(a.replay.checksums, b.replay.checksums);
}

#[test]
fn a_fight_without_nukes_is_refused() {
    // The same match with the loadout's weapons replaced by a weapon that never explodes
    // (Liero v1.0 Reloaded's "Laser") hits the guard.
    let err = stage_with_weapons(&content(), &["Laser"]).unwrap_err().to_string();
    assert!(err.contains("set off no nukes"), "{err}");
}
```

`stage_with_weapons(content, weapons)` is the `pub(crate)` body of `stage_fight`, with its weapons given. Expose it to the test as `#[doc(hidden)] pub`.

- [ ] **Step 2: Run them; they fail**

Run: `cargo nextest run -p olr-cli --test integration render_profile::`
Expected: FAIL. `stage_fight` is not defined.

- [ ] **Step 3: Implement `with_match_options`, the constants, `fight_spec`, `fight_options`, `has_explosion` and `stage_fight`**

`stage_fight` loads the assets with `LevelAssets::load(content, FIGHT_MOD, FIGHT_LEVEL)` and plays `Match::with_match_options` for `FIGHT_TICKS` with `step()`, settled. It takes the replay the way `stress --record` does. If Medium bots with only nukes set off fewer than 20 explosions, try `Personality::Chaos` before anything else. If chaos's `ChaosCycle` replaces the loadout, use `MatchSpec::locals` 1 with scripted keys firing (`spec::local_keys`). Whatever you settle on goes in the constants and the doc comment, with the numbers measured.

- [ ] **Step 4: Run them; they pass. Then `cargo xtask check --offline`**

Expected: PASS; `check passed`. The goldens are unchanged, because `with_assets` behaves as before.

- [ ] **Step 5: Commit**

```bash
git add crates/olr-cli && git commit -m "olr-cli: stage a nuke fight for the render profile"
```

---

### Task 3: `olr-cli`: the profile run, by phase

**Files:**
- Modify: `crates/olr-cli/src/render_profile.rs`
- Modify: `crates/olr-cli/src/gpu_bench.rs` (make `draw`, `real` and `frame_sample` `pub(crate)`; `Spread` gets `mean`)
- Modify: `crates/olr-cli/src/stress.rs:109-130` (`Spread { …, #[serde(default)] pub mean: f64 }`)
- Modify: `crates/olr-cli/Cargo.toml` (`olr-ui`, for `olr_ui::hud::layout::playfield`)
- Test: `crates/olr-cli/tests/render_profile.rs` (GPU tier), unit tests in `render_profile.rs`

**Interfaces:**
- Consumes:
  - from Task 1: `Renderer::{enable_gpu_timing, gpu_times, last_passes, workload, uploads}`, `GpuTimes::{passes, untimed, copies_timed, frame_ms}`, `PassGroup`, `Workload`;
  - from Task 2: `Fight`, `stage_fight`, `has_explosion`;
  - from `gpu_bench`: `check_adapter`, `PassTime`, `draw`, `frame_sample`.
- Produces:
  - `pub enum Phase { Calm, Blast, Hot }` (serde, `Display` as `calm` / `blast` / `hot`).
  - `pub const BLAST_TICKS: u32 = 50`.
  - `pub fn phase_of(ticks_since_explosion: Option<u32>, heat_area: u64) -> Phase`:
    - `Blast` when an explosion came within `BLAST_TICKS` ticks;
    - else `Hot` when `heat_area > 0`;
    - else `Calm`.
  - `pub const FRAMES_PER_TICK: u32 = 2` (about 200 FPS, the maintainer's frame rate).
  - `pub struct ProfileConfig { pub sizes: Vec<(u32, u32)>, pub frames_per_tick: u32, pub options: OffscreenOptions, pub window: Option<(u32, u32)> }`: `window` is the ticks `[from, to)` drawn (`None`: all). The ticks outside it are played undrawn: `on_tick`, plus `present.upload` and `renderer.run_heat()`, so the heat is the same.
  - `pub struct GroupCost { pub pass: String, pub ms: Spread, pub passes: Spread, pub largest_ms: Spread }`: each frame's total in the group, its number of passes, and its single largest pass.
  - `pub struct PhaseReport { pub phase: Phase, pub frames: u32, pub tick_ms: Spread, pub cpu_ms: Spread, pub gpu_ms: Option<Spread>, pub between_ms: Option<Spread>, pub groups: Vec<GroupCost>, pub untimed: u32, pub heat_area: Spread, pub discs: Spread, pub upload_bytes: Spread }`.
  - `pub struct SizeProfile { pub size: (u32, u32), pub playfield: [u32; 4], pub world_targets: Vec<(u32, u32)>, pub glow_grid: Option<(u32, u32)>, pub cascades: u32, pub phases: Vec<PhaseReport> }`.
  - `pub struct ProfileReport { pub adapter: String, pub backend: String, pub device_type: String, pub timestamps: bool, pub copies_timed: bool, pub scene: String, pub frames_per_tick: u32, pub sizes: Vec<SizeProfile> }` (serde, `Default`).
  - `pub fn playfield_area(size: (u32, u32)) -> [u32; 4]`: `olr_ui::hud::layout::HudLayout::new(screen, HudArrangement::Auto).playfield` (the maintainer's `hud_layout: Auto`) of the whole screen in pixels at UI scale 1, rounded to whole pixels.
  - `pub fn run(content: &Content, fight: &Fight, cfg: &ProfileConfig, progress: &mut dyn FnMut(String)) -> anyhow::Result<ProfileReport>`.

- [ ] **Step 1: Write the failing unit tests**

```rust
#[test]
fn frames_are_put_in_their_phase() {
    assert_eq!(phase_of(Some(0), 0), Phase::Blast);
    assert_eq!(phase_of(Some(BLAST_TICKS), 500), Phase::Blast);
    assert_eq!(phase_of(Some(BLAST_TICKS + 1), 500), Phase::Hot);
    assert_eq!(phase_of(None, 1), Phase::Hot);
    assert_eq!(phase_of(None, 0), Phase::Calm);
}

#[test]
fn the_playfield_is_the_games() {
    assert_eq!(playfield_area((1920, 1080)), [0, 0, 1920, 864]);   // 16:9, the bar's fifth below
    let [x, _, w, _] = playfield_area((3440, 1440));
    assert_eq!(w, 2560, "the 16:9 frame between the side columns");
    assert_eq!(x, (3440 - 2560) / 2);
    // Whatever height the side layout leaves it, it is the HUD's own playfield.
    let hud = HudLayout::new(Rect::from_min_size(Pos2::ZERO, vec2(3440.0, 1440.0)), HudArrangement::Auto);
    assert_eq!(playfield_area((3440, 1440))[3], hud.playfield.height().round() as u32);
}

#[test]
fn spread_has_a_mean() {
    assert_eq!(Spread::of(&[1.0, 2.0, 6.0]).mean, 3.0);
}
```

- [ ] **Step 2: Run them; they fail**

Run: `cargo nextest run -p olr-cli --lib render_profile:: stress::`
Expected: FAIL. `phase_of` and `mean` are not defined.

- [ ] **Step 3: Implement `Phase`, `phase_of`, `playfield_area` and `Spread::mean`**

- [ ] **Step 4: Write the GPU-tier test**

```rust
#[test]
fn a_profile_reports_every_phase_and_the_heat() {
    if std::env::var_os("OLR_GPU_TESTS").is_none() { return; }
    let content = content();
    let fight = stage_fight(&content).unwrap();
    let cfg = ProfileConfig {
        sizes: vec![(320, 180)],
        frames_per_tick: 1,
        options: OffscreenOptions { prefer: AdapterPreference::Software, timestamps: true, ..OffscreenOptions::default() },
        // llvmpipe draws a lit frame in about 0.2 s: 140 frames round the first nuke.
        window: fight.first_explosion.map(|t| (t.saturating_sub(20), t + 120)),
    };
    let r = render_profile::run(&content, &fight, &cfg, &mut |_| {}).unwrap();
    let s = &r.sizes[0];
    let phases: Vec<Phase> = s.phases.iter().map(|p| p.phase).collect();
    assert_eq!(phases, [Phase::Calm, Phase::Blast, Phase::Hot]);
    let frames: u32 = s.phases.iter().map(|p| p.frames).sum();
    let (from, to) = cfg.window.unwrap();
    assert_eq!(frames, to - from);
    assert!(s.glow_grid.is_some() && !s.world_targets.is_empty());
    let blast = &s.phases[1];
    assert!(blast.heat_area.max > 0.0 && blast.discs.max > 0.0);
    if r.timestamps {
        assert!(blast.groups.iter().any(|g| g.pass == "heat deposit"));
        assert!(blast.groups.iter().any(|g| g.pass == "cascades"));
    } else {
        assert!(blast.gpu_ms.is_none() && blast.groups.is_empty(), "no 0 ms rows without timestamps");
    }
}
```

- [ ] **Step 5: Implement `run`**

Follow `gpu_bench::run` (`gpu_bench.rs:337-505`):
- `Offscreen::open`, then `check_adapter`;
- `LevelAssets::load` of the replay's packs, with no `bench_look`: the level's own look;
- `replay_world`, then `Presentation` with `Viewport::GAME`, `Interpolation::Linear` and `SceneOptions { show_health_bars: false }`. The worms are added as `gpu_bench` adds them;
- the maintainer's video settings: `RenderSettings { filter: UpscaleFilter::SharpBilinear }`, blood at 100 % (the presentation's default; set it explicitly if the default differs), one view.

Per output size, make a target of `size`. Draw the view into `playfield_area(size)` through `draw`. Every tick of the replay in `window` (all without it) is drawn:
- `world.tick` and `present.on_tick` are timed as `tick_ms`;
- then `frames_per_tick` frames, frame `k` with `present.build(&world, k as f32 / fpt as f32 + 1.0 / fpt as f32, …)` (the last at 1.0);
- each frame runs `present.upload`, then `draw`, then waits for the device, and `gpu_times()`.

Per frame, record:
- `cpu_ms`: build, upload, encode and submit;
- the GPU frame;
- `between`: the frame less the sum of `passes`;
- each group's total, count and largest pass from `last_passes`;
- `untimed`;
- `workload().heat_area`;
- `present.look().map_or(0, |l| l.emitters.len())` as discs;
- `uploads().since(before)` bytes, texture plus buffer.

Put each frame in `phase_of(ticks since the last tick with has_explosion(&out), heat_area)`. Phases with no frames are left out, in the order Calm, Blast, Hot. Only groups some frame of the phase ran are listed (as `gpu_bench::pass_times` does). End with `check_on`, so the scene is the recorded fight. Without timestamps, `gpu_ms`, `between_ms` and `groups` are `None` or empty.

- [ ] **Step 6: Run the unit and GPU-tier tests**

Run: `cargo nextest run -p olr-cli --lib render_profile:: && OLR_GPU_TESTS=1 cargo nextest run -p olr-cli --test integration render_profile::`
Expected: PASS.

- [ ] **Step 7: `cargo xtask check --offline`, then commit**

`tests/tool_output.rs` keeps `gpu-bench.ron` byte-equal. If `Spread::mean` changes it, re-bless with `OLR_BLESS_TOOL_OUTPUT=1`, and check that xtask's strict structs in `xtask/src/envelope/` either tolerate or gain `mean` (`#[serde(default)]`).

```bash
git add crates/olr-cli fixtures/bench/tool-output && git commit -m "olr-cli: profile the staged fight's frames by phase"
```

---

### Task 4: `olr-cli render-profile`: the command, the ranked table and the comparison

**Files:**
- Create: `crates/olr-cli/src/render_profile_cmd.rs`
- Modify: `crates/olr-cli/src/main.rs:6,102,141` (the `RenderProfile` command)
- Modify: `crates/olr-cli/src/render_profile.rs` (`ranked`, `table`)
- Modify: `crates/olr-cli/CLAUDE.md` (the command's entry, beside `gpu-bench`'s)
- Test: unit tests in `render_profile.rs`; `crates/olr-cli/tests/cli.rs` (`--help` lists it)

**Interfaces:**
- Consumes: Task 3's `ProfileReport`, `run`, `ProfileConfig`; Task 2's `stage_fight`, `Fight`; `gpu_bench::parse_sizes`.
- Produces:
  - Flags:
    - `olr-cli render-profile [--sizes 1920x1080,3440x1440,3840x2160] [--frames-per-tick 2] [--adapter hardware|integrated|software] [--backend auto|vulkan|dx12|gl]`
    - `[--ticks FROM..TO] [--content content/packs] [--out F.ron] [--against F.ron] [--save-replay F.olrp]`
  - `--ticks` is `ProfileConfig::window`, for a quicker look at one stretch.
  - `--save-replay` writes the fight's replay for the game.
  - `--against` reads an earlier `--out`. If the file is missing or unreadable, it says `no base to compare against: <why>` and prints the table without deltas. It never fails on it.
  - `pub struct Cost { pub what: String, pub mean_ms: f64, pub share: f64 }`.
  - `pub fn ranked(phase: &PhaseReport) -> Vec<Cost>`: the GPU groups by `ms.mean`, plus `between` (`"copies and gaps"`). A phase without timestamps gives the CPU's `"cpu"` alone. The list is sorted by `mean_ms`, largest first, and `share` is of the phase's `gpu_ms.mean`.
  - `pub fn table(report: &ProfileReport, against: Option<&ProfileReport>) -> String`.

`table` prints:
1. A header: adapter, backend, the timestamps state, and `software adapter: times are relative only` when `device_type` is `Cpu`.
2. Per size: the size, the playfield, the world targets, the glow grid and cascades.
3. Per phase, a line `phase  frames  gpu mean/p99/max  cpu mean/p99/max  heat area mean/max  discs max  uploads mean`. Then the ranked costs: name, mean ms, share %, p99, largest pass. With `against`, add a delta column `±ms (±%)`, matched by size, phase and group name; a row missing in the base reads `new`.
4. `N passes untimed` where `untimed > 0`.
5. `heat copy: not timed (no timestamps inside encoders)` when `!copies_timed`.
6. Last, `Top costs`: the five largest rows over every phase at the size nearest 3440×1440, by `mean_ms`.

- [ ] **Step 1: Write the failing unit tests**

```rust
#[test]
fn costs_are_ranked_largest_first_with_their_share() {
    let p = phase_for_test(&[("cascades", 2.0), ("heat step", 0.5), ("bloom", 1.0)], 4.0); // gpu mean 4.0, between 0.5
    let r = ranked(&p);
    let names: Vec<&str> = r.iter().map(|c| c.what.as_str()).collect();
    assert_eq!(names, ["cascades", "bloom", "heat step", "copies and gaps"]);
    assert_eq!(r[0].share, 0.5);
}

#[test]
fn the_table_compares_with_a_base_and_names_new_rows() {
    let now = report_for_test(&[("cascades", 2.0), ("heat copy", 0.3)]);
    let base = report_for_test(&[("cascades", 4.0)]);
    let t = table(&now, Some(&base));
    assert!(t.contains("-2.00 ms (-50 %)"), "{t}");
    assert!(t.contains("new"), "{t}");
    assert!(t.contains("Top costs"), "{t}");
}

#[test]
fn a_software_run_says_its_times_are_relative() {
    let mut r = report_for_test(&[("cascades", 2.0)]);
    r.device_type = "Cpu".into();
    assert!(table(&r, None).contains("software adapter: times are relative only"));
}
```

- [ ] **Step 2: Run them; they fail**

Run: `cargo nextest run -p olr-cli --lib render_profile::`
Expected: FAIL. `ranked` and `table` are not defined.

- [ ] **Step 3: Implement `ranked`, `table` and the command**

The command:
1. Opens the content (`Content::open_bundled`).
2. Runs `stage_fight`, and writes `--save-replay` (the format `stress --record` writes).
3. Runs `run`, printing progress on stderr.
4. Prints `table` on stdout.
5. Writes `--out` as RON (`ron::ser::to_string_pretty`).

Backends and adapters are parsed as in `gpu_bench_cmd.rs`. Reuse its `Adapter` and `Backend` enums by moving them to `gpu_bench_cmd.rs`'s `pub(crate)` scope.

- [ ] **Step 4: Run the tests; then a software smoke run**

Run: `cargo nextest run -p olr-cli --lib render_profile:: && cargo run --release -p olr-cli -- render-profile --adapter software --sizes 320x180 --frames-per-tick 1 --out target/render-profile/smoke.ron`
Expected: PASS. The smoke run prints a table with `calm`, `blast` and `hot` and the `software adapter` line. Then run it again with `--against target/render-profile/smoke.ron`: every row reads within a few % (llvmpipe noise), and none reads `new`.

- [ ] **Step 5: Document in `crates/olr-cli/CLAUDE.md`; `cargo xtask check --offline`; commit**

```bash
git add crates/olr-cli && git commit -m "olr-cli: render-profile prints the ranked costs and compares with a base"
```

---

### Task 5: `cargo xtask profile`: on the real GPU from WSL, and the game cross-check

**Files:**
- Create: `xtask/src/profile.rs`
- Modify: `xtask/src/main.rs` (`Command::Profile`, dispatch)
- Modify: root `CLAUDE.md` rule 1 (one sentence: `cargo xtask profile` is the render profiler, run freely; its use)
- Test: unit tests in `xtask/src/profile.rs`

**Interfaces:**
- Consumes:
  - `winbuild::{missing, build, stage, run, RUN_TIMEOUT}`, used as `xtask/src/envelope/gpu.rs:90-176` uses them;
  - the game's `--replay --uncapped --no-sound --autoquit-after S --frame-log F --frame-log-skip S --content --data-dir` flags and its `FrameLog` RON (`crates/olr-game/src/stats.rs:211`).
- Produces:
  - `cargo xtask profile [--backend vulkan|dx12] [--native] [--set-base] [--game] [-- <extra render-profile args>]`.
  - Defaults to Windows on Vulkan. `--native` runs the Linux release `olr-cli` on the local hardware adapter instead, or the software adapter with `--adapter software` passed through.
  - Every run writes `target/render-profile/last.ron` and compares against `target/render-profile/base.ron` (`--against`). `--set-base` copies `last.ron` over `base.ron` after a successful run. Bases are per backend, `base-<backend>.ron`.
  - `--game` saves the fight's replay (`--save-replay`), stages a data directory, and plays the replay in `olr.exe`. It prints the game's `FrameLog` (fps, frame ms p50/p99/max, busy ms) beside the profile's 3440×1440 GPU mean/p99/max. The data directory is `game-data/` in the stage, holding a `settings.ron` copied from the maintainer's `%APPDATA%\open-liero-reloaded\data\settings.ron`, with `video.fullscreen: true` kept, `frame_rate` left to `--uncapped`, and the rest as theirs.
  - `fn profile_args(backend: &str, base: Option<&str>, save_replay: bool, extra: &[String]) -> Vec<String>` and `fn base_name(backend: &str) -> String`, both pure.

- [ ] **Step 1: Write the failing unit tests**

```rust
#[test]
fn the_profile_compares_with_its_backends_base() {
    assert_eq!(base_name("vulkan"), "base-vulkan.ron");
    let a = profile_args("vulkan", Some("base-vulkan.ron"), false, &[]);
    assert!(a.windows(2).any(|w| w == ["--backend", "vulkan"]));
    assert!(a.windows(2).any(|w| w == ["--against", "base-vulkan.ron"]));
    assert!(a.windows(2).any(|w| w == ["--out", "last.ron"]));
    assert!(!a.contains(&"--save-replay".to_string()));
}

#[test]
fn a_failed_run_leaves_the_base_alone() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("base-vulkan.ron"), "old").unwrap();
    assert!(finish(dir.path(), "vulkan", Err("timed out".into()), true).is_err());
    assert_eq!(std::fs::read_to_string(dir.path().join("base-vulkan.ron")).unwrap(), "old");
}
```

`finish(dir, backend, result: Result<String, String>, set_base: bool) -> Result<(), String>` takes the run's `last.ron` text or its error. It writes `last.ron`, and `base-<backend>.ron` when `set_base`, only on `Ok`.

- [ ] **Step 2: Run them; they fail**

Run: `cargo nextest run -p xtask profile::`
Expected: FAIL.

- [ ] **Step 3: Implement `profile.rs` and the command**

For Windows:
1. Run `winbuild::missing()` (a clear message if mingw or interop is missing).
2. Build with `winbuild::build()`.
3. Stage with `winbuild::stage(release, content, &[base file if any])`.
4. Run `winbuild::run(stage, "olr-cli.exe", &args, &[], RUN_TIMEOUT)`. Stream its stdout (the table) to the terminal, and read `last.ron` back from the stage.

With `--game`, after the profile:
1. Write `game-data/settings.ron`. If the maintainer's file is missing, write defaults with `fullscreen: true`, and say so.
2. Run `olr.exe` with the replay, `--autoquit-after` the fight's length in seconds plus 2, and `--frame-log-skip 1`.
3. Read `frame-log.ron` and print the comparison.

Print `the game will open fullscreen on the display for about 25 s` before it starts.

- [ ] **Step 4: Run the tests; `cargo xtask check --offline`; commit**

```bash
git add xtask CLAUDE.md && git commit -m "xtask: profile runs the render profile on the Windows GPU and checks it against the game"
```

---

### Task 6: The first run and the report

**Files:**
- Create: `docs/perf/2026-10-24-render-profile.md`
- Modify: `docs/porting/STATUS.md` (one line under the graphics section pointing at the report)

- [ ] **Step 1: Run the profiler on the RTX 5070 Ti**

Run: `cargo xtask profile --set-base` (Windows, Vulkan), then `cargo xtask profile --backend dx12 --set-base`.
Expected: two tables, timestamps on, `heat copy` timed or marked `not timed`, no run over about 90 s. If a run takes much longer, lower `FIGHT_TICKS` to 1500 and say so in the report.

- [ ] **Step 2: Tell the maintainer, then run the game check**

Say in the session that the game is about to open fullscreen for about 25 s. Then run `cargo xtask profile --game`, and say when it is done.
Expected: the game's frame log beside the profile's numbers.

- [ ] **Step 3: If a group other than the heat dominates and has many passes, look at its passes**

For example, the cascades or the jump flood. Read their per-pass spans (`largest_ms` against `ms`, and the pass counts) to see whether one pass or the count is the cost. Add nothing to the code for this unless the existing numbers cannot tell.

- [ ] **Step 4: Write `docs/perf/2026-10-24-render-profile.md`**

Sections:
- **Setup:** adapter, backends, sizes, fight, frames per tick.
- **Where the time goes:** the `Top costs` and each phase's ranked table at 3440×1440, for Vulkan and DX12.
- **What scales with what:**
  - the costs across the three sizes, against the playfield's pixels;
  - against the world targets and glow grid, which do not change with size;
  - against the heat area and discs between phases.
- **The game against the profile:** the frame log beside the GPU times, and the gap with its likely parts (egui, presenting, the game loop) if large.
- **Candidates for cheap wins:** ranked by expected gain over effort and look risk, each with the number that points at it. No optimization is done in this work.

- [ ] **Step 5: Commit**

```bash
git add docs/perf docs/porting/STATUS.md && git commit -m "docs: the first render profile of a nuke fight"
```
