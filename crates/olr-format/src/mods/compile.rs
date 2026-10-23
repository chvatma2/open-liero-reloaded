//! Authoring mod -> runtime `ModDef` (spec §2). Pure, deterministic, never
//! panics. Projectile keys follow `olr-import-lgs`'s naming, so lifting an
//! import and compiling it gives the import back (Task 6).

use std::collections::{BTreeMap, BTreeSet};

use olr_data::mod_def as rt;
use olr_data::mod_def::{ChildLink, child_key};

#[cfg(test)]
use super::spawn::{EventSpawn, ShotSpawn, TrailSpawn};
use super::spawn::{Spawn, SpawnContext};
use super::types::*;
use super::{ModSource, Rope, Worm};
use crate::FormatError;
use olr_data::loadouts::{DEFAULT_LOADOUT, LOADING_TIME_RANGE, clean_name};

pub const MAX_INLINE_DEPTH: usize = 32;
pub const MAX_WEAPONS: usize = 512;
pub const MAX_SHARED: usize = 4096;
pub const MAX_EFFECTS: usize = 16;
pub const MAX_COLOURS: usize = 256;
/// The most children one spawn makes at once (`olr-sim`'s `MAX_AMOUNT`).
pub const MAX_AMOUNT: i32 = 1000;
/// The most shipped loadouts a mod lists.
pub const MAX_LOADOUTS: usize = 64;

pub(crate) const MOD_FILE: &str = "mod.ron";

pub(crate) fn weapon_file(id: &str) -> String {
    format!("weapons/{id}.ron")
}

pub(crate) fn shared_file(id: &str) -> String {
    format!("projectiles/{id}.ron")
}

/// Pack ids name files: lowercase ASCII, digits, `-`, `_`, `.`; not starting with `.`.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && !id.starts_with('.')
        && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_' | b'.'))
}

fn at(path: &str, seg: &str) -> String {
    if path.is_empty() { seg.to_string() } else { format!("{path}.{seg}") }
}

struct Compiler<'a> {
    src: &'a ModSource,
    file: String,
    taken: BTreeSet<String>,
    out: BTreeMap<String, rt::ProjectileDef>,
    looks: BTreeMap<String, olr_look::ProjectileLook>,
    /// Every error found, in the order the checks run: the walk goes on
    /// after one, so the first is the one a stop at the first would give.
    errors: Vec<FormatError>,
}

/// A mod's look by authoring key: each projectile's by its `ModDef` key,
/// each weapon's in order (empty when no weapon has one; read past its end
/// as none), and `mod.ron`'s default explosion light (issue #15) and heat
/// (issue #42). `olr-content` indexes it like `SimMod`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModLookSource {
    pub projectiles: BTreeMap<String, olr_look::ProjectileLook>,
    pub weapons: Vec<Option<olr_look::WeaponLook>>,
    pub explosion: Option<olr_look::ExplosionLight>,
    pub explosion_heat: Option<olr_look::ExplosionHeatLook>,
}

/// The shipped loadouts, each naming a preset of `restrictions.ron` (not
/// `Default`, which every mod has), its name usable as a loadout's
/// (`olr_data::loadouts::clean_name` leaves it as it is) and its loading
/// time within `LOADING_TIME_RANGE`. Every problem is pushed to `errors`.
fn check_loadouts(src: &ModSource, errors: &mut Vec<FormatError>) {
    if src.loadouts.len() > MAX_LOADOUTS {
        errors.push(FormatError::invalid(MOD_FILE, "loadouts", format!("more than {MAX_LOADOUTS} loadouts")));
        return;
    }
    for (name, l) in &src.loadouts {
        let bad = |why: String| FormatError::invalid(MOD_FILE, &format!("loadouts[{name:?}]"), why);
        if !src.restrictions.contains_key(name) {
            errors.push(bad("names no preset of restrictions.ron".into()));
        } else if name == DEFAULT_LOADOUT || clean_name(name) != *name {
            errors.push(bad("is not a usable loadout name".into()));
        } else if !LOADING_TIME_RANGE.contains(&l.loading_time) {
            errors.push(bad(format!("loading_time {} is not within {LOADING_TIME_RANGE:?}", l.loading_time)));
        }
    }
}

pub fn compile_mod(src: &ModSource) -> Result<rt::ModDef, FormatError> {
    compile_all(src).map(|(def, _)| def)
}

/// The mod's looks (projectiles and weapons), checked as `compile_mod` checks them.
pub fn compile_mod_look(src: &ModSource) -> Result<ModLookSource, FormatError> {
    compile_all(src).map(|(_, look)| look)
}

/// Every error `compile_mod` could report, in the order it checks: the
/// first is `compile_mod`'s. Empty when the mod compiles (issue #16, the
/// mod editor's list of located errors).
pub fn mod_errors(src: &ModSource) -> Vec<FormatError> {
    walk(src).2
}

