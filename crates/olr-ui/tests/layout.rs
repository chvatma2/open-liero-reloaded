//! The menus' page frame: every sub-screen (local play, the options and
//! each of their tabs, the Multiplayer screen's tabs, profiles, replays,
//! about) draws one column centred in the window, whatever its
//! size, with its title centred above
//! and Back at the column's left edge; nothing hugs a corner of a big
//! screen.

use egui::accesskit::Role;
use egui::{Rect, pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use olr_content::{Catalog, LevelInfo, ModInfo, SkinInfo};
use olr_ui::screens::{self, MenuData};
use olr_ui::{AppCommand, Profiles, Screen, Settings, UiAction, UiState};

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
    let h = Harness::builder().with_size(size).with_pixels_per_point(ppp).build_ui_state(
        |ui, app: &mut App| {
            for a in screens::show(ui, &app.ui, &mut app.data) {
                let c = app.ui.update(a);
                app.commands.extend(c);
            }
        },
        app,
    );
    // The game's look, as players get it: the theme's text sizes and spacing.
    olr_ui::theme::apply(&h.ctx);
    h
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
    (Screen::About, None),
    (Screen::Options, Some("Video")),
    (Screen::Multiplayer, None),
    (Screen::Multiplayer, Some("LAN")),
    (Screen::Multiplayer, Some("Direct")),
    (Screen::Host, None),
    (Screen::Lobby, None),
    (Screen::Editor, None),
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
        Screen::Main => &[],
        s => &[s],
    };
    for s in path {
        app.ui.update(UiAction::Open(*s));
    }
    if screen == Screen::Lobby {
        app.ui.update(UiAction::Host(olr_ui::HostPrefs::default()));
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
        Screen::About => "About",
        Screen::Multiplayer => "Multiplayer",
        Screen::Host => "Host a game",
        Screen::Lobby => "Lobby",
        Screen::Editor => "Editor",
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
            // The page's panels (the scroll areas of the body) end where
            // the footer begins, 10 points above its button: what a scroll
            // area has scrolled past that line is clipped, not on the page
            // (the accessibility tree still has its bounds), so a widget
            // starting above the line counts down to the line. One starting
            // in the footer is below Start's panel, and counts in full.
            let footer = b.y0 as f32 - 10.0;
            let shown = |r: Rect| if r.top() < footer { r.bottom().min(footer) } else { r.bottom() };
            let lowest = others.map(|(_, r)| shown(r)).fold(0.0, f32::max);
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
/// screen's button (olr-online's `no_path`, about 300 characters), and the
/// Options' tab row and Online tab, every widget of it (ruling UI-1: at
/// 427 points too, and in borderless fullscreen on 16:10 and 4:3 desktops
/// at UI scale 3, 384 and 320 points wide).
#[test]
fn the_longest_join_texts_and_turn_options_fit() {
    let no_path = "no network path to the game: hole punching failed and no TURN relay could be used. The host can \
                   open port 23400 (TCP and UDP) on their router (UPnP, or port forwarding) or use a dedicated \
                   server; anyone can run a TURN relay (olr-server --relay)";
    // Windows, and borderless fullscreen on 16:10 and 4:3 desktops at UI
    // scale 3 (`theme::pixels_per_point`: height / 720 × 3; 384 × 240 and
    // 320 × 240 points).
    let fullscreen = |w: f32, h: f32| (w, h, olr_ui::theme::pixels_per_point(h, 3.0));
    for (w, h, ppp) in [
        (1280.0, 720.0, 1.0),
        (1280.0, 720.0, 2.0),
        (1280.0, 720.0, 3.0),
        (3440.0, 1440.0, 2.0),
        fullscreen(1920.0, 1200.0),
        fullscreen(1024.0, 768.0),
    ] {
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
        for tab in ["Internet", "LAN", "Direct"] {
            inside(&harness, tab);
        }
        let mut settings = Settings::default();
        settings.online.finding.turn = vec!["anna:s3cret@turn.example.org:3478".into()];
        let ui = UiState::new(settings, Profiles::default(), 1);
        harness.state_mut().ui = ui;
        reach(&mut harness, Screen::Options);
        for tab in ["Video", "Audio", "Keys", "Online"] {
            inside(&harness, tab);
        }
        harness.get_by_label("Online").click();
        harness.run();
        harness.step();
        harness.step();
        // Every widget of the Online tab, not only these.
        for (name, r) in widgets(&harness) {
            assert!(r.left() >= -0.5 && r.right() <= w + 0.5, "{name:?} at {r:?} leaves the window: {at}");
        }
        inside(&harness, "Open the game's port on my router (UPnP/NAT-PMP/PCP)");
        inside(&harness, "anna:•••@turn.example.org:3478");
    }
}

/// The Internet tab's games at every size above (and 4K): long names, the
/// widest players text, every region; each widget stays in the window, and
/// a game can be selected and joined.
#[test]
fn the_internet_table_fits_and_a_game_can_be_joined() {
    use olr_ui::{InternetEntry, InternetStatus, InternetView, PingView, Region};
    let fullscreen = |w: f32, h: f32| (w, h, olr_ui::theme::pixels_per_point(h, 3.0));
    for (w, h, ppp) in [
        (1280.0, 720.0, 1.0),
        (1280.0, 720.0, 2.0),
        (1280.0, 720.0, 3.0),
        (3440.0, 1440.0, 2.0),
        (3840.0, 2160.0, 2.0),
        fullscreen(1920.0, 1200.0),
        fullscreen(1024.0, 768.0),
    ] {
        let at = format!("{w}x{h} at {ppp} pixels a point");
        let games: Vec<InternetEntry> = Region::ALL
            .iter()
            .enumerate()
            .map(|(i, region)| InternetEntry {
                key: [i as u8 + 1; 32],
                name: format!("Game {i} with a very long name that nobody could ever fit in a column"),
                map: "A level whose name is long too".into(),
                mod_name: "Liero v1.0 extended edition".into(),
                players: 8,
                max_players: 8,
                bots: 24,
                ping: if i % 2 == 0 { PingView::Ms(123) } else { PingView::NoAnswer },
                region: *region,
                password: i % 3 == 0,
                addr: (i != 1).then(|| format!("81.2.69.{i}:23400").parse().unwrap()),
                incompatible: (i == 2).then(|| "needs version 0.12.3-beta".to_string()),
                playing: true,
            })
            .collect();
        // The one to join: not full, with an address, of this version.
        let mut games = games;
        games[0].players = 3;
        games[0].password = false;
        let mut harness = harness(egui::vec2(w / ppp, h / ppp), ppp);
        reach(&mut harness, Screen::Multiplayer);
        harness.state_mut().data.internet = InternetView { status: InternetStatus::Listed, games };
        harness.run();
        harness.step();
        harness.step();
        for (name, r) in widgets(&harness) {
            assert!(r.left() >= -0.5 && r.right() <= w + 0.5, "{name:?} at {r:?} leaves the window: {at}");
        }
        // The aligned table with its headers on wide screens, the two-line
        // form (no headers) on narrow ones; in between either.
        let points = w / ppp;
        let headers = harness.query_by_label("Players").is_some();
        if points >= 1700.0 {
            assert!(headers, "{at}: the table has its headers");
            for h in ["Name", "Map", "Mod", "Ping", "Region"] {
                harness.get_by_label(h);
            }
        }
        if points <= 640.0 {
            assert!(!headers, "{at}: the games take two lines each");
        }
        // Select the first game by its name's start; Join and Block appear.
        harness.get_by_label_contains("Game 0 with").click();
        harness.run();
        harness.step();
        for (n, r) in widgets(&harness) {
            assert!(r.left() >= -0.5 && r.right() <= w + 0.5, "{n:?} at {r:?} leaves the window: {at}");
        }
        harness.get_by_label("Block this host");
        // A window too short for the list scrolls to the button.
        harness.get_by_role_and_label(Role::Button, "Join").scroll_to_me();
        harness.run();
        harness.step();
        harness.get_by_role_and_label(Role::Button, "Join").click();
        harness.run();
        let sent = harness.state().commands.iter().any(|c| matches!(c, AppCommand::JoinGame { .. }));
        let cmds = &harness.state().commands;
        assert!(sent, "{at}: Join sent no JoinListed: {cmds:?}");
    }
}

/// Plan 12c Task 9: the Host lost screen and a migrated lobby's listing
/// line at 4K, on the user's ultrawide screen, in a 1280 × 720 window at
/// UI scale 3 and in borderless fullscreen on 16:10 and 4:3 desktops at
/// UI scale 3 (384 and 320 points wide): every widget in the window, the
/// screen a centred column (its title centred, Back at its left), the
/// reason and the listing wrapping.
#[test]
fn the_host_lost_screen_and_a_migrated_lobby_fit() {
    let why = "The host left and no other player could take over the game.";
    let fullscreen = |w: f32, h: f32| (w, h, olr_ui::theme::pixels_per_point(h, 3.0));
    for (w, h, ppp) in [
        (3840.0, 2160.0, 2.0),
        (3440.0, 1440.0, 2.0),
        (1280.0, 720.0, 3.0),
        fullscreen(1920.0, 1200.0),
        fullscreen(1024.0, 768.0),
    ] {
        let at = format!("{w}x{h} at {ppp} pixels a point");
        let window = Rect::from_min_max(pos2(0.0, 0.0), pos2(w, h));
        // `down`: also within the window's height (the lobby's lists
        // scroll, so its rows are checked across only).
        let inside = |name: &str, r: Rect, down: bool| {
            let across = r.left() >= -0.5 && r.right() <= w + 0.5;
            let along = r.top() >= -0.5 && r.bottom() <= h + 0.5;
            assert!(across && (along || !down), "{name:?} at {r:?} leaves the window: {at}");
        };
        let mut harness = harness(egui::vec2(w / ppp, h / ppp), ppp);
        reach(&mut harness, Screen::Lobby);
        let mut v = crate::online::lobby_view(true);
        v.listing = Some(olr_ui::MIGRATED_LISTING.into());
        harness.state_mut().data.lobby = Some(v);
        harness.run();
        harness.step();
        // The host's controls and the listing line (the setup's panels
        // scroll in their column).
        let found = widgets(&harness);
        for label in [olr_ui::MIGRATED_LISTING, "Start", "Force start", "Kick", "Ban", "To spectators"] {
            let r = found.iter().find(|(n, _)| n == label).map(|(_, r)| *r);
            inside(label, r.unwrap_or_else(|| panic!("{at}: {label:?} in {found:?}")), false);
        }
        // The match starts, its host is lost: the Host lost screen.
        let app = harness.state_mut();
        app.ui.update(UiAction::MatchStarted);
        app.data.host_lost = Some(why.into());
        app.data.host_lost_replay = true;
        app.ui.update(UiAction::HostLost(why.into()));
        harness.run();
        harness.step();
        harness.step();
        assert_eq!(harness.state().ui.screen(), Screen::HostLost, "{at}");
        let found = widgets(&harness);
        for (name, r) in &found {
            inside(name, *r, true);
        }
        for label in [why, olr_ui::screens::HOST_LOST_REPLAY, "Replays", "Back", "Host lost"] {
            assert!(found.iter().any(|(n, _)| n == label), "{at}: {label:?} in {found:?}");
        }
        let all = found.iter().fold(Rect::NOTHING, |u, (_, r)| u.union(*r));
        let off = all.center().x - window.center().x;
        assert!(off.abs() <= 4.0 * ppp, "{at}: the content's centre is {off} px off, {all:?}");
        let title = found.iter().find(|(n, _)| n == "Host lost").map(|(_, r)| *r).unwrap();
        assert!((title.center().x - window.center().x).abs() <= 4.0 * ppp, "{at}: the title is off centre");
        let back = harness.get_by_label("Back").accesskit_node().bounding_box().unwrap();
        assert!((back.x0 as f32 - all.left()).abs() <= 1.0, "{at}: Back is not at the column's left");
        if w / ppp > 2000.0 {
            assert!(all.width() < 0.5 * w, "{at}: the content spans {} of {w}", all.width());
        }
    }
}

/// Every node's name, role and bounds, in tree order.
fn all_nodes(h: &Harness<'static, App>) -> Vec<(String, Rect)> {
    let mut out = Vec::new();
    for n in h.root().children_recursive() {
        let a = n.accesskit_node();
        let Some(b) = a.bounding_box() else { continue };
        let name = format!("{:?} {:?}", a.role(), a.label().or(a.value()).unwrap_or_default());
        out.push((name, Rect::from_min_max(pos2(b.x0 as f32, b.y0 as f32), pos2(b.x1 as f32, b.y1 as f32))));
    }
    out
}

/// Hovering a widget changes only its colours: no widget, container or
/// text moves or changes size on any screen (the playtest's "the layout
/// changes by a pixel or two when I highlight a control").
#[test]
fn hovering_a_widget_never_changes_the_layout() {
    type Setup = Box<dyn Fn(&mut Harness<'static, App>)>;
    // Every screen, then the ones a screen alone does not show: the lobby
    // with its members (Start, Kick, Ban, the seat buttons), the Internet
    // tab with games (the selectable rows, the selected one's Join) and
    // the Host lost screen. `must`: widgets each has to offer, so a case
    // that stops reaching its screen fails.
    let mut cases: Vec<(String, Setup, &[&str])> = Vec::new();
    for (screen, tab) in SCREENS.into_iter().chain([(Screen::Main, None)]) {
        cases.push((
            format!("{screen:?} {tab:?}"),
            Box::new(move |h| {
                reach(h, screen);
                if let Some(tab) = tab {
                    h.get_by_label(tab).click();
                    h.run();
                }
            }),
            if screen == Screen::Lobby { &["Start", "Kick", "Ban", "To spectators"] } else { &[] },
        ));
    }
    cases.push((
        "Internet games".into(),
        Box::new(|h| {
            use olr_ui::{InternetEntry, InternetStatus, InternetView, PingView, Region};
            reach(h, Screen::Multiplayer);
            let games = (0..3u8)
                .map(|i| InternetEntry {
                    key: [i + 1; 32],
                    name: format!("Game {i}"),
                    map: "Dirt Level".into(),
                    mod_name: "Classic".into(),
                    players: 2,
                    max_players: 8,
                    bots: 1,
                    ping: PingView::Ms(40),
                    region: Region::ALL[0],
                    password: i == 1,
                    addr: Some(format!("81.2.69.{i}:23400").parse().unwrap()),
                    incompatible: None,
                    playing: false,
                })
                .collect();
            h.state_mut().data.internet = InternetView { status: InternetStatus::Listed, games };
            h.run();
            h.get_by_label("Game 0").click();
            h.run();
        }),
        &["Game 0", "Game 1", "Join"],
    ));
    cases.push((
        "Host lost".into(),
        Box::new(|h| {
            reach(h, Screen::Lobby);
            let why = "The host left and no other player could take over the game.";
            let app = h.state_mut();
            app.ui.update(UiAction::MatchStarted);
            app.data.host_lost = Some(why.into());
            app.data.host_lost_replay = true;
            app.ui.update(UiAction::HostLost(why.into()));
            h.run();
            h.step();
            h.step();
        }),
        &["Replays", "Back"],
    ));
    for (w, h, ppp) in [(1280.0, 720.0, 1.0), (3440.0, 1440.0, 2.0)] {
        let mut harness = harness(egui::vec2(w / ppp, h / ppp), ppp);
        for (screen, setup, must) in &cases {
            setup(&mut harness);
            harness.hover_at(pos2(1.0, 1.0));
            harness.run();
            let before = all_nodes(&harness);
            let targets: Vec<(String, Rect)> =
                widgets(&harness).into_iter().filter(|(n, _)| !n.is_empty()).collect();
            for label in *must {
                assert!(targets.iter().any(|(n, _)| n == label), "{screen} at {w}x{h}: no {label:?}");
            }
            let mut bad = Vec::new();
            for (name, r) in targets {
                harness.hover_at(r.center() / ppp);
                harness.run();
                let during = all_nodes(&harness);
                // A tooltip is a new node after the others; the rest stay.
                if during.len() < before.len() {
                    bad.push(format!("hover {name:?}: {} nodes became {}", before.len(), during.len()));
                }
                for (b, d) in before.iter().zip(&during) {
                    if b != d {
                        bad.push(format!("hover {name:?}: {} {:?} became {:?}", b.0, b.1, d.1));
                    }
                }
                harness.hover_at(pos2(1.0, 1.0));
                harness.run();
            }
            bad.dedup();
            assert!(
                bad.is_empty(),
                "{screen} at {w}x{h}: {} changes, e.g.\n{}",
                bad.len(),
                bad[..bad.len().min(8)].join("\n")
            );
        }
    }
}
