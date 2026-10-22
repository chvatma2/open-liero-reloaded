//! The mod editor's panels driven by label (`egui_kittest`, issue #17), on
//! a copy of Classic: gravity and the bazooka's speed changed, the shared
//! shrapnel and its users, Make a copy, a reorder and a rename with the
//! presets following, undo and redo, Reset, Save and opening the saved
//! pack again; an error listed, gone to and refusing the save; the fire's
//! kind, an effect and a weapon copied from MW; Play (issue #18) with an
//! unsaved change on a level and bots chosen in Play ▾, refused with an
//! error, and the editor as it was when the match ends. The test host
//! saves into a temporary directory and loads mods from the content at
//! once; while Play's match runs it draws no editor, as the game's does.

use std::path::PathBuf;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use olr_edit::mods;
use olr_edit::pack;
use olr_mod_editor::fields;
use olr_mod_editor::ui::{self, ModEditorData, labels};
use olr_mod_editor::{Item, ModAction as A, ModCommand as C, ModEditorState, NodeKind};

use crate::common::{CLASSIC, MW, editor, installed};

/// The editor, what it asked the host to do, and where saves go.
pub(crate) struct App {
    pub state: ModEditorState,
    pub data: ModEditorData,
    pub commands: Vec<C>,
    dir: PathBuf,
}

pub(crate) fn harness(state: ModEditorState, dir: PathBuf) -> Harness<'static, App> {
    harness_sized(state, dir, egui::vec2(1500.0, 1000.0))
}

fn harness_sized(state: ModEditorState, dir: PathBuf, size: egui::Vec2) -> Harness<'static, App> {
    let app = App { state, data: ModEditorData::default(), commands: Vec::new(), dir };
    let mut h = Harness::builder().with_size(size).build_ui_state(
        |ui, app: &mut App| {
            // Play's match has the window: the host draws no editor.
            if app.state.playing() {
                ui.label("The match");
                return;
            }
            let actions = ui::show(ui, &app.state, &mut app.data);
            if !actions.is_empty() {
                // Drawn again from the changed state, as the game's host does.
                ui.ctx().request_repaint();
            }
            for a in actions {
                for c in app.state.update(a) {
                    match &c {
                        C::Save(p) => {
                            let answer = match pack::save(&app.dir, p) {
                                Ok(_) => A::Saved,
                                Err(e) => A::Failed(e.to_string()),
                            };
                            app.state.update(answer);
                        }
                        C::LoadMod(id) => {
                            if let Some(p) = installed(id) {
                                app.state.update(A::ModLoaded(id.clone(), Box::new(p)));
                            }
                        }
                        // Issue #19: the browser's folders and files, from the disk.
                        C::List(dir) => {
                            let entries = std::fs::read_dir(dir).map_err(|e| e.to_string()).map(|rd| {
                                rd.filter_map(Result::ok)
                                    .map(|e| olr_mod_editor::FileEntry {
                                        name: e.file_name().to_string_lossy().into_owned(),
                                        is_dir: e.path().is_dir(),
                                    })
                                    .collect()
                            });
                            app.state.update(A::Listed { dir: dir.clone(), entries });
                        }
                        C::ReadFile { import, path } => {
                            let name = path.file_name().unwrap().to_string_lossy().into_owned();
                            let answer = match std::fs::read(path) {
                                Ok(bytes) => A::FileRead { import: import.clone(), name, bytes },
                                Err(e) => A::Failed(e.to_string()),
                            };
                            app.state.update(answer);
                        }
                        C::Play { .. } | C::SavePlaySettings(_) | C::Close | C::PlaySound { .. } => {}
                    }
                    app.commands.push(c);
                }
            }
        },
        app,
    );
    h.run();
    h
}

fn label(key: &str) -> String {
    fields::get(key).unwrap_or_else(|| panic!("no field {key}")).label()
}

pub(crate) fn click(h: &mut Harness<'static, App>, label: &str) {
    h.get_by_label(label).click();
    h.run();
}

/// Types `text` into the field named `name`, and Enter.
pub(crate) fn type_into(h: &mut Harness<'static, App>, name: &str, text: &str) {
    h.get_by_label(name).click();
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.get_by_label(name).type_text(text);
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
}

