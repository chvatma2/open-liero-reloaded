//! The colour map's inputs (spec §19 E7) and the simulation's terrain edits
//! as it replays them (plan 5, ruling R1: `olr-terrain` cannot see `olr-sim`).

use olr_sim::{EventKind, LostArea, StampId, TerrainOp, TickEvents};
use olr_terrain::{Area, ColorEdit, ColorMap, Terrain, ThemeArt};

use crate::{LoadedLevel, LoadedTheme};

impl LoadedTheme {
    /// What the colour map edits with: the holes, the green-dirt mask and
    /// `FrontTile`.
    pub fn art(&self) -> ThemeArt {
        ThemeArt { holes: self.stamps(), green_mask: self.green_mask(), front_tile: self.front_tile.clone() }
    }
}

impl LoadedLevel {
    /// The level's colour map before any edit (worms' spawn holes included:
    /// they arrive with the first tick's events).
    pub fn color_map(&self, theme: &LoadedTheme) -> ColorMap {
        let mut map = ColorMap::new(
            &Terrain::from_grid(&self.material),
            &self.front,
            &self.back,
            self.art_scale,
            theme.art(),
        );
        // Glow that does not fit the map (it always does from a checked
        // pack) is left out.
        map.set_glow(self.glow.clone());
        map
    }
}

/// A [`EventKind::TerrainEdit`] as the colour map replays it; `None` for
/// other events (and for a stamp the op never uses).
pub fn color_edit(kind: &EventKind) -> Option<ColorEdit> {
    let EventKind::TerrainEdit { op, stamp, x, y } = *kind else { return None };
    match (op, stamp) {
        (TerrainOp::Carve | TerrainOp::Beam, StampId::Hole(hole)) => Some(ColorEdit::Carve { hole, x, y }),
        (TerrainOp::Dirt, StampId::Hole(hole)) => Some(ColorEdit::Dirt { hole, x, y }),
        (TerrainOp::GreenDirt, StampId::GreenBall) => Some(ColorEdit::GreenDirt { x, y }),
        _ => None,
    }
}

/// `olr-sim`'s [`LostArea`] as `olr-terrain`'s [`Area`].
pub fn lost_area(a: LostArea) -> Area {
    Area { x0: a.x0, y0: a.y0, x1: a.x1, y1: a.y1 }
}

/// Replays a tick's terrain edits on `map` in order, then rebuilds the area
/// of any edit the tick dropped from `material` (the world's terrain after
/// the tick). A renderer that samples `Dirt { probe }` colours between the
/// edits iterates the events itself with [`color_edit`].
pub fn apply_edits(map: &mut ColorMap, events: &TickEvents, material: &Terrain) {
    for e in events {
        if let Some(edit) = color_edit(&e.kind) {
            map.apply(edit);
        }
    }
    if let Some(area) = events.lost_area() {
        map.rebuild_area(lost_area(area), material);
    }
}
