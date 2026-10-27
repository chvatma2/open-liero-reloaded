//! The asset tab's properties, health, sounds and script (issue #80): the
//! Properties tab offering exactly `offers`, values per state, exposure,
//! "Also changes", Destructible, the sounds, the refusals naming the entry,
//! the script committed on a pause with its syntax errors listed, and an
//! asset saved with a broken script.

use egui::accesskit::Role;
use egui_kittest::kittest::{NodeT, Queryable};
use olr_edit::asset::{Asset, ExposedChange, Piece};
use olr_editor::browser::Purpose;
use olr_editor::state_ops::HEALTH_ENDS_HERE;
use olr_editor::ui::labels;
use olr_editor::{Choice, Dialog, EditorAction as A, EditorCommand as C, EditorState, Selection};
use olr_format::assets::{HealthSource, PropertySource as P, ValueSource as V};
use olr_format::levels::ElementMaterial;
use olr_format::look::GlowColourSource;

use crate::common::run;
use crate::states::{alarm_lamp, editing, lamp};

/// A WAV of `samples` silent 8-bit samples at 8000 Hz.
pub(crate) fn wav(samples: u32) -> Vec<u8> {
    let mut b = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&[1, 0, 1, 0]);
    b.extend_from_slice(&8000u32.to_le_bytes());
    b.extend_from_slice(&8000u32.to_le_bytes());
    b.extend_from_slice(&[1, 0, 8, 0]);
    b.extend_from_slice(b"data");
    b.extend_from_slice(&samples.to_le_bytes());
    b.resize(b.len() + samples as usize, 128);
    b
}

/// The lamp with a second layer, Buttons, glowing in the lamps' group,
/// and a dirt layer, Mud.
fn shared_lamp() -> Asset {
    let mut a = alarm_lamp();
    let lamps = a.pieces.iter().find(|p| p.element.id == "lamps").unwrap().clone();
    let mut buttons: Piece = lamps.clone();
    buttons.element.id = "buttons".into();
    buttons.element.name = "Buttons".into();
    buttons.glow_mask = None;
    if let Some(g) = buttons.element.glow.as_mut() {
        g.mask = false;
    }
    a.pieces.push(buttons);
    let mut mud = lamps;
    mud.element.id = "mud".into();
    mud.element.name = "Mud".into();
    mud.element.glow = None;
    mud.glow_mask = None;
    mud.element.surface = None;
    if let olr_format::levels::ElementKind::Drawn { material, .. } = &mut mud.element.kind {
        *material = ElementMaterial::Dirt;
    }
    a.pieces.insert(0, mud);
    a
}

fn doc(s: &EditorState) -> &olr_editor::Document {
    s.active_document().unwrap()
}

fn status(s: &EditorState) -> String {
    s.status().message.clone().unwrap_or_default()
}

/// Shows state `name`.
fn show(s: &mut EditorState, name: &str) {
    let i = doc(s).states().iter().position(|st| st.name == name).unwrap();
    run(s, A::SelectState(i));
}

fn exposed(s: &EditorState) -> Vec<olr_format::assets::ExposedSource> {
    doc(s).parts_snap().unwrap().exposed
}

#[test]
fn the_script_limit_is_the_formats() {
    assert_eq!(olr_script::MAX_SCRIPT_BYTES as u64, olr_format::assets::MAX_SCRIPT_BYTES);
}

