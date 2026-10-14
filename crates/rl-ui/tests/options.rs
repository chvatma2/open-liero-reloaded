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
    AppCommand, AudioSettings, DownloadCacheView, FrameRate, HudArrangement, Profile, Profiles, Screen,
    Settings, Store, UiAction, UiState, UpscaleFilter, VideoSettings, WindowPreset,
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

fn clears(h: &Harness<'static, App>) -> usize {
    h.state().commands.iter().filter(|c| **c == AppCommand::ClearDownloadCache).count()
}

/// Plan 11: the Online tab's "Download cache: N MB" and its Clear, which
/// the app disables while an online game is on (a hover says why); no
/// cache says why.
#[test]
fn options_clears_the_download_cache() {
    let mut h = harness(UiState::new(Settings::default(), Profiles::default(), 1));
    h.state_mut().data.download_cache = Ok(DownloadCacheView { bytes: 312 << 20, clear: Ok(()) });
    // Only the options clear.
    assert!(h.state_mut().ui.update(UiAction::ClearDownloadCache).is_empty());
    h.get_by_label("Options").click();
    h.run();
    h.get_by_label("Online").click();
    h.run();
    h.get_by_label("Download cache: 312 MB");
    // Below the Online tab's lists: scrolled into view first.
    h.get_by_role_and_label(Role::Button, "Clear").scroll_to_me();
    h.run();
    h.get_by_role_and_label(Role::Button, "Clear").click();
    h.run();
    assert_eq!(clears(&h), 1);
    let busy = Err("leave the online game first".to_string());
    h.state_mut().data.download_cache = Ok(DownloadCacheView { bytes: 0, clear: busy });
    h.run();
    h.get_by_label("Download cache: 0.0 MB");
    let clear = h.get_by_role_and_label(Role::Button, "Clear");
    assert!(clear.accesskit_node().is_disabled());
    clear.click();
    h.run();
    assert_eq!(clears(&h), 1, "disabled while online");
    h.get_by_role_and_label(Role::Button, "Clear").hover();
    h.run();
    h.get_by_label("leave the online game first");
    h.state_mut().data.download_cache = Err("no cache directory".into());
    h.run();
    h.get_by_label("Download cache: none (no cache directory)");
    assert!(h.query_by_label("Clear").is_none());
}

/// Plan 12a: the Online tab edits the region, the relays and STUN servers
/// (Remove, Add with a valid entry only, Reset to the game's defaults) and
/// unblocks a host; each change saved.
#[test]
fn options_edit_relays_and_unblock() {
    let mut settings = Settings::default();
    settings.online.finding.blocked = vec!["ab".repeat(32), "cd".repeat(32)];
    let mut h = harness(UiState::new(settings, Profiles::default(), 1));
    h.state_mut().data.default_relays = vec!["wss://relay.one".into(), "wss://relay.two".into()];
    h.state_mut().data.default_stun = vec!["stun.one:3478".into()];
    h.get_by_label("Options").click();
    h.run();
    h.get_by_label("Online").click();
    h.run();
    // The defaults show while the settings name none.
    h.get_by_label("wss://relay.one");
    h.get_by_label("stun.one:3478");
    assert!(h.get_by_label("Reset relays to defaults").accesskit_node().is_disabled());
    // Remove the first relay: the list is the player's from now on.
    h.get_all_by_label("Remove").next().unwrap().click();
    h.run();
    assert_eq!(saved(&h).online.finding.relays, Some(vec!["wss://relay.two".to_string()]));
    // Add one: refused until it is a relay URL.
    h.get_by_role_and_label(Role::TextInput, "New relay").focus();
    h.run();
    h.get_by_role_and_label(Role::TextInput, "New relay").type_text("relay.example.org");
    h.run();
    let add = h.get_by_label("Add relay");
    assert!(add.accesskit_node().is_disabled());
    h.get_by_label("needs wss:// or ws://");
    add.hover();
    h.run();
    assert_eq!(h.get_all_by_label("needs wss:// or ws://").count(), 2, "beside the field and on hover");
    // One already listed.
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    h.get_by_role_and_label(Role::TextInput, "New relay").type_text("wss://relay.two");
    h.run();
    // Beside the field (and on Add's hover, the pointer still there).
    assert!(h.get_all_by_label("already listed").count() >= 1);
    assert!(h.get_by_label("Add relay").accesskit_node().is_disabled());
    // Replace the text: select all, then the URL.
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    h.get_by_role_and_label(Role::TextInput, "New relay").type_text("wss://relay.example.org");
    h.run();
    h.get_by_label("Add relay").click();
    h.run();
    assert_eq!(
        saved(&h).online.finding.relays,
        Some(vec!["wss://relay.two".to_string(), "wss://relay.example.org".to_string()])
    );
    // Reset: the defaults again.
    h.get_by_label("Reset relays to defaults").click();
    h.run();
    assert_eq!(saved(&h).online.finding.relays, None);
    h.get_by_label("wss://relay.one");
    // STUN: none asked at all.
    let stun_remove = h.get_all_by_label("Remove").count() - 1;
    h.get_all_by_label("Remove").nth(stun_remove).unwrap().click();
    h.run();
    assert_eq!(saved(&h).online.finding.stun, Some(Vec::new()));
    h.get_by_label("No STUN servers: none is asked.");
    h.get_by_role_and_label(Role::TextInput, "New STUN server").focus();
    h.run();
    h.get_by_role_and_label(Role::TextInput, "New STUN server").type_text("stun.example.org");
    h.run();
    h.get_by_label("needs host:port");
    assert!(h.get_by_label("Add STUN server").accesskit_node().is_disabled());
    // The region.
    h.get_by_label("Region").click();
    h.run();
    h.get_by_label("Oceania").click();
    h.run();
    assert_eq!(saved(&h).online.finding.region, rl_ui::Region::Oceania);
    // Unblock the first host (below the lists: scrolled into view first).
    h.get_all_by_label("Unblock").next().unwrap().scroll_to_me();
    h.run();
    h.get_all_by_label("Unblock").next().unwrap().click();
    h.run();
    assert_eq!(saved(&h).online.finding.blocked, ["cd".repeat(32)]);
    assert_eq!(h.state().ui.settings().online.finding.blocked, ["cd".repeat(32)]);
}

