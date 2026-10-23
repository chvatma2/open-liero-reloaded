//! Issue #55: the skin editor fits the window at every size `olr-ui`'s
//! screens do (issue #49: borderless fullscreen at UI scale 3 on 16:10
//! and 4:3 desktops, 1280 × 720 at UI scale 3 and 1, 4K and an
//! ultrawide), and at `flows.rs`' 1400 × 860 points, in the game's theme
//! and at the menus' scale (`olr-game`'s `scale_height`: the window's
//! height), a classic and a double-resolution skin with the skin light on,
//! below `ui::NARROW` each side panel in turn, and the Name dialog open.
//!
//! What fits (`olr-ui/tests/fits.rs`' way): every widget lies across the
//! window; side by side, every widget of the frames panel lies within the
//! panel; and every enabled control, focused (as Tab or a screen reader
//! would), is scrolled into the window whole and not clipped. egui's
//! scroll areas add no accessibility node and a scrolled-out widget
//! reports its unclipped bounds, so only the focused control's rectangle
//! is checked down the window.

use egui::accesskit::{Action, NodeId, Role};
use egui::containers::panel::PanelState;
use egui::{Id, Rect, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use olr_skin_editor::state::{DEFAULT_LIGHT, frame_name};
use olr_skin_editor::ui::{self, SkinEditorData, labels};
use olr_skin_editor::{Dialog, SkinAction as A, SkinDoc, SkinEditorState};

use crate::common::classic;

/// The windows: width and height in pixels, and the pixels a point.
fn sizes() -> Vec<(f32, f32, f32)> {
    // `olr-ui/tests/fits.rs`' `SIZES`, at the menus' scale.
    let mut out: Vec<_> = [
        (1920.0, 1200.0, 3.0),
        (1024.0, 768.0, 3.0),
        (1280.0, 720.0, 3.0),
        (1280.0, 720.0, 1.0),
        (3840.0, 2160.0, 1.0),
        (3440.0, 1440.0, 1.0),
    ]
    .into_iter()
    .map(|(w, h, scale)| (w, h, olr_ui::theme::pixels_per_point(h, scale)))
    .collect();
    // `flows.rs`' harness.
    out.push((1400.0, 860.0, 1.0));
    out
}

/// A frame button's size at full size (`ui/frames.rs`' `THUMB`).
const THUMB: egui::Vec2 = egui::vec2(60.0, 54.0);

struct App {
    state: SkinEditorState,
    data: SkinEditorData,
}

/// The editor on `state` in a window of `w` × `h` pixels at `ppp`.
fn harness(w: f32, h: f32, ppp: f32, state: SkinEditorState) -> Harness<'static, App> {
    let app = App { state, data: SkinEditorData::default() };
    let h = Harness::builder().with_size(vec2(w / ppp, h / ppp)).with_pixels_per_point(ppp).build_ui_state(
        |ui, app: &mut App| {
            for a in ui::show(ui, &app.state, &mut app.data) {
                app.state.update(a);
            }
        },
        app,
    );
    olr_ui::theme::apply(&h.ctx);
    // A scroll follows the focus at once (no animation to wait for).
    h.ctx.all_styles_mut(|s| s.scroll_animation = egui::style::ScrollAnimation::none());
    h
}

/// A classic skin, or the double-resolution one made from it; the skin
/// light on (its sliders shown).
fn skin(double: bool) -> SkinEditorState {
    let mut s = SkinEditorState::new(SkinDoc::new(classic()).unwrap(), None);
    if double {
        s.update(A::MakeDouble);
        assert_eq!(s.doc().frame_size(), (40, 36));
    }
    s.update(A::SetLight(Some(DEFAULT_LIGHT)));
    s
}

/// A widget the accessibility tree has: its name, bounds (pixels), id,
/// and whether it is a control that can take the focus.
struct Widget {
    name: String,
    rect: Rect,
    id: NodeId,
    focusable: bool,
}

