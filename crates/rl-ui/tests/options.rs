//! Options (plan 7 task 4): key bindings that always keep the game
//! controllable (Review Focus 2), rebinding through the widgets with the
//! "press a key" capture, and the video and audio sections.

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use rl_input::{
    Action, Binding, Bindings, Button, Device, KeyCode, PadEvent, PadInput, RebindError, Resolve,
};
use rl_ui::screens::{self, MenuData};
use rl_ui::{
    AppCommand, AudioSettings, FrameRate, HudArrangement, Profile, Profiles, Screen, Settings, Store,
    UiAction, UiState, UpscaleFilter, VideoSettings, WindowPreset,
};

const KB: Device = Device::Keyboard;
const PAD: Device = Device::Gamepad;

/// Every action can be pressed with one key alone, and with one pad input.
fn controllable(b: &Bindings) -> bool {
    Action::ALL.iter().all(|a| {
        let key = b.keys.iter().any(|(_, k)| b.key_bits(&[*k]) & a.bit() != 0);
        let pad = b.pad.iter().any(|(_, p)| b.pad_bits(|q| q == *p) & a.bit() != 0);
        key && pad
    })
}

/// A small deterministic generator (no rand in the tests' dependencies).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self, n: usize) -> usize {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 33) as usize) % n
    }
}

#[test]
fn rebinding_never_leaves_an_action_unreachable() {
    // Any sequence of rebinds: keys used by other actions moved or shared,
    // every key of an action unbound, the keyboard and the pad on one
    // action. Refused steps change nothing; every step keeps the game
    // controllable.
    let inputs: Vec<Binding> = [
        KeyCode::ArrowUp,
        KeyCode::ArrowLeft,
        KeyCode::ControlLeft,
        KeyCode::AltLeft,
        KeyCode::KeyX,
        KeyCode::KeyZ,
        KeyCode::Space,
    ]
    .into_iter()
    .map(Binding::Key)
    .chain(
        [Button::South, Button::North, Button::DPadUp, Button::RightTrigger2]
            .into_iter()
            .map(|b| Binding::Pad(PadInput::Button(b))),
    )
    .collect();
    let mut rng = Lcg(7);
    let mut b = Bindings::olx();
    let (mut refused, mut moved) = (0, 0);
    for _ in 0..5000 {
        let action = Action::ALL[rng.next(Action::ALL.len())];
        let input = inputs[rng.next(inputs.len())];
        let before = b.clone();
        let r = match rng.next(3) {
            0 => b.bind(action, input, Resolve::Share),
            1 => b.bind(action, input, Resolve::Move).inspect(|_| moved += 1),
            _ => b.unbind(action, input),
        };
        if let Err(RebindError::Unreachable { .. }) = r {
            refused += 1;
            assert_eq!(b, before, "a refused step changes nothing");
        }
        assert!(b.unreachable().is_empty() && controllable(&b), "after {action:?} {input:?}: {b:?}");
    }
    assert!(refused > 100 && moved > 100, "the walk tried both: {refused} refused, {moved} moved");

    // Bindings that leave actions unreachable, from anywhere, are repaired
    // when the menus get them: OLX's defaults for what is missing.
    let mut broken = Bindings::olx();
    broken.keys.retain(|(a, _)| *a != Action::Shoot);
    broken.pad.clear();
    let mut ui = UiState::new(Settings::default(), Profiles::default(), 1);
    let out = ui.update(UiAction::SetBindings(broken));
    assert!(controllable(ui.bindings()));
    assert!(out.contains(&AppCommand::ApplyBindings(ui.bindings().clone())));
    assert_eq!(ui.bindings().of(Action::Shoot, KB), [Binding::Key(KeyCode::ControlLeft)]);

    // A hand-edited settings file: Shoot has no key and the pad section is
    // gone. It loads with Shoot's OLX key and OLX's pad, a warning, and the
    // rest of the file kept (not reset).
    let tmp = tempfile::tempdir().unwrap();
    let text = "(version: 1, settings: (video: (window: W1920x1080), input: (keys: [(Up, KeyW), (Down, KeyS), \
                (Left, KeyA), (Right, KeyD), (Jump, Space), (Rope, KeyE), (SelectWeapon, ShiftLeft)])))";
    std::fs::write(tmp.path().join("settings.ron"), text).unwrap();
    let got = Store::open(tmp.path()).load_settings();
    assert_eq!(got.value.video.window, WindowPreset::W1920x1080, "the rest of the file is kept");
    let input = &got.value.input;
    assert_eq!(input.of(Action::Up, KB), [Binding::Key(KeyCode::KeyW)]);
    assert_eq!(input.of(Action::Shoot, KB), [Binding::Key(KeyCode::ControlLeft)]);
    assert_eq!(input.pad, Bindings::olx().pad);
    assert!(got.warnings.iter().any(|w| w.contains("Shoot")), "{:?}", got.warnings);
    // The fixed file was saved: it loads again without a warning.
    let again = Store::open(tmp.path()).load_settings();
    assert!(again.warnings.is_empty(), "{:?}", again.warnings);
    assert_eq!(again.value, got.value);

    // A profile's own keys are repaired the same way.
    let mut own = Bindings::olx();
    own.keys.retain(|(a, _)| *a != Action::Up);
    let mut p = Profiles { list: vec![Profile { bindings: Some(own), ..Profile::default() }], selected: 0 };
    let w = p.sanitize();
    assert!(w.iter().any(|w| w.contains("Up")), "{w:?}");
    assert!(controllable(p.list[0].bindings.as_ref().unwrap()));
}

