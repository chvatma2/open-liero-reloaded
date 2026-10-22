# olr-input

Keyboard, gamepad and mouse input for one player, turned into the simulation's 11
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
  a shortcut browsers keep (Ctrl+W, ...; each keyboard list on its own);
  the defaults have none. Mouse controls' own lists (issue #25):
  `mouse_keys` (their keyboard, in place of `keys`, default `MOUSE_KEYS`)
  and `mouse` (`MouseInput`: Left, Right, Middle, Back, Forward buttons,
  WheelUp, WheelDown; default `MOUSE_BUTTONS`), never of `MOUSE_MADE`
  (Up, Down, SelectWeapon, Strafe: the controller's); `mouse_bits`.
  `#[serde(default)]`: a file without a list takes the default (OLX's,
  #24's for the mouse lists).
- **Rebinding** (`rebind.rs`, plan 7 Review Focus 2; issue #25): `Binding`
  (a key, a `PadInput` or a `MouseInput`) of a `Device` (Keyboard,
  Gamepad, Mouse), in a `Mode` (`Keys`, or `Mouse`: mouse controls; its
  Keyboard is `mouse_keys`, the pad list is shared; `Mode::devices`, the
  options' columns; `Mode::binds(action, device)`); `of(mode, action,
  device)`, `actions_of(mode, input)`, `unreachable()` (both modes' gaps,
  `(Action, Need)`: `Need::Key` and `PadInput` mouse controls off, every
  action; `KeyOrMouse` with them on, every action but `MOUSE_MADE`'s);
  `bind(mode, action, input, Resolve::{Share, Move})` and `unbind` refuse
  (`RebindError::Unreachable { action, need }`, nothing changed) any step
  that leaves an action unreachable, and `bind` a key or mouse input for
  `MOUSE_MADE`'s in mouse mode or the mouse with it off
  (`NotBindable`); `repair()` drops `MOUSE_MADE`'s from the mouse lists
  and gives unreachable actions their defaults back (OLX's, #24's; one
  warning each). `Capture::new(device)` is the options' "press a key" box:
  fed window and gilrs events, it keeps the first press (modifiers too; a
  trigger or stick past its threshold; a mouse button, or a whole wheel
  notch counted as `InputState` counts them); Esc, for a pad or mouse
  capture, sets `cancelled()`. `key_name` and `Binding`'s
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
  plays with `Bindings::for_mouse` (`mouse_keys`, `mouse` and `pad`
  without `MOUSE_MADE`'s; by default A and D Left and Right, Space Jump,
  S Dig; off, `keys` again, untouched); `on_mouse` (the caller passes
  mouse events only while the game has the pointer): the buttons through
  `mouse` (latched like keys; by default left Shoot, right Rope), the
  wheel a notch per line (pixel deltas one per `NOTCH_PIXELS`, at most
  `WHEEL_QUEUE` an event): a notch's Previous/Next weapon go to the
  controller's queue (by default up Previous, down Next), its other
  actions are pressed and released at once (a tick's tap);
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
