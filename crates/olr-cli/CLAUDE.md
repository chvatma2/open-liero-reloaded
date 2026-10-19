# olr-cli

The headless command-line tool. Its commands:

- `olr-cli import --olx <share/gamedir> --out <dir> [--mod D]... [--level F]... [--theme D]... [--skins]
  [--core F]... [--extra-skins DIR --extra-skins-source S --extra-skins-revision R] [--prune]`
  or `--all` (everything `olr_import::discover` finds; an error with item flags) writes one pack per item to `<out>/<pack id>/`, plus `import-report.ron`;
  the `--core` files (shared sprites, game sounds, `colours.cfg`) make one pack, `olx-core`.
  It exits with status 1 if any item failed, and still imports the rest.
  The pipeline is `olr_import` (plan 7 ruling Q3; the game's Import screen is gone), without `--prune` an import only adds or replaces packs and keeps the others; with it (as `cargo xtask fetch-olx-content` passes) `<out>` is exactly this import.
- `olr-cli pack check <path>...` (`pack_cmd.rs`; it replaced `validate` in
  plan 13d) runs `olr_pack::validate` on a pack, or on each pack directly
  under a directory, prints `<pack dir>: <severity>: <file>[:line:col]:
  <message>` per diagnostic and `checked N packs: E errors, W warnings`,
  and exits with status 1 on any error. Discovery skips hidden directories
  (an interrupted save's or import's `.import-<id>`/`.replaced-<id>`) and
  names the path it cannot read. `pack fmt [--check] <path>...`
  (`olr_edit::fmt`) rewrites packs in canonical form (`formatted <pack
  dir>: N files` for each pack it changed) or, with `--check`, lists
  `<pack dir>/<file>: not canonical` and exits 1 if any; a pack that does
  not load, or would not check, is an error on stderr and stays untouched.
  A save's warnings print on stderr as `warning: …`. `pack new
  <mod|level|skin|theme|core> <id> [--out DIR] [--name N] [--size WxH]
  [--theme ID] [--from PACK]` (`olr_edit::new`): `--out` defaults to
  `./<id>`, `--size` (640x480) and `--theme` (`olx-theme-dirt`) are a
  level's, and skin, theme and core packs need `--from`.
- `olr-cli level carve|fill|paint|stamp|resize <pack> …`, `level object
  list|add|move|remove <pack> …` and `level preview <pack> --out F.png
  [--scale 1..8] [--material]` (`level_cmd.rs`, plan 13d): the
  `olr_edit::level` operations. Each loads the pack, applies the
  operation, refreshes `thumb.png` and saves through
  `olr_edit::pack::save`; a refused operation writes nothing. Arguments
  are strict clap values naming their flag on error: `--rect X,Y,W,H`,
  `--circle X,Y,R`, `--at X,Y`, `--offset DX,DY` (negative values
  allowed), colours `RRGGBB` or `#RRGGBB`, `--tile`/`--image` PNGs read
  and decoded within `IMAGES` while parsing (regular files only, read at
  most `MAX_FILE_BYTES`). Messages print only once the save succeeded:
  `resize` prints `dropped: Spawn at (x, y)` per object it drops,
  `object add` `added object <i>`; `object add|move` warn on stderr about
  a spawn outside the level. `object list` reads `level.ron` alone
  (`read_level_header`) and prints `<i>: Spawn at (x, y)[ — notes]`, the
  notes' control characters escaped, and a light object (plan 15a) as
  `<i>: Light at (x, y), <intensity>%, reach <n|unlimited>`; `preview`
  crosses spawns only. `preview` draws `front` (or the
  material through `MATERIAL_PALETTE`) at art pixels × `--scale`, a white
  cross outlined in black on each spawn, bounded by `LEVEL_ART`, and
  refuses an `--out` inside the pack. Tests: `tests/level.rs`, and `tests/cli.rs`'s `pack_check_*`,
  `pack_fmt_*` and `pack_new_*`.
- `olr-cli level element list|add-image|add-layer|set|remove|raise|lower|draw|erase|merge-down|split|mask
  <pack> …` and `level bake <pack>` (`element_cmd.rs`, plan 14a; plan
  14d: `draw --material`, `merge-down <id>`, `split <id>` printing `added
  element <id>` per part, `mask <id> (--rect|--circle|--colour-at X,Y
  [--tolerance N]) [--restore]`, a restore taking a shape only; plan 14e:
  `fill <id> --at X,Y [--tolerance N] (--colour|--tile|--texture)
  [--material M]` (a seed outside the layer refused), `duplicate <id>`
  printing `added element <id>`, `--texture` an alias of `--tile` on every
  paint; plan 15a: `set --glow N` (0-100, 0 removes the glow) `--glow-group
  G` (a group of the level's look; a new glow needs both, either alone
  keeps the other and the painted mask: `glow_property`) `--clear
  true|false` (its rock and dirt let light through, still solid), and
  `glow <id> (--rect|--circle|--colour-at X,Y [--tolerance N]) --strength
  S` painting a glowing element's glow mask (`element::paint_glow`, a
  first mask whole; 0 erases) or lighting the connected pixels alike in
  colour (`element::glow_colour`, the user's playtest, 2026-10-19:
  tolerance 10 by default, `--tolerance` only with `--colour-at`, a glow
  without a mask then dark but for them); an element without a glow is
  refused, "set its glow first"): the
  `olr_edit::element` operations on a level's composition. Each loads the
  pack, applies, bakes (`compose::bake`, the back tile of the level's
  theme in `--content`, `content/packs` by default) and saves; the first
  on a plain level composes it (raster `base`). `list` prints `<i>: <id>
  <Image|Drawn|Raster> <material|->[ "<name>"][ hidden][ locked]`, bottom
  first, from `composition.ron` alone (`read_composition`); `add-image`
  reads its file bounded (`level_cmd::read_bounded`), applies its settings
  with the image (`add_image_with`) and prints `added element <id>`; `set`
  applies its flags in one step (`set_all`), unlocking first and locking
  last; `bake` prints `baked <pack>` and refuses a
  level without a composition. `level carve|fill|paint|stamp` refuse a
  composed level (ruling R1, message naming `level element draw|erase`);
  object commands keep its composition; `level resize --content` resizes
  the composition first (`element::resize`), then the level, then bakes.
  `pack check --content DIR` adds, for each valid composed level, an
  error per stale baked file (`<file>: differs from the composition's
  bake (run olr-cli level bake)`) or a warning when its theme is missing;
  both count in the summary. Tests: `tests/element.rs` (plan 15a:
  `cli_sets_and_paints_glow`, `cli_glows_pixels_by_colour`,
  `cli_sets_clear`).
