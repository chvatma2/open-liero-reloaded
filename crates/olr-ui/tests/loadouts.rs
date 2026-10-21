//! Loadouts in the menus (issue #5): the editor through its widgets, own
//! loadouts saved per mod, read-only shipped ones, unique names, Default
//! when a chosen loadout is gone.

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use olr_content::{Catalog, MatchLoadout, ModInfo, WeaponChoice};
use olr_ui::screens::{self, MenuData, NO_WEAPON};
use olr_ui::{AppCommand, OwnLoadouts, Profiles, Screen, Settings, UiAction, UiState};

fn shipped(name: &str, weapons: &[&str], loading_time: i32) -> MatchLoadout {
    let weapons = weapons.iter().map(|s| s.to_string()).collect();
    MatchLoadout { name: name.into(), weapons, loading_time, unique: false }
}

fn catalog() -> Catalog {
    let names = |p: &[&str]| p.iter().map(|s| s.to_string()).collect();
    Catalog {
        mods: vec![
            ModInfo {
                id: "olx-classic".into(),
                name: "Liero v1.0".into(),
                presets: names(&["Rifle only"]),
                weapons: names(&["Bazooka", "Rifle", "Napalm"]),
                loadouts: vec![MatchLoadout::default(), shipped("Rifle only", &["Rifle"], 20)],
            },
            ModInfo {
                id: "olx-mw-1-0".into(),
                name: "Modern Warfare1.0".into(),
                weapons: names(&["Mortar", "Uzi"]),
                loadouts: vec![MatchLoadout::default()],
                ..ModInfo::default()
            },
        ],
        levels: vec![olr_content::LevelInfo {
            id: "olx-level-dirt-level".into(),
            name: "Dirt Level".into(),
            width: 504,
            height: 350,
            thumb: None,
        }],
        ..Catalog::default()
    }
}

struct App {
    ui: UiState,
    data: MenuData,
    commands: Vec<AppCommand>,
}

fn harness(ui: UiState) -> Harness<'static, App> {
    let app = App { ui, data: MenuData::new(catalog(), None), commands: Vec::new() };
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

fn local_play() -> Harness<'static, App> {
    let mut h = harness(UiState::new(Settings::default(), Profiles::default(), 7));
    h.get_by_label("Local Play").click();
    h.run();
    h
}

fn own(h: &Harness<'static, App>) -> Vec<MatchLoadout> {
    h.state().ui.loadouts().of("olx-classic").to_vec()
}

fn last_saved<'a>(h: &'a Harness<'static, App>) -> Option<&'a OwnLoadouts> {
    h.state().commands.iter().rev().find_map(|c| match c {
        AppCommand::SaveLoadouts(l) => Some(l),
        _ => None,
    })
}

fn click(h: &mut Harness<'static, App>, label: &str) {
    h.get_by_label(label).click();
    h.run();
}

