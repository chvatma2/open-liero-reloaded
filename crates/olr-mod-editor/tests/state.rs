//! The mod editor's state on a copy of Classic (issue #17): fields set,
//! undone and reset, the projectile tree, shared projectiles, weapons
//! added, moved and renamed, presets, events, errors refusing the save,
//! and Close asking first.

use olr_data::restrictions::{ShippedLoadout, WeaponState};
use olr_mod_editor::state::{ORDER_NOTE, effect_names};
use olr_mod_editor::{Choice, Dialog, Event, Item, ModAction as A, ModCommand as C, NodeKind};

use crate::common::{MW, editor, installed};

const BAZOOKA: &str = "weapons/bazooka";
const SHRAPNEL: &str = "weapons/bazooka.fire.shot.projectile.spawns.spawn";

fn set(path: &str, value: &str) -> A {
    A::Set { path: path.into(), value: value.into() }
}

#[test]
fn a_copy_of_classic_opens_clean() {
    let Some(s) = editor() else { return };
    assert_eq!(s.base_id(), Some("olx-classic"));
    assert_eq!(s.errors(), 0, "{:?}", s.issues());
    assert!(!s.dirty());
    assert_eq!(s.item(), &Item::World);
    assert_eq!(s.value("mod.name").as_deref(), Some("\"My Classic\""));
}

#[test]
fn a_field_set_is_one_step_undone_and_redone() {
    let Some(mut s) = editor() else { return };
    s.update(set("mod.worm.gravity", "120.0"));
    s.update(set("weapons/bazooka.fire.shot.speed", "150"));
    assert!(s.dirty());
    assert_eq!(s.value("mod.worm.gravity").as_deref(), Some("120.0"));
    assert_eq!(s.original("mod.worm.gravity").as_deref(), Some("100.0"));
    assert_eq!(s.original("weapons/bazooka.fire.shot.speed").as_deref(), Some("140"));
    assert_eq!(s.original("mod.worm.jump_speed"), None, "unchanged: no original shown");

    s.update(A::Undo);
    assert_eq!(s.value("weapons/bazooka.fire.shot.speed").as_deref(), Some("140"));
    assert_eq!(s.value("mod.worm.gravity").as_deref(), Some("120.0"));
    s.update(A::Undo);
    assert_eq!(s.value("mod.worm.gravity").as_deref(), Some("100.0"));
    assert!(!s.dirty() && !s.can_undo());
    s.update(A::Redo);
    assert_eq!(s.value("mod.worm.gravity").as_deref(), Some("120.0"));
    assert!(s.dirty());

    // The same value again is no step.
    let g = s.generation();
    s.update(set("mod.worm.gravity", "120.0"));
    assert_eq!(s.generation(), g);
    // A value the format refuses is no step either, and says why.
    s.update(set("mod.worm.gravity", "\"heavy\""));
    assert_eq!(s.generation(), g);
    assert!(s.status().starts_with("Not changed"), "{}", s.status());
}

#[test]
fn reset_restores_the_base_mods_value() {
    let Some(mut s) = editor() else { return };
    s.update(set("weapons/bazooka.fire.shot.speed", "150"));
    s.update(A::Reset("weapons/bazooka.fire.shot.speed".into()));
    assert_eq!(s.value("weapons/bazooka.fire.shot.speed").as_deref(), Some("140"));
    assert_eq!(s.original("weapons/bazooka.fire.shot.speed"), None);
    s.update(A::Undo);
    assert_eq!(s.value("weapons/bazooka.fire.shot.speed").as_deref(), Some("150"));
}

