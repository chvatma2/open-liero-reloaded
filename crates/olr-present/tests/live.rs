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
    // Another timeline's state, the lamp off and the first console alarmed.
    let mut other = WorldSim::new(world(&c));
    let w = other.world_mut();
    w.set_live_state(copy(w, "lamp"), state(w, "lamp", "Off")).unwrap();
    w.set_live_state(copy(w, "console-a"), state(w, "console-a", "Alarm")).unwrap();
    for _ in 0..10 {
        other.tick(&idle());
    }
    let console = present.color().pixels().to_vec();
    sim.decode_state(&other.encode_state()).unwrap();
    present.state_loaded(sim.world());
    let mut net = NetPresenter::new();
    net.reset(10);
    let restored = sim.take_restored_areas();
    net.apply(&mut present, sim.world(), &restored, &[], &mut Vec::new());
    assert_eq!(glow_of(present.color(), sim.world(), "lamp"), 0, "the loaded lamp is off");
    assert_ne!(present.color().pixels(), &console[..], "the loaded console is alarmed");
    assert_drawn_as(&present, &p, sim.world(), "after the load");
}
