//! Keyboard input with OLX's latching (`src/client/CInput.cpp`).
//!
//! OLX reads keys once per physics sub-step (`getInput`), not once per frame,
//! and keeps two latches per key until that read ends with `CInput::reset()`:
//! - a key pressed since the last read counts as held (`isDown`: `wasDown() || bDown`);
//! - a key pressed since the last read counts as pressed once (`isDownOnce`: `nDownOnce`).
//!
//! So a tap shorter than the gap between two sub-steps is still seen.

/// The keys a player holds during a frame. Bits match `olr_oracle::Keys`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Keys(pub u16);

impl Keys {
    pub const UP: u16 = 1;
    pub const DOWN: u16 = 2;
    pub const LEFT: u16 = 4;
    pub const RIGHT: u16 = 8;
    pub const JUMP: u16 = 16;
    pub const FIRE: u16 = 32;
    pub const ROPE: u16 = 64;
    pub const CHANGE: u16 = 128;
    /// OLX's Strafe (`SIN_STRAFE`): held, Left and Right move without
    /// turning the worm.
    pub const STRAFE: u16 = 256;
    /// OLX's PreviousWeapon (`SIN_PREVWEAPON`): a press selects the previous slot.
    pub const PREV_WEAPON: u16 = 512;
    /// OLX's NextWeapon (`SIN_NEXTWEAPON`): a press selects the next slot.
    pub const NEXT_WEAPON: u16 = 1024;
    /// OLX's Weapon1 (`SIN_WEAPON1`): held, selects the first slot.
    pub const WEAPON1: u16 = 1 << 11;
    /// OLX's Weapon2 (`SIN_WEAPON2`): held, selects the second slot.
    pub const WEAPON2: u16 = 1 << 12;
    /// OLX's Weapon3 (`SIN_WEAPON3`): held, selects the third slot.
    pub const WEAPON3: u16 = 1 << 13;
    /// OLX's Weapon4 (`SIN_WEAPON4`): held, selects the fourth slot.
    pub const WEAPON4: u16 = 1 << 14;
    /// OLX's Weapon5 (`SIN_WEAPON5`): held, selects the fifth slot.
    pub const WEAPON5: u16 = 1 << 15;
    /// Weapon1 to Weapon5, in slot order.
    pub const WEAPONS: [u16; 5] = [Keys::WEAPON1, Keys::WEAPON2, Keys::WEAPON3, Keys::WEAPON4, Keys::WEAPON5];
    /// Every key bit.
    pub const ALL: u16 = u16::MAX;
}

/// One key's latched state between two input reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct KeyLatch {
    down: bool,
    pressed: bool,
    once: bool,
}

impl KeyLatch {
    /// Sets the physical key state; a press (up to down) latches both flags.
    pub fn set(&mut self, down: bool) {
        if down && !self.down {
            self.pressed = true;
            self.once = true;
        }
        self.down = down;
    }

    /// `CInput::isDown`: down now, or pressed since the last read.
    pub fn is_down(&self) -> bool {
        self.down || self.pressed
    }

    /// `CInput::isDownOnce`: pressed since the last read.
    pub fn is_once(&self) -> bool {
        self.once
    }

    /// `CInput::reset`: called at the end of every read.
    pub fn reset(&mut self) {
        self.pressed = false;
        self.once = false;
    }
}

/// All keys of one worm.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Latches {
    pub up: KeyLatch,
    pub down: KeyLatch,
    pub left: KeyLatch,
    pub right: KeyLatch,
    pub jump: KeyLatch,
    pub fire: KeyLatch,
    pub rope: KeyLatch,
    pub change: KeyLatch,
    pub strafe: KeyLatch,
    pub prev_weapon: KeyLatch,
    pub next_weapon: KeyLatch,
    /// Weapon1 to Weapon5 (issue #48), in slot order.
    pub weapons: [KeyLatch; 5],
}

impl Latches {
    /// Sets every key from this frame's `Keys`.
    pub fn set(&mut self, k: Keys) {
        let on = |bit: u16| k.0 & bit != 0;
        self.up.set(on(Keys::UP));
        self.down.set(on(Keys::DOWN));
        self.left.set(on(Keys::LEFT));
        self.right.set(on(Keys::RIGHT));
        self.jump.set(on(Keys::JUMP));
        self.fire.set(on(Keys::FIRE));
        self.rope.set(on(Keys::ROPE));
        self.change.set(on(Keys::CHANGE));
        self.strafe.set(on(Keys::STRAFE));
        self.prev_weapon.set(on(Keys::PREV_WEAPON));
        self.next_weapon.set(on(Keys::NEXT_WEAPON));
        for (latch, bit) in self.weapons.iter_mut().zip(Keys::WEAPONS) {
            latch.set(on(bit));
        }
    }