#[test]
fn the_tree_offers_what_offers_lists() {
    let Some((_tmp, mut s, _)) = editing(&shared_lamp()) else { return };
    let offers = doc(&s).offers();
    let mut h = crate::fits::harness(1400.0, 860.0, 1.0, s);
    h.run();
    h.get_by_label(labels::PROPERTIES).click();
    crate::fits::settle(&mut h);
    s = std::mem::replace(&mut h.state_mut().state, EditorState::new(Default::default(), Default::default()));
    let names = |id: &str| doc(&s).items().iter().find(|i| i.element.id == id).unwrap().element.name.clone();
    // A row (and its Expose switch) for each offer, and no other.
    let mut rows = 0;
    for n in h.root().children_recursive() {
        let label = n.accesskit_node().label().unwrap_or_default();
        if n.accesskit_node().role() == Role::CheckBox && label.starts_with(&format!("{} ", labels::EXPOSE)) {
            rows += 1;
        }
    }
    assert_eq!(rows, offers.len());
    for o in &offers {
        let owner = o.property.layer().map_or(labels::WHOLE_ASSET.to_string(), names);
        let row = h.root().children_recursive().find(|n| {
            let l = n.accesskit_node().label().unwrap_or_default();
            l.starts_with(&format!("{} {owner}: ", labels::EXPOSE))
        });
        assert!(row.is_some(), "{owner}: {:?}", o.property);
    }
    // The dirt layer is listed, greyed, and offers nothing.
    h.get_by_label(&format!("Mud: {}", labels::NOT_OFFERED_DIRT));
    assert!(offers.iter().all(|o| o.property.layer() != Some("mud")));
    // The rock does not glow: no glow rows.
    assert!(h.query_by_label(&labels::expose("Rock: Glow strength")).is_none());
    assert!(h.query_by_label(&labels::expose("Rock: Glow colour")).is_none());
    h.get_by_label(&labels::expose("Lamps: Glow strength"));
    h.get_by_label(&labels::expose("Rock: Offset X"));
    // No health: the whole asset offers none.
    assert!(h.query_by_label(&labels::expose("Whole asset: Health")).is_none());
    h.get_by_label(&labels::expose("Whole asset: Facing"));
}

#[test]
fn editing_a_value_sets_the_selected_states_value() {
    let Some((_tmp, mut s, _)) = editing(&alarm_lamp()) else { return };
    let glow = P::GlowStrength("lamps".into());
    show(&mut s, "Alarm");
    let own = doc(&s).shown_value(&glow);
    assert_eq!((own.value, own.own), (Some(V::Int(100)), false), "the layer's own");
    let before = doc(&s).to_asset();
    run(&mut s, A::SetValue { property: glow.clone(), value: Some(V::Int(30)) });
    let v = doc(&s).shown_value(&glow);
    assert_eq!((v.value, v.own), (Some(V::Int(30)), true));
    let a = doc(&s).to_asset();
    let alarm = a.state("Alarm").unwrap();
    assert_eq!(a.states[alarm].values.last().unwrap().value, V::Int(30));
    assert!(a.states[a.start].values.is_empty(), "the start is untouched");
    // Off sets its own 0.
    show(&mut s, "Off");
    assert_eq!(doc(&s).shown_value(&glow).value, Some(V::Int(0)));
    // One step: undo takes it back, in Alarm.
    run(&mut s, A::Undo);
    assert_eq!(doc(&s).to_asset(), before);
    run(&mut s, A::Redo);
    show(&mut s, "Alarm");
    // Clear: the state follows the default again.
    run(&mut s, A::SetValue { property: glow.clone(), value: None });
    let v = doc(&s).shown_value(&glow);
    assert_eq!((v.value, v.own), (Some(V::Int(100)), false));
    // A value outside the range is refused, the status saying why.
    run(&mut s, A::SetValue { property: glow.clone(), value: Some(V::Int(500)) });
    assert!(status(&s).contains("0 to 100"), "{}", status(&s));
    assert!(!doc(&s).shown_value(&glow).own);
}

#[test]
fn exposing_names_defaults_and_limits() {
    let Some((_tmp, mut s, _)) = editing(&alarm_lamp()) else { return };
    run(&mut s, A::Expose { property: P::OffsetX("lamps".into()), default: None });
    run(&mut s, A::Expose { property: P::GlowStrength("lamps".into()), default: None });
    let x = exposed(&s);
    assert_eq!(x[0].name, "lamps_x");
    assert_eq!(
        (x[0].default.clone(), x[0].min, x[0].max),
        (Some(V::Int(0)), Some(0), Some(0)),
        "at the current value"
    );
    assert_eq!(x[1].name, "lamps_glow");
    assert_eq!(
        (x[1].default.clone(), x[1].min, x[1].max),
        (Some(V::Int(100)), Some(0), Some(100)),
        "its range"
    );
    // Widened, renamed.
    run(&mut s, A::SetExposed { name: "lamps_x".into(), change: ExposedChange::Max(12) });
    run(&mut s, A::RenameExposed { name: "lamps_x".into(), new: "claw_x".into() });
    let x = exposed(&s);
    assert_eq!((x[0].name.as_str(), x[0].max), ("claw_x", Some(12)));
    // A duplicate name is refused, saying so.
    run(&mut s, A::RenameExposed { name: "lamps_glow".into(), new: "claw_x".into() });
    assert!(status(&s).contains("\"claw_x\" is exposed already"), "{}", status(&s));
    assert_eq!(exposed(&s)[1].name, "lamps_glow");
    // Exposed again: a new name.
    run(&mut s, A::Unexpose("lamps_glow".into()));
    assert!(matches!(s.dialog(), Some(Dialog::Unexpose { .. })));
    run(&mut s, A::Choose(Choice::Confirm));
    assert_eq!(exposed(&s).len(), 1);
    // The default shows in a state that sets nothing.
    run(&mut s, A::SetExposed { name: "claw_x".into(), change: ExposedChange::Default(V::Int(7)) });
    show(&mut s, "Alarm");
    assert_eq!(doc(&s).shown_value(&P::OffsetX("lamps".into())).value, Some(V::Int(7)));
}