#[test]
fn the_tree_shows_the_shared_shrapnel_and_a_copy_makes_it_the_weapons() {
    let Some(mut s) = editor() else { return };
    s.update(A::Select(Item::Weapon("bazooka".into())));
    let nodes = s.nodes();
    let kinds: Vec<_> = nodes.iter().map(|n| (n.path.as_str(), &n.kind)).collect();
    assert_eq!(kinds[0], (BAZOOKA, &NodeKind::Weapon));
    let shrapnel = format!("{SHRAPNEL}.projectile");
    let node = nodes.iter().find(|n| n.path == shrapnel).expect("the shrapnel's node");
    assert_eq!(node.kind, NodeKind::Projectile { shared: Some("bazooka.spawn".into()) });
    assert!(node.label.contains("shared: bazooka.spawn"), "{}", node.label);
    let users = s.used_by();
    assert!(users["bazooka.spawn"].iter().any(|u| u.starts_with(BAZOOKA)), "{users:?}");

    s.update(A::Node(shrapnel.clone()));
    assert_eq!(s.node(), shrapnel);
    s.update(A::MakeCopy(SHRAPNEL.into()));
    let node = s.nodes().into_iter().find(|n| n.path == shrapnel).unwrap();
    assert_eq!(node.kind, NodeKind::Projectile { shared: None });
    assert_eq!(s.node(), shrapnel, "the node shown stays");
    // Its own copy: a change to it leaves the shared one alone.
    s.update(set(&format!("{shrapnel}.gravity"), "10"));
    assert_eq!(s.value("projectiles/bazooka.spawn.gravity").as_deref(), Some("70"));
    s.update(A::Undo);
    s.update(A::Undo);
    let node = s.nodes().into_iter().find(|n| n.path == shrapnel).unwrap();
    assert_eq!(node.kind, NodeKind::Projectile { shared: Some("bazooka.spawn".into()) });
}

#[test]
fn weapons_move_with_a_note_once_and_rename_with_their_presets() {
    let Some(mut s) = editor() else { return };
    s.update(A::MoveWeapon { id: "bazooka".into(), by: 1 });
    assert_eq!(s.src().weapons[1].0, "bazooka");
    assert_eq!(s.dialog(), Some(&Dialog::Note(ORDER_NOTE.into())));
    s.update(A::Confirm(true));
    assert_eq!(s.dialog(), None);
    s.update(A::MoveWeapon { id: "bazooka".into(), by: -1 });
    assert_eq!(s.src().weapons[0].0, "bazooka");
    assert_eq!(s.dialog(), None, "the note is said once");

    let before = s.src().restrictions.values().filter(|p| p.contains_key("Bazooka")).count();
    assert!(before > 0);
    s.update(set("weapons/bazooka.name", "\"Rocket\""));
    assert_eq!(s.src().weapons[0].1.name, "Rocket");
    assert_eq!(s.src().restrictions.values().filter(|p| p.contains_key("Rocket")).count(), before);
    assert!(s.src().restrictions.values().all(|p| !p.contains_key("Bazooka")));
    s.update(A::Undo);
    assert_eq!(s.src().weapons[0].1.name, "Bazooka");
}

#[test]
fn weapons_are_added_duplicated_and_removed() {
    let Some(mut s) = editor() else { return };
    let n = s.src().weapons.len();
    s.update(A::AddWeapon);
    s.update(A::AddConfirm);
    assert_eq!(s.src().weapons.len(), n + 1);
    assert_eq!(s.item(), &Item::Weapon("new-weapon".into()));
    assert_eq!(s.errors(), 0, "{:?}", s.issues());

    // A copy from MW: the host loads it first.
    s.update(A::AddWeapon);
    let cmds = s.update(A::AddFrom(Some(MW.into())));
    assert_eq!(cmds, vec![C::LoadMod(MW.into())]);
    let Some(mw) = installed(MW) else { return };
    let first = olr_edit::mods::source(&mw).unwrap().weapons[0].clone();
    s.update(A::ModLoaded(MW.into(), Box::new(mw)));
    s.update(A::AddPick(first.0.clone()));
    s.update(A::AddConfirm);
    assert_eq!(s.dialog(), None, "{}", s.status());
    assert_eq!(s.src().weapons.len(), n + 2);
    let Item::Weapon(id) = s.item().clone() else { panic!("the copy is shown") };
    let w = &s.src().weapons.iter().find(|(i, _)| *i == id).unwrap().1;
    assert_eq!(w.fire, first.1.fire, "a copy of MW's weapon");
    assert_eq!(s.errors(), 0, "{:?}", s.issues());
    s.update(A::Undo);
    assert_eq!(s.src().weapons.len(), n + 1);
    assert_eq!(s.item(), &Item::World, "the weapon shown is gone");

    s.update(A::DuplicateWeapon("bazooka".into()));
    assert_eq!(s.src().weapons[1].0, "bazooka-2");
    assert_eq!(s.item(), &Item::Weapon("bazooka-2".into()));
    s.update(A::RemoveWeapon("bazooka-2".into()));
    assert_eq!(s.src().weapons.len(), n + 1);
    assert_eq!(s.item(), &Item::Weapon("big-nuke".into()), "the next weapon is shown");
}