/// A full list of relays takes no more, and says why.
#[test]
fn a_full_relay_list_says_so() {
    let mut settings = Settings::default();
    settings.online.finding.relays =
        Some((0..rl_ui::MAX_SERVERS).map(|i| format!("wss://relay{i}.example")).collect());
    let mut h = harness(UiState::new(settings, Profiles::default(), 1));
    h.get_by_label("Options").click();
    h.run();
    h.get_by_label("Online").click();
    h.run();
    h.get_by_label("the list is full");
    assert!(h.get_by_label("Add relay").accesskit_node().is_disabled());
}

/// A hand-edited file's finding settings are fixed: bad relays, STUN
/// servers, addresses and keys dropped, keys lowercased, duplicates gone.
#[test]
fn finding_settings_are_sanitized() {
    let mut f = rl_ui::FindingSettings {
        region: rl_ui::Region::Asia,
        relays: Some(vec![
            "wss://a.example".into(),
            "https://not.a.relay".into(),
            " wss://a.example ".into(),
            "ws://127.0.0.1:7000".into(),
            "wss://".into(),
        ]),
        stun: Some(vec!["stun.example:3478".into(), "nope".into(), "x:0".into()]),
        public_address: "not an ip".into(),
        blocked: vec!["AB".repeat(32), "ab".repeat(32), "zz".into()],
        port_mapping: false,
        turn: vec![
            "me:secret@turn.example:3478".into(),
            "turn.example:3478".into(),
            " me:secret@turn.example:3478 ".into(),
            ":secret@turn.example:3478".into(),
            "me:secret@turn.example".into(),
            "me:secret".into(),
            "me:secret turn.example:3478".into(),
        ],
    };
    let w = f.sanitize();
    assert_eq!(f.relays, Some(vec!["wss://a.example".to_string(), "ws://127.0.0.1:7000".to_string()]));
    assert_eq!(f.stun, Some(vec!["stun.example:3478".to_string()]));
    assert_eq!(f.public_address, "");
    assert_eq!(f.blocked, ["ab".repeat(32)]);
    assert_eq!(f.turn, ["me:secret@turn.example:3478"]);
    assert!(!f.port_mapping, "the player's choice stays");
    assert_eq!(w.len(), 11, "{w:?}");
    assert!(!w.iter().any(|w| w.contains("secret")), "no password in a warning: {w:?}");
    let mut ok = rl_ui::FindingSettings { public_address: " 81.2.69.142 ".into(), ..Default::default() };
    assert!(ok.sanitize().is_empty());
    assert_eq!(ok.public_address, "81.2.69.142");
    // Printing the settings, an action or a command shows no password.
    let settings = Settings {
        online: rl_ui::OnlineSettings { finding: f.clone(), ..Default::default() },
        ..Settings::default()
    };
    for printed in [
        format!("{f:?}"),
        format!("{:?}", UiAction::SetFinding(f.clone())),
        format!("{:?}", AppCommand::SaveSettings(settings)),
    ] {
        assert!(!printed.contains("secret") && printed.contains("me:•••@turn.example:3478"), "{printed}");
    }
    // At most `MAX_TURN` TURN servers.
    let mut many = rl_ui::FindingSettings {
        turn: (0..12).map(|i| format!("u:p@turn{i}.example:3478")).collect(),
        ..Default::default()
    };
    assert_eq!(many.sanitize().len(), 1);
    assert_eq!(many.turn.len(), rl_ui::MAX_TURN);
    // Port mapping is on by default, and for a file written before it.
    assert!(rl_ui::FindingSettings::default().port_mapping);
    let old: rl_ui::FindingSettings = ron::from_str("(region: Europe)").unwrap();
    assert!(old.port_mapping && old.turn.is_empty());
}