/// The menus and what they asked the app to do.
struct App {
    ui: UiState,
    data: MenuData,
    commands: Vec<AppCommand>,
}

fn harness(ui: UiState) -> Harness<'static, App> {
    let app = App { ui, data: MenuData::new(Default::default(), None), commands: Vec::new() };
    Harness::builder().with_size(egui::vec2(1280.0, 720.0)).build_ui_state(
        |ui, app: &mut App| {
            for a in screens::show(ui, &app.ui, &mut app.data) {
                let c = app.ui.update(a);
                app.commands.extend(c);
            }
        },
        app,
    )
}

/// What the app does with a key pressed while the options capture one.
fn press(h: &mut Harness<'static, App>, key: KeyCode) {
    h.state_mut().data.capture.as_mut().expect("capturing").on_key(key, true);
    h.run();
}

fn keys(h: &Harness<'static, App>, action: Action) -> Vec<Binding> {
    h.state().ui.bindings().of(action, KB)
}

fn open_keys(h: &mut Harness<'static, App>) {
    h.get_by_label("Options").click();
    h.run();
    assert_eq!(h.state().ui.screen(), Screen::Options);
    h.get_by_label("Keys").click();
    h.run();
}

#[test]
fn options_rebind_a_key() {
    let mut h = harness(UiState::new(Settings::default(), Profiles::default(), 1));
    open_keys(&mut h);
    let (z, ctrl, alt) =
        (Binding::Key(KeyCode::KeyZ), Binding::Key(KeyCode::ControlLeft), Binding::Key(KeyCode::AltLeft));

    // "Press a key" (OLX's "Input for:" box): a free key is added.
    h.get_by_label("Add key for Jump").click();
    h.run();
    h.get_by_label_contains("Press a key for Jump");
    assert_eq!(h.state().data.capture.as_ref().map(|c| c.device()), Some(KB));
    press(&mut h, KeyCode::KeyZ);
    assert!(h.state().data.capture.is_none());
    assert_eq!(keys(&h, Action::Jump), [alt, z]);
    let applied = AppCommand::ApplyBindings(h.state().ui.bindings().clone());
    assert!(h.state().commands.contains(&applied));
    assert!(matches!(h.state().commands.last(), Some(AppCommand::SaveSettings(_))));

    // A key another action has: shown, and moved on request.
    h.get_by_label("Add key for Shoot").click();
    h.run();
    press(&mut h, KeyCode::KeyZ);
    h.get_by_label("Z is Jump's.");
    assert_eq!(keys(&h, Action::Shoot), [ctrl], "nothing changes before the choice");
    h.get_by_label("Move it to Shoot").click();
    h.run();
    assert_eq!(keys(&h, Action::Shoot), [ctrl, z]);
    assert_eq!(keys(&h, Action::Jump), [alt]);

    // Moving Shoot's last key away is not offered: shared instead.
    h.get_by_label("Remove Left Ctrl from Shoot").click();
    h.run();
    assert_eq!(keys(&h, Action::Shoot), [z]);
    assert!(h.get_by_label("Remove Z from Shoot").accesskit_node().is_disabled(), "the last key stays");
    h.get_by_label("Add key for Rope").click();
    h.run();
    press(&mut h, KeyCode::KeyZ);
    assert!(h.get_by_label("Move it to Rope").accesskit_node().is_disabled());
    h.get_by_label_contains("Shoot would have no key");
    h.get_by_label("Use it for both").click();
    h.run();
    assert_eq!(keys(&h, Action::Shoot), [z]);
    assert_eq!(keys(&h, Action::Rope), [Binding::Key(KeyCode::KeyX), z]);

    // Cancel leaves everything as it was.
    let before = h.state().ui.bindings().clone();
    h.get_by_label("Add key for Up").click();
    h.run();
    press(&mut h, KeyCode::KeyZ);
    h.get_by_label("Cancel").click();
    h.run();
    assert_eq!(*h.state().ui.bindings(), before);

    // The game's own keys are refused; Esc cancels and stays in options.
    h.get_by_label("Add key for Up").click();
    h.run();
    press(&mut h, KeyCode::F2);
    h.get_by_label_contains("F2 is the game's");
    assert_eq!(*h.state().ui.bindings(), before);
    h.get_by_label("Add key for Up").click();
    h.run();
    // The app feeds the capture every key and egui none (it swallows them).
    press(&mut h, KeyCode::Escape);
    assert!(h.state().data.capture.is_none());
    assert_eq!(h.state().ui.screen(), Screen::Options);
    assert_eq!(*h.state().ui.bindings(), before);

    // A gamepad input on the same action as its keys.
    h.get_by_label("Add pad input for Shoot").click();
    h.run();
    assert_eq!(h.state().data.capture.as_ref().map(|c| c.device()), Some(PAD));
    h.state_mut().data.capture.as_mut().unwrap().on_pad(PadEvent::Button(Button::West, 1.0));
    h.run();
    let west = Binding::Pad(PadInput::Button(Button::West));
    assert_eq!(h.state().ui.bindings().of(Action::Shoot, PAD).last(), Some(&west));
    assert_eq!(keys(&h, Action::Shoot), [z]);

    // A browser shortcut is pointed out (Alt+F4 with Jump's Alt, spec §8.5).
    h.get_by_label("Add key for Jump").click();
    h.run();
    press(&mut h, KeyCode::F4);
    h.get_by_label_contains("Left Alt + F4");

    // Back to OLX's keys.
    h.get_by_label("Reset to OLX's keys").click();
    h.run();
    assert_eq!(*h.state().ui.bindings(), Bindings::olx());
    assert!(h.state().commands.contains(&AppCommand::ApplyBindings(Bindings::olx())));
    h.key_press(egui::Key::Escape);
    h.run();
    assert_eq!(h.state().ui.screen(), Screen::Main);
    assert!(h.state().ui.bindings().unreachable().is_empty());
}