#[test]
fn changing_the_fire_kind_asks_first() {
    let Some(mut s) = editor() else { return };
    s.update(A::FireKind { weapon: "bazooka".into(), kind: "Beam".into() });
    assert!(matches!(s.dialog(), Some(Dialog::FireKind { .. })));
    s.update(A::Confirm(false));
    assert_eq!(s.variant("weapons/bazooka.fire").as_deref(), Some("Projectile"));
    s.update(A::FireKind { weapon: "bazooka".into(), kind: "Beam".into() });
    s.update(A::Confirm(true));
    assert_eq!(s.variant("weapons/bazooka.fire").as_deref(), Some("Beam"));
    assert_eq!(s.errors(), 0, "{:?}", s.issues());
    s.update(A::Undo);
    assert_eq!(s.variant("weapons/bazooka.fire").as_deref(), Some("Projectile"));
}

#[test]
fn an_event_is_one_effect_and_a_spawn() {
    let Some(mut s) = editor() else { return };
    let p = "weapons/bazooka.fire.shot.projectile";
    let v = s.event(p, Event::HitWorm).unwrap();
    assert!(matches!(v.effect, Some((0, olr_format::mods::Effect::Explode { .. }))), "{v:?}");
    assert_eq!(v.spawn.as_deref(), Some("spawn"));
    assert!(effect_names().contains(&"Bounce"), "{:?}", effect_names());

    s.update(A::Effect { projectile: p.into(), event: Event::HitWorm, effect: Some("Bounce".into()) });
    let v = s.event(p, Event::HitWorm).unwrap();
    assert!(matches!(v.effect, Some((0, olr_format::mods::Effect::Bounce { .. }))), "{v:?}");
    s.update(A::EventSpawn { projectile: p.into(), event: Event::HitWorm, on: false });
    assert_eq!(s.event(p, Event::HitWorm).unwrap().spawn, None);
    // The terrain still spawns: the spawn stays.
    assert!(s.value(&format!("{p}.spawns[\"spawn\"]")).is_some());

    s.update(A::Timer { projectile: p.into(), on: true });
    s.update(A::Select(Item::Weapon("bazooka".into())));
    assert!(s.nodes().iter().any(|n| n.kind == NodeKind::Timer));
    s.update(A::EventSpawn { projectile: p.into(), event: Event::Timer, on: true });
    assert_eq!(s.event(p, Event::Timer).unwrap().spawn.as_deref(), Some("spawn"));
    assert_eq!(s.errors(), 0, "{:?}", s.issues());
    s.update(A::Timer { projectile: p.into(), on: false });
    assert!(s.event(p, Event::Timer).is_none());
}

#[test]
fn presets_and_loadouts_are_edited() {
    let Some(mut s) = editor() else { return };
    let first = s.preset().unwrap().to_string();
    s.update(A::PresetState { preset: first.clone(), weapon: "bazooka".into(), state: WeaponState::Banned });
    assert_eq!(s.src().restrictions[&first]["Bazooka"], WeaponState::Banned, "names compare as OLX's");
    s.update(A::AddPreset("No nukes".into()));
    assert_eq!(s.preset(), Some("No nukes"));
    s.update(A::Loadout {
        preset: "No nukes".into(),
        loadout: Some(ShippedLoadout { loading_time: 100, unique: false }),
    });
    assert!(s.src().loadouts.contains_key("No nukes"));
    let steps = s.generation();
    s.update(A::RemovePreset("No nukes".into()));
    assert!(!s.src().restrictions.contains_key("No nukes"));
    assert!(!s.src().loadouts.contains_key("No nukes"), "its loadout goes with it");
    assert_eq!(s.generation(), steps + 1, "one step");
    s.update(A::Undo);
    assert!(s.src().loadouts.contains_key("No nukes"));
}

