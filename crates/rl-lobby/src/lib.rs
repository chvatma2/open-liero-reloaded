//! Online lobby logic (networking spec §4, §5.2, §5.4, §7.2), sans-IO: the
//! player's identity, and the lobby's host and client.
//!
//! `#![no_std]` + `alloc`.
#![no_std]

extern crate alloc;

mod client;
#[cfg(feature = "test-hooks")]
pub mod harness;
mod host;
pub mod identity;
mod model;

pub use client::LobbyClient;
pub use host::LobbyHost;
pub use identity::{Identity, challenge_message, fingerprint, password_proof, verify};
pub use model::*;
