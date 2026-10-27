//! GPU render goldens (plan 5 task 5): `olr-cli render <scenario> --frame N`
//! of one scenario on each level must look like its committed PNG in
//! `fixtures/render/` (a perceptual diff, so two software rasterisers that
//! round a blend differently still agree).
//!
//! The goldens are drawn from the imported OLX content at test time (spec
//! §9.3: converted assets are never committed, the goldens are accepted
//! fixtures); they were blessed on lavapipe (Vulkan, CPU), the adapter
//! `olr_render::Offscreen` prefers. They run in `cargo xtask full`, which
//! sets `OLR_GPU_TESTS`; without it, the content or an adapter they skip
//! with a notice. `cargo xtask bless-render --reason "<why>"` re-draws
//! them (`OLR_BLESS_RENDER`).

use std::path::{Path, PathBuf};
use std::process::Command;

use olr_data::image::RgbaImage;

/// A pixel differs when its YIQ distance is above this (pixelmatch's 0.1).
const TOLERANCE: f32 = 0.1;
/// The share of differing pixels a golden tolerates: about 6 pixels at
/// 640x480. Lavapipe and llvmpipe (GL) both reproduce all four goldens with
/// no differing pixel; the allowance is for another LLVM's rounding.
const MAX_DIFFERING: f64 = 0.00002;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).expect("the workspace root").to_path_buf()
}

fn blessing() -> bool {
    std::env::var_os("OLR_BLESS_RENDER").is_some()
}

/// The content directory, if GPU tests are on and the content is imported.
fn content_dir() -> Option<String> {
    if std::env::var_os("OLR_GPU_TESTS").is_none() && !blessing() {
        eprintln!("OLR_GPU_TESTS not set (`cargo xtask full` sets it); skipping");
        return None;
    }
    let dir = std::env::var("OLR_CONTENT_DIR").ok().filter(|d| Path::new(d).is_dir());
    if dir.is_none() {
        assert!(!blessing(), "blessing needs the imported content (`cargo xtask fetch-olx-content`)");
        eprintln!("OLR_CONTENT_DIR not set or missing (run `cargo xtask fetch-olx-content`); skipping");
    }
    dir
}

/// Renders `scenario` at `frame` with `extra` options into
/// `target/render-actual/<name>` and compares it with
/// `fixtures/render/<name>`, or writes the golden when blessing.
fn golden(name: &str, scenario: &str, frame: i64, extra: &[&str]) {
    let Some(content) = content_dir() else { return };
    let fixtures = root().join("fixtures/oracle");
    let frame = frame.to_string();
    let args = [
        "render",
        scenario,
        "--frame",
        &frame,
        "--content",
        &content,
        "--fixtures",
        fixtures.to_str().unwrap(),
    ];
    compare(name, &args, extra);
}

/// `olr-cli look-preview` of the look test level (`fixtures/look/levels/
/// look-test`, plan 15a Task 14) centred on its building, with `extra`
/// options, against `fixtures/render/<name>`.
fn look_golden(name: &str, extra: &[&str]) {
    look_golden_of("look-test", name, extra);
}

/// [`look_golden`] of the level pack `pack` in `fixtures/look/levels/`.
fn look_golden_of(pack: &str, name: &str, extra: &[&str]) {
    let Some(content) = content_dir() else { return };
    let pack = root().join("fixtures/look/levels").join(pack);
    let args = ["look-preview", pack.to_str().unwrap(), "--at", "256,128", "--content", &content];
    compare(name, &args, extra);
}

