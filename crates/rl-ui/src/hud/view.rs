//! What the HUD shows this frame: plain data the app fills from the
//! world (and tests by hand).

use egui::{Color32, Pos2, Rect, TextureId, Vec2};
use rl_sim::World;

use super::model::{Names, Row};

/// OLX's `WRM_OUT`: no lives left.
const OUT: i32 = -1;

/// OLX's weapon bar shows a cooldown only for a rate of fire above this
/// (`CClient_Draw.cpp:1221-1257`).
const COOLDOWN_ROF: f32 = 0.2;

/// The weapon bar's state, OLX's three (`CClient_Draw.cpp:1221-1257`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bar {
    /// Ready to fire: `(64, 64, 255)`.
    Ready,
    /// Reloading: `(128, 64, 64)`.
    Reloading,
    /// Fired and cooling down (`LastFire > 0`, `ROF > 0.2`): `(255, 161, 66)`.
    Cooldown,
}

/// One weapon slot.
#[derive(Clone, Debug, PartialEq)]
pub struct SlotHud {
    pub name: String,
    /// 0 to 1 (`Charge`).
    pub charge: f32,
    pub bar: Bar,
}

/// The worm the HUD follows (the player's; a replay's first).
#[derive(Clone, Debug, PartialEq)]
pub struct WormHud {
    pub name: String,
    /// 0 to 100.
    pub health: f32,
    pub alive: bool,
    /// OLX's lives: -2 unlimited, -1 out, else the number.
    pub lives: i32,
    pub kills: i32,
    pub deaths: i32,
    pub suicides: i32,
    pub slots: Vec<SlotHud>,
    /// The slot in hand.
    pub current: usize,
    /// While dead: what the player can do ("Jump to respawn"), or that it
    /// is out of the game.
    pub status: Option<String>,
}

impl WormHud {
    /// Worm `i` of `world`, named from `names`; `can_respawn` says whether
    /// a dead worm's player may press Jump (not in a replay).
    pub fn of(world: &World, i: usize, names: &Names, can_respawn: bool) -> Option<WormHud> {
        let w = world.worms().get(i)?;
        let m = world.sim_mod();
        let slots = w
            .slots
            .iter()
            .map(|s| {
                let weapon = m.weapon(s.weapon);
                let bar = if s.reloading {
                    Bar::Reloading
                } else if s.last_fire > 0.0 && weapon.is_some_and(|w| w.rof > COOLDOWN_ROF) {
                    Bar::Cooldown
                } else {
                    Bar::Ready
                };
                // OLX strips the dots from the weapon label.
                let name = weapon.map_or_else(|| "-".to_string(), |w| w.name.replace('.', ""));
                SlotHud { name, charge: s.charge.clamp(0.0, 1.0), bar }
            })
            .collect();
        let status = if w.alive {
            None
        } else if w.lives == OUT {
            Some("Out of the game".to_string())
        } else if can_respawn {
            Some("Dead: Jump to respawn".to_string())
        } else {
            Some("Dead".to_string())
        };
        Some(WormHud {
            name: names.of(w.id),
            health: w.health.clamp(0.0, 100.0),
            alive: w.alive,
            lives: w.lives,
            kills: w.kills,
            deaths: w.deaths,
            suicides: w.suicides,
            slots,
            current: usize::try_from(w.current_weapon).unwrap_or(0),
            status,
        })
    }
}

/// A worm on the minimap (`drawOnMiniMap`, `CMap.cpp:1708-1770`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dot {
    /// In world pixels.
    pub pos: Pos2,
    /// The worm's game colour.
    pub colour: Color32,
    /// The local human's: 3 × 3 minimap pixels, others 2 × 2.
    pub big: bool,
}

/// The minimap: the level's colour picture as an egui texture the app
/// keeps up to date ([`super::minimap::image`]), the worms and the view.
#[derive(Clone, Debug, PartialEq)]
pub struct MinimapView {
    pub texture: TextureId,
    /// The level's size in world pixels.
    pub map: Vec2,
    /// The camera's view in world pixels (drawn as a rectangle).
    pub view: Option<Rect>,
    /// The alive worms.
    pub dots: Vec<Dot>,
    /// OLX's blink sawtooth, 0 to 0.5 s.
    pub blink: f32,
}

/// A line under the clock (a replay's progress, a divergence).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub text: String,
    /// Red: something is wrong.
    pub warn: bool,
}

/// Everything the HUD shows this frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HudView {
    pub worm: Option<WormHud>,
    /// The kill feed's visible lines, oldest first.
    pub feed: Vec<String>,
    /// The time-limit clock, if the match has a limit.
    pub time_left_ms: Option<u64>,
    /// The kill limit, if on.
    pub kill_limit: Option<i32>,
    pub notices: Vec<Notice>,
    /// Tab held: the rows of the scoreboard.
    pub scoreboard: Option<Vec<Row>>,
    pub minimap: Option<MinimapView>,
    /// The match ended and its results do not show yet.
    pub game_over: bool,
    /// The keys, bottom right, where there is room.
    pub hint: Option<String>,
    /// Space left at the top-left for another panel (the F2 counters), in
    /// points: the feed starts below it.
    pub top_left_reserved: f32,
}