/// Plan 12b (N51, N52): the Online tab's port mapping switch and the
/// player's own TURN servers, `user:pass@host:port`, each refused with
/// why (as relays are), at most `MAX_TURN`; every change saved.
#[test]
fn options_edit_turn_relays_and_port_mapping() {
    let mut h = harness(UiState::new(Settings::default(), Profiles::default(), 1));
    h.get_by_label("Options").click();
    h.run();
    h.get_by_label("Online").click();
    h.run();
    // On by default; unchecked, it is saved off.
    let switch = "Open the game's port on my router (UPnP/NAT-PMP/PCP)";
    assert!(h.state().ui.settings().online.finding.port_mapping);
    h.get_by_label(switch).click();
    h.run();
    assert!(!saved(&h).online.finding.port_mapping);
    // None by default (the project ships no TURN list, N52).
    h.get_by_label("No TURN relays: none is asked.");
    // Final review C-M5: the new TURN relay is masked as it is typed (its
    // password), unless "Show password" is ticked.
    let field = Role::PasswordInput;
    h.get_by_role_and_label(field, "New TURN relay").focus();
    h.run();
    h.get_by_role_and_label(field, "New TURN relay").type_text("turn.example.org:3478");
    h.run();
    h.get_by_label("needs user:pass@host:port");
    let add = h.get_by_label("Add TURN relay");
    assert!(add.accesskit_node().is_disabled());
    add.hover();
    h.run();
    assert_eq!(h.get_all_by_label("needs user:pass@host:port").count(), 2, "beside the field and on hover");
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    h.get_by_role_and_label(field, "New TURN relay").type_text("anna:s3cret@turn.example.org:3478");
    h.run();
    let typed = h.get_by_role_and_label(field, "New TURN relay").accesskit_node().value().unwrap_or_default();
    assert!(!typed.contains("s3cret"), "masked while typed: {typed}");
    h.get_by_label("Show password").click();
    h.run();
    let shown = h.get_by_role_and_label(Role::TextInput, "New TURN relay").accesskit_node().value();
    assert_eq!(shown.as_deref(), Some("anna:s3cret@turn.example.org:3478"), "shown when asked");
    h.get_by_label("Add TURN relay").click();
    h.run();
    assert_eq!(saved(&h).online.finding.turn, ["anna:s3cret@turn.example.org:3478"]);
    // Listed without its password.
    h.get_by_label("anna:•••@turn.example.org:3478");
    assert!(h.query_by_label_contains("s3cret").is_none(), "the password is not shown");
    // Removed again.
    h.get_by_role_and_label(Role::Button, "Remove").click();
    h.run();
    assert!(saved(&h).online.finding.turn.is_empty());
    // A full list takes no more.
    let mut settings = Settings::default();
    settings.online.finding.turn =
        (0..rl_ui::MAX_TURN).map(|i| format!("u:p@turn{i}.example:3478")).collect();
    let mut h = harness(UiState::new(settings, Profiles::default(), 1));
    h.get_by_label("Options").click();
    h.run();
    h.get_by_label("Online").click();
    h.run();
    h.get_by_label("the list is full");
    assert!(h.get_by_label("Add TURN relay").accesskit_node().is_disabled());
}