fn value(h: &Harness<'static, App>, path: &str) -> String {
    h.state().state.value(path).unwrap_or_default()
}

#[test]
fn edit_a_copy_of_classic_and_save_it() {
    let Some(state) = editor() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("my-classic");
    std::fs::create_dir_all(&dir).unwrap();
    let mut h = harness(state, dir.clone());
    assert!(h.query_by_label(labels::UNSAVED).is_none());

    // The world's gravity.
    let gravity = label("Worm.gravity");
    type_into(&mut h, &gravity, "120");
    assert_eq!(value(&h, "mod.worm.gravity"), "120.0");
    h.get_by_label(labels::UNSAVED);
    h.get_by_label(&labels::reset(&gravity));

    // The bazooka's shot speed, through the tree.
    click(&mut h, "Bazooka");
    click(&mut h, &labels::node("Shot"));
    type_into(&mut h, &label("Spawn.speed"), "150");
    assert_eq!(value(&h, "weapons/bazooka.fire.shot.speed"), "150");

    // The shrapnel is shared, with its users.
    let shrapnel = "weapons/bazooka.fire.shot.projectile.spawns.spawn";
    click(&mut h, &labels::node("Pixel (shared: bazooka.spawn)"));
    assert_eq!(h.state().state.node(), format!("{shrapnel}.projectile"));
    let users = h.state().state.used_by()["bazooka.spawn"].clone();
    assert!(!users.is_empty());
    for u in &users {
        h.get_by_label(&labels::used_by(u));
    }
    // Make a copy for this weapon: the bazooka's own now.
    click(&mut h, labels::MAKE_COPY);
    let node =
        h.state().state.nodes().into_iter().find(|n| n.path == format!("{shrapnel}.projectile")).unwrap();
    assert_eq!(node.kind, NodeKind::Projectile { shared: None });
    h.get_by_label(&labels::node("Pixel"));

    // Reorder: the note once; rename with the presets following.
    click(&mut h, "Bazooka");
    click(&mut h, labels::MOVE_DOWN);
    assert_eq!(h.state().state.src().weapons[1].0, "bazooka");
    click(&mut h, labels::OK);
    assert!(h.state().state.dialog().is_none());
    type_into(&mut h, &label("Weapon.name"), "Rocket");
    let src = h.state().state.src();
    assert_eq!(src.weapons[1].1.name, "Rocket");
    assert!(src.restrictions.values().any(|p| p.contains_key("Rocket")));
    assert!(src.restrictions.values().all(|p| !p.contains_key("Bazooka")));
    // Undo and redo.
    click(&mut h, labels::UNDO);
    assert_eq!(h.state().state.src().weapons[1].1.name, "Bazooka");
    click(&mut h, labels::REDO);
    assert_eq!(h.state().state.src().weapons[1].1.name, "Rocket");

    // Reset restores Classic's gravity.
    click(&mut h, labels::WORLD);
    click(&mut h, &labels::reset(&gravity));
    assert_eq!(value(&h, "mod.worm.gravity"), "100.0");
    assert!(h.query_by_label(&labels::reset(&gravity)).is_none());

    // Save writes the pack; opening it again shows the changes.
    click(&mut h, labels::SAVE);
    assert!(matches!(h.state().commands.last(), Some(C::Save(_))));
    assert!(h.query_by_label(labels::UNSAVED).is_none(), "{:?}", h.state().state.dialog());
    let saved = pack::load(&dir).unwrap();
    assert_eq!(ModEditorState::base_of(&saved).as_deref(), Some(CLASSIC));
    let base = mods::source(&installed(CLASSIC).unwrap()).unwrap().clone();
    let s = ModEditorState::new(saved, Some(base), Vec::new(), false).unwrap();
    assert_eq!(s.value("weapons/bazooka.fire.shot.speed").as_deref(), Some("150"));
    assert_eq!(s.src().weapons[1].1.name, "Rocket");
    assert_eq!(s.original("weapons/bazooka.fire.shot.speed").as_deref(), Some("140"));
    assert!(!s.dirty());
}

