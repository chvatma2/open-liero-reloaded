//! The menus' page frame: every sub-screen (local play, the options and
//! each of their tabs, the Multiplayer screen's tabs, profiles, replays,
//! import, about) draws one column centred in the window, whatever its
//! size, with its title centred above
//! and Back at the column's left edge; nothing hugs a corner of a big
//! screen.

use egui::accesskit::Role;
use egui::{Rect, pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use rl_content::{Catalog, LevelInfo, ModInfo, SkinInfo};
use rl_ui::screens::{self, MenuData};
use rl_ui::{AppCommand, Profiles, Screen, Settings, UiAction, UiState};

fn catalog() -> Catalog {
    let level = |i: usize| LevelInfo {
        id: format!("lvl-{i}"),
        name: format!("Level {i}"),
        width: 640,
        height: 480,
        thumb: None,
    };
    Catalog {
        mods: vec![ModInfo {
            id: "olx-classic".into(),
            name: "Liero v1.0".into(),
            presets: vec!["Standard 100lt".into()],
        }],
        levels: (0..7).map(level).collect(),
        skins: vec![SkinInfo { id: "olx-skin-ant".into(), name: "ant".into() }],
        ..Catalog::default()
    }
}

struct App {
    ui: UiState,
    data: MenuData,
    commands: Vec<AppCommand>,
}

/// The menus in a window of `size` points at `ppp` pixels a point.
fn harness(size: egui::Vec2, ppp: f32) -> Harness<'static, App> {
    let ui = UiState::new(Settings::default(), Profiles::default(), 1);
    let app = App { ui, data: MenuData::new(catalog(), None), commands: Vec::new() };
    Harness::builder().with_size(size).with_pixels_per_point(ppp).build_ui_state(
        |ui, app: &mut App| {
            for a in screens::show(ui, &app.ui, &mut app.data) {
                let c = app.ui.update(a);
                app.commands.extend(c);
            }
        },
        app,
    )
}

/// The widgets' bounding rectangles (containers left out), as the
/// accessibility tree has them.
fn widgets(h: &Harness<'static, App>) -> Vec<(String, Rect)> {
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
        );
        let (true, Some(b)) = (widget, a.bounding_box()) else { continue };
        let name = a.label().or(a.value()).unwrap_or_default().to_string();
        let r = Rect::from_min_max(pos2(b.x0 as f32, b.y0 as f32), pos2(b.x1 as f32, b.y1 as f32));
        out.push((name, r));
    }
    out
}

/// The screens to check: each opened from the main menu, then the
/// options' and the Multiplayer screen's tabs clicked.
const SCREENS: [(Screen, Option<&str>); 15] = [
    (Screen::LocalPlay, None),
    (Screen::Options, None),
    (Screen::Options, Some("Audio")),
    (Screen::Options, Some("Keys")),
    (Screen::Options, Some("Online")),
    (Screen::Profiles, None),
    (Screen::Replays, None),
    (Screen::Import, None),
    (Screen::About, None),
    (Screen::Options, Some("Video")),
    (Screen::Multiplayer, None),
    (Screen::Multiplayer, Some("LAN")),
    (Screen::Multiplayer, Some("Direct")),
    (Screen::Host, None),
    (Screen::Lobby, None),
];