/// Runs `olr-cli <args> --out target/render-actual/<name> <extra>` and
/// compares the PNG with `fixtures/render/<name>`, or blesses it.
fn compare(name: &str, args: &[&str], extra: &[&str]) {
    let actual = root().join("target/render-actual").join(name);
    std::fs::create_dir_all(actual.parent().unwrap()).unwrap();
    let scenario = args[1];
    let out = Command::new(env!("CARGO_BIN_EXE_olr-cli"))
        .args(args)
        .arg("--out")
        .arg(&actual)
        .args(extra)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() && stderr.contains("no graphics adapter") && !blessing() {
        eprintln!("{stderr}skipping");
        return;
    }
    assert!(out.status.success(), "olr-cli {} {scenario} failed:\n{stderr}", args[0]);
    eprint!("{}", String::from_utf8_lossy(&out.stdout));
    let decode = |p: &Path| std::fs::read(p).ok().and_then(|b| olr_pack::image_io::decode_rgba(&b).ok());
    let img: RgbaImage = decode(&actual).expect("olr-cli wrote a PNG");
    let path = root().join("fixtures/render").join(name);
    let committed = decode(&path);
    if blessing() {
        // Keep the committed file when it still passes, so a re-bless on
        // another rasteriser does not churn the goldens.
        let same = committed.as_ref().and_then(|c| olr_render::perceptual_diff(c, &img, TOLERANCE));
        if !same.is_some_and(|d| d.differing == 0) {
            std::fs::copy(&actual, &path).unwrap();
            eprintln!("blessed {}", path.display());
        }
        return;
    }
    let bless = "if the change is intended, run `cargo xtask bless-render --reason \"<why>\"`";
    let Some(committed) = committed else {
        panic!("{}: missing or unreadable golden; {bless}", path.display());
    };
    let Some(d) = olr_render::perceptual_diff(&committed, &img, TOLERANCE) else {
        panic!(
            "{name}: the size is {}x{}, not {}x{}; the actual image is {}; {bless}",
            img.width(),
            img.height(),
            committed.width(),
            committed.height(),
            actual.display()
        );
    };
    assert!(
        d.fraction() <= MAX_DIFFERING,
        "{name}: {} of {} pixels differ (max distance {:.3}) from {}; the actual image is {}; {bless}",
        d.differing,
        d.total,
        d.max_distance,
        path.display(),
        actual.display()
    );
}

/// Dirt Level: digging through solid dirt, interpolated half way between
/// two ticks and upscaled 3× (sharp bilinear over a sub-pixel camera).
#[test]
fn offscreen_renders_dig_frame_like_golden() {
    golden("frame-dig.png", "dig", 200, &["--scale", "3", "--alpha", "0.5"]);
}

/// Liero Factory: swinging on the ninja rope.
#[test]
fn offscreen_renders_rope_frame_like_golden() {
    golden("frame-rope.png", "rope", 160, &[]);
}

/// FossilFacility: a bazooka hit, blood and smoke.
#[test]
fn offscreen_renders_bazooka_frame_like_golden() {
    golden("frame-bazooka.png", "weapons/classic/bazooka", 56, &[]);
}

/// CastleStrike, the 1× level: the rope to the hall's ceiling, integer
/// nearest at 2×.
#[test]
fn offscreen_renders_castle_frame_like_golden() {
    golden("frame-castle.png", "castle-shoot-rope", 200, &["--filter", "integer-nearest"]);
}

/// The look test level lit: sky, windows, lava, two lamps, dark ambient,
/// a grade (plan 15a Task 14).
#[test]
fn look_test_level_lit() {
    look_golden("look-lit.png", &[]);
}

/// Its light alone.
#[test]
fn look_test_level_light() {
    look_golden("look-light.png", &["--view", "light"]);
}

/// Full bright: the art as it is.
#[test]
fn look_test_level_full() {
    look_golden("look-full.png", &["--view", "full"]);
}

/// Half a second on: the lava flickers and the blue lamp pulses.
#[test]
fn look_test_level_at_500_ms() {
    look_golden("look-t500.png", &["--time", "500"]);
}

/// Issue #10: a copy of the look test level with ambient occlusion
/// (strength 60, reach 12): its tunnels and the air by its rock darker.
#[test]
fn look_test_level_occluded() {
    look_golden_of("look-test-occlusion", "look-occlusion.png", &[]);
}

/// Its light alone: the occlusion shows plainly.
#[test]
fn look_test_level_occluded_light() {
    look_golden_of("look-test-occlusion", "look-occlusion-light.png", &["--view", "light"]);
}

/// Issue #11: `look-preview` of the distant demo level (a copy of the look
/// test level, `fixtures/look/levels/look-test-distant`: its base split,
/// two gaps erased through every layer, a night sky at depth 90 and a far
/// city at depth 55, both images, `level element add-image … --material
/// background` then `set --distant N`) centred on `at`.
fn distant_golden(name: &str, at: &str, extra: &[&str]) {
    let Some(content) = content_dir() else { return };
    let pack = root().join("fixtures/look/levels/look-test-distant");
    compare(name, &["look-preview", pack.to_str().unwrap(), "--at", at, "--content", &content], extra);
}