#[test]
fn the_editor_makes_and_changes_own_loadouts() {
    let mut h = local_play();
    click(&mut h, "Edit loadouts…");
    h.get_by_label("Loadouts of Liero v1.0");
    assert_eq!(h.query_all_by_label("read-only").count(), 2, "Default and Rifle only");
    // Default is read-only: its weapons greyed, no Rename or Delete.
    assert!(h.get_by_label("Rename").accesskit_node().is_disabled());
    assert!(h.get_by_label("Delete").accesskit_node().is_disabled());
    assert!(h.get_by_label("Napalm").accesskit_node().is_disabled());

    // New: a copy of Default, every weapon written out, saved.
    click(&mut h, "New");
    let l = own(&h);
    assert_eq!(l.len(), 1);
    assert_eq!((l[0].name.as_str(), l[0].weapons.len(), l[0].loading_time), ("New loadout", 3, 100));
    assert_eq!(last_saved(&h).map(|o| o.of("olx-classic").len()), Some(1));

    // No weapon on: said, not saved.
    click(&mut h, "All off");
    // (A wrapped label's lines are text runs of their own.)
    h.get_by_label_contains(&NO_WEAPON[..27]);
    assert_eq!(own(&h)[0].weapons.len(), 3);
    click(&mut h, "Rifle");
    assert_eq!(own(&h)[0].weapons, ["Rifle"]);
    assert!(h.query_by_label_contains(&NO_WEAPON[..27]).is_none());
    click(&mut h, "Napalm");
    click(&mut h, "Unique weapons");
    assert_eq!((own(&h)[0].weapons.join(","), own(&h)[0].unique), ("Rifle,Napalm".into(), true));

    // Rename: a shipped name or Default is refused, ignoring case.
    click(&mut h, "Rename");
    for (name, refused) in [("rifle ONLY", true), ("default", true), ("Mine", false)] {
        h.get_by_role_and_label(Role::TextInput, "New name").focus();
        h.run();
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        h.run();
        h.get_by_role_and_label(Role::TextInput, "New name").type_text(name);
        h.run();
        click(&mut h, "OK");
        assert_eq!(h.query_by_label_contains("is a loadout of the mod's").is_some(), refused, "{name}");
    }
    assert_eq!(own(&h)[0].name, "Mine");

    // Copy a shipped one: an own loadout to change.
    click(&mut h, "Rifle only");
    click(&mut h, "Copy");
    let names: Vec<String> = own(&h).into_iter().map(|l| l.name).collect();
    assert_eq!(names, ["Mine", "Rifle only copy"]);
    assert_eq!(own(&h)[1].loading_time, 20);

    // Delete, confirmed.
    click(&mut h, "Delete");
    click(&mut h, "Yes, delete");
    assert_eq!(own(&h).len(), 1);

    // Done selects the loadout shown.
    click(&mut h, "Mine");
    click(&mut h, "Done");
    assert!(h.query_by_label("Loadouts of Liero v1.0").is_none());
    assert_eq!(h.state().ui.screen(), Screen::LocalPlay);
    let p = h.state().ui.settings().gameplay.match_prefs.clone();
    assert_eq!(p.weapons.name, "Mine");
    // It plays: Start carries it in full.
    click(&mut h, "Start");
    let o = h.state().commands.iter().rev().find_map(|c| match c {
        AppCommand::StartMatch(o) => Some(o.clone()),
        _ => None,
    });
    let WeaponChoice::Loadout(l) = o.unwrap().weapons else { panic!("a loadout") };
    assert_eq!((l.name.as_str(), l.weapons.join(","), l.unique), ("Mine", "Rifle,Napalm".into(), true));
}

#[test]
fn a_chosen_loadout_gone_plays_default() {
    let mut settings = Settings::default();
    settings.gameplay.match_prefs.weapons = shipped("Mine", &["Rifle"], 50);
    let mut ui = UiState::new(settings, Profiles::default(), 7);
    let mut mine = OwnLoadouts::default();
    // A weapon a mod update removed: dropped when played, kept in the file.
    mine.of_mut("olx-classic").push(shipped("Mine", &["Rifle", "Laser"], 50));
    assert!(ui.set_loadouts(mine).is_empty());
    let mut h = harness(ui);
    click(&mut h, "Local Play");
    let p = h.state().ui.settings().gameplay.match_prefs.clone();
    assert_eq!((p.weapons.name.as_str(), p.weapons.weapons.join(",")), ("Mine", "Rifle".into()));
    assert_eq!(own(&h)[0].weapons.len(), 2, "the file keeps the unknown weapon");
    let w = h.state_mut().data.take_warnings();
    assert!(w.iter().any(|w| w.contains("Laser")), "{w:?}");
    // The mod without it: Default.
    h.get_by_label("Mod").click();
    h.run();
    click(&mut h, "Modern Warfare1.0");
    assert_eq!(h.state().ui.settings().gameplay.match_prefs.weapons, MatchLoadout::default());
    // Gone from the own loadouts: Default too.
    let mut state = h.state().ui.clone();
    let mut p = state.settings().gameplay.match_prefs.clone();
    p.mod_id = "olx-classic".into();
    p.weapons.name = "Mine".into();
    state.update(UiAction::SetMatch(p));
    state.update(UiAction::SetLoadouts(OwnLoadouts::default()));
    let mut h = harness(state);
    h.run();
    assert_eq!(h.state().ui.settings().gameplay.match_prefs.weapons, MatchLoadout::default());
}

#[test]
fn own_loadouts_are_saved_only_when_changed() {
    let mut ui = UiState::new(Settings::default(), Profiles::default(), 7);
    let mut mine = OwnLoadouts::default();
    mine.of_mut("olx-classic").push(shipped("Mine", &["Rifle"], 50));
    assert_eq!(ui.update(UiAction::SetLoadouts(mine.clone())), [AppCommand::SaveLoadouts(mine.clone())]);
    assert!(ui.update(UiAction::SetLoadouts(mine)).is_empty());
    // Round trip through RON, as the store writes it.
    let text = ron::to_string(ui.loadouts()).unwrap();
    assert_eq!(&ron::from_str::<OwnLoadouts>(&text).unwrap(), ui.loadouts());
}