#[test]
fn every_sub_screen_is_a_centred_column() {
    // 1280 × 720 and the user's 3440 × 1440, at one and two pixels a point.
    for (w, h, ppp) in [(1280.0, 720.0, 1.0), (3440.0, 1440.0, 1.0), (3440.0, 1440.0, 2.0)] {
        let mut harness = harness(egui::vec2(w / ppp, h / ppp), ppp);
        // Physical pixels, as the accessibility tree's bounds are.
        let window = Rect::from_min_max(pos2(0.0, 0.0), pos2(w, h));
        for (screen, tab) in SCREENS {
            reach(&mut harness, screen);
            if let Some(tab) = tab {
                harness.get_by_label(tab).click();
                harness.run();
            }
            let at = format!("{screen:?} {tab:?} at {w}x{h} ({ppp} pixels a point)");
            assert_eq!(harness.state().ui.screen(), screen, "{at}");
            let found = widgets(&harness);
            assert!(found.len() > 3, "{at}: {found:?}");
            let all = found.iter().fold(Rect::NOTHING, |u, (_, r)| u.union(*r));
            for (name, r) in &found {
                assert!(
                    r.left() >= window.left() - 0.5 && r.right() <= window.right() + 0.5,
                    "{name:?} at {r:?} leaves the window: {at}"
                );
                assert!(r.top() >= window.top() - 0.5, "{name:?} at {r:?} leaves the window: {at}");
            }
            let off = all.center().x - window.center().x;
            assert!(off.abs() <= 4.0 * ppp, "{at}: the content's centre is {off} px off, {all:?}");
            // The title is centred too, and Back sits at the column's left.
            let title = found.iter().find(|(n, _)| title_of(screen) == n).map(|(_, r)| *r);
            let title = title.unwrap_or_else(|| panic!("{at}: no title in {found:?}"));
            let off = title.center().x - window.center().x;
            assert!(off.abs() <= 4.0 * ppp, "{at}: the title is {off} px off centre");
            let back = harness.get_by_label("Back").accesskit_node().bounding_box().unwrap();
            assert!((back.x0 as f32 - all.left()).abs() <= 1.0, "{at}: Back is not at the column's left");
            // On a wide screen the column keeps to the middle.
            if w / ppp > 2000.0 {
                assert!(all.width() < 0.5 * w, "{at}: the content spans {} of {w}", all.width());
            }
        }
    }
}

/// Opens `screen` from wherever the menus are: back to Main, then the
/// way the player goes (the lobby by hosting and being let in).
fn reach(harness: &mut Harness<'static, App>, screen: Screen) {
    let app = harness.state_mut();
    while app.ui.screen() != Screen::Main {
        app.ui.update(UiAction::Back);
    }
    let path: &[Screen] = match screen {
        Screen::Host | Screen::Lobby => &[Screen::Multiplayer, Screen::Host],
        s => &[s],
    };
    for s in path {
        app.ui.update(UiAction::Open(*s));
    }
    if screen == Screen::Lobby {
        app.ui.update(UiAction::Host(rl_ui::HostPrefs::default()));
        app.ui.update(UiAction::LobbyJoined);
        app.data.lobby = Some(crate::online::lobby_view(true));
    }
    harness.run();
    harness.step();
    harness.step();
}

fn title_of(screen: Screen) -> &'static str {
    match screen {
        Screen::LocalPlay => "Local Play",
        Screen::Options => "Options",
        Screen::Profiles => "Profiles",
        Screen::Replays => "Replays",
        Screen::Import => "Import OLX content",
        Screen::About => "About",
        Screen::Multiplayer => "Multiplayer",
        Screen::Host => "Host a game",
        Screen::Lobby => "Lobby",
        _ => "",
    }
}

/// Every page's primary action is one large button centred under the
/// column, below everything else.
#[test]
fn the_primary_action_is_centred_below() {
    for (w, h) in [(1280.0, 720.0), (3440.0, 1440.0)] {
        let mut harness = harness(egui::vec2(w, h), 1.0);
        for (screen, label) in [
            (Screen::LocalPlay, "Start"),
            (Screen::Import, "Import"),
            (Screen::Multiplayer, "Host a game"),
            (Screen::Host, "Start hosting"),
        ] {
            reach(&mut harness, screen);
            let b =
                harness.get_by_role_and_label(Role::Button, label).accesskit_node().bounding_box().unwrap();
            let at = format!("{screen:?} at {w}x{h}");
            let cx = (b.x0 + b.x1) as f32 / 2.0;
            assert!((cx - w / 2.0).abs() <= 4.0, "{at}: {label} is centred at {cx}");
            assert!(b.y1 as f32 > h * 0.6, "{at}: {label} is low, not at {}", b.y1);
            // Below everything else on the page.
            let others = widgets(&harness).into_iter().filter(|(n, _)| n != label);
            let lowest = others.map(|(_, r)| r.bottom()).fold(0.0, f32::max);
            assert!(b.y0 as f32 >= lowest - 0.5, "{at}: {label} at {} is above a widget at {lowest}", b.y0);
            assert!(b.height() >= 36.0 && b.width() >= 160.0, "{at}: {label} is large: {b:?}");
        }
    }
}

