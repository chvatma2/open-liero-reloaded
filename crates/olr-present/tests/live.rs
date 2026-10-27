//! Live copies drawn by state (issue #87): the presentation draws each
//! copy as the world shows it, after each tick, through a rollback and a
//! loaded state, always what a colour map drawn afresh from that world
//! shows. On `fixtures/live/levels/live-test` laid over the imported
//! content (`OLR_CONTENT_DIR`); skips with a notice without it.

use std::path::Path;

use olr_content::{Content, LoadedLevel, LoadedMod, LoadedTheme, MatchOptions, Player};
use olr_netcode::world_sim::{TickRecorder, TickedTick};
use olr_netcode::{Simulation, TickInfo, WorldSim};
use olr_present::net::NetPresenter;
use olr_present::{PresentSetup, Presentation, default_colour};
use olr_scene::{SceneOptions, Viewport};
use olr_sim::{Keys, World};
use olr_terrain::{ColorMap, LiveFrame};

const MOD: &str = "olx-classic";
const LEVEL: &str = "olr-level-live-test";

fn content() -> Option<Content> {
    let Some(dir) = std::env::var("OLR_CONTENT_DIR").ok().filter(|d| Path::new(d).is_dir()) else {
        eprintln!("OLR_CONTENT_DIR not set or missing (run `cargo xtask fetch-olx-content`); skipping");
        return None;
    };
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/live/levels");
    Some(Content::open_layers(&[Path::new(&dir), &fixtures]).expect("the content directories"))
}

struct Packs {
    m: LoadedMod,
    level: LoadedLevel,
    theme: LoadedTheme,
}

fn packs(c: &Content) -> Packs {
    let level = c.load_level(LEVEL).unwrap();
    let theme = c.load_theme(&level.theme).unwrap();
    Packs { m: c.load_mod(MOD).unwrap(), level, theme }
}

/// A match on the fixture with two idle players.
fn world(c: &Content) -> World {
    c.build_match(&MatchOptions::deathmatch(MOD, LEVEL, 5, vec![Player::Human, Player::Human])).unwrap().world
}

fn presentation(c: &Content, p: &Packs, world: &World) -> Presentation {
    let setup = PresentSetup {
        core: c.core(),
        m: &p.m,
        level: &p.level,
        theme: &p.theme,
        viewport: Viewport::GAME,
        options: SceneOptions::default(),
        seed: 1,
    };
    let mut present = Presentation::new(&setup, world);
    let skin = c.skin_or_default("olx-skin-default");
    for i in 0..2 {
        present.add_worm(&skin, default_colour(i), i == 0, i == 0);
    }
    present
}

fn idle() -> [Keys; 2] {
    [Keys(0); 2]
}

fn copy(world: &World, id: &str) -> u16 {
    world.live().unwrap().level().copy(id).unwrap()
}

fn state(world: &World, id: &str, name: &str) -> u16 {
    world.live().unwrap().level().copy_by_id(id).unwrap().state(name).unwrap()
}

/// Asserts the presentation's colour map shows what one drawn afresh
/// over the same ground from `world`'s live copies shows.
fn assert_drawn_as(present: &Presentation, p: &Packs, world: &World, what: &str) {
    let mut fresh = present.color().clone();
    fresh.set_live_art(p.level.live_art());
    let mut frame = LiveFrame::default();
    olr_content::live_frame(world.live().unwrap(), &mut frame);
    fresh.set_live(&frame);
    let shown = present.color();
    assert!(shown.pixels() == fresh.pixels(), "{what}: the colours drawn afresh");
    assert_eq!(shown.glow(), fresh.glow(), "{what}: the glow drawn afresh");
    assert_eq!(shown.glow_groups(), fresh.glow_groups(), "{what}: the groups drawn afresh");
}

/// The glow shown over copy `id`'s rectangle, summed.
fn glow_of(map: &ColorMap, world: &World, id: &str) -> u64 {
    let c = world.live().unwrap().copy_by_id(id).unwrap();
    let (x, y, w, h) = c.rect();
    let (w, h) = (w as i32, h as i32);
    let s = map.art_scale() as i32;
    let width = map.width() as i32;
    let glow = map.glow().unwrap();
    let mut sum = 0;
    for ay in y * s..(y + h) * s {
        for ax in x * s..(x + w) * s {
            sum += u64::from(glow[(ay * width + ax) as usize]);
        }
    }
    sum
}

#[test]
fn a_live_copy_is_drawn_in_its_starting_state() {
    let Some(c) = content() else { return };
    let p = packs(&c);
    let world = world(&c);
    let present = presentation(&c, &p, &world);
    assert_drawn_as(&present, &p, &world, "at the start");
    assert_ne!(present.color().pixels(), present.color().ground_pixels(), "the copies are drawn");
    assert!(glow_of(present.color(), &world, "lamp") > 0, "the lamp starts dim");
}

