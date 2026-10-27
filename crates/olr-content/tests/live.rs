//! A level's live copies (issue #85): loaded with the level from its
//! played form (`fixtures/live/levels/live-test`, made by
//! `fixtures/live/make-live-test.sh`), refused with it when they do not
//! check, the same from the editor's bake as from the saved pack.

use std::path::{Path, PathBuf};

use olr_content::{Content, LoadedLevel};
use olr_data::image::RgbaImage;
use olr_data::level::Material;
use olr_sim::{LivePartKind, LiveTarget};

const ID: &str = "olr-level-live-test";

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/live/levels/live-test")
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let path = e.path();
        if path.is_dir() {
            copy_dir(&path, &to.join(e.file_name()));
        } else {
            std::fs::copy(&path, to.join(e.file_name())).unwrap();
        }
    }
}

/// A content directory of the synthetic packs and the fixture level,
/// `edit` applied to the level's copy.
fn content(edit: impl FnOnce(&Path)) -> (tempfile::TempDir, Content) {
    let tmp = tempfile::tempdir().unwrap();
    super::content::synthetic(tmp.path());
    let dir = tmp.path().join("live-test");
    copy_dir(&fixture(), &dir);
    edit(&dir);
    let content = Content::open(tmp.path()).unwrap();
    (tmp, content)
}

#[test]
fn a_live_level_loads_its_copies() {
    let (_tmp, content) = content(|_| ());
    let level = content.load_level(ID).unwrap();
    let live = level.live_sim().expect("live copies");
    let ids: Vec<&str> = live.copies.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, ["lamp", "console-a", "console-b", "press", "crate", "gate"]);
    let copy = |id: &str| live.copy_by_id(id).unwrap();
    // The lamp: three states, starting dim, a click entering Off.
    let lamp = copy("lamp");
    let names: Vec<&str> = lamp.states.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["On", "Off", "Dim"]);
    assert_eq!(lamp.state("Dim"), Some(lamp.start_state));
    let click = lamp.sound("click.wav").unwrap();
    assert_eq!(lamp.states[usize::from(lamp.state("Off").unwrap())].enter_sound, Some(click));
    let strength: Vec<i64> = lamp.states.iter().map(|s| s.values[0].1).collect();
    assert_eq!(strength, [80, 0, 30]);
    assert_eq!(lamp.props[0].target, LiveTarget::GlowStrength(1));
    assert!(lamp.states[0].parts.iter().any(|p| p.kind == LivePartKind::Background));
    // The consoles: one asset, apart in colour and width.
    let (a, b) = (copy("console-a"), copy("console-b"));
    let colour = |c: &olr_sim::LiveCopy, state: &str| {
        let i = c.prop("screen_colour").unwrap();
        let s = &c.states[usize::from(c.state(state).unwrap())];
        s.values.iter().find(|v| v.0 == i).unwrap().1
    };
    assert_eq!((colour(a, "Normal"), colour(b, "Normal")), (0x20ff40, 0x4080ff));
    assert_eq!((colour(a, "Alarm"), colour(b, "Alarm")), (0xff2020, 0xff2020));
    assert_eq!((a.size, b.size), ((12, 10), (24, 10)));
    assert_eq!(b.start_state, b.state("Alarm").unwrap());
    // The press: its head's height, a rock part of its own, set to 3.
    let press = copy("press");
    let head_y = press.prop("head_y").unwrap();
    let p = &press.props[usize::from(head_y)];
    assert_eq!((p.target, p.min, p.max), (LiveTarget::OffsetY(1), 0, 6));
    assert_eq!(press.start_values, [(head_y, 3)]);
    let head = press.states[0].parts.iter().find(|p| p.layer == Some(1)).unwrap();
    assert_eq!(head.kind, LivePartKind::Rock);
    assert!(press.pictures[usize::from(head.picture)].solid(0, 0));
    // The crate: 40 points, wrecked at 0, solid but not in the ground.
    let crate_ = copy("crate");
    let health = crate_.health.unwrap();
    assert_eq!((health.points, Some(health.at_zero)), (40, crate_.state("Wrecked")));
    let (x, y) = crate_.at;
    assert_eq!(level.material.get(x as u32 + 1, y as u32 + 1), Material::Background);
    // The gate: a mixed layer, a background and a rock part driven together.
    let gate = copy("gate");
    let kinds: Vec<(Option<u16>, LivePartKind)> =
        gate.states[0].parts.iter().map(|p| (p.layer, p.kind)).collect();
    assert_eq!(kinds, [(Some(0), LivePartKind::Background), (Some(0), LivePartKind::Rock)]);
    assert_eq!(gate.start_values, [(gate.prop("gate_x").unwrap(), -2)]);
    // Scripts: the lamp's default, the gate's own.
    let script = |c: &olr_sim::LiveCopy| live.scripts[usize::from(c.scripts[0])].clone();
    assert_eq!((script(lamp), script(gate)), ("fn on_enter(state) {}\n".into(), "fn tick() {}\n".into()));
    // Presentation's share: the pictures and the sound's bytes.
    let source = &level.live.as_ref().unwrap().source;
    assert!(source.sounds[usize::from(click)].starts_with(b"RIFF"));
    assert_eq!(source.pictures.len(), 6);
}

