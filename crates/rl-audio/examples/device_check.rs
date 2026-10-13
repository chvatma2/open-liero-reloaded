//! Opens the default output device and plays OLX's rope throw and a bump
//! to the right, then reports what kira said. A manual check of the real
//! device (spec §19 E12); exits 0 when the sounds played, 3 when no device
//! opened (the game would run silently), 1 on any other problem.
//!
//! `RL_CONTENT_DIR=content/packs cargo run -p rl-audio --example device_check`

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use rl_audio::Audio;
use rl_content::{Content, olx_mod_id};
use rl_math::Vec2;
use rl_sim::{Event, EventKind, SoundId};

fn main() -> ExitCode {
    let dir =
        std::env::var_os("RL_CONTENT_DIR").map_or_else(|| PathBuf::from("content/packs"), PathBuf::from);
    let content = match Content::open(&dir) {
        Ok(c) => c,
        Err(e) => {
            println!("no content in {}: {e:#}", dir.display());
            return ExitCode::from(1);
        }
    };
    let m = match content.load_mod(&olx_mod_id("Classic")) {
        Ok(m) => m,
        Err(e) => {
            println!("cannot load the classic mod: {e:#}");
            return ExitCode::from(1);
        }
    };
    let t = Instant::now();
    let mut audio = Audio::new();
    println!("device opened: {} ({:.0} ms)", !audio.is_silent(), t.elapsed().as_secs_f32() * 1000.0);
    for w in audio.take_warnings() {
        println!("warning: {w}");
    }
    if audio.is_silent() {
        return ExitCode::from(3);
    }
    audio.load(&m, content.core());
    let warnings = audio.take_warnings();
    for w in &warnings {
        println!("warning: {w}");
    }
    println!("decoded: throw {}, bump {}", audio.has_sound(SoundId::THROW), audio.has_sound(SoundId::BUMP));
    // The device's callback runs each voice to its end: a voice stops after
    // about the sound's length only if the device really consumes audio.
    let mut ok = true;
    for (name, id, pos) in [
        ("throw (global)", SoundId::THROW, Vec2::ZERO),
        ("bump (60 px right)", SoundId::BUMP, Vec2::new(60.0, 0.0)),
    ] {
        let t = Instant::now();
        audio.play(&[Event { substep: 0, death: false, kind: EventKind::Sound { id, pos } }], Vec2::ZERO);
        let started = audio.playing();
        while audio.playing() > 0 && t.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(5));
            audio.play(&[], Vec2::ZERO);
        }
        println!(
            "{name}: voices started {started}, stopped after {:.0} ms",
            t.elapsed().as_secs_f32() * 1000.0
        );
        ok &= started == 1 && audio.playing() == 0;
    }
    if warnings.is_empty() && ok { ExitCode::SUCCESS } else { ExitCode::from(1) }
}
