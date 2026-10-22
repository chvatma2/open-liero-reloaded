//! The rail's and the brush's line icons (issue #8): drawn, not emoji,
//! in the colour of the widget's text.

use egui::{Painter, Pos2, Rect, Shape, Stroke};

use crate::tools::Tool;

/// An icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Icon {
    Import,
    Select,
    Brush,
    Eraser,
    Fill,
    Rect,
    Ellipse,
    Line,
    SelectPixels,
    Wand,
    Eyedropper,
    Glow,
    Surface,
    Spawn,
    Round,
    Square,
}

impl Icon {
    pub(super) fn of(tool: Tool) -> Icon {
        match tool {
            Tool::Select => Icon::Select,
            Tool::Brush => Icon::Brush,
            Tool::Eraser => Icon::Eraser,
            Tool::Rect => Icon::Rect,
            Tool::Ellipse => Icon::Ellipse,
            Tool::Line => Icon::Line,
            Tool::Spawn => Icon::Spawn,
            Tool::Eyedropper => Icon::Eyedropper,
            Tool::Wand => Icon::Wand,
            Tool::Fill => Icon::Fill,
            Tool::SelectPixels => Icon::SelectPixels,
            Tool::Glow => Icon::Glow,
            Tool::Surface => Icon::Surface,
        }
    }
}

/// Draws `icon` in the square `rect` with `stroke`.
pub(super) fn paint(painter: &Painter, rect: Rect, icon: Icon, stroke: Stroke) {
    // The icon's points are fractions of its square.
    let at = |x: f32, y: f32| Pos2::new(rect.left() + x * rect.width(), rect.top() + y * rect.height());
    let line = |pts: &[(f32, f32)]| {
        painter.add(Shape::line(pts.iter().map(|&(x, y)| at(x, y)).collect(), stroke));
    };
    let closed = |pts: &[(f32, f32)]| {
        painter.add(Shape::closed_line(pts.iter().map(|&(x, y)| at(x, y)).collect(), stroke));
    };
    let dot = |x: f32, y: f32, r: f32| {
        painter.circle_filled(at(x, y), r * rect.width(), stroke.color);
    };
    let ring = |x: f32, y: f32, r: f32| {
        painter.circle_stroke(at(x, y), r * rect.width(), stroke);
    };
    match icon {
        Icon::Import => {
            line(&[(0.15, 0.6), (0.15, 0.85), (0.85, 0.85), (0.85, 0.6)]);
            line(&[(0.5, 0.12), (0.5, 0.64)]);
            line(&[(0.32, 0.46), (0.5, 0.64), (0.68, 0.46)]);
        }
        Icon::Select => closed(&[
            (0.28, 0.1),
            (0.28, 0.8),
            (0.44, 0.65),
            (0.56, 0.9),
            (0.67, 0.85),
            (0.55, 0.6),
            (0.76, 0.6),
        ]),
        Icon::Brush => {
            line(&[(0.88, 0.12), (0.52, 0.48)]);
            closed(&[(0.52, 0.48), (0.38, 0.45), (0.2, 0.63), (0.12, 0.88), (0.37, 0.8), (0.55, 0.62)]);
        }
        Icon::Eraser => {
            closed(&[(0.1, 0.6), (0.45, 0.25), (0.75, 0.55), (0.4, 0.9)]);
            line(&[(0.27, 0.43), (0.57, 0.73)]);
            line(&[(0.4, 0.9), (0.9, 0.9)]);
        }
        Icon::Fill => {
            closed(&[(0.15, 0.45), (0.48, 0.12), (0.8, 0.45), (0.48, 0.78)]);
            line(&[(0.15, 0.45), (0.8, 0.45)]);
            dot(0.85, 0.78, 0.08);
        }
        Icon::Rect => closed(&[(0.15, 0.25), (0.85, 0.25), (0.85, 0.75), (0.15, 0.75)]),
        Icon::Ellipse => {
            let pts: Vec<(f32, f32)> = (0..32)
                .map(|i| {
                    let a = i as f32 / 32.0 * std::f32::consts::TAU;
                    (0.5 + 0.38 * a.cos(), 0.5 + 0.27 * a.sin())
                })
                .collect();
            closed(&pts);
        }
        Icon::Line => {
            line(&[(0.18, 0.82), (0.82, 0.18)]);
            dot(0.18, 0.82, 0.07);
            dot(0.82, 0.18, 0.07);
        }
        Icon::SelectPixels => {
            let pts =
                [(0.15, 0.2), (0.85, 0.2), (0.85, 0.8), (0.15, 0.8), (0.15, 0.2)].map(|(x, y)| at(x, y));
            let dash = rect.width() * 0.12;
            painter.extend(Shape::dashed_line(&pts, stroke, dash, dash * 0.8));
        }
        Icon::Wand => {
            line(&[(0.15, 0.85), (0.6, 0.4)]);
            line(&[(0.72, 0.12), (0.72, 0.44)]);
            line(&[(0.56, 0.28), (0.88, 0.28)]);
            line(&[(0.62, 0.18), (0.82, 0.38)]);
            line(&[(0.62, 0.38), (0.82, 0.18)]);
        }
        Icon::Eyedropper => {
            line(&[(0.14, 0.86), (0.56, 0.44)]);
            line(&[(0.42, 0.34), (0.66, 0.58)]);
            ring(0.7, 0.3, 0.14);
        }
        Icon::Glow => {
            ring(0.5, 0.5, 0.17);
            for i in 0..8 {
                let a = i as f32 / 8.0 * std::f32::consts::TAU;
                let (c, s) = (a.cos(), a.sin());
                line(&[(0.5 + 0.28 * c, 0.5 + 0.28 * s), (0.5 + 0.42 * c, 0.5 + 0.42 * s)]);
            }
        }
        // A swatch of hatched material.
        Icon::Surface => {
            closed(&[(0.15, 0.15), (0.85, 0.15), (0.85, 0.85), (0.15, 0.85)]);
            line(&[(0.15, 0.5), (0.5, 0.15)]);
            line(&[(0.15, 0.85), (0.85, 0.15)]);
            line(&[(0.5, 0.85), (0.85, 0.5)]);
        }
        Icon::Spawn => {
            line(&[(0.3, 0.9), (0.3, 0.1)]);
            closed(&[(0.3, 0.12), (0.8, 0.28), (0.3, 0.45)]);
        }
        Icon::Round => ring(0.5, 0.5, 0.3),
        Icon::Square => closed(&[(0.2, 0.2), (0.8, 0.2), (0.8, 0.8), (0.2, 0.8)]),
    }
}