    /// Ends a read: clears every key's latches.
    pub fn reset(&mut self) {
        for k in [
            &mut self.up,
            &mut self.down,
            &mut self.left,
            &mut self.right,
            &mut self.jump,
            &mut self.fire,
            &mut self.rope,
            &mut self.change,
            &mut self.strafe,
            &mut self.prev_weapon,
            &mut self.next_weapon,
        ] {
            k.reset();
        }
        for k in &mut self.weapons {
            k.reset();
        }
    }
}

/// The classic 8 keys are hashed always; Strafe and Previous/Next weapon
/// only when one of them is not at rest, and Weapon1-5 only when one of
/// them is not at rest, so a match without them hashes as before they
/// existed (match goldens, oracle), as `World::gone` does.
impl crate::checksum::Checksum for Latches {
    fn hash(&self, h: &mut crate::checksum::Fnv) {
        let Latches {
            up,
            down,
            left,
            right,
            jump,
            fire,
            rope,
            change,
            strafe,
            prev_weapon,
            next_weapon,
            weapons,
        } = self;
        let byte = |KeyLatch { down, pressed, once }: &KeyLatch| {
            *down as u8 | (*pressed as u8) << 1 | (*once as u8) << 2
        };
        for k in [up, down, left, right, jump, fire, rope, change] {
            h.bytes(&[byte(k)]);
        }
        let extra = [strafe, prev_weapon, next_weapon];
        if extra.iter().any(|k| **k != KeyLatch::default()) {
            h.bytes(&extra.map(byte));
        }
        if weapons.iter().any(|k| *k != KeyLatch::default()) {
            h.bytes(&weapons.each_ref().map(byte));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tap_is_seen_once_even_after_release() {
        let mut k = KeyLatch::default();
        k.set(true);
        k.set(false); // released before any read
        assert!(k.is_down() && k.is_once());
        k.reset();
        assert!(!k.is_down() && !k.is_once());
    }

    #[test]
    fn holding_is_down_but_once_only_after_a_press() {
        let mut k = KeyLatch::default();
        k.set(true);
        k.reset();
        k.set(true); // still held: no new press
        assert!(k.is_down() && !k.is_once());
    }

    #[test]
    fn the_new_keys_latch_and_reset() {
        let mut l = Latches::default();
        l.set(Keys(Keys::STRAFE | Keys::PREV_WEAPON | Keys::NEXT_WEAPON));
        assert!(l.strafe.is_down() && l.prev_weapon.is_once() && l.next_weapon.is_once());
        l.reset();
        assert!(
            l.strafe.is_down()
                && l.next_weapon.is_down()
                && !l.prev_weapon.is_once()
                && !l.next_weapon.is_once()
        );
    }

    #[test]
    fn the_new_keys_are_hashed_only_when_not_at_rest() {
        use crate::checksum::{Checksum, Fnv};
        let hash = |l: &Latches| {
            let mut h = Fnv::new();
            l.hash(&mut h);
            h.finish()
        };
        let classic = |l: &Latches| {
            let mut h = Fnv::new();
            let Latches { up, down, left, right, jump, fire, rope, change, .. } = l;
            for KeyLatch { down, pressed, once } in [up, down, left, right, jump, fire, rope, change] {
                h.bytes(&[*down as u8 | (*pressed as u8) << 1 | (*once as u8) << 2]);
            }
            h.finish()
        };
        let mut l = Latches::default();
        l.set(Keys(Keys::LEFT | Keys::FIRE));
        assert_eq!(hash(&l), classic(&l));
        let mut s = l;
        s.set(Keys(Keys::LEFT | Keys::FIRE | Keys::STRAFE));
        assert_ne!(hash(&s), hash(&l));
        s.set(Keys(Keys::LEFT | Keys::FIRE));
        s.reset();
        l.reset();
        assert_eq!(hash(&s), hash(&l), "released and read: at rest again");

        let mut q = l;
        q.set(Keys(Keys::LEFT | Keys::FIRE | Keys::WEAPON3));
        assert_ne!(hash(&q), hash(&l), "Weapon3 held");
        let mut s = l;
        s.set(Keys(Keys::LEFT | Keys::FIRE | Keys::STRAFE));
        assert_ne!(hash(&q), hash(&s), "the weapon keys hash apart from the other new ones");
        q.set(Keys(Keys::LEFT | Keys::FIRE));
        q.reset();
        assert_eq!(hash(&q), hash(&l), "released and read: at rest again");
    }

    #[test]
    fn the_weapon_keys_latch_held_and_reset() {
        let mut l = Latches::default();
        l.set(Keys(Keys::WEAPON1 | Keys::WEAPON5));
        l.set(Keys(Keys::WEAPON5)); // Weapon1 tapped before any read
        assert!(l.weapons[0].is_down() && l.weapons[4].is_down());
        assert!(!l.weapons[1].is_down() && !l.weapons[2].is_down() && !l.weapons[3].is_down());
        l.reset();
        assert!(!l.weapons[0].is_down() && l.weapons[4].is_down(), "held stays down");
    }
}
