//! The About screen (spec §11, §21 G9): who made the games this one
//! remakes, the licenses, and where the imported content came from (each
//! pack's recorded origin; `cargo xtask fetch-olx-content` imports what
//! `content/olx-manifest.ron` pins).
//!
//! OLX names its authors in the main menu (`Menu_Main.cpp:276-283`) and
//! Liero Xtreme's license asks for credit and that its name not be used
//! for a new release (`doc/original_source_release/License.txt`).

use std::collections::BTreeMap;

use egui::{RichText, ScrollArea, Ui};
use olr_content::PackSource;
use olr_data::manifest::{Origin, PackKind};

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
    "The game ships no content: the levels, mods, skins, theme, sounds and images it plays are \
     imported from OpenLieroX and converted, and each pack records where it came from:",
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

/// The packs as `(label, origin)` rows: mods, levels, themes and the core
/// pack one by one; skins grouped by the directory they came from.
pub fn sources(packs: &[PackSource]) -> Vec<(String, String)> {
    let mut rows = Vec::new();
    for (kind, word) in [
        (PackKind::Mod, "Mod"),
        (PackKind::Level, "Level"),
        (PackKind::Theme, "Theme"),
        (PackKind::Core, "Core"),
    ] {
        for p in packs.iter().filter(|p| p.kind == kind) {
            rows.push((format!("{word} {}", p.name), origin_text(p.origin.as_ref())));
        }
    }
    // Skins: one row per source directory (OLX's `skins/`), counted.
    let mut skins: BTreeMap<String, usize> = BTreeMap::new();
    for p in packs.iter().filter(|p| p.kind == PackKind::Skin) {
        let dir = p.origin.as_ref().map(|o| {
            let path = o.path.rsplit_once('/').map_or(o.path.as_str(), |(d, _)| d);
            Origin { path: path.to_string(), ..o.clone() }
        });
        *skins.entry(origin_text(dir.as_ref())).or_default() += 1;
    }
    for (origin, n) in skins {
        rows.push((format!("{n} skins"), origin));
    }
    rows
}

/// The page's column.
const WIDTH: f32 = 860.0;

pub(super) fn show(ui: &mut Ui, data: &MenuData, out: &mut Vec<UiAction>) {
    let mut ui = page(ui, "About", WIDTH, out);
    ScrollArea::vertical().id_salt("about").show(&mut ui, |ui| {
        ui.vertical_centered(|ui| {
            let version = format!("Open Liero Reloaded {}", env!("CARGO_PKG_VERSION"));
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
            ui.add_space(4.0);
            let c = &data.catalog;
            egui::Grid::new("about content").num_columns(2).spacing([16.0, 2.0]).striped(true).show(
                ui,
                |ui| {
                    for (label, origin) in sources(&c.sources) {
                        ui.label(label);
                        ui.label(RichText::new(origin).weak());
                        ui.end_row();
                    }
                },
            );
            if c.sources.is_empty() {
                ui.label(RichText::new("No content imported.").weak());
            }
        });
    });
}
