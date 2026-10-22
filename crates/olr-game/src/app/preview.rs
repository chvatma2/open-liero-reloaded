//! The material editor's live heat preview in the window (issue #45):
//! `editor::heat_preview::HeatPreview` on the window's device, its picture
//! an egui texture the editor's panel shows. Made when the material editor
//! opens, its texture freed on the first frame after it closes (the frame
//! that closed it still painted it), drawn only on frames that draw the
//! editor: closed or hidden, it costs nothing.

use olr_material_editor::PreviewCommand;
use olr_material_editor::ui::PreviewImage;

use crate::editor::heat_preview::HeatPreview;
use crate::editor::material::MaterialHost;
use crate::gpu::Gpu;

/// The preview and its texture in the window's egui renderer.
pub(super) struct MaterialPreview {
    preview: HeatPreview,
    texture: egui::TextureId,
}

/// Before the frame's UI: the preview made for an open editor (its state
/// as the editor has it now) and its picture handed to the panel, or
/// dropped with its texture once the editor is closed.
pub(super) fn sync(slot: &mut Option<MaterialPreview>, gpu: &mut Gpu, host: Option<&mut MaterialHost>) {
    let Some(host) = host else {
        if let Some(p) = slot.take() {
            gpu.free_texture(p.texture);
        }
        return;
    };
    if slot.is_some() {
        return;
    }
    let mut preview = HeatPreview::new(gpu.renderer.device(), gpu.renderer.queue(), host.holes().cloned());
    let state = host.state();
    preview.set_open(state.doc());
    preview.command(PreviewCommand::SetCompare(state.compare_material().cloned()));
    preview.command(PreviewCommand::TimeScale(state.preview().scale.factor()));
    // What the editor asked before there was a preview is in its state.
    host.take_preview();
    let texture = gpu.register_texture(preview.view());
    host.set_preview(Some(PreviewImage { texture }));
    *slot = Some(MaterialPreview { preview, texture });
}

/// After the editor's frame (`Shell::material_frame`): its commands
/// carried out, the material open as edited given, and, when the editor
/// was drawn (`shown`), a frame `elapsed_ms` later drawn. The renderer's
/// warnings, for the log.
pub(super) fn frame(
    slot: &mut Option<MaterialPreview>,
    host: Option<&mut MaterialHost>,
    shown: bool,
    elapsed_ms: u32,
) -> Vec<String> {
    let (Some(p), Some(host)) = (slot.as_mut(), host) else { return Vec::new() };
    for c in host.take_preview() {
        p.preview.command(c);
    }
    p.preview.set_open(host.state().doc());
    if shown {
        p.preview.frame(elapsed_ms);
    }
    p.preview.take_warnings()
}