/// The camera at the level's top-left as far as the view goes: the city
/// and the moon shifted by their depths, lit by the dark ambient.
#[test]
fn distant_layers_top_left() {
    distant_golden("look-distant-a.png", "213,96", &[]);
}

/// The camera at its bottom-right: the layers moved less than the level.
#[test]
fn distant_layers_bottom_right() {
    distant_golden("look-distant-b.png", "299,160", &[]);
}

/// Full bright: the layers' own colours.
#[test]
fn distant_layers_full_bright() {
    distant_golden("look-distant-full.png", "213,96", &["--view", "full"]);
}

/// A chaos-bot match on the look test level with the look test mod's
/// Glow Rocket (plan 15a Task 16): rockets with their light, end flashes,
/// muzzle and explosion lights, the worms' floor.
#[test]
fn look_rocket() {
    let Some(content) = content_dir() else { return };
    let (mods, levels) = (root().join("fixtures/look/mods"), root().join("fixtures/look/levels"));
    let args = [
        "render-match",
        "--mod",
        "look-test-mod",
        "--level",
        "look-test",
        "--bots",
        "3",
        "--personality",
        "chaos",
        "--seed",
        "1",
        "--ticks",
        "300",
        "--packs",
        mods.to_str().unwrap(),
        "--packs",
        levels.to_str().unwrap(),
        "--content",
        &content,
    ];
    compare("look-rocket.png", &args, &[]);
}

/// Issue #15: [`look_rocket`]'s match with its copy, the look test blast
/// mod (`fixtures/look/mods/look-test-blast-mod`), whose Glow Rocket
/// explodes in a large blue light (`#4080ff`, 500 %, a disc of 14, reach
/// 160) instead of the engine's.
#[test]
fn look_blue_explosion() {
    let Some(content) = content_dir() else { return };
    let (mods, levels) = (root().join("fixtures/look/mods"), root().join("fixtures/look/levels"));
    let args = [
        "render-match",
        "--mod",
        "look-test-blast-mod",
        "--level",
        "look-test",
        "--bots",
        "3",
        "--personality",
        "chaos",
        "--seed",
        "1",
        "--ticks",
        "300",
        "--packs",
        mods.to_str().unwrap(),
        "--packs",
        levels.to_str().unwrap(),
        "--content",
        &content,
    ];
    compare("look-blast.png", &args, &[]);
}

/// Issue #19: [`look_rocket`]'s match with the look test glow mod
/// (`fixtures/look/mods/look-test-glow-mod`), whose Glow Rocket is an image
/// (Classic's missile, its tip painted hot with `olr-cli mod gfx`) with a
/// glow mask on the tip (`gfx/rocket.glow.png`) and no whole-sprite glow:
/// the tips show bright in the dark, the rest of the sprite as lit.
#[test]
fn look_glowing_rocket_tip() {
    let Some(content) = content_dir() else { return };
    let (mods, levels) = (root().join("fixtures/look/mods"), root().join("fixtures/look/levels"));
    let args = [
        "render-match",
        "--mod",
        "look-test-glow-mod",
        "--level",
        "look-test",
        "--bots",
        "3",
        "--personality",
        "chaos",
        "--seed",
        "1",
        "--ticks",
        "300",
        "--packs",
        mods.to_str().unwrap(),
        "--packs",
        levels.to_str().unwrap(),
        "--content",
        &content,
    ];
    compare("look-glow-tip.png", &args, &[]);
}

/// Issue #12: idle bots on the dark skin test level (`fixtures/look/levels/
/// look-test-skin-level`) wearing the hand-made double-resolution skin
/// (`fixtures/look/skins/look-test-skin`, art scale 4): its cyan visor
/// glows, its light lights the air and the rock pillar stops it. At
/// `--scale 4`, one output pixel per skin art pixel.
#[test]
fn look_double_skin() {
    let Some(content) = content_dir() else { return };
    let (skins, levels) = (root().join("fixtures/look/skins"), root().join("fixtures/look/levels"));
    let args = [
        "render-match",
        "--mod",
        "olx-classic",
        "--level",
        "look-test-skin-level",
        "--bots",
        "2",
        "--personality",
        "idle",
        "--seed",
        "4",
        "--ticks",
        "500",
        "--skin",
        "look-test-skin",
        "--scale",
        "4",
        "--packs",
        skins.to_str().unwrap(),
        "--packs",
        levels.to_str().unwrap(),
        "--content",
        &content,
    ];
    compare("look-skin.png", &args, &[]);
}

