//! The menus by gamepad (issue #31): fake `PadEvent`s fed to
//! `MenuData::pad` and its keys added to each frame's input, as the app
//! does; the widgets found by label (`egui_kittest`).

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use olr_input::{Axis, Button, PadEvent};
use olr_ui::pad::{Nav, PadNav, REPEAT_DELAY, REPEAT_EVERY, Who};
use olr_ui::{HudArrangement, Profiles, Screen, Settings, UiAction, UiState};

use crate::flows::{App, harness, starts};

/// A frame as the app runs it, the pads' keys first, then a few more (a
/// focus move is watched over the next passes, which ask for them).
fn frame(h: &mut Harness<'static, App>) {
    let ctx = h.ctx.clone();
    let app = h.state_mut();
    let events = app.data.pad_events(&ctx, &app.ui);
    h.input_mut().events.extend(events);
    h.run_steps(12);
}

/// Pad `pad` presses `b` and lets go of it, then a frame.
fn press(h: &mut Harness<'static, App>, pad: usize, b: Button) {
    for v in [1.0, 0.0] {
        h.state_mut().data.pad.on_pad(pad, PadEvent::Button(b, v));
    }
    frame(h);
}

/// The focused widget's label (`None`: nothing focused).
fn focused(h: &Harness<'static, App>) -> Option<String> {
    h.query_by(|n| n.is_focused()).map(|n| n.accesskit_node().label().unwrap_or_default())
}

/// Pad 0 presses `b` until `label` has the focus (at most 30 times),
/// never stopping on a text field.
fn focus_on(h: &mut Harness<'static, App>, b: Button, label: &str) {
    let mut path = Vec::new();
    for _ in 0..30 {
        press(h, 0, b);
        assert!(!h.ctx.text_edit_focused(), "a text field kept the focus: {path:?}");
        let f = focused(h);
        if f.as_deref() == Some(label) {
            return;
        }
        path.push(f);
    }
    panic!("{b:?} never reached {label}: {path:?}");
}

fn in_menus() -> Harness<'static, App> {
    harness(UiState::new(Settings::default(), Profiles::default(), 7))
}

/// The d-pad moves the focus (the first press focuses the first widget),
/// A activates: from the main menu to local play, the mod's drop-down
/// stepped with right, then Start.
#[test]
fn a_pad_starts_a_local_match_from_the_main_menu() {
    let mut h = in_menus();
    assert_eq!(focused(&h), None);
    press(&mut h, 0, Button::DPadDown);
    assert_eq!(focused(&h).as_deref(), Some("Local Play"));
    // The focused widget is ringed while the pad leads.
    assert!(h.state().data.pad.active());
    press(&mut h, 0, Button::South);
    assert_eq!(h.state().ui.screen(), Screen::LocalPlay);

    focus_on(&mut h, Button::DPadDown, "Mod");
    let mod_id = |h: &Harness<'static, App>| h.state().ui.settings().gameplay.match_prefs.mod_id.clone();
    assert_eq!(mod_id(&h), "olx-classic");
    press(&mut h, 0, Button::DPadRight);
    assert_eq!(mod_id(&h), "olx-mw-1-0");
    assert_eq!(focused(&h).as_deref(), Some("Mod"), "left and right step a drop-down, the focus stays");
    press(&mut h, 0, Button::DPadRight);
    assert_eq!(mod_id(&h), "olx-mw-1-0", "no step past the last");
    press(&mut h, 0, Button::DPadLeft);
    assert_eq!(mod_id(&h), "olx-classic");

    // A opens the list, down and A pick an entry and close it, the focus
    // back on the drop-down; B closes an open list without going back.
    press(&mut h, 0, Button::South);
    assert!(egui::Popup::is_any_open(&h.ctx));
    press(&mut h, 0, Button::DPadDown);
    assert_eq!(focused(&h).as_deref(), Some("Liero v1.0"));
    press(&mut h, 0, Button::DPadDown);
    assert_eq!(focused(&h).as_deref(), Some("Modern Warfare1.0"));
    press(&mut h, 0, Button::South);
    frame(&mut h);
    assert_eq!(mod_id(&h), "olx-mw-1-0");
    assert!(!egui::Popup::is_any_open(&h.ctx));
    assert_eq!(focused(&h).as_deref(), Some("Mod"));
    press(&mut h, 0, Button::South);
    assert!(egui::Popup::is_any_open(&h.ctx));
    press(&mut h, 0, Button::East);
    assert!(!egui::Popup::is_any_open(&h.ctx));
    assert_eq!(h.state().ui.screen(), Screen::LocalPlay);
    assert_eq!(focused(&h).as_deref(), Some("Mod"));

    focus_on(&mut h, Button::DPadDown, "Start");
    press(&mut h, 0, Button::South);
    assert!(h.state().ui.is_starting());
    let s = starts(h.state());
    assert_eq!(s.len(), 1, "{:?}", h.state().commands);
    assert_eq!(s[0].mod_id, "olx-mw-1-0");
}

