//! Issue #27 (split screen): a camera per view over one shared scene. Each
//! view's draw list is what a scene of one view with that camera draws,
//! the cameras follow their own worms (shake, death, respawn), and the
//! look records are kept once.

use crate::common;

use olr_content::olx_mod_id;
use olr_math::Vec2;
use olr_scene::{
    Camera, DrawList, Interpolation, MAX_VIEWS, Scene, SceneAssets, SceneOptions, ShadeRules, View, Viewport,
};

const OPTIONS: SceneOptions = SceneOptions { interpolation: Interpolation::Linear, show_health_bars: true };

/// A scene of one view following worm id `target`, its camera seeded
/// `seed`; `lit` gives it shade rules (occluders and look records).
fn scene(target: i32, seed: u32, lit: bool) -> Scene {
    let mut s = Scene::new(Camera::new(Viewport::GAME, target, seed), OPTIONS, 2);
    s.set_shades(lit.then(|| ShadeRules { floor: 50, ..Default::default() }));
    s
}

/// What a camera's view and its centre show of a frame.
#[derive(Default)]
struct Seen {
    shook: [bool; 2],
    apart: bool,
    /// View 1 while its worm is dead and the shake is over.
    dead: Vec<View>,
    /// View 1 on the first frame its worm lives again after a death.
    respawned: Option<(View, Vec2)>,
}

/// Plays oracle scenario `name` through a scene of two views (worm 0's and
/// worm 1's) beside two scenes of one view each, checking every frame
/// that each view draws what its single-view scene draws.
fn two_views(name: &str, lit: bool) -> Option<Seen> {
    let content = common::content()?;
    let (sc, trace) = olr_scenario::load_fixture(&common::root().join("fixtures/oracle"), name).unwrap();
    let m = content.load_mod(&olx_mod_id(&sc.mod_dir)).unwrap();
    let mut assets = SceneAssets::new(content.core(), &m);
    let skin = content.skin_or_default("olx-skin-default");
    for i in 0..sc.worms.len() {
        assets.add_worm(&skin, olr_data::image::Rgba::rgb(255, 0, 0), true, i == 0);
    }
    let mut both = scene(0, 1, lit);
    assert_eq!(both.add_view(Camera::new(Viewport::GAME, 1, 9)), Some(1));
    let (mut solo0, mut solo1) = (scene(0, 1, lit), scene(1, 9, lit));
    let mut lists = [DrawList::new(), DrawList::new()];
    let (mut l0, mut l1) = (DrawList::new(), DrawList::new());
    let mut seen = Seen::default();
    let mut was_dead = false;
    olr_scenario::play(&content, &sc, &trace, |world, frame, events| {
        for s in [&mut both, &mut solo0, &mut solo1] {
            s.after_tick(world, events);
        }
        both.build_views(world, 0.5, &assets, &mut lists);
        solo0.build(world, 0.5, &assets, &mut l0);
        solo1.build(world, 0.5, &assets, &mut l1);
        for (v, (list, solo)) in lists.iter().zip([&l0, &l1]).enumerate() {
            assert_eq!(list.describe(&assets), solo.describe(&assets), "{name} frame {frame}: view {v}");
            assert!(list == solo, "{name} frame {frame}: view {v}'s list is the single view's");
        }
        assert_eq!(lists[0].occluders, lists[1].occluders, "every view holds every occluder");
        assert_eq!(lit, !lists[0].occluders.is_empty() || world.worms().iter().all(|w| !w.alive));
        // The look records are one, the first view's (they are camera-free).
        assert_eq!(both.drawn_projectiles(), solo0.drawn_projectiles());
        assert_eq!(both.drawn_worms(), solo0.drawn_worms());
        assert_eq!(both.ended_projectiles(), solo0.ended_projectiles());
        assert_eq!(both.shots(), solo0.shots());
        for s in [&mut both, &mut solo0, &mut solo1] {
            s.clear_look_events();
        }
        let c = both.cameras();
        for (v, c) in c.iter().enumerate() {
            seen.shook[v] |= c.shake_amount() > 0;
        }
        seen.apart |= c[0].view() != c[1].view();
        if let Some(w) = world.worms().get(1) {
            if !w.alive && c[1].shake_amount() == 0 {
                seen.dead.push(c[1].view());
            }
            if w.alive && was_dead && seen.respawned.is_none() {
                seen.respawned = Some((c[1].view(), w.pos));
            }
            was_dead |= !w.alive;
        }
    })
    .unwrap();
    Some(seen)
}

#[test]
fn two_cameras_follow_two_worms_through_shake_death_and_respawn() {
    for lit in [false, true] {
        let Some(seen) = two_views("explode-kill-respawn", lit) else { return };
        assert_eq!(seen.shook, [true, true], "the explosion is in both views: both shake");
        assert!(seen.apart, "the cameras follow their own worms");
        let still = seen.dead[0];
        assert!(seen.dead.len() > 100, "worm 1 lies dead for most of the respawn time");
        assert!(seen.dead.iter().all(|v| *v == still), "a dead worm's view stays where it died");
        let (view, at) = seen.respawned.expect("worm 1 respawns");
        assert_ne!(view, still, "the view jumps to the respawned worm");
        assert!(view.contains(at.x, at.y), "the respawned worm is in its view");
    }
}

/// The laser sight's dots come from one generator a frame: every view
/// draws the same dots, as a single view does.
#[test]
fn views_draw_the_laser_sights_dots_alike() {
    for lit in [false, true] {
        if two_views("weapons/classic/rifle", lit).is_none() {
            return;
        }
    }
}

#[test]
fn a_scene_holds_one_to_four_views() {
    let mut s = Scene::new(Camera::new(Viewport::GAME, 0, 1), OPTIONS, 2);
    assert_eq!(s.views(), 1);
    for v in 1..MAX_VIEWS {
        assert_eq!(s.add_view(Camera::new(Viewport::GAME, v as i32, 1)), Some(v));
    }
    assert_eq!(s.add_view(Camera::new(Viewport::GAME, 9, 1)), None, "four at most");
    let targets: Vec<i32> = s.cameras().iter().map(Camera::target).collect();
    assert_eq!(targets, [0, 1, 2, 3]);
    assert_eq!(s.listeners().len(), MAX_VIEWS);
    assert_eq!(&s.listeners()[..], s.cameras().iter().map(Camera::listener).collect::<Vec<_>>());
    s.truncate_views(0);
    assert_eq!(s.views(), 1, "never fewer than one");
    assert_eq!(s.camera().target(), 0);
    assert_eq!(s.listeners().len(), 1);
}
