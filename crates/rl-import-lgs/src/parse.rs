//! Byte-level parsing of `script.lgs` v7 into a tree that mirrors the file:
//! v7 stores every spawned projectile inline, so children are owned here and
//! names are assigned later by `convert`.

use rl_data::image::Rgba;
use rl_data::mod_def::*;

use crate::LgsError;
use crate::reader::Reader;

const ID: &[u8] = b"Liero Game Script";
const VERSION_LX56: u32 = 7;
const HEADER_LEN: usize = 88;
const MAX_WEAPONS: i32 = 4096;
const MAX_LIST: u32 = 4096;
const MAX_DEPTH: usize = 128;

// OLX enum values used while parsing (include/WeaponDesc.h, ProjectileDesc.h, ProjAction.h).
const WPN_PROJECTILE: i32 = 0;
const WPN_SPECIAL: i32 = 1;
const WPN_BEAM: i32 = 2;
const PRJ_PIXEL: i32 = 0;
const PRJ_IMAGE: i32 = 1;
#[cfg(test)]
const PJ_EXPLODE: i32 = 1;
#[cfg(test)]
const PJ_INJURE: i32 = 2;
const TRL_PROJECTILE: i32 = 3;

/// A parsed mod before projectile naming and asset resolution. Image and sound
/// names are exactly as written in the file; spawn targets are left empty.
#[derive(Clone, Debug, PartialEq)]
pub struct RawMod {
    pub name: String,
    pub weapons: Vec<RawWeapon>,
    pub rope: RopeParams,
    pub worm: WormParams,
    /// Bytes after the worm block; OLX ignores them.
    pub trailing_bytes: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RawWeapon {
    pub def: WeaponDef,
    pub projectile: Option<ProjNode>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProjNode {
    pub def: ProjectileDef,
    pub event_child: Option<Box<ProjNode>>,
    pub trail_child: Option<Box<ProjNode>>,
}

/// Parses a complete `script.lgs` file.
pub fn parse_lgs(data: &[u8]) -> Result<RawMod, LgsError> {
    let mut r = Reader::new(data);
    let header = r.bytes(HEADER_LEN, "header")?;
    let id_end = header[..18].iter().position(|&b| b == 0).unwrap_or(18);
    if &header[..id_end] != ID {
        return Err(LgsError::BadId);
    }
    // ID[18], 2 bytes of struct padding, then Version at offset 20 (gs_header_t).
    let version = u32::from_le_bytes(header[20..24].try_into().expect("4 bytes"));
    if version != VERSION_LX56 {
        return Err(LgsError::UnsupportedVersion(version));
    }
    let name = Reader::new(&header[24..88]).fixed_cstr(64, "mod name")?;

    let count = r.i32("weapon count")?;
    if !(0..=MAX_WEAPONS).contains(&count) {
        return Err(LgsError::Implausible { what: "weapon count", value: count as i64 });
    }
    let mut weapons = Vec::with_capacity(count as usize);
    for _ in 0..count {
        weapons.push(parse_weapon(&mut r)?);
    }

    let rope = RopeParams {
        max_length: r.i32("rope length")?,
        rest_length: r.i32("rope rest length")?,
        strength: r.f32("rope strength")?,
    };
    let worm = WormParams {
        angle_speed: r.f32("worm")?,
        ground_speed: r.f32("worm")?,
        air_speed: r.f32("worm")?,
        gravity: r.f32("worm")?,
        jump_force: r.f32("worm")?,
        air_friction: r.f32("worm")?,
        ground_friction: r.f32("worm")?,
    };
    Ok(RawMod { name, weapons, rope, worm, trailing_bytes: r.remaining() })
}

fn parse_weapon(r: &mut Reader) -> Result<RawWeapon, LgsError> {
    let name = r.pstring("weapon name")?;
    let ty = r.i32("weapon type")?;
    let mut projectile = None;
    let (kind, recharge, drain, rof, laser_sight) = match ty {
        WPN_SPECIAL => {
            let special = SpecialKind::from_lx(r.i32("special")?);
            let thrust = r.u32("thrust")?;
            let (recharge, drain, rof) = (r.f32("recharge")?, r.f32("drain")?, r.f32("rof")?);
            let laser = r.flag("laser sight")?;
            (WeaponKind::Special { special, thrust }, recharge, drain, rof, laser)
        }
        WPN_BEAM => {
            let recoil = r.i32("recoil")?;
            let (recharge, drain, rof) = (r.f32("recharge")?, r.f32("drain")?, r.f32("rof")?);
            let laser = r.flag("laser sight")?;
            // Wpn_Beam::read for v7 (CGameScript.cpp:2058-2083): colour as three ints,
            // truncated to bytes like OLX's Color(Uint8, Uint8, Uint8).
            let colour = Rgba::rgb(
                r.i32("beam colour")? as u8,
                r.i32("beam colour")? as u8,
                r.i32("beam colour")? as u8,
            );
            let beam = BeamDef {
                colour,
                damage: r.i32("beam damage")?,
                player_damage: r.i32("beam player damage")?,
                length: r.i32("beam length")?,
                init_width: 1,
                width_increase: 0.0,
                distribute_damage_over_width: false,
            };
            (WeaponKind::Beam { recoil, beam }, recharge, drain, rof, laser)
        }
        WPN_PROJECTILE => {
            let class = WeaponClass::from_lx(r.i32("weapon class")?);
            let recoil = r.i32("recoil")?;
            let (recharge, drain, rof) = (r.f32("recharge")?, r.f32("drain")?, r.f32("rof")?);
            // weapon_t's constructor sets UseRandomRot, Useangle and AddParentVel for shots.
            let mut spawn = SpawnDef::olx_default(String::new());
            spawn.speed = r.i32("speed")?;
            spawn.speed_var = r.f32("speed var")?;
            spawn.spread = r.f32("spread")?;
            spawn.amount = r.i32("amount")?;
            spawn.use_random_rot = true;
            spawn.use_angle = true;
            spawn.add_parent_vel = true;
            let laser = r.flag("laser sight")?;
            let sound = if r.flag("use sound")? { Some(r.pstring("sound name")?) } else { None };
            projectile = Some(parse_projectile(r, 0)?);
            (WeaponKind::Projectile { class, recoil, sound, spawn }, recharge, drain, rof, laser)
        }
        other => return Err(LgsError::UnknownWeaponType(other, name)),
    };
    let def = WeaponDef {
        id: String::new(),
        name,
        // CGameScript.cpp:776-777
        recharge: recharge / 10.0,
        drain,
        rof: rof / 1000.0,
        laser_sight,
        kind,
    };
    Ok(RawWeapon { def, projectile })
}

/// The Hit, Timer, PlayerHit, Exp and Touch actions of a v7 projectile.
/// Exp and Touch are obsolete in OLX but their "projectiles" flags still decide
/// whether the file contains the event spawn block.
fn parse_projectile(r: &mut Reader, depth: usize) -> Result<ProjNode, LgsError> {
    if depth > MAX_DEPTH {
        return Err(LgsError::TooDeep);
    }
    let ty = r.i32("projectile type")?;
    let time = r.f32("timer time")?;
    let time_var = r.f32("timer time var")?;
    let trail_type = r.i32("trail type")?;
    let gravity = if r.flag("use custom gravity")? { Some(r.i32("gravity")?) } else { None };
    // Read with sizeof(int) into a float field: the bytes are an f32.
    let dampening = r.f32("dampening")?;
    let (width, height) = if ty == PRJ_PIXEL { (2, 2) } else { (4, 4) };

    let gfx = match ty {
        PRJ_PIXEL => {
            let n = r.u32("colour count")?;
            if n > MAX_LIST {
                return Err(LgsError::Implausible { what: "colour count", value: n as i64 });
            }
            let mut colours = Vec::with_capacity(n as usize);
            for _ in 0..n {
                let (cr, cg, cb) = (r.i32("colour")?, r.i32("colour")?, r.i32("colour")?);
                colours.push(Rgba::rgb(cr as u8, cg as u8, cb as u8));
            }
            ProjectileGfx::Pixel { colours }
        }
        PRJ_IMAGE => {
            let file = r.pstring("image name")?;
            let rotating = r.flag("rotating")?;
            let rot_increment = r.i32("rot increment")?;
            let rot_speed = r.i32("rot speed")?;
            let use_angle = r.flag("use angle")?;
            let use_spec_angle = r.flag("use spec angle")?;
            let angle_images = if use_angle || use_spec_angle { r.i32("angle images")? } else { 0 };
            let animation = if r.flag("animating")? {
                Some(Animation { rate: r.f32("anim rate")?, kind: AnimKind::from_lx(r.i32("anim type")?) })
            } else {
                None
            };
            ProjectileGfx::Image {
                file,
                rotating,
                rot_increment,
                rot_speed,
                use_angle,
                use_spec_angle,
                angle_images,
                animation,
            }
        }
        // Circle, polygon and rect (2-4) only exist in v8 files.
        other => return Err(LgsError::UnknownProjectileType(other)),
    };

    // Hit (terrain).
    let mut hit = ActionDef::olx_default(ActionKind::from_lx(r.i32("hit type")?));
    match hit.kind {
        ActionKind::Explode => {
            hit.damage = r.i32("hit damage")?;
            hit.spawns_projectiles = r.flag("hit projectiles")?;
            let use_sound = r.flag("hit use sound")?;
            hit.shake = r.i32("hit shake")?;
            if use_sound {
                hit.sound = Some(r.pstring("hit sound")?);
            }
        }
        ActionKind::Bounce => {
            hit.bounce_coeff = r.f32("hit bounce coeff")?;
            hit.bounce_explode = r.i32("hit bounce explode")?;
        }
        ActionKind::Carve => hit.damage = r.i32("hit damage")?,
        _ => {}
    }

    // Timer: only present when Time > 0.
    let timer_action = if time > 0.0 {
        let mut a = ActionDef::olx_default(ActionKind::from_lx(r.i32("timer type")?));
        if a.kind == ActionKind::Explode {
            a.damage = r.i32("timer damage")?;
            a.spawns_projectiles = r.flag("timer projectiles")?;
            a.shake = r.i32("timer shake")?;
        }
        Some(a)
    } else {
        None
    };

    // Player hit.
    let mut player_hit = ActionDef::olx_default(ActionKind::from_lx(r.i32("player hit type")?));
    match player_hit.kind {
        ActionKind::Injure | ActionKind::Explode => {
            player_hit.damage = r.i32("player hit damage")?;
            player_hit.spawns_projectiles = r.flag("player hit projectiles")?;
        }
        ActionKind::Bounce => player_hit.bounce_coeff = r.f32("player hit bounce coeff")?,
        _ => {}
    }

    // Obsolete Exp and Touch actions: read and dropped, except their projectile flags.
    let mut obsolete_spawns = false;
    for what in ["explode action", "touch action"] {
        let _ty = r.i32(what)?;
        let _damage = r.i32(what)?;
        obsolete_spawns |= r.flag(what)?;
        if r.flag(what)? {
            r.pstring(what)?;
        }
    }

    let any_spawns = obsolete_spawns
        || hit.spawns_projectiles
        || player_hit.spawns_projectiles
        || timer_action.as_ref().is_some_and(|a| a.spawns_projectiles);
    let mut event_child = None;
    let event_spawn = if any_spawns {
        let mut s = SpawnDef::olx_default(String::new());
        s.use_angle = r.flag("spawn use angle")?;
        s.angle = r.i32("spawn angle")?;
        s.amount = r.i32("spawn amount")?;
        s.spread = r.f32("spawn spread")?;
        s.speed = r.i32("spawn speed")?;
        s.speed_var = r.f32("spawn speed var")?;
        event_child = Some(Box::new(parse_projectile(r, depth + 1)?));
        Some(s)
    } else {
        None
    };

    let kind = TrailKind::from_lx(trail_type);
    let mut trail = TrailDef { kind, delay: 0.1, spawn: None };
    let mut trail_child = None;
    if trail_type == TRL_PROJECTILE {
        // Proj_Trail's constructor sets UseSpecial11VecForSpeedVar.
        let mut s = SpawnDef::olx_default(String::new());
        s.use_special11_vec_for_speed_var = true;
        s.use_parent_velocity_for_spread = r.flag("trail use parent velocity")?;
        trail.delay = r.f32("trail delay")? / 1000.0;
        s.amount = r.i32("trail amount")?;
        s.speed = r.i32("trail speed")?;
        s.speed_var = r.f32("trail speed var")?;
        s.spread = r.f32("trail spread")?;
        trail_child = Some(Box::new(parse_projectile(r, depth + 1)?));
        trail.spawn = Some(s);
    }

    let def = ProjectileDef {
        gfx,
        gravity,
        dampening,
        width,
        height,
        timer: TimerDef { time, time_var, action: timer_action },
        trail,
        hit,
        player_hit,
        event_spawn,
    };
    Ok(ProjNode { def, event_child, trail_child })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Builds v7 files byte by byte in the order `CGameScript::Load` reads them.
    #[derive(Default)]
    pub(crate) struct Lgs(pub Vec<u8>);

    impl Lgs {
        pub(crate) fn header(name: &str, weapons: i32) -> Self {
            let mut b = Lgs::default();
            let mut id = [0u8; 20];
            id[..ID.len()].copy_from_slice(ID);
            b.0.extend_from_slice(&id);
            b.0.extend_from_slice(&VERSION_LX56.to_le_bytes());
            let mut n = [0xAAu8; 64]; // garbage after the NUL, like real files
            n[..name.len()].copy_from_slice(name.as_bytes());
            n[name.len()] = 0;
            b.0.extend_from_slice(&n);
            b.i(weapons)
        }
        pub(crate) fn i(mut self, v: i32) -> Self {
            self.0.extend_from_slice(&v.to_le_bytes());
            self
        }
        pub(crate) fn f(mut self, v: f32) -> Self {
            self.0.extend_from_slice(&v.to_le_bytes());
            self
        }
        pub(crate) fn s(mut self, v: &str) -> Self {
            self.0.push(v.len() as u8);
            self.0.extend_from_slice(v.as_bytes());
            self
        }
        /// A pixel projectile with one colour, no timer, no trail, `hit` explode with
        /// optional spawning of `child`, player hit injure.
        pub(crate) fn pixel_projectile(self, damage: i32, child: Option<fn(Lgs) -> Lgs>) -> Self {
            let b = self.i(PRJ_PIXEL).f(0.0).f(0.0).i(0).i(0).f(1.0); // type, timer, trail, no gravity, damp
            let b = b.i(1).i(255).i(0).i(300); // 1 colour; blue 300 truncates to 44
            let b = b.i(PJ_EXPLODE).i(damage).i(child.is_some() as i32).i(0).i(0); // hit: no sound, shake 0
            let b = b.i(PJ_INJURE).i(7).i(0); // player hit
            let b = b.i(0).i(0).i(0).i(0).i(0).i(0).i(0).i(0); // exp + touch
            match child {
                Some(c) => c(b.i(1).i(90).i(3).f(0.5).i(40).f(0.25)),
                None => b,
            }
        }
        pub(crate) fn footer(self) -> Self {
            self.i(300).i(27).f(4.5).f(150.0).f(8.0).f(4.0).f(100.0).f(-65.0).f(0.0).f(0.4)
        }
        pub(crate) fn projectile_weapon(self, name: &str, sound: Option<&str>) -> Self {
            let b = self.s(name).i(WPN_PROJECTILE).i(4).i(5).f(5.0).f(30.0).f(250.0);
            let b = b.i(100).f(0.1).f(2.0).i(1).i(0); // speed, speed var, spread, amount, laser
            match sound {
                Some(s) => b.i(1).s(s),
                None => b.i(0),
            }
        }
    }

    fn one_weapon_mod() -> Vec<u8> {
        Lgs::header("Tiny Mod", 1)
            .projectile_weapon("Rocket", Some("boom.wav"))
            .pixel_projectile(12, Some(|b| b.pixel_projectile(3, None)))
            .footer()
            .0
    }

    #[test]
    fn parses_header_and_footer() {
        let m = parse_lgs(&one_weapon_mod()).unwrap();
        assert_eq!(m.name, "Tiny Mod");
        assert_eq!(m.rope, RopeParams { max_length: 300, rest_length: 27, strength: 4.5 });
        assert_eq!(m.worm.jump_force, -65.0);
        assert_eq!(m.trailing_bytes, 0);
    }

    #[test]
    fn applies_olx_post_processing_and_shot_flags() {
        let m = parse_lgs(&one_weapon_mod()).unwrap();
        let w = &m.weapons[0].def;
        assert_eq!(w.rof, 250.0 / 1000.0);
        assert_eq!(w.recharge, 5.0 / 10.0);
        let WeaponKind::Projectile { class, sound, spawn, .. } = &w.kind else { panic!("projectile weapon") };
        assert_eq!(*class, WeaponClass::Missile);
        assert_eq!(sound.as_deref(), Some("boom.wav"));
        assert!(spawn.use_random_rot && spawn.use_angle && spawn.add_parent_vel);
        assert_eq!((spawn.speed, spawn.amount), (100, 1));
    }

    #[test]
    fn parses_nested_event_spawn() {
        let m = parse_lgs(&one_weapon_mod()).unwrap();
        let p = m.weapons[0].projectile.as_ref().unwrap();
        assert_eq!(p.def.hit.kind, ActionKind::Explode);
        assert_eq!(p.def.hit.damage, 12);
        assert!(p.def.hit.spawns_projectiles);
        let s = p.def.event_spawn.as_ref().unwrap();
        assert_eq!((s.use_angle, s.angle, s.amount, s.speed), (true, 90, 3, 40));
        let child = p.event_child.as_ref().unwrap();
        assert_eq!(child.def.hit.damage, 3);
        assert!(child.event_child.is_none());
        assert_eq!(p.def.gfx, ProjectileGfx::Pixel { colours: vec![Rgba::rgb(255, 0, 44)] });
        assert_eq!((p.def.width, p.def.height), (2, 2));
        assert_eq!(p.def.player_hit.damage, 7);
    }

    #[test]
    fn rejects_bad_header() {
        let mut data = one_weapon_mod();
        data[0] = b'X';
        assert!(matches!(parse_lgs(&data), Err(LgsError::BadId)));
        let mut data = one_weapon_mod();
        data[20] = 8;
        assert!(matches!(parse_lgs(&data), Err(LgsError::UnsupportedVersion(8))));
    }

    #[test]
    fn rejects_implausible_weapon_count() {
        let data = Lgs::header("x", -1).0;
        assert!(matches!(parse_lgs(&data), Err(LgsError::Implausible { what: "weapon count", .. })));
    }

    #[test]
    fn every_truncation_is_an_error() {
        let data = one_weapon_mod();
        for len in 0..data.len() {
            assert!(parse_lgs(&data[..len]).is_err(), "prefix of {len} bytes parsed");
        }
    }

    #[test]
    fn rejects_deep_nesting() {
        fn nest(b: Lgs, depth: usize) -> Lgs {
            if depth == 0 {
                return b.pixel_projectile(1, None);
            }
            let b = b.i(PRJ_PIXEL).f(0.0).f(0.0).i(0).i(0).f(1.0).i(0);
            let b = b.i(PJ_EXPLODE).i(1).i(1).i(0).i(0).i(8);
            let b = b.i(0).i(0).i(0).i(0).i(0).i(0).i(0).i(0);
            nest(b.i(0).i(0).i(1).f(0.0).i(0).f(0.0), depth - 1)
        }
        let data = nest(Lgs::header("deep", 1).projectile_weapon("w", None), 200).footer().0;
        assert!(matches!(parse_lgs(&data), Err(LgsError::TooDeep)));
    }

    proptest::proptest! {
        #[test]
        fn never_panics_on_arbitrary_bytes(tail in proptest::collection::vec(proptest::num::u8::ANY, 0..512)) {
            let mut data = Lgs::header("fuzz", 1).0;
            data.extend_from_slice(&tail);
            let _ = parse_lgs(&data);
        }

        #[test]
        fn never_panics_on_corrupted_real_layout(pos in 0usize..200, byte in proptest::num::u8::ANY) {
            let mut data = one_weapon_mod();
            let pos = pos % data.len();
            data[pos] = byte;
            let _ = parse_lgs(&data);
        }
    }
}