#[test]
fn an_error_is_listed_goes_to_its_field_and_refuses_the_save() {
    let Some(state) = editor() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let mut h = harness(state, tmp.path().join("my-classic"));
    // The shared shrapnel spawns itself when it hits a worm: a cycle.
    click(&mut h, "bazooka.spawn");
    assert_eq!(h.state().state.item(), &Item::Shared("bazooka.spawn".into()));
    click(&mut h, &labels::event("Hit worm", "Spawns"));
    click(&mut h, &labels::event("Hit worm", "Edit the spawn"));
    assert!(matches!(
        h.state().state.nodes().iter().find(|n| n.path == h.state().state.node()).map(|n| &n.kind),
        Some(NodeKind::Spawn(_))
    ));
    click(&mut h, labels::SPAWNED);
    click(&mut h, "Shared: bazooka.spawn");
    let s = &h.state().state;
    assert!(s.errors() > 0, "{:?}", s.issues());
    assert!(s.show_problems());
    let issue = s.issues().iter().find(|i| i.is_error()).unwrap().clone();
    // Save is refused.
    click(&mut h, labels::SAVE);
    assert!(!h.state().commands.iter().any(|c| matches!(c, C::Save(_))));
    assert!(h.state().state.status().starts_with("Not saved"), "{}", h.state().state.status());
    // The error is listed; clicking it goes there.
    click(&mut h, labels::WORLD);
    click(&mut h, &labels::issue(&issue));
    assert_eq!(h.state().state.item(), &Item::Shared("bazooka.spawn".into()));
    assert_eq!(h.state().state.focus(), issue.path.as_deref());
    // Undone, it saves.
    click(&mut h, labels::UNDO);
    assert_eq!(h.state().state.errors(), 0);
    click(&mut h, labels::SAVE);
    assert!(h.state().commands.iter().any(|c| matches!(c, C::Save(_))));
}

#[test]
fn fire_kind_effects_and_a_weapon_from_another_mod() {
    let Some(state) = editor() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let mut h = harness(state, tmp.path().join("my-classic"));
    click(&mut h, "Bazooka");
    // The fire's kind asks first.
    click(&mut h, labels::FIRE);
    click(&mut h, "Beam");
    click(&mut h, labels::CHANGE);
    assert_eq!(h.state().state.variant("weapons/bazooka.fire").as_deref(), Some("Beam"));
    type_into(&mut h, &label("Fire::Beam.length"), "300");
    assert_eq!(value(&h, "weapons/bazooka.fire.length"), "300");
    click(&mut h, labels::UNDO);
    click(&mut h, labels::UNDO);
    assert_eq!(h.state().state.variant("weapons/bazooka.fire").as_deref(), Some("Projectile"));

    // An event's effect and its fields.
    click(&mut h, &labels::node("missile.png"));
    click(&mut h, &labels::event("Hit worm", "Effect"));
    click(&mut h, "Bounce");
    let p = "weapons/bazooka.fire.shot.projectile";
    assert!(
        value(&h, &format!("{p}.hit_worm[0]")).starts_with("Bounce"),
        "{}",
        value(&h, &format!("{p}.hit_worm"))
    );
    type_into(&mut h, &labels::event("Hit worm", &label("Effect::Bounce.factor")), "0.5");
    assert_eq!(value(&h, &format!("{p}.hit_worm[0].factor")), "0.5");
    // Under More ▸: the rare fields.
    assert!(h.query_by_label(&label("Projectile.size")).is_none());
    click(&mut h, labels::MORE);
    h.get_by_label(&label("Projectile.size"));

    // A weapon copied from MW.
    let Some(mw) = installed(MW) else { return };
    let (_, first) = mods::source(&mw).unwrap().weapons[0].clone();
    let n = h.state().state.src().weapons.len();
    click(&mut h, labels::ADD_WEAPON);
    click(&mut h, labels::COPY_FROM);
    click(&mut h, "MW 1.0");
    click(&mut h, labels::WEAPON_TO_COPY);
    click(&mut h, &labels::copy(&first.name));
    click(&mut h, labels::ADD);
    let src = h.state().state.src();
    assert_eq!(src.weapons.len(), n + 1);
    assert_eq!(src.weapons[n].1.fire, first.fire);
    assert_eq!(h.state().state.errors(), 0, "{:?}", h.state().state.issues());
}

