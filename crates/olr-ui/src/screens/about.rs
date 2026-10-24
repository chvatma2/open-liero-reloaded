//! The About screen (spec §11, §21 G9): who made the games this one
//! remakes, and the licenses.
//!
//! OLX names its authors in the main menu (`Menu_Main.cpp:276-283`) and
//! Liero Xtreme's license asks for credit and that its name not be used
//! for a new release (`doc/original_source_release/License.txt`).

use egui::{RichText, ScrollArea, Ui};

use super::{MenuData, page, section};
use crate::state::UiAction;

/// The credits, as `(heading, lines)`.
pub const CREDITS: &[(&str, &[&str])] = &[
    ("Liero", &["Joosa Riekkinen, the original Liero (1998)."]),
    (
        "Liero Xtreme",
        &[
            "Jason Boettcher, Liero Xtreme (2002-2006), which OpenLieroX and this game are based on.",
            "As its license asks, this game credits it and does not use its name for itself.",
        ],
    ),
    (
        "OpenLieroX",
        &[
            "The OpenLieroX team: original code by Jason Boettcher; ported and enhanced by",
            "K. Petránek, Albert Zeyer, Daniel Sjoholm, Martin Griffin and Sergiy Pylypenko;",
            "music by Corentin Larsen. This game reproduces how OpenLieroX plays.",
        ],
    ),
];

/// The licenses (spec §11).
pub const LICENSES: &[&str] = &[
    "Open Liero Reloaded is free software under the GNU General Public License, version 3 or later \
     (GPL-3.0-or-later).",
    "OpenLieroX's code is under the GNU Lesser General Public License (LGPL, version 2 or later), \
     which permits its use under the GPL.",
    "Liero Xtreme's original code is under a zlib-style license (Jason Boettcher).",
];

/// The page's column.
const WIDTH: f32 = 860.0;

pub(super) fn show(ui: &mut Ui, data: &MenuData, out: &mut Vec<UiAction>) {
    let mut ui = page(ui, "About", WIDTH, out);
    ScrollArea::vertical().id_salt("about").show(&mut ui, |ui| {
        ui.vertical_centered(|ui| {
            let v = if data.version.is_empty() { env!("CARGO_PKG_VERSION") } else { data.version.as_str() };
            let version = format!("Open Liero Reloaded {v}");
            ui.label(RichText::new(version).size(22.0).strong());
            ui.label("A remake of OpenLieroX in Rust: its levels, mods and skins, played as it plays them.");
        });
        ui.add_space(10.0);
        section(ui, "Credits", |ui, _| {
            for (heading, lines) in CREDITS {
                ui.label(RichText::new(*heading).strong());
                for line in *lines {
                    ui.label(*line);
                }
                ui.add_space(6.0);
            }
        });
        ui.add_space(10.0);
        section(ui, "Licenses", |ui, _| {
            for line in LICENSES {
                ui.label(*line);
            }
        });
        crate::pad::follow_focus(ui);
    });
}