#[test]
fn a_live_file_that_does_not_check_refuses_the_level() {
    // A start state past the states.
    let (_tmp, c) = content(|dir| {
        let path = dir.join("live.ron");
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, text.replacen("start_state: 2", "start_state: 9", 1)).unwrap();
    });
    let err = format!("{:#}", c.load_level(ID).unwrap_err());
    assert!(err.contains("live.ron"), "{err}");
    // A picture missing.
    let (_tmp, c) = content(|dir| std::fs::remove_file(dir.join("live/0/0.png")).unwrap());
    assert!(c.load_level(ID).is_err());
    // A sound that does not play.
    let (_tmp, c) = content(|dir| std::fs::write(dir.join("live/0.wav"), b"RIFF not a sound").unwrap());
    let err = format!("{:#}", c.load_level(ID).unwrap_err());
    assert!(err.contains("live/0.wav"), "{err}");
}

/// Issue #88: the scripts are compiled with the level; one that does not
/// compile refuses it, naming its file.
#[test]
fn a_script_that_does_not_compile_refuses_the_level() {
    let (_tmp, c) = content(|_| ());
    assert!(c.load_level(ID).unwrap().live_director().is_some(), "the fixture's scripts run");
    let (_tmp, c) =
        content(|dir| std::fs::write(dir.join("live/1.rhai"), "fn tick() { let x = 1.5; }").unwrap());
    let err = format!("{:#}", c.load_level(ID).unwrap_err());
    assert!(err.contains("live/1.rhai"), "{err}");
}

#[test]
fn a_level_without_live_copies_has_none() {
    let (_tmp, content) = content(|_| ());
    let level = content.load_level("test-level").unwrap();
    assert!(level.live.is_none() && level.live_sim().is_none() && level.live_director().is_none());
}

/// The editor plays its bake in memory: the same live copies as the
/// saved pack's.
#[test]
fn from_source_gives_the_saved_packs_live_level() {
    let (_tmp, content) = content(|_| ());
    let saved = content.load_level(ID).unwrap().live_sim().unwrap();
    let mut pack = olr_edit::pack::load(&fixture()).unwrap();
    olr_edit::placed::bake_pack(&mut pack, &RgbaImage::new(1, 1)).unwrap();
    let olr_edit::pack::Source::Level(src) = &pack.source else { panic!("a level") };
    let baked = LoadedLevel::from_source(src).unwrap().live_sim().unwrap();
    assert_eq!(*baked, *saved);
}

/// Issue #86: a match on a live level holds its copies' state, entered at
/// their starting states, and a replay's assets carry the same copies.
#[test]
fn a_live_levels_world_holds_its_copies() {
    let (_tmp, content) = content(|dir| {
        let path = dir.join("level.ron");
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, text.replace("olx-theme-dirt", "test-theme")).unwrap();
    });
    let setup = olr_content::SandboxSetup {
        mod_id: "broken-mod".into(),
        level_id: ID.into(),
        seed: 3,
        settings: olr_sim::SimSettings::lx56(),
        worms: vec![olr_content::SpawnSpec::default()],
        restrictions: olr_content::DEFAULT_PRESET.into(),
    };
    let world = content.new_world(&setup).unwrap();
    let live = world.live().expect("live copies in the world");
    let states: Vec<(&str, &str)> = live.copies().map(|c| (c.id(), c.state_name())).collect();
    assert_eq!(states[..3], [("lamp", "Dim"), ("console-a", "Normal"), ("console-b", "Alarm")]);
    // The press's own start value, in 1/256 pixel.
    let press = live.copy_by_id("press").unwrap();
    assert_eq!(press.value("head_y"), Some(3 * olr_sim::LIVE_SUBPIXELS));
    let (m, level) = (content.load_mod("broken-mod").unwrap(), content.load_level(ID).unwrap());
    let theme = content.load_theme(&level.theme).unwrap();
    let assets = olr_content::replay_assets_from(&m, &level, &theme);
    assert_eq!(assets.live.as_deref(), Some(live.level()));
    // A level without live copies has none in its world.
    let plain =
        content.new_world(&olr_content::SandboxSetup { level_id: "test-level".into(), ..setup }).unwrap();
    assert!(plain.live().is_none());
}