#[test]
fn also_changes_names_the_layers_sharing_the_group() {
    let Some((_tmp, s, _)) = editing(&shared_lamp()) else { return };
    let o = doc(&s).offers();
    let colour = o.iter().find(|o| o.property == P::GlowColour("lamps".into())).unwrap();
    assert_eq!(colour.sharing, vec!["buttons".to_string()]);
    let mut h = crate::fits::harness(1400.0, 860.0, 1.0, s);
    h.run();
    h.get_by_label(labels::PROPERTIES).click();
    crate::fits::settle(&mut h);
    // Under each of the four group properties of each of the two layers.
    let also = |name: &str| {
        let text = format!("{}: {name}", labels::ALSO_CHANGES);
        h.root()
            .children_recursive()
            .filter(|n| {
                let a = n.accesskit_node();
                a.label().or(a.value()).as_deref() == Some(text.as_str())
            })
            .count()
    };
    assert_eq!(also("Buttons"), 4, "under Lamps' colour, intensity, reach and animation");
    assert_eq!(also("Lamps"), 4);
}

#[test]
fn destructible_needs_a_state_and_unexposing_first() {
    let Some((_tmp, mut s, _)) = editing(&alarm_lamp()) else { return };
    run(&mut s, A::Destructible(true));
    let health = |s: &EditorState| doc(s).parts_snap().unwrap().health;
    assert_eq!(
        health(&s),
        Some(HealthSource { points: 100, at_zero: "Alarm".into() }),
        "the first state but the start"
    );
    // At 0 names a state of the asset.
    run(&mut s, A::SetHealth(HealthSource { points: 100, at_zero: "Nope".into() }));
    assert!(status(&s).contains("Nope"), "{}", status(&s));
    run(&mut s, A::SetHealth(HealthSource { points: 40, at_zero: "Off".into() }));
    assert_eq!(health(&s), Some(HealthSource { points: 40, at_zero: "Off".into() }));
    // Health's state stays.
    show(&mut s, "Off");
    assert_eq!(doc(&s).remove_refusal(doc(&s).shown_state().unwrap()).as_deref(), Some(HEALTH_ENDS_HERE));
    // Exposed, its default follows the points; Off is refused until unexposed.
    run(&mut s, A::Expose { property: P::Health, default: None });
    assert_eq!(exposed(&s)[0].default, Some(V::Int(40)));
    run(&mut s, A::SetHealth(HealthSource { points: 25, at_zero: "Off".into() }));
    assert_eq!(exposed(&s)[0].default, Some(V::Int(25)));
    run(&mut s, A::Destructible(false));
    assert!(status(&s).contains("unexpose health first"), "{}", status(&s));
    assert!(health(&s).is_some());
    run(&mut s, A::Unexpose("health".into()));
    run(&mut s, A::Choose(Choice::Confirm));
    run(&mut s, A::Destructible(false));
    assert_eq!(health(&s), None);
}

/// A folder with `files` written in it.
fn folder(files: &[(&str, Vec<u8>)]) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    for (name, bytes) in files {
        std::fs::write(d.path().join(name), bytes).unwrap();
    }
    d
}

/// Imports file `name` of `dir` through the browser.
fn import(s: &mut EditorState, dir: &std::path::Path, name: &str) -> Vec<C> {
    run(s, A::Browse(Purpose::ImportSound));
    run(s, A::Listed { dir: dir.to_path_buf(), entries: Ok(crate::common::list_dir(dir)) });
    run(s, A::Pick(name.into()))
}

