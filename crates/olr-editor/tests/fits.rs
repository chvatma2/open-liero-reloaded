//! Issue #78: the asset tab fits the window at every size `olr-ui`'s
//! screens do (issue #49: borderless fullscreen at UI scale 3 on 16:10
//! and 4:3 desktops, 1280 × 720 at UI scale 3 and 1, 4K and an
//! ultrawide), and at 1400 × 860 points, in the game's theme
//! (`olr_ui::theme`, a dev-dependency) and at the menus' scale (the
//! window's height / 720 × the UI scale), below `ui::NARROW` each side
//! panel in turn: the built-in ceiling lamp with nothing selected (the
//! asset's panel, its Lighting and Materials) and the Draw tool, with a
//! layer selected (its properties) and the Glow brush, with that layer
//! made glass (its sliders) and the Surface brush, and (issue #79) with
//! 32 states, the most an asset has.
//!
//! What fits (the skin editor's `tests/fits.rs`, issue #55): every widget
//! lies across the window, and each side panel's within it; every enabled
//! control, focused (as Tab or a screen reader would), is scrolled into
//! the window whole and not clipped. egui's scroll areas add no
//! accessibility node and a scrolled-out widget reports its unclipped
//! bounds, so only the focused control's rectangle is checked down the
//! window. The script editor (issue #80) is a text area as large as its
//! script (its lines unwrapped), in the central panel's scroll area: only
//! its top-left corner is checked.

use egui::accesskit::{Action, NodeId, Role};
use egui::containers::panel::PanelState;
use egui::{Id, Rect, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use olr_edit::element::Property;
use olr_editor::tools::Tool;
use olr_editor::ui::{self, EditorData, labels};
use olr_editor::{AssetKey, DocTab, EditorAction as A, EditorState, Selection};

use olr_format::levels::ElementGlass;

use crate::common::{built_in_assets, content, editor, run, set_library};

/// The windows: width and height in pixels, and the pixels a point.
pub fn sizes() -> Vec<(f32, f32, f32)> {
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
    out.push((1400.0, 860.0, 1.0));
    out
}

/// The side panels' ids (`ui/asset.rs`).
const TOOLS: &str = "olr-editor-asset-tools";
const LAYERS: &str = "olr-editor-asset-layers";

pub struct App {
    pub state: EditorState,
    pub data: EditorData,
}

/// The editor on `state` in a window of `w` × `h` pixels at `ppp`, under
/// the game's theme.
pub fn harness(w: f32, h: f32, ppp: f32, state: EditorState) -> Harness<'static, App> {
    let app = App { state, data: EditorData::default() };
    let h = Harness::builder().with_size(vec2(w / ppp, h / ppp)).with_pixels_per_point(ppp).build_ui_state(
        |ui, app: &mut App| {
            for a in ui::show(ui, &app.state, &mut app.data) {
                run(&mut app.state, a);
            }
        },
        app,
    );
    olr_ui::theme::apply(&h.ctx);
    // A scroll follows the focus at once (no animation to wait for).
    h.ctx.all_styles_mut(|s| s.scroll_animation = egui::style::ScrollAnimation::none());
    h
}

/// The fixture level open and the built-in ceiling lamp in the asset tab.
pub fn lamp_open() -> Option<(tempfile::TempDir, EditorState)> {
    let root = content()?;
    let (tmp, mut s) = editor(&root);
    let assets = set_library(&built_in_assets(), &tmp.path().join("yours"));
    run(&mut s, A::AssetsChanged(assets));
    run(&mut s, A::EditAsset(AssetKey { builtin: true, id: "ceiling-lamp".into() }));
    assert_eq!(s.doc_tab(), DocTab::Asset);
    Some((tmp, s))
}

/// A widget the accessibility tree has: its name, bounds (pixels), id,
/// and whether it is a control that can take the focus.
struct Widget {
    name: String,
    rect: Rect,
    id: NodeId,
    focusable: bool,
    /// A text area larger than the room may be (the script editor).
    area: bool,
}

/// Of a widget's rectangle, what has to fit: all of it, or a text area's
/// top-left corner.
fn fitting(r: Rect, area: bool) -> Rect {
    if area { Rect::from_min_size(r.min, vec2(1.0, 1.0)) } else { r }
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
        let area = a.role() == Role::TextInput && a.label().as_deref() == Some(labels::SCRIPT_EDITOR);
        let rect = fitting(rect, area);
        out.push(Widget { name, rect, id: a.locate().0, focusable, area });
    }
    out
}

