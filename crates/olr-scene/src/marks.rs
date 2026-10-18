//! What the last tick left for drawing besides the world's state: muzzle
//! flashes, green crosshairs, laser sights and beams. The sim reports them
//! as events; OLX keeps them as worm flags and short-lived entities.

use olr_data::level::Material;
use olr_math::{GlibcRand, Vec2};
use olr_sim::{CosmeticKind, EventKind, MAX_WORMS, TickEvents, WeaponType, World};

use crate::assets::SceneAssets;
use crate::draw_list::{DrawList, Item, Layer, Line, Shade, View, perform_line};

/// A laser sight entity alive at the end of the tick.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Laser {
    /// The worm id it belongs to.
    worm: i32,
    start: Vec2,
    dir: Vec2,
    len: i16,
}

/// The tick's marks and the laser sights' own generator.
#[derive(Clone, Debug)]
pub struct Marks {
    muzzle: [bool; MAX_WORMS],
    target: [bool; MAX_WORMS],
    lasers: Vec<Laser>,
    beams: [Option<Line>; MAX_WORMS],
    rng: GlibcRand,
    /// `LaserSightPutPixel`'s `static int laseralt`.
    laser_alt: i32,
}

impl Marks {
    pub fn new(seed: u32) -> Marks {
        Marks {
            muzzle: [false; MAX_WORMS],
            target: [false; MAX_WORMS],
            lasers: Vec::with_capacity(MAX_WORMS),
            beams: [None; MAX_WORMS],
            rng: GlibcRand::new(seed),
            laser_alt: 0,
        }
    }

    /// Forgets the marks (after a restart).
    pub fn clear(&mut self) {
        self.muzzle = [false; MAX_WORMS];
        self.target = [false; MAX_WORMS];
        self.lasers.clear();
        self.beams = [None; MAX_WORMS];
    }

    /// Worm id `worm` fired a projectile weapon in the last tick (the flash
    /// shows for that tick; OLX shows it for one drawn frame, about one tick).
    pub fn muzzle(&self, worm: i32) -> bool {
        usize::try_from(worm).ok().and_then(|i| self.muzzle.get(i)).copied().unwrap_or(false)
    }

    /// Worm id `worm`'s laser sight found another worm in the last tick.
    pub fn target(&self, worm: i32) -> bool {
        usize::try_from(worm).ok().and_then(|i| self.target.get(i)).copied().unwrap_or(false)
    }

    /// Reads a tick's events (call after every [`World::tick`]).
    ///
    /// - **Muzzle** (`ProcessShot`, `CClient_Game.cpp:888-917`): projectile
    ///   weapons only; a respawn later in the tick clears it (`CWorm::Spawn`).
    /// - **Laser sights and beams** live two entity passes
    ///   (`Entity.cpp:47-51`, `:449-452`): one spawned in the worm loop of
    ///   the tick's last sub-step is drawn, older ones are gone. A tick that
    ///   ran no sub-step keeps the last tick's.
    /// - **Green crosshair** (`CClient::LaserSight`, `:778-793`): a trace
    ///   this tick stopped at a worm. The sim reports where it stopped
    ///   (`len`); it stopped at a worm when the pixel there lets particles
    ///   through and the trace was not cut at 9999.
    pub fn observe(&mut self, world: &World, events: &TickEvents) {
        self.muzzle = [false; MAX_WORMS];
        self.target = [false; MAX_WORMS];
        let substeps = world.substeps_last_tick();
        if substeps > 0 {
            self.lasers.clear();
            self.beams = [None; MAX_WORMS];
        }
        let m = world.sim_mod();
        for e in events {
            match e.kind {
                EventKind::WeaponFired { worm, weapon } => {
                    if m.weapon(weapon).is_some_and(|w| w.kind == WeaponType::Projectile)
                        && let Some(f) = usize::try_from(worm).ok().and_then(|i| self.muzzle.get_mut(i))
                    {
                        *f = true;
                    }
                }
                EventKind::Spawned { worm } => {
                    if let Some(f) = usize::try_from(worm).ok().and_then(|i| self.muzzle.get_mut(i)) {
                        *f = false;
                    }
                }
                EventKind::SpawnCosmetic { kind: CosmeticKind::LaserSight { worm, len }, pos, vel } => {
                    if stopped_at_worm(world, pos, vel, len)
                        && let Some(f) = usize::try_from(worm).ok().and_then(|i| self.target.get_mut(i))
                    {
                        *f = true;
                    }
                    if substeps > 0 && e.substep + 1 == substeps && self.lasers.len() < MAX_WORMS {
                        self.lasers.push(Laser { worm, start: pos, dir: vel, len });
                    }
                }
                EventKind::SpawnCosmetic { kind: CosmeticKind::Beam { worm, colour }, pos, vel } => {
                    let slot = usize::try_from(worm).ok().filter(|&i| i < MAX_WORMS);
                    if let Some(i) = slot
                        && substeps > 0
                        && e.substep + 1 == substeps
                    {
                        let end = pos + vel;
                        let a = (pos.x as i32, pos.y as i32);
                        self.beams[i] = Some(Line {
                            a,
                            b: (end.x as i32, end.y as i32),
                            colour,
                            shade: Default::default(),
                        });
                    }
                }
                _ => {}
            }
        }
    }