/// A slider and a drop-down of the options with left and right, the
/// left stick moving the focus, B back to the main menu.
#[test]
fn a_pad_changes_an_option() {
    let mut h = in_menus();
    // The left stick pushed down and let go: a press.
    for v in [-0.9, -0.5, 0.0, -0.3] {
        h.state_mut().data.pad.on_pad(0, PadEvent::Axis(Axis::LeftStickY, v));
    }
    frame(&mut h);
    assert_eq!(focused(&h).as_deref(), Some("Local Play"));
    frame(&mut h);
    assert_eq!(focused(&h).as_deref(), Some("Local Play"), "one press");
    focus_on(&mut h, Button::DPadDown, "Options");
    press(&mut h, 0, Button::South);
    assert_eq!(h.state().ui.screen(), Screen::Options);

    focus_on(&mut h, Button::DPadDown, "Blood");
    let video = |h: &Harness<'static, App>| h.state().ui.settings().video.clone();
    assert_eq!(video(&h).blood, 100);
    press(&mut h, 0, Button::DPadRight);
    assert_eq!(video(&h).blood, 110);
    press(&mut h, 0, Button::DPadLeft);
    press(&mut h, 0, Button::DPadLeft);
    assert_eq!(video(&h).blood, 90);
    assert_eq!(focused(&h).as_deref(), Some("Blood"));

    focus_on(&mut h, Button::DPadDown, "HUD layout");
    assert_eq!(video(&h).hud_layout, HudArrangement::Auto);
    press(&mut h, 0, Button::DPadRight);
    assert_eq!(video(&h).hud_layout, HudArrangement::ALL[1]);

    press(&mut h, 0, Button::East);
    assert_eq!(h.state().ui.screen(), Screen::Main);
    assert_eq!(video(&h).hud_layout, HudArrangement::ALL[1], "kept");
}

/// A text field is skipped (there is no on-screen keyboard): the
/// profiles' name and skin search are passed over.
#[test]
fn a_pad_skips_text_fields() {
    let mut h = in_menus();
    focus_on(&mut h, Button::DPadDown, "Profiles");
    press(&mut h, 0, Button::South);
    assert_eq!(h.state().ui.screen(), Screen::Profiles);
    // The name field is right above the colour sliders: up from Red
    // passes it.
    focus_on(&mut h, Button::DPadDown, "Red");
    press(&mut h, 0, Button::DPadUp);
    assert!(!h.ctx.text_edit_focused());
    assert!(!matches!(focused(&h).as_deref(), None | Some("Red" | "Name")), "{:?}", focused(&h));
    // Every way, every widget: never a text field.
    let mut seen = Vec::new();
    for b in [Button::DPadDown, Button::DPadRight, Button::DPadDown, Button::DPadUp, Button::DPadLeft] {
        for _ in 0..8 {
            press(&mut h, 0, b);
            assert!(!h.ctx.text_edit_focused(), "{b:?} after {seen:?}");
            assert!(focused(&h).is_some(), "{b:?} after {seen:?}");
            seen.push(focused(&h));
        }
    }
    assert!(seen.iter().flatten().any(|l| l == "Red"), "the colour sliders below the name: {seen:?}");
}

/// While a match runs the menus hear no pad; the pause menu hears the
/// pad whose Start paused, and its Options too; after Esc paused, any.
#[test]
fn the_pause_menu_works_from_the_pad_that_paused() {
    let mut h = in_menus();
    for a in [UiAction::Open(Screen::LocalPlay), UiAction::StartMatch, UiAction::MatchStarted] {
        h.state_mut().ui.update(a);
    }
    h.state_mut().ui.update(UiAction::ChooseWeapons { weapons: [0; 5], names: vec![] });
    frame(&mut h);
    assert_eq!(h.state().ui.screen(), Screen::InGame);
    for b in [Button::DPadDown, Button::South, Button::East] {
        press(&mut h, 1, b);
    }
    assert!(!h.state().ui.is_paused(), "B is no Esc in a match");
    assert_eq!(focused(&h), None);

    // Pad 1's Start pauses (the app's `PadPause`).
    h.state_mut().data.pad.on_pad(1, PadEvent::Button(Button::Start, 1.0));
    h.state_mut().ui.update(UiAction::PadPause);
    h.state_mut().data.pad.on_pad(1, PadEvent::Button(Button::Start, 0.0));
    frame(&mut h);
    assert!(h.state().ui.is_paused());
    for b in [Button::DPadDown, Button::South, Button::East] {
        press(&mut h, 2, b);
    }
    assert!(h.state().ui.is_paused(), "another pad");
    assert_eq!(focused(&h), None);
    press(&mut h, 1, Button::DPadDown);
    assert_eq!(focused(&h).as_deref(), Some("Resume"));
    press(&mut h, 1, Button::DPadDown);
    assert_eq!(focused(&h).as_deref(), Some("Options"));
    press(&mut h, 1, Button::South);
    assert_eq!(h.state().ui.screen(), Screen::Options);
    press(&mut h, 2, Button::East);
    assert_eq!(h.state().ui.screen(), Screen::Options, "another pad");
    press(&mut h, 1, Button::East);
    assert_eq!(h.state().ui.screen(), Screen::InGame);
    assert!(h.state().ui.is_paused());
    press(&mut h, 1, Button::DPadDown);
    assert_eq!(focused(&h).as_deref(), Some("Resume"));
    press(&mut h, 1, Button::South);
    assert!(!h.state().ui.is_paused());

    // Esc paused: any pad; B resumes.
    h.key_press(egui::Key::Escape);
    frame(&mut h);
    assert!(h.state().ui.is_paused());
    press(&mut h, 2, Button::DPadDown);
    assert_eq!(focused(&h).as_deref(), Some("Resume"));
    press(&mut h, 2, Button::East);
    assert!(!h.state().ui.is_paused());
    assert_eq!(h.state().ui.screen(), Screen::InGame);
}