fn widgets(h: &Harness<'static, App>) -> Vec<Widget> {
    let mut out = Vec::new();
    for n in h.root().children_recursive() {
        let a = n.accesskit_node();
        let widget = matches!(
            a.role(),
            Role::Button
                | Role::Label
                | Role::Slider
                | Role::CheckBox
                | Role::RadioButton
                | Role::ComboBox
                | Role::TextInput
                | Role::SpinButton
                | Role::ColorWell
                | Role::Image
                | Role::Unknown
        );
        let (true, Some(b)) = (widget, a.bounding_box()) else { continue };
        let name = format!("{:?} {:?}", a.role(), a.label().or(a.value()).unwrap_or_default());
        let rect =
            Rect::from_min_max(egui::pos2(b.x0 as f32, b.y0 as f32), egui::pos2(b.x1 as f32, b.y1 as f32));
        let focusable = a.data().supports_action(Action::Focus) && !a.is_disabled();
        out.push(Widget { name, rect, id: a.locate().0, focusable });
    }
    out
}

fn settle(h: &mut Harness<'static, App>) {
    h.run();
    h.step();
    h.step();
}

/// `r` (points) in pixels.
fn pixels(r: Rect, ppp: f32) -> Rect {
    Rect::from_min_max((r.min.to_vec2() * ppp).to_pos2(), (r.max.to_vec2() * ppp).to_pos2())
}

/// What of the editor does not fit its window (the module's comment);
/// `frames`: the frames panel shows.
fn misfits(h: &mut Harness<'static, App>, frames: bool) -> Vec<String> {
    settle(h);
    let ppp = h.ctx.pixels_per_point();
    let window = h.ctx.content_rect();
    let px = pixels(window, ppp);
    let found = widgets(h);
    let mut bad = Vec::new();
    if found.len() < 2 {
        bad.push("nothing shown".to_string());
    }
    for w in &found {
        if w.rect.left() < px.left() - 0.5 || w.rect.right() > px.right() + 0.5 {
            bad.push(format!("{} at {:?} leaves the window across", w.name, w.rect));
        }
    }
    if frames {
        let panel = PanelState::load(&h.ctx, Id::new("olr-skin-editor-frames")).expect("the frames panel");
        let panel = pixels(panel.outer_rect, ppp);
        for w in found.iter().filter(|w| w.rect.left() >= panel.left() - 0.5) {
            if w.rect.right() > panel.right() + 0.5 {
                bad.push(format!("{} at {:?} leaves the frames panel {panel:?}", w.name, w.rect));
            }
        }
    }
    for w in found.iter().filter(|w| w.focusable) {
        let Some(n) = h.root().children_recursive().find(|n| n.accesskit_node().locate().0 == w.id) else {
            // Gone: an earlier control's focus changed the panel (a
            // slider's value turned into a field).
            continue;
        };
        n.focus();
        // The focus lands, then each scroll area holding it follows.
        h.run_steps(12);
        let Some(id) = h.ctx.memory(|m| m.focused()).filter(|id| id.accesskit_id() == w.id) else {
            // Under a modal dialog the editor's controls wait for it.
            if h.ctx.memory(|m| m.top_modal_layer()).is_none() {
                bad.push(format!("{} cannot take the focus", w.name));
            }
            continue;
        };
        let r = h.ctx.read_response(id).expect("the focused widget's response");
        h.step();
        let again = h.ctx.read_response(id).expect("the focused widget's response");
        if again.rect != r.rect {
            bad.push(format!("{} at {:?} keeps moving (to {:?})", w.name, r.rect, again.rect));
        }
        if !window.expand(0.5).contains_rect(r.rect) {
            bad.push(format!("{} at {:?} cannot be scrolled into the window {window:?}", w.name, r.rect));
        } else if !r.interact_rect.expand(0.5).contains_rect(r.rect) {
            bad.push(format!("{} at {:?} is clipped to {:?}", w.name, r.rect, r.interact_rect));
        }
        h.ctx.memory_mut(|m| m.surrender_focus(id));
    }
    h.run();
    bad.dedup();
    bad
}

/// Shows the side panel `label` (the session bar's switch).
fn switch(h: &mut Harness<'static, App>, label: &str) {
    h.get_by_role_and_label(Role::Button, label).click_accesskit();
    settle(h);
}