/// Issue #87: idle bots on the live test level (`fixtures/live/levels/
/// live-test`, its copies in their starting states, or with `extra`'s
/// `--live` states), the copies composited into the terrain and lit by
/// their own glow groups.
fn live_golden(name: &str, extra: &[&str]) {
    let Some(content) = content_dir() else { return };
    let levels = root().join("fixtures/live/levels");
    let args = [
        "render-match",
        "--mod",
        "olx-classic",
        "--level",
        "olr-level-live-test",
        "--bots",
        "2",
        "--personality",
        "idle",
        "--seed",
        "1",
        "--ticks",
        "200",
        "--packs",
        levels.to_str().unwrap(),
        "--content",
        &content,
    ];
    compare(name, &args, extra);
}

/// Issue #87: the live copies as they start (the lamp dim, the second
/// console alarmed).
#[test]
fn live_start() {
    live_golden("live-start.png", &[]);
}

/// Issue #87: the lamp off and the first console alarmed before the
/// first tick (`--live`).
#[test]
fn live_states() {
    live_golden("live-states.png", &["--live", "lamp=Off", "--live", "console-a=Alarm"]);
}

/// Issue #34, byte identity: chaos bots on Castle Strike Reloaded (its
/// look: the flames' flicker and the sky's glow), its last 30 ticks drawn
/// lit (`olr-cli frame-hashes`), a hash of each frame's light grid and lit
/// target against `fixtures/render/lit-hashes.ron`: a change meant to keep
/// the picture keeps every byte. Hashes recorded on another adapter are not
/// compared (a notice); a different world is another match, said so.
#[test]
fn lit_frames_keep_their_bytes() {
    use olr_cli::frame_hash::{self, Comparison, FrameHashes};
    let Some(content) = content_dir() else { return };
    let actual = root().join("target/render-actual/lit-hashes.ron");
    std::fs::create_dir_all(actual.parent().unwrap()).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_olr-cli"))
        .args(["frame-hashes", "--mod", "olx-classic", "--level", "olr-level-castlestrike-reloaded"])
        .args(["--bots", "4", "--personality", "chaos", "--seed", "1", "--ticks", "330", "--frames", "30"])
        .args(["--content", &content, "--out"])
        .arg(&actual)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() && stderr.contains("no graphics adapter") && !blessing() {
        eprintln!("{stderr}skipping");
        return;
    }
    assert!(out.status.success(), "olr-cli frame-hashes failed:\n{stderr}");
    let read = |p: &Path| -> Option<FrameHashes> { ron::from_str(&std::fs::read_to_string(p).ok()?).ok() };
    let now = read(&actual).expect("olr-cli wrote the hashes");
    let path = root().join("fixtures/render/lit-hashes.ron");
    let committed = read(&path);
    let same = committed.as_ref().map(|c| frame_hash::compare(c, &now));
    if blessing() {
        if same != Some(Comparison::Same) {
            std::fs::copy(&actual, &path).unwrap();
            eprintln!("blessed {}", path.display());
        }
        return;
    }
    let bless = "if the change is intended, run `cargo xtask bless-render --reason \"<why>\"`";
    match same {
        None => panic!("{}: missing or unreadable; {bless}", path.display()),
        Some(Comparison::Same) => {}
        Some(Comparison::OtherAdapter(why)) => eprintln!("{why}; skipping"),
        Some(Comparison::Differs(why)) => {
            panic!("{why} (against {}; this run's: {}); {bless}", path.display(), actual.display())
        }
    }
}

/// Issue #40's heat match: two chaos bots on the heat test level
/// (`fixtures/look/levels/look-test-heat`: stone, dirt, wood, metal and
/// brick in a dark cave) with the heat test mod
/// (`fixtures/look/mods/look-test-heat-mod`: a Bazooka, a Minigun and a
/// Nuke, Classic's with a 12.5 s reload), seed 1, `ticks` ticks.
fn heat_match(name: &str, ticks: &str) {
    heat_match_on(name, "look-test-heat", ticks);
}

