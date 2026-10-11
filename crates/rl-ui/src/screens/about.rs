//! The About screen (spec §11, §21 G9): who made the games this one
//! remakes, the licenses, and where the bundled content came from (each
//! pack's recorded origin, which `content/olx-manifest.ron` pins).
//!
//! OLX names its authors in the main menu (`Menu_Main.cpp:276-283`) and
//! Liero Xtreme's license asks for credit and that its name not be used
//! for a new release (`doc/original_source_release/License.txt`).

use egui::{Color32, RichText, ScrollArea, Ui};
use rl_data::manifest::Origin;

use super::{MenuData, header};
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
    "rust-liero is free software under the GNU General Public License, version 3 or later \
     (GPL-3.0-or-later).",
    "OpenLieroX's code is under the GNU Lesser General Public License (LGPL, version 2 or later), \
     which permits its use under the GPL.",
    "Liero Xtreme's original code is under a zlib-style license (Jason Boettcher).",
    "The bundled levels, mods, skins, sounds and images come from the OpenLieroX repository, \
     converted at import; each keeps its origin:",
];

/// `origin` as one line: `OpenLieroX share/gamedir/Classic (d1ad953f)`.
pub fn origin_text(origin: Option<&Origin>) -> String {
    match origin {
        Some(o) => {
            let rev: String = o.revision.chars().take(8).collect();
            format!("{} {} ({rev})", o.source, o.path)
        }
        None => "no origin recorded".to_string(),
    }
}

pub(super) fn show(ui: &mut Ui, data: &MenuData, out: &mut Vec<UiAction>) {
    header(ui, "About", out);
    ScrollArea::vertical().id_salt("about").show(ui, |ui| {
        ui.label(RichText::new(format!("rust-liero {}", env!("CARGO_PKG_VERSION"))).size(22.0).strong());
        ui.label("A remake of OpenLieroX in Rust: its levels, mods and skins, played as it plays them.");
        ui.add_space(12.0);
        for (heading, lines) in CREDITS {
            ui.label(RichText::new(*heading).strong().color(Color32::from_rgb(255, 200, 80)));
            for line in *lines {
                ui.label(*line);
            }
            ui.add_space(8.0);
        }
        ui.label(RichText::new("Licenses").strong().color(Color32::from_rgb(255, 200, 80)));
        for line in LICENSES {
            ui.label(*line);
        }
        ui.add_space(4.0);
        let c = &data.catalog;
        egui::Grid::new("about content").num_columns(2).spacing([16.0, 2.0]).striped(true).show(ui, |ui| {
            for m in &c.mods {
                ui.label(format!("Mod {}", m.name));
                ui.label(RichText::new(origin_text(m.origin.as_ref())).weak());
                ui.end_row();
            }
            for l in &c.levels {
                ui.label(format!("Level {}", l.name));
                ui.label(RichText::new(origin_text(l.origin.as_ref())).weak());
                ui.end_row();
            }
        });
        if !c.skins.is_empty() {
            ui.label(RichText::new(format!("and {} skins", c.skins.len())).weak());
        }
        if c.mods.is_empty() && c.levels.is_empty() {
            ui.label(RichText::new("No content imported.").weak());
        }
    });
}