- `olr-cli level look <pack> [--ambient RRGGBB:PCT] [--max-light P]
  [--steps N] [--softness N] [--soak N] [--solid-floor RRGGBB:PCT]
  [--exposure N] [--contrast P] [--saturation P] [--tint RRGGBB] |
  --clear`, `level group add|set|remove <pack> <id> [--colour own|RRGGBB]
  [--intensity P] [--reach N|unlimited] [--animation RON] [--phase MS]`
  (`set --rename NEW`, the elements naming it follow) and `level light
  add|set|remove <pack> [<index>] [--at X,Y] [--colour RRGGBB]
  [--intensity P] [--reach N|unlimited] [--radius 1..16] [--animation RON]
  [--phase MS]` (`look_cmd.rs`, plan 15a Task 7): the `olr_edit::look`
  operations, each checked by `compile_level` on a copy before it is kept.
  `level look` sets the given fields (a look made when the level has
  none); `--clear` removes the look and every group first (their elements
  stop glowing) and rebakes; there is no flag for `shadows` (soft by
  default; `level.ron` or the editor's Look panel). `group add` (needs
  `--reach`) prints `added group <i>`; `group remove` zeroes its pixels'
  strength, renumbers the groups above it, rebakes a composed level and
  prints `unglowed element <id>` per element that lost its glow. `light
  add` (needs `--at`, `--colour`, `--intensity`, `--reach`; radius 2 by
  default) prints `added object <i>`; `set` keeps what is not given;
  `set` and `remove` refuse an index that is not a light. Animations are
  RON, such as `Flicker(speed: 8, depth: 25)` or `Pulse(period_ms: 2000,
  low: 60, high: 100)`. Tests: `tests/level.rs`'s
  `look_cli_sets_every_field`, `groups_and_lights_round_trip_through_the_cli`,
  `bad_look_values_are_errors`.
- `olr-cli scenario [name...] [--dump F] [--all | --fields a+b] [--time F.ron]` replays oracle
  scenarios in `olr-sim` and prints the first divergence of each. `--time`
  (plan 8, `timing.rs`: `time_scenario`, `TimingReport`) instead times each
  frame's tick (a scenario bonus placed before it included; the trace
  record made outside the span) and records its projectiles and `rand()`
  calls, to set beside OLX's `ORACLE_TIMING` lines of the same scenario
  (`cargo xtask envelope`); it prints one line each (tick p50, nearest
  rank) and writes the RON. A trace with frame −1 only (OLX's
  `ORACLE_HEADER_ONLY`) is timed without comparing (`matches_olx: None`);
  a full trace is also replayed and compared on `olr_scenario::CHECKED`,
  and a divergence exits 1.
- `olr-cli render <scenario> --frame N --out f.png [--scale S] [--filter F]
  [--alpha A]` replays the scenario with an `olr-present` presentation (`olr-scene` and `olr-fx`)
  (camera on worm 0, OLX's default skin and colours, a 4:3 view of 320 ×
  240 world pixels) and draws frame N (the state after its tick; -1 is the
  start) offscreen with `olr-render`: `S` output pixels per world pixel
  (default 2), OLX's whole-pixel latest state unless `--alpha` interpolates.
  It draws with the presentation's look (`render_lit_to_image`; the
  imported OLX packs have none, so the frame goldens are unchanged).
  Use it to see what a change does; view the PNG.
- `olr-cli look-preview <pack> --at X,Y [--time MS] [--view lit|full|light] [--shadows hard|aa|soft] [--size WxH] [--content DIR] --out f.png`
  (plan 15a Task 13, `look_preview.rs`) draws a level's terrain alone (no
  worms) with its look at match clock `--time`, centred on `--at`, at
  `Viewport::GAME`, culling lights by `olr_render::LIGHT_CULL_PAD`. It
  reads the pack's level files strictly (`read_level_files`), its theme
  from `--content` (`content/packs`), and draws through the real renderer
  offscreen (`set_lighting`, `render_lit_to_image`) with IntegerNearest at
  854 × 384 by default; the renderer's warnings print on stderr.
  `--shadows` replaces the level's shadow filter (`LookFrame::shadow_filter`:
  `aa` is `Shadows::Smooth`; the comparison behind the user's choice of
  soft shadows, 2026-10-19). The fast loop for a look, and the look goldens
  (`look-lit.png`, `look-light.png`, `look-full.png`, `look-t500.png`:
  `fixtures/look/levels/look-test` at 256,128). Tests:
  `tests/look_preview.rs` (`look_preview_writes_a_png` edits a copy of
  Dirt Level with `level look` and `level light add`; `render_match_writes_a_png`).