/// [`heat_match`] on the fixture level `level`.
fn heat_match_on(name: &str, level: &str, ticks: &str) {
    let Some(content) = content_dir() else { return };
    let (mods, levels) = (root().join("fixtures/look/mods"), root().join("fixtures/look/levels"));
    let args = [
        "render-match",
        "--mod",
        "look-test-heat-mod",
        "--level",
        level,
        "--bots",
        "2",
        "--personality",
        "chaos",
        "--seed",
        "1",
        "--ticks",
        ticks,
        "--packs",
        mods.to_str().unwrap(),
        "--packs",
        levels.to_str().unwrap(),
        "--content",
        &content,
    ];
    compare(name, &args, &[]);
}

/// Issue #40: the nuke's fragments have just carved the floor and the
/// mound: their rims glow from yellow-white to orange, the right dirt
/// wall's Bazooka craters orange, and the hot rock lights the cave.
#[test]
fn heat_fresh_rims_glow() {
    heat_match("heat-fresh.png", "380");
}

/// Issue #40: [`heat_fresh_rims_glow`]'s match 5.2 s later, nothing fired
/// since (the weapons reload): the rims have cooled to dull orange and red.
#[test]
fn heat_cooled_rims() {
    heat_match("heat-cooled.png", "900");
}

/// Issue #69's beam match: two chaos bots firing Classic's Laser
/// (`fixtures/look/mods/look-test-laser-mod`: each slot empty after five
/// bursts, then a 12.5 s reload) on the heat test level, seed 1, `ticks`
/// ticks.
fn laser_match(name: &str, ticks: &str) {
    let Some(content) = content_dir() else { return };
    let (mods, levels) = (root().join("fixtures/look/mods"), root().join("fixtures/look/levels"));
    let args = [
        "render-match",
        "--mod",
        "look-test-laser-mod",
        "--level",
        "look-test-heat",
        "--bots",
        "2",
        "--personality",
        "chaos",
        "--seed",
        "1",
        "--ticks",
        ticks,
        "--packs",
        mods.to_str().unwrap(),
        "--packs",
        levels.to_str().unwrap(),
        "--content",
        &content,
    ];
    compare(name, &args, &[]);
}

/// Issue #69: both bots' lasers light the cave along their beams and at
/// their tips, and the edges of the tunnels they dug in the dirt walls and
/// the mound glow with heat.
#[test]
fn beam_fresh_lights_and_heats() {
    laser_match("beam-fresh.png", "200");
}

/// Issue #69: [`beam_fresh_lights_and_heats`]'s match 1 s later, nothing
/// fired for 0.5 s (every slot reloads): no beam light, the tunnels' edges
/// cooler, dull orange and red.
#[test]
fn beam_cooled_edges() {
    laser_match("beam-cooled.png", "300");
}

/// Issue #41: [`heat_match`] on its distant copy
/// (`fixtures/look/levels/look-test-heat-distant`: the cave open onto a
/// wooden palisade at depth 8 before a stone sky at depth 80): behind each
/// explosion the palisade glows in a smaller circle than the floor's rim,
/// and the sky, beyond any explosion's reach, stays as it was.
#[test]
fn heat_distant_layer_glows() {
    heat_match_on("heat-distant.png", "look-test-heat-distant", "380");
}

/// Issue #40: the heat does not depend on the frame rate: [`heat_match`]'s
/// match drawn every tick and drawn once at its end (as `render-match`
/// does) leave the same heat, except where the terrain changed after heat
/// landed on it (a frame drawn every tick saw the pixel before it was dug;
/// one frame at the end sees only the dug pixel).
#[test]
fn heat_is_the_same_drawn_every_tick_or_once() {
    let Some(h) = HeatMatch::open() else { return };
    let (every, changed, size) = h.play(1, |_| {});
    let (once, _, _) = h.play(h.spec.ticks, |_| {});
    let hot = same_but_where_dug(&every, &once, &changed, size);
    assert!(hot > 1000, "the match heated the level: {hot} texels");
}

