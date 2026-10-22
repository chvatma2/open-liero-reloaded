//! The long-session soak (plan 5 review focus 5): ten minutes of sandbox
//! (60 000 ticks at 100 Hz) headless, driven by scripted chaos keys, with
//! `olr-fx` attached and an offscreen render every 10th tick. Entity counts,
//! the projectile pool, audio voices (kira's mock device) and GPU buffers
//! must stay bounded, and nothing may panic.
//!
//! Runs in `cargo xtask full` (`OLR_GPU_TESTS`); without that, the imported
//! content (`OLR_CONTENT_DIR`) or a graphics adapter it skips with a notice.
//! Everything stays on the test's main thread: under WSL's d3d12 GL path a
//! thread that used the device crashes when it exits.

use std::path::Path;

use olr_audio::Audio;
use olr_audio::kira::backend::mock::{MockBackend, MockBackendSettings};
use olr_game::session::Session;
use olr_game::setup::{SandboxOptions, load};
use olr_render::{Offscreen, RenderSettings};
use olr_scene::{DrawList, Interpolation, Viewport};
use olr_sim::Keys;

const TICKS: u64 = 60_000;
const RENDER_EVERY: u64 = 10;
/// The simulation's projectile pool (`ProjectilePool::DEFAULT_CAPACITY`).
const POOL: usize = 3000;
/// Play's cosmetic-entity pool (`olr-fx`, spec §19 E1 amended 2026-10-13).
const FX_CAP: usize = olr_fx::PLAY_ENTITIES;
/// The simulation's emulated dedicated-server entity counter (OLX's 1024),
/// which only gates the gibs' draws.
const SIM_ENTITY_CAP: usize = 1024;

/// A cheap deterministic generator for the chaos keys (no wall clock).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) as u32
    }
}

/// Holds a random key set for 1 to 40 ticks, with bias towards firing and
/// digging, and now and then cycles weapons and throws the rope.
struct Chaos {
    rng: Lcg,
    held: u16,
    left: u32,
}

impl Chaos {
    fn keys(&mut self) -> Keys {
        if self.left == 0 {
            let r = self.rng.next();
            self.left = 1 + (r >> 8) % 40;
            self.held = u16::from(r as u8) & (Keys::UP | Keys::DOWN | Keys::LEFT | Keys::RIGHT | Keys::JUMP);
            if r & 0x1_0000 != 0 {
                self.held |= Keys::FIRE;
            }
            if r & 0x6_0000 == 0 {
                self.held |= Keys::ROPE;
            }
            if r & 0x38_0000 == 0 {
                self.held |= Keys::CHANGE;
            }
        }
        self.left -= 1;
        Keys(self.held)
    }
}

