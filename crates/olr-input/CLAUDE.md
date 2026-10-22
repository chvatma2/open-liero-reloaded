# olr-input

Keyboard and gamepad input for one player, turned into the simulation's 11
`Keys` once per tick (spec §8.5, §19 E5; presentation note §10).

- **Layer 5, uses `std`.** Depends on `olr-sim` (`Keys`, `World` for the
  mouse controller), `olr-bots` (its motor's `aim_key`), `olr-math`, `winit` 0.30.13
  (physical `KeyCode`s, `WindowEvent`; the version egui-winit 0.36.2 uses),
  `gilrs` 0.11.2, `serde`. `#![forbid(unsafe_code)]`. It opens no device:
  the caller owns the window and the `Gilrs` context and passes events in.
- **Bindings** (`bindings.rs`): `Action` is OLX's control with a sim key
  (Weapon1-5 have none), plus `Dig`, olr-input's own (`DIG`, bit 15, never
  reaches the sim). `Bindings::olx()` (the default) is OLX's player 1:
  arrows, Shoot `ControlLeft`, Jump `AltLeft`, Rope `KeyX`, SelectWeapon
  `ShiftLeft`, Strafe `KeyZ`, Previous/NextWeapon `KeyA`/`KeyS`, and Dig
  `KeyC` (free in OLX's defaults); pad d-pad, right trigger, A, Y, left
  trigger, plus left stick X / right stick Y as digital directions (OLX's
  analogue twin-stick is not ported), Strafe West, Previous/NextWeapon the
  bumpers (`LeftTrigger`/`RightTrigger`), Dig East (OLX binds no pad input
  to the new four). Several inputs may share an action. Serde for M7's options.
  `browser_conflicts()` / `is_browser_reserved` flag bound keys that form
  a shortcut browsers keep (Ctrl+W, ...); the defaults have none.
  `#[serde(default)]`: a file without `keys` or `pad` takes OLX's.
- **Rebinding** (`rebind.rs`, plan 7 Review Focus 2): `Binding` (a key or
  a `PadInput`) of a `Device`; `of(action, device)`, `actions_of(input)`,
  `unreachable()`; `bind(action, input, Resolve::{Share, Move})` and
  `unbind` refuse (`RebindError::Unreachable`, nothing changed) any step
  that leaves an action without a key or without a gamepad input;
  `repair()` gives a file's unreachable actions OLX's defaults back (one
  warning each). `Capture::new(device)` is the options' "press a key" box:
  fed window and gilrs events, it keeps the first press (modifiers too; a
  trigger or stick past its threshold). `key_name` and `Binding`'s
  `Display` are the names the options show; `Action::name()` OLX's.
- **Thresholds:** a trigger from 75 % of its travel, a stick from 60 %
  (OLX's `CInput`). Only gilrs's `ButtonChanged` (sent for every button)
  and `AxisChanged` are read; `Disconnected` releases that pad.
- **State** (`state.rs`): `on_window_event` (repeats ignored; focus loss
  releases every key), `on_key`, `on_gamepad` / `on_pad` (`PadEvent`, for
  tests: gilrs ids and codes cannot be built outside gilrs), `held`,
  `latch_tick` once per sim tick.
- **Latching** (`latch.rs`, `TickLatch`): a press since the last tick is
  down for this tick even if released again; a release of a key the last
  tick saw down is up for this tick even if pressed again, and that press
  is down for the next. So no press or release is lost; the sim's own
  `KeyLatch` turns them into OLX's `isDownOnce` taps. `tap(bits)` marks
  keys pressed for this tick only (released at the next).
- **Dig** (`dig.rs`, `Dig`): the Dig action presses OLX's dig gesture
  itself, after the latch: one direction held taps the other, none held
  taps both, both or SelectWeapon held does nothing; a key down at the last
  tick waits a tick (so the sim sees its edge). A press digs once; held, it
  digs again every `DIG_REPEAT_TICKS` (20, OLX's 0.2 s auto-carve,
  `CWormHuman.cpp:357-394`).
- **Mouse controls** (`mouse.rs`, issue #24, experimental): the mouse
  never reaches the simulation; `MouseController` turns it into the keys
  a key player could press each tick. `InputState::set_mouse_controls`
  plays with `Bindings::for_mouse` (the player's bindings without Up,
  Down, SelectWeapon and Strafe, and without anything on `MOUSE_KEYS`: A
  and D Left and Right, Space Jump, S Dig); `on_mouse` (the caller passes
  mouse events only while the game has the pointer): left button Shoot,
  right Rope (latched like keys), the wheel a notch per line (pixel
  deltas one per `NOTCH_PIXELS`), up Previous, down Next weapon;
  `latch_tick_for(local, cursor)` gives the controller the local worm
  (`LocalWorm::of(world, worm, ahead)`: its state, latches and the next
  ticks' sub-steps from `World::substeps_ahead`/`latches`) and the
  cursor's direction from the drawn worm in game pixels. The controller:
  the aim by Up/Down through `olr_bots::motor::aim_key` (so never faster
  than the keys; a 1° dead zone, nothing pressed in a tick with no read);
  the facing follows the cursor's side of the worm's vertical line with
  hysteresis (`FACE_MARGIN`, `FACE_MARGIN_MIN`, nothing nearer than
  `NEAR`), turned by the side's key alone for a tick, no Strafe, held
  until a read turned it, and only when the other key has no unread press
  and the player is not pressing both (so a turn never digs); Left/Right
  otherwise with Strafe; the player's Up, Down, SelectWeapon and Strafe
  bits dropped; a wheel notch (queue `WHEEL_QUEUE`, more dropped, cleared
  while dead or without a worm) pressed when the last tick had none, the
  last press was read, this tick reads, and not while firing without
  `WeaponCombos`. Look-ahead: `LocalWorm::run` models `get_input`'s aim,
  facing and latches, replaying the keys given for the `ahead` ticks
  in flight (the input delay). `repeatable()` is `held()` plus, in mouse
  mode, `MADE_UP` (Up, Down, Strafe): what a stalled online game may
  repeat.

Test: `cargo nextest run -p olr-input`.