#[test]
fn a_state_change_is_drawn_after_the_tick() {
    let Some(c) = content() else { return };
    let p = packs(&c);
    let mut world = world(&c);
    let mut present = presentation(&c, &p, &world);
    let mut events = olr_sim::TickEvents::new();
    for _ in 0..5 {
        world.tick(&idle(), &mut events);
        present.on_tick(&world, &events);
    }
    present.color_mut().take_dirty().for_each(drop);
    let dim = glow_of(present.color(), &world, "lamp");
    world.set_live_state(copy(&world, "lamp"), state(&world, "lamp", "Off")).unwrap();
    assert_eq!(glow_of(present.color(), &world, "lamp"), dim, "nothing changes before the tick");
    world.tick(&idle(), &mut events);
    present.on_tick(&world, &events);
    assert_eq!(glow_of(present.color(), &world, "lamp"), 0, "the lamp is off");
    assert_drawn_as(&present, &p, &world, "after the tick");
    let (x, y, w, h) = world.live().unwrap().copy_by_id("lamp").unwrap().rect();
    let (w, h) = (w as i32, h as i32);
    let s = present.color().art_scale() as i32;
    let dirty: Vec<_> = present.color_mut().take_dirty().collect();
    // The tiles under the lamp, and nothing else.
    let (x0, y0) = (dirty.iter().map(|r| r.x).min().unwrap(), dirty.iter().map(|r| r.y).min().unwrap());
    let x1 = dirty.iter().map(|r| r.x + r.w).max().unwrap();
    let y1 = dirty.iter().map(|r| r.y + r.h).max().unwrap();
    let area: u32 = dirty.iter().map(|r| r.w * r.h).sum();
    assert_eq!(area, (x1 - x0) * (y1 - y0), "one block: {dirty:?}");
    let (x0, y0, x1, y1) = (x0 as i32, y0 as i32, x1 as i32, y1 as i32);
    let tile = ColorMap::DIRTY_TILE as i32 * s;
    assert!(x0 <= x * s && y0 <= y * s && x1 >= (x + w) * s && y1 >= (y + h) * s, "the lamp's: {dirty:?}");
    assert!(x0 > x * s - tile && y0 > y * s - tile && x1 < (x + w) * s + tile && y1 < (y + h) * s + tile);
}

/// A predicted state change a correction erases: the corrected world's
/// state is drawn after the advance.
#[test]
fn a_rollback_draws_the_corrected_state() {
    let Some(c) = content() else { return };
    let p = packs(&c);
    let mut sim = WorldSim::new(world(&c));
    let mut present = presentation(&c, &p, sim.world());
    let (mut rec, mut net) = (TickRecorder::default(), NetPresenter::new());
    let mut advance = |sim: &mut WorldSim, present: &mut Presentation, tick: u32, resim: bool| {
        sim.tick(&idle());
        rec.simulated(sim, TickInfo { tick, resim, all_final: true });
        let ticked: Vec<TickedTick> = rec.take_ticked();
        let restored = sim.take_restored_areas();
        net.apply(present, sim.world(), &restored, &ticked, &mut Vec::new());
    };
    for t in 0..5 {
        advance(&mut sim, &mut present, t, false);
    }
    let dim = glow_of(present.color(), sim.world(), "lamp");
    let before = sim.save();
    let lamp = copy(sim.world(), "lamp");
    let off = state(sim.world(), "lamp", "Off");
    sim.world_mut().set_live_state(lamp, off).unwrap();
    advance(&mut sim, &mut present, 5, false);
    assert_eq!(glow_of(present.color(), sim.world(), "lamp"), 0, "predicted: the lamp is off");
    // The correction: tick 5 again without the change.
    sim.restore(&before);
    advance(&mut sim, &mut present, 5, true);
    assert_eq!(glow_of(present.color(), sim.world(), "lamp"), dim, "corrected: the lamp is dim");
    assert_drawn_as(&present, &p, sim.world(), "after the correction");
}