#[test]
fn the_editor_fits_every_window() {
    let mut failed = Vec::new();
    let mut cases = 0;
    for (w, hgt, ppp) in sizes() {
        let narrow = w / ppp < ui::NARROW;
        for double in [false, true] {
            // Side by side, or below `NARROW` each side panel in turn; the
            // Name dialog over the classic skin.
            let mut setups: Vec<(&str, Option<&str>, bool)> = if narrow {
                vec![("tools", Some(labels::SHOW_TOOLS), false), ("frames", Some(labels::SHOW_FRAMES), false)]
            } else {
                vec![("both panels", None, false)]
            };
            if !double {
                setups.push(("the Name dialog", None, true));
            }
            for (what, side, dialog) in setups {
                cases += 1;
                let mut h = harness(w, hgt, ppp, skin(double));
                settle(&mut h);
                assert_eq!(h.query_by_role_and_label(Role::Button, labels::SHOW_FRAMES).is_some(), narrow);
                if let Some(side) = side {
                    switch(&mut h, side);
                }
                if dialog {
                    h.state_mut().state.update(A::Save);
                    settle(&mut h);
                    assert_eq!(h.state().state.dialog(), Some(&Dialog::Name));
                }
                let frames = !narrow || side == Some(labels::SHOW_FRAMES);
                let bad = misfits(&mut h, frames);
                if !bad.is_empty() {
                    let kind = if double { "double resolution" } else { "classic" };
                    let at = format!("{kind}, {what}, at {w}x{hgt} ({ppp} pixels a point)");
                    let first = bad.iter().take(8).map(|b| format!("    {b}")).collect::<Vec<_>>().join("\n");
                    failed.push(format!("{at}: {} problems, e.g.\n{first}", bad.len()));
                }
            }
        }
    }
    assert!(cases >= 20);
    assert!(failed.is_empty(), "{} cases do not fit:\n{}", failed.len(), failed.join("\n"));
}

/// Narrow, the switch shows one side panel, the other gone; the canvas
/// keeps the rest, and the editor's state is untouched.
#[test]
fn narrow_windows_switch_between_the_side_panels() {
    let mut h = harness(1280.0, 720.0, olr_ui::theme::pixels_per_point(720.0, 3.0), skin(false));
    settle(&mut h);
    h.get_by_label(labels::FLIP);
    assert!(h.query_by_label(&frame_name(0)).is_none(), "tools first");
    let before = (h.state().state.frame(), h.state().state.tool(), h.state().state.dirty());
    switch(&mut h, labels::SHOW_FRAMES);
    assert!(h.query_by_label(labels::FLIP).is_none());
    h.get_by_label(&frame_name(0));
    assert_eq!((h.state().state.frame(), h.state().state.tool(), h.state().state.dirty()), before);
    let canvas = h.state().data.canvas_rect().expect("the canvas");
    assert!(canvas.width() > 40.0, "the canvas keeps room: {canvas:?}");
    switch(&mut h, labels::SHOW_TOOLS);
    h.get_by_label(labels::FLIP);
}

/// At `flows.rs`' size the three columns show as before, the frames at
/// full size.
#[test]
fn a_large_window_shows_three_columns_and_full_size_frames() {
    let mut h = harness(1400.0, 860.0, 1.0, skin(false));
    settle(&mut h);
    assert!(h.query_by_role_and_label(Role::Button, labels::SHOW_FRAMES).is_none());
    let tools = PanelState::load(&h.ctx, Id::new("olr-skin-editor-tools")).expect("the tools").outer_rect;
    let frames = PanelState::load(&h.ctx, Id::new("olr-skin-editor-frames")).expect("the frames").outer_rect;
    let canvas = h.state().data.canvas_rect().expect("the canvas");
    assert!(
        tools.right() <= canvas.left() && canvas.right() <= frames.left(),
        "{tools:?} {canvas:?} {frames:?}"
    );
    assert!(canvas.width() > 400.0, "{canvas:?}");
    for f in 0..21 {
        let b = h.get_by_label(&frame_name(f)).accesskit_node().bounding_box().expect("bounds");
        let size = vec2((b.x1 - b.x0) as f32, (b.y1 - b.y0) as f32);
        assert!((size - THUMB).length() < 0.01, "{}: {size:?}", frame_name(f));
    }
}