#[test]
fn sounds_import_play_and_refuse_removal_while_used() {
    let Some((_tmp, mut s, _)) = editing(&alarm_lamp()) else { return };
    let files =
        folder(&[("Alarm Loop.WAV", wav(800)), ("notes.txt", b"hi".to_vec()), ("bad.ogg", b"no".to_vec())]);
    // The browser shows WAV and Ogg files only.
    run(&mut s, A::Browse(Purpose::ImportSound));
    run(
        &mut s,
        A::Listed { dir: files.path().to_path_buf(), entries: Ok(crate::common::list_dir(files.path())) },
    );
    let shown: Vec<String> = s.browser().unwrap().shown().iter().map(|e| e.name.clone()).collect();
    assert_eq!(shown, ["Alarm Loop.WAV", "bad.ogg"]);
    run(&mut s, A::CloseBrowser);
    let cmds = import(&mut s, files.path(), "Alarm Loop.WAV");
    assert!(cmds.iter().any(|c| matches!(c, C::ReadSound(_))));
    let sounds = |s: &EditorState| doc(s).parts_snap().unwrap().sounds;
    assert_eq!(sounds(&s).keys().collect::<Vec<_>>(), ["alarm_loop.wav"]);
    // One step.
    run(&mut s, A::Undo);
    assert!(sounds(&s).is_empty());
    run(&mut s, A::Redo);
    // ▶ sends the bytes.
    let cmds = run(&mut s, A::PlaySound("alarm_loop.wav".into()));
    assert_eq!(cmds, vec![C::PlaySound(wav(800))]);
    // A taken name, a bad sound: refused, the status saying why.
    import(&mut s, files.path(), "Alarm Loop.WAV");
    assert!(status(&s).contains("already"), "{}", status(&s));
    import(&mut s, files.path(), "bad.ogg");
    assert!(status(&s).contains("not a WAV or Ogg"), "{}", status(&s));
    assert_eq!(sounds(&s).len(), 1);
    // Removal refused while a state names it.
    show(&mut s, "Alarm");
    run(&mut s, A::StateSounds { loop_sound: Some(Some("alarm_loop.wav".into())), enter_sound: None });
    run(&mut s, A::RemoveSound("alarm_loop.wav".into()));
    assert!(status(&s).contains("\"Alarm\"'s loop sound"), "{}", status(&s));
    assert_eq!(sounds(&s).len(), 1);
    run(&mut s, A::StateSounds { loop_sound: Some(None), enter_sound: None });
    run(&mut s, A::RemoveSound("alarm_loop.wav".into()));
    assert!(sounds(&s).is_empty());
}

#[test]
fn loop_and_enter_sounds_are_per_state() {
    let Some((_tmp, mut s, _)) = editing(&alarm_lamp()) else { return };
    run(&mut s, A::SoundRead { name: "siren.wav".into(), bytes: wav(100) });
    run(&mut s, A::SoundRead { name: "click.wav".into(), bytes: wav(50) });
    show(&mut s, "Alarm");
    run(&mut s, A::StateSounds { loop_sound: Some(Some("siren.wav".into())), enter_sound: None });
    show(&mut s, "Off");
    run(&mut s, A::StateSounds { loop_sound: None, enter_sound: Some(Some("click.wav".into())) });
    let a = doc(&s).to_asset();
    let st = |n: &str| a.states[a.state(n).unwrap()].clone();
    assert_eq!(
        (st("Alarm").loop_sound.as_deref(), st("Alarm").enter_sound.as_deref()),
        (Some("siren.wav"), None)
    );
    assert_eq!(
        (st("Off").loop_sound.as_deref(), st("Off").enter_sound.as_deref()),
        (None, Some("click.wav"))
    );
    assert_eq!(a.states[a.start].loop_sound, None);
    // A sound the asset lacks is refused.
    run(&mut s, A::StateSounds { loop_sound: Some(Some("nope.wav".into())), enter_sound: None });
    assert!(status(&s).contains("nope.wav"), "{}", status(&s));
}

