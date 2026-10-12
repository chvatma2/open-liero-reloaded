//! Online lobby logic (networking spec §5.2, §7.2), sans-IO.
//!
//! `#![no_std]` + `alloc`.
#![no_std]

extern crate alloc;

pub mod identity;
pub use identity::{Identity, challenge_message, fingerprint, password_proof, verify};
