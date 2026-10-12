//! The harness's factory for [`ToySim`]: drivers press seeded pseudo-random
//! keys, holding each for a while like a player does.

use alloc::boxed::Box;

use rl_sim::Keys;

use crate::harness::Factory;
use crate::{Driver, PeerId, Rng, Seat, ToySim};

pub struct ToyFactory {
    pub slots: u8,
    pub seed: u64,
}

impl Factory<ToySim> for ToyFactory {
    fn sim(&mut self, _peer: PeerId) -> ToySim {
        ToySim::new(self.slots as usize)
    }

    fn driver(&mut self, peer: PeerId, _seat: Option<Seat>) -> Box<dyn Driver<ToySim>> {
        Box::new(ToyDriver { rng: Rng::new(self.seed ^ (peer as u64).wrapping_mul(0x9e37)), held: [0; 32] })
    }

    /// Every fault moves slot 0 by one.
    fn corrupt(&mut self, sim: &mut ToySim, _index: usize) {
        sim.displace(0, 1);
    }
}

/// Changes a slot's key with probability 1/8 per tick, otherwise holds it.
/// Reads nothing but its own generator.
struct ToyDriver {
    rng: Rng,
    held: [u8; 32],
}

impl Driver<ToySim> for ToyDriver {
    fn local_inputs(&mut self, _sim: &ToySim, _tick: u32, slots: &[u8], out: &mut [Keys]) {
        for (&s, k) in slots.iter().zip(out.iter_mut()) {
            let held = &mut self.held[s as usize % 32];
            if self.rng.below(8) == 0 {
                *held = self.rng.next_u64() as u8;
            }
            *k = Keys(*held);
        }
    }
}
