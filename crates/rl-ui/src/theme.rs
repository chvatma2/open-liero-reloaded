//! The menus' and HUD's egui look (spec §8.2-8.3): dark, a little larger
//! than egui's defaults, scaled by the UI-scale setting on top of the
//! display's DPI scale (which egui-winit sets as the native pixels per
//! point).

use egui::{Color32, Context, FontId, Frame, Stroke, TextStyle, Theme};

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

/// Applies the theme to `ctx` with `ui_scale` (clamped to
/// [`crate::UI_SCALE_RANGE`], 1 when NaN) as egui's zoom factor. Call it
/// at startup and whenever the scale changes; it takes effect from the
/// next frame.
pub fn apply(ctx: &Context, ui_scale: f32) {
    ctx.set_theme(Theme::Dark);
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 4.0);
        style.text_styles = [
            (TextStyle::Small, FontId::proportional(11.0)),
            (TextStyle::Body, FontId::proportional(15.0)),
            (TextStyle::Monospace, FontId::monospace(14.0)),
            (TextStyle::Button, FontId::proportional(16.0)),
            (TextStyle::Heading, FontId::proportional(26.0)),
        ]
        .into();
    });
    ctx.set_zoom_factor(clamp_scale(ui_scale));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_scale_is_clamped() {
        let ctx = Context::default();
        for (asked, got) in [(1.5, 1.5), (0.1, 0.5), (9.0, 3.0), (f32::NAN, 1.0), (f32::INFINITY, 3.0)] {
            apply(&ctx, asked);
            // The zoom takes effect at the end of a pass.
            ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
            assert_eq!(ctx.zoom_factor(), got, "{asked}");
        }
        assert_eq!(ctx.global_style().text_styles[&TextStyle::Heading].size, 26.0);
    }
}
