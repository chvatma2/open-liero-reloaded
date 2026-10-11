//! The match HUD (spec §8.3, §21 G5), Modern style: the health, the five
//! weapon slots with their charge and reload, the kill feed, the clock and
//! the kill limit, the minimap (OLX's colours and scale), the Tab
//! scoreboard and "Game Over". What it shows is a [`HudView`], built from
//! the world each frame ([`WormHud::of`]); where it goes is a
//! [`HudLayout`] (pure, so Review Focus 5 is tested without a window);
//! [`show`] paints them. The minimap's picture is an egui texture the app
//! uploads from [`minimap::image`] (no GPU handle here).

mod fade;
pub mod layout;
pub mod minimap;
mod model;
mod paint;
mod view;

pub use fade::{Covered, FADE_SECONDS, FADED, HudFade, Opacity};
pub use layout::{HudLayout, MAX_MINIMAP};
pub use model::{FEED_LINES, FEED_TICKS, Feed, Names, Results, Row, clock, lives_text, message_text, rows};
pub use paint::{show, table};
pub use view::{Bar, Dot, HudView, MinimapView, Notice, SlotHud, WormHud};