fn compile_all(src: &ModSource) -> Result<(rt::ModDef, ModLookSource), FormatError> {
    let (def, look, errors) = walk(src);
    match errors.into_iter().next() {
        Some(first) => Err(first),
        None => Ok((def, look)),
    }
}

/// The whole mod checked and compiled: the `ModDef` and look are complete
/// only when the error list is empty.
fn walk(src: &ModSource) -> (rt::ModDef, ModLookSource, Vec<FormatError>) {
    let mut errors = Vec::new();
    if src.weapons.len() > MAX_WEAPONS {
        errors.push(FormatError::invalid(MOD_FILE, "weapons", format!("more than {MAX_WEAPONS} weapons")));
    }
    if src.shared.len() > MAX_SHARED {
        errors.push(FormatError::invalid(
            "projectiles/",
            "",
            format!("more than {MAX_SHARED} shared projectiles"),
        ));
    }
    if let Err(e) = check_cycles(src) {
        errors.push(e);
    }
    let mut c = Compiler {
        src,
        file: String::new(),
        taken: BTreeSet::new(),
        out: BTreeMap::new(),
        looks: BTreeMap::new(),
        errors,
    };
    for (id, p) in &src.shared {
        c.file = shared_file(id);
        if p.id.is_some() {
            let e = c.err("id", "a shared projectile's id is its file name: remove `id`");
            c.errors.push(e);
        }
        c.projectile(p, id, "", 1);
    }
    let mut weapons = Vec::with_capacity(src.weapons.len());
    let mut weapon_looks = Vec::with_capacity(src.weapons.len());
    let mut ids = BTreeSet::new();
    for (id, w) in &src.weapons {
        c.file = weapon_file(id);
        if !valid_id(id) {
            c.errors.push(FormatError::invalid(MOD_FILE, "weapons", format!("invalid weapon id {id:?}")));
            continue;
        }
        if !ids.insert(id.as_str()) {
            c.errors.push(FormatError::invalid(
                MOD_FILE,
                "weapons",
                format!("weapon id {id:?} is listed twice"),
            ));
            continue;
        }
        weapons.push(c.weapon(id, w));
        let look = w.look.as_ref().map(|l| crate::look::compile_weapon_look(l, &c.file, "look"));
        weapon_looks.push(look.and_then(|l| c.note(l)));
    }
    check_loadouts(src, &mut c.errors);
    let def = rt::ModDef {
        name: src.name.clone(),
        worm: worm(&src.worm),
        rope: rope(&src.rope),
        weapons,
        projectiles: c.out,
        restrictions: src.restrictions.clone(),
        loadouts: src.loadouts.clone(),
    };
    // No weapon look at all is no list: a mod without looks is the empty look.
    if weapon_looks.iter().all(Option::is_none) {
        weapon_looks.clear();
    }
    let explosion = src
        .look
        .as_ref()
        .and_then(|l| l.explosion.as_ref())
        .map(|x| crate::look::compile_explosion_light(x, MOD_FILE, "look.explosion"));
    let explosion = noted(&mut c.errors, explosion);
    let explosion_heat = noted(
        &mut c.errors,
        src.look
            .as_ref()
            .and_then(|l| l.explosion_heat.as_ref())
            .map(|x| crate::look::compile_explosion_heat(x, MOD_FILE, "look.explosion_heat")),
    );
    let look = ModLookSource { projectiles: c.looks, weapons: weapon_looks, explosion, explosion_heat };
    (def, look, c.errors)
}

/// A value compiled, if there was one: an error is pushed to `errors`.
fn noted<T>(errors: &mut Vec<FormatError>, r: Option<Result<T, FormatError>>) -> Option<T> {
    r?.map_err(|e| errors.push(e)).ok()
}

fn worm(w: &Worm) -> rt::WormParams {
    rt::WormParams {
        angle_speed: w.unused_angle_speed,
        ground_speed: w.ground_acceleration,
        air_speed: w.air_acceleration,
        gravity: w.gravity,
        jump_force: w.jump_speed,
        air_friction: w.air_drag,
        ground_friction: w.unused_ground_friction,
    }
}

fn rope(r: &Rope) -> rt::RopeParams {
    rt::RopeParams { max_length: r.max_length, rest_length: r.rest_length, strength: r.strength }
}

