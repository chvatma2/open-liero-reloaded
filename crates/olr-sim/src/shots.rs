//! Shots: the server's shot list (`CShootList`, `src/common/CShootList.cpp`),
//! its delivery to the client (Model R, spec §18 D1) and the client's
//! `ProcessShot` (`src/client/CClient_Game.cpp:852-921`).
//!
//! A shot fired in a sub-step does not spawn anything at once. It is added
//! to the list, which goes over OLX's network round trip ([`encode_decode`])
//! after the frame following the one that started the list; the decoded
//! shots then spawn their projectiles and apply recoil.

use alloc::vec::Vec;

use olr_math::{AbsTime, GlibcRand, TimeDiff, Vec2};
use olr_terrain::{Stamp, Terrain};

use crate::SimSettings;
use crate::beam::{BeamCtx, process_shot_beam};
use crate::bonus::{Bonus, MAX_BONUSES};
use crate::buf::Buf;
use crate::events::EventKind;
use crate::mod_rt::{SimMod, WeaponType};
use crate::projectile::{ProjectilePool, c_int};
use crate::rng::Draws;
use crate::spawn::{SpawnCtx, SpawnParent, spawn_info_apply, vec_from_angle};
use crate::world::MAX_WORMS;
use crate::worm::Worm;

pub use crate::shot_codec::{RoundTrip, encode_decode};

/// `MAX_SHOOTINGS` (`include/CShootList.h:49`).
pub const MAX_SHOOTINGS: usize = 256;

/// OLX `shoot_t` (`include/CShootList.h:53-66`), plus the spawn time it was
/// delivered with.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Shot {
    /// `nWormID`.
    pub worm: i32,
    /// `nWeapon`, the weapon id.
    pub weapon: i32,
    /// `cPos`, the worm's position when it fired.
    pub pos: Vec2,
    /// `cWormVel`: the worm's velocity with `ProjRelativeVel`, else zero.
    pub worm_vel: Vec2,
    /// `nAngle`, whole degrees in `0..360`.
    pub angle: i32,
    /// `nRandom`, the index into the fixed random table. Spawning advances
    /// it, so a delivered shot shows the value after spawning.
    pub random: i32,
    /// `nSpeed`: the worm's speed times 100 (`ProjRelativeVel`); nothing reads it.
    pub speed: i32,
    /// `fTime`: the server time when it was fired (`serverFrame * 10` ms).
    pub time: TimeDiff,
    pub release: bool,
    /// `shoot_t::spawnTime()` at delivery; zero before.
    pub spawn: AbsTime,
}

/// The server's list for the local client, with Model R's send rule.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct ShootList {
    shots: Buf<Shot>,
    /// The list was started in an earlier frame, so the next delivery sends it.
    started: bool,
}

impl ShootList {
    pub fn new() -> Self {
        Self { shots: Buf::with_capacity(MAX_SHOOTINGS), started: false }
    }

    /// `CShootList::addShoot` (`CShootList.cpp:80-112`). A full list drops the
    /// shot before its random draw.
    #[allow(clippy::too_many_arguments)]
    pub fn add(
        &mut self,
        weapon: i32,
        time: TimeDiff,
        speed: f32,
        angle: i32,
        w: &Worm,
        relative_vel: bool,
        rand: &mut GlibcRand,
    ) -> bool {
        if self.shots.len() >= MAX_SHOOTINGS {
            return false;
        }
        self.shots.push(Shot {
            pos: w.pos,
            worm_vel: if relative_vel { w.vel } else { Vec2::ZERO },
            time,
            angle,
            random: rand.random_int(255),
            speed: if relative_vel { c_int(speed * 100.0) } else { 0 },
            weapon,
            worm: w.id,
            release: false,
            spawn: AbsTime(0),
        });
        true
    }

    #[cfg(test)]
    pub fn last(&self) -> Option<&Shot> {
        self.shots.last()
    }

    /// Model R (spec §18 D1, `OracleHooks::deliverShots`): called between
    /// frames. A list that is not empty is sent the second time this is
    /// called, so shots fired in frames F and F+1 travel together after F+1.
    /// Returns the shots to send and clears the list, or `None`.
    pub fn take_due(&mut self) -> Option<&mut Vec<Shot>> {
        if self.shots.is_empty() {
            return None;
        }
        if !self.started {
            self.started = true;
            return None;
        }
        self.started = false;
        Some(&mut self.shots)
    }
}

