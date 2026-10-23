//! The material editor's live heat preview on a GPU (issue #45): drawn by
//! `olr-render`'s heat pass on a renderer of its own, a blast heats the
//! patch, the heat cools, two materials side by side draw differently, a
//! field edit changes the next frame, and an idle preview draws nothing.
//! Runs with `OLR_GPU_TESTS` (`cargo xtask check --render`, `full`); skips
//! without it or an adapter.

use olr_data::image::{Rgba, RgbaImage};
use olr_format::Colour;
use olr_format::materials::{HeatLookSource, HeatSource, MaterialSource, RampKeySource};
use olr_game::editor::heat_preview::{HeatPreview, MAX_FRAME_MS, PICTURE_SCALE, SETTLE_FRAMES};
use olr_material_editor::preview::{PATCH, patch_x};
use olr_material_editor::{BlastSize, PreviewCommand};
use olr_render::{HEAT_SCALE, Offscreen};

fn offscreen() -> Option<Offscreen> {
    if std::env::var_os("OLR_GPU_TESTS").is_none() {
        eprintln!("OLR_GPU_TESTS not set (`cargo xtask check --render` sets it); skipping");
        return None;
    }
    match Offscreen::new() {
        Ok(g) => Some(g),
        Err(e) => {
            eprintln!("{e}; skipping");
            None
        }
    }
}

/// Stone's heat (every value 100), the engine's ramp.
fn stone() -> MaterialSource {
    MaterialSource {
        light_soak: None,
        id: "stone".into(),
        name: "Stone".into(),
        notes: String::new(),
        heat: Some(HeatSource {
            sensitivity: 100,
            capacity: 100,
            conductivity: 100,
            cooling: 100,
            look: HeatLookSource::default(),
        }),
    }
}

/// Stone that shows blue hot.
fn blue() -> MaterialSource {
    let mut m = stone();
    m.id = "blue".into();
    let key = RampKeySource { temperature: 1, colour: Colour(Rgba::rgb(0, 0, 255)), glow: 100 };
    m.heat.as_mut().unwrap().look.ramp = vec![key];
    m
}

/// The preview with `open` on the left and `compare` on the right.
fn preview(gpu: &Offscreen, open: Option<&MaterialSource>, compare: Option<MaterialSource>) -> HeatPreview {
    let mut p = HeatPreview::new(gpu.device(), gpu.queue(), None);
    p.set_open(open);
    p.command(PreviewCommand::SetCompare(compare));
    p
}

/// Where both patches are blasted: dirt under the ground's line.
const AT: [u32; 2] = [20, 30];

fn blast(p: &mut HeatPreview, size: BlastSize) {
    p.command(PreviewCommand::Blast { at: AT, size });
}

/// Each world pixel's temperature, degrees.
fn temps(gpu: &Offscreen, p: &HeatPreview) -> Vec<f32> {
    gpu.read_bytes(p.renderer().heat_texture().expect("the preview heats"))
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| f32::from(u16::from_le_bytes(*b)) / HEAT_SCALE)
        .collect()
}

/// The hottest pixel of patch `i`.
fn hottest(t: &[f32], i: usize) -> f32 {
    let w = olr_material_editor::preview::SCENE.0;
    (0..PATCH.1)
        .flat_map(|y| (0..PATCH.0).map(move |x| (y * w + patch_x(i) + x) as usize))
        .map(|at| t[at])
        .fold(0.0, f32::max)
}

fn picture(gpu: &Offscreen, p: &HeatPreview) -> RgbaImage {
    gpu.read_texture(p.target())
}

/// Patch `i`'s pixels round the blast, `r` world pixels each way, as one
/// list (the same order in both patches).
fn around(img: &RgbaImage, i: usize, r: u32) -> Vec<Rgba> {
    let s = PICTURE_SCALE;
    let (cx, cy) = (patch_x(i) + AT[0], AT[1]);
    let mut out = Vec::new();
    for y in (cy - r) * s..(cy + r) * s {
        for x in (cx - r) * s..(cx + r) * s {
            out.push(img.get(x, y));
        }
    }
    out
}

/// The most red of `px`.
fn reddest(px: &[Rgba]) -> u8 {
    px.iter().map(|c| c.r).max().unwrap_or(0)
}

/// The mean blueness (blue over red) of `px`.
fn blueness(px: &[Rgba]) -> f32 {
    px.iter().map(|c| f32::from(c.b) - f32::from(c.r)).sum::<f32>() / px.len() as f32
}

#[test]
fn a_blast_heats_the_patch_and_it_glows() {
    let Some(gpu) = offscreen() else { return };
    let mut p = preview(&gpu, Some(&stone()), None);
    assert!(p.frame(20));
    let cold = picture(&gpu, &p);
    assert_eq!(hottest(&temps(&gpu, &p), 0), 0.0);
    blast(&mut p, BlastSize::Bazooka);
    assert!(p.frame(20));
    let t = temps(&gpu, &p);
    assert!(hottest(&t, 0) > 200.0, "the rim takes the Bazooka's heat: {}", hottest(&t, 0));
    assert_eq!(hottest(&t, 1), 0.0, "no material on the right: no heat");
    let hot = picture(&gpu, &p);
    let (was, now) = (reddest(&around(&cold, 0, 10)), reddest(&around(&hot, 0, 10)));
    // Issue #56: only the rim's outer 2 px take the heat (red 43 to 60 on
    // lavapipe), where the dirt behind it glowed too (30 more).
    assert!(now > was + 12, "the crater's rim glows in the dark: red {now} against {was}");
    // The right patch's crater is carved too, but cold.
    assert!(reddest(&around(&hot, 1, 10)) <= reddest(&around(&cold, 1, 10)) + 4);
    // A nuke-like blast is hotter still; a small one, on stone, stays under
    // the glow but warms.
    let mut nuke = preview(&gpu, Some(&stone()), None);
    blast(&mut nuke, BlastSize::Nuke);
    nuke.frame(20);
    assert!(hottest(&temps(&gpu, &nuke), 0) > hottest(&t, 0));
    let mut small = preview(&gpu, Some(&stone()), None);
    blast(&mut small, BlastSize::Small);
    small.frame(20);
    let s = hottest(&temps(&gpu, &small), 0);
    assert!(s > 0.0 && s < 100.0, "a small blast's: {s}");
}

