//! The final-input table (networking spec §3.2): what the host forwarded for
//! each (tick, slot), and the predictions made from it (§3.3).

use alloc::vec::Vec;

/// What [`InputLog::set`] did with a value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetResult {
    /// Stored: the slot's next final input.
    New,
    /// Already final with this value.
    Same,
    /// Already final with another value; the first value stays.
    Conflict,
    /// Beyond the slot's known prefix: not stored.
    Gap,
}

/// One value per slot per tick from tick 0, final once set. Every slot's
/// knowledge is a contiguous prefix of ticks (`known_upto`); the frontier is
/// the smallest prefix: every tick below it is confirmed.
#[derive(Clone, Debug)]
pub struct InputLog {
    slots: u8,
    rows: Vec<[u8; 32]>,
    known: Vec<u32>,
    upto: [u32; 32],
}

impl InputLog {
    /// A log for `slots` slots (at most 32; more are cut to 32).
    pub fn new(slots: u8) -> Self {
        Self { slots: slots.min(32), rows: Vec::new(), known: Vec::new(), upto: [0; 32] }
    }

    pub fn slots(&self) -> u8 {
        self.slots
    }

    /// The final value of `slot` at `tick`, if known.
    pub fn get(&self, tick: u32, slot: u8) -> Option<u8> {
        if slot >= self.slots || tick >= self.upto[slot as usize] {
            return None;
        }
        Some(self.rows[tick as usize][slot as usize])
    }

    /// Records `v` as the final value of `slot` at `tick`: only the slot's
    /// next unknown tick is stored ([`SetResult::Gap`] beyond it).
    pub fn set(&mut self, tick: u32, slot: u8, v: u8) -> SetResult {
        if slot >= self.slots {
            return SetResult::Gap;
        }
        let s = slot as usize;
        let upto = self.upto[s];
        if tick < upto {
            return if self.rows[tick as usize][s] == v { SetResult::Same } else { SetResult::Conflict };
        }
        if tick > upto {
            return SetResult::Gap;
        }
        let t = tick as usize;
        if self.rows.len() <= t {
            self.rows.push([0; 32]);
            self.known.push(0);
        }
        self.rows[t][s] = v;
        self.known[t] |= 1 << s;
        self.upto[s] = upto + 1;
        SetResult::New
    }

    /// The first tick `slot` lacks.
    pub fn known_upto(&self, slot: u8) -> u32 {
        if slot >= self.slots { 0 } else { self.upto[slot as usize] }
    }

    /// The first tick some slot lacks: every tick below it is confirmed.
    pub fn frontier(&self) -> u32 {
        self.upto[..self.slots as usize].iter().copied().min().unwrap_or(0)
    }

    /// The highest prefix over all slots: the first tick nobody's input is known for.
    pub fn horizon(&self) -> u32 {
        self.upto[..self.slots as usize].iter().copied().max().unwrap_or(0)
    }

    /// Whether every slot's final value is known at `tick`.
    pub fn complete(&self, tick: u32) -> bool {
        tick < self.frontier()
    }

    /// The keys to assume for `slot` at `tick`: the final value, else the
    /// slot's last final value before it, else `0` (spec §3.3).
    pub fn predicted(&self, tick: u32, slot: u8) -> u8 {
        if let Some(v) = self.get(tick, slot) {
            return v;
        }
        match self.known_upto(slot) {
            0 => 0,
            upto => self.rows[upto as usize - 1][slot as usize],
        }
    }

    /// Every slot's final value at a confirmed `tick` (`None` at or above the frontier).
    pub fn row(&self, tick: u32) -> Option<[u8; 32]> {
        (tick < self.frontier()).then(|| self.rows[tick as usize])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knowledge_is_a_prefix_and_final() {
        let mut log = InputLog::new(2);
        assert_eq!(log.set(1, 0, 5), SetResult::Gap);
        assert_eq!(log.set(0, 0, 5), SetResult::New);
        assert_eq!(log.set(1, 0, 6), SetResult::New);
        assert_eq!(log.set(1, 0, 6), SetResult::Same);
        assert_eq!(log.set(1, 0, 7), SetResult::Conflict);
        assert_eq!(log.known_upto(0), 2);
        assert_eq!(log.frontier(), 0);
        log.set(0, 1, 1);
        assert_eq!(log.frontier(), 1);
        assert_eq!(
            log.row(0),
            Some({
                let mut r = [0; 32];
                r[0] = 5;
                r[1] = 1;
                r
            })
        );
        assert_eq!(log.row(1), None);
    }

    #[test]
    fn prediction_repeats_the_last_final_input() {
        let mut log = InputLog::new(1);
        assert_eq!(log.predicted(3, 0), 0);
        log.set(0, 0, 9);
        log.set(1, 0, 4);
        assert_eq!(log.predicted(1, 0), 4);
        assert_eq!(log.predicted(7, 0), 4);
    }
}