pub fn settle(h: &mut Harness<'static, App>) {
    h.run();
    h.step();
    h.step();
}

/// `r` (points) in pixels.
fn pixels(r: Rect, ppp: f32) -> Rect {
    Rect::from_min_max((r.min.to_vec2() * ppp).to_pos2(), (r.max.to_vec2() * ppp).to_pos2())
}

/// What of the asset tab does not fit its window (the module's comment);
/// `panels`: the side panels shown.
fn misfits(h: &mut Harness<'static, App>, panels: &[&str]) -> Vec<String> {
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
    for id in panels {
        let panel = PanelState::load(&h.ctx, Id::new(*id)).expect("the side panel");
        let panel = pixels(panel.outer_rect, ppp);
        for w in found.iter().filter(|w| panel.contains(w.rect.center())) {
            if w.rect.left() < panel.left() - 0.5 || w.rect.right() > panel.right() + 0.5 {
                bad.push(format!("{} at {:?} leaves its panel {panel:?}", w.name, w.rect));
            }
        }
    }
    for w in found.iter().filter(|w| w.focusable) {
        let Some(n) = h.root().children_recursive().find(|n| n.accesskit_node().locate().0 == w.id) else {
            // Gone: an earlier control's focus changed the panel.
            continue;
        };
        n.focus();
        // The focus lands, then each scroll area holding it follows.
        h.run_steps(12);
        let Some(id) = h.ctx.memory(|m| m.focused()).filter(|id| id.accesskit_id() == w.id) else {
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
        let rect = fitting(r.rect, w.area);
        if !window.expand(0.5).contains_rect(rect) {
            bad.push(format!("{} at {:?} cannot be scrolled into the window {window:?}", w.name, r.rect));
        } else if !r.interact_rect.expand(0.5).contains_rect(rect) {
            bad.push(format!("{} at {:?} is clipped to {:?}", w.name, r.rect, r.interact_rect));
        }
        h.ctx.memory_mut(|m| m.surrender_focus(id));
    }
    h.run();
    bad.dedup();
    bad
}

/// Shows the side panel `label` (the session bar's narrow switch).
pub fn switch(h: &mut Harness<'static, App>, label: &str) {
    h.get_by_role_and_label(Role::Button, label).click_accesskit();
    settle(h);
}

/// Issue #80: the lamp's Offset X and health exposed, the second state's
/// own glow, health, a sound looping in it and a script with an error.
fn parts(s: &mut EditorState) {
    use olr_format::assets::{PropertySource as P, ValueSource as V};
    run(s, A::Expose { property: P::OffsetX("lamps".into()), default: None });
    run(s, A::Destructible(true));
    run(s, A::Expose { property: P::Health, default: None });
    run(
        s,
        A::SoundRead {
            name: "a-rather-long-sound-name-to-fit.wav".into(),
            bytes: crate::properties::wav(200),
        },
    );
    run(s, A::SelectState(1));
    run(s, A::SetValue { property: P::GlowStrength("lamps".into()), value: Some(V::Int(20)) });
    run(
        s,
        A::StateSounds {
            loop_sound: Some(Some("a-rather-long-sound-name-to-fit.wav".into())),
            enter_sound: None,
        },
    );
    run(s, A::SetScript("fn alarm() {\n    let x = ;\n}\n".into()));
    let p = s.active_document().unwrap().parts_snap().unwrap();
    assert_eq!((p.exposed.len(), p.sounds.len()), (2, 1), "{:?}", s.status().message);
    assert!(p.script.is_some() && p.health.is_some());
}

#[test]
fn the_asset_tab_fits() {
    if content().is_none() {
        return;
    }
    let mut failed = Vec::new();
    let mut cases = 0;
    for (w, hgt, ppp) in sizes() {
        let narrow = w / ppp < ui::NARROW;
        let sides: &[Option<&str>] =
            if narrow { &[Some(labels::TOOLS_TAB), Some(labels::LAYERS_TAB)] } else { &[None] };
        // Nothing selected and Draw; a layer and the Glow brush; the layer
        // glass and the Surface brush.
        let lamps = || Selection::Element("lamps".into());
        // Issue #79: an asset of 32 states (`MAX_STATES`), every state's
        // button reachable without a horizontal scroll.
        // Issue #80: each of the right column's tabs, on an asset with an
        // exposed property, a state's value, health, a sound and a broken
        // script (the script editor open in the middle).
        let setups = [
            ("the asset, Draw", Selection::None, Tool::Brush, false, 1, None),
            ("a layer, the Glow brush", lamps(), Tool::Glow, false, 1, None),
            ("a glass layer, the Surface brush", lamps(), Tool::Surface, true, 1, None),
            ("an asset of 32 states", Selection::None, Tool::Brush, false, 32, None),
            ("the Properties tab", Selection::None, Tool::Brush, true, 2, Some(labels::PROPERTIES)),
            ("the Sounds tab", Selection::None, Tool::Brush, false, 2, Some(labels::SOUNDS)),
            ("the Script tab", Selection::None, Tool::Brush, false, 2, Some(labels::SCRIPT)),
        ];
        for &side in sides {
            for (what, selection, tool, glass, states, column) in &setups {
                cases += 1;
                let (_tmp, mut s) = lamp_open().unwrap();
                for _ in 1..*states {
                    run(&mut s, A::AddState);
                }
                assert_eq!(s.active_document().unwrap().states().len(), *states);
                if *glass {
                    let g = ElementGlass { opacity: 35, tint: 0, frost: 0 };
                    run(&mut s, A::Set("lamps".into(), Property::Glass(Some(g))));
                    assert!(s.active_dirty(), "made glass");
                }
                if column.is_some() {
                    parts(&mut s);
                }
                run(&mut s, A::Select(selection.clone()));
                run(&mut s, A::Tool(*tool));
                assert_eq!((s.selection(), s.tool()), (selection, *tool));
                let mut h = harness(w, hgt, ppp, s);
                settle(&mut h);
                let switch_shown = h.query_by_role_and_label(Role::Button, labels::LAYERS_TAB).is_some();
                assert_eq!(switch_shown, narrow, "{w}x{hgt}");
                if let Some(column) = column {
                    if narrow {
                        switch(&mut h, labels::LAYERS_TAB);
                    }
                    h.get_by_role_and_label(Role::Button, column).click_accesskit();
                    settle(&mut h);
                }
                if let Some(side) = side {
                    switch(&mut h, side);
                }
                let panels: &[&str] = match side {
                    None => &[TOOLS, LAYERS],
                    Some(labels::TOOLS_TAB) => &[TOOLS],
                    Some(_) => &[LAYERS],
                };
                let bad = misfits(&mut h, panels);
                if !bad.is_empty() {
                    let panel = side.unwrap_or("both panels");
                    let at = format!("{what}, {panel}, at {w}x{hgt} ({ppp} pixels a point)");
                    let first = bad.iter().take(8).map(|b| format!("    {b}")).collect::<Vec<_>>().join("\n");
                    failed.push(format!("{at}: {} problems, e.g.\n{first}", bad.len()));
                }
            }
        }
    }
    assert!(cases >= 30, "{cases}");
    assert!(failed.is_empty(), "{} cases do not fit:\n{}", failed.len(), failed.join("\n"));
}

/// Issue #78: a tool's options show in the tools panel, under the tools:
/// picking Draw shows its brush's size there.
#[test]
fn the_options_are_under_the_tools() {
    let Some((_tmp, mut s)) = lamp_open() else { return };
    run(&mut s, A::Tool(Tool::Select));
    let mut h = harness(1280.0, 720.0, 1.0, s);
    settle(&mut h);
    assert!(h.query_by_role_and_label(Role::SpinButton, "Size").is_none(), "Select has no brush");
    h.get_by_label(labels::DRAW).click();
    settle(&mut h);
    assert_eq!(h.state().state.tool(), Tool::Brush);
    let draw = h.get_by_label(labels::DRAW).rect();
    let size = h.get_by_role_and_label(Role::SpinButton, "Size").rect();
    let tools = PanelState::load(&h.ctx, Id::new(TOOLS)).expect("the tools panel").outer_rect;
    assert!(size.top() > draw.bottom(), "under the tools: {size:?}, Draw {draw:?}");
    assert!(tools.contains_rect(size) && tools.contains_rect(draw), "in the tools panel {tools:?}");
}

/// The level tab's side panels (`ui/level_tab.rs`).
const LIBRARY: &str = "olr-editor-level-library";
const COPY: &str = "olr-editor-level-copy";

/// Issue #83: the level tab fits as the asset tab does (the module's
/// rules; `the_toolbar_stays_in_the_window_and_in_place` before it), at
/// the same sizes, below `ui::NARROW` each side panel in turn: a level of
/// placed copies (the ground, a lamp, a stretched torch, an own strip
/// light) with nothing selected (the level's panel: its Lighting and size
/// fields; then with its Fine tuning open) and Select, a copy selected (its
/// panel and link) and the Haze brush, and the Spawns tab with Spawn.
#[test]
fn the_level_tab_fits() {
    let Some(root) = content() else { return };
    let mut failed = Vec::new();
    let mut cases = 0;
    for (w, hgt, ppp) in sizes() {
        let narrow = w / ppp < ui::NARROW;
        let sides: &[Option<&str>] =
            if narrow { &[Some(labels::SHOW_LIBRARY), Some(labels::SHOW_COPY)] } else { &[None] };
        let setups = [
            ("nothing selected, Select", Selection::None, Tool::Select, false),
            ("the lighting's fine tuning", Selection::None, Tool::Select, false),
            ("a copy, the Haze brush", Selection::Copy("torch".into()), Tool::Haze, false),
            ("the own strip light, Spawn", Selection::Copy("strip-light".into()), Tool::Spawn, false),
            ("the Spawns tab", Selection::None, Tool::Spawn, true),
        ];
        for &side in sides {
            for (what, selection, tool, spawns) in &setups {
                cases += 1;
                let (_t, _t2, mut s) = crate::level_tab::placed(&root);
                if what.contains("fine tuning") {
                    if s.document().unwrap().level().look.is_none() {
                        run(&mut s, A::SetLook(Some(olr_format::look::LookSource::default())));
                    }
                    run(&mut s, A::ShowFineTuning(true));
                    assert!(s.document().unwrap().level().look.is_some(), "{:?}", s.status().message);
                }
                run(&mut s, A::Select(selection.clone()));
                run(&mut s, A::Tool(*tool));
                if *spawns {
                    run(&mut s, A::Tab(olr_editor::PanelTab::Spawns));
                }
                assert_eq!((s.selection(), s.tool()), (selection, *tool));
                let mut h = harness(w, hgt, ppp, s);
                settle(&mut h);
                let switch_shown = h.query_by_role_and_label(Role::Button, labels::SHOW_COPY).is_some();
                assert_eq!(switch_shown, narrow, "{w}x{hgt}");
                if let Some(side) = side {
                    switch(&mut h, side);
                }
                let panels: &[&str] = match side {
                    None => &[LIBRARY, COPY],
                    Some(labels::SHOW_LIBRARY) => &[LIBRARY],
                    Some(_) => &[COPY],
                };
                let bad = misfits(&mut h, panels);
                if !bad.is_empty() {
                    let panel = side.unwrap_or("both panels");
                    let at = format!("{what}, {panel}, at {w}x{hgt} ({ppp} pixels a point)");
                    let first = bad.iter().take(8).map(|b| format!("    {b}")).collect::<Vec<_>>().join("\n");
                    failed.push(format!("{at}: {} problems, e.g.\n{first}", bad.len()));
                }
            }
        }
    }
    assert!(cases >= 20, "{cases}");
    assert!(failed.is_empty(), "{} cases do not fit:\n{}", failed.len(), failed.join("\n"));
}

/// Issue #84: the ceiling lamp with twenty more glowing layers, 64
/// values exposed (the most an asset has), two states and a script.
fn many_values() -> olr_edit::asset::Asset {
    use olr_format::assets::{ExposedSource, PropertySource as P};
    let mut a = olr_edit::asset::load_dir(&built_in_assets().join("ceiling-lamp")).unwrap();
    a.name = "Busy panel".into();
    let lamps = a.pieces.iter().find(|p| p.element.id == "lamps").unwrap().clone();
    for i in 0..20 {
        let mut p = lamps.clone();
        p.element.id = format!("lamp_{i}");
        p.element.name = format!("Lamp {i}");
        a.pieces.push(p);
    }
    for o in olr_edit::asset::offers(&a) {
        if a.exposed.len() == olr_format::assets::MAX_EXPOSED {
            break;
        }
        let name = format!("value_{}", a.exposed.len());
        let (min, max) = match o.property {
            P::X | P::Y | P::OffsetX(_) | P::OffsetY(_) => (Some(-8), Some(8)),
            _ => (None, None),
        };
        let x = ExposedSource { name, property: o.property, default: None, min, max };
        let _shared = olr_edit::asset::expose(&mut a, x);
    }
    assert_eq!(a.exposed.len(), olr_format::assets::MAX_EXPOSED);
    olr_edit::asset::add_state(&mut a, "Alarm", None).unwrap();
    a.script = Some("fn alarm() {}\n".into());
    a
}

/// A saved level of ten consoles from the user's library, opened again
/// after the library changed four, lost three and broke three: the
/// notice open.
fn notice_of_ten(root: &std::path::Path) -> (tempfile::TempDir, tempfile::TempDir, EditorState) {
    use olr_editor::level_tab::CopyChange;
    use olr_format::assets::ValueSource as V;
    let (t, t2, mut s, yours) = crate::copy_panel::level(root);
    let mut placed = Vec::new();
    for i in 0..10 {
        let mut a = crate::copy_panel::console();
        a.name = format!("Console with a rather long name {i}");
        let id = crate::copy_panel::place(&mut s, &yours, &a, (20 + 30 * i, 60));
        crate::copy_panel::set(&mut s, &id, CopyChange::Value("glow".into(), Some(V::Int(10))));
        let Some(olr_format::levels::AssetOrigin::Yours(folder)) = s.document().unwrap().copy_origin(&id)
        else {
            panic!("linked")
        };
        placed.push((a, folder));
    }
    run(&mut s, A::Save);
    for (i, (mut a, folder)) in placed.into_iter().enumerate() {
        match i {
            0..4 => a.name.push_str(" mark 2"),
            4..7 => {
                std::fs::remove_dir_all(yours.join(&folder)).unwrap();
                continue;
            }
            _ => a.exposed.retain(|x| x.name != "glow"),
        }
        olr_edit::asset::add_to_library(&yours, &a, Some(&folder)).unwrap();
    }
    let list = set_library(&built_in_assets(), &yours);
    let dir = s.document().unwrap().dir().to_path_buf();
    run(&mut s, A::Open(dir));
    run(&mut s, A::AssetsChanged(list));
    assert!(matches!(s.dialog(), Some(olr_editor::Dialog::LibraryChanged)), "{:?}", s.dialog());
    assert_eq!(s.library_links().iter().filter(|l| l.status != olr_edit::library::Status::Same).count(), 10);
    (t, t2, s)
}

/// Issue #84: the copy panel of a Live copy of an asset exposing 64
/// values (some set), with a starting state, a script and the asset's
/// script, and the library-change notice of ten assets, fit as the level
/// tab does (`the_level_tab_fits`).
#[test]
fn the_copy_panel_and_the_notice_fit() {
    use olr_editor::level_tab::CopyChange;
    let Some(root) = content() else { return };
    let mut failed = Vec::new();
    let mut cases = 0;
    for (w, hgt, ppp) in sizes() {
        let narrow = w / ppp < ui::NARROW;
        let sides: &[Option<&str>] =
            if narrow { &[Some(labels::SHOW_LIBRARY), Some(labels::SHOW_COPY)] } else { &[None] };
        for &side in sides {
            for what in ["the full copy panel", "the notice of ten assets"] {
                cases += 1;
                let (_t, _t2, s) = if what.contains("notice") {
                    notice_of_ten(&root)
                } else {
                    let (t, t2, mut s, yours) = crate::copy_panel::level(&root);
                    let id = crate::copy_panel::place(&mut s, &yours, &many_values(), (60, 60));
                    crate::copy_panel::set(&mut s, &id, CopyChange::Live(true));
                    crate::copy_panel::set(&mut s, &id, CopyChange::State(Some("Alarm".into())));
                    let v = olr_format::assets::ValueSource::Bool(false);
                    crate::copy_panel::set(&mut s, &id, CopyChange::Value("value_2".into(), Some(v)));
                    let script = olr_editor::copy_ops::ScriptTarget::Copy(id.clone());
                    run(&mut s, A::OpenScript(script));
                    run(&mut s, A::CloseScript);
                    run(&mut s, A::Select(Selection::Copy(id)));
                    (t, t2, s)
                };
                let mut h = harness(w, hgt, ppp, s);
                settle(&mut h);
                if let Some(side) = side {
                    switch(&mut h, side);
                }
                // The notice floats over the panels: the window holds it.
                let panels: &[&str] = match side {
                    _ if what.contains("notice") => &[],
                    None => &[LIBRARY, COPY],
                    Some(labels::SHOW_LIBRARY) => &[LIBRARY],
                    Some(_) => &[COPY],
                };
                let bad = misfits(&mut h, panels);
                if !bad.is_empty() {
                    let panel = side.unwrap_or("both panels");
                    let at = format!("{what}, {panel}, at {w}x{hgt} ({ppp} pixels a point)");
                    let first = bad.iter().take(8).map(|b| format!("    {b}")).collect::<Vec<_>>().join("\n");
                    failed.push(format!("{at}: {} problems, e.g.\n{first}", bad.len()));
                }
            }
        }
    }
    assert!(cases >= 8, "{cases}");
    assert!(failed.is_empty(), "{} cases do not fit:\n{}", failed.len(), failed.join("\n"));
}