/// The fixture on the synthetic packs' theme, its world (one worm) and
/// its level and colour map with the live copies drawn as the world
/// starts them (issue #87).
fn drawn() -> (tempfile::TempDir, LoadedLevel, olr_terrain::ColorMap) {
    let (tmp, content) = content(|dir| {
        let path = dir.join("level.ron");
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, text.replace("olx-theme-dirt", "test-theme")).unwrap();
    });
    let setup = olr_content::SandboxSetup {
        mod_id: "broken-mod".into(),
        level_id: ID.into(),
        seed: 3,
        settings: olr_sim::SimSettings::lx56(),
        worms: vec![olr_content::SpawnSpec::default()],
        restrictions: olr_content::DEFAULT_PRESET.into(),
    };
    let world = content.new_world(&setup).unwrap();
    let level = content.load_level(ID).unwrap();
    let theme = content.load_theme(&level.theme).unwrap();
    let mut map = level.color_map(&theme);
    map.set_live_art(level.live_art());
    let mut frame = olr_terrain::LiveFrame::default();
    olr_content::live_frame(world.live().unwrap(), &mut frame);
    map.set_live(&frame);
    (tmp, level, map)
}

/// Issue #87: a live part glows its unscaled mask × its `GlowStrength`
/// value, as the static bake computes it: every copy (the dim lamp at 30
/// %, the consoles' screens over their cases) shows the glow the same
/// copies baked static show, and its colours.
#[test]
fn glow_strength_is_mask_times_value() {
    let (_tmp, live, map) = drawn();
    let mut pack = olr_edit::pack::load(&fixture()).unwrap();
    let sim = live.live_sim().unwrap();
    for c in &sim.copies {
        olr_edit::placed::set_copy(&mut pack, &c.id, olr_edit::placed::CopyProperty::Live(false)).unwrap();
    }
    olr_edit::placed::bake_pack(&mut pack, &RgbaImage::new(1, 1)).unwrap();
    let olr_edit::pack::Source::Level(src) = &pack.source else { panic!("a level") };
    let fixed = LoadedLevel::from_source(src).unwrap();
    let static_glow = &fixed.glow.as_ref().expect("the static copies glow").front;
    let (s, w) = (live.art_scale, map.width());
    let mut lit = 0;
    for c in &sim.copies {
        let (x0, y0) = (c.at.0 as u32 * s, c.at.1 as u32 * s);
        for y in y0..y0 + c.size.1 * s {
            for x in x0..x0 + c.size.0 * s {
                let i = (y * w + x) as usize;
                assert_eq!(map.glow().unwrap()[i], static_glow[i], "{}: art pixel ({x}, {y})", c.id);
                let shown = &map.pixels()[i * 4..i * 4 + 4];
                let baked = fixed.front.get(x, y);
                assert_eq!(shown, [baked.r, baked.g, baked.b, baked.a], "{}: art pixel ({x}, {y})", c.id);
                lit += usize::from(static_glow[i] != 0);
            }
        }
    }
    assert!(lit > 0, "the bulb and the screens glow");
    assert_eq!(olr_terrain::live_glow(30, 255), olr_format::levels::glow_value(30, 255));
}

/// Issue #87: the fixture has no glow maps of its own (#85's bake gave it
/// the default look): its live copies give the colour map a glow
/// channel, their own groups after the level's (none), in copy order.
#[test]
fn a_level_lit_only_by_live_copies_draws_lit() {
    let (_tmp, level, map) = drawn();
    assert!(level.glow.is_none() && level.has_look());
    let groups = map.glow_groups().expect("a glow channel for the live copies");
    let live = level.live.as_ref().unwrap();
    let ids: Vec<&str> = live.groups.iter().flatten().map(|g| g.id.as_str()).collect();
    assert_eq!(ids, ["lamp", "screen", "screen"], "the lamp's, then each console's");
    let at = |id: &str| live.sim.copy_by_id(id).unwrap().at;
    let shown = |id: &str| {
        let (x, y) = at(id);
        let w = level.material.width();
        let c = live.sim.copy_by_id(id).unwrap();
        (0..c.size.1).flat_map(|dy| (0..c.size.0).map(move |dx| (dx, dy))).find_map(|(dx, dy)| {
            Some(groups[((y as u32 + dy) * w + x as u32 + dx) as usize]).filter(|&v| v != 0)
        })
    };
    assert_eq!((shown("lamp"), shown("console-a"), shown("console-b")), (Some(1), Some(2), Some(3)));
    assert!(map.glow().unwrap().iter().any(|&g| g != 0));
}
