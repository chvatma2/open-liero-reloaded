//! Content definitions shared by importers, the pack format and the simulation.
//!
//! This crate is `no_std` + `alloc`: it holds data only, never performs I/O.
//! Field semantics mirror OpenLieroX (OLX) at commit `d1ad953f`; each type
//! documents the OLX source it corresponds to.
#![no_std]

extern crate alloc;

pub mod image;
pub mod level;
pub mod manifest;
pub mod skin;
pub mod slug;
pub mod theme;
