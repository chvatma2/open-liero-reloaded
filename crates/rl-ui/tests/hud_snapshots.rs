//! Image snapshots of the HUD (plan 7 task 5) at 1080p and 1440p, and the
//! side layout at 3440 × 1440,
//! rendered by egui's wgpu renderer (`egui_kittest`, its software adapter
//! preferred) and compared with `fixtures/ui/hud-*.png`. They need a
//! graphics adapter: the GPU tier (`cargo xtask full`, `RL_GPU_TESTS`)
//! runs them; elsewhere they skip. `cargo xtask bless-render --reason
//! <why>` re-records them (with `RL_BLESS_RENDER`) and logs why.

use egui::{Color32, ColorImage, Pos2, Rect, TextureHandle, TextureOptions, pos2, vec2};
use egui_kittest::{Harness, SnapshotOptions};
use rl_ui::HudArrangement;
use rl_ui::hud::{self, Bar, Dot, HudView, LogLine, MinimapView, Notice, Row, SlotHud, WormHud, minimap};

/// The accepted snapshots, from the crate's directory.
const DIR: &str = "../../fixtures/ui";

fn gpu_tests() -> bool {
    let on = std::env::var_os("RL_GPU_TESTS").is_some();
    if !on {
        eprintln!("RL_GPU_TESTS not set; skipping the HUD snapshots");
    }
    on
}

/// A level picture: earth with rock bands and a sky.
fn level() -> (Vec<u8>, u32, u32) {
    let (w, h) = (504u32, 350u32);
    let mut px = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let c = if y < 60 {
                [40, 40, 90]
            } else if (x / 40 + y / 30) % 5 == 0 {
                [100, 100, 100]
            } else if (x * 7 + y * 3) % 23 < 4 {
                [24, 16, 8]
            } else {
                [136, 84, 40]
            };
            px.extend_from_slice(&[c[0], c[1], c[2], 255]);
        }
    }
    (px, w, h)
}

fn row(worm: i32, name: &str, kills: i32, deaths: i32, damage: i32) -> Row {
    Row { worm, name: name.into(), score: kills, lives: "∞".into(), kills, deaths, suicides: 0, damage }
}

fn view(texture: egui::TextureId, scores: bool, arrangement: HudArrangement) -> HudView {
    let slot = |name: &str, charge, bar| SlotHud { name: name.into(), charge, bar };
    HudView {
        worm: Some(WormHud {
            name: "OpenLieroXor".into(),
            health: 72.0,
            alive: true,
            lives: 3,
            kills: 4,
            deaths: 2,
            suicides: 0,
            slots: vec![
                slot("Minigun", 1.0, Bar::Ready),
                slot("Super Shotgun", 0.35, Bar::Reloading),
                slot("Blaster", 0.8, Bar::Cooldown),
                slot("Gauss Gun", 1.0, Bar::Ready),
                slot("Big Nuke", 0.1, Bar::Reloading),
            ],
            current: 1,
            status: None,
        }),
        log: [
            "Bot 2 killed Bot 1",
            "OpenLieroXor drew first blood",
            "Bot 1 killed OpenLieroXor",
            "Bot 2 is on a killing spree!",
        ]
        .into_iter()
        .map(LogLine::event)
        .collect(),
        time_left_ms: Some(83_000),
        kill_limit: Some(15),
        notices: vec![Notice { text: "Replay 12.0 s / 90.0 s".into(), warn: false }],
        scoreboard: scores.then(|| {
            vec![row(1, "Bot 1", 5, 1, 380), row(0, "OpenLieroXor", 4, 2, 310), row(2, "Bot 2", 0, 6, 45)]
        }),
        minimap: Some(MinimapView {
            texture,
            map: vec2(504.0, 350.0),
            dots: vec![
                Dot { pos: pos2(250.0, 180.0), colour: Color32::from_rgb(0, 177, 0), big: true },
                Dot { pos: pos2(60.0, 300.0), colour: Color32::from_rgb(255, 0, 0), big: false },
                Dot { pos: pos2(480.0, 100.0), colour: Color32::from_rgb(0, 0, 255), big: false },
            ],
            blink: 0.0,
        }),
        game_over: false,
        hint: Some("Tab scores   F2 debug   F5 rematch   Esc pause".into()),
        arrangement,
    }
}

fn snapshot(size: (f32, f32), name: &str, scores: bool, arrangement: HudArrangement) {
    let mut h =
        Harness::builder().with_size(vec2(size.0, size.1)).with_pixels_per_point(1.0).wgpu().build_ui_state(
            move |ui, tex: &mut Option<TextureHandle>| {
                let t = tex.get_or_insert_with(|| {
                    let (px, w, hgt) = level();
                    let img: ColorImage = minimap::image(&px, w, hgt, minimap::TEXTURE_SIZE);
                    ui.ctx().load_texture("minimap", img, TextureOptions::LINEAR)
                });
                // The game behind the HUD.
                ui.painter().rect_filled(
                    Rect::from_min_size(Pos2::ZERO, vec2(size.0, size.1)),
                    0.0,
                    Color32::from_rgb(70, 50, 30),
                );
                hud::show(ui, &view(t.id(), scores, arrangement));
            },
            None,
        );
    h.run();
    if std::env::var_os("RL_BLESS_RENDER").is_some() {
        // `cargo xtask bless-render --reason <why>`: logged in
        // `fixtures/blessings.log`.
        let img = h.render().expect("a rendered HUD");
        img.save(format!("{DIR}/{name}.png")).expect("the snapshot written");
        return;
    }
    h.snapshot_options(name, &SnapshotOptions::new().output_path(DIR));
}

#[test]
fn hud_snapshot_1080p() {
    if !gpu_tests() {
        return;
    }
    snapshot((1920.0, 1080.0), "hud-1080p", false, HudArrangement::Auto);
}

#[test]
fn hud_snapshot_1440p_with_scores() {
    if !gpu_tests() {
        return;
    }
    snapshot((2560.0, 1440.0), "hud-1440p", true, HudArrangement::Auto);
}

/// An ultrawide window's side layout: the health and weapons left of the
/// 16:9 frame, the log right of it, the minimap alone in the bar.
#[test]
fn hud_snapshot_3440x1440_side_panels() {
    if !gpu_tests() {
        return;
    }
    snapshot((3440.0, 1440.0), "hud-3440x1440-sides", false, HudArrangement::Sides);
}
