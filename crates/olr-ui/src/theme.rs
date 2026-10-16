//! The menus' and HUD's egui look (spec §8.2-8.3): dark, a little larger
//! than egui's defaults, and their scale. The UI fills the same share of
//! the screen's height on every display (the user's ruling after the v0.1
//! playtest): [`REFERENCE_HEIGHT`] pixels of height are a pixel a point,
//! times the UI-scale setting; the display's DPI scale does not count.

use egui::{Color32, Context, FontId, Frame, Stroke, TextStyle, Theme};

use crate::hud::REFERENCE_HEIGHT;
use crate::settings::clamp_scale;

/// The menus' background.
pub const BACKGROUND: Color32 = Color32::from_rgb(16, 16, 20);

/// A section's title colour (OLX's menu yellow).
pub const ACCENT: Color32 = Color32::from_rgb(255, 200, 80);

/// A page's primary action's fill (Start, Import): egui's selection blue.
pub const PRIMARY: Color32 = Color32::from_rgb(0, 92, 128);

/// The frame a menu groups a section's widgets in: a panel a shade
/// lighter than the background, with a thin border.
pub fn section_frame() -> Frame {
    Frame::new()
        .fill(Color32::from_rgb(25, 25, 31))
        .stroke(Stroke::new(1.0, Color32::from_rgb(52, 52, 62)))
        .corner_radius(6)
        .inner_margin(12)
}

/// Applies the theme's look to `ctx` (once, at startup; the scale is
/// [`set_scale`]'s).
pub fn apply(ctx: &Context) {
    ctx.set_theme(Theme::Dark);
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 4.0);
        // Every state occupies the same space: egui's `Frame` adds its
        // stroke's width to a widget's size, and its defaults give a
        // hovered, pressed or open widget a 1 px border an idle one lacks,
        // so highlighting a control grew it (and what holds it) a pixel or
        // two. Hover, press and open change only colours.
        let widgets = &mut style.visuals.widgets;
        widgets.hovered.bg_stroke.width = widgets.inactive.bg_stroke.width;
        widgets.active.bg_stroke.width = widgets.inactive.bg_stroke.width;
        widgets.open.bg_stroke.width = widgets.inactive.bg_stroke.width;
        for w in [&mut widgets.inactive, &mut widgets.hovered, &mut widgets.active, &mut widgets.open] {
            w.expansion = 0.0;
        }
        widgets.hovered.weak_bg_fill = Color32::from_gray(82);
        widgets.hovered.bg_fill = Color32::from_gray(82);
        style.text_styles = [
            (TextStyle::Small, FontId::proportional(11.0)),
            (TextStyle::Body, FontId::proportional(15.0)),
            (TextStyle::Monospace, FontId::monospace(14.0)),
            (TextStyle::Button, FontId::proportional(16.0)),
            (TextStyle::Heading, FontId::proportional(26.0)),
        ]
        .into();
    });
}

/// The pixels per point for a reference of `height` pixels (the window's
/// height in the menus, its 16:9 frame's in game:
/// `crate::hud::frame`) at `ui_scale` (clamped to
/// [`crate::UI_SCALE_RANGE`], 1 when NaN): `height / 720 × ui_scale`.
/// A height of no size or not a number counts as 720.
pub fn pixels_per_point(height: f32, ui_scale: f32) -> f32 {
    let h = if height.is_finite() && height > 0.0 { height } else { REFERENCE_HEIGHT };
    (h / REFERENCE_HEIGHT * clamp_scale(ui_scale)).max(0.05)
}

/// Sets egui's zoom so `ctx` has [`pixels_per_point`]`(height, ui_scale)`
/// on a display of `native` pixels per point (egui-winit's, the window's
/// scale factor). Call it every frame before the input is taken; a change
/// takes effect from the next frame.
pub fn set_scale(ctx: &Context, native: f32, height: f32, ui_scale: f32) {
    let native = if native.is_finite() && native > 0.0 { native } else { 1.0 };
    let zoom = pixels_per_point(height, ui_scale) / native;
    if (ctx.zoom_factor() - zoom).abs() > 1e-4 {
        ctx.set_zoom_factor(zoom);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The UI fills the same share of the screen's height on every
    /// display: a 720-pixel-high reference is one pixel a point, the UI
    /// scale on top; the display's own scale does not count.
    #[test]
    fn the_scale_follows_the_height() {
        assert_eq!(pixels_per_point(720.0, 1.0), 1.0);
        assert_eq!(pixels_per_point(1080.0, 1.0), 1.5);
        assert_eq!(pixels_per_point(1440.0, 1.0), 2.0);
        assert_eq!(pixels_per_point(1440.0, 1.5), 3.0);
        assert_eq!(pixels_per_point(2160.0, 9.0), 9.0, "the UI scale clamped to 3");
        for (h, s) in [(0.0, 1.0), (-5.0, 1.0), (f32::NAN, 1.0), (f32::INFINITY, 1.0), (720.0, f32::NAN)] {
            let p = pixels_per_point(h, s);
            assert!(p.is_finite() && p > 0.0, "{h} {s}: {p}");
        }
        // As egui's zoom on the display's scale: the points per height stay.
        let ctx = Context::default();
        apply(&ctx);
        for (native, height) in [(1.0, 720.0), (2.0, 1440.0), (1.25, 1440.0), (1.0, 2160.0)] {
            set_scale(&ctx, native, height, 1.0);
            // The zoom takes effect at the end of a pass.
            ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
            let points = height / (native * ctx.zoom_factor());
            assert!((points - REFERENCE_HEIGHT).abs() < 1e-3, "{native} {height}: {points} points");
        }
        assert_eq!(ctx.global_style().text_styles[&TextStyle::Heading].size, 26.0);
    }
}
