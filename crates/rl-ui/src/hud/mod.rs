//! The match HUD (spec §8.3, §21 G5), Modern style: the health, the five
//! weapon slots with their charge and reload, the minimap (OLX's colours
//! and scale) and the event log in an opaque bar along the bottom (OLX
//! 0.58's status bar), the clock and the kill limit over the playfield,
//! the Tab scoreboard and "Game Over". What it shows is a [`HudView`], built from
//! the world each frame ([`WormHud::of`]); where it goes is a
//! [`HudLayout`] (pure, so Review Focus 5 is tested without a window);
//! [`show`] paints them. The minimap's picture is an egui texture the app
//! uploads from [`minimap::image`] (no GPU handle here).

pub mod layout;
pub mod minimap;
mod model;
mod paint;
mod view;

pub use layout::{BAR_FULL, HudLayout, MAX_MINIMAP, bar_height};
pub use model::{
    LOG_KEEP, LineKind, Log, LogLine, Names, Results, Row, clock, lives_text, message_text, rows,
};
pub use paint::{show, table};
pub use view::{Bar, Dot, HudView, MinimapView, Notice, SlotHud, WormHud};