impl Compiler<'_> {
    fn err(&self, path: &str, message: impl Into<String>) -> FormatError {
        FormatError::invalid(&self.file, path, message)
    }

    /// Records the error at `path`.
    fn fail(&mut self, path: &str, message: impl Into<String>) {
        let e = self.err(path, message);
        self.errors.push(e);
    }

    /// `r`'s value, or `None` with its error recorded.
    fn note<T>(&mut self, r: Result<T, FormatError>) -> Option<T> {
        r.map_err(|e| self.errors.push(e)).ok()
    }

    fn weapon(&mut self, id: &str, w: &Weapon) -> rt::WeaponDef {
        let kind = match &w.fire {
            Fire::Projectile { class, recoil, sound, shot } => rt::WeaponKind::Projectile {
                class: weapon_class(*class),
                recoil: *recoil,
                sound: sound.clone(),
                spawn: self.spawn(shot, id, "fire.shot", 0),
            },
            Fire::Beam {
                recoil,
                colour,
                hole_size,
                worm_damage,
                length,
                width,
                width_growth,
                split_damage_over_width,
            } => rt::WeaponKind::Beam {
                recoil: *recoil,
                beam: rt::BeamDef {
                    colour: colour.0,
                    damage: *hole_size,
                    player_damage: *worm_damage,
                    length: *length,
                    init_width: *width,
                    width_increase: *width_growth,
                    distribute_damage_over_width: *split_damage_over_width,
                },
            },
            Fire::Special { kind, thrust } => rt::WeaponKind::Special {
                special: match kind {
                    SpecialKind::Off => rt::SpecialKind::None,
                    SpecialKind::Jetpack => rt::SpecialKind::Jetpack,
                    SpecialKind::Unknown(i) => rt::SpecialKind::Other(*i),
                },
                thrust: *thrust,
            },
        };
        rt::WeaponDef {
            id: id.to_string(),
            name: w.name.clone(),
            recharge: w.recharge,
            drain: w.drain,
            rof: w.between_shots_s,
            laser_sight: w.laser_sight,
            kind,
        }
    }

    /// The spawn's runtime form; its projectile key is empty when it names
    /// none (an error recorded).
    fn spawn<C: SpawnContext>(
        &mut self,
        s: &Spawn<C>,
        default_key: &str,
        path: &str,
        depth: usize,
    ) -> rt::SpawnDef {
        if s.amount > MAX_AMOUNT {
            self.fail(
                &at(path, "amount"),
                format!("amount {} is above {MAX_AMOUNT}, the most one spawn makes", s.amount),
            );
        }
        let projectile = match (&s.shared, &s.projectile) {
            (Some(id), None) => {
                if !self.src.shared.contains_key(id) {
                    self.fail(
                        &at(path, "shared"),
                        format!("no shared projectile {id:?} ({})", shared_file(id)),
                    );
                }
                id.clone()
            }
            (None, Some(p)) => {
                let key = p.id.clone().unwrap_or_else(|| default_key.to_string());
                self.projectile(p, &key, &at(path, "projectile"), depth + 1);
                key
            }
            (Some(_), Some(_)) => {
                self.fail(path, "a spawn takes either `shared` or `projectile`, not both");
                String::new()
            }
            (None, None) => {
                self.fail(path, "a spawn needs `shared` or `projectile`");
                String::new()
            }
        };
        rt::SpawnDef {
            projectile,
            amount: s.amount,
            speed: s.speed,
            speed_var: s.speed_var,
            spread: s.spread,
            use_angle: s.relative_to_parent,
            angle: s.angle,
            use_parent_velocity_for_spread: s.parent_velocity_as_direction,
            parent_vel_spread_factor: s.parent_velocity_direction_scale,
            use_special11_vec_for_speed_var: s.speed_var_diagonal,
            use_random_rot: s.random_rotation,
            add_parent_vel: s.inherit_velocity,
            parent_vel_factor: s.inherit_velocity_factor,
        }
    }

    fn projectile(&mut self, p: &Projectile, key: &str, path: &str, depth: usize) {
        if depth > MAX_INLINE_DEPTH {
            self.fail(path, format!("projectiles nested deeper than {MAX_INLINE_DEPTH}"));
            return;
        }
        if !valid_id(key) {
            self.fail(&at(path, "id"), format!("invalid projectile id {key:?}"));
        } else if !self.taken.insert(key.to_string()) {
            self.fail(path, format!("projectile id {key:?} is used twice; give one an `id`"));
        }
        if let Some(look) = &p.look {
            let look = crate::look::compile_projectile_look(look, &self.file, &at(path, "look"));
            if let Some(look) = self.note(look) {
                self.looks.insert(key.to_string(), look);
            }
        }
        let timed = p.timer.as_ref().filter(|t| t.after_s > 0.0);
        if let Some(t) = &p.timer
            && (t.after_s.is_nan() || t.after_s <= 0.0)
            && !t.effects.is_empty()
        {
            self.fail(
                &at(path, "timer.effects"),
                "a timer's effects need after_s > 0 (OLX ignores them otherwise)",
            );
        }
        let (hit, hit_spawn) = self.effects(&p.hit_terrain, &at(path, "hit_terrain"));
        let (player_hit, worm_spawn) = self.effects(&p.hit_worm, &at(path, "hit_worm"));
        let timer_action = timed.map(|t| self.effects(&t.effects, &at(path, "timer.effects")));
        let named: BTreeSet<&str> = [
            hit_spawn.as_deref(),
            worm_spawn.as_deref(),
            timer_action.as_ref().and_then(|(_, s)| s.as_deref()),
        ]
        .into_iter()
        .flatten()
        .collect();
        if named.len() > 1 {
            let list: Vec<&str> = named.iter().copied().collect();
            self.fail(
                path,
                format!(
                    "this runtime gives a projectile's events one spawn, but they name {}",
                    list.join(", ")
                ),
            );
        }
        if p.spawns.len() > 1 {
            self.fail(&at(path, "spawns"), "this runtime takes at most one named spawn per projectile");
        }
        if named.len() == 1
            && let Some(name) = named.first()
            && !p.spawns.contains_key(*name)
        {
            let have: Vec<&str> = p.spawns.keys().map(String::as_str).collect();
            self.fail(path, format!("no spawn named {name:?} (spawns: {})", have.join(", ")));
        }
        let event_spawn = p.spawns.iter().next().map(|(name, s)| {
            self.spawn(s, &child_key(key, ChildLink::Spawn), &at(path, &format!("spawns[{name:?}]")), depth)
        });
        let trail = match &p.trail {
            Trail::Projectile { every_s, spawn } => rt::TrailDef {
                kind: rt::TrailKind::Projectile,
                delay: *every_s,
                spawn: Some(self.spawn(
                    spawn,
                    &child_key(key, ChildLink::Trail),
                    &at(path, "trail.spawn"),
                    depth,
                )),
            },
            other => rt::TrailDef {
                kind: match other {
                    Trail::Off => rt::TrailKind::None,
                    Trail::Smoke => rt::TrailKind::Smoke,
                    Trail::ChemSmoke => rt::TrailKind::ChemSmoke,
                    Trail::Doomsday => rt::TrailKind::Doomsday,
                    Trail::Explosive => rt::TrailKind::Explosive,
                    Trail::Unknown(i) => rt::TrailKind::Other(*i),
                    Trail::Projectile { .. } => unreachable!("matched above"),
                },
                delay: 0.1,
                spawn: None,
            },
        };
        let gfx = self.gfx(&p.gfx, &at(path, "gfx"));
        let (width, height) = p.size.unwrap_or(match gfx {
            rt::ProjectileGfx::Pixel { .. } => (2, 2),
            rt::ProjectileGfx::Image { .. } => (4, 4),
        });
        let timer = match &p.timer {
            Some(t) => rt::TimerDef {
                time: t.after_s,
                time_var: t.variation_s,
                action: timer_action.map(|(a, _)| a),
            },
            None => rt::TimerDef { time: 0.0, time_var: 0.0, action: None },
        };
        self.out.insert(
            key.to_string(),
            rt::ProjectileDef {
                gfx,
                gravity: p.gravity,
                dampening: p.velocity_factor,
                width,
                height,
                timer,
                trail,
                hit,
                player_hit,
                event_spawn,
            },
        );
    }

    fn gfx(&mut self, g: &Gfx, path: &str) -> rt::ProjectileGfx {
        match g {
            Gfx::Pixels(cs) => {
                if cs.len() > MAX_COLOURS {
                    self.fail(path, format!("more than {MAX_COLOURS} colours"));
                }
                rt::ProjectileGfx::Pixel { colours: cs.iter().map(|c| c.0).collect() }
            }
            Gfx::Image {
                file,
                rotating,
                rotation_step_deg,
                rotation_speed,
                frames_follow_heading,
                frames_follow_heading_mirrored,
                heading_frames,
                animation,
            } => rt::ProjectileGfx::Image {
                file: file.clone(),
                rotating: *rotating,
                rot_increment: *rotation_step_deg,
                rot_speed: *rotation_speed,
                use_angle: *frames_follow_heading,
                use_spec_angle: *frames_follow_heading_mirrored,
                angle_images: *heading_frames,
                animation: animation.as_ref().map(|a| rt::Animation {
                    rate: a.fps,
                    kind: match a.mode {
                        AnimMode::Once => rt::AnimKind::Once,
                        AnimMode::Loop => rt::AnimKind::Loop,
                        AnimMode::PingPong => rt::AnimKind::PingPong,
                        AnimMode::Unknown(i) => rt::AnimKind::Other(i),
                    },
                }),
            },
        }
    }

    /// One event's effect list as LX56's single action, and the spawn it
    /// names (its first `Spawn`).
    fn effects(&mut self, list: &[Effect], path: &str) -> (rt::ActionDef, Option<String>) {
        let mut action = rt::ActionDef::olx_default(rt::ActionKind::Nothing);
        if list.len() > MAX_EFFECTS {
            self.fail(path, format!("more than {MAX_EFFECTS} effects"));
            return (action, None);
        }
        let (mut kind_set, mut spawn) = (false, None::<String>);
        for (i, e) in list.iter().enumerate() {
            let here = format!("{path}[{i}]");
            if let Effect::Spawn(name) = e {
                if spawn.is_some() {
                    self.fail(&here, "more than one Spawn per event is not supported by this runtime");
                } else {
                    spawn = Some(name.clone());
                }
                continue;
            }
            if kind_set {
                self.fail(
                    &here,
                    "more than one effect per event besides Spawn is not supported by this runtime",
                );
                continue;
            }
            kind_set = true;
            apply(&mut action, e);
        }
        action.spawns_projectiles = spawn.is_some();
        (action, spawn)
    }
}

