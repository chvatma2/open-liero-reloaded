//! The tools and what the canvas draws over the bake.

use olr_data::image::Rgba;
use olr_edit::compose::Rect;

/// The canvas tools; their shortcut letters are V B E R O L P I W G M H K.
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
    /// Flood fill (plan 14e R5).
    Fill,
    /// A rectangle of the selected drawn layer's pixels, to copy, cut or
    /// delete (plan 14e R7).
    SelectPixels,
    /// Paints the selected glowing element's glow mask (plan 15a R12).
    Glow,
}

impl Tool {
    /// The drawing tools: the toolbar's first row (the look's tools sit
    /// with the look, plan 15a Task 18).
    pub const DRAWING: [Tool; 11] = [
        Tool::Select,
        Tool::Brush,
        Tool::Eraser,
        Tool::Rect,
        Tool::Ellipse,
        Tool::Line,
        Tool::Spawn,
        Tool::Eyedropper,
        Tool::Wand,
        Tool::Fill,
        Tool::SelectPixels,
    ];

    /// Every tool (each has its shortcut).
    pub const ALL: [Tool; 12] = [
        Tool::Select,
        Tool::Brush,
        Tool::Eraser,
        Tool::Rect,
        Tool::Ellipse,
        Tool::Line,
        Tool::Spawn,
        Tool::Eyedropper,
        Tool::Wand,
        Tool::Fill,
        Tool::SelectPixels,
        Tool::Glow,
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
            Tool::Fill => 'G',
            Tool::SelectPixels => 'M',
            Tool::Glow => 'H',
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
            Tool::Fill => "Fill",
            Tool::SelectPixels => "Select pixels",
            Tool::Glow => "Glow",
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
    /// The level theme's hole `i` (plan 14e R4), scaled in whole steps by
    /// the brush size.
    Crater(u8),
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

/// What painting lays: a colour, the theme's front or back tile, or a
/// texture (plan 14e R3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaintChoice {
    Colour(Rgba),
    FrontTile,
    BackTile,
    Texture(TextureRef),
}

/// A texture: a built-in one (`olr_edit::textures::builtin`'s index) or
/// one of the user's library, by name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextureRef {
    Builtin(usize),
    User(String),
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
