//! The user packs layer in the game (`shell.rs`, spec §9.4, plan 13c): the
//! data directory's `packs/` over the bundled content (`--content`), as
//! the menus list it. A user pack replaces the bundled one of its id; a
//! pack of another id is added; without the layer the bundled one shows.

use std::path::Path;

use olr_audio::Audio;
use olr_data::image::RgbaImage;
use olr_data::level::{Level, LevelDef, Material, MaterialGrid};
use olr_data::manifest::{PACK_FORMAT_VERSION, PackKind, PackManifest};
use olr_game::shell::{Shell, ShellConfig};
use olr_scene::{Interpolation, Viewport};
use olr_ui::{Profiles, Screen, Settings, Store, UiAction, UiState};
use olr_vfs::{ContentSink, DirFs, MemFs};

/// A tiny valid level pack `id` named `name`, written to `root/<id>`.
fn level(root: &Path, id: &str, name: &str) {
    let manifest = PackManifest {
        format_version: PACK_FORMAT_VERSION,
        id: id.into(),
        name: name.into(),
        version: "1".into(),
        kind: PackKind::Level,
        authors: vec![],
        origin: None,
        license: None,
    };
    let level = Level {
        def: LevelDef {
            name: name.into(),
            width: 4,
            height: 4,
            art_scale: 1,
            theme: "olx-theme-dirt".into(),
            spawns: vec![],
        },
        material: MaterialGrid::new(4, 4, Material::Dirt),
        front: RgbaImage::new(4, 4),
        back: RgbaImage::new(4, 4),
    };
    let mut fs = MemFs::new();
    olr_pack::write_level(&mut fs, &olr_pack::LevelPack { manifest, level, thumb: None }).unwrap();
    let mut sink = DirFs::new(root.join(id));
    for (p, bytes) in fs.files() {
        sink.write(p, bytes).unwrap();
    }
}

fn shell(content: &Path, store: Store) -> Shell {
    let cfg = ShellConfig {
        content: content.to_path_buf(),
        interpolation: Interpolation::Linear,
        viewport: Viewport::from_aspect(16.0 / 9.0, 16.0 / 9.0),
    };
    let ui = UiState::new(Settings::default(), Profiles::default(), 1);
    Shell::new(cfg, ui, store, Audio::silent())
}

fn name_of(sh: &Shell, id: &str) -> Option<String> {
    sh.data.catalog.level(id).map(|l| l.name.clone())
}

#[test]
fn user_packs_layer_over_the_bundled_content() {
    let content = tempfile::tempdir().unwrap();
    level(content.path(), "shared-level", "Bundled");
    level(content.path(), "bundled-only", "Bundled only");

    // No user packs yet: the bundled levels.
    let data = tempfile::tempdir().unwrap();
    let mut sh = shell(content.path(), Store::open(data.path()));
    assert_eq!(name_of(&sh, "shared-level").as_deref(), Some("Bundled"));
    assert_eq!(name_of(&sh, "user-only"), None);
    drop(sh);

    // `<data>/packs` holds one level of a bundled id and one new level.
    let packs = data.path().join(olr_ui::PACKS_DIR);
    level(&packs, "shared-level", "User");
    level(&packs, "user-only", "User only");
    sh = shell(content.path(), Store::open(data.path()));
    assert_eq!(name_of(&sh, "shared-level").as_deref(), Some("User"), "the user pack wins");
    assert_eq!(name_of(&sh, "user-only").as_deref(), Some("User only"));
    assert_eq!(name_of(&sh, "bundled-only").as_deref(), Some("Bundled only"), "the rest stays");
    assert_eq!(sh.data.catalog.levels.iter().filter(|l| l.id == "shared-level").count(), 1);

    // The menus draw them (local play lists the levels).
    sh.act(UiAction::Open(Screen::LocalPlay));
    let ctx = egui::Context::default();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 720.0))),
        ..Default::default()
    };
    let mut out = ctx.run_ui(input, |ui| {
        olr_ui::screens::show(ui, &sh.ui, &mut sh.data);
    });
    out.textures_delta.clear(); // no renderer here

    // Without a data directory the bundled content shows alone.
    let sh = shell(content.path(), Store::in_memory());
    assert_eq!(name_of(&sh, "shared-level").as_deref(), Some("Bundled"));
    assert_eq!(name_of(&sh, "user-only"), None);
}