- `olr-cli render-match <match flags> --out f.png [--scale 2] [--packs DIR]... [--content DIR]`
  (plan 15a Task 16, `render_match.rs`) plays a headless bot match (the
  runner's `advance`, unsettled: no invariants, no recording) on
  `--content` with each `--packs` directory laid over it (the first wins;
  any packs, such as `fixtures/look/mods` and `fixtures/look/levels`,
  which no OLX trace replays) and draws its last tick as the game does,
  look included; prints the number of lights. `--scale` is in (0, 16];
  worms wear the default skin and colours, worm 0 the local one; the
  packs' warnings print on stderr. The rocket golden (`look-rocket.png`)
  is chaos bots with the look test mod (`--mod look-test-mod --level
  look-test --bots 3 --personality chaos --seed 1 --ticks 300`); a bot
  change moves it (re-bless with a reason).
- `olr-cli match --mod <id> --level <id> [--mode deathmatch] --bots N
  [--difficulty easy|medium|hard|xtreme|mixed] [--personality dm|chaos]
  --seed S --ticks T [--bonus-frequency F] [--kill-limit K] [--time-limit M]
  [--record f] [--coverage] [--no-verify] [--failures DIR] [--p99-budget-ms B]`
  plays a headless Deathmatch of bots (spec §7.1): invariants every tick
  (`invariants.rs`; a worm's centre may sit in rock where OLX's collision
  wedges it, but never with rock all over its collision cross:
  `buried_in_rock`), the end, p50/p99/max tick time (bots + sim), the bots'
  work units; then plays it again (determinism) and replays its recording
  from the content (round trip). `--coverage` prints weapons fired, actions
  triggered, events, bonuses, rope attachments and kills by the killer's
  selected weapon. A failure (a panic in a bot or the tick, an invariant, a
  budget, a divergence) writes `<stem>.olrp` and `<stem>.ron` (the spec, so
  the seed, and the tick) to `target/failures/` (spec §7.3) and exits 1.
- `olr-cli replay <file> [--until N] [--dump]` plays a replay on its content,
  checking every checksum; `--dump` prints the world (before tick N) as JSON.
- `olr-cli rollback-cost [--mod olx-classic] [--level L] [--worms 16] [--depth 12]
  [--samples 300] [--out f.ron]` (plan 9, `rollback_cost.rs`): 500 warm-up ticks
  of chaos bots, then every 10 ticks save, play `depth` ticks, time `restore`
  and the re-simulation with the recorded keys (bots not timed), nearest-rank
  percentiles, checksum checked after each; prints `RollbackCost`.
- `olr-cli stress [--mod M] [--level L] [--worms 32] [--pool 3000] [--seconds
  180] [--seed S] [--render-every 100] [--render-width W --render-height H]
  [--no-verify] [--fx-cap 16384] [--build-every 0] [--memory-ceiling-mib
  4096] [--record f.olrp] [--personality chaos|dm] [--out report.ron]`: the
  engine stress test (plan 7 task 8, `stress.rs`;
  `stress::Config::new(spec)` gives the defaults): `stress::spec` (32 bots,
  mixed difficulties, `LoadingTime` 0, bonuses every `BONUS_FREQUENCY` s, no
  limits, the pool at `--pool`) played by the runner (invariants every
  tick), its presentation's entities stepped every tick and a frame rendered
  offscreen (lavapipe/llvmpipe) every `--render-every` ticks; asserts
  bounded buffers (pool, the entity pool, `TickEvents::CAPACITY`: fixed
  capacities today, so cheap asserts against a future change) every tick and
  memory under `--memory-ceiling-mib` (`MEMORY_CEILING_MIB`; `VmHWM`, read
  every 1000 ticks; an absolute stop, `xtask bench` gates the peak against
  its baseline), then `verify` (determinism and the replay). The bots are
  chaos bots (they fire everything) unless `--personality dm`
  (`Config::personality`, which replaces the spec's) plays OLX's Deathmatch
  bots, whose routes go through the shared area cache (chaos bots never
  touch it): the envelope's map-size and art-scale sweeps use them. The
  performance envelope's knobs (plan 8) never touch the match: `--fx-cap`
  sizes the entity pool (`Presentation::set_fx_capacity`; play's
  `olr_fx::PLAY_ENTITIES` by default), `--build-every N` builds and times the
  draw list every N ticks, `--record` writes the match's replay. `Report`
  (RON with `--out`): tick, sim and think µs (p50/p99/max; `Stats::think_ns`
  splits the bots' share), `present_us` (`on_tick`), `build_us` (the draw
  list), units, pool peak/mean/ticks at the cap, entity peak/ticks at the
  cap and `fx_capacity` (the pool as built), events a tick, drops, the frame
  ms, peak MiB, `personality`, `area_cache` (held, capacity), `level`
  (width, height, art scale) and `caps` (hard limits reached, such as a full
  area cache); the new fields are `#[serde(default)]`, so older reports
  still read. A failed match writes its replay to `target/failures/`. `cargo
  xtask stress` runs both mods × pool 3000/20 000 on Castle Strike.
- `olr-cli net-sim [--mod M --level L --seed N --slots 8 --clients 3
  --spectators 1 --join-tick T --profile typical --drift-ppm P --ticks 1000
  --outage peer:from:until --net-seed N --difficulty D --exit
  peer:tick:crash|leave --host-freeze from:until --profile-change
  tick:peer:profile --migration] [--from CASE.ron]
  [--seeds a..b] [--content DIR] [--failures DIR]` (plan 9, `netsim.rs`, the
  command in `net_sim_cmd.rs`): an online match of bots in one process over
  `olr-netcode`'s network model. Every peer (host, clients, spectators) builds
  its own `World` and area cache from the same `MatchOptions` (all worms
  `Player::Bot`); a peer runs only the bots of its local slots (the host
  `clients..slots`, client `k` = peer `1 + k` slot `k`), thinking with its
  own last tick's events (the world and the bots from
  `olr_online::matches::build_options` and `HostBots`: the online game's
  one implementation). Prints per peer rollbacks (depth p50/p99), fills,
  stalls, desyncs and bytes/s received, then `ok` or `FAIL <what> at tick
  <t>`; a failure writes `target/failures/net-<stem>.ron` (a `NetCase`) and
  exits 1. **Host migration** (plan 12c, spec §3.7; `NetSpec`'s `exits`,
  `host_freeze`, `profile_changes`, `migration`, all `#[serde(default)]`
  so older cases load, each a harness `Scenario` field; `--exit` and
  `--profile-change` repeatable, every value checked by its parser so a
  bad one is an error naming the flag, and again by `scenario`, the spec
  being untrusted): an exit naming the host (peer 0) or a host freeze
  turns migration on (`MigrationPlan::default()`: every worm a bot, no
  `host_human`); the succession is the harness's (clients in order, then
  spectators). `WorldFactory::host_driver` keeps the peer's own bot
  (`PeerBots`: the hosted bots and a client's own, shared with the
  factory through an `Rc`, pressed through `HostBots::press`'s human
  callback) and restarts the host's bots (the slots no client plays) as
  `olr_online::matches::restart_bots` does: `Bot::with_cache` on the
  current world, the match's seed, one new `AreaCache`. One line per
  migration, `migrated: <from> -> <to> at tick <t> (<ms> ms after the
  host was found gone)`, and `host lost: <peers>` (in `summary`).
  **Stalls (Task 8's RS-2, Task 11's SK-1):** a host gone stalls its
  players at the prediction limit before they find it gone, an episode
  lasting until the new host's resume reaches them, and an outage stalls
  the peer it cuts off. So with migration or an outage in the spec the
  verdict (`judge`) runs the harness's without `expect_no_stalls` and then
  judges every stall episode of the host and the clients (not dropped) as
  `StallTracker` (an observer beside the bundle recorder) saw it: its start
  (`Event::Stalled`'s tick) and its end, the current host's tick when the
  player ran a tick again (the first host's, from each resume the new
  host's: not the furthest any host reached, since an old host cut off
  from its peers plays on ahead, left behind; soak case
  `lan-10s1c0v-seed3202236798`). An episode must begin and end inside one
  excused window (`excused_windows`, `stalls_within_windows`): a migration's,
  from the departure (the exit's tick or the freeze's start) less
  `MAX_PREDICTION` to the later of the departure and the resume tick plus
  `MAX_PREDICTION`, excusing every player (the exit is the migration's when
  its peer went before the resume, `Report::exited_us`: a new host behind
  the match resumes at its log's frontier, below the departure tick,
  RS-1a; `a_migration_resumed_below_its_departure_tick_keeps_its_departure`); or an outage's, `from - MAX_PREDICTION ..= until + recovery`
  (a peer's overlapping outages joined first, `merged_outages`: the
  network cuts the link until the last is over; SK-4's
  `typical-8s4c0v-seed2732616080`)
  (SK-2: `Profile::outage_recovery_ticks` of the outage on the peer's
  slowest link, `slowest_profile`, with `CLIENT_FRAME_US`, at least
  `MAX_PREDICTION`) excusing only the peer it cuts off (everyone's while that peer is the
  current host, `host_at`; and every player whose slowest link stalls on
  the host's fills for the peer, `fills_stall`: two trips plus
  `FILL_WINDOW` and `TARGET_MARGIN` reaching `MAX_PREDICTION`, true on
  `bad`, soak case `bad-16s4c0v-seed3660989505`); an outage of a
  spectator excuses nothing. Per client crash (N87, ruling PF12d-5) a
  window as an outage of the crashed peer, from the exit's tick (a host
  tick already) to `SILENCE_TIMEOUT_US / TICK_US` plus the recovery of an
  outage that long on its slowest link (at least `MAX_PREDICTION`): the
  host fills its slot with its last keys until the timeout withdraws it,
  which stalls the players whose links cannot bear the fills
  (`fills_stall`); a client crash sends the run through `judge`'s own
  stall rule too. A client's leave has none (withdrawn at once); the unit
  test `a_client_crash_excuses_the_fills_stalls_until_its_timeout` pins
  it with its negative twins (no crash, a leave, before, past).
  Outages are in virtual time, windows in host ticks: `host_tick_at`
  converts (from the last migration's resume, or less a host freeze that
  did not migrate; a time during a migration, from its `silent_us` to its
  resume, is the migration's tick: SK-4's `typical-3s3c1v-seed2454907585`). A migration called off (N69: the old host heard
  again) has no window of its own: only an outage (the peer's, or its
  host's while that peer hosts) silences a host 2 s on a profile up to
  `bad` without a step-down, and that outage's window covers it (the kept
  case `bad-4s3c1v-seed562805195`). The last episode of a player that
  went (host lost, crashed, left) never ends and is not judged (soak case
  `good-5s4c2v-seed1973712135`: host lost in a 5.5 s outage during the
  migration). Any
  other stall, a longer one or one that never ends fails. Since
  olr-netcode's resume settles the time sync (RS-1), no profile stalls after a
  resume. `n66` always runs (bounds each migration by N66 (`N66_US`) when
  nothing but hosts left (the first host or one that resumed: a successor
  that committed and crashed before its resume makes the others wait out
  its window, soak case `typical-14s4c1v-seed1857358438`): no other exit,
  outage or profile change; within
  3 s from the first `HostSilent` or `PeerLeft` to the resume on `lan`,
  `good` and `typical`, within `WINDOW_US` on `bad` and `awful`). Only a
  profile change to `awful` (or an `awful` base) turns the stall rule off
  (`expect_no_stalls`). A player whose link changes to a worse profile than
  the match's (ping plus jitter; its input delay was chosen on the
  match's, fixed per match) is not judged from that change on
  (`worsened_from`, ruling SK-6; net-soak's `lan-10s6c1v-seed335636286`),
  and such a change sends the run through `judge`'s own stall rule. `fixtures/net/cases/*.ron` are kept cases that must pass
  (`tests/net_cases.rs`); `cargo xtask bless-regression --from
  target/failures/net-*.ron` keeps one. `tests/net_sim.rs`: in `check`, two
  short runs, a spectator joining, `a_host_crash_migrates_a_real_match`
  (typical, the host crashes at tick 400 of 1000: `hosts` `[0, 1]`, a
  restarted bot plays on; fails without the stall rule or without the
  restarted bots) and `a_frozen_host_steps_down_in_a_real_match` (good, a
  1.7 s freeze: the host exited, no peer ever `HostSilent`; a 2.4 s freeze
  fails it), about 1.5 s each; in `full`, the profile grid and
  `net_migration_grid` (5 profiles × a crash, a leave and a 2 s freeze at
  tick 800 of 2000, seed 1, MW 1.0 for the freezes; about 29 s). Plan
  12d (check): `a_client_leave_withdraws_its_worm_in_a_real_match` (the
  host's withdrawal of slot 1 replayed on a real world holds the worm
  out), `a_client_crash_on_bad_passes` (a client crash on `bad`, 1600
  ticks: fails without the crash window, "player peer 3 stalled") and
  `a_bundle_with_a_withdrawal_reproduces_to_the_fault` (a leave at 200, a
  fault at 500: the bundle holds the withdrawal, `reproduce` first
  diverges at or after 500 and the host's state is equal; without the
  withdrawal it diverges at 209).
  `net-sim --feel <out.ron>` measures the **feel numbers** instead (plan 9
  task 11, spec §9.2, `feel.rs`): `feel_spec(profile, seed)` (16 worms, 7
  clients, Classic on Castle Strike, 3000 ticks) for every profile ×
  seeds 1 and 2 (per metric the worse seed) and `bandwidth_spec()` (32
  worms, 1 client, typical), in parallel threads (about 40 s in release);
  one line per profile and a `FeelReport` (RON). Every number is virtual
  time from seeded runs: the same on every host. They cover the match
  (until the host reaches its last tick; the settle ticks are left out):
  rollback depth p50/p99 (`to - from` of every client's `Rollback`,
  nearest rank), `correction_px_p99` (at each client frame the displayed
  tick `D = current - 1` and every worm's position are remembered; a later
  re-simulation of `D` (`TickInfo::resim`) records the largest jump of a
  worm other than the client's own, if above 0), own corrections per
  minute (`Stats::own_corrections`, the worst client), stalls (every
  player's stall episodes, `Stats::stalls`, to the end of the run) and
  bytes per second down and up (the worst client;
  `Stats::bytes_in`/`bytes_out`, `OVERHEAD` per datagram included). A
  failed verdict is an error. `cargo xtask net-feel` gates them (see
  xtask); `tests/feel.rs` checks the definitions on a short run.
- `olr-cli net-soak [--minutes 10] [--seed S] [--transport-every 5] [--runs N]
  [--content DIR] [--failures DIR]` (plan 12c Task 11, N77, `soak.rs`,
  the command in `net_soak_cmd.rs`; `cargo xtask net-soak` runs it, not a
  `full` step): seeded online cases one after the other until the time
  (or `--runs`) is up (checked between cases, so up to one case late; `--minutes`
  must be above 0). The seed defaults to the clock and is printed first
  (`net-soak: seed N`), `--seed N` replays it. `soak::case(seed)` is a pure function of the seed
  (all draws from `olr_netcode::Rng`): both mods, the 4 levels, 2-16 slots,
  1-min(slots, 7) clients, 0-2 spectators (one in two arriving mid-match),
  a base profile, 0-3 profile changes, 0-2 outages of 0.5-6 s, in half of
  them a host crash, departure or 1.6-2.5 s freeze at a random tick in the
  middle 60 %, in one in four with two clients or more a client's
  departure, a leave or a crash (plan 12d: withdrawn at once or at its
  timeout, the crash window excusing the fills, PF12d-5; drawn last, so
  the other draws of a seed stay; client exits turn `n66` off for that
  case), 1500-6000 ticks; `case_short` (tests) the same with 600.
  Case `n` (from 0) has seed `seed + n`; every `--transport-every`th run
  (the 5th, 10th, ..) is `soak::transport_case(seed)`: 2-4 clients through
  good, typical or bad, 0.3-0.5 minutes, 127.0.0.1 only, in half of them a
  server freeze or departure at 25-70 % of the match (migration on, the
  server's own bots playing on), the others clean control runs (migration
  off; N77's "every transport case has a server exit" is the brief's
  half). One line per case, `run <n> seed <s> <stem>: ok (<secs>s)` or
  `FAIL <what> at tick <t> (<file>)` (no tick for a transport or error case), then `soak: <runs> runs (<k>
  transport), <f> failures in <m> min`; exit 1 if any failed. **Bounded:**
  each case's report is dropped before the next, a failure is a file (at
  most `MAX_FAILURES` 32 per invocation: files of earlier runs stay and do not count; then the soak stops with its summary; a failure that cannot be written is reported, counted and the soak goes on), nothing
  else accumulates (`tests/net_soak_memory.rs`, its own `[[test]]`: bytes
  left allocated, by `allocation_counter`, after 3 and after 9 runs of 3
  cycled seeds within 256 KiB, `VmRSS` within 16 MiB as a backstop). A `net-sim` failure
  is `net-<stem>.ron` (`netsim::write_failure`); a case that cannot run,
  or panics, `soak::write_error_case` (a `NetCase` whose note names the
  error, since `write_failure` keeps only failed verdicts); a transport
  failure `soak-transport-<seed>/` with `spec.ron` (`TransportSpec`, now
  serde: its `difficulty` by name, `easy`..`xtreme`, through
  `difficulty_name`), `failure.txt`, `server.log` and any desync bundles.
  `run_cases(cfg, cases, out)` takes the case function (tests; a transport
  case gets a temporary data directory of its own). The first minute found
  that an outage of 0.5 s or more stalls the peer it cuts off on any
  profile: outage windows excuse that (SK-1, above).
  `tests/net_soak.rs` (check): the cases' determinism and bounds (200
  seeds, every exit kind, every profile valid; the host's exits matched
  apart from the client departures, 25-75 of them, leaves and crashes), a short soak's lines and
  summary, a failing case kept and counted (and the cap), the error
  writer's round trip, `TransportSpec` through RON and a real outage match; `netsim`'s unit
  tests pin the excused windows, the negative twins included.
- `olr-cli net-client --bot (--join <host[:port]> | --invite olr1… | --key
  <64 hex> | --listed "<name>") [--relay URL]... [--stun host:port]...
  [--turn user:pass@host:port]... [--no-nostr-turn] [--region R]
  [--allow-local] [--lobby-only] [--no-standby] [--difficulty
  easy|medium|hard|xtreme] [--name N] [--matches 1] [--spectate] [--impair
  <profile>] [--impair-seed S] [--timeout 3600] [--content DIR] [--cache
  DIR]` (plan 10,
  spec §9.4, the command in `net_client_cmd.rs`, the client in
  `transport.rs`): a headless bot client over real connections (port
  23400 by default). Exactly one way to the game (plan 12b, N46; `target`
  resolves it, unit-tested): `--join` an address, directly only;
  `--invite` its address first (when the code has one), then through the
  relays (`join_relays`: the code's hints, then the `--relay`s; neither an
  address nor a relay is an error), its key expected; `--key` through the
  `--relay`s only (one needed); `--listed` subscribes a `GameList` on the
  `--relay`s (one needed; `allow_local` with `--allow-local`), waits up to
  10 s (`LISTED_WAIT`, never past `--timeout`) for a compatible game of
  that exact name and joins it as the game's Internet tab does (its
  announced address first, then through the `--relay`s, its key
  expected). There are no default relays or STUN servers: nothing public
  unless named. A join through the relays gets the ICE settings (`--stun`,
  `--turn`, `--allow-local`; a direct one none); its `NostrJoin` takes
  `--region` (default unknown), `--no-nostr-turn` and `--allow-local`
  (N62: listed games, TURN notes and relayed addresses may be local or
  private). Once the link opens it prints `path: <tag>` on stdout
  (`olr_online::join::path_tag`: direct, open port, punched, relayed) and
  the host's end on stderr; `--lobby-only` exits 0 once in the lobby (no
  ready, no match). Leaving waits at most 1 s (`SHUTDOWN_WAIT`) for the
  endpoint's shutdown (`OnlinePeer::shutdown`, ruling NT-2; a client maps
  no port today, so it ends at once, untested). The tests here join
  directly only; a join through the relays from the CLI first runs in
  the Docker NAT tests (`cargo xtask net-nat`). It joins, readies whenever the lobby gathers and it
  wants another match (or asks to watch, `--spectate`), plays with a bot on
  its slot (`Bot::new` for its slot and the start's seed, thinking in
  `on_tick` on the world each newest tick leaves: a re-simulation of an
  older tick changes nothing; `local_keys` hands out the last keys), and
  prints each finished match: `match <n>: <mod> on <level>, seed S, slot
  K|watching, <T> ticks|joined mid-match, <C> checksums, replay <B>
  bytes|no replay`, then `  checksum <tick> <16 hex>` per confirmed
  checksum. **Host migration** (plan 12c) is on by default, a change of
  behaviour: a client whose server stops or restarts mid-match no longer
  ends with "disconnected" (exit 1) but carries the match on with the
  others, or takes it over as the first successor (alone, if it was the
  only client, only when the server said `Bye`: ruling FR12c-1, kept by
  FR12d-Q's majority quorum, a lone
  client cannot tell a crashed server from its own drop and is host lost);
  `--no-standby` never takes it over but still follows it
  to another player. Its configuration (`migration_config`): a standby on port 0 (through `--impair`) asking
  the `--stun` servers, the `--relay`s to listen on once it hosts (none:
  no Nostr), its joins to a successor through the relays with the
  `--stun`/`--turn` ICE, `--region`, TURN notes and `--allow-local`;
  `--no-standby` follows only (never hosts). It prints `migrated: <name>
  hosts` (or `migrated: hosting` when it took the match over) and `host
  lost: <why>` (then exit 1) on stdout (`migration_line`). A client that
  took a match over hosts on until its members left (they finish their
  match on it), bounded by `--timeout`. `--impair` sends its datagrams through a netsim profile. What
  `--content` lacks of a match it downloads from the host (plan 11) into
  `--cache` (by default a fresh temporary directory, removed at exit).
  Exits 1 when refused, kicked, disconnected or past `--timeout` seconds;
  every wait is bounded by it (the name resolves on its own thread; olr-net's
  TCP connect and signalling never block the poll loop).
