//! Worm state (OLX `CWorm`, `src/game/CWorm.h`), only what the simulation uses.

use alloc::vec::Vec;

use rl_math::{AbsTime, PI_F32, Vec2};

use crate::SimSettings;
use crate::damage::WRM_UNLIM;
use crate::rope::NinjaRope;

/// OLX `DIR_TYPE`: left is 0, right is 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Dir {
    Left = 0,
    Right = 1,
}

/// OLX `worm_state_t`: what the worm does this sub-step, derived from input.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WormState {
    pub shoot: bool,
    pub carve: bool,
    pub moving: bool,
    pub jump: bool,
}

impl WormState {
    /// `worm_state_t::asInt` (`src/common/CWorm.cpp:62-69`): shoot 1, carve 2, move 4, jump 8.
    pub fn as_int(&self) -> u8 {
        self.shoot as u8 | (self.carve as u8) << 1 | (self.moving as u8) << 2 | (self.jump as u8) << 3
    }
}

/// OLX `wpnslot_t`.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Slot {
    pub weapon: i32,
    pub charge: f32,
    pub reloading: bool,
    pub last_fire: f32,
}

impl Slot {
    /// A slot holding `weapon`, full and ready (OLX's `simSetWeapons`, and
    /// `GameServer::PrepareWorm`'s selection).
    pub fn ready(weapon: i32) -> Slot {
        Slot { weapon, charge: 1.0, reloading: false, last_fire: 0.0 }
    }
}

/// What a worm starts a match with: OLX's weapon selection and the
/// `CWorm::Prepare` counters.
#[derive(Clone, Debug, PartialEq)]
pub struct Loadout {
    pub slots: Vec<Slot>,
    pub current_weapon: i32,
    pub lives: i32,
    pub kills: i32,
    pub deaths: i32,
}

impl Loadout {
    /// `slots` with the counters a game's start gives (`Game.cpp:845-890`):
    /// the first slot current, the `Lives` setting (unlimited below 0), no
    /// kills or deaths.
    pub fn new(slots: Vec<Slot>, s: &SimSettings) -> Loadout {
        let lives = if s.lives < 0 { WRM_UNLIM } else { s.lives };
        Loadout { slots, current_weapon: 0, lives, kills: 0, deaths: 0 }
    }
}

/// `VecTimeRecorder`, OLX's `DataTimeRecorder<CVec, 100>`
/// (`src/util/VecTimeRecorder.h:21-62`): the worm's position after each
/// physics step, which projectile hit tests and explosions read.
///
/// `cur` is the next slot to write, so OLX's `get(0)` is the *oldest*
/// slot (100 pushes ago, or whatever an earlier life or the zeroed start
/// left there), not the newest. [`get`](Self::get) and
/// [`get_best`](Self::get_best) keep that quirk only with
/// [`SimSettings::olx_quirks`] (the oracle; spec §18 D7, amended
/// 2026-10-13); without, frames-back 0 is the newest entry, and play's
/// hit tests read it ([`hit_position`](Self::hit_position)).
///
/// Serialised as [`RecorderWire`] (serde has no impl for 100-element arrays).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(into = "RecorderWire", try_from = "RecorderWire")]
pub struct PosRecorder {
    data: [Vec2; PosRecorder::FRAMES],
    cur: usize,
    count: usize,
}

impl Default for PosRecorder {
    fn default() -> Self {
        Self { data: [Vec2::ZERO; Self::FRAMES], cur: 0, count: 0 }
    }
}

impl PosRecorder {
    const FRAMES: usize = 100;

    /// `clear()`: forgets the entries but keeps their contents.
    pub fn clear(&mut self) {
        self.count = 0;
    }

    /// `clear(v)`: `clear()`, then `push_back(v)`.
    pub fn clear_to(&mut self, v: Vec2) {
        self.clear();
        self.push(v);
    }

    /// `CWorm::Spawn`'s `clear(vPos)` (`src/common/CWorm.cpp:611`). With
    /// `olx_quirks` the old life's entries stay, for `get(0)`'s off-by-one
    /// to read; without, every slot becomes `v`, so no read can reach a
    /// position of the previous life.
    pub fn respawn_to(&mut self, v: Vec2, olx_quirks: bool) {
        if !olx_quirks {
            self.data = [v; Self::FRAMES];
        }
        self.clear_to(v);
    }

    /// `push_back`.
    pub fn push(&mut self, v: Vec2) {
        self.data[self.cur] = v;
        self.cur = (self.cur + 1) % Self::FRAMES;
        if self.count < Self::FRAMES {
            self.count += 1;
        }
    }