/// The weapon selection is the game's keys' (a pad's too): the menus'
/// pad does nothing there.
#[test]
fn the_weapon_selection_hears_no_menu_pad() {
    let mut h = in_menus();
    for a in [UiAction::Open(Screen::LocalPlay), UiAction::StartMatch, UiAction::MatchStarted] {
        h.state_mut().ui.update(a);
    }
    frame(&mut h);
    assert_eq!(h.state().ui.screen(), Screen::WeaponSelect);
    for b in [Button::DPadDown, Button::South, Button::East] {
        press(&mut h, 0, b);
    }
    assert_eq!(h.state().ui.screen(), Screen::WeaponSelect);
    assert_eq!(focused(&h), None);
}

/// A held direction repeats after `REPEAT_DELAY`, every `REPEAT_EVERY`;
/// A and B never; a stick has a release margin; a pad gone drops its
/// presses.
#[test]
fn held_directions_repeat_and_sticks_settle() {
    let mut p = PadNav::default();
    p.on_pad(0, PadEvent::Button(Button::DPadDown, 1.0));
    assert_eq!(p.next(0.0, Who::Anyone), Some(Nav::Down));
    assert_eq!(p.next(0.1, Who::Anyone), None);
    assert_eq!(p.next(REPEAT_DELAY - 0.01, Who::Anyone), None);
    assert_eq!(p.next(REPEAT_DELAY, Who::Anyone), Some(Nav::Down));
    assert_eq!(p.next(REPEAT_DELAY + REPEAT_EVERY / 2.0, Who::Anyone), None);
    assert_eq!(p.next(REPEAT_DELAY + REPEAT_EVERY, Who::Anyone), Some(Nav::Down));
    p.on_pad(0, PadEvent::Button(Button::DPadDown, 0.0));
    assert_eq!(p.next(5.0, Who::Anyone), None);

    p.on_pad(0, PadEvent::Button(Button::South, 1.0));
    assert_eq!(p.next(6.0, Who::Anyone), Some(Nav::Activate));
    assert_eq!(p.next(9.0, Who::Anyone), None, "A does not repeat");
    p.on_pad(0, PadEvent::Button(Button::South, 0.0));

    for (v, nav) in [(0.7, Some(Nav::Right)), (0.5, None), (0.3, None), (0.65, Some(Nav::Right))] {
        p.on_pad(3, PadEvent::Axis(Axis::LeftStickX, v));
        assert_eq!(p.next(10.0, Who::Anyone), nav, "{v}");
    }
    p.on_pad(3, PadEvent::Axis(Axis::LeftStickX, -0.8));
    assert_eq!(p.next(10.0, Who::Anyone), Some(Nav::Left));
    p.on_pad(3, PadEvent::Axis(Axis::LeftStickY, 0.8));
    assert_eq!(p.next(10.0, Who::Anyone), Some(Nav::Up));
    p.on_pad(3, PadEvent::Axis(Axis::LeftStickX, 0.0));
    p.on_pad(3, PadEvent::Axis(Axis::LeftStickY, 0.0));

    p.on_pad(4, PadEvent::Button(Button::East, 1.0));
    p.on_pad(4, PadEvent::Disconnected);
    assert_eq!(p.next(11.0, Who::Only(4)), None);
    p.on_pad(5, PadEvent::Button(Button::East, 1.0));
    assert_eq!(p.next(11.0, Who::Only(4)), None, "another pad's press is dropped");
    assert_eq!(p.next(11.0, Who::Anyone), None);
}
