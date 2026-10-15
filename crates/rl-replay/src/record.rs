//! Recording a match as it is played.

use alloc::vec;
use alloc::vec::Vec;

use rl_sim::{Keys, MAX_WORMS, MatchSetup, World};

use crate::play::apply_call;
use crate::{
    CHECKSUM_EVERY, MAX_SETUP_CALLS, MAX_TICKS, ModeSpec, Packs, RecordError, Replay, ReplaySetup, SetupCall,
};

/// Follows a match: its setup calls, the world's checksum before the first
/// tick, then every tick's keys, and the world's checksum every
/// [`CHECKSUM_EVERY`] ticks and after the last.
///
/// ```text
/// let mut world = World::new(setup.clone(), mode.game_mode(&setup.settings)?)?;
/// let mut rec = Recorder::new(packs, &setup, &mode);
/// rec.apply(&mut world, SetupCall::AlignPhysicsClock)?;   // instead of world.align_physics_clock()
/// rec.start(&world)?;                                    // after the last setup call
/// loop { world.tick(&keys, &mut out); rec.push(&keys, &world)?; }
/// let replay = rec.finish(&world)?;
/// ```
#[derive(Clone, Debug)]
pub struct Recorder {
    setup: ReplaySetup,
    mode: ModeSpec,
    ticks: u32,
    /// The world's server frame at [`Recorder::start`] (`None` before).
    base_frame: Option<u64>,
    /// Column `i` holds worm `i`'s keys, one per tick.
    columns: Vec<Vec<Keys>>,
    checksums: Vec<(u32, u64)>,
    withdrawals: Vec<(u32, u8)>,
}

impl Recorder {
    /// A recorder for the match `World::new(setup.clone(), mode.game_mode(&setup.settings)?)`
    /// starts, `setup` being made of `packs` (`rl_content::Content::replay_packs`
    /// gives them with their hashes). For a Deathmatch, `setup` is the one
    /// `Deathmatch::prepare` changed.
    pub fn new(packs: Packs, setup: &MatchSetup, mode: &ModeSpec) -> Self {
        Recorder {
            setup: ReplaySetup::new(packs, setup),
            mode: mode.clone(),
            ticks: 0,
            base_frame: None,
            columns: Vec::new(),
            checksums: Vec::new(),
            withdrawals: Vec::new(),
        }
    }

    /// Records the worm of the player whose game this is (`None`: nobody
    /// here plays, a bots-only match or an online spectator); a slot that
    /// is no worm of the setup is dropped.
    pub fn set_viewer(&mut self, slot: Option<u8>) {
        self.setup.viewer = slot.filter(|&s| usize::from(s) < self.setup.worms.len());
    }

    /// Makes `call` on `world` and records it: every change to the world
    /// before its first tick goes through here, or the replay diverges.
    /// Gives `find_spot`'s position.
    pub fn apply(&mut self, world: &mut World, call: SetupCall) -> Result<Option<(i32, i32)>, RecordError> {
        if self.base_frame.is_some() {
            return Err(RecordError::Started);
        }
        if self.setup.calls.len() >= MAX_SETUP_CALLS {
            return Err(RecordError::TooManyCalls);
        }
        let at = apply_call(world, self.setup.calls.len(), call).map_err(RecordError::Call)?;
        self.setup.calls.push(call);
        Ok(at)
    }

    /// Starts recording ticks: `world` is the match's world after its last
    /// setup call, before its first tick. Its checksum is recorded as tick
    /// 0's, so a replay whose setup or content differs is reported before
    /// any tick is played.
    pub fn start(&mut self, world: &World) -> Result<(), RecordError> {
        if self.base_frame.is_some() {
            return Err(RecordError::Started);
        }
        self.base_frame = Some(world.server_frame());
        self.checksums.push((0, world.checksum()));
        Ok(())
    }

