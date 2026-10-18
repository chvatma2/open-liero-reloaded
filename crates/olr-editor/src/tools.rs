//! The tools and what the canvas draws over the bake.

use olr_data::image::Rgba;
use olr_edit::compose::Rect;

/// The canvas tools; their shortcut letters are V B E R O L P I W.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Select,
    Brush,
    Eraser,
    Rect,
    Ellipse,
    Line,
    Spawn,
    Eyedropper,
    /// Remove colour (plan 14d R8): masks a picture's region of alike colour.
    Wand,
}

impl Tool {
    pub const ALL: [Tool; 9] = [
        Tool::Select,
        Tool::Brush,
        Tool::Eraser,
        Tool::Rect,
        Tool::Ellipse,
        Tool::Line,
        Tool::Spawn,
        Tool::Eyedropper,
        Tool::Wand,
    ];

    /// The shortcut letter.
    pub fn key(self) -> char {
        match self {
            Tool::Select => 'V',
            Tool::Brush => 'B',
            Tool::Eraser => 'E',
            Tool::Rect => 'R',
            Tool::Ellipse => 'O',
            Tool::Line => 'L',
            Tool::Spawn => 'P',
            Tool::Eyedropper => 'I',
            Tool::Wand => 'W',
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::Brush => "Brush",
            Tool::Eraser => "Eraser",
            Tool::Rect => "Rectangle",
            Tool::Ellipse => "Ellipse",
            Tool::Line => "Line",
            Tool::Spawn => "Spawn",
            Tool::Eyedropper => "Eyedropper",
            Tool::Wand => "Remove colour",
        }
    }

    /// Whether the tool paints into the selected drawn layer.
    pub fn paints(self) -> bool {
        matches!(self, Tool::Brush | Tool::Eraser | Tool::Rect | Tool::Ellipse | Tool::Line)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrushShape {
    Round,
    Square,
}

/// The brush, sized in world pixels (1 to `MAX_BRUSH`); also a line's width.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Brush {
    pub shape: BrushShape,
    pub size: u32,
}

impl Default for Brush {
    fn default() -> Brush {
        Brush { shape: BrushShape::Round, size: 8 }
    }
}

pub const MAX_BRUSH: u32 = 256;

/// What painting lays: a colour, or the theme's front or back tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaintChoice {
    Colour(Rgba),
    FrontTile,
    BackTile,
}

impl Default for PaintChoice {
    fn default() -> PaintChoice {
        PaintChoice::Colour(Rgba::rgb(140, 90, 40))
    }
}

/// A pointer event's kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerKind {
    Down,
    Move,
    Up,
}

/// What the canvas draws over the bake, in world pixels (the UI maps them
/// to the screen).
#[derive(Clone, Debug, PartialEq)]
pub enum Overlay {
    /// An image element's box.
    Box {
        rect: Rect,
        selected: bool,
        locked: bool,
    },
    /// One of the 8 handles of the selected unlocked image.
    Handle {
        at: (i32, i32),
    },
    /// A resize or rectangle/ellipse preview.
    Outline {
        rect: Rect,
    },
    /// A line preview.
    Segment {
        from: (i32, i32),
        to: (i32, i32),
    },
    Brush {
        centre: (i32, i32),
        size: u32,
        shape: BrushShape,
    },
    Spawn {
        at: (i32, i32),
        selected: bool,
    },
}
