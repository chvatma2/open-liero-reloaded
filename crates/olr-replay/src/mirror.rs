//! The format's own copies of the types a replay records (postcard, which
//! is not self-describing: field order is the format). Every conversion
//! names every field, so a field added to `SimSettings`, `WormSetup`,
//! `Loadout`, `Slot` or `DeathmatchSettings` does not compile until it is
//! recorded here and [`crate::FORMAT_VERSION`] is bumped.

use alloc::string::String;
use alloc::vec::Vec;

use olr_modes::{ChaosCycle, DeathmatchSettings, RespawnLoadout};
use olr_sim::{BonusItem, Loadout, SimSettings, Slot, WormSetup};
use serde::{Deserialize, Serialize};

use crate::{ModeSpec, PackRef, Packs, SetupCall, WormLook};

/// A copy of a struct whose fields are all `Copy`, converted both ways.
macro_rules! mirror {
    ($wire:ident <-> $ty:path { $($f:ident: $t:ty),* $(,)? }) => {
        #[derive(Serialize, Deserialize)]
        pub(crate) struct $wire {
            $($f: $t),*
        }
        impl From<&$ty> for $wire {
            fn from(v: &$ty) -> Self {
                let $ty { $($f),* } = v;
                $wire { $($f: *$f),* }
            }
        }
        impl From<$wire> for $ty {
            fn from(v: $wire) -> Self {
                let $wire { $($f),* } = v;
                $ty { $($f),* }
            }
        }
    };
}

mirror!(SettingsW <-> SimSettings {
    lx56_physics_fps: i32,
    game_speed: f32,
    game_speed_only_for_projs: bool,
    worm_ground_speed: f32,
    worm_air_speed: f32,
    worm_max_ground_move_speed: f32,
    worm_max_air_move_speed: f32,
    worm_jump_force: f32,
    worm_gravity: f32,
    worm_air_friction: f32,
    worm_ground_friction: f32,
    worm_ground_stop_speed: f32,
    worm_speed_factor: f32,
    relative_air_jump: bool,
    relative_air_jump_delay: f32,
    jump_to_aim_dir: bool,
    full_aim_angle: bool,
    rope_max_length: i32,
    rope_rest_length: i32,
    rope_strength: f32,
    rope_speed: f32,
    rope_gravity: f32,
    rope_falling_gravity: f32,
    rope_add_parent_speed: bool,
    rope_can_attach_worm: bool,
    ninjarope_precision: f32,
    weapon_combos: bool,
    instant_air_jump: bool,
    loading_time: i32,
    proj_relative_vel: bool,
    shoot_spawn_distance: f32,
    lx56_wall_shooting: bool,
    disable_wpns_when_empty: bool,
    proj_gravity_factor: f32,
    self_hit: bool,
    self_injure: bool,
    worm_damage_factor: f32,
    worm_shield_factor: f32,
    lives: i32,
    kill_limit: i32,
    respawn_time: f32,
    max_respawn_time: f32,
    empty_weapons_on_respawn: bool,
    allow_negative_score: bool,
    death_decreases_score: f32,
    suicide_decreases_score: bool,
    bonuses: bool,
    bonus_frequency: f32,
    bonus_life: f32,
    bonus_health_to_weapon_chance: f32,
    olx_quirks: bool,
});

mirror!(SlotW <-> Slot { weapon: i32, charge: f32, reloading: bool, last_fire: f32 });

#[derive(Serialize, Deserialize)]
pub(crate) struct WormW {
    team: u8,
    pub(crate) slots: Vec<SlotW>,
    current_weapon: i32,
    lives: i32,
    kills: i32,
    deaths: i32,
    spawn: Option<(i32, i32)>,
}

impl From<&WormSetup> for WormW {
    fn from(w: &WormSetup) -> Self {
        let WormSetup { team, loadout, spawn } = w;
        let Loadout { slots, current_weapon, lives, kills, deaths } = loadout;
        WormW {
            team: *team,
            slots: slots.iter().map(SlotW::from).collect(),
            current_weapon: *current_weapon,
            lives: *lives,
            kills: *kills,
            deaths: *deaths,
            spawn: *spawn,
        }
    }
}