    /// Records a tick: `keys` (as `World::tick` got them; keys from
    /// [`MAX_WORMS`] on are ignored, as the world ignores them) and `world`
    /// after it.
    pub fn push(&mut self, keys: &[Keys], world: &World) -> Result<(), RecordError> {
        if self.ticks >= MAX_TICKS {
            return Err(RecordError::Full);
        }
        let base = self.base_frame.ok_or(RecordError::NotStarted)?;
        let frame = world.server_frame();
        if frame.wrapping_sub(base) != u64::from(self.ticks) + 1 {
            return Err(RecordError::OutOfStep { recorded: self.ticks, frame });
        }
        let keys = &keys[..keys.len().min(MAX_WORMS)];
        while self.columns.len() < keys.len() {
            self.columns.push(vec![Keys(0); self.ticks as usize]);
        }
        for (i, column) in self.columns.iter_mut().enumerate() {
            column.push(keys.get(i).copied().unwrap_or(Keys(0)));
        }
        self.ticks += 1;
        if self.ticks.is_multiple_of(CHECKSUM_EVERY) {
            self.checksums.push((self.ticks, world.checksum()));
        }
        Ok(())
    }

    /// Withdraws `worm` from `world` before the next tick
    /// ([`World::withdraw`]) and records it. `RecordError::NotStarted`
    /// before [`Recorder::start`]; `RecordError::NoWorm` for a worm `world`
    /// does not have or has withdrawn already.
    pub fn withdraw(&mut self, world: &mut World, worm: u8) -> Result<(), RecordError> {
        if self.base_frame.is_none() {
            return Err(RecordError::NotStarted);
        }
        if usize::from(worm) >= world.worms().len() || world.gone() & (1 << worm) != 0 {
            return Err(RecordError::NoWorm(worm));
        }
        world.withdraw(usize::from(worm));
        self.withdrawals.push((self.ticks, worm));
        Ok(())
    }

    /// How many ticks are recorded.
    pub fn ticks(&self) -> u32 {
        self.ticks
    }

    /// The replay, ending with `world`'s checksum: `world` is the one after
    /// the last [`Recorder::push`] (a tick played since is an error). A
    /// failing match can be saved from a clone of the recorder.
    pub fn finish(self, world: &World) -> Result<Replay, RecordError> {
        let base = self.base_frame.ok_or(RecordError::NotStarted)?;
        let frame = world.server_frame();
        if frame.wrapping_sub(base) != u64::from(self.ticks) {
            return Err(RecordError::OutOfStep { recorded: self.ticks, frame });
        }
        let Recorder { setup, mode, ticks, columns, mut checksums, withdrawals, .. } = self;
        if checksums.last().is_none_or(|c| c.0 != ticks) {
            checksums.push((ticks, world.checksum()));
        }
        let width = columns.len();
        let mut keys = Vec::with_capacity(ticks as usize * width);
        for t in 0..ticks as usize {
            keys.extend(columns.iter().map(|c| c[t]));
        }
        Ok(Replay { setup, mode, checksums, withdrawals, ticks, width, keys })
    }

    /// The replay of a match whose next tick failed (spec §7.3): `world` is
    /// the one after the last [`Recorder::push`] (a panic inside
    /// `World::tick` leaves the world broken: rebuild it by replaying the
    /// recorded ticks), `failing` the keys of the tick that failed. That
    /// tick is recorded with no checksum after it, so playing the replay
    /// checks every tick before it and then plays it: the failure again.
    pub fn finish_failed(self, world: &World, failing: &[Keys]) -> Result<Replay, RecordError> {
        if self.ticks >= MAX_TICKS {
            return Err(RecordError::Full);
        }
        let mut r = self.finish(world)?;
        let failing = &failing[..failing.len().min(MAX_WORMS)];
        if failing.len() > r.width {
            let wide = failing.len();
            let mut keys = Vec::with_capacity((r.ticks as usize + 1) * wide);
            for t in 0..r.ticks {
                keys.extend_from_slice(r.row(t));
                keys.extend(core::iter::repeat_n(Keys(0), wide - r.width));
            }
            (r.keys, r.width) = (keys, wide);
        }
        r.keys.extend((0..r.width).map(|i| failing.get(i).copied().unwrap_or(Keys(0))));
        r.ticks += 1;
        Ok(r)
    }
}
