//! The bot's own random generator (spec §20 F7): every `rand()` OLX's bot
//! code draws becomes a draw here, never from the world's generators.

use rl_math::GlibcRand;

use crate::Personality;

/// A glibc `rand()` stream of the bot's own, with OLX's `GetRandom*` on top.
#[derive(Clone, Debug, PartialEq)]
pub struct BotRng(GlibcRand);

impl BotRng {
    /// The stream of the bot playing worm `worm` with `personality` in a match
    /// seeded `match_seed`: the three are mixed (SplitMix64) into one seed.
    pub fn new(match_seed: u32, worm: usize, personality: Personality) -> Self {
        let p: u64 = match personality {
            Personality::Deathmatch => 0,
            Personality::Chaos => 1,
        };
        let x = (u64::from(match_seed) << 32) ^ ((worm as u64) << 8) ^ p;
        Self::from_seed((splitmix64(x) >> 32) as u32)
    }

    /// `srand(seed)`.
    pub fn from_seed(seed: u32) -> Self {
        Self(GlibcRand::new(seed))
    }

    /// `rand()`: 0..=`RAND_MAX`.
    pub fn rand(&mut self) -> i32 {
        self.0.next()
    }

    /// `GetRandomInt(max)` (`src/common/MathLib.cpp:71-76`): 0..=`max`.
    pub fn random_int(&mut self, max: i32) -> i32 {
        self.0.random_int(max.max(0))
    }

    /// `GetRandomLong(max)` (`src/common/MathLib.cpp:78-81`): 0..=`max`.
    pub fn random_long(&mut self, max: u64) -> u64 {
        let f = self.0.random_pos_num() * (max as f32 + 1.0);
        (f as u64).min(max)
    }

    /// `GetRandomNum` (`MathLib.cpp:58-62`): -1..=1.
    pub fn random_num(&mut self) -> f32 {
        self.0.random_num()
    }

    /// `GetRandomPosNum` (`MathLib.cpp:64-67`): 0..=1.
    pub fn random_pos_num(&mut self) -> f32 {
        self.0.random_pos_num()
    }

    /// How many `rand()` calls the stream has made.
    pub fn calls(&self) -> u64 {
        self.0.calls()
    }
}

fn splitmix64(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streams_differ_by_seed_worm_and_personality() {
        let a = BotRng::new(1, 0, Personality::Deathmatch).rand();
        assert_eq!(a, BotRng::new(1, 0, Personality::Deathmatch).rand());
        assert_ne!(a, BotRng::new(2, 0, Personality::Deathmatch).rand());
        assert_ne!(a, BotRng::new(1, 1, Personality::Deathmatch).rand());
        assert_ne!(a, BotRng::new(1, 0, Personality::Chaos).rand());
    }

    #[test]
    fn random_long_stays_in_range() {
        let mut r = BotRng::from_seed(7);
        for max in [0, 1, 5, 37] {
            for _ in 0..200 {
                assert!(r.random_long(max) <= max);
            }
        }
    }
}
