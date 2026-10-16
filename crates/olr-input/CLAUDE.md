# olr-input

Keyboard and gamepad input for one player, turned into the simulation's 8
`Keys` once per tick (spec §8.5, §19 E5; presentation note §10).

- **Layer 5, uses `std`.** Depends on `olr-sim` (`Keys`), `winit` 0.30.13
  (physical `KeyCode`s, `WindowEvent`; the version egui-winit 0.36.2 uses),
  `gilrs` 0.11.2, `serde`. `#![forbid(unsafe_code)]`. It opens no device:
  the caller owns the window and the `Gilrs` context and passes events in.
- **Bindings** (`bindings.rs`): `Action` is OLX's control with a sim key
  (Strafe, Previous/NextWeapon and Weapon1-5 have none). `Bindings::olx()`
  (the default) is OLX's player 1: arrows, Shoot `ControlLeft`, Jump
  `AltLeft`, Rope `KeyX`, SelectWeapon `ShiftLeft`; pad d-pad, right
  trigger, A, Y, left trigger, plus left stick X / right stick Y as
  digital directions (OLX's analogue twin-stick is not ported). Several
  inputs may share an action. Serde for M7's options.
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
  `KeyLatch` turns them into OLX's `isDownOnce` taps.

Test: `cargo nextest run -p olr-input`.
