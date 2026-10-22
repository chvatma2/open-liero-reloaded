//! How loud a sound is, and where, as OLX's OpenAL setup makes it
//! (presentation note §8.3). Pure maths, no device.

use olr_math::Vec2;

/// `AL_GAIN` of every positional source.
/// OLX: src/sound/sound_sample_openal.cpp:206-221 (`initSound`).
pub const SOURCE_GAIN: f32 = 0.7;

/// `AL_GAIN` of a global source (THROW, PICKUP).
/// OLX: src/sound/sound_sample_openal.cpp:237-255 (`play`, `volume` 1.0 from
/// `Sfx::playSimpleGlobal`, src/sound/sfx.cpp:120-126).
pub const GLOBAL_GAIN: f32 = 1.0;

/// `AL_REFERENCE_DISTANCE`: `loudness / 100 * 50` with loudness 100.
/// OLX: src/sound/sound_sample_openal.cpp:258-270, src/sound/sfx.cpp:128-134.
pub const REFERENCE_DISTANCE: f32 = 50.0;

/// `AL_ROLLOFF_FACTOR`. OLX: src/sound/sound_sample_openal.cpp:212.
pub const ROLLOFF: f32 = 2.0;

/// `SFX_LISTENER_DISTANCE`: the listener sits this far out of the world
/// plane, at the view centre. OLX: src/sound/sfxdriver.h:16,
/// src/sound/sfxdriver_openal.cpp:55-63.
pub const LISTENER_DISTANCE: f32 = 20.0;

/// The listener gain, `Audio.Volume / 100` with OLX's default volume 70.
/// OLX: src/client/Options.cpp:209, src/sound/sfxdriver.cpp:63-67,
/// src/sound/sfxdriver_openal.cpp:88-93.
pub const DEFAULT_VOLUME: f32 = 0.7;

/// What a voice plays at: an amplitude factor and a stereo pan (-1 left to
/// 1 right).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mix {
    pub amplitude: f32,
    pub pan: f32,
}

/// OpenAL's default distance model, `AL_INVERSE_DISTANCE_CLAMPED` (OLX sets
/// none): the attenuation of a source at `source` (world px, in the world
/// plane) heard by a listener [`LISTENER_DISTANCE`] out at `listener`.
/// 1 within the reference distance, then `ref / (ref + rolloff (d - ref))`.
pub fn attenuation(listener: Vec2, source: Vec2) -> f32 {
    let d = distance(listener, source).max(REFERENCE_DISTANCE);
    REFERENCE_DISTANCE / (REFERENCE_DISTANCE + ROLLOFF * (d - REFERENCE_DISTANCE))
}

fn distance(listener: Vec2, source: Vec2) -> f32 {
    let (dx, dy) = (source.x - listener.x, source.y - listener.y);
    (dx * dx + dy * dy + LISTENER_DISTANCE * LISTENER_DISTANCE).sqrt()
}

/// The mix of a source with gain `gain` ([`SOURCE_GAIN`] or
/// [`GLOBAL_GAIN`]) at `source`, for a listener at the view centre
/// `listener` and the master `volume` (0 to 1).
///
/// The amplitude is OpenAL's `listener gain × source gain × attenuation`,
/// clamped to 1 (`AL_MAX_GAIN`). The pan is the source direction's
/// left-right component, `dx / d` (OLX's listener faces into the screen
/// with world +x to the right, `src/sound/sfxdriver_openal.cpp:43-44`); OpenAL Soft's
/// own stereo pan law differs in detail, and kira applies a constant-power
/// law to this value.
pub fn mix(listener: Vec2, source: Vec2, gain: f32, volume: f32) -> Mix {
    let amplitude = (volume.clamp(0.0, 1.0) * gain * attenuation(listener, source)).clamp(0.0, 1.0);
    let pan = ((source.x - listener.x) / distance(listener, source)).clamp(-1.0, 1.0);
    let amplitude = if amplitude.is_finite() { amplitude } else { 0.0 };
    let pan = if pan.is_finite() { pan } else { 0.0 };
    Mix { amplitude, pan }
}

/// The listener of `listeners` nearest to `source` (the first of equally
/// near ones; `Vec2::ZERO` for none): split screen hears each sound from
/// the nearest local player's view (a deliberate deviation: OLX's OpenAL
/// uses viewport 0's listener only, `src/sound/sfxdriver_openal.cpp:55-65`;
/// `docs/porting/STATUS.md`), so no player is deaf to their own fight.
pub fn nearest(listeners: &[Vec2], source: Vec2) -> Vec2 {
    let mut best = (f32::INFINITY, listeners.first().copied().unwrap_or(Vec2::ZERO));
    for &l in listeners {
        let d = distance(l, source);
        if d < best.0 {
            best = (d, l);
        }
    }
    best.1
}

/// [`mix`] heard from the [`nearest`] of `listeners`: with one listener,
/// exactly `mix`.
pub fn mix_nearest(listeners: &[Vec2], source: Vec2, gain: f32, volume: f32) -> Mix {
    mix(nearest(listeners, source), source, gain, volume)
}

/// An amplitude factor in decibels, as kira takes volumes; kira treats
/// [`SILENCE_DB`] and below as silent.
pub fn decibels(amplitude: f32) -> f32 {
    if amplitude <= 0.001 { SILENCE_DB } else { (20.0 * amplitude.log10()).max(SILENCE_DB) }
}

/// kira's silence (`Decibels::SILENCE`).
pub const SILENCE_DB: f32 = -60.0;