#[test]
fn a_profile_with_its_own_keys_is_rebound_there() {
    let own = Bindings::olx();
    let p = Profiles { list: vec![Profile { bindings: Some(own), ..Profile::default() }], selected: 0 };
    let mut h = harness(UiState::new(Settings::default(), p, 1));
    open_keys(&mut h);
    h.get_by_label_contains("OpenLieroXor has its own keys");
    h.get_by_label("Add key for Jump").click();
    h.run();
    press(&mut h, KeyCode::KeyZ);
    let profile = h.state().ui.profiles().list[0].bindings.clone().unwrap();
    assert!(profile.of(Action::Jump, KB).contains(&Binding::Key(KeyCode::KeyZ)));
    assert_eq!(h.state().ui.settings().input, Bindings::olx(), "the options' keys are not the ones used");
    assert!(matches!(h.state().commands.first(), Some(AppCommand::SaveProfiles(_))));
}

/// The settings the menus last asked to save.
fn saved(h: &Harness<'static, App>) -> Settings {
    h.state()
        .commands
        .iter()
        .rev()
        .find_map(|c| match c {
            AppCommand::SaveSettings(s) => Some(s.clone()),
            _ => None,
        })
        .expect("saved")
}

fn video(h: &Harness<'static, App>) -> Vec<VideoSettings> {
    h.state()
        .commands
        .iter()
        .filter_map(|c| match c {
            AppCommand::ApplyVideo(v) => Some(v.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn options_set_video_and_audio() {
    let mut h = harness(UiState::new(Settings::default(), Profiles::default(), 1));
    h.get_by_label("Options").click();
    h.run();
    // Video opens first.
    h.get_by_label("Display").click();
    h.run();
    h.get_by_label("1920 × 1080").click();
    h.run();
    assert_eq!(video(&h).last().map(|v| v.window), Some(WindowPreset::W1920x1080));
    // Fullscreen is the list's last entry; it keeps the window's size.
    h.get_by_label("Display").click();
    h.run();
    h.get_by_label("Fullscreen").click();
    h.run();
    assert_eq!(video(&h).last().map(|v| (v.window, v.fullscreen)), Some((WindowPreset::W1920x1080, true)));
    h.get_by_label("Uncapped").click();
    h.run();
    h.get_by_label("Nearest").click();
    h.run();
    h.get_by_label("Smooth motion").click();
    h.run();
    let v = video(&h).last().cloned().unwrap();
    assert!(v.fullscreen && !v.interpolation);
    assert_eq!((v.frame_rate, v.filter), (FrameRate::Uncapped, UpscaleFilter::Nearest));
    assert_eq!(video(&h).len(), 5, "one apply per change");
    assert_eq!(saved(&h).video, v);
    // A window size picked while fullscreen ends it.
    h.get_by_label("Display").click();
    h.run();
    h.get_by_label("1600 × 900").click();
    h.run();
    assert_eq!(video(&h).last().map(|v| (v.window, v.fullscreen)), Some((WindowPreset::W1600x900, false)));

    h.get_by_label("Audio").click();
    h.run();
    h.get_by_role_and_label(Role::Slider, "Master volume");
    h.get_by_role_and_label(Role::Slider, "Effects volume");
    // OLX's default volume (Audio.Volume 70).
    assert_eq!(AudioSettings::default().master, 0.7);

    // The HUD's layout: the bar, or side panels on an ultrawide window.
    assert_eq!(v.hud_layout, HudArrangement::Auto, "Auto by default");
    h.get_by_label("Video").click();
    h.run();
    h.get_by_label("HUD layout").click();
    h.run();
    h.get_by_label("Side panels").click();
    h.run();
    assert_eq!(video(&h).last().map(|v| v.hud_layout), Some(HudArrangement::Sides));
    assert_eq!(saved(&h).video.hud_layout, HudArrangement::Sides);
    h.get_by_label("HUD layout").click();
    h.run();
    h.get_by_label("Bottom bar").click();
    h.run();
    assert_eq!(saved(&h).video.hud_layout, HudArrangement::Bar);
    // A file from before the setting loads with Auto; the setting is kept.
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("settings.ron"), "(version: 1, settings: (video: (ui_scale: 1.5)))")
        .unwrap();
    let got = Store::open(tmp.path()).load_settings();
    assert_eq!((got.value.video.hud_layout, got.value.video.ui_scale), (HudArrangement::Auto, 1.5));
    assert!(got.warnings.is_empty(), "{:?}", got.warnings);
    let mut s = Settings::default();
    s.video.hud_layout = HudArrangement::Sides;
    Store::open(tmp.path()).save_settings(&s).unwrap();
    assert_eq!(Store::open(tmp.path()).load_settings().value.video.hud_layout, HudArrangement::Sides);

    // The match is local play's, set up before each game: no tab here.
    assert!(h.query_by_label("Gameplay").is_none());
    assert!(h.query_by_label("Lives").is_none());
}

/// The game's own keys (`GAME_KEYS`: Esc, F5, F2, F3, T, Tab, H, and PageUp and
/// PageDown for a watched game's fast-forward) are never bound,
/// even from a file edited by hand: they are dropped, and an action left
/// without a key gets OLX's back (Review Focus 2).
#[test]
fn game_keys_are_never_bound_from_a_file() {
    use rl_ui::screens::GAME_KEYS;
    let game = |b: &Bindings| b.keys.iter().any(|(_, k)| GAME_KEYS.iter().any(|(g, _)| g == k));
    let mut s = Settings::default();
    s.input.keys.push((Action::Shoot, KeyCode::Tab));
    s.input.keys.retain(|(a, _)| *a != Action::Jump);
    s.input.keys.push((Action::Jump, KeyCode::Escape));
    s.input.keys.push((Action::Rope, KeyCode::KeyH));
    // The fast-forward keys too, even as an action's only key.
    s.input.keys.retain(|(a, _)| *a != Action::Down);
    s.input.keys.push((Action::Down, KeyCode::PageDown));
    s.input.keys.push((Action::Up, KeyCode::PageUp));
    let w = s.sanitize();
    assert!(!game(&s.input), "{:?}", s.input.keys);
    assert!(controllable(&s.input));
    assert_eq!(s.input.of(Action::Shoot, KB), [Binding::Key(KeyCode::ControlLeft)]);
    assert_eq!(s.input.of(Action::Jump, KB), [Binding::Key(KeyCode::AltLeft)], "OLX's back");
    assert_eq!(s.input.of(Action::Down, KB), [Binding::Key(KeyCode::ArrowDown)], "OLX's back");
    let names = [rl_input::key_name(KeyCode::PageUp), rl_input::key_name(KeyCode::PageDown)];
    for k in ["Tab", "Esc", "H", &names[0], &names[1]] {
        assert!(w.iter().any(|w| w.contains(k)), "{k}: {w:?}");
    }
    assert!(s.sanitize().is_empty(), "fixed once");

    // Through the options' state, and a profile's own keys.
    let mut ui = UiState::new(Settings::default(), Profiles::default(), 1);
    let mut b = Bindings::olx();
    b.keys.push((Action::Up, KeyCode::F5));
    ui.update(UiAction::SetBindings(b));
    assert!(!game(ui.bindings()));
    let mut own = Bindings::olx();
    own.keys = vec![(Action::Up, KeyCode::F2)];
    let mut p = Profiles { list: vec![Profile { bindings: Some(own), ..Profile::default() }], selected: 0 };
    p.sanitize();
    let own = p.list[0].bindings.as_ref().unwrap();
    assert!(!game(own) && controllable(own), "{:?}", own.keys);
}

/// The key the capture takes is the capture's alone: egui does not also
/// act on it (Space would press the focused button, Esc leave the screen).
#[test]
fn capture_swallows_the_captured_key() {
    let mut h = harness(UiState::new(Settings::default(), Profiles::default(), 1));
    open_keys(&mut h);
    h.get_by_label("Add key for Jump").click();
    h.run();
    // Keys egui hears while the box is open do nothing.
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.state().data.capture.is_some(), "Esc reaches the capture, not egui");
    assert_eq!(h.state().ui.screen(), Screen::Options);
    // The captured key, also seen by egui in the same frame, binds once.
    h.state_mut().data.capture.as_mut().unwrap().on_key(KeyCode::Space, true);
    h.key_press(egui::Key::Space);
    h.key_press(egui::Key::Enter);
    h.run();
    h.run();
    assert!(h.state().data.capture.is_none(), "no second capture opened");
    assert_eq!(keys(&h, Action::Jump), [Binding::Key(KeyCode::AltLeft), Binding::Key(KeyCode::Space)]);
    assert_eq!(h.state().ui.screen(), Screen::Options);
}

/// The chat key (online) is rebound in the Keys tab; the game's keys are refused.
#[test]
fn options_rebind_the_chat_key() {
    let mut h = harness(UiState::new(Settings::default(), Profiles::default(), 1));
    open_keys(&mut h);
    assert_eq!(h.state().ui.settings().online.chat_key, KeyCode::KeyT);
    h.get_by_label("Change chat key").click();
    h.run();
    h.get_by_label_contains("Press a key for Chat");
    press(&mut h, KeyCode::F2);
    h.get_by_label_contains("F2 is the game's");
    assert_eq!(h.state().ui.settings().online.chat_key, KeyCode::KeyT);
    h.get_by_label("Change chat key").click();
    h.run();
    press(&mut h, KeyCode::KeyY);
    h.run();
    assert_eq!(h.state().ui.settings().online.chat_key, KeyCode::KeyY);
    assert!(matches!(h.state().commands.last(), Some(AppCommand::SaveSettings(_))));
    assert_eq!(*h.state().ui.bindings(), Bindings::olx(), "the action keys are untouched");
}
