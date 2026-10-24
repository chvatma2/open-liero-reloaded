//! What a frame worked on (the render profile, spec
//! `docs/superpowers/specs/2026-10-24-render-profile-design.md`): the
//! heat's active area, the view targets' and the glow light's sizes, so a
//! cost can be set against what it scales with.

use super::Renderer;

/// The renderer's workload as now (bench only).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Workload {
    /// The active heat boxes' pixels (world pixels), the level's and the
    /// distant layers', and their count.
    pub heat_area: u64,
    pub heat_boxes: u32,
    /// Heat steps and heat passes run so far (the level's).
    pub heat_steps: u64,
    pub heat_passes: u64,
    /// Each view target's size in texels.
    pub world_targets: Vec<(u32, u32)>,
    /// The glow light's grid in cells and its cascades (`None` and 0
    /// unlit); the grid covers the whole level, whatever the view.
    pub glow_grid: Option<(u32, u32)>,
    pub cascades: u32,
}

impl Renderer {
    /// The workload as now (bench only).
    pub fn workload(&self) -> Workload {
        let (heat_area, heat_boxes) = self.heat_area();
        let gi = self.look.as_ref().and_then(|l| l.textures.as_ref()).map(|t| &t.gi);
        Workload {
            heat_area,
            heat_boxes,
            heat_steps: self.heat_steps(),
            heat_passes: self.heat_passes(),
            world_targets: self.targets.iter().map(|t| t.size).collect(),
            glow_grid: gi.map(|g| g.grid),
            cascades: gi.map_or(0, |g| g.cascades.len() as u32),
        }
    }
}