    /// The laser sights, then the beams, into [`Layer::Beams`]. Each laser
    /// is OLX's dotted line: per screen pixel of `perform_line` from the
    /// worm to the trace's end, `alt = (alt + 1) % (GetRandomInt(35) + 1)`
    /// and a dot of a random laser colour where it is 0, snapped to world
    /// pixels. The dots change every drawn frame (their own generator).
    ///
    /// `drawn(id)` is worm id `id`'s drawn position when it is interpolated
    /// (`None` draws OLX's: where the worm was in the sub-step): a laser
    /// sight then starts there, keeping its direction and length, and a
    /// beam runs from there to where it stopped, so neither trails the worm.
    ///
    /// In a lit match (`lit`) the dots are markers and the beams glow.
    ///
    /// OLX: src/client/Entity.cpp:305-314, src/client/GfxPrimitives.cpp:1878-1901, :2507-2520
    pub fn draw(
        &mut self,
        assets: &SceneAssets,
        view: View,
        drawn: impl Fn(i32) -> Option<Vec2>,
        lit: bool,
        out: &mut DrawList,
    ) {
        let (dot, beam) = if lit { (Shade::MARKER, Shade::GLOW) } else { (Shade::LIT, Shade::LIT) };
        let Marks { lasers, rng, laser_alt, beams, .. } = self;
        for l in lasers.iter() {
            let start = drawn(l.worm).unwrap_or(l.start);
            let end = start + l.dir * l.len as f32;
            let screen = |v: f32| (v as i32).saturating_mul(2);
            let (a, b) = ((screen(start.x), screen(start.y)), (screen(end.x), screen(end.y)));
            perform_line(a, b, |x, y| {
                *laser_alt = (*laser_alt + 1) % (rng.random_int(35) + 1);
                if *laser_alt != 0 {
                    return;
                }
                let colour = assets.laser_colours[rng.random_int(1).clamp(0, 1) as usize];
                let (wx, wy) = (((x - x % 2) / 2) as f32, ((y - y % 2) / 2) as f32);
                if view.overlaps(wx, wy, 1.0, 1.0) {
                    out.pixel_shaded(Layer::Beams, assets.white, (wx, wy), colour, dot);
                }
            });
        }
        for (id, line) in beams.iter().enumerate() {
            let Some(mut line) = *line else { continue };
            if let Some(p) = i32::try_from(id).ok().and_then(&drawn) {
                line.a = (p.x as i32, p.y as i32);
            }
            line.shade = beam;
            out.push(Layer::Beams, Item::Line(line));
        }
    }
}

/// Redoes the end of the sim's 3-pixel trace (`len / 3` steps from `pos`)
/// and reads the material there, wrapped like `getMaterialWrapped`.
fn stopped_at_worm(world: &World, pos: Vec2, dir: Vec2, len: i16) -> bool {
    if !(0..9999).contains(&len) {
        return false;
    }
    let mut p = pos;
    for _ in 0..len / 3 {
        p += dir * 3.0;
    }
    let t = world.terrain();
    let (tw, th) = (t.width().max(1) as i64, t.height().max(1) as i64);
    let (x, y) = ((p.x as i64).rem_euclid(tw), (p.y as i64).rem_euclid(th));
    t.material(x, y) == Some(Material::Background)
}
