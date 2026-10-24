//! Issue #49: every screen, Options tab and dialog fits the window at
//! every supported size: borderless fullscreen at UI scale 3 on 16:10 and
//! 4:3 desktops (384 × 240 and 320 × 240 points), 1280 × 720 at UI scale
//! 3 (427 × 240) and 1, 4K and the user's ultrawide 3440 × 1440. The
//! in-match panels (the weapon selection, the pause menu, the results and
//! the options from the pause) at the scale the app gives them there: the
//! 16:9 frame's height's, not the window's (`olr-game`'s `scale_height`).
//!
//! What fits: every widget lies across the window, and every control can
//! be brought into it whole. egui's scroll areas add no accessibility
//! node and a scrolled-out widget reports its unclipped bounds, so each
//! enabled control is focused (as a keyboard's Tab or a pad would), the
//! scroll areas holding it follow the focus (`pad::follow_focus`), and
//! then its rectangle must be inside the window and not clipped (egui's
//! `interact_rect`, its rectangle within its clip, is all of it). A
//! disabled control cannot take the focus: it is checked across only.

use egui::accesskit::{Action, NodeId, Role};
use egui::{Pos2, Rect, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use olr_content::{Catalog, LevelInfo, MatchLoadout, ModInfo, SkinInfo};
use olr_input::Seat;
use olr_sim::MAX_SLOTS;
use olr_ui::hud::{Results, Row};
use olr_ui::screens::{self, MenuData};
use olr_ui::weapon_select::WeaponMenu;
use olr_ui::{InternetEntry, InternetStatus, InternetView, PingView, Region};
use olr_ui::{Profiles, Screen, Settings, UiAction, UiState};

use crate::layout::App;

/// The windows: width and height in pixels, and the UI scale.
pub const SIZES: [(f32, f32, f32); 6] = [
    // Borderless fullscreen on a 16:10 and a 4:3 desktop at UI scale 3.
    (1920.0, 1200.0, 3.0),
    (1024.0, 768.0, 3.0),
    (1280.0, 720.0, 3.0),
    (1280.0, 720.0, 1.0),
    (3840.0, 2160.0, 1.0),
    (3440.0, 1440.0, 1.0),
];

/// The pixels a point of a window `w` × `h` at UI `scale`: the window's
/// height's in the menus, its 16:9 frame's in a match.
pub fn pixels_per_point(w: f32, h: f32, scale: f32, in_match: bool) -> f32 {
    let window = Rect::from_min_size(Pos2::ZERO, vec2(w, h));
    let height = if in_match { olr_ui::hud::frame(window).height() } else { h };
    olr_ui::theme::pixels_per_point(height, scale)
}

fn catalog() -> Catalog {
    let names = |n: &[&str]| n.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let weapons: Vec<String> = (0..40).map(|i| format!("Weapon number {i} of the mod")).collect();
    let level = |i: usize| LevelInfo {
        id: format!("lvl-{i}"),
        name: format!("A level with a rather long name {i}"),
        width: 640,
        height: 480,
        thumb: None,
    };
    Catalog {
        mods: vec![
            ModInfo {
                id: "olx-classic".into(),
                name: "Liero v1.0 with a long name for its picker".into(),
                presets: names(&["Standard 100lt"]),
                weapons: weapons.clone(),
                loadouts: vec![
                    MatchLoadout::default(),
                    MatchLoadout {
                        name: "A shipped loadout with a long name".into(),
                        weapons: weapons[..3].to_vec(),
                        loading_time: 50,
                        unique: false,
                    },
                ],
            },
            ModInfo { id: "olx-mw".into(), name: "Modern Warfare".into(), ..ModInfo::default() },
        ],
        levels: (0..12).map(level).collect(),
        skins: vec![SkinInfo { id: "olx-skin-ant".into(), name: "ant".into() }],
        ..Catalog::default()
    }
}

/// The menus in a window of `w` × `h` pixels at UI `scale` (`in_match`:
/// the scale a match gives them).
fn harness(w: f32, h: f32, scale: f32, in_match: bool, ui: UiState) -> Harness<'static, App> {
    let ppp = pixels_per_point(w, h, scale, in_match);
    let mut data = MenuData::new(catalog(), None);
    data.update = olr_ui::update::UpdateData {
        enabled: true,
        offer: Some(crate::update::offer(olr_ui::update::UpdateMethod::Portable)),
        ..Default::default()
    };
    let app = App { ui, data, commands: Vec::new() };
    let h = Harness::builder().with_size(vec2(w / ppp, h / ppp)).with_pixels_per_point(ppp).build_ui_state(
        |ui, app: &mut App| {
            for a in screens::show(ui, &app.ui, &mut app.data) {
                let c = app.ui.update(a);
                app.commands.extend(c);
            }
        },
        app,
    );
    olr_ui::theme::apply(&h.ctx);
    // A scroll follows the focus at once (no animation to wait for).
    h.ctx.all_styles_mut(|s| s.scroll_animation = egui::style::ScrollAnimation::none());
    h
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
                | Role::ProgressIndicator
                | Role::SpinButton
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

/// The frames a screen needs to settle (a page's footer finds its place
/// in the second).
fn settle(h: &mut Harness<'static, App>) {
    h.run();
    h.step();
    h.step();
}

/// What of `h`'s screen does not fit its window (the module's comment).
fn misfits(h: &mut Harness<'static, App>) -> Vec<String> {
    settle(h);
    let ppp = h.ctx.pixels_per_point();
    let window = h.ctx.content_rect();
    let px =
        Rect::from_min_max((window.min.to_vec2() * ppp).to_pos2(), (window.max.to_vec2() * ppp).to_pos2());
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
    for w in found.iter().filter(|w| w.focusable) {
        let Some(n) = h.root().children_recursive().find(|n| n.accesskit_node().locate().0 == w.id) else {
            // Gone: an earlier control's focus changed the page (a slider's
            // value turned into a field).
            continue;
        };
        n.focus();
        // The focus lands, then each scroll area holding it follows.
        h.run_steps(12);
        let Some(id) = h.ctx.memory(|m| m.focused()).filter(|id| id.accesskit_id() == w.id) else {
            // Under a modal dialog the page's controls wait for it.
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

/// A case: its name, whether it shows in a match, and how to get there
/// from a fresh harness.
type Setup = fn(&mut Harness<'static, App>);

/// Checks every case at every size, then reports every one that does
/// not fit (each with its first problems).
fn check(cases: &[(&str, bool, Setup)]) {
    let mut failed = Vec::new();
    for (name, in_match, setup) in cases {
        for (w, h, scale) in SIZES {
            let mut harness =
                harness(w, h, scale, *in_match, UiState::new(Settings::default(), profiles(), 1));
            setup(&mut harness);
            settle(&mut harness);
            let ppp = harness.ctx.pixels_per_point();
            let bad = misfits(&mut harness);
            if !bad.is_empty() {
                let at = format!("{name} at {w}x{h}, UI scale {scale} ({ppp} pixels a point)");
                let first = bad.iter().take(6).map(|b| format!("    {b}")).collect::<Vec<_>>().join("\n");
                failed.push(format!("{at}: {} problems, e.g.\n{first}", bad.len()));
            }
        }
    }
    assert!(failed.is_empty(), "{} cases do not fit:\n{}", failed.len(), failed.join("\n"));
}

fn profiles() -> Profiles {
    let mut p = Profiles::default();
    p.add();
    p.selected = 0;
    p
}

/// Opens `screen` from the main menu.
fn open(h: &mut Harness<'static, App>, screen: Screen) {
    let ui = &mut h.state_mut().ui;
    ui.update(UiAction::Open(screen));
    settle(h);
}

/// Clicks the widget `label` (through the accessibility tree: it may be
/// scrolled out at a small size).
fn press(h: &mut Harness<'static, App>, label: &str) {
    h.get_by_label(label).click_accesskit();
    settle(h);
}

fn press_button(h: &mut Harness<'static, App>, label: &str) {
    h.get_by_role_and_label(Role::Button, label).click_accesskit();
    settle(h);
}

fn tab(h: &mut Harness<'static, App>, screen: Screen, tab: &str) {
    open(h, screen);
    press(h, tab);
}

#[test]
fn every_menu_screen_fits() {
    check(&[
        ("Main", false, |_| {}),
        ("Local Play", false, |h| open(h, Screen::LocalPlay)),
        ("Players", false, |h| {
            let ui = &mut h.state_mut().ui;
            ui.update(UiAction::Open(Screen::LocalPlay));
            ui.update(UiAction::OpenJoin(Seat::Keyboard1));
            ui.update(UiAction::JoinSeat(Seat::Pad(0)));
            ui.update(UiAction::JoinSeat(Seat::Keyboard2));
        }),
        ("Profiles", false, |h| open(h, Screen::Profiles)),
        ("Replays", false, |h| open(h, Screen::Replays)),
        ("About", false, |h| open(h, Screen::About)),
        ("Report", false, |h| {
            h.state_mut().data.report.available = true;
            open(h, Screen::Report);
        }),
        ("Update", false, |h| open(h, Screen::Update)),
        ("Editor: Level", false, |h| {
            h.state_mut().data.set_editor_packs(
                ["lvl-1".to_string()].into(),
                vec![("lvl-2".into(), "A recovered level with a long name".into(), false)],
            );
            open(h, Screen::Editor);
        }),
        ("Editor: New level", false, |h| {
            open(h, Screen::Editor);
            press_button(h, "New level");
        }),
        ("Editor: Skin", false, |h| tab(h, Screen::Editor, "Skin")),
        ("Editor: Mod", false, |h| {
            h.state_mut().data.set_editor_mods(
                ["olx-mw".to_string()].into(),
                vec![("m".into(), "A recovered mod".into(), false)],
            );
            tab(h, Screen::Editor, "Mod");
        }),
        ("Editor: Material", false, |h| tab(h, Screen::Editor, "Material")),
    ]);
}

#[test]
fn every_options_tab_fits() {
    check(&[
        ("Options: Video", false, |h| open(h, Screen::Options)),
        ("Options: Audio", false, |h| tab(h, Screen::Options, "Audio")),
        ("Options: Keys, Keyboard 1", false, |h| tab(h, Screen::Options, "Keys")),
        ("Options: Keys, Keyboard 2", false, |h| {
            tab(h, Screen::Options, "Keys");
            press(h, "Keyboard 2");
        }),
        ("Options: Keys, Gamepads", false, |h| {
            tab(h, Screen::Options, "Keys");
            press(h, "Gamepads");
        }),
        ("Options: Online", false, |h| tab(h, Screen::Options, "Online")),
    ]);
}

#[test]
fn every_online_screen_fits() {
    check(&[
        ("Multiplayer: Internet", false, |h| {
            h.state_mut().data.internet = internet();
            open(h, Screen::Multiplayer);
            h.get_by_label_contains("Game 0 with").click_accesskit();
            settle(h);
        }),
        ("Multiplayer: LAN", false, |h| tab(h, Screen::Multiplayer, "LAN")),
        ("Multiplayer: Direct", false, |h| tab(h, Screen::Multiplayer, "Direct")),
        ("Host", false, |h| {
            open(h, Screen::Multiplayer);
            open(h, Screen::Host);
        }),
        ("Lobby (host)", false, |h| lobby(h, lobby_view(true))),
        ("Lobby (guest)", false, |h| lobby(h, lobby_view(false))),
        ("Host lost", false, |h| {
            lobby(h, lobby_view(false));
            let why = "The host left and no other player could take over the game.";
            let app = h.state_mut();
            app.ui.update(UiAction::MatchStarted);
            app.data.host_lost = Some(why.into());
            app.data.host_lost_replay = true;
            app.ui.update(UiAction::HostLost(why.into()));
        }),
    ]);
}

#[test]
fn every_dialog_fits() {
    check(&[
        ("The loadout editor", false, |h| {
            open(h, Screen::LocalPlay);
            press(h, "Edit loadouts…");
            press(h, "New");
        }),
        ("The loadout editor's Rename", false, |h| {
            open(h, Screen::LocalPlay);
            press(h, "Edit loadouts…");
            press(h, "New");
            press(h, "Rename");
        }),
        ("The loadout editor's Delete", false, |h| {
            open(h, Screen::LocalPlay);
            press(h, "Edit loadouts…");
            press(h, "New");
            press(h, "Delete");
        }),
        ("The bots' Advanced…", false, |h| {
            open(h, Screen::LocalPlay);
            press(h, "Advanced…");
            press(h, "Each bot its own difficulty");
        }),
        ("The Keys tab's capture", false, |h| {
            tab(h, Screen::Options, "Keys");
            press(h, "Add key for Jump");
        }),
        ("The Keys tab's conflict", false, |h| {
            tab(h, Screen::Options, "Keys");
            press(h, "Add key for Shoot");
            let capture = h.state_mut().data.capture.as_mut().expect("capturing");
            capture.on_key(olr_input::KeyCode::AltLeft, true);
            settle(h);
            h.get_by_label_contains("is Jump's");
        }),
        ("A profile's Delete", false, |h| {
            open(h, Screen::Profiles);
            press(h, "Delete");
            h.get_by_label("Yes, delete");
        }),
        ("The lobby's download prompt", false, |h| {
            let mut v = lobby_view(false);
            v.prompt = Some(123_456_789);
            lobby(h, v);
            h.get_by_label("Download");
        }),
        ("The lobby's Leave the match?", false, |h| {
            let mut v = lobby_view(false);
            v.phase = "Match running".into();
            lobby(h, v);
            h.get_by_label("Back").click_accesskit();
            settle(h);
            h.get_by_label("Stay");
        }),
        ("The lobby's Ban", false, |h| {
            lobby(h, lobby_view(true));
            press(h, "Ban");
            h.get_by_label("Yes, ban");
        }),
    ]);
}

#[test]
fn every_match_panel_fits_at_the_matchs_scale() {
    check(&[
        ("The weapon selection", true, |h| {
            h.state_mut().data.weapons = Some(weapon_menu());
            in_weapon_select(h, 1);
        }),
        ("The weapon selection of two", true, |h| {
            h.state_mut().data.weapons = Some(weapon_menu());
            in_weapon_select(h, 2);
        }),
        ("The weapon selection of four", true, |h| {
            h.state_mut().data.weapons = Some(weapon_menu());
            in_weapon_select(h, 4);
        }),
        ("The pause menu", true, |h| {
            in_game(h);
            h.state_mut().ui.update(UiAction::Back);
        }),
        ("The online match's menu", true, |h| {
            lobby(h, lobby_view(false));
            let ui = &mut h.state_mut().ui;
            ui.update(UiAction::MatchStarted);
            ui.update(UiAction::Back);
        }),
        ("The results", true, |h| {
            in_game(h);
            h.state_mut().data.results = Some(results(32));
            h.state_mut().ui.update(UiAction::MatchOver);
        }),
        ("The online results", true, |h| {
            lobby(h, lobby_view(false));
            h.state_mut().data.results = Some(results(8));
            let ui = &mut h.state_mut().ui;
            ui.update(UiAction::MatchStarted);
            ui.update(UiAction::MatchOver);
        }),
        ("Options from the pause: Video", true, |h| {
            in_game(h);
            h.state_mut().ui.update(UiAction::Back);
            open(h, Screen::Options);
        }),
        ("Options from the pause: Keys", true, |h| {
            in_game(h);
            h.state_mut().ui.update(UiAction::Back);
            tab(h, Screen::Options, "Keys");
        }),
        ("Options from the pause: Online", true, |h| {
            in_game(h);
            h.state_mut().ui.update(UiAction::Back);
            tab(h, Screen::Options, "Online");
        }),
    ]);
}

/// With two to four views (split screen) the pause menu and the results
/// sit at the window's centre, over the views' corner; with one, at the
/// playfield's (issue #49; the weapon selection already did).
#[test]
fn split_screen_panels_centre_on_the_window() {
    let paused = |h: &mut Harness<'static, App>| {
        in_game(h);
        h.state_mut().ui.update(UiAction::Back);
    };
    let over = |h: &mut Harness<'static, App>| {
        in_game(h);
        h.state_mut().data.results = Some(results(4));
        h.state_mut().ui.update(UiAction::MatchOver);
    };
    let panels: [(&str, &str, Setup); 2] =
        [("pause menu", "The pause menu", paused), ("results", "The results", over)];
    for (w, hgt) in [(1920.0, 1080.0), (3440.0, 1440.0), (1024.0, 768.0)] {
        for (area, name, setup) in panels {
            for views in 1..=4 {
                let mut h = harness(w, hgt, 1.0, true, UiState::new(Settings::default(), profiles(), 1));
                setup(&mut h);
                h.state_mut().data.views = views;
                settle(&mut h);
                let rect = h.ctx.memory(|m| m.area_rect(egui::Id::new(area))).expect(area);
                let window = h.ctx.content_rect();
                let centre =
                    if views >= 2 { window.center() } else { olr_ui::hud::playfield(window).center() };
                assert!(
                    (rect.center() - centre).length() < 1.0,
                    "{name} with {views} views in {w}x{hgt}: centred at {:?}, not {centre:?}",
                    rect.center()
                );
            }
        }
    }
}

fn weapon_menu() -> WeaponMenu {
    WeaponMenu {
        names: (0..40).map(|i| format!("Weapon number {i}")).collect(),
        enabled: vec![true; 40],
        fallback: 0,
        seed: 3,
        unique: false,
    }
}

/// The weapon selection of `n` local players (keyboard 1, then pads).
fn in_weapon_select(h: &mut Harness<'static, App>, n: usize) {
    let mut p = Profiles::default();
    for _ in 1..n {
        p.add();
    }
    p.selected = 0;
    let ui = &mut h.state_mut().ui;
    *ui = UiState::new(Settings::default(), p, 1);
    ui.update(UiAction::Open(Screen::LocalPlay));
    if n > 1 {
        ui.update(UiAction::OpenJoin(Seat::Keyboard1));
        for k in 1..n {
            ui.update(UiAction::JoinSeat(Seat::Pad(k)));
        }
    }
    ui.update(UiAction::StartMatch);
    ui.update(UiAction::MatchStarted);
    assert_eq!((ui.screen(), ui.local_players().max(1)), (Screen::WeaponSelect, n));
    h.state_mut().data.views = n;
}

/// A local match under way, its weapons chosen.
fn in_game(h: &mut Harness<'static, App>) {
    let ui = &mut h.state_mut().ui;
    ui.update(UiAction::Open(Screen::LocalPlay));
    ui.update(UiAction::StartMatch);
    ui.update(UiAction::MatchStarted);
    ui.update(UiAction::ChooseWeapons { weapons: [0; MAX_SLOTS], names: vec![] });
    assert_eq!(ui.screen(), Screen::InGame);
}

pub fn results(rows: usize) -> Results {
    let row = |worm: i32| Row {
        worm,
        name: format!("A player with a long name {worm}"),
        score: 15,
        lives: "∞".into(),
        kills: 15,
        deaths: 3,
        suicides: 1,
        damage: 51234,
        followed: worm == 0,
        left: worm == 3,
    };
    Results {
        winner: Some("A player with a long name 0".into()),
        reason: "kill limit".into(),
        rows: (0..rows as i32).map(row).collect(),
        time_left_ms: None,
    }
}

/// The lobby, `view` shown (the host's own when `view.host`).
fn lobby(h: &mut Harness<'static, App>, view: olr_ui::LobbyView) {
    let app = h.state_mut();
    for a in [
        UiAction::Open(Screen::Multiplayer),
        UiAction::Open(Screen::Host),
        UiAction::Host(olr_ui::HostPrefs::default()),
        UiAction::LobbyJoined,
    ] {
        app.ui.update(a);
    }
    app.data.lobby = Some(view);
    app.data.chat = (0..30).map(|i| olr_ui::hud::LogLine::event(format!("Line {i} of the chat"))).collect();
    settle(h);
}

/// A lobby with everything it can show: the invite, the listing, the
/// reach lines, a download, members of every kind.
fn lobby_view(host: bool) -> olr_ui::LobbyView {
    let mut v = crate::online::lobby_view(host);
    if host {
        v.invite = Some("olr1".to_string() + &"a1b2c3d4".repeat(12));
        v.listing = Some("Listed on the Internet: 3 relays".into());
        v.reach = vec![
            "Your router opened port 23400 (UPnP), but its own address 100.64.12.34 is not public (double NAT \
             or carrier-grade NAT): players connect by hole punching or a TURN relay"
                .into(),
        ];
    } else {
        v.connection = Some("connected directly to 81.2.69.7:23400".into());
    }
    v.members[1].path = Some("relayed".into());
    v.members[1].missing = vec!["A level with a long name".into()];
    v.downloads = vec![olr_ui::DownloadView {
        name: "A level with a long name".into(),
        kind: "level".into(),
        have: 1000,
        size: 4000,
        state: "downloading".into(),
    }];
    v
}

fn internet() -> InternetView {
    let games = Region::ALL
        .iter()
        .enumerate()
        .map(|(i, region)| InternetEntry {
            key: [i as u8 + 1; 32],
            name: format!("Game {i} with a very long name that nobody could ever fit in a column"),
            map: "A level whose name is long too".into(),
            mod_name: "Liero v1.0 extended edition".into(),
            players: 3,
            max_players: 8,
            bots: 24,
            ping: PingView::Ms(123),
            region: *region,
            password: i % 3 == 0,
            addr: Some(format!("81.2.69.{i}:23400").parse().unwrap()),
            incompatible: None,
            playing: true,
        })
        .collect();
    InternetView { status: InternetStatus::Listed, games }
}

/// The playtest of build 714: the first frame after Start, the one the
/// app shows while the match loads (`olr-game`'s `Shell::poll` loads after
/// it), is the page as it settles: saying it is loading grows nothing, so
/// the footer, which finds its height a frame late, never pushes it out of
/// the window.
#[test]
fn the_frame_that_starts_loading_is_settled() {
    let mut failed = Vec::new();
    for (w, hh, scale) in SIZES {
        for players in [false, true] {
            let mut h = harness(w, hh, scale, false, UiState::new(Settings::default(), profiles(), 1));
            let ui = &mut h.state_mut().ui;
            ui.update(UiAction::Open(Screen::LocalPlay));
            if players {
                ui.update(UiAction::OpenJoin(Seat::Keyboard1));
            }
            settle(&mut h);
            h.state_mut().ui.update(UiAction::StartMatch);
            assert!(h.state().ui.is_starting());
            h.step();
            let at = |h: &Harness<'static, App>| {
                widgets(h)
                    .into_iter()
                    .filter(|x| x.name.contains("Loading"))
                    .map(|x| x.rect)
                    .collect::<Vec<_>>()
            };
            let first = at(&h);
            settle(&mut h);
            if first.len() != 1 || first != at(&h) {
                failed
                    .push(format!("{w} × {hh} at {scale}, players {players}: {first:?}, then {:?}", at(&h)));
            }
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}
