//! The live heat preview's controls and scene (issue #45, Materials 9/10):
//! the host (`olr-game`) renders the scene with the real renderer's heat
//! pass and hands the panels its picture; the editor asks for blasts, the
//! material compared, a restart and the time scale through
//! [`PreviewCommand`]s, and the material open goes to the scene as it is
//! edited (the host reads [`crate::MaterialEditorState::doc`]).
//!
//! The scene: two identical patches side by side, the material open on
//! the left and the one compared on the right, split by a wall of rock
//! that has no material (it neither heats nor lets light through). Each
//! patch is open air over a rock and dirt shape, a back wall of the
//! patch's material behind it all.

/// A patch's size, world pixels.
pub const PATCH: (u32, u32) = (64, 72);
/// The wall between the patches, world pixels.
pub const DIVIDER: u32 = 8;
/// The scene's size, world pixels: the two patches and the wall.
pub const SCENE: (u32, u32) = (2 * PATCH.0 + DIVIDER, PATCH.1);

/// Patch `i`'s (0 the material open, 1 the one compared) left edge in
/// the scene, world pixels.
pub const fn patch_x(i: usize) -> u32 {
    if i == 0 { 0 } else { PATCH.0 + DIVIDER }
}

/// The patch and the pixel in it of scene point `at` (world pixels):
/// `None` on the wall or outside the scene.
pub fn patch_at(at: [f32; 2]) -> Option<(usize, [u32; 2])> {
    let [x, y] = at;
    if !(x.is_finite() && y.is_finite()) || x < 0.0 || y < 0.0 || y >= PATCH.1 as f32 {
        return None;
    }
    (0..2).find_map(|i| {
        let rel = x - patch_x(i) as f32;
        (0.0..PATCH.0 as f32).contains(&rel).then_some((i, [rel as u32, y as u32]))
    })
}

/// An engine explosion's size to blast with: its damage sizes the crater
/// and the engine's heat (`olr_look::engine::explosion_heat`, Materials
/// 4/10's curve), as in play.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BlastSize {
    /// A minigun hit's (damage 3): stone's rim stays under the glow
    /// unless hits pile up.
    Small,
    /// A Bazooka's (damage 4): the rim glows a few pixels deep.
    #[default]
    Bazooka,
    /// A Big Nuke fragment's (damage 15): a wide patch near white.
    Nuke,
}

impl BlastSize {
    pub const ALL: [BlastSize; 3] = [BlastSize::Small, BlastSize::Bazooka, BlastSize::Nuke];

    /// OLX's `Explode` damage of this size.
    pub fn damage(self) -> i32 {
        match self {
            BlastSize::Small => 3,
            BlastSize::Bazooka => 4,
            BlastSize::Nuke => 15,
        }
    }

    /// Its button's label.
    pub fn label(self) -> &'static str {
        match self {
            BlastSize::Small => "Small",
            BlastSize::Bazooka => "Bazooka",
            BlastSize::Nuke => "Nuke",
        }
    }
}

/// How fast the preview's clock runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TimeScale {
    #[default]
    One,
    Four,
}

impl TimeScale {
    pub const ALL: [TimeScale; 2] = [TimeScale::One, TimeScale::Four];

    /// Match milliseconds a millisecond.
    pub fn factor(self) -> u32 {
        match self {
            TimeScale::One => 1,
            TimeScale::Four => 4,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TimeScale::One => "1×",
            TimeScale::Four => "4×",
        }
    }
}

/// The preview's controls as set.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PreviewSettings {
    pub size: BlastSize,
    pub scale: TimeScale,
    /// The library's material in the right patch (none: rock that never
    /// heats).
    pub compare: Option<crate::Key>,
}

/// What the preview's controls ask of the editor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreviewAction {
    /// Blast both patches at this pixel of a patch.
    Blast([u32; 2]),
    Size(BlastSize),
    Scale(TimeScale),
    /// The library's material to compare with (none: none).
    Compare(Option<crate::Key>),
    /// The scene afresh: craters filled, the heat gone.
    Restart,
}

/// What the editor asks of the host's preview.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreviewCommand {
    /// An engine explosion of `size` at pixel `at` of both patches: a
    /// crater carved, the engine's heat deposited.
    Blast { at: [u32; 2], size: BlastSize },
    /// The right patch's material (none: none); sent again whenever the
    /// library's copy of it changes.
    SetCompare(Option<olr_format::materials::MaterialSource>),
    /// The scene afresh.
    Restart,
    /// Match milliseconds a millisecond.
    TimeScale(u32),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_find_their_patch() {
        assert_eq!(patch_at([0.0, 0.0]), Some((0, [0, 0])));
        assert_eq!(patch_at([63.9, 71.9]), Some((0, [63, 71])));
        assert_eq!(patch_at([64.0, 10.0]), None, "the wall");
        assert_eq!(patch_at([72.0, 10.0]), Some((1, [0, 10])));
        assert_eq!(patch_at([135.5, 10.0]), Some((1, [63, 10])));
        for p in [[136.0, 0.0], [-0.1, 0.0], [0.0, 72.0], [f32::NAN, 1.0], [1.0, f32::INFINITY]] {
            assert_eq!(patch_at(p), None, "{p:?}");
        }
        assert_eq!(SCENE, (136, 72));
    }
}
