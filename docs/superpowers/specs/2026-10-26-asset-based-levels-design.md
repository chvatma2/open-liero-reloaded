# Asset-based levels: design

Status: brainstormed with the maintainer on 2026-10-26, for milestone 0.3.0.
Mockups from the session (the level tab, resizing, the asset tab in the
skin editor's layout) are described in words below; they were throwaway.

## Intent

Levels are made of **assets**, not layers. An asset is a self-contained
object (a console, a crane, a lamp, a stretch of floor, the ground itself)
built once in its own editor tab and placed in levels. The asset is the
basic unit scripts address. The aim is levels that feel alive (machines
moving, screens changing, lights failing, sounds) and are easier to build
from reusable pieces than painted layer stacks.

This is the first and strongest focus of 0.3. It prepares the 0.4 campaign
(scriptable, dynamic levels) without touching the existing game modes.

## Goals

- An **asset tab** where an asset is painted and composed from layers,
  given states (one painted picture each), exposed properties, sounds and
  a default script.
- A **level tab** focused on placing, moving and resizing assets with ease,
  each placed copy optionally **Live** and with its own short script.
- Assets owned by a **library**; levels refer to library assets and pick up
  their changes. **Export** and **import** of a level with its scripts and
  assets as one archive.
- **Live assets in matches**: states, exposed values, solid moving rock
  parts, sounds and scripts, all deterministic, rollback-safe and the same
  for every player online.
- Today's **Liero Factory Reloaded** kept as **Liero Factory After Dark**,
  and a new, dynamic **Liero Factory Reloaded** built with the new editor as
  the test bed and showcase.

## Non-goals (this design)

- Teams, Team Deathmatch, the checkpoint modes, Rampage and the bot work of
  0.3: a separate brainstorm. (Checkpoints will be assets, the maintainer's
  note; their game rules are that brainstorm's.)
- Online ratings and matchmaking: moved to 0.4 (2026-10-26).
- Cutscenes, objectives, missions as packs, campaign-only commands: 0.4.
- Remaking Castle Strike Reloaded: it stays as it is.
- A live camera feed of the match on an in-level screen (possible later;
  the security screen uses painted pictures).
- Animation frames inside a state (the maintainer: animation is switching
  states).

## Anti-goals

- **The classic game must not change.** OLX levels, the imported packs, the
  oracle traces, the match goldens and the existing modes play exactly as
  before. A level with no live assets has no runtime cost and no new
  behaviour.
- **No dependence on the library to play.** A shared or downloaded level
  plays without its author's library.
- **No desyncs.** Nothing a script or a live asset does may differ between
  peers, replays or rollbacks.
- **No breaking the user's work silently**: a library change that breaks a
  level's script is reported, never applied quietly.

## Decisions (the maintainer's)

### The model

- **A level is placed assets.** Layers exist only inside assets. The
  painted ground of a level is the level's own asset (static, unnamed, not
  in the library unless saved there); a level may have several. A parallax
  sky is an asset placed with a distance setting; checkpoints for game
  modes will be assets. What remains of the level itself: its look
  (ambient light, haze, grading), spawn points, and its scripts.
- **Static and Live.** Every placed copy has a **Live** switch, off by
  default.
  - **Static** (off): baked into the ground when the match starts; it digs
    and blasts like ground. Its states and scripts do nothing.
  - **Live** (on): stays an object in the match; scripts reach it by its
    id. Only assets made of **rock and background** layers can be live
    (rock cannot be dug and background is not solid, so a live asset never
    needs digging). Dirt belongs to static assets.
- **Live rock parts are solid**: worms stand on them, are carried and
  pushed by them, and the rope follows them, through the separate solid
  layer the 0.3.0 milestone description already sets out (one terrain
  query; the oracle path never sees it). Background parts are scenery.
  Explosions never dig a live asset; one that can be destroyed has health
  and switches to a chosen state (such as `wrecked`) at 0.
- **Live assets work in every mode**, Deathmatch and online included (the
  maintainer, 2026-10-26): a level's live assets and scripts are part of
  the level, wherever it is played. This narrows the milestone
  description's "campaign-only machinery in a new scenario mode": what
  stays campaign-only is the campaign's own machinery (objectives,
  cutscenes, mission scoring, mission packs), not live assets and
  scripts.