#[test]
fn edits_that_break_exposure_or_state_values_are_refused() {
    let Some((_tmp, mut s, _)) = editing(&shared_lamp()) else { return };
    // Narrowing an exposed Int past a state's value names the state.
    run(&mut s, A::Expose { property: P::OffsetX("lamps".into()), default: None });
    run(&mut s, A::RenameExposed { name: "lamps_x".into(), new: "claw_x".into() });
    run(&mut s, A::SetExposed { name: "claw_x".into(), change: ExposedChange::Max(50) });
    show(&mut s, "Alarm");
    run(&mut s, A::SetValue { property: P::OffsetX("lamps".into()), value: Some(V::Int(40)) });
    let before = doc(&s).to_asset();
    run(&mut s, A::SetExposed { name: "claw_x".into(), change: ExposedChange::Max(30) });
    assert_eq!(status(&s), "Alarm sets claw_x to 40: change it first");
    assert_eq!(doc(&s).to_asset(), before);
    // A state value outside the exposed limits.
    run(&mut s, A::SetValue { property: P::OffsetX("lamps".into()), value: Some(V::Int(60)) });
    assert!(status(&s).contains("0 to 50"), "{}", status(&s));
    // Removing a sound a state names.
    run(&mut s, A::SoundRead { name: "siren.wav".into(), bytes: wav(100) });
    run(&mut s, A::StateSounds { loop_sound: None, enter_sound: Some(Some("siren.wav".into())) });
    run(&mut s, A::RemoveSound("siren.wav".into()));
    assert!(status(&s).contains("\"Alarm\"'s enter sound"), "{}", status(&s));
    // Removing the state health ends in.
    run(&mut s, A::Destructible(true));
    run(&mut s, A::RemoveState);
    assert_eq!(status(&s), HEALTH_ENDS_HERE);
    // A group's setting exposed through another of its layers.
    run(&mut s, A::Expose { property: P::LightIntensity("lamps".into()), default: None });
    let before = doc(&s).to_asset();
    run(&mut s, A::Expose { property: P::LightIntensity("buttons".into()), default: None });
    assert!(status(&s).contains("\"lamps_light\""), "{}", status(&s));
    assert_eq!(doc(&s).to_asset(), before);
    // Removing an exposed layer names the entry (issue #77's check).
    run(&mut s, A::Select(Selection::Element("lamps".into())));
    run(&mut s, A::Remove);
    assert!(status(&s).contains("lamps is used by exposed claw_x"), "{}", status(&s));
    assert_eq!(doc(&s).to_asset(), before);
}

#[test]
fn an_own_glow_colour_asks_its_default_first() {
    let mut a = lamp();
    a.groups[0].colour = GlowColourSource::Own;
    let Some((_tmp, mut s, _)) = editing(&a) else { return };
    let colour = P::GlowColour("lamps".into());
    assert_eq!(doc(&s).shown_value(&colour).value, None);
    run(&mut s, A::Expose { property: colour.clone(), default: None });
    assert!(matches!(s.dialog(), Some(Dialog::ExposeColour { .. })));
    assert!(exposed(&s).is_empty());
    let red = V::Colour(olr_format::Colour::parse("#ff2020").unwrap());
    run(&mut s, A::Expose { property: colour.clone(), default: Some(red.clone()) });
    assert!(s.dialog().is_none());
    assert_eq!(exposed(&s)[0].default, Some(red.clone()));
    assert_eq!(doc(&s).shown_value(&colour).value, Some(red));
}

/// The asset tab in a harness at 1400 × 860.
fn harness_at(s: EditorState) -> egui_kittest::Harness<'static, crate::fits::App> {
    crate::fits::harness(1400.0, 860.0, 1.0, s)
}

