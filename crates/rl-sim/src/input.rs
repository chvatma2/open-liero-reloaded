//! Keyboard input with OLX's latching (`src/client/CInput.cpp`).
//!
//! OLX reads keys once per physics sub-step (`getInput`), not once per frame,
//! and keeps two latches per key until that read ends with `CInput::reset()`:
//! - a key pressed since the last read counts as held (`isDown`: `wasDown() || bDown`);
//! - a key pressed since the last read counts as pressed once (`isDownOnce`: `nDownOnce`).
//!
//! So a tap shorter than the gap between two sub-steps is still seen.

/// The keys a player holds during a frame. Bits match `rl_oracle::Keys`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Keys(pub u8);

impl Keys {
    pub const UP: u8 = 1;
    pub const DOWN: u8 = 2;
    pub const LEFT: u8 = 4;
    pub const RIGHT: u8 = 8;
    pub const JUMP: u8 = 16;
    pub const FIRE: u8 = 32;
    pub const ROPE: u8 = 64;
    pub const CHANGE: u8 = 128;
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
}

impl Latches {
    /// Sets every key from this frame's `Keys`.
    pub fn set(&mut self, k: Keys) {
        let on = |bit: u8| k.0 & bit != 0;
        self.up.set(on(Keys::UP));
        self.down.set(on(Keys::DOWN));
        self.left.set(on(Keys::LEFT));
        self.right.set(on(Keys::RIGHT));
        self.jump.set(on(Keys::JUMP));
        self.fire.set(on(Keys::FIRE));
        self.rope.set(on(Keys::ROPE));
        self.change.set(on(Keys::CHANGE));
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
        ] {
            k.reset();
        }
    }
}

impl crate::checksum::Checksum for Latches {
    fn hash(&self, h: &mut crate::checksum::Fnv) {
        let Latches { up, down, left, right, jump, fire, rope, change } = self;
        for KeyLatch { down, pressed, once } in [up, down, left, right, jump, fire, rope, change] {
            h.bytes(&[*down as u8 | (*pressed as u8) << 1 | (*once as u8) << 2]);
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
}
