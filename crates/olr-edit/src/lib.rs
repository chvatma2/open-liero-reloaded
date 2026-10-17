//! Pack authoring operations shared by `olr-cli` and the editors (plan 13d):
//! a pack's authoring source loaded and saved whole ([`pack`]), the
//! canonical formatter ([`fmt`]) and pack templates ([`new`]).

pub mod fmt;
pub mod level;
pub mod new;
pub mod pack;