#[test]
fn a_loaded_state_draws_its_live_states() {
    let Some(c) = content() else { return };
    let p = packs(&c);
    let mut sim = WorldSim::new(world(&c));
    let mut present = presentation(&c, &p, sim.world());
    for _ in 0..20 {
        sim.tick(&idle());
        present.on_tick(sim.world(), sim.events());
    }
    // By tick 20 the level script has alarmed the consoles (the press
    // moves, #89) and the lamp is lit.
    let state_of = |w: &World, id: &str| w.live().unwrap().copy_by_id(id).unwrap().state_name().to_string();
    assert_eq!(state_of(sim.world(), "console-a"), "Alarm");
    assert_ne!(glow_of(present.color(), sim.world(), "lamp"), 0, "the lamp is lit");
    // Another timeline's state after one tick, before the scripts change
    // it (the lamp's first stutter is at tick 2 at the earliest, and the
    // consoles alarm once the press moves): the lamp off and the first
    // console normal.
    let mut other = WorldSim::new(world(&c));
    let w = other.world_mut();
    w.set_live_state(copy(w, "lamp"), state(w, "lamp", "Off")).unwrap();
    w.set_live_state(copy(w, "console-a"), state(w, "console-a", "Normal")).unwrap();
    other.tick(&idle());
    assert_eq!(state_of(other.world(), "lamp"), "Off");
    assert_eq!(state_of(other.world(), "console-a"), "Normal");
    let console = present.color().pixels().to_vec();
    sim.decode_state(&other.encode_state()).unwrap();
    present.state_loaded(sim.world());
    let mut net = NetPresenter::new();
    net.reset(1);
    let restored = sim.take_restored_areas();
    net.apply(&mut present, sim.world(), &restored, &[], &mut Vec::new());
    assert_eq!(glow_of(present.color(), sim.world(), "lamp"), 0, "the loaded lamp is off");
    assert_ne!(present.color().pixels(), &console[..], "the loaded console is normal");
    assert_drawn_as(&present, &p, sim.world(), "after the load");
}

/// The look `present` built last (a lit frame of `world`).
fn built(present: &mut Presentation, world: &World) -> olr_look::LookFrame {
    present.build(world, 0.0, &mut olr_scene::DrawList::new());
    present.look().expect("a lit level").clone()
}

/// The frame group of copy `id`'s group `group` (by id) in `look`.
fn group_of(p: &Packs, look: &olr_look::LookFrame, id: &str, group: &str) -> olr_look::FrameGroup {
    let live = p.level.live.as_ref().unwrap();
    let mut v = p.level.look.as_ref().map_or(0, |l| l.groups.len());
    for c in &live.light.copies {
        for g in &c.groups {
            if c.id == id && g.id == group {
                return look.groups[v];
            }
            v += 1;
        }
    }
    panic!("no group {group} of {id}")
}

/// Issue #93: each built frame's copies' groups take their values as the
/// world holds them after the tick: console-a's screen turns red, the
/// lamp's light dims by `lamp_light`, console-b stays as it was.
#[test]
fn a_copys_light_follows_its_values_each_tick() {
    let Some(c) = content() else { return };
    let p = packs(&c);
    let mut world = world(&c);
    let mut present = presentation(&c, &p, &world);
    let mut events = olr_sim::TickEvents::new();
    world.tick(&idle(), &mut events);
    present.on_tick(&world, &events);
    let before = built(&mut present, &world);
    let (a, b, lamp) = (
        group_of(&p, &before, "console-a", "screen"),
        group_of(&p, &before, "console-b", "screen"),
        group_of(&p, &before, "lamp", "lamp"),
    );
    assert!(a.cast[1] > a.cast[0], "console-a starts green: {:?}", a.cast);
    let live = world.live().unwrap().level().clone();
    let set = |world: &mut World, id: &str, name: &str, v: i64| {
        let c = live.copy(id).unwrap();
        world.set_live_value(c, live.copies[usize::from(c)].prop(name).unwrap(), v).unwrap();
    };
    set(&mut world, "console-a", "screen_colour", 0xff_2020);
    set(&mut world, "lamp", "lamp_light", 60);
    world.tick(&idle(), &mut events);
    present.on_tick(&world, &events);
    let after = built(&mut present, &world);
    let (a2, b2, lamp2) = (
        group_of(&p, &after, "console-a", "screen"),
        group_of(&p, &after, "console-b", "screen"),
        group_of(&p, &after, "lamp", "lamp"),
    );
    assert!(a2.cast[0] > a2.cast[1] && a2.cast[0] > a2.cast[2], "console-a red: {:?}", a2.cast);
    assert_eq!((b2.cast, b2.reach), (b.cast, b.reach), "console-b as it was");
    let k = lamp2.cast[0] / lamp.cast[0];
    assert!((k - 60.0 / 300.0).abs() < 1e-4, "the lamp at 60 % of its 300 %: {k}");
    assert_eq!(after.groups.len(), before.groups.len(), "the same groups, numbered as the map's");
}

/// The spot's beam as `present` placed it in its last built frame.
fn spot_beam(present: &mut Presentation, world: &World) -> Option<olr_look::glow_beams::GlowBeamSource> {
    let look = built(present, world);
    assert!(look.live_beams.len() <= 1, "only the spot beams");
    look.live_beams.first().cloned()
}

