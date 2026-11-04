# Liero Factory Reloaded: the plant works (plan)

The maintainer's go (2026-11-04) on the frames of the "Factory Pass
Mockups" page, after the night shift: build every idea drawn there but
the painted particles (an engine sub-project of its own), then a
playtest tunes each. Ideas the maintainer ruled on: the stamp under the
13-37 sign presses the belt's loose dirt into dirt balls; the red box is
the belt's drive; the belt's stripe is the line's status light (red
stopped, white running); a ball falling off the incline vanishes at the
crate's rim; object-to-projectile is a later idea (memory only).
Illogical things found on the way (two lamps side by side, …) may be
fixed as they come.

Branch `factory-plant-works`. Each task ends green (`cargo xtask check`,
`--render` where pictures change); the tools re-run byte for byte.

## Tasks

1. **Engine: level glow groups under script control.** Scripts set a
   level glow group's intensity (0-1000) and colour, or give it back its
   own (`this.group_intensity(id, v)`, `this.group_colour(id,
   "#rrggbb")`, `this.group_reset(id)`), state of the world (hashed only
   when one is set, transferred, refused out of range); presentation lays
   them over the level's groups. Also `this.centre_x()`/`centre_y()`
   (and handle forms): a copy's centre, for blast reactions.
   `olr-sim`, `olr-script` (API, name checks of group ids), `olr-content`
   (`live_level` gives the level's group ids; `live_look` applies them),
   `olr-look` (`LookFrame::set_level_group`), `olr-present`.
2. **Sounds** (`tools/sounds/factory_sounds.py`): thump, rumble, whine,
   cart, engine, beep, idle, bubble, klaxon, horn, roar, drip, crickets,
   smash, pop.
3. **Art and built-in assets** (`tools/levels/factory_live.py`'s art,
   `content/assets/README.md` blocks): Stamp (the Console's place: the
   die on its rod, the LED column as a countdown), Conveyor (stripe
   Stopped/Running, six dirt balls, the drive's running lamp), Lift,
   Mine cart, Forklift, Generator (health, Wrecked), Siren (two turning
   beams), Lab flask; the Strip light gains health and a Broken state and
   reacts to blasts near it; window panes that shatter.
4. **Ground** (`tools/levels/factory_assets.py`, while the ground is
   alone): the lift's shaft, the cart's rails, the emergency lamps (group
   `emergency`, dark until a blackout), the work lamp over the bonus
   boxes (a still beam), the truck's hazards and cab, the lab's flask and
   burner glow, the pieces the new copies take.
5. **Scripts**: `stamp.rhai`, `belt.rhai`, `lift.rhai`, `cart.rhai`,
   `forklift.rhai`, `generator.rhai`, `siren.rhai`, `tube.rhai` (blasts,
   broken), `screen.rhai` (cuts to the last kill), `level.rhai` (the
   shift cycle, the alarm, the blackout, maintenance).
6. **Tests, goldens, docs**: `olr-cli`'s `tests/reloaded.rs`, the render
   and live goldens re-blessed, `content/reloaded/README.md`,
   `tools/levels/CLAUDE.md`, STATUS.md, CHANGELOG, the deferred
   verification row (playtest, frame cost).
7. **Frames and cost**: real frames of each, the render profile against
   the night shift's.