/// Issue #42: a projectile whose look has `heat` warms the level along its
/// path, the same drawn every tick or once: [`heat_match`]'s match with
/// every Minigun bullet heating each tick and no explosion heating
/// (`explosion_heat` 0 %) is hot only by the bullets' paths; with neither,
/// nothing is. The bullets' heat reaches as far as a projectile's may
/// (16 px), past the back wall 14 px behind the plane (issue #65), so
/// their paths through the air warm it; a smaller ball would heat only the
/// dirt beside their holes, which the bullets dig, and drawn once that heat
/// lands on the dug terrain.
#[test]
fn a_projectiles_heat_warms_its_path_drawn_every_tick_or_once() {
    let Some(h) = HeatMatch::open() else { return };
    let cold = |m: &mut olr_look::ModLook| {
        m.explosion_heat = Some(olr_look::ExplosionHeatLook { amount: 0, radius: None });
    };
    let hot = |m: &mut olr_look::ModLook| {
        cold(m);
        let bullets = h.assets.m.sim.projectiles().iter().map(|p| p.key.starts_with("minigun"));
        for (look, bullet) in m.projectiles.iter_mut().zip(bullets) {
            if bullet {
                let heat = olr_look::ProjectileHeat {
                    amount: 40,
                    radius: olr_look::engine::MAX_PROJECTILE_HEAT_RADIUS,
                };
                look.get_or_insert_with(Default::default).heat = Some(heat);
            }
        }
    };
    let (none, _, _) = h.play(h.spec.ticks, cold);
    assert!(none.iter().all(|&v| v == 0), "no explosion heat and no bullet heat: cold");
    let (every, changed, size) = h.play(1, hot);
    let (once, _, _) = h.play(h.spec.ticks, hot);
    let warm = same_but_where_dug(&every, &once, &changed, size);
    assert!(warm > 200, "the bullets warmed their paths: {warm} texels");
}

/// [`heat_match`]'s match, played headless with its heat drawn.
struct HeatMatch {
    gpu: olr_render::Offscreen,
    content: olr_content::Content,
    spec: olr_cli::spec::MatchSpec,
    assets: olr_cli::assets::LevelAssets,
}

impl HeatMatch {
    /// `None` without content or an adapter (skipped).
    fn open() -> Option<HeatMatch> {
        use olr_cli::spec::{MatchSpec, Personality};
        let dir = content_dir()?;
        let gpu = match olr_render::Offscreen::new() {
            Ok(gpu) => gpu,
            Err(e) => {
                eprintln!("{e}; skipping");
                return None;
            }
        };
        let (mods, levels) = (root().join("fixtures/look/mods"), root().join("fixtures/look/levels"));
        let bundled = olr_content::bundled_roots(Path::new(&dir));
        let roots: Vec<&Path> = [&mods, &levels].into_iter().chain(&bundled).map(PathBuf::as_path).collect();
        let content = olr_content::Content::open_layers(&roots).unwrap();
        let mut spec = MatchSpec::new("look-test-heat-mod", "look-test-heat", 2, 1, 400);
        spec.personality = Personality::Chaos;
        let assets = olr_cli::assets::LevelAssets::load(&content, &spec.mod_id, &spec.level_id).unwrap();
        Some(HeatMatch { gpu, content, spec, assets })
    }