/// `shoot_t::spawnTime` (`CShootList.cpp:715-731`) with the clock as the
/// oracle pins it. `server_frame` is `game.serverFrame` (`serverTime()` is
/// `serverFrame * 10` ms, `Game.h:111`).
///
/// A decoded `fTime` later than the server time (only from frame 819205 on,
/// through the float round trip) falls back to `currentTime - ping / 2`;
/// a local client's ping is taken as 0. Times wrap like OLX's `Uint64`.
pub fn spawn_time(shot_time: TimeDiff, server_frame: u64, current_time: AbsTime) -> AbsTime {
    let server_time = server_frame.wrapping_mul(10);
    let ping = 0.0f32;
    let fallback = AbsTime(current_time.0.wrapping_sub((ping / 1000.0 / 2.0 * 1000.0) as u64));
    if shot_time.0 <= server_time {
        AbsTime(current_time.0.wrapping_sub(server_time.wrapping_sub(shot_time.0)))
    } else {
        fallback
    }
}

/// What delivering a shot reads or changes besides the worms.
pub(crate) struct ShotCtx<'a> {
    pub sim_mod: &'a SimMod,
    pub settings: &'a SimSettings,
    pub pool: &'a mut ProjectilePool,
    pub terrain: &'a mut Terrain,
    pub holes: &'a [Stamp],
    pub draws: &'a mut Draws,
    /// The beam trace's `goodWidthParts`.
    pub parts: &'a mut Vec<bool>,
    /// `GetPhysicsTime()`: the frame clock.
    pub now: AbsTime,
    /// The client's bonuses, which beams explode.
    pub bonuses: &'a mut [Bonus; MAX_BONUSES],
    /// The game mode, which credits kills.
    pub referee: &'a mut crate::referee::Referee,
}

/// `CClient::ProcessShot` (`CClient_Game.cpp:852-921`). The shot of a worm
/// that is dead at delivery is dropped. A beam's shot is traced through the
/// map ([`process_shot_beam`]); a projectile weapon's plays its sound,
/// applies recoil and spawns its projectiles. Both report `WeaponFired`.
pub(crate) fn process_shot(shot: &mut Shot, spawn: AbsTime, worms: &mut [Worm], ctx: &mut ShotCtx) {
    let by_worm = (0..MAX_WORMS as i32).contains(&shot.worm);
    if by_worm {
        let Some(w) = worms.iter().find(|w| w.id == shot.worm) else { return };
        if !w.alive {
            return;
        }
    }
    let Some(wpn) = ctx.sim_mod.weapon(shot.weapon) else { return };
    if shot.release {
        // `FinalProj` exists only in mods newer than LX56.
        return;
    }
    let fired = EventKind::WeaponFired { worm: shot.worm, weapon: shot.weapon };
    if wpn.kind == WeaponType::Beam {
        ctx.draws.events.push(fired);
        let mut beam = BeamCtx { terrain: ctx.terrain, holes: ctx.holes, draws: ctx.draws, parts: ctx.parts };
        process_shot_beam(shot, wpn, worms, &mut beam, ctx.settings, ctx.now, ctx.bonuses, ctx.referee);
        return;
    }
    let Some(info) = &wpn.spawn else { return };
    ctx.draws.events.push(fired);
    if let Some(id) = wpn.sound {
        ctx.draws.events.sound(id, shot.pos);
    }
    if by_worm && let Some(w) = worms.iter_mut().find(|w| w.id == shot.worm) {
        let dir = vec_from_angle(shot.angle as f32);
        w.vel -= dir * wpn.recoil as f32;
    }
    let mut sctx = SpawnCtx {
        sim_mod: ctx.sim_mod,
        pool: ctx.pool,
        rand: &mut ctx.draws.rand,
        settings: ctx.settings,
        now: ctx.now,
        work: &mut ctx.draws.spawn_work.0,
        rot_bumped: &mut ctx.draws.rot_bumped,
    };
    spawn_info_apply(info, SpawnParent::Shot(shot), spawn, &mut sctx);
}

impl ShootList {
    /// Whether `s` (a received state) fits the list's capacity, with every
    /// shot `valid`.
    pub(crate) fn can_load(&self, s: &ShootList, valid: impl Fn(&Shot) -> bool) -> bool {
        s.shots.len() <= MAX_SHOOTINGS.min(self.shots.capacity()) && s.shots.iter().all(valid)
    }

    /// Takes `s`, which [`can_load`](Self::can_load) accepted, keeping the
    /// capacity.
    pub(crate) fn load(&mut self, s: ShootList) {
        self.shots.clear();
        self.shots.extend_from_slice(&s.shots);
        self.started = s.started;
    }
}