- `olr-cli nostr-relay [--listen 127.0.0.1:0]` and `olr-cli stun-server
  [--listen 127.0.0.1:0]` (feature `nat-tools`, `nat_tools.rs`; plan 12b,
  the Docker NAT tests' WAN infrastructure): olr-discovery's
  `TestRelay::start_on` (honest) and `StunResponder::reflecting` (answers
  each binding with its observed source) as processes. The first stdout
  line is `nostr relay: ws://<addr>:<port>` or `stun server:
  <addr>:<port>`, loopback for an unspecified address (`local`; a caller
  listening on every interface, as in Docker, names the reachable
  address itself); then they serve until killed.
  Loopback by default (tests and e2e send only to 127.0.0.0/8). The
  feature enables `olr-discovery/test-relay`; a build without it has no
  such commands, and no release build has it (`xtask dist` does not build
  olr-cli). `xtask e2e` builds olr-cli with it in a cargo run of its own (so
  olr-game and olr-server keep olr-discovery without the test relay).
- `olr-cli gpu-bench --replay F [--sizes 1920x1080,2560x1440,3440x1440,3840x2160]
  [--start-tick 1000] [--frames 600] [--probe-passes 0,1,2,4,8,16,32]
  [--probe-taps 9] [--probe-frames 120] [--adapter hardware|software]
  [--backend auto|gl|vulkan|dx12] [--content DIR] [--out F.ron]
  [--list-adapters]`: the GPU half of the performance envelope (plan 8,
  `gpu_bench.rs`, the command in `gpu_bench_cmd.rs`). A recorded match
  (`stress --record`) on the assets its packs name (`LevelAssets`, so a
  generated level too) is played unrendered to `--start-tick`, then each
  measured tick and its frame is drawn offscreen at each size on
  `Offscreen::open` (the view, `Viewport::GAME`, over the whole target, no
  HUD: ruling P11), the device waited for after each frame; then the
  shader-headroom probe (`olr_render::Probe`, 0 passes first: the
  baseline, which runs the probe's untimed seed of the drawn frame too) for `--probe-frames` frames at each pass count. `GpuReport`
  (RON with `--out`; `cargo xtask envelope` reads it): adapter, backend,
  device type, `timestamps` (GPU times from timestamp queries, else CPU
  wall times only), scene, start tick and a `SizeReport` per size:
  `tick_ms` (the world's tick and `on_tick`), `cpu_ms` (draw list, uploads,
  encoding and submit), `frame_ms` (the draw list to the device idle),
  `total_ms` (tick and frame), `gpu_ms` (the world and upscale passes, with
  timestamps; a frame whose span was not written or runs backwards is
  left out and counted in `gpu_dropped`, never a 0 ms sample) and `probe`
  (`ProbePoint`: the probe passes' GPU ms, or the frame's wall ms over the
  baseline's median, `timed_on_gpu` saying which; its own
  `gpu_dropped`). `check_adapter`: a hardware run that got a software rasteriser
  (`DeviceType::Cpu`: llvmpipe, lavapipe; or one wgpu's GL backend reports
  as `Other`, known by name, `software_name`: Microsoft's Basic Render
  Driver (WARP, also behind Mesa's d3d12), softpipe, SwiftShader) is an error naming WSL's
  `GALLIUM_DRIVER=d3d12 WGPU_BACKEND=gl`; `--adapter software` allows it.
  At the end it plays on to the next recorded checksum and compares, so
  the scene drawn is the recorded match. All device work stays on the
  main thread (WSL's d3d12 driver crashes when a thread that used the
  device exits). Percentiles are nearest rank (`stress::Spread::of`).
  Run times seen in the envelope on the user's machine (2026-10-12,
  release, Ryzen 5 7600X, RTX 5070 Ti): a `stress` sweep point (20 s of
  match, 16 bots, `--no-verify`) takes well under a second, a 4096 ×
  4096 level about 2 s and art scale 8 about 5 s (5.1 GiB peak); a
  `gpu-bench` of the heavy replay at four sizes with the probe about
  11 s (WSL GL), 10 s (DX12) or 8 s (Vulkan), a level's replay about
  1 s (art 8: 3 to 8 s).
- `olr-cli nostr-probe [urls…] [--wait-min 3]` and `olr-cli stun-probe
  [servers…]` (plan 12a Task 8, `nostr_probe.rs`, library and commands):
  measure public relays and STUN servers to choose `olr_discovery::defaults`
  (the default lists when none are named). **By hand only: the relay probe
  publishes to public relays**; no test runs it against the internet. Per
  relay, two pool connections: a kind-31470 note (its `OK` and round trip)
  and a kind-31471 note (TURN relays, plan 12b), each read back at once on
  the second and after the wait on the first, and a kind-21470 event of
  `MAX_HANDSHAKE_CONTENT` (16 KiB of random base64, `probe_handshake`)
  forwarded from the first to the second. Every event is a
  probe event: a fresh random key per run (`probe_key`, never the player's
  identity), content `"olr relay probe"` (the handshake's random),
  tags `d` and `t` = `open-liero-reloaded-probe`, expiry 10 minutes ahead
  (`probe_event`). A relay passes when it accepts both notes, keeps them
  (both reads), forwards and refuses nothing
  for payment or auth (`needs_payment_or_auth`: `restricted:`,
  `auth-required:`, or naming payment or a whitelist). The STUN probe asks
  over IPv4 (3 tries of 1 s) and keeps the RTT, the mapped address's family
  and whether it is global, never the address (the prober's own IP). Both
  print Markdown tables and exit 1 unless everything passes.
  `tests/nostr_probe.rs` checks the verdicts against `TestRelay`
  misbehaviours (`Refuse`, `RefuseKind`, `KeepFor`, `NoForward`) and
  `StunResponder`.

**The library** (`src/lib.rs`) holds the match runner for the commands and
the tests: `spec::MatchSpec` (the options, serialized in failures and
goldens; `match_options` gives what `olr_content::build_match` assembles,
the same builder as `olr-game`'s Deathmatch), `runner` (`Match` stepwise: `advance` = bots think + tick, no
allocation; `settle` = record, invariants, coverage; `step` catches a panic,
and a panic inside `World::tick` rebuilds the world from the logged keys so
`Recorder::finish_failed` saves a replay that plays into the failure;
`play`), `verify` (determinism and replay round trip), `stress` (the
stress test's runner and `Report`), `gpu_bench` (the GPU bench's runner
and `GpuReport`), `timing` (`scenario --time`), `coverage` (required
weapons and action kinds derived from the mod: `olr_sim::projectile_actions`
and `World::actions_triggered`), `failure`, `golden`, `assets`,
`genlevel`, `netsim` (`NetSpec`, `WorldFactory`, `run`, `NetCase`), `feel`
(`measure`, `measure_one`, `FeelReport`), `transport` (below).

**Real transport** (`transport.rs`, plan 10, spec §9.4):
`run_bot_client(content, ClientSpec, stop, progress)` is one bot client
(`net-client`; `ClientSpec::join`, an `olr_online::Join`, with
`expected_host`, the endpoint's `ice` and `lobby_only`, plan 12b;
`ClientSpec::cache`, its download cache's directory or
`None` for no downloads; `Progress::Connected(path, remote)` once its
link opens; a `ClientRun`: its `PeerMatch`es (start, slot, confirmed
checksums, record, replay), why it was `dropped`, desync bundles). A
match is finished once it ended and olr-online kept its record (the
replay then awaited), or at its end for a spectator that joined mid-match
(its session's checksums as last seen: olr-online hides the session once
the match ended; no record, no replay). `run_transport(content_dir,
TransportSpec)` runs `olr_server::run_with` in a thread on port 0 (its
`last_record` after each match, its log in a `SharedLog`) and the
clients in threads on 127.0.0.1 (players `Client N`, then spectators
`Watcher N` joining a set time after the first player's match started),
the clients' datagrams through the spec's netsim profile (`EndpointConfig::impair`,
seed `net_seed + i`), all bounded by `timeout` (the match and 40 s);
each client's download cache `TransportSpec::client_cache(i)` in the
data directory, and `client_content` a content directory of its own for
some (a partial copy: they download the rest);
`TransportSpec::new(players, minutes, profile, data_dir)` (Classic on
Dirt Level, one time-limited match with bonuses, no fill bots: a match
needs two worms; `fill_bots` adds the server's). `TransportRun::verdict`:
the server finished its matches; no client dropped (unless `may_drop`
names it) and each finished as many; per match (matched by start) a
player's confirmed checksums and input rows equal the server's, its replay
bytes equal the server's (made from its record with
`olr_online::replay::make`), a late spectator's checksums equal the
server's at their ticks and reach the match's end; no desync bundle; the
server's replay played back (`replay_checksums`, its withdrawals before
their ticks, plan 12d) meets every server checksum; every server record holds at least `min_checksums` (at least
one: an empty record proves nothing). The server observer tells a new
record by a cheap key (seed, start checksum, rows, checksums), not by
comparing whole records every poll.
**Migration** (plan 12c): `ClientSpec::migration` (a `MigrationConfig`);
`TransportSpec::migration` (`false`: every client `MigrationConfig::off()`,
D7, so the transport, stress and feel gates gain no failure mode; `true`:
`direct()` with the standby through the client's impairment) and
`server_exit: Option<ServerExit { after_s, how: Freeze | Leave }>`
(serde): `after_s` after the first match started (a client's `Started`)
the server's observer takes `Server::confirmed_rows()` (`TransportRun::
server_rows`) and freezes (stops polling and stays until the run ends,
holding its sockets: a hung host) or sets the server's own stop flag
(`Server::shutdown`, `Bye`); the run then waits for no server record.
Each `MigrationLog` line goes to stderr as `migration: …` (FR12c-4).
`ClientRun` gains `migrated` (`"<name> hosts"` per migration, in order),
`host_lost` (then `dropped` too) and `held` (per slot `known_upto` at its
first `Migrating`); `Progress` gains `Migrated { host, me }` and
`HostLost(why)`. A server exit plays one match (`run_transport` refuses
`matches > 1`: a client-turned-host starts no further match). With a server exit the verdict
(`verdict_after_exit`) names no successor (the clients join
concurrently): no client dropped, each finished its matches through one
migration named alike by all, their records (start, rows, checksums) and
replays agree, the replay made from the first's record meets its
checksums (at least `min_checksums`), no desync bundle, and the server's
rows as it went agree with theirs per slot up to the furthest any client
held (pre-flight B3; in the runs so far never load-bearing: the server's
confirmed rows had all reached the clients), a comparison of nothing
(held prefixes of zero) failing.

**Generated levels** (`genlevel.rs`, plan 8; bench only, a game never
loads one): `GenLevel` makes seeded noise caves in memory, no pack written,
from its id alone: `gen-<width>x<height>-a<art scale>-s<seed>` (such as
`gen-2048x2048-a1-s1`), which `--level` and a replay's level pack id hold.
`GenLevel::parse` refuses anything else: sides `MIN_SIDE` 64 to `MAX_SIDE`
4096, art scale 1 to `MAX_ART_SCALE`, side × art scale at most
`MAX_ART_SIDE` 16 384, the canonical spelling only (ids are untrusted; plan
8 ruling PF7). Its theme is Castle Strike's (`THEME_LEVEL`). A level whose
colour image a pack loader would refuse (`MAX_COLOR_MAP_BYTES`) is still
made: `content_cap` names the cap, which a stress `Report` lists in `caps`
(reported, not enforced).

**Assets** (`assets.rs`): `LevelAssets::load(content, mod, level)` is what
`Match::new` (through `Match::with_assets`), `verify`, `stress` and
`gpu-bench` load through: a pack level exactly as `Content::build_match`
loads it (so goldens do not move), or a generated level with the theme's
and mod's pack references and the hash `GENERATED_HASH`. An id starting
`gen-` that does not parse is an error before anything is made.
`replay_world` checks a replay's packs (a pack level's against the
content first, as `Content::world_for_replay` does; then, for both kinds,
they must equal the assets': a replay on other assets is an error, not
another match) and makes its start world.

**Layer 6.** `dump-state` comes later (spec §7.4).

Test: `cargo nextest run -p olr-cli` (the dev-dependency on
`olr-net/test-hooks` keeps the port mapper off real routers in tests; a
dev-dependency on itself with `nat-tools`, olr-discovery's pattern, gives
the test build the dev tools, so `check`'s plain `nextest --workspace`
runs their test; `cargo build -p olr-cli` builds without them). `tests/cli.rs`
(check): `net_client_joins_by_invite_through_a_test_relay` (an in-process
Invite olr-server, `public_address` 127.0.0.1 with `allow_local`, the
`TestRelay` its code's hint: `--invite --lobby-only` prints `path:
direct`), `net_client_joins_a_listed_game` (a Public one found by
`--listed` on the relay; a name nobody announces fails within
`--timeout 1`), `net_sim_migration_flags_run_and_replay` (plan 12c:
`net-sim --exit 0:300:crash --ticks 800` prints `ok` and `migrated: 0 ->
1`, the same spec kept as a `NetCase` plays the same output with
`--from`, bad `--exit`, `--host-freeze` and `--profile-change` values are
errors naming the flag; about 1.6 s),
`nostr_relay_and_stun_server_run_until_killed` (the
binary's two dev tools on loopback answer a REQ with EOSE and a binding
with the asker's address, then are killed); the unit tests of
`net_client_cmd.rs` check the target's resolution, the migration
configuration (a standby unless `--no-standby`) and the printed lines. The tests are modules of one binary,
`tests/integration.rs`, except those xtask runs by name (`golden`,
`net_cases`, `grid`, `net_sim`, `transport`, `render_golden`, `gpu_bench`) and
`net_soak_memory` (it reads the process's memory): one module's
tests are `cargo nextest run -p olr-cli --test integration corpus::`.
`tests/corpus.rs` checks every imported
pack when `OLR_CONTENT_DIR` is set. `tests/playtest.rs` checks the hand-made playtest packs
(`content/playtest/`, such as the lit Castle Strike) with `pack check
--content`, and on the GPU tier plays a bot match on it (`render-match
--packs content/playtest`). `tests/digest.rs` pins a SHA-256 of what
the game receives from every bundled pack (each skin's drawn frames, the
images the runtime uses, level and theme data; plan 13c): a pack format or
importer change that alters a drawn pixel changes the snapshot. To accept a reviewed snapshot change, run
`INSTA_UPDATE=always cargo test -p olr-cli --test integration corpus::`.
`MatchSpec`'s `--loading-time` and `--pool-capacity` (serialized only when
given, so goldens and failure records keep their form) are the stress
test's; without them a match is OLX's (`LoadingTime` 100, pool 3000).
`tests/stress.rs` (check) plays a 4 s stress run on a 200-slot pool, an
8 s one with and without the envelope's knobs (same match, more measured),
a 4 s Deathmatch-bot one that fills some of the area cache, and chaos and
Deathmatch runs on a small generated level (`gen-256x256-a2-s1`; the large
ones run only in the envelope, ruling PF3), and a replay refused on
assets other than its own, a pack level's and a generated one's.
`tests/hits.rs` (check): in seeded bot matches of both mods no projectile
hit or explosion injury comes from outside the struck worm's current hit
geometry (`World::hit_audit`), some do with `SimSettings::olx_quirks` on,
and the investigation's phantom kill (Classic, Dirt Level, 2 bots, seed
2: the control finds the first lethal hit on the worm a stale hit struck
in that tick, `HitAudit::last_stale_worm`, tick 4629 in 2026-10;
a match that no longer has one fails the control, saying so) happens only
with it on. `tests/deaths.rs` (check):
every kill of seeded bot matches, by a lethal hit or at exactly 0 health,
comes with its death sound, the victim's 7 gibs and 50 rounds of blood
right after it (`Event::death`), the presentation spawns every one of
them (`Fx::death_spawns`, none dropped: play's pool keeps a death
reserve), and exact-zero deaths are credited.
`tests/state_transfer.rs` (check) loads every 5th state of 40 s chaos
matches (16 bots, MW 1.0 and Classic on Castle Strike) into another world:
`World::load_state`'s checks are never stricter than play.
`tests/tool_output.rs` keeps `fixtures/bench/tool-output/` (`stress.ron`,
`gpu-bench.ron`, `scenario-time.ron`, `feel.ron`) equal to what `stress --out`,
`gpu-bench --out`, `scenario --time` and `net-sim --feel` write; `xtask`'s envelope
and `net-feel` read them with strict structs, so a renamed field fails a test
instead of reading as 0 (rewrite with `OLR_BLESS_TOOL_OUTPUT=1`).
`tests/gpu_bench.rs` (the GPU tier, `cargo xtask full`, `OLR_GPU_TESTS=1`)
records a short stress match and runs `gpu_bench::run` on it on the
software adapter at 320 × 180 with a 2-pass probe (GPU probe time above 0
when the adapter has timestamps; lavapipe has them), and a tampered replay
is refused at its checksum.
`benches/game.rs` (divan, `cargo xtask bench`): 8 bots' think + tick and a
draw-list build on a warm Classic match (needs `OLR_CONTENT_DIR`).
`tests/render_golden.rs` (the GPU frame goldens) runs in `cargo xtask full`
(`OLR_GPU_TESTS=1`). `tests/matches.rs` (two 8-bot matches, determinism and
round trip; failure capture through the binary) and `tests/golden.rs` (the
golden checksum streams in `fixtures/matches/`; accept a change with `cargo
xtask bless-regression --reason "<why>"`) run in `check`; `tests/grid.rs`
(both mods × 4 levels × 3 seeds, chaos and Deathmatch bots, every weapon and
action kind covered; `long_bot_match_bounded`, 30 minutes) runs in `full`
(`OLR_FULL_TESTS=1`).
`tests/transport.rs` (spec §9.4, real sockets on 127.0.0.1, every wait
bounded; skipped without content): in `check`, two bot clients through
`typical` play a 0.1-minute match (about 10 s, at least 3 checksums;
impairment seeds 3 and 4, whose 1 % loss spares the handshakes), the
second's content lacking Dirt Level (`common::partial_content`): it
downloads it into its cache, plays, its replay the server's; and `net-client` (the
binary, through `good`, with one server bot, its content lacking the
level) downloads it into its temporary cache (`TMPDIR` empty after it
exits), plays a 0.05-minute match and
prints the server's checksums (about 7 s); in `full`
(`OLR_FULL_TESTS=1`, `check::FULL_TESTS`), 8 clients per profile `good`,
`typical`, `bad` (0.5 minutes each, about 106 s in all), a spectator
joining 4 s into a match (about 19 s), and plan 12c's
`a_frozen_server_hands_its_match_to_a_client` (`good`) and
`a_leaving_server_hands_its_match_to_a_client` (`bad`): 3 clients with
migration and 2 server bots, a 0.3-minute match, the server going 8 s in;
one migration named alike by all to a client, the bots playing on there,
the verdict above, the server's rows as it went at least 500 ticks;
about 24 s each (fails with the clients' migration off: all dropped).

**Desync bundles** (`bundle.rs`, `BUNDLE_VERSION` 3: 2 for `NetSpec`'s migration fields, 3 for withdrawals, plan 12d): `.olds` files from `net-sim` (and, in plan 10, real games); `olr-cli net-repro <file>` names the first diverging tick and world field. `net-sim` writes them to `--failures` when a client reports a desync (`netsim::run_with`; `NetSpec::faults` plants one: `(peer, host tick, worm, health)`). A bundle's `gone` holds the peer's withdrawals below its rows' end (`InputLog::gone_marks`, taken at the desync and again when the state loads, with the rows); `reproduce` withdraws each before its tick, as the harness's offline replay does. A state diff lists the first differing path of every differing `WorldState` field, in its declaration order (`worms[3].health, ents`; `bundle::FIELDS`, kept equal to the real field set by a test), so the cause is read from the whole list, not the first entry.
