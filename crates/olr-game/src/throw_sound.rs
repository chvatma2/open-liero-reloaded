//! Whose rope throws are heard. OLX plays the throw sound
//! (`sfxGame.smpNinja`) in the human input handler only, right after the
//! local player's own throw, and never from the rope itself: a bot's
//! throw (`CWormBotInputHandler` calls `CNinjaRope::Shoot` alone) and a
//! worm's across the network (its rope comes in the worm's state) make no
//! sound. The simulation emits a `THROW` sound for every worm's throw
//! (`olr-sim` marks no owner on a sound), so the session keeps only as many
//! as the players' own ropes were thrown (each local player's, issue #29):
//! they all play at the listener (`olr_audio::is_global`), so which of them
//! stay makes no difference.
//!
//! OLX: src/common/CWormHuman.cpp:340-342, :555-563, :570-574 (the only
//! `PlaySoundSample(sfxGame.smpNinja)` calls), src/common/CWormBot.cpp:1753,
//! :3823 (the bot's throws, silent), src/common/PhysicsLX56.cpp:300-303
//! (only a local worm reads its input), src/sound/Sounds.cpp:201-209.

use olr_sim::{Event, EventKind, SoundId, World};

/// Keeps the rope throw sounds of the players' worms: the local slots
/// (several in split screen, issue #29: each player hears its own throws),
/// or a replay's viewers (whose game it was); none when nobody here plays.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThrowSounds {
    /// Each worm, and its rope's throws ([`olr_sim::NinjaRope::throws`])
    /// last seen.
    worms: Vec<(u8, u32)>,
}

impl ThrowSounds {
    /// The throws of `worms` heard from `world` on.
    pub fn new(worms: &[u8], world: &World) -> ThrowSounds {
        let worms = worms.iter().map(|&w| (w, thrown(world, w))).collect();
        ThrowSounds { worms }
    }

    /// Drops from `sounds` (the `Sound` events of the ticks that led to
    /// `world`) the rope throws that are not the worms': of the `THROW`
    /// sounds, as many stay as their ropes were thrown since the last call,
    /// the first ones; every other sound stays. Called for every tick or
    /// poll, played or not, so a throw is never heard late. A rollback
    /// that takes a worm's throws back (fewer than seen) plays none of its.
    pub fn retain(&mut self, world: &World, sounds: &mut Vec<Event>) {
        let mut own = 0;
        for (w, seen) in &mut self.worms {
            let now = thrown(world, *w);
            own += now.saturating_sub(*seen);
            *seen = now;
        }
        keep(own, sounds);
    }
}

/// Worm `worm`'s rope throws in `world` (0 for no such worm).
fn thrown(world: &World, worm: u8) -> u32 {
    world.worms().get(usize::from(worm)).map_or(0, |w| w.rope.throws())
}

/// Keeps the first `own` `THROW` sounds of `sounds` and every other sound.
fn keep(mut own: u32, sounds: &mut Vec<Event>) {
    sounds.retain(|e| match e.kind {
        EventKind::Sound { id: SoundId::THROW, .. } if own > 0 => {
            own -= 1;
            true
        }
        EventKind::Sound { id: SoundId::THROW, .. } => false,
        _ => true,
    });
}

#[cfg(test)]
mod tests {
    use olr_math::Vec2;

    use super::*;

    fn sound(id: SoundId) -> Event {
        Event { substep: 0, death: false, kind: EventKind::Sound { id, pos: Vec2::ZERO } }
    }

    #[test]
    fn keeps_as_many_throws_as_the_worms_threw() {
        let (throw, bump) = (sound(SoundId::THROW), sound(SoundId::BUMP));
        let mut sounds = vec![throw, bump, throw, throw];
        keep(1, &mut sounds);
        assert_eq!(sounds, [throw, bump], "one throw of the worms', the bump");
        let mut sounds = vec![throw, throw];
        keep(0, &mut sounds);
        assert!(sounds.is_empty(), "others' throws are not heard");
        let mut sounds = vec![throw, throw, throw];
        keep(2, &mut sounds);
        assert_eq!(sounds, [throw, throw], "two players' throws (issue #29)");
        let mut sounds = vec![bump];
        keep(4, &mut sounds);
        assert_eq!(sounds, [bump], "no throw to keep");
    }

    #[test]
    fn nobody_hears_no_throw() {
        let mut sounds = vec![sound(SoundId::THROW), sound(SoundId::PICKUP)];
        keep(0, &mut sounds);
        assert_eq!(sounds, [sound(SoundId::PICKUP)]);
    }
}