impl crate::checksum::Checksum for ShootList {
    fn hash(&self, h: &mut crate::checksum::Fnv) {
        let ShootList { shots, started } = self;
        shots.hash(h);
        h.bool(*started);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shot(t: u64, x: f32, vx: f32, angle: i32, random: i32, speed: i32) -> Shot {
        Shot {
            worm: 0,
            weapon: 26,
            pos: Vec2::new(x, 384.7),
            worm_vel: Vec2::new(vx, 1.9),
            angle,
            random,
            speed,
            time: TimeDiff(t),
            ..Shot::default()
        }
    }

    #[test]
    fn shot_round_trip_truncates_like_olx() {
        // One shot: positions and velocities truncated toward zero, the
        // angle and seed exact, the time through `(float)ms * 0.001f * 1000.0f`.
        let s = shot(30, 118.9, -6.7, 359, 231, 110);
        let d: Vec<_> = encode_decode(&[s], 37).collect();
        assert_eq!(d.len(), 1);
        assert_eq!((d[0].pos, d[0].worm_vel), (Vec2::new(118.0, 384.0), Vec2::new(-6.0, 1.0)));
        assert_eq!((d[0].angle, d[0].random, d[0].speed, d[0].time), (359, 231, 110, TimeDiff(30)));

        // A run by one worm and weapon goes out as S2C_MULTISHOOT: the later
        // shots keep the first decoded position and seed, a velocity increase
        // is lost, a decrease is kept, and the angle is exact.
        let list = [
            shot(550, 119.5, 29.9, 10, 136, 3590),
            shot(560, 125.2, 33.4, 12, 26, 3360),
            shot(570, 126.0, 30.1, 9, 77, 3300),
        ];
        let d: Vec<_> = encode_decode(&list, 37).collect();
        assert_eq!(d.len(), 3);
        for x in &d {
            assert_eq!((x.pos, x.random), (Vec2::new(119.0, 384.0), 136));
        }
        assert_eq!([d[0].time, d[1].time, d[2].time], [TimeDiff(550), TimeDiff(560), TimeDiff(570)]);
        assert_eq!([d[0].angle, d[1].angle, d[2].angle], [10, 12, 9]);
        assert_eq!([d[0].worm_vel.x, d[1].worm_vel.x, d[2].worm_vel.x], [29.0, 29.0, 26.0]);

        // nSpeed: the header byte keeps it modulo 256 above 255 + 255
        // (3590 - 255 = 3335 -> byte 7 -> 262). In a small shot the misplaced
        // velocity flag adds a second, wrapped speed byte: 262 + (256 - 230) - 230.
        assert_eq!(d[0].speed, 262);
        assert_eq!(d[1].speed, 262 + 26 - 230);

        // A speed change above 255 breaks the run: separate packets, own seeds.
        let list = [shot(580, 119.0, 30.0, 0, 166, 3000), shot(590, 120.0, 27.0, 0, 211, 2700)];
        let d: Vec<_> = encode_decode(&list, 37).collect();
        assert_eq!([d[0].random, d[1].random], [166, 211]);
        assert_eq!(d[1].pos.x, 120.0);

        // The weapon id is clamped to the weapons the client has.
        let d: Vec<_> = encode_decode(&[shot(0, 1.0, 0.0, 0, 0, 0)], 20).collect();
        assert_eq!(d[0].weapon, 20);
    }

    #[test]
    fn spawn_time_round_trip_matches_olx_at_large_frames() {
        // Review Focus 5: the float round trip of fTime is exact below
        // serverFrame 819205 and one millisecond late at 819205.
        let decode = |sf: u64| encode_decode(&[shot(sf * 10, 0.0, 0.0, 0, 0, 0)], 37).next().unwrap().time;
        assert_eq!(decode(819204), TimeDiff(8_192_040));
        assert_eq!(decode(819205), TimeDiff(8_192_051));
        assert_eq!(decode(3), TimeDiff(30));

        // Model R: a shot fired in frame F (serverFrame F+1) is delivered after
        // frame F+1, at serverFrame F+2, with currentTime = simulationTime + 10.
        let now = |sf: u64| AbsTime(100_000 + sf * 10 + 10);
        assert_eq!(spawn_time(decode(819204), 819205, now(819205)), AbsTime(now(819205).0 - 10));
        assert_eq!(spawn_time(decode(819205), 819206, now(819206)), AbsTime(now(819206).0 - 9));
        // The second shot of a list is as old as the server time. From 819205 on
        // its decoded time can be in the future, and the ping fallback (ping 0)
        // gives currentTime.
        assert_eq!(decode(819205).0, 819205 * 10 + 1);
        assert_eq!(spawn_time(decode(819205), 819205, now(819205)), now(819205));
        assert_eq!(spawn_time(decode(4), 4, now(4)), now(4));
    }

    #[test]
    fn a_list_is_sent_after_the_frame_after_it_started() {
        let mut list = ShootList::new();
        let mut rand = GlibcRand::new(1);
        let w = Worm::spawned(0, Vec2::new(10.0, 20.0), AbsTime(0), &crate::SimSettings::lx56());
        assert!(list.take_due().is_none());
        list.add(1, TimeDiff(10), 0.0, 0, &w, true, &mut rand);
        assert!(list.take_due().is_none(), "started this frame");
        list.add(1, TimeDiff(20), 0.0, 0, &w, true, &mut rand);
        assert_eq!(list.take_due().map(|s| s.len()), Some(2));
        assert_eq!(rand.calls(), 2);
    }
}
