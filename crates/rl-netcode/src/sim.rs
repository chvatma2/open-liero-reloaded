use alloc::vec::Vec;

use rl_sim::Keys;

/// A rejected world state (networking spec §5.4: never panic, refuse).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StateRejected;

/// What the session drives. One tick takes one `Keys` per slot.
pub trait Simulation {
    type Snapshot;
    fn tick(&mut self, keys: &[Keys]);
    fn save(&mut self) -> Self::Snapshot;
    fn restore(&mut self, s: &Self::Snapshot);
    /// Snapshots older than `s` will not be restored again.
    fn forget_before(&mut self, s: &Self::Snapshot);
    /// Compared between peers every few confirmed ticks.
    fn checksum(&self) -> u64;
    fn encode_state(&self) -> Vec<u8>;
    fn decode_state(&mut self, bytes: &[u8]) -> Result<(), StateRejected>;
}

/// A tiny deterministic simulation for the session's own tests: each slot
/// walks by its keys, and a hash chains every tick's inputs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToySim {
    tick: u32,
    pos: Vec<i64>,
    hash: u64,
}

impl ToySim {
    pub fn new(slots: usize) -> Self {
        Self { tick: 0, pos: alloc::vec![0; slots], hash: 0xcbf2_9ce4_8422_2325 }
    }

    pub fn tick_count(&self) -> u32 {
        self.tick
    }

    /// Moves `slot` by `d` outside its inputs: a planted desync. A slot
    /// out of range changes nothing.
    pub fn displace(&mut self, slot: usize, d: i64) {
        if let Some(p) = self.pos.get_mut(slot) {
            *p += d;
        }
    }
}

impl Simulation for ToySim {
    type Snapshot = ToySim;

    fn tick(&mut self, keys: &[Keys]) {
        for (i, p) in self.pos.iter_mut().enumerate() {
            let k = keys.get(i).copied().unwrap_or_default().0;
            *p += (k & 0x0f) as i64 - (k >> 4) as i64;
            self.hash = (self.hash ^ (k as u64 | (i as u64) << 8)).wrapping_mul(0x0000_0100_0000_01b3);
        }
        self.hash = (self.hash ^ self.tick as u64).wrapping_mul(0x0000_0100_0000_01b3);
        self.tick += 1;
    }

    fn save(&mut self) -> ToySim {
        self.clone()
    }

    fn restore(&mut self, s: &ToySim) {
        self.clone_from(s);
    }

    fn forget_before(&mut self, _: &ToySim) {}

    fn checksum(&self) -> u64 {
        self.pos.iter().fold(self.hash, |h, &p| (h ^ p as u64).wrapping_mul(0x0000_0100_0000_01b3))
    }

    fn encode_state(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.tick.to_le_bytes());
        out.extend_from_slice(&self.hash.to_le_bytes());
        out.extend_from_slice(&(self.pos.len() as u32).to_le_bytes());
        for p in &self.pos {
            out.extend_from_slice(&p.to_le_bytes());
        }
        out
    }

    fn decode_state(&mut self, b: &[u8]) -> Result<(), StateRejected> {
        let n = self.pos.len();
        if b.len() != 16 + 8 * n || u32::from_le_bytes(b[12..16].try_into().unwrap()) as usize != n {
            return Err(StateRejected);
        }
        self.tick = u32::from_le_bytes(b[0..4].try_into().unwrap());
        self.hash = u64::from_le_bytes(b[4..12].try_into().unwrap());
        for (i, p) in self.pos.iter_mut().enumerate() {
            *p = i64::from_le_bytes(b[16 + 8 * i..24 + 8 * i].try_into().unwrap());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(v: u8) -> Keys {
        Keys(v)
    }

    #[test]
    fn toy_sim_restores_and_transfers() {
        let mut a = ToySim::new(3);
        a.tick(&[k(1), k(2), k(4)]);
        let s = a.save();
        let h = a.checksum();
        a.tick(&[k(8), k(8), k(8)]);
        assert_ne!(a.checksum(), h);
        a.restore(&s);
        assert_eq!(a.checksum(), h);
        let mut b = ToySim::new(3);
        b.decode_state(&a.encode_state()).unwrap();
        assert_eq!(b.checksum(), a.checksum());
        assert_eq!(b.decode_state(&[1, 2, 3]), Err(StateRejected));
    }

    #[test]
    fn toy_sim_depends_on_every_key_and_the_order() {
        let mut a = ToySim::new(2);
        let mut b = ToySim::new(2);
        a.tick(&[k(1), k(2)]);
        b.tick(&[k(2), k(1)]);
        assert_ne!(a.checksum(), b.checksum());
    }
}