impl From<WormW> for WormSetup {
    fn from(w: WormW) -> Self {
        let WormW { team, slots, current_weapon, lives, kills, deaths, spawn } = w;
        let slots = slots.into_iter().map(Slot::from).collect();
        WormSetup { team, loadout: Loadout { slots, current_weapon, lives, kills, deaths }, spawn }
    }
}

#[derive(Serialize, Deserialize)]
pub(crate) struct PackRefW {
    id: String,
    hash: String,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct PacksW {
    mod_pack: PackRefW,
    level: PackRefW,
    theme: PackRefW,
}

impl From<&Packs> for PacksW {
    fn from(p: &Packs) -> Self {
        let Packs { mod_pack, level, theme } = p;
        let r = |r: &PackRef| PackRefW { id: r.id.clone(), hash: r.hash.clone() };
        PacksW { mod_pack: r(mod_pack), level: r(level), theme: r(theme) }
    }
}

impl From<PacksW> for Packs {
    fn from(p: PacksW) -> Self {
        let r = |r: PackRefW| PackRef { id: r.id, hash: r.hash };
        Packs { mod_pack: r(p.mod_pack), level: r(p.level), theme: r(p.theme) }
    }
}

#[derive(Clone, Copy, Serialize, Deserialize)]
pub(crate) enum CallW {
    SetHealth { worm: u32, health: f32 },
    PlaceHealth { x: i32, y: i32 },
    PlaceWeapon { x: i32, y: i32, weapon: i32 },
    FindSpot,
    AlignPhysicsClock,
}

impl From<SetupCall> for CallW {
    fn from(c: SetupCall) -> Self {
        match c {
            SetupCall::SetHealth { worm, health } => CallW::SetHealth { worm, health },
            SetupCall::PlaceBonus { x, y, item: BonusItem::Health } => CallW::PlaceHealth { x, y },
            SetupCall::PlaceBonus { x, y, item: BonusItem::Weapon(weapon) } => {
                CallW::PlaceWeapon { x, y, weapon }
            }
            SetupCall::FindSpot => CallW::FindSpot,
            SetupCall::AlignPhysicsClock => CallW::AlignPhysicsClock,
        }
    }
}

impl From<CallW> for SetupCall {
    fn from(c: CallW) -> Self {
        match c {
            CallW::SetHealth { worm, health } => SetupCall::SetHealth { worm, health },
            CallW::PlaceHealth { x, y } => SetupCall::PlaceBonus { x, y, item: BonusItem::Health },
            CallW::PlaceWeapon { x, y, weapon } => {
                SetupCall::PlaceBonus { x, y, item: BonusItem::Weapon(weapon) }
            }
            CallW::FindSpot => SetupCall::FindSpot,
            CallW::AlignPhysicsClock => SetupCall::AlignPhysicsClock,
        }
    }
}

/// [`RespawnLoadout`]; a chaos rotation by what it was made from.
#[derive(Serialize, Deserialize)]
pub(crate) enum RespawnW {
    Keep,
    ChaosCycle { worms: Vec<u8>, weapons: u32 },
}

#[derive(Serialize, Deserialize)]
pub(crate) struct DeathmatchW {
    lives: i32,
    kill_limit: i32,
    pub(crate) time_limit: f32,
    allow_negative_score: bool,
    death_decreases_score: f32,
    suicide_decreases_score: bool,
    sprees: [i32; 5],
    dying_sprees: [i32; 5],
    pub(crate) respawn_loadout: RespawnW,
}

#[derive(Serialize, Deserialize)]
pub(crate) enum ModeW {
    Oracle,
    Deathmatch(DeathmatchW),
}

impl From<&ModeSpec> for ModeW {
    fn from(m: &ModeSpec) -> Self {
        let ModeSpec::Deathmatch(s) = m else { return ModeW::Oracle };
        let DeathmatchSettings {
            lives,
            kill_limit,
            time_limit,
            allow_negative_score,
            death_decreases_score,
            suicide_decreases_score,
            sprees,
            dying_sprees,
            respawn_loadout,
        } = s;
        let respawn_loadout = match respawn_loadout {
            RespawnLoadout::Keep => RespawnW::Keep,
            // Worm ids are below `MAX_WORMS` (32) and weapon counts fit a
            // `u32`: `ChaosCycle` keeps only those.
            RespawnLoadout::ChaosCycle(c) => RespawnW::ChaosCycle {
                worms: c.worms().filter_map(|w| u8::try_from(w).ok()).collect(),
                weapons: u32::try_from(c.weapon_count()).unwrap_or(u32::MAX),
            },
        };
        ModeW::Deathmatch(DeathmatchW {
            lives: *lives,
            kill_limit: *kill_limit,
            time_limit: *time_limit,
            allow_negative_score: *allow_negative_score,
            death_decreases_score: *death_decreases_score,
            suicide_decreases_score: *suicide_decreases_score,
            sprees: *sprees,
            dying_sprees: *dying_sprees,
            respawn_loadout,
        })
    }
}

impl From<ModeW> for ModeSpec {
    fn from(m: ModeW) -> Self {
        let ModeW::Deathmatch(d) = m else { return ModeSpec::Oracle };
        let DeathmatchW {
            lives,
            kill_limit,
            time_limit,
            allow_negative_score,
            death_decreases_score,
            suicide_decreases_score,
            sprees,
            dying_sprees,
            respawn_loadout,
        } = d;
        let respawn_loadout = match respawn_loadout {
            RespawnW::Keep => RespawnLoadout::Keep,
            RespawnW::ChaosCycle { worms, weapons } => {
                let worms: Vec<usize> = worms.into_iter().map(usize::from).collect();
                RespawnLoadout::ChaosCycle(ChaosCycle::new(&worms, weapons as usize))
            }
        };
        ModeSpec::Deathmatch(DeathmatchSettings {
            lives,
            kill_limit,
            time_limit,
            allow_negative_score,
            death_decreases_score,
            suicide_decreases_score,
            sprees,
            dying_sprees,
            respawn_loadout,
        })
    }
}

/// A look on the wire: the skin's content hash as its 32 raw bytes
/// (`NO_SKIN_HASH` is all zeros), hex only at the edges.
#[derive(Serialize, Deserialize)]
pub(crate) struct LookW {
    pub(crate) name: String,
    colour: [u8; 3],
    pub(crate) skin: String,
    skin_hash: [u8; 32],
}

/// The 32 bytes of 64 lowercase hex digits; zeros for anything else (a
/// recorder sanitises, so only a hand-made replay gets here).
fn hash_bytes(hex: &str) -> [u8; 32] {
    let nibble = |b: u8| match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        _ => None,
    };
    let mut out = [0u8; 32];
    if hex.len() == 64 {
        for (o, pair) in out.iter_mut().zip(hex.as_bytes().chunks_exact(2)) {
            match (nibble(pair[0]), nibble(pair[1])) {
                (Some(h), Some(l)) => *o = h << 4 | l,
                _ => return [0; 32],
            }
        }
    }
    out
}

fn hash_hex(bytes: &[u8; 32]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|&b| [DIGITS[usize::from(b >> 4)], DIGITS[usize::from(b & 15)]])
        .map(char::from)
        .collect()
}

impl From<&WormLook> for LookW {
    fn from(l: &WormLook) -> Self {
        let WormLook { name, colour, skin, skin_hash } = l;
        LookW { name: name.clone(), colour: *colour, skin: skin.clone(), skin_hash: hash_bytes(skin_hash) }
    }
}

impl LookW {
    /// Worm `i`'s look: the name sanitised, the skin as recorded (the
    /// caller checked its id).
    pub(crate) fn into_look(self, i: usize) -> WormLook {
        let LookW { name, colour, skin, skin_hash } = self;
        WormLook { name: crate::sanitise_name(&name, i), colour, skin, skin_hash: hash_hex(&skin_hash) }
    }
}