#[test]
fn the_heat_cools_and_an_idle_preview_draws_nothing() {
    let Some(gpu) = offscreen() else { return };
    let mut p = preview(&gpu, Some(&stone()), None);
    blast(&mut p, BlastSize::Bazooka);
    p.frame(20);
    let start = hottest(&temps(&gpu, &p), 0);
    for _ in 0..20 {
        p.frame(MAX_FRAME_MS);
    }
    let later = hottest(&temps(&gpu, &p), 0);
    assert!(later < start, "it cools: {later} from {start}");
    // At 4× the clock runs four times as fast: a frame's steps.
    p.command(PreviewCommand::TimeScale(4));
    let steps = p.renderer().heat_steps();
    p.frame(MAX_FRAME_MS);
    assert_eq!(p.renderer().heat_steps() - steps, u64::from(4 * MAX_FRAME_MS / olr_render::HEAT_STEP_MS));
    let mut frames = 0;
    while p.renderer().heat_active() {
        p.frame(MAX_FRAME_MS);
        frames += 1;
        assert!(frames < 2000, "still hot after {frames} frames");
    }
    assert!(temps(&gpu, &p).iter().all(|&v| v == 0.0), "cold again");
    // It settles, then draws nothing until something changes.
    for _ in 0..SETTLE_FRAMES + 1 {
        p.frame(20);
    }
    let drawn = p.drawn();
    for _ in 0..10 {
        assert!(!p.frame(20));
    }
    assert_eq!(p.drawn(), drawn, "idle: no frame drawn");
    p.command(PreviewCommand::Restart);
    assert!(p.frame(20), "a change draws again");
}

#[test]
fn side_by_side_draws_two_materials_differently() {
    let Some(gpu) = offscreen() else { return };
    // The same material on both: both patches heat alike.
    let mut same = preview(&gpu, Some(&stone()), Some(stone()));
    blast(&mut same, BlastSize::Bazooka);
    same.frame(20);
    let t = temps(&gpu, &same);
    assert_eq!(hottest(&t, 0), hottest(&t, 1), "identical patches");
    let img = picture(&gpu, &same);
    let (l, r) = (around(&img, 0, 8), around(&img, 1, 8));
    assert!((blueness(&l) - blueness(&r)).abs() < 2.0);
    // Blue on the right: the left glows as lava, the right blue.
    let mut two = preview(&gpu, Some(&stone()), Some(blue()));
    blast(&mut two, BlastSize::Bazooka);
    two.frame(20);
    let t = temps(&gpu, &two);
    assert_eq!(hottest(&t, 0), hottest(&t, 1), "the same heat");
    let img = picture(&gpu, &two);
    let (l, r) = (around(&img, 0, 8), around(&img, 1, 8));
    assert!(blueness(&r) > blueness(&l) + 20.0, "{} against {}", blueness(&r), blueness(&l));
    // Compared with none: the right takes no heat.
    two.command(PreviewCommand::SetCompare(None));
    blast(&mut two, BlastSize::Nuke);
    two.frame(20);
    assert!(hottest(&temps(&gpu, &two), 0) > 0.0);
}

#[test]
fn a_field_edit_changes_the_next_frame_and_keeps_the_heat() {
    let Some(gpu) = offscreen() else { return };
    let mut p = preview(&gpu, Some(&stone()), None);
    blast(&mut p, BlastSize::Nuke);
    p.frame(20);
    let before = temps(&gpu, &p);
    let lava = picture(&gpu, &p);
    // The ramp edited to blue, as the editor's field would: no restart.
    let mut edited = blue();
    edited.id = "stone".into();
    p.set_open(Some(&edited));
    assert!(p.frame(0), "a change draws");
    assert_eq!(temps(&gpu, &p), before, "the heat is kept");
    let now = picture(&gpu, &p);
    assert_ne!(now, lava, "the next frame shows the edit");
    assert!(blueness(&around(&now, 0, 8)) > blueness(&around(&lava, 0, 8)) + 20.0);
    // Takes heat off: the patch holds none any more.
    let mut numb = stone();
    numb.heat = None;
    p.set_open(Some(&numb));
    p.frame(20);
    assert!(p.renderer().heat_texture().is_none() || hottest(&temps(&gpu, &p), 0) == 0.0);
    // And on again: it heats from the next blast.
    p.set_open(Some(&stone()));
    blast(&mut p, BlastSize::Bazooka);
    p.frame(20);
    assert!(hottest(&temps(&gpu, &p), 0) > 0.0);
}