#[test]
fn an_error_is_listed_goes_to_its_field_and_refuses_the_save() {
    let Some(mut s) = editor() else { return };
    s.update(set("projectiles/bazooka.spawn.spawns", "{\"s\": (shared: \"bazooka.spawn\")}"));
    assert!(s.errors() > 0);
    assert!(s.show_problems());
    assert!(s.update(A::Save).is_empty(), "not saved");
    assert!(s.status().starts_with("Not saved"), "{}", s.status());
    let issue = s.issues().iter().find(|i| i.is_error()).unwrap().clone();
    let path = issue.path.clone().expect("a located error");
    s.update(A::GoTo(path.clone()));
    assert_eq!(s.item(), &Item::Shared("bazooka.spawn".into()));
    assert_eq!(s.focus(), Some(path.as_str()));

    s.update(A::Undo);
    assert_eq!(s.errors(), 0);
    let cmds = s.update(A::Save);
    assert!(matches!(cmds.as_slice(), [C::Save(_)]), "{cmds:?}");
    s.update(A::Saved);
    assert!(!s.dirty());
}

#[test]
fn close_asks_when_there_are_unsaved_changes() {
    let Some(mut s) = editor() else { return };
    assert_eq!(s.update(A::Close), vec![C::Close]);
    s.update(set("mod.worm.gravity", "120.0"));
    assert!(s.update(A::Close).is_empty());
    assert_eq!(s.dialog(), Some(&Dialog::Unsaved));
    s.update(A::Choice(Choice::Cancel));
    assert_eq!(s.dialog(), None);
    s.update(A::Close);
    let cmds = s.update(A::Choice(Choice::Save));
    assert!(matches!(cmds.as_slice(), [C::Save(_)]));
    assert_eq!(s.update(A::Saved), vec![C::Close]);

    // A recovered mod is unsaved work.
    let Some(pack) = crate::common::classic_copy() else { return };
    let s = olr_mod_editor::ModEditorState::new(pack, None, Vec::new(), true).unwrap();
    assert!(s.dirty());
}

#[test]
fn fresh_kinds_check_clean() {
    let Some(mut s) = editor() else { return };
    // Beam back to Projectile: a fresh shot.
    s.update(A::FireKind { weapon: "laser".into(), kind: "Projectile".into() });
    s.update(A::Confirm(true));
    assert_eq!(s.variant("weapons/laser.fire").as_deref(), Some("Projectile"), "{}", s.status());
    // A projectile trail, and an image.
    let p = "weapons/bazooka.fire.shot.projectile";
    let trail =
        format!("Projectile(every_s: 0.1, spawn: (projectile: (gfx: {})))", olr_mod_editor::state::PIXEL);
    s.update(set(&format!("{p}.trail"), &trail));
    assert_eq!(s.variant(&format!("{p}.trail")).as_deref(), Some("Projectile"), "{}", s.status());
    s.update(set(&format!("{p}.gfx"), olr_mod_editor::state::PIXEL));
    assert_eq!(s.variant(&format!("{p}.gfx")).as_deref(), Some("Pixels"), "{}", s.status());
    s.update(set(&format!("{p}.gfx"), "Image(file: \"gfx/missile.png\")"));
    s.update(A::Animation { gfx: format!("{p}.gfx"), on: true });
    assert!(s.value(&format!("{p}.gfx.animation.fps")).is_some(), "{}", s.status());
    assert_eq!(s.errors(), 0, "{:?}", s.issues());
}