    /// `get(framesBack)`, `frames_back` pushes before the newest entry.
    /// With `olx_quirks` OLX's index, `m_data[(cur + 100 - framesBack %
    /// 100) % 100]`, which is one slot too far back: `get(0)` is the
    /// oldest slot (spec §18 D7). Without, `m_data[(cur + 100 - 1 -
    /// framesBack % 100) % 100]`: `get(0)` is the newest.
    pub fn get(&self, frames_back: usize, olx_quirks: bool) -> Vec2 {
        let back = frames_back % Self::FRAMES + usize::from(!olx_quirks);
        self.data[(self.cur + 2 * Self::FRAMES - back) % Self::FRAMES]
    }

    /// Where a projectile hit test (`ProjWormColl`) or an explosion sees
    /// the worm. With `olx_quirks`, OLX's `getBest(msPerFrame, msBack)`,
    /// `msBack` measured from the wall clock pinned 10 ms ahead (spec §18
    /// D2), so frames-back is mostly 1 and sometimes 0, which the
    /// off-by-one turns into the newest entry or the oldest. Without, the
    /// newest entry: the worm's current position, which the projectile's
    /// step in the same sub-step follows (OLX's frames-back would read the
    /// entry a sub-step older most of the time, a pixel or two off).
    pub fn hit_position(&self, ms_per_frame: u64, ms_back: u64, olx_quirks: bool) -> Vec2 {
        if olx_quirks { self.get_best(ms_per_frame, ms_back, true) } else { self.get(0, false) }
    }

    /// Whether the indices are ones `push` and `clear` reach, so `get`
    /// stays in the buffer, and every entry (a worm's position, or zero)
    /// is finite (for a received state).
    pub(crate) fn is_valid(&self) -> bool {
        self.cur < Self::FRAMES
            && self.count <= Self::FRAMES
            && self.data.iter().all(|v| v.x.is_finite() && v.y.is_finite())
    }

    /// `getBest(millisecsPerFrame, millisecsBack)`.
    pub fn get_best(&self, ms_per_frame: u64, ms_back: u64, olx_quirks: bool) -> Vec2 {
        let frames_back = (ms_back / ms_per_frame.max(1)).min(usize::MAX as u64) as usize;
        if frames_back < self.count {
            self.get(frames_back, olx_quirks)
        } else if self.count > 0 {
            self.get(self.count - 1, olx_quirks)
        } else {
            self.data[(self.cur + Self::FRAMES - 1) % Self::FRAMES]
        }
    }
}

/// [`PosRecorder`] on the wire: the entries as a list of exactly 100.
#[derive(serde::Serialize, serde::Deserialize)]
struct RecorderWire {
    data: Vec<Vec2>,
    cur: u64,
    count: u64,
}

impl From<PosRecorder> for RecorderWire {
    fn from(r: PosRecorder) -> Self {
        RecorderWire { data: r.data.to_vec(), cur: r.cur as u64, count: r.count as u64 }
    }
}

impl TryFrom<RecorderWire> for PosRecorder {
    type Error = &'static str;