/// Sets `a.kind` and the fields that kind uses (`mod-fields.md` §12).
fn apply(a: &mut rt::ActionDef, e: &Effect) {
    use rt::ActionKind as K;
    a.kind = match e {
        Effect::Explode { damage, shake, sound } => {
            (a.damage, a.shake, a.sound) = (*damage, *shake, sound.clone());
            K::Explode
        }
        Effect::Bounce { factor, explode } => {
            (a.bounce_coeff, a.bounce_explode) = (*factor, *explode);
            K::Bounce
        }
        Effect::Injure { damage } => {
            a.damage = *damage;
            K::Injure
        }
        Effect::InjureWorm { damage } => {
            a.damage = *damage;
            K::InjureWorm
        }
        Effect::Carve { size } => {
            a.damage = *size;
            K::Carve
        }
        Effect::Dirt { shake } => {
            a.shake = *shake;
            K::Dirt
        }
        Effect::GreenDirt { shake } => {
            a.shake = *shake;
            K::GreenDirt
        }
        Effect::Sound { file } => {
            a.sound = file.clone();
            K::PlaySound
        }
        Effect::Halt => K::Disappear,
        Effect::Remove => K::Disappear2,
        Effect::GoThrough => K::GoThrough,
        Effect::InjureProjectile => K::InjureProj,
        Effect::ChangeRadius => K::ChangeRadius,
        Effect::SetSpeed => K::OverwriteOwnSpeed,
        Effect::MultiplySpeed => K::MultiplyOwnSpeed,
        Effect::AddSpeed => K::DiffOwnSpeed,
        Effect::SetTargetSpeed => K::OverwriteTargetSpeed,
        Effect::MultiplyTargetSpeed => K::MultiplyTargetSpeed,
        Effect::AddTargetSpeed => K::DiffTargetSpeed,
        Effect::HomeOnNearestWorm => K::HeadingToNextWorm,
        Effect::HomeOnOwner => K::HeadingToOwner,
        Effect::HomeOnNearestOtherWorm => K::HeadingToNextOtherWorm,
        Effect::HomeOnNearestEnemy => K::HeadingToNextEnemyWorm,
        Effect::HomeOnNearestTeamMate => K::HeadingToNextTeamMate,
        Effect::PullTargetsToUs => K::HeadTargetToUs,
        Effect::Unknown(i) => K::Other(*i),
        Effect::Spawn(_) => unreachable!("handled by the caller"),
    };
}