/// Issue #93: the spot's lens beam is placed at the lens (its part's
/// place), turned to `beam_dir` and spread `beam_spread`, and mirrored with
/// the copy's `Facing` (`spot_facing`); its beam map texels follow.
#[test]
fn live_beams_are_placed_turned_and_mirrored() {
    let Some(c) = content() else { return };
    let p = packs(&c);
    let mut world = world(&c);
    let mut present = presentation(&c, &p, &world);
    let live = world.live().unwrap().level().clone();
    let spot = live.copy("spot").unwrap();
    let setup = &live.copies[usize::from(spot)];
    let prop = |name: &str| setup.prop(name).unwrap();
    let b = spot_beam(&mut present, &world).expect("the spot beams");
    // At its default, 45° spread 20; the lens a 1 × 2 column at (6, 1) of
    // the 7 × 4 spot at (150, 40): its face's middle at the lens's far
    // corner, within a pixel.
    let (x, y) = (150.0 + 6.0, 40.0 + 1.0);
    let towards = |b: &olr_look::glow_beams::GlowBeamSource, deg: f32| {
        let d = deg.to_radians();
        (b.dir[0] - d.cos()).abs() < 1e-5 && (b.dir[1] - d.sin()).abs() < 1e-5
    };
    assert!(towards(&b, 45.0), "{:?}", b.dir);
    assert!((b.spread - 20f32.to_radians()).abs() < 1e-5);
    assert!(b.at[0] >= x && b.at[0] <= x + 2.0 && b.at[1] >= y && b.at[1] <= y + 3.0, "{:?}", b.at);
    assert!(b.back, "a background part's beam");
    let mut events = olr_sim::TickEvents::new();
    let mut tick = |world: &mut World, present: &mut Presentation| {
        world.tick(&idle(), &mut events);
        present.on_tick(world, &events);
    };
    // Turned: the value, after the tick.
    world.set_live_value(spot, prop("beam_dir"), 100).unwrap();
    world.set_live_value(spot, prop("beam_spread"), 30).unwrap();
    tick(&mut world, &mut present);
    let t = spot_beam(&mut present, &world).unwrap();
    assert!(towards(&t, 100.0) && (t.spread - 30f32.to_radians()).abs() < 1e-5, "{:?}", t.dir);
    // Its beam map texel at the lens: 100° in 2° steps, spread 30.
    let texel = |present: &Presentation, x: usize, y: usize| {
        let beams = present.color().shown_beams().expect("a shown beam map");
        let i = (y * present.color().material().width() as usize + x) * 2;
        [beams[i], beams[i + 1]]
    };
    assert_eq!(texel(&present, 156, 41), [51, 30]);
    // Mirrored: the lens at the copy's left, beaming at 80°.
    world.set_live_value(spot, prop("spot_facing"), 1).unwrap();
    tick(&mut world, &mut present);
    let m = spot_beam(&mut present, &world).unwrap();
    assert!(towards(&m, 80.0), "mirrored to 80°: {:?}", m.dir);
    assert!(m.at[0] >= 149.0 && m.at[0] <= 152.0, "at the copy's left: {:?}", m.at);
    assert_eq!(texel(&present, 150, 41), [41, 30], "its texel mirrored there");
    assert_eq!(texel(&present, 156, 41), [0, 0], "none where it was");
}

/// Issue #93: a hidden part casts no beam: the spot's lens hidden by its
/// `LayerVisible` (`lens_on`), then the whole copy by none of its parts
/// showing, leaves the frame without live beams, and its texels go.
#[test]
fn a_hidden_part_casts_no_beam() {
    let Some(c) = content() else { return };
    let p = packs(&c);
    let mut world = world(&c);
    let mut present = presentation(&c, &p, &world);
    assert!(spot_beam(&mut present, &world).is_some());
    let live = world.live().unwrap().level().clone();
    let spot = live.copy("spot").unwrap();
    let lens_on = live.copies[usize::from(spot)].prop("lens_on").unwrap();
    world.set_live_value(spot, lens_on, 0).unwrap();
    let mut events = olr_sim::TickEvents::new();
    world.tick(&idle(), &mut events);
    present.on_tick(&world, &events);
    assert!(spot_beam(&mut present, &world).is_none(), "the lens hidden: no beam");
    let beams = present.color().shown_beams().unwrap();
    let i = (41 * present.color().material().width() as usize + 156) * 2;
    assert_eq!(&beams[i..i + 2], &[0, 0], "nor its texels");
    assert_drawn_as(&present, &p, &world, "the lens hidden");
}