/// Issue #18: Play takes the mod as it is, unsaved changes included, on
/// the level and bots of Play ▾ (remembered); a mod with errors does not
/// play and lists them; when the match ends the editor is where it was:
/// the item, the node, the scroll, the undo history, the unsaved mark.
#[test]
fn play_the_unsaved_mod_and_come_back() {
    let Some(mut state) = editor() else { return };
    let levels = vec![
        ("olx-level-dirt-level".to_string(), "Dirt Level".to_string()),
        ("olx-level-liero-factory".to_string(), "Liero Factory".to_string()),
    ];
    state.set_play(levels, olr_mod_editor::PlaySettings::default());
    assert_eq!(state.play_level().map(|(id, _)| id.as_str()), Some("olx-level-dirt-level"), "the first");
    let tmp = tempfile::tempdir().unwrap();
    let mut h = harness_sized(state, tmp.path().join("my-classic"), egui::vec2(1500.0, 420.0));
    click(&mut h, "Bazooka");
    click(&mut h, &labels::node("Shot"));
    type_into(&mut h, &label("Spawn.speed"), "150");

    // Play ▾: the level and the bots, each remembered.
    click(&mut h, labels::PLAY_OPTIONS);
    click(&mut h, &labels::play_level("Liero Factory"));
    click(&mut h, &labels::play_bots(3));
    let saved: Vec<_> = h
        .state()
        .commands
        .iter()
        .filter_map(|c| if let C::SavePlaySettings(s) = c { Some(s) } else { None })
        .collect();
    assert_eq!(saved.last().map(|s| (s.level.as_str(), s.bots)), Some(("olx-level-liero-factory", 3)));
    h.key_press(egui::Key::Escape);
    h.run();

    // Scrolled down the shot's panel, its rare fields shown.
    click(&mut h, labels::MORE);
    let speed = label("Spawn.speed");
    let top = h.get_by_label(&speed).rect();
    h.get_by_label(&speed).scroll_down();
    h.run();
    let scrolled = h.get_by_label(&speed).rect();
    assert!(scrolled.top() < top.top(), "{scrolled:?} above {top:?}");

    click(&mut h, labels::PLAY);
    let Some(C::Play { pack, level, bots }) = h.state().commands.last().cloned() else {
        panic!("Play reaches the host: {:?}", h.state().commands.last())
    };
    assert_eq!(mods::get(mods::source(&pack).unwrap(), "weapons/bazooka.fire.shot.speed").unwrap(), "150");
    assert_eq!((level.as_str(), bots), ("olx-level-liero-factory", 3));
    assert!(h.state().state.playing());
    assert!(h.query_by_label(labels::PLAY).is_none(), "the match has the window");
    for _ in 0..5 {
        h.run();
    }
    h.state_mut().state.update(A::PlayEnded);
    h.run();
    let s = &h.state().state;
    assert!(!s.playing());
    assert_eq!(s.item(), &Item::Weapon("bazooka".into()));
    assert_eq!(s.node(), "weapons/bazooka.fire.shot");
    assert!(s.dirty() && s.can_undo(), "the unsaved work and its history stay");
    assert_eq!(h.get_by_label(&speed).rect(), scrolled, "the scroll stays");
    h.get_by_label(labels::UNSAVED);

    // An error: Play lists it and does not start.
    let n = h.state().commands.len();
    let cycle = r#"{"s": (shared: "bazooka.spawn")}"#;
    h.state_mut()
        .state
        .update(A::Set { path: "projectiles/bazooka.spawn.spawns".into(), value: cycle.into() });
    h.state_mut().state.update(A::ShowProblems(false));
    h.run();
    click(&mut h, labels::PLAY);
    let s = &h.state().state;
    assert!(!s.playing());
    assert!(!h.state().commands[n..].iter().any(|c| matches!(c, C::Play { .. })));
    assert!(s.show_problems(), "the problems show");
    assert!(s.status().starts_with("Not played"), "{}", s.status());
    let issue = s.issues().iter().find(|i| i.is_error()).unwrap().clone();
    h.get_by_label(&labels::issue(&issue));
}
