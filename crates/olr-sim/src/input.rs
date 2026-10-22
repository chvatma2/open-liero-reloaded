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
    /// Every key bit.
    pub const ALL: u16 = 2047;
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
    }
}

/// The classic 8 keys are hashed always; Strafe and Previous/Next weapon
/// only when one of them is not at rest, so a match without them hashes as
/// before they existed (match goldens, oracle), as `World::gone` does.
impl crate::checksum::Checksum for Latches {
    fn hash(&self, h: &mut crate::checksum::Fnv) {
        let Latches { up, down, left, right, jump, fire, rope, change, strafe, prev_weapon, next_weapon } =
            self;
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
    }
}