fn weapon_class(c: WeaponClass) -> rt::WeaponClass {
    match c {
        WeaponClass::Automatic => rt::WeaponClass::Automatic,
        WeaponClass::PowerGun => rt::WeaponClass::PowerGun,
        WeaponClass::Grenade => rt::WeaponClass::Grenade,
        WeaponClass::Missile => rt::WeaponClass::Missile,
        WeaponClass::CloseRange => rt::WeaponClass::CloseRange,
        WeaponClass::Unknown(i) => rt::WeaponClass::Other(i),
    }
}

/// The shared ids each shared projectile's tree references.
fn shared_refs(p: &Projectile, out: &mut BTreeSet<String>, depth: usize) {
    if depth > MAX_INLINE_DEPTH {
        return; // compile reports the depth with its path
    }
    let mut visit = |shared: &Option<String>, inline: &Option<Box<Projectile>>| {
        if let Some(id) = shared {
            out.insert(id.clone());
        }
        if let Some(child) = inline {
            shared_refs(child, out, depth + 1);
        }
    };
    for s in p.spawns.values() {
        visit(&s.shared, &s.projectile);
    }
    if let Trail::Projectile { spawn, .. } = &p.trail {
        visit(&spawn.shared, &spawn.projectile);
    }
}

fn check_cycles(src: &ModSource) -> Result<(), FormatError> {
    let graph: BTreeMap<&str, BTreeSet<String>> = src
        .shared
        .iter()
        .map(|(id, p)| {
            let mut refs = BTreeSet::new();
            shared_refs(p, &mut refs, 1);
            (id.as_str(), refs)
        })
        .collect();
    // Iterative depth-first search: 0 = new, 1 = on the path, 2 = done.
    let mut state: BTreeMap<&str, u8> = BTreeMap::new();
    for &start in graph.keys() {
        if state.get(start).copied().unwrap_or(0) != 0 {
            continue;
        }
        let mut path: Vec<(&str, Vec<&str>)> =
            vec![(start, graph[start].iter().map(String::as_str).collect())];
        state.insert(start, 1);
        while let Some((node, next)) = path.last_mut() {
            let node = *node;
            let child = next.pop(); // the borrow of `path` ends here
            match child {
                Some(child) if graph.contains_key(child) => match state.get(child).copied().unwrap_or(0) {
                    0 => {
                        state.insert(child, 1);
                        let kids = graph[child].iter().map(String::as_str).collect();
                        path.push((child, kids));
                    }
                    1 => {
                        let mut cycle: Vec<&str> =
                            path.iter().map(|(n, _)| *n).skip_while(|n| *n != child).collect();
                        cycle.push(child);
                        return Err(FormatError::invalid(
                            &shared_file(node),
                            "",
                            format!("shared projectiles form a cycle: {}", cycle.join(" -> ")),
                        ));
                    }
                    _ => {}
                },
                Some(_) => {} // an unknown id: compile reports it with its path
                None => {
                    state.insert(node, 2);
                    path.pop();
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use olr_data::mod_def as rt;

    fn one_weapon(shot: ShotSpawn) -> ModSource {
        ModSource {
            name: "Test".into(),
            notes: String::new(),
            worm: Worm::default(),
            rope: Rope::default(),
            weapons: vec![(
                "gun".into(),
                Weapon {
                    name: "Gun".into(),
                    notes: String::new(),
                    recharge: 0.5,
                    drain: 30.0,
                    between_shots_s: 0.25,
                    laser_sight: false,
                    fire: Fire::Projectile { class: WeaponClass::Missile, recoil: 5, sound: None, shot },
                    look: None,
                },
            )],
            shared: BTreeMap::new(),
            restrictions: BTreeMap::new(),
            loadouts: Default::default(),
            look: None,
        }
    }

    fn pixel() -> Projectile {
        Projectile::new(Gfx::Pixels(vec![]))
    }

    fn inline(p: Projectile) -> Option<Box<Projectile>> {
        Some(Box::new(p))
    }

    #[test]
    fn a_tree_compiles_to_derived_keys_and_lx56_actions() {
        let mut rocket = pixel();
        rocket.hit_terrain = vec![
            Effect::Explode { damage: 4, shake: 2, sound: Some("sfx/x.ogg".into()) },
            Effect::Spawn("bits".into()),
        ];
        rocket.spawns.insert(
            "bits".into(),
            EventSpawn { amount: 5, projectile: inline(pixel()), ..EventSpawn::default() },
        );
        rocket.trail = Trail::Projectile {
            every_s: 0.04,
            spawn: TrailSpawn { projectile: inline(pixel()), ..TrailSpawn::default() },
        };
        let def = compile_mod(&one_weapon(ShotSpawn {
            speed: 100,
            projectile: inline(rocket),
            ..ShotSpawn::default()
        }))
        .unwrap();
        let keys: Vec<&str> = def.projectiles.keys().map(String::as_str).collect();
        assert_eq!(keys, ["gun", "gun.s", "gun.t"]);
        let gun = &def.projectiles["gun"];
        assert_eq!(gun.hit.kind, rt::ActionKind::Explode);
        assert_eq!((gun.hit.damage, gun.hit.shake, gun.hit.spawns_projectiles), (4, 2, true));
        assert_eq!(gun.hit.sound.as_deref(), Some("sfx/x.ogg"));
        assert_eq!(gun.player_hit, rt::ActionDef::olx_default(rt::ActionKind::Nothing));
        assert_eq!(gun.event_spawn.as_ref().unwrap().projectile, "gun.s");
        assert_eq!(gun.event_spawn.as_ref().unwrap().amount, 5);
        assert_eq!(gun.trail.kind, rt::TrailKind::Projectile);
        assert_eq!(gun.trail.delay, 0.04);
        let trail = gun.trail.spawn.as_ref().unwrap();
        assert!(trail.use_special11_vec_for_speed_var && !trail.use_angle);
        assert_eq!((gun.width, gun.height), (2, 2));
        let rt::WeaponKind::Projectile { spawn, class, recoil, .. } = &def.weapons[0].kind else { panic!() };
        assert!(spawn.use_angle && spawn.use_random_rot && spawn.add_parent_vel);
        assert_eq!(
            (spawn.projectile.as_str(), spawn.speed, *class, *recoil),
            ("gun", 100, rt::WeaponClass::Missile, 5)
        );
        assert_eq!(def.weapons[0].id, "gun");
        assert_eq!(def.weapons[0].rof, 0.25);
    }

    #[test]
    fn an_explicit_id_and_a_shared_projectile_keep_their_keys() {
        let mut src = one_weapon(ShotSpawn { shared: Some("shell".into()), ..ShotSpawn::default() });
        let mut shell = pixel();
        shell.spawns.insert(
            "s".into(),
            EventSpawn {
                projectile: inline(Projectile { id: Some("odd-name".into()), ..pixel() }),
                ..EventSpawn::default()
            },
        );
        src.shared.insert("shell".into(), shell);
        let def = compile_mod(&src).unwrap();
        let keys: Vec<&str> = def.projectiles.keys().map(String::as_str).collect();
        assert_eq!(keys, ["odd-name", "shell"]);
    }

    #[test]
    fn what_lx56_cannot_express_is_an_error_with_a_path() {
        let mut two = pixel();
        two.hit_worm =
            vec![Effect::Injure { damage: 1 }, Effect::Explode { damage: 1, shake: 0, sound: None }];
        let e = compile_mod(&one_weapon(ShotSpawn { projectile: inline(two), ..ShotSpawn::default() }))
            .unwrap_err();
        assert!(e.to_string().contains("fire.shot.projectile.hit_worm[1]"), "{e}");
        assert!(e.to_string().contains("not supported by this runtime"), "{e}");
    }

    #[test]
    fn a_spawn_names_an_entry_of_spawns() {
        let mut p = pixel();
        p.hit_terrain = vec![Effect::Spawn("missing".into())];
        let e = compile_mod(&one_weapon(ShotSpawn { projectile: inline(p), ..ShotSpawn::default() }))
            .unwrap_err();
        assert!(e.to_string().contains("no spawn named \"missing\""), "{e}");
    }

    #[test]
    fn events_must_share_one_spawn() {
        let mut p = pixel();
        p.hit_terrain = vec![Effect::Spawn("a".into())];
        p.hit_worm = vec![Effect::Spawn("b".into())];
        p.spawns.insert("a".into(), EventSpawn { projectile: inline(pixel()), ..EventSpawn::default() });
        p.spawns.insert("b".into(), EventSpawn { projectile: inline(pixel()), ..EventSpawn::default() });
        assert!(
            compile_mod(&one_weapon(ShotSpawn { projectile: inline(p), ..ShotSpawn::default() })).is_err()
        );
    }

    #[test]
    fn a_spawn_takes_exactly_one_of_shared_and_projectile() {
        assert!(compile_mod(&one_weapon(ShotSpawn::default())).is_err());
        let both =
            ShotSpawn { shared: Some("x".into()), projectile: inline(pixel()), ..ShotSpawn::default() };
        assert!(compile_mod(&one_weapon(both)).is_err());
        let unknown = ShotSpawn { shared: Some("nope".into()), ..ShotSpawn::default() };
        let e = compile_mod(&one_weapon(unknown)).unwrap_err();
        assert!(e.to_string().contains("projectiles/nope.ron"), "{e}");
    }

    /// `MAX_SHARED` shared projectiles chained by name (each spawns the
    /// next): `check_cycles` is iterative, so the length costs no stack,
    /// whether the chain ends (a mod) or closes (a cycle, refused).
    #[test]
    fn a_chain_of_max_shared_shared_projectiles_neither_crashes_nor_overflows() {
        let id = |i: usize| format!("p{i:05}");
        let chained = |close: bool| {
            let mut src = one_weapon(ShotSpawn { shared: Some(id(0)), ..ShotSpawn::default() });
            for i in 0..MAX_SHARED {
                let mut p = pixel();
                let next = if i + 1 < MAX_SHARED {
                    Some(id(i + 1))
                } else if close {
                    Some(id(0))
                } else {
                    None
                };
                if let Some(next) = next {
                    p.hit_terrain = vec![Effect::Spawn("s".into())];
                    p.spawns.insert("s".into(), EventSpawn { shared: Some(next), ..EventSpawn::default() });
                }
                src.shared.insert(id(i), p);
            }
            src
        };
        let def = compile_mod(&chained(false)).expect("an open chain compiles");
        assert_eq!(def.projectiles.len(), MAX_SHARED);
        let e = compile_mod(&chained(true)).unwrap_err().to_string();
        assert!(e.contains("cycle"), "{e}");
    }

    #[test]
    fn a_key_used_twice_is_an_error() {
        let mut p = pixel();
        p.spawns.insert(
            "s".into(),
            EventSpawn {
                projectile: inline(Projectile { id: Some("gun".into()), ..pixel() }),
                ..EventSpawn::default()
            },
        );
        let e = compile_mod(&one_weapon(ShotSpawn { projectile: inline(p), ..ShotSpawn::default() }))
            .unwrap_err();
        assert!(e.to_string().contains("\"gun\""), "{e}");
    }

    #[test]
    fn shared_cycles_are_errors() {
        let mut src = one_weapon(ShotSpawn { shared: Some("a".into()), ..ShotSpawn::default() });
        let refer = |to: &str| {
            let mut p = pixel();
            p.spawns.insert("s".into(), EventSpawn { shared: Some(to.into()), ..EventSpawn::default() });
            p
        };
        src.shared.insert("a".into(), refer("b"));
        src.shared.insert("b".into(), refer("a"));
        let e = compile_mod(&src).unwrap_err();
        assert!(e.to_string().contains("cycle"), "{e}");
    }

    #[test]
    fn deep_inline_nesting_is_an_error_not_a_crash() {
        let mut p = pixel();
        for _ in 0..40 {
            let mut parent = pixel();
            parent.spawns.insert("s".into(), EventSpawn { projectile: inline(p), ..EventSpawn::default() });
            p = parent;
        }
        let e = compile_mod(&one_weapon(ShotSpawn { projectile: inline(p), ..ShotSpawn::default() }))
            .unwrap_err();
        assert!(e.to_string().contains("nested deeper than 32"), "{e}");
    }

    #[test]
    fn timer_effects_need_a_positive_time() {
        let mut p = pixel();
        p.timer = Some(Timer { after_s: 0.0, variation_s: 0.0, effects: vec![Effect::Remove] });
        assert!(
            compile_mod(&one_weapon(ShotSpawn { projectile: inline(p), ..ShotSpawn::default() })).is_err()
        );
        let mut q = pixel();
        q.timer = Some(Timer { after_s: 2.0, variation_s: 0.5, effects: vec![] });
        let def =
            compile_mod(&one_weapon(ShotSpawn { projectile: inline(q), ..ShotSpawn::default() })).unwrap();
        let t = &def.projectiles["gun"].timer;
        assert_eq!((t.time, t.time_var), (2.0, 0.5));
        assert_eq!(t.action.as_ref().unwrap().kind, rt::ActionKind::Nothing);
    }

    #[test]
    fn a_nan_timer_with_effects_is_an_error_and_without_them_no_action() {
        let mut p = pixel();
        p.timer = Some(Timer { after_s: f32::NAN, variation_s: 0.0, effects: vec![Effect::Remove] });
        let e = compile_mod(&one_weapon(ShotSpawn { projectile: inline(p), ..ShotSpawn::default() }))
            .unwrap_err();
        assert!(e.to_string().contains("timer.effects"), "{e}");
        let mut q = pixel();
        q.timer = Some(Timer { after_s: f32::NAN, variation_s: 0.0, effects: vec![] });
        let def =
            compile_mod(&one_weapon(ShotSpawn { projectile: inline(q), ..ShotSpawn::default() })).unwrap();
        assert!(def.projectiles["gun"].timer.action.is_none());
    }

    #[test]
    fn an_absurd_spawn_amount_is_an_error_with_its_path() {
        let ok = ShotSpawn { amount: MAX_AMOUNT, projectile: inline(pixel()), ..ShotSpawn::default() };
        assert!(compile_mod(&one_weapon(ok)).is_ok());
        let bad = ShotSpawn { amount: MAX_AMOUNT + 1, projectile: inline(pixel()), ..ShotSpawn::default() };
        let e = compile_mod(&one_weapon(bad)).unwrap_err();
        assert!(e.to_string().contains("fire.shot.amount"), "{e}");
    }

    #[test]
    fn a_shared_projectile_nests_as_deep_as_a_shot() {
        // Both roots are depth 1, so MAX_INLINE_DEPTH levels compile and one more does not.
        let chain = |levels: usize| {
            let mut p = pixel();
            for _ in 1..levels {
                let mut parent = pixel();
                parent
                    .spawns
                    .insert("s".into(), EventSpawn { projectile: inline(p), ..EventSpawn::default() });
                p = parent;
            }
            p
        };
        let with = |levels| {
            let mut src = one_weapon(ShotSpawn { shared: Some("root".into()), ..ShotSpawn::default() });
            src.shared.insert("root".into(), chain(levels));
            compile_mod(&src)
        };
        assert!(with(MAX_INLINE_DEPTH).is_ok());
        assert!(with(MAX_INLINE_DEPTH + 1).is_err());
    }

    /// Issue #16: every error, each with its path; the first is `compile_mod`'s.
    #[test]
    fn mod_errors_lists_every_error_and_compile_mod_the_first() {
        let mut two = pixel();
        two.hit_worm =
            vec![Effect::Injure { damage: 1 }, Effect::Explode { damage: 1, shake: 0, sound: None }];
        let mut src =
            one_weapon(ShotSpawn { amount: MAX_AMOUNT + 1, projectile: inline(two), ..ShotSpawn::default() });
        let mut orphan = pixel();
        orphan.hit_terrain = vec![Effect::Spawn("nowhere".into())];
        src.shared.insert("orphan".into(), orphan);
        src.loadouts
            .insert("Nope".into(), olr_data::loadouts::ShippedLoadout { loading_time: 100, unique: false });
        let errors = mod_errors(&src);
        let at: Vec<(String, String)> = errors
            .iter()
            .map(|e| match e {
                FormatError::Invalid { file, path, .. } => (file.clone(), path.clone()),
                e => panic!("{e}"),
            })
            .collect();
        let at: Vec<(&str, &str)> = at.iter().map(|(f, p)| (f.as_str(), p.as_str())).collect();
        assert_eq!(
            at,
            [
                ("projectiles/orphan.ron", ""),
                ("weapons/gun.ron", "fire.shot.amount"),
                ("weapons/gun.ron", "fire.shot.projectile.hit_worm[1]"),
                ("mod.ron", "loadouts[\"Nope\"]"),
            ]
        );
        assert_eq!(compile_mod(&src).unwrap_err(), errors[0]);
        assert!(
            mod_errors(&one_weapon(ShotSpawn { projectile: inline(pixel()), ..ShotSpawn::default() }))
                .is_empty()
        );
    }

    #[test]
    fn invalid_ids_are_errors() {
        let mut src = one_weapon(ShotSpawn { projectile: inline(pixel()), ..ShotSpawn::default() });
        src.weapons[0].0 = "../evil".into();
        assert!(compile_mod(&src).is_err());
    }
}