- **Live rock crushes** (the maintainer: "an interesting mechanic, but
  easy to get wrong"): a moving or appearing live rock part that meets a
  worm with nowhere to go hurts it, and can kill it. Deaths must not come
  too easily: the damage is tuned in a careful playtest before it ships
  (`docs/deferred-verification.md`).

### The asset tab

- Modelled on the **skin editor**: session bar (Save, Close, Undo, Redo,
  name, unsaved mark), tools on the left, a large zoomable pixel grid in
  the middle, states and preview on the right, a status line. Any size
  (the skin editor's frames are fixed).
- **Layers** (rock, dirt, background; glow, materials, glass, masks as
  today), composed in the asset. Folders, masks, merge and split move here
  from today's level editor.
- **States**: named (Normal, Alarm, Off, …), one marked as the starting
  state (★). **Each state is its own painted picture plus its own
  values.** The states panel shows every state small; the middle panel
  edits the selected one; previews (light and dark, game size and
  enlarged, as the skin preview) show states as in play. No animation
  frames within a state.
- **Exposed properties**: unroll the layer tree; each layer offers
  settable properties; marking one exposes it under a name scripts use
  (`claw_x`, `screen_colour`), with a type, default and limits. An asset
  may expose just its states, or finer values too. The offer:
  - *The asset as a whole*: state; position x and y with minimum and
    maximum; visible; facing (flipped); health (if destructible).
  - *Rock layer*: offset x and y inside the asset with minimum and maximum
    (the crane's claw; it pushes worms); visible (hidden means not solid:
    a door); glow strength, glow colour, light intensity, light reach,
    glow animation; beam direction and spread.
  - *Background layer*: offset x and y with limits; visible; opacity;
    glow strength, glow colour, light intensity, light reach, glow
    animation; beam direction and spread; glass opacity and tint.
  - Not offered: material, dirt layers, scale and rotation.
  - Light: the glow group a layer uses is internal to the asset; glow
    colour, intensity, reach and animation are offered on the layer. Two
    layers sharing a group change together, and the editor says so when
    one is exposed ("also changes: Buttons").
- **Sounds**: a state may loop a sound while it lasts (the crane's hum) and
  play one when entered (a lamp's tick). Positional, heard by the nearest
  worm's listener as other game sounds are. Presentation only.
- **Default script**: an asset may carry a Rhai script that runs on every
  live placed copy (the faulty lamp stutters wherever it is dropped).

### The level tab

- Focus on **placement, resizing and ease of use**: the asset library on
  the left, the level in the middle, the selected copy's panel on the
  right. Drag from the library to place; click selects a whole asset; drag
  moves; handles resize; double-click opens the asset in the asset tab.
- **Resizing stretches**, keeping the aspect ratio by default; a toggle in
  the copy's panel allows free resizing.
- The **placed copy's panel**: its id (what scripts use), position, size,
  the Live switch, its starting state, its exposed values for this copy
  (two consoles may start in different colours), and its **script**.
- **Per-copy scripts are short Rhai scripts** (chosen over picked rules for
  their freedom), e.g. cycling the console's three states every 300 ticks.

### The library, export and import

- **The library owns assets.** Levels refer to library assets; editing an
  asset in the library changes every level of the user's that uses it.
- **Export** writes one archive: the level, its scripts and the assets it
  uses. **Import** reads such an archive: the level among the user's
  levels, its assets into the library.

### Liero Factory

- Today's `Liero Factory Reloaded` is renamed **Liero Factory After Dark**
  and otherwise kept.
- A new **Liero Factory Reloaded** is built in the new editor from After
  Dark's art: the ground as the level's own asset, pieces as library
  assets, and live assets with scripts and sounds:
  - fluorescent lights that fail like a faulty tube: stuttering on and
    off in uneven bursts, sometimes staying dark, with the tube's buzz and
    tick in time (a lamp asset with On, Off and perhaps Dim states, sounds
    per state, a default script switching them irregularly);
  - the crane's claw moving from one end of its rail to the other every
    couple of seconds, humming while it moves, pushing worms in its way;
  - the console going red (Alarm) while the crane moves;
  - the upper control room's screen (today a miniature of the truck)
    showing a different part of the factory every couple of seconds, like
    a security camera (states with painted miniatures);
  - the 13-37 display and a few more glowing assets changing their glowing
    dots now and then;
  - in the dark ceiling left of the crane console, a spotlight turning
    slowly: a cone circling in a plane perpendicular to the view (flat on
    the depth axis), so seen from the side it sweeps left and right, is
    longest at the sides and shorter and wider as it swings towards or
    away from the viewer. A look decision: judged from real rendered
    frames before it is kept.
- It is the test bed: what the factory needs, the editor and the runtime
  must do.

## Calls I made (the maintainer may overrule)

1. **After Dark's pack id** becomes `olr-level-liero-factory-after-dark`;
   the new level takes `olr-level-liero-factory-reloaded`. Replays and
   settings naming the old level then name the new one (compatibility
   breaks are acceptable at this stage).
2. **The Live switch is offered only on assets with states, exposed
   properties, sounds or a script**, and is disabled, with the reason, on
   an asset containing dirt.
3. **A new state starts as a copy of the starting state**; a layer not
   painted in a state follows the starting state's pixels, so a fix to the
   body appears in every state. Painting a layer in a state makes it that
   state's own.
4. **Static assets bake in their starting state.**
5. **Exposed position limits are relative to the asset**, so stretching a
   placed crane stretches its claw's travel with it.
6. **Snapping**: while a key is held, resize handles snap to whole
   multiples (2×, 3×), where pixel art stays even.
7. **Library changes are announced, not silent**: opening a level whose
   library assets changed lists them and updates with one click; a change
   that breaks a level's script (an exposed name it uses removed) is shown
   as an error on that level. Built-in levels never follow the user's
   library.
8. **A saved level is playable on its own**: saving bakes the ground with
   its static assets, the live assets' pictures, states, values, sounds
   and scripts into the level pack (as today's baked files beside the
   editable source). Downloads, online play and replays use only that.
   Editing needs the library.
9. **Import conflicts**: an imported asset identical to one in the library
   is reused; a different one of the same name comes in alongside under a
   new name ("Console 2") and the imported level uses it. Import never
   overwrites the user's assets.
10. **Scripts**: a placed copy's script runs alongside its asset's default
    script; the copy may switch the default off. Scripts work only through
    the public face: states, exposed values, the asset's sounds, and
    reading the match (time, positions, events).
11. **States and values**: entering a state applies its values; a script
    setting an exposed value afterwards changes just that value until the
    next state change.
12. **"Now and then"** uses the match's seeded randomness, never the clock.
13. **Each live copy gets its own glow groups** at runtime, so two consoles
    in one level change colour independently, without the author managing
    groups.
14. **Live beams are worked out every frame** (today's are baked into the
    level), so a spotlight's direction, spread and reach can change.
15. **Painted miniatures** for the security screen; no live feed.
16. **A level opens in the new editor as one ground asset** when it has no
    assets (every OLX level, After Dark, Castle Strike Reloaded), so every
    existing level stays editable.
17. **A level pack always holds a copy of every asset it uses** (the
    editable asset, not only its baked result), in its editable source. A
    level shared by export is fully editable; online downloads carry only
    the baked, playable level, as today (amended while cutting the
    issues: the export archive is the editable form). Opening a level compares its copies with the library: changed
    library assets are announced (call 7); an asset missing from the
    library is offered to be added from the level's copy.
18. **Built-in assets** (`content/assets/`, shipped) are a read-only
    library beside the user's; editing one saves the user's own copy, and
    editing a built-in level saves the user's own copy of the level, as the
    editor does today. Today's built-in and user assets are converted to
    the new format.
19. **Asset sounds are the asset's own files**, WAV or Ogg within the
    limits `olr_pack::sounds` sets for mod sounds, checked as untrusted
    content. The factory's crane hum, tube buzz and tick are made by the
    builder (synthesised or CC0, credited) and replaceable by the
    maintainer.
20. **Scripts may read and drive other placed copies by their id**, and a
    level has a **level script** too (the console going red while the crane
    moves couples two copies). Ids and exposed names a script uses are
    checked when the level is saved and when its library assets change.
21. **Damage to live assets**: explosions only, by the explosion's own
    damage to worms, with no kill credit; a wrecked state's own picture
    decides what is still solid.
22. **Crushing**: damage a tick while a worm is wedged (a level object can
    kill but is credited to no player: a death without a killer and
    without a suicide's penalty); a part does not pass through static
    rock or other live rock (it stops there). The amount is a tunable
    constant, set in the playtest.
23. **Bots** in every mode must not break on live parts: at least they
    treat live solid parts as obstacles where they are now and avoid
    standing where crushing is likely; smarter use is 0.4's. The bot grid
    covers a level with live assets.

## Constraints

- `CLAUDE.md`'s rules hold: the simulation crates stay `no_std` and build
  for `wasm32`; content is untrusted and never panics the game (bounded
  files, scripts, operation counts); output is deterministic (no `HashMap`
  iteration, no clock in logic); shipped paths stay within 80 characters.
- **Everything that matters to play lives in the simulation's world**:
  live assets' positions, velocities, states, health, exposed values and
  script state, so they are cloned for rollback, transferred on desync
  repair and checksummed. Presentation (pictures, glow, sounds, beams)
  only follows them and stays out of the checksum.
- **Rhai** (pure Rust, `no_std` + `alloc`, `wasm32`), sandboxed with an
  operation limit, integer or `olr-math` fixed-point maths; script state in
  the world, not in the interpreter (the milestone description).
- The **oracle** path never sees live assets or scripts; OLX levels play
  bit for bit as today.
- Bots treat live solid parts only as far as needed not to break (their
  navigation cache assumes rock never changes; call 23); smarter handling
  is 0.4's.
- Live assets travel with the level in online play: content download,
  replays and desync bundles carry them, and a level's hash covers its
  live assets and scripts.
- The level and asset formats change; compatibility breaks are fine
  (bump formats, no shims), but every shipped level is converted and
  `cargo xtask check` stays green.

## Order of work (to be cut into issues)

1. **Assets and levels as data, and the two tabs.** The asset format
   (layers, states with pictures and values, exposed properties, sounds,
   default script as text), the level format (placed copies, Live,
   per-copy values and scripts), the library with export and import, the
   asset tab, the level tab, existing levels opening as one ground asset,
   and the rename to After Dark. Nothing moves yet: live assets play as
   static.
2. **Live assets in play.** The solid object layer, states and exposed
   values in the world, health, Rhai scripts (default and per copy),
   positional asset sounds, per-copy glow groups, live beams.
3. **The new Liero Factory Reloaded**, built in steps as 1 and 2 land:
   first as assets (playing like After Dark), then alive.
4. The spotlight's look: a throwaway spike of the projected circling cone
   on real frames, judged by the maintainer, before it is built.

## Left to the builder

- File layouts, field names and limits of the new formats.
- How the editor's existing code is reused or reorganised between the two
  tabs.
- The Rhai API's exact functions (moving with a speed, timers, randomness,
  events), within the rules above.
- How the solid object layer meets the simulation's terrain queries, worm
  physics and the rope, within the milestone description's sketch.
- How live assets are drawn and lit.