#[test]
fn soak_sandbox_10_minutes_headless() {
    if std::env::var_os("OLR_GPU_TESTS").is_none() {
        eprintln!("OLR_GPU_TESTS not set (`cargo xtask full` sets it); skipping");
        return;
    }
    let Some(dir) = std::env::var_os("OLR_CONTENT_DIR").filter(|d| Path::new(d).is_dir()) else {
        eprintln!("OLR_CONTENT_DIR not set or missing (run `cargo xtask fetch-olx-content`); skipping");
        return;
    };
    let gpu = match Offscreen::new() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("{e}; skipping");
            return;
        }
    };
    let options =
        SandboxOptions { mod_id: String::new(), level_id: String::new(), seed: 7, targets: 3, bonuses: true };
    let a = load(&[Path::new(&dir)], "MW 1.0", "Liero Factory", "default", options).unwrap();
    let viewport = Viewport::from_aspect(16.0 / 9.0, 16.0 / 9.0);
    let mut s = Session::new(&a, viewport, Interpolation::Linear, Audio::silent()).unwrap();
    // The session's own audio is silent; sound is driven on a mock device.
    let mut audio = Audio::<MockBackend>::with_backend(MockBackendSettings { sample_rate: 8000 });
    audio.load(&a.m, a.content.core());
    assert!(!audio.is_silent());
    audio.take_warnings();

    let mut renderer = gpu.renderer(RenderSettings::default());
    s.present.upload(&mut renderer);
    let atlas = renderer.atlas_layout().map(|l| (l.width, l.height));
    let terrain = renderer.terrain_texture().map(|t| (t.width(), t.height()));
    let first_buffer = renderer.instance_buffer_bytes();

    let mut chaos = Chaos { rng: Lcg(1), held: 0, left: 0 };
    let mut list = DrawList::new();
    let (mut max_fx, mut max_proj, mut max_entities, mut max_voices, mut max_popups) = (0, 0, 0, 0, 0);
    // The instance buffer may grow to a frame's next power of two; it must
    // stop growing once the busiest frames have been seen.
    let (mut buffer_first_half, mut buffer_second_half) = (first_buffer, first_buffer);
    let mut frames = 0u32;
    let started = std::time::Instant::now();

    for t in 0..TICKS {
        if t == TICKS / 2 {
            // F5 in the middle: a restart must not leak either.
            s.restart(&a).unwrap();
            audio.stop_all();
        }
        s.tick(chaos.keys());
        audio.play_events_heard(&s.events, &s.present.listeners());
        let manager = audio.manager_mut().unwrap();
        manager.backend_mut().on_start_processing();
        for _ in 0..80 {
            manager.backend_mut().process();
        }
        max_fx = max_fx.max(s.present.fx.len());
        max_proj = max_proj.max(s.world.projectiles().count());
        max_entities = max_entities.max(s.world.entity_count());
        max_voices = max_voices.max(audio.playing());
        max_popups = max_popups.max(s.popups.len());
        if t % RENDER_EVERY == 0 {
            s.build(0.5, &mut list);
            s.present.upload(&mut renderer);
            let img = gpu.render_to_image(&mut renderer, &list, (640, 360));
            assert_eq!((img.width(), img.height()), (640, 360));
            frames += 1;
            let bytes = renderer.instance_buffer_bytes();
            if t < TICKS / 2 {
                buffer_first_half = buffer_first_half.max(bytes);
            } else {
                buffer_second_half = buffer_second_half.max(bytes);
            }
        }
    }

    eprintln!(
        "soak: {TICKS} ticks, {frames} frames in {:.1}s on {} ({:?}); peaks: fx {max_fx}, projectiles \
         {max_proj}, entities {max_entities}, voices {max_voices}, popups {max_popups}, instance buffer \
         {buffer_first_half} B then {buffer_second_half} B; player deaths {}",
        started.elapsed().as_secs_f32(),
        gpu.adapter_info().name,
        gpu.adapter_info().backend,
        s.player_deaths,
    );
    assert!(max_fx > 0 && max_voices > 0 && max_proj > 0, "the chaos exercised effects, sounds and shots");
    assert!(max_fx <= FX_CAP, "cosmetic entities are capped by play's pool");
    assert!(max_proj <= POOL);
    assert!(max_entities as usize <= SIM_ENTITY_CAP, "the sim's entity count: {max_entities}");
    assert!(max_voices <= olr_audio::GLOBAL_VOICES, "audio voices: {max_voices}");
    assert!(max_popups <= 64, "popups: {max_popups}");
    assert_eq!(atlas, renderer.atlas_layout().map(|l| (l.width, l.height)), "the atlas is uploaded once");
    assert_eq!(terrain, renderer.terrain_texture().map(|t| (t.width(), t.height())));
    // The buffer grows to the busiest frame's next power of two and no
    // further: every cosmetic entity, every projectile and some scenery.
    let cap = 48 * (FX_CAP + POOL + 256).next_power_of_two() as u64;
    assert!(buffer_second_half <= cap, "instance buffer: {buffer_second_half} B (cap {cap} B)");
    assert!(
        buffer_second_half <= 2 * buffer_first_half,
        "the instance buffer kept growing: {buffer_first_half} B then {buffer_second_half} B"
    );
    assert!(renderer.warnings().is_empty(), "{:?}", renderer.warnings());
}