/// Plan 12b: the lobby's reachability lines are long; they wrap inside the
/// window at the smallest window size, also at a large UI scale (two and
/// three pixels a point: a 640- and a 427-point-wide window), and on the
/// user's ultrawide screen.
#[test]
fn the_lobbys_long_lines_wrap_in_a_narrow_window() {
    let long = [
        "Your router opened port 23400 (UPnP), but its own address 100.64.12.34 is not public (double NAT or \
         carrier-grade NAT): players connect by hole punching or a TURN relay",
        "Your router changes ports for every destination (symmetric NAT): hole punching will fail; players need \
         the open port or a TURN relay",
        "Taking joins through 3 of 8 relays",
    ];
    for (w, h, ppp) in
        [(1280.0, 720.0, 1.0), (1280.0, 720.0, 2.0), (1280.0, 720.0, 3.0), (3440.0, 1440.0, 2.0)]
    {
        let mut harness = harness(egui::vec2(w / ppp, h / ppp), ppp);
        reach(&mut harness, Screen::Lobby);
        let mut v = crate::online::lobby_view(true);
        v.listing = Some("Announcing on the Internet…".into());
        v.reach = long.iter().map(|l| l.to_string()).collect();
        v.members[1].path = Some("open port".into());
        harness.state_mut().data.lobby = Some(v);
        harness.run();
        harness.step();
        let at = format!("{w}x{h} at {ppp} pixels a point");
        for line in long {
            let b = harness.get_by_label(line).accesskit_node().bounding_box().unwrap();
            assert!(b.x0 >= -0.5 && b.x1 as f32 <= w + 0.5, "{line:?} at {b:?} leaves the window: {at}");
            assert!(b.x1 > b.x0, "{line:?} is laid out: {at}");
        }
    }
}

/// Review Minor 5: the longest new texts outside the lobby fit too, at
/// the same sizes and scales: a join's failure under the Multiplayer
/// screen's button (rl-online's `no_path`, about 300 characters), and the
/// Options' port mapping switch and TURN relays (from 640 points wide).
#[test]
fn the_longest_join_texts_and_turn_options_fit() {
    let no_path = "no network path to the game: hole punching failed and no TURN relay could be used. The host can \
                   open port 23400 (TCP and UDP) on their router (UPnP, or port forwarding) or use a dedicated \
                   server; anyone can run a TURN relay (rl-server --relay)";
    for (w, h, ppp) in
        [(1280.0, 720.0, 1.0), (1280.0, 720.0, 2.0), (1280.0, 720.0, 3.0), (3440.0, 1440.0, 2.0)]
    {
        let at = format!("{w}x{h} at {ppp} pixels a point");
        let inside = |harness: &Harness<'static, App>, label: &str| {
            let b = harness.get_by_label(label).accesskit_node().bounding_box().unwrap();
            assert!(b.x0 >= -0.5 && b.x1 as f32 <= w + 0.5, "{label:?} at {b:?} leaves the window: {at}");
        };
        let mut harness = harness(egui::vec2(w / ppp, h / ppp), ppp);
        reach(&mut harness, Screen::Multiplayer);
        let app = harness.state_mut();
        app.ui.update(UiAction::Join { addr: "10.0.0.7".into(), password: None });
        app.ui.update(UiAction::OnlineFailed(no_path.into()));
        harness.run();
        harness.step();
        inside(&harness, no_path);
        // The options' four tabs need 500 points: at 427 the Online tab
        // cannot be clicked (the tab row predates plan 12b).
        if w / ppp < 500.0 {
            continue;
        }
        let mut settings = Settings::default();
        settings.online.finding.turn = vec!["anna:s3cret@turn.example.org:3478".into()];
        let ui = UiState::new(settings, Profiles::default(), 1);
        harness.state_mut().ui = ui;
        reach(&mut harness, Screen::Options);
        harness.get_by_label("Online").click();
        harness.run();
        harness.step();
        harness.step();
        inside(&harness, "Open the game's port on my router (UPnP/NAT-PMP/PCP)");
        inside(&harness, "anna:•••@turn.example.org:3478");
    }
}