#[test]
fn the_script_commits_on_pause_and_lists_syntax_errors() {
    let Some((_tmp, s, _)) = editing(&alarm_lamp()) else { return };
    let mut h = harness_at(s);
    h.run();
    h.get_by_label(labels::SCRIPT).click();
    crate::fits::settle(&mut h);
    h.get_by_label(labels::SCRIPT_NOT_RUN);
    h.get_by_label(labels::NEW_SCRIPT).click();
    crate::fits::settle(&mut h);
    let script = |h: &egui_kittest::Harness<'static, crate::fits::App>| {
        doc(&h.state().state).parts_snap().unwrap().script.unwrap_or_default()
    };
    assert_eq!(script(&h), olr_editor::parts_ops::NEW_SCRIPT);
    let pushes = h.state().state.can_undo();
    assert!(pushes);
    // Typing is one draft: nothing commits until the pause.
    h.get_by_label(labels::SCRIPT_EDITOR).focus();
    h.step();
    h.get_by_label(labels::SCRIPT_EDITOR).type_text("let x = ;");
    h.step();
    assert_eq!(script(&h), olr_editor::parts_ops::NEW_SCRIPT, "not a step per keystroke");
    // The error is listed as the text changes.
    let issue = olr_script::check(&format!("{}let x = ;", olr_editor::parts_ops::NEW_SCRIPT)).remove(0);
    h.step();
    h.get_by_label(&labels::script_error(&issue));
    // A pause of a second of UI time commits it: one step.
    for _ in 0..80 {
        h.step();
    }
    let typed = format!("{}let x = ;", olr_editor::parts_ops::NEW_SCRIPT);
    assert_eq!(script(&h), typed);
    // Fixed, then the editor loses the keyboard: committed at once.
    h.key_press(egui::Key::Backspace);
    h.get_by_label(labels::SCRIPT_EDITOR).type_text("1;");
    h.step();
    let before = h.state().state.active_document().map(|d| d.parts_snap().unwrap().script);
    h.get_by_label(labels::SCRIPT).click();
    h.step();
    let fixed = format!("{}let x = 1;", olr_editor::parts_ops::NEW_SCRIPT);
    assert_eq!(script(&h), fixed);
    assert_ne!(before.flatten().as_deref(), Some(fixed.as_str()), "committed when the keyboard left");
    crate::fits::settle(&mut h);
    assert!(doc(&h.state().state).script_issues().is_empty());
    assert!(h.query_by_label(&labels::script_error(&issue)).is_none());
    h.get_by_label(labels::NO_SCRIPT_ERRORS);
    // One step per commit: undo goes back through them.
    let mut s =
        std::mem::replace(&mut h.state_mut().state, EditorState::new(Default::default(), Default::default()));
    let script_now = |s: &EditorState| doc(s).parts_snap().unwrap().script;
    run(&mut s, A::Undo);
    assert_eq!(script_now(&s), Some(typed.clone()));
    run(&mut s, A::Undo);
    assert_eq!(script_now(&s).as_deref(), Some(olr_editor::parts_ops::NEW_SCRIPT));
    run(&mut s, A::Undo);
    assert_eq!(script_now(&s), None);
    for _ in 0..3 {
        run(&mut s, A::Redo);
    }
    assert_eq!(script_now(&s), Some(fixed));
    // Undo restores the previous committed text.
    let committed = doc(&s).parts_snap().unwrap().script;
    run(&mut s, A::SetScript("let y = 2;".into()));
    assert!(doc(&s).script_issues().is_empty());
    run(&mut s, A::Undo);
    assert_eq!(doc(&s).parts_snap().unwrap().script, committed);
    run(&mut s, A::SetScript("let y = (;".into()));
    let issues = doc(&s).script_issues();
    assert_eq!((issues.len(), issues[0].line), (1, 1));
    // Remove asks first; undo brings the text back.
    run(&mut s, A::RemoveScript);
    run(&mut s, A::Choose(Choice::Confirm));
    assert_eq!(doc(&s).parts_snap().unwrap().script, None);
    run(&mut s, A::Undo);
    assert_eq!(doc(&s).parts_snap().unwrap().script.as_deref(), Some("let y = (;"));
}

#[test]
fn an_asset_saves_with_a_broken_script() {
    let Some((tmp, mut s, key)) = editing(&alarm_lamp()) else { return };
    run(&mut s, A::SetScript("fn broken( {".into()));
    assert_eq!(doc(&s).script_issues().len(), 1);
    let cmds = run(&mut s, A::Save);
    let saved = cmds
        .iter()
        .find_map(|c| match c {
            C::SaveAsset { asset, .. } => Some(asset.clone()),
            _ => None,
        })
        .expect("saved");
    assert_eq!(saved.script.as_deref(), Some("fn broken( {"));
    assert!(!s.active_dirty());
    let back = olr_edit::asset::load_dir(&tmp.path().join("yours").join(&key.id)).unwrap();
    assert_eq!(back.script.as_deref(), Some("fn broken( {"));
}