    fn try_from(w: RecorderWire) -> Result<Self, Self::Error> {
        let data = w.data.try_into().map_err(|_| "a position recorder has 100 entries")?;
        let cur = usize::try_from(w.cur).map_err(|_| "a recorder index fits a usize")?;
        let count = usize::try_from(w.count).map_err(|_| "a recorder count fits a usize")?;
        Ok(PosRecorder { data, cur, count })
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Worm {
    pub id: i32,
    /// Its team ([`crate::WormSetup::team`]); Deathmatch has none.
    pub team: u8,
    pub alive: bool,
    pub pos: Vec2,
    pub vel: Vec2,
    pub health: f32,
    /// Aim angle in degrees: negative is up.
    pub angle: f32,
    pub angle_speed: f32,
    pub face: Dir,
    pub move_dir: Dir,
    pub on_ground: bool,
    pub state: WormState,
    pub current_weapon: i32,
    pub slots: Vec<Slot>,
    pub rope: NinjaRope,
    pub lives: i32,
    pub kills: i32,
    pub deaths: i32,
    pub suicides: i32,
    /// `fDamage`, the damage score (client-side count only, `docs/porting/STATUS.md`).
    pub damage: f32,
    /// `fDamageFactor`: multiplies the damage this worm deals.
    pub damage_factor: f32,
    /// `fShieldFactor`: divides the damage this worm takes; 0 makes every hit deadly.
    pub shield_factor: f32,
    /// `fTimeofDeath`.
    pub time_of_death: AbsTime,
    /// `fLastBlood`: when the worm last bled. Never reset.
    pub last_blood: AbsTime,
    /// `bSpawnedOnce`.
    pub spawned_once: bool,
    /// `bCanRespawnNow`: the server waits for the worm's jump key.
    pub can_respawn_now: bool,
    /// `bRespawnRequested`.
    pub respawn_requested: bool,
    pub dirt_count: i32,
    /// Animation frame (0..3), only for rendering.
    pub frame: f32,
    pub speed_factor: f32,
    pub can_use_ninja: bool,
    pub can_air_jump: bool,
    pub last_input_time: AbsTime,
    pub last_air_jump_time: AbsTime,
    /// `posRecordings`.
    pub pos_recorder: PosRecorder,
}

impl Worm {
    /// A worm as `CWorm::Prepare` and `CWorm::Spawn` leave it
    /// (`src/common/CWorm.cpp:300-330`, `591-650`; `fTimeofDeath` is set
    /// by `Prepare`, `fLastBlood` by the constructor), with the lives the game's
    /// start gives (`Lives`), except the carving, which [`crate::World::spawn_worm`] does.
    pub fn spawned(id: i32, pos: Vec2, now: AbsTime, s: &SimSettings) -> Self {
        // Prepare's `clear()`, then Spawn's `clear(vPos)` (`CWorm.cpp:398`, `611`).
        let mut pos_recorder = PosRecorder::default();
        pos_recorder.clear();
        pos_recorder.clear_to(pos);
        Self {
            id,
            team: 0,
            alive: true,
            pos,
            vel: Vec2::ZERO,
            health: 100.0,
            angle: 0.0,
            angle_speed: 0.0,
            face: Dir::Right,
            move_dir: Dir::Right,
            on_ground: false,
            state: WormState::default(),
            current_weapon: 0,
            slots: Vec::new(),
            rope: NinjaRope::default(),
            // `Game.cpp:851`, `CServer.cpp:1712`: at the start of the game.
            lives: if s.lives < 0 { WRM_UNLIM } else { s.lives },
            kills: 0,
            deaths: 0,
            suicides: 0,
            damage: 0.0,
            damage_factor: s.worm_damage_factor,
            shield_factor: s.worm_shield_factor,
            time_of_death: now,
            last_blood: AbsTime(0),
            spawned_once: true,
            can_respawn_now: false,
            respawn_requested: false,
            dirt_count: 0,
            frame: 0.0,
            speed_factor: s.worm_speed_factor,
            can_use_ninja: true,
            can_air_jump: s.instant_air_jump,
            last_input_time: now,
            last_air_jump_time: AbsTime(0),
            pos_recorder,
        }
    }

    /// `CWorm::getCurWeapon`/`writeCurWeapon` (`src/game/CWorm.h:539-549`): the
    /// current slot, or the last one if the index is past the end. Panics
    /// if the worm has no slots; callers check first, as OLX does.
    pub fn current_slot(&self) -> &Slot {
        &self.slots[self.current_slot_index()]
    }

    pub fn current_slot_mut(&mut self) -> &mut Slot {
        let i = self.current_slot_index();
        &mut self.slots[i]
    }

    /// `MIN(tWeapons.size()-1, (size_t)iCurrentWeapon)`: a negative index
    /// becomes a huge `size_t`, so it picks the last slot too.
    pub(crate) fn current_slot_index(&self) -> usize {
        (self.current_weapon as usize).min(self.slots.len().saturating_sub(1))
    }

    fn direction(&self, side: Dir) -> Vec2 {
        let rad = self.angle * (PI_F32 / 180.0);
        let sx = if side == Dir::Left { -1.0 } else { 1.0 };
        Vec2::new(rl_math::cosf(rad) * sx, rl_math::sinf(rad))
    }

    /// `CWorm::getFaceDirection` (`src/game/CWorm.h:514-516`).
    pub fn face_direction(&self) -> Vec2 {
        self.direction(self.face)
    }

    /// `CWorm::getMoveDirection` (`src/game/CWorm.h:518-520`).
    pub fn move_direction(&self) -> Vec2 {
        self.direction(self.move_dir)
    }
}

impl crate::checksum::Checksum for PosRecorder {
    fn hash(&self, h: &mut crate::checksum::Fnv) {
        let PosRecorder { data, cur, count } = self;
        data.iter().for_each(|&p| h.vec2(p));
        h.usize(*cur);
        h.usize(*count);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawned_worms_start_with_the_lives_setting() {
        // `Game.cpp:851`: `FT_Lives < 0 ? WRM_UNLIM : FT_Lives`.
        let mut s = SimSettings::lx56();
        for (lives, expected) in [(-2, -2), (-1, -2), (0, 0), (5, 5)] {
            s.lives = lives;
            assert_eq!(Worm::spawned(0, Vec2::ZERO, AbsTime(0), &s).lives, expected, "Lives {lives}");
        }
    }

    #[test]
    fn vec_time_recorder_get0_returns_the_oldest() {
        let mut r = PosRecorder::default();
        r.clear_to(Vec2::new(1.0, 0.0));
        // One push: get(0) is the slot about to be written, still (0, 0).
        assert_eq!((r.get(0, true), r.get(1, true)), (Vec2::ZERO, Vec2::new(1.0, 0.0)));
        for i in 2..=100 {
            r.push(Vec2::new(i as f32, 0.0));
        }
        // Full: get(0) is the oldest entry, 100 pushes back; get(1) the newest.
        assert_eq!(
            (r.get(0, true), r.get(1, true), r.get(99, true)),
            (Vec2::new(1.0, 0.0), Vec2::new(100.0, 0.0), Vec2::new(2.0, 0.0))
        );
        // getBest: 0-10 ms back is get(0), 11-21 ms get(1); past the entries, the oldest.
        assert_eq!(r.get_best(11, 10, true), Vec2::new(1.0, 0.0));
        assert_eq!(r.get_best(11, 11, true), Vec2::new(100.0, 0.0));
        assert_eq!(r.get_best(11, 5000, true), r.get(99, true));
        // clear() keeps the contents: after a respawn get(0) is a stale position.
        r.respawn_to(Vec2::new(7.0, 0.0), true);
        assert_eq!((r.get(0, true), r.get(1, true)), (Vec2::new(2.0, 0.0), Vec2::new(7.0, 0.0)));
        assert_eq!(
            r.get_best(11, 30, true),
            Vec2::new(2.0, 0.0),
            "one entry: get(count - 1) is get(0), stale too"
        );
        r.clear();
        assert_eq!(r.get_best(11, 0, true), Vec2::new(7.0, 0.0), "no entries: the last written slot");
    }

    #[test]
    fn without_olx_quirks_get0_is_the_newest_and_a_respawn_forgets_the_old_life() {
        let mut r = PosRecorder::default();
        r.clear_to(Vec2::new(1.0, 0.0));
        assert_eq!(r.get(0, false), Vec2::new(1.0, 0.0), "one push: get(0) is it");
        for i in 2..=100 {
            r.push(Vec2::new(i as f32, 0.0));
        }
        // Full: get(k) is k pushes before the newest.
        assert_eq!(
            (r.get(0, false), r.get(1, false), r.get(99, false)),
            (Vec2::new(100.0, 0.0), Vec2::new(99.0, 0.0), Vec2::new(1.0, 0.0))
        );
        // getBest: 0-10 ms back is the newest, 11-21 ms one before; past the entries, the oldest.
        assert_eq!(r.get_best(11, 10, false), Vec2::new(100.0, 0.0));
        assert_eq!(r.get_best(11, 11, false), Vec2::new(99.0, 0.0));
        assert_eq!(r.get_best(11, 5000, false), Vec2::new(1.0, 0.0));
        // A respawn forgets the old life: every read is the spawn position.
        r.respawn_to(Vec2::new(7.0, 0.0), false);
        for back in [0, 1, 50, 99] {
            assert_eq!(r.get(back, false), Vec2::new(7.0, 0.0), "get({back})");
        }
        for ms in [0, 10, 30, 5000] {
            assert_eq!(r.get_best(11, ms, false), Vec2::new(7.0, 0.0), "getBest {ms} ms");
        }
        r.push(Vec2::new(8.0, 0.0));
        assert_eq!((r.get(0, false), r.get(1, false)), (Vec2::new(8.0, 0.0), Vec2::new(7.0, 0.0)));
        // Hit tests read the newest entry whatever the pinned clock says;
        // with the quirks, OLX's `getBest`.
        for ms in [0, 10, 11, 21, 5000] {
            assert_eq!(r.hit_position(11, ms, false), Vec2::new(8.0, 0.0), "{ms} ms");
            assert_eq!(r.hit_position(11, ms, true), r.get_best(11, ms, true), "{ms} ms");
        }
    }
}
