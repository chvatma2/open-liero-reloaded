//! [`Buf`]: a tick-time buffer that a clone does not shrink.

use alloc::vec::Vec;
use core::ops::{Deref, DerefMut};

/// A `Vec` sized at setup that `tick` fills without allocating. Unlike a
/// `Vec`, its clone keeps the capacity, so a cloned world (a snapshot)
/// does not allocate when it ticks either.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Buf<T>(pub Vec<T>);

impl<T> Buf<T> {
    pub fn with_capacity(n: usize) -> Self {
        Buf(Vec::with_capacity(n))
    }
}

impl<T: Clone> Clone for Buf<T> {
    fn clone(&self) -> Self {
        let mut v = Vec::with_capacity(self.0.capacity());
        v.extend_from_slice(&self.0);
        Buf(v)
    }
}

impl<T> Deref for Buf<T> {
    type Target = Vec<T>;

    fn deref(&self) -> &Vec<T> {
        &self.0
    }
}

impl<T> DerefMut for Buf<T> {
    fn deref_mut(&mut self) -> &mut Vec<T> {
        &mut self.0
    }
}
