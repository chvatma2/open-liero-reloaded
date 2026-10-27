# olr-script

Asset scripts (the asset-based levels design, calls 10, 12, 19 and 20; spec
`docs/superpowers/specs/2026-10-26-asset-based-levels-design.md`): an
asset's default script, a placed copy's own and the level script are Rhai
text. Issue #80 checks their syntax (`check`); issue #88 runs them
(`Scripts`, the simulation's `olr_sim::Director`); #89 adds the name
checks (`ScriptNames`, `check_level`), `move_to`, `stop`, `moving`,
`min`, `max` and `arrived`; #90 `blocked`, #92 `play`, #94 `hit` and
`wrecked`.

- **Layer 3, `#![no_std]` + `alloc`, `#![forbid(unsafe_code)]`.** Builds for
  `wasm32-unknown-unknown` (`xtask/layers.ron`'s `no_std` and `wasm`).
  Depends on `rhai` and `olr-sim` (the `same_layer` pair `("olr-script",
  "olr-sim")`; `("olr-edit", "olr-script")` is #89's save refusal,
  `olr_edit::scripts`). Its callers: `olr-content` (compiles a level's
  scripts at load), `olr-edit` (`check_level`) and `olr-editor`
  (`check`). Tests also use `olr-data` and `olr-terrain`.
- **Rhai's pin:** `rhai = "=1.27.0"`, default features off, with
  `no_std`, `sync`, `no_float`, `only_i64`, `no_module`,
  `no_custom_syntax`, `no_time`, `no_closure` and `internals` (the
  workspace's `Cargo.toml`), so the editor never parses a script the
  simulation would read differently. **Why pinned:** a run's operation
  count is part of the simulation (the budgets below decide which runs
  happen), so a Rhai version change is a simulation change.
  `RHAI_VERSION` (`"1.27.0"`) is checked against `Cargo.lock`
  (`rhai_version_matches_cargo_lock`), and `olr-proto`'s fingerprint
  hashes this crate's sources, so a bump changes the fingerprint too.
  `sync`: `Engine` and `AST` are `Send + Sync`, as a `World` must be.
  `no_float`, `only_i64`: integers only (`INT` is `i64`; overflow is a
  script error); no trigonometry (scripts use integer tables).
  `no_module`, `no_time`, `no_closure`: no files, no clock, no shared
  captured state. On wasm Rhai depends on `web-time`, unused with
  `no_time`. It pulls in `ahash`, `bitflags`, `hashbrown`,
  `no-std-compat`, `num-traits`, `once_cell`, `rhai_codegen`,
  `smallvec`, `smartstring` and `thin-vec`, all in the offline registry.
  Rhai implements `Add<…>` for `String`: a crate linking it may need
  `"a".to_string() + s.as_str()` where `+ &s` inferred before.
- **The hash-order rule:** in game builds `ahash` is unified with
  `getrandom` (egui, epaint, winit), so its seeds differ from run to run.
  That is harmless only because nothing may depend on hash order: Rhai's
  object maps are `BTreeMap`s, the API returns no hash map, and
  `AST::iter_functions` (hash order) is sorted before a function gets an
  index (`Scripts::compile`).
- **`engine()`**: `Engine::new_raw`, no optimisation (a compile evaluates
  nothing; a run's count is the script's as written), `eval` disabled,
  the string interner off (`set_max_strings_interned(0)`: no shared
  mutable state), `on_print`/`on_debug` doing nothing, and the limits:
  `MAX_EXPR_DEPTH` 32 and `MAX_FUNCTION_EXPR_DEPTH` 16 (Rhai's debug-build
  defaults, so parsing never runs out of stack), `MAX_STRING_BYTES` 256
  (literals at compile, every string a run makes), `MAX_RUN_OPERATIONS`
  5,000, `MAX_CALL_LEVELS` 16, `MAX_ARRAY` 64, `MAX_MAP` 64,
  `MAX_VARIABLES` 64, `MAX_FUNCTIONS` 64. `check` and the runtime parse
  with it alike.
- **`check(text) -> Vec<ScriptIssue>`** (`ScriptIssue { line, column,
  message }`, shown `line:column: message`): compiles to an AST and runs
  nothing; at most one issue, or one for text over `MAX_SCRIPT_BYTES` (64
  KiB, `olr_format::assets::MAX_SCRIPT_BYTES`: `olr-editor`'s tests check
  they agree). Syntax only.
- **`check_level(&ScriptNames, &[RunScript]) -> Vec<LevelIssue>`**
  (`names.rs`, issue #89; the design's call 20): `ScriptNames { copies:
  Vec<CopyNames { id, states, exposed, sounds }> }` is a plain table (no
  `olr-format` types; `olr_edit::scripts::names` fills it from a
  placement's live copies), `RunScript { copy: Option<&str>, text }` a
  script the level runs on that copy (none: the level script), and a
  `LevelIssue { script, issue }` names the script by index, its
  `ScriptIssue` the line and column. Per script, by line: its syntax
  error (`check`'s; then nothing else), a handler (the runtime's
  `HANDLERS`) defined only with another number of parameters ("handler
  tick() takes 0 parameters, not 1 parameter: it never runs"; its line
  found in the text), and every **string literal** passed where the API
  takes a name: `this.copy("…")` naming no copy, `set_state` naming no
  state of the copy it acts on, `get`, `set`, `move_to`, `stop`,
  `moving`, `min`, `max` naming no exposed name of it (#92 adds `play`
  against `sounds`). The copy acted on: this one (the own form, or a
  handle `this.me()`), a handle `this.copy("…")` itself, or a variable
  every `let` of which in the script binds the same `this.copy("…")`;
  any other handle (a parameter, a computed id) is not checked, nor is a
  name built at runtime (a script error when it runs). In the level
  script an own form is an issue ("the level script has no copy of its
  own"). Walks the AST with Rhai's `internals` (`AST::walk`, `Expr::Dot`
  of `ThisPtr` and a `MethodCall`); runs nothing.
- **`Scripts::compile(&LiveLevel)`** (`runtime.rs`): the runtime engine
  (`engine()` plus Rhai's arithmetic, logic, basic string, iterator,
  array and map packages: no `exit`, no `LanguageCorePackage`) and every
  script of the level compiled; the first that does not compile is a
  `CompileError { script, issue }`, which refuses the level
  (`olr_content::LoadedLive::new`: every peer refuses the same bytes
  alike). `director(&level)` gives `None` when no copy and no level script
  runs a script. Each AST gets the wrappers `__olr_run0`..`__olr_run3`
  merged in; per script its functions of no parameter sorted (what a
  timer names, by index: `Director::timer_functions`) and which handlers
  it defines. Top-level statements never run: work goes in `start()`.
  `Scripts` is the engine and the ASTs behind an `Arc`: stateless; its
  native functions write only the run's API object.
- **Run order** (`runtime::tick`, once a tick in `World::frame` after the
  mode's `on_tick` and its commands, before the frame clock advances;
  none while the world is frozen after the match's end): the level
  script's instance, then each copy in level order, each of its
  instances in run order (its asset's default, then its own). In each,
  `start()` at its first turn, its due timers in the order set (each
  advanced before it runs: once removed, `every` due again `period`
  ticks on), its handlers for the tick's events in event order, then
  `tick()`. A switched-off instance runs nothing.
- **Which events a handler sees** (`LiveCtx::events`): the last script
  phase's state changes, then this tick's events emitted before the
  script phase (setup calls' first, so the starting states' entries
  reach `entered` in the first tick). Events of the script phase itself
  (a `LiveState` from a `set_state`, a `ScriptError`) reach handlers
  **next tick**, so `entered` → `set_state` → `entered` never loops
  within a tick. Handlers (none required; another arity is not one):
  `start()`, `tick()`, `entered(state)` (this copy, by name),
  `spawned(worm)`, `killed(victim, killer)` (-1 nobody), `exploded(x, y,
  damage)` (whole pixels, truncated), `damaged(worm, amount, by)`
  (`amount` truncated), issue #89's `arrived(name)` (this copy's value
  `name` reached its motion's target: `EventKind::LiveArrived`, emitted
  in the motions' step before the physics, so it reaches `arrived` in the
  same tick) and issue #90's `blocked(name)` (this copy's value `name`
  was stopped by rock, another part or the level's edge, at the last
  place reached: `EventKind::LiveBlocked`, in the same step, so the same
  tick).
- **The API object** (`api.rs`, `Live`): each run (one handler or timer
  call) gets a fresh one bound as `this` (`CallFnOptions::bind_this_ptr`),
  holding the run's reads (copies' states and values, worms, the clocks:
  `LiveReads`, moved in and out of the world's phase buffer, never
  copied), its memory, timers, draws and command queue. A copy of `this`
  (`let me = this;`) is dead: its methods are script errors. A script's
  own helpers are called `this.helper(x)` so they keep `this`:
  ```rhai
  fn tick() { this.blink(3); }
  fn blink(n) { if this.rand(n) == 0 { this.set_state("Off"); } }
  ```
  Copy handles are integer indices. This copy: `this.me()` (-1 in the
  level script), `this.id()`, `this.state()`, `this.set_state(name)`,
  `this.get(name)`, `this.set(name, v)`, `this.has(name)`,
  `this.switch_off_default()` (switches off the copy's scripts that run
  before this one: its asset's default; call 10); in the level script
  these are script errors. Other copies: `this.copy(id)` (an unknown id
  a script error), `this.id(c)`, `this.state(c)`, `this.set_state(c,
  name)`, `this.get(c, name)`, `this.set(c, name, v)`, `this.has(c,
  name)`. `get` gives positions in whole world pixels (rounded down),
  `set` takes them so; the world clamps into the prop's limits; a value
  set lasts until the next state change (call 11), which sets the
  state's complete look. Reads see the world as the run started; its
  commands apply after it. **Motions** (issue #89; own-copy and handle
  forms alike): `this.move_to(name, target, speed)` (a position-like
  value, `X`, `Y`, `OffsetX`, `OffsetY`, else a script error; `target`
  whole world pixels, the world clamps it into the limits; `speed` world
  pixels a second, at least 1 (else a script error), made 1/256 pixel a
  10-ms tick, `speed × 256 / 100` rounded down and at least 1, whatever
  the game speed; `olr-sim` clamps it to `MAX_MOTION_SPEED`, 8 pixels a
  tick; it starts or replaces the value's motion: `LiveCommand::Move`),
  `this.stop(name)` (`LiveCommand::Stop`), `this.moving(name)` (from the
  run's reads: a `move_to` of the same run is not seen yet), and
  `this.min(name)`, `this.max(name)`: the value's limits for this copy
  (a stretched copy's scaled by the bake, `Health`'s within its points),
  whole world pixels for a position, so a default script sweeps a
  stretched crane's whole travel (`this.move_to("claw_x",
  this.max("claw_x"), 40)`). The world steps motions once a tick before
  the first physics sub-step (`olr-sim`'s `LiveWorld::step_motions`); a
  `set` of the value stops its motion, a state change does not. Timers: `this.after(ticks, "fn")`,
  `this.every(ticks, "fn")` (ticks are 10-ms frames, at least 1; one
  timer a function: a new one replaces it; at most `MAX_TIMERS` 16),
  `this.cancel("fn")`; a timer names a function of no parameter of the
  same script. Randomness: `this.rand(n)` (0 to n-1, n at least 1),
  `this.chance(percent)`, from the scripts' own generator
  (`olr_sim::ScriptRng`, in the world). The match: `this.time()` (ms since
  it started), `this.worm_count()`, `this.worm_alive(i)`,
  `this.worm_x(i)`, `this.worm_y(i)` (whole pixels, truncated),
  `this.game_over()`. Memory: `this.mem` (get and set), an object map of
  integers, booleans and strings, at most `MAX_MEMORY_KEYS` 32 keys,
  keys and strings at most `MAX_MEMORY_BYTES` 64 bytes (a setter past
  them is a script error), rebuilt from the world before each run and
  written back after a run that succeeded. Nothing else outlives a run.
- **Limits and how runs are counted.** Per run `MAX_RUN_OPERATIONS`
  (Rhai's `set_max_operations`) and the engine limits above. Per tick
  `MAX_TICK_OPERATIONS` 20,000, **counted per run**: each run calls its
  function through a wrapper (`fn __olr_run1(f, a) { this.call(f, a);
  this.__done(); }`) whose last call, the native `__done`, stores Rhai's
  operation count (`NativeCallContext::global_runtime_state()
  .num_operations()`, restarted at every `call_fn_with_options`) in the
  API object; a run charged its own count, a failed one (or one that
  never reached `__done`) its limit. A run starts only while the tick has
  at least `MAX_RUN_OPERATIONS` left, so no run overruns; the remaining
  runs are skipped, the same on every peer (their timers fire next tick).
  At most `MAX_LIVE_COMMANDS` 128 commands a tick, checked when queued (a
  script error past it). **Failures** (a script error, an overflow, a
  limit, `this` replaced): none of the run's commands apply, its memory,
  timers and draws stay as before, `EventKind::ScriptError { instance }`
  is emitted; after `MAX_SCRIPT_ERRORS` 16 the instance is switched off
  for the match. The state-bound constants are `olr-sim`'s, re-exported.
- **Allocation:** Rhai allocates while it runs, so a tick that runs
  scripts may allocate, bounded by the limits and freed by the run's end.
  A world without a director allocates nothing (`olr-sim`'s `no_alloc`).

Tests: issue #89's `tests/motion.rs`
(`move_to_reaches_its_target_at_its_speed_and_calls_arrived`,
`min_and_max_follow_a_stretched_copy`,
`a_set_stops_a_motion_and_a_state_change_does_not`, issue #90's
`rock_stops_a_motion_and_calls_blocked`; `tests/common/mod.rs`' `copy`
makes scenery parts, `rock_copy` rock ones) and `tests/names.rs`
(`check_level_finds_unknown_copies_states_names_and_bad_handlers`, with
lines); `tests/check.rs` (`a_good_script_has_no_issues`,
`syntax_errors_name_line_and_column`,
`hostile_scripts_are_cheap_and_never_panic`); `tests/runtime.rs` on a
world made in `tests/common/mod.rs`
(`the_console_cycles_its_states_every_300_ticks`,
`a_copy_script_runs_after_its_default_and_can_switch_it_off`,
`the_level_script_drives_two_copies_by_id`,
`events_from_the_script_phase_reach_handlers_next_tick`,
`timers_fire_in_order_and_every_repeats`,
`memory_persists_between_runs_and_not_after_a_failed_run`,
`an_endless_loop_hits_the_run_limit_and_changes_nothing`,
`the_tick_budget_skips_the_same_runs_every_time`,
`cheap_runs_are_charged_their_own_count`,
`two_worlds_on_two_threads_count_alike`,
`a_script_switched_off_after_too_many_errors`,
`floats_and_imports_do_not_compile`,
`rand_never_draws_from_the_worlds_generators`,
`scripts_read_the_match_and_a_frozen_world_runs_none`,
`rhai_version_matches_cargo_lock`).

Test: `cargo nextest run -p olr-script`.
