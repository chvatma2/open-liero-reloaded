//! Whose rope throws are heard. OLX plays the throw sound
//! (`sfxGame.smpNinja`) in the human input handler only, right after the
//! local player's own throw, and never from the rope itself: a bot's
//! throw (`CWormBotInputHandler` calls `CNinjaRope::Shoot` alone) and a
//! worm's across the network (its rope comes in the worm's state) make no
//! sound. The simulation emits a `THROW` sound for every worm's throw
//! (`olr-sim` marks no owner on a sound), so the session keeps only as many
//! as the player's own rope was thrown: they all play at the listener
//! (`olr_audio::is_global`), so which of them stay makes no difference.
//!
//! OLX: src/common/CWormHuman.cpp:340-342, :555-563, :570-574 (the only
//! `PlaySoundSample(sfxGame.smpNinja)` calls), src/common/CWormBot.cpp:1753,
//! :3823 (the bot's throws, silent), src/common/PhysicsLX56.cpp:300-303
//! (only a local worm reads its input), src/sound/Sounds.cpp:201-209.

use olr_sim::{Event, EventKind, SoundId, World};

/// Keeps the rope throw sounds of one worm, the player's: the local slot,
/// or a replay's viewer (whose game it was); none when nobody here plays.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThrowSounds {
    worm: Option<u8>,
    /// Its rope's throws ([`olr_sim::NinjaRope::throws`]) last seen.
    seen: u32,
}

impl ThrowSounds {
    /// The throws of `worm` heard from `world` on.
    pub fn new(worm: Option<u8>, world: &World) -> ThrowSounds {
        let mut t = ThrowSounds { worm, seen: 0 };
        t.seen = t.thrown(world);
        t
    }

    fn thrown(&self, world: &World) -> u32 {
        self.worm.and_then(|w| world.worms().get(usize::from(w))).map_or(0, |w| w.rope.throws())
    }

    /// Drops from `sounds` (the `Sound` events of the ticks that led to
    /// `world`) the rope throws that are not the worm's: of the `THROW`
    /// sounds, as many stay as its rope was thrown since the last call,
    /// the first ones; every other sound stays. Called for every tick or
    /// poll, played or not, so a throw is never heard late. A rollback
    /// that takes throws back (fewer than seen) plays none.
    pub fn retain(&mut self, world: &World, sounds: &mut Vec<Event>) {
        let now = self.thrown(world);
        self.keep(now, sounds);
    }

    /// [`ThrowSounds::retain`] with the worm's throws `now`.
    fn keep(&mut self, now: u32, sounds: &mut Vec<Event>) {
        let mut own = now.saturating_sub(self.seen);
        self.seen = now;
        sounds.retain(|e| match e.kind {
            EventKind::Sound { id: SoundId::THROW, .. } if own > 0 => {
                own -= 1;
                true
            }
            EventKind::Sound { id: SoundId::THROW, .. } => false,
            _ => true,
        });
    }
}

#[cfg(test)]
mod tests {
    use olr_math::Vec2;

    use super::*;

    fn sound(id: SoundId) -> Event {
        Event { substep: 0, death: false, kind: EventKind::Sound { id, pos: Vec2::ZERO } }
    }

    #[test]
    fn keeps_as_many_throws_as_the_worm_threw() {
        let mut t = ThrowSounds { worm: Some(1), seen: 2 };
        let (throw, bump) = (sound(SoundId::THROW), sound(SoundId::BUMP));
        let mut sounds = vec![throw, bump, throw, throw];
        t.keep(3, &mut sounds);
        assert_eq!(sounds, [throw, bump], "one throw of the worm's, the bump");
        let mut sounds = vec![throw, throw];
        t.keep(3, &mut sounds);
        assert!(sounds.is_empty(), "others' throws are not heard");
        let mut sounds = vec![throw];
        t.keep(1, &mut sounds);
        assert!(sounds.is_empty(), "a rollback took throws back");
        let mut sounds = vec![throw];
        t.keep(2, &mut sounds);
        assert_eq!(sounds, [throw], "counted from the rollback's");
        let mut sounds = vec![bump];
        t.keep(4, &mut sounds);
        assert_eq!(sounds, [bump], "no throw to keep");
    }

    #[test]
    fn nobody_hears_no_throw() {
        let mut t = ThrowSounds { worm: None, seen: 0 };
        let mut sounds = vec![sound(SoundId::THROW), sound(SoundId::PICKUP)];
        t.keep(0, &mut sounds);
        assert_eq!(sounds, [sound(SoundId::PICKUP)]);
    }
}