    /// The heat after the match (texels of the level's heat texture), its
    /// mod's look made as `look` makes it, drawing a frame after every
    /// `stride`th tick and after the last; and whether each world pixel's
    /// material changed after heat may have reached it.
    fn play(
        &self,
        stride: u32,
        look: impl FnOnce(&mut olr_look::ModLook),
    ) -> (Vec<u16>, Vec<bool>, (usize, usize)) {
        use olr_cli::runner::{Match, Options};
        use olr_present::{PresentSetup, Presentation, default_colour};
        use olr_render::{RenderSettings, UpscaleFilter};
        use olr_scene::{DrawList, Interpolation, SceneOptions, Viewport};

        let (gpu, content, spec) = (&self.gpu, &self.content, &self.spec);
        let mut m = self.assets.m.clone();
        look(&mut m.look);
        let size = (Viewport::GAME.width as u32, Viewport::GAME.height as u32);
        let output = gpu.output_texture(size);
        let view = output.create_view(&wgpu::TextureViewDescriptor::default());
        let mut game = Match::with_assets(&self.assets, spec, &Options::default()).unwrap();
        let setup = PresentSetup {
            core: content.core(),
            m: &m,
            level: &self.assets.level,
            theme: &self.assets.theme,
            viewport: Viewport::GAME,
            options: SceneOptions { interpolation: Interpolation::Latest, show_health_bars: false },
            seed: spec.seed,
        };
        let mut present = Presentation::new(&setup, game.world());
        let skin = content.skin_or_default("olx-skin-default");
        for i in 0..game.world().worms().len() {
            present.add_worm(&skin, default_colour(i), i == 0, false);
        }
        let mut renderer = gpu.renderer(RenderSettings { filter: UpscaleFilter::IntegerNearest });
        let (w, h) = (game.world().terrain().width(), game.world().terrain().height());
        let n = w as usize * h as usize;
        // Each world pixel's last change of material and first heat within
        // its reach (match ms; `u64::MAX` none).
        let (mut list, mut changed, mut heated) = (DrawList::new(), vec![0; n], vec![u64::MAX; n]);
        for tick in 1..=spec.ticks {
            let before = game.world().terrain().clone();
            game.advance();
            present.on_tick(game.world(), game.events());
            let now = game.world().time().0;
            for (i, c) in changed.iter_mut().enumerate() {
                let (x, y) = ((i % w as usize) as i64, (i / w as usize) as i64);
                if before.material(x, y) != game.world().terrain().material(x, y) {
                    *c = now;
                }
            }
            if tick % stride == 0 || tick == spec.ticks {
                present.build(game.world(), 1.0, &mut list);
                // Heat may reach a deposit's circle and spread 48 px past it.
                for &(at, d) in present.heat_waiting() {
                    let r = d.radius as i64 + 48;
                    let (cx, cy) = (d.pos[0] as i64, d.pos[1] as i64);
                    for y in (cy - r).max(0)..(cy + r + 1).min(i64::from(h)) {
                        for x in (cx - r).max(0)..(cx + r + 1).min(i64::from(w)) {
                            let i = y as usize * w as usize + x as usize;
                            heated[i] = heated[i].min(at);
                        }
                    }
                }
                present.upload(&mut renderer);
                renderer.render_lit(&list, present.look(), &view, [0, 0, size.0, size.1]);
            }
        }
        assert!(renderer.has_heat(), "the heat test level heats");
        let heat: Vec<u16> = gpu
            .read_bytes(renderer.heat_texture().unwrap())
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| u16::from_le_bytes(*b))
            .collect();
        // Dug or filled after heat may have reached it.
        let late: Vec<bool> = changed.iter().zip(&heated).map(|(&c, &h)| h != u64::MAX && c > h).collect();
        (heat, late, (w as usize, h as usize))
    }
}

/// Asserts that two heat textures of a level of `(w, h)` are the same but
/// near the pixels `changed` marks (where conduction carries a
/// difference), and those few; the texels hot in `every`.
fn same_but_where_dug(every: &[u16], once: &[u16], changed: &[bool], (w, h): (usize, usize)) -> usize {
    assert_eq!(every.len(), w * h);
    let hot = every.iter().filter(|&&v| f32::from(v) > olr_render::HEAT_SCALE).count();
    let (mut same, mut differ, mut near) = (0, 0, 0);
    for (i, (a, b)) in every.iter().zip(once).enumerate() {
        if a == b {
            same += 1;
            continue;
        }
        differ += 1;
        let (x, y) = (i % w, i / w);
        let by_dig = (y.saturating_sub(R)..(y + R + 1).min(h))
            .any(|yy| (x.saturating_sub(R)..(x + R + 1).min(w)).any(|xx| changed[yy * w + xx]));
        near += usize::from(by_dig);
    }
    eprintln!("{hot} hot texels; {same} the same, {differ} differ, {near} of them near a dug pixel");
    assert_eq!(differ, near, "the heat differs only where the terrain changed after it landed");
    assert!(differ * 4 < hot, "most of the heat is the same: {differ} of {hot} differ");
    hot
}

/// How far from a pixel dug after heat landed [`heat_is_the_same_drawn_every_tick_or_once`]
/// allows a difference, world pixels.
const R: usize = 24;
