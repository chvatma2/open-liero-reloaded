//! The output device (kira), the decoded sounds, and playing a tick's
//! `Sound` events.

use std::fmt::Debug;
use std::io::Cursor;
use std::panic::AssertUnwindSafe;
use std::time::Duration;

use kira::backend::Backend;
use kira::sound::PlaybackState;
use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle};
use kira::track::MainTrackBuilder;
use kira::{AudioManager, AudioManagerSettings, Decibels, DefaultBackend, Panning, Tween};
use olr_content::{CorePack, LoadedMod};
use olr_math::Vec2;
use olr_sim::{Event, EventKind, GAME_SOUND_PREFIX, SoundId, TickEvents};

use crate::mix::{DEFAULT_VOLUME, GLOBAL_GAIN, Mix, SOURCE_GAIN, decibels, mix, mix_nearest};
use crate::voices::{Admit, GLOBAL_VOICES, Voice, Voices};

/// How long a voice a death sound takes the place of fades out.
const STEAL_FADE: Duration = Duration::from_millis(5);

/// kira's room for sounds on the main track: the voice limit plus the
/// stopped and stopping sounds kira has not removed yet.
const SOUND_CAPACITY: usize = GLOBAL_VOICES * 2;

impl Voice for StaticSoundHandle {
    fn playing(&self) -> bool {
        self.state() != PlaybackState::Stopped
    }

    fn set_mix(&mut self, m: Mix) {
        self.set_volume(decibels(m.amplitude), Tween::default());
        self.set_panning(m.pan, Tween::default());
    }
}

/// Decodes a sound file. The bytes come from a pack (untrusted): a decoder
/// panic becomes an error, never a crash (never panic on content); the
/// panic is the log's error (the game's run log has its record too).
fn decode(bytes: &[u8]) -> Result<StaticSoundData, String> {
    let cursor = Cursor::new(bytes.to_vec());
    match std::panic::catch_unwind(AssertUnwindSafe(|| StaticSoundData::from_cursor(cursor))) {
        Ok(r) => r.map_err(|e| e.to_string()),
        Err(_) => {
            log::error!("a sound's decoder panicked ({} bytes): the sound is left out", bytes.len());
            Err("the decoder panicked".into())
        }
    }
}

/// Whether sound `id` plays at the listener, not at its position: OLX plays
/// the rope throw and the bonus pickup with `PlaySoundSample`, every other
/// sound with `StartSound` (spec §19 E1).
/// OLX: src/common/CWormHuman.cpp:572-574, src/client/CClient_Game.cpp:142,
/// src/sound/sound_sample_openal.cpp:237-255 (`play`: at the listener).
pub fn is_global(id: SoundId) -> bool {
    id == SoundId::THROW || id == SoundId::PICKUP
}

/// The game's sound output (spec §8.4).
///
/// [`Audio::new`] opens the default device; when none opens the game runs
/// silently and [`Audio::take_warnings`] says why, once (spec §19 E12).
/// Generic over kira's backend so tests can use a mock or failing device.
pub struct Audio<B: Backend = DefaultBackend> {
    manager: Option<AudioManager<B>>,
    /// Decoded sounds by [`SoundId`]; `None` plays nothing.
    sounds: Vec<Option<StaticSoundData>>,
    voices: Voices<StaticSoundHandle>,
    volume: f32,
    /// Where the listeners were last (each view's centre at the last tick;
    /// never empty, the first the global sounds' and the preview's).
    listeners: Vec<Vec2>,
    /// The mod editor's sound preview playing (issue #19), apart from
    /// the game's voices.
    preview: Option<StaticSoundHandle>,
    warnings: Vec<String>,
}

impl Audio<DefaultBackend> {
    /// Opens the default output device, or runs silently.
    pub fn new() -> Audio {
        Audio::with_backend(Default::default())
    }
}

impl Default for Audio<DefaultBackend> {
    fn default() -> Self {
        Audio::new()
    }
}

impl<B: Backend> Audio<B>
where
    B::Error: Debug,
{
    /// Opens backend `B` with `settings`, or runs silently when it fails.
    pub fn with_backend(settings: B::Settings) -> Audio<B> {
        let manager_settings = AudioManagerSettings::<B> {
            main_track_builder: MainTrackBuilder::new().sound_capacity(SOUND_CAPACITY),
            backend_settings: settings,
            capacities: Default::default(),
            internal_buffer_size: 128,
        };
        let mut audio = Audio::silent();
        match AudioManager::<B>::new(manager_settings) {
            Ok(m) => audio.manager = Some(m),
            Err(e) => audio.warnings.push(format!("no audio output, playing silently: {e:?}")),
        }
        audio
    }
}

impl<B: Backend> Audio<B> {
    /// No device: everything is accepted and nothing plays.
    pub fn silent() -> Audio<B> {
        Audio {
            manager: None,
            sounds: Vec::new(),
            voices: Voices::new(GLOBAL_VOICES),
            volume: DEFAULT_VOLUME,
            listeners: vec![Vec2::ZERO],
            preview: None,
            warnings: Vec::new(),
        }
    }

    /// Whether no device is open.
    pub fn is_silent(&self) -> bool {
        self.manager.is_none()
    }

    /// The problems since the last call (no device, undecodable sounds),
    /// for the caller to log once.
    pub fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
    }

    /// The master volume, 0 to 1 (OLX's `Audio.Volume / 100`, default 0.7).
    pub fn volume(&self) -> f32 {
        self.volume
    }

    /// Sets the master volume (0 to 1; not a number: the default). The
    /// voices playing follow at once, as OLX's `SetSoundVolume` sets the
    /// listener's gain (the options apply it live, even while no tick runs).
    /// OLX: src/sound/Sounds.cpp:164-174, src/sound/sfxdriver_openal.cpp:88-93.
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = if volume.is_finite() { volume.clamp(0.0, 1.0) } else { DEFAULT_VOLUME };
        self.voices.follow_nearest(&self.listeners, self.volume);
    }

    /// How many voices play (for the debug overlay).
    pub fn playing(&self) -> usize {
        self.voices.len()
    }

    /// Decodes mod `m`'s sounds (OGG, or WAV), indexed like its
    /// [`olr_sim::SimMod::sounds`]: the mod pack's bytes, or for an OLX game
    /// sound (`olx:` names) the core pack's. Stops every voice. A missing
    /// sound plays nothing (`olr-content` already warned); one that does not
    /// decode plays nothing and is a warning.
    pub fn load(&mut self, m: &LoadedMod, core: &CorePack) {
        let lookup = |name: &str| -> Option<&[u8]> {
            m.sounds
                .get(name)
                .map(Vec::as_slice)
                .or_else(|| if name.starts_with(GAME_SOUND_PREFIX) { core.game_sound(name) } else { None })
        };
        self.load_sounds(m.sim.sounds(), lookup);
    }

    /// [`Audio::load`] from sound `names` and a lookup of their bytes.
    /// Silent audio decodes nothing.
    pub fn load_sounds<'a>(&mut self, names: &[String], bytes: impl Fn(&str) -> Option<&'a [u8]>) {
        self.stop_all();
        self.sounds.clear();
        for name in names {
            let data = match (&self.manager, bytes(name)) {
                (Some(_), Some(b)) => match decode(b) {
                    Ok(d) => Some(d),
                    Err(e) => {
                        self.warnings.push(format!("sound {name} does not decode, it plays nothing: {e}"));
                        None
                    }
                },
                _ => None,
            };
            self.sounds.push(data);
        }
    }

    /// Whether sound `id` was loaded and decoded.
    pub fn has_sound(&self, id: SoundId) -> bool {
        matches!(self.sounds.get(usize::from(id.0)), Some(Some(_)))
    }

    /// Starts the tick's `Sound` events for a listener at the view centre
    /// `listener` (world px), and re-mixes the playing voices for it.
    ///
    /// - Positional sounds play at their position with OpenAL's gain 0.7 and
    ///   inverse-distance attenuation ([`crate::mix`]).
    /// - THROW and PICKUP play at the listener with gain 1.0 ([`is_global`]).
    /// - A start beyond the sound's or the global voice limit is skipped,
    ///   but a death sound's: it takes the oldest voice's place
    ///   ([`Voices::admit`]), which fades out over 5 ms.
    /// - Beam weapons fire silently: the simulation emits no `Sound` for
    ///   their shots (OLX: src/client/CClient_Game.cpp:900-906).
    pub fn play_events(&mut self, events: &TickEvents, listener: Vec2) {
        self.play(events.as_slice(), listener);
    }

    /// [`Audio::play_events`] for several listeners (split screen: each
    /// view's centre, `olr_present::Presentation::listeners`): each
    /// positional sound is heard once, from the listener nearest to it
    /// ([`crate::mix::nearest`]; a deliberate deviation from OLX, which
    /// hears viewport 0 only), and the playing voices are re-mixed the
    /// same way. THROW and PICKUP play at the first listener. With one
    /// listener it is [`Audio::play_events`]; with none the last ones
    /// stay.
    pub fn play_events_heard(&mut self, events: &TickEvents, listeners: &[Vec2]) {
        self.play_heard(events.as_slice(), listeners);
    }

    /// [`Audio::play_events`] for any events.
    pub fn play(&mut self, events: &[Event], listener: Vec2) {
        self.play_heard(events, &[listener]);
    }

    /// [`Audio::play_events_heard`] for any events.
    pub fn play_heard(&mut self, events: &[Event], listeners: &[Vec2]) {
        let Some(manager) = &mut self.manager else { return };
        if !listeners.is_empty() {
            self.listeners.clear();
            self.listeners.extend_from_slice(listeners);
        }
        let listeners = &self.listeners[..];
        self.voices.prune();
        self.voices.follow_nearest(listeners, self.volume);
        for e in events {
            let EventKind::Sound { id, pos } = e.kind else { continue };
            let Some(Some(data)) = self.sounds.get(usize::from(id.0)) else { continue };
            let admit = self.voices.admit(id);
            if admit == Admit::Skip {
                continue;
            }
            let (source, gain) = if is_global(id) { (listeners[0], GLOBAL_GAIN) } else { (pos, SOURCE_GAIN) };
            let m = mix_nearest(listeners, source, gain, self.volume);
            let data = data.volume(Decibels(decibels(m.amplitude))).panning(Panning(m.pan));
            // A start that fails takes nothing: a stolen voice plays on.
            let Ok(handle) = manager.play(data) else { continue };
            match admit {
                // A death sound takes another voice's place once it has
                // started: the old one stops with a short fade, no click.
                Admit::Steal(victim) => {
                    let mut old = self.voices.replace(victim, id, source, gain, handle);
                    old.stop(Tween { duration: STEAL_FADE, ..Tween::default() });
                }
                _ => self.voices.push(id, source, gain, handle),
            }
        }
    }

    /// Plays sound file `bytes` (OGG or WAV) once at the listener, as a
    /// global sound at the master volume: the mod editor's preview (issue
    /// #19). A preview still playing stops first; the game's voices and
    /// loaded sounds are left alone. Silent without a device; why it did
    /// not play when it does not decode.
    pub fn preview(&mut self, bytes: &[u8]) -> Result<(), String> {
        let Some(manager) = &mut self.manager else { return Ok(()) };
        if let Some(mut old) = self.preview.take() {
            old.stop(Tween { duration: STEAL_FADE, ..Tween::default() });
        }
        let data = decode(bytes)?;
        let listener = self.listeners[0];
        let m = mix(listener, listener, GLOBAL_GAIN, self.volume);
        let data = data.volume(Decibels(decibels(m.amplitude))).panning(Panning(m.pan));
        self.preview = Some(manager.play(data).map_err(|e| e.to_string())?);
        Ok(())
    }

    /// Whether a preview plays ([`Audio::preview`]).
    pub fn previewing(&self) -> bool {
        self.preview.as_ref().is_some_and(Voice::playing)
    }

    /// Stops every voice (a restart).
    pub fn stop_all(&mut self) {
        for v in self.voices.voices_mut() {
            v.stop(Tween::default());
        }
        self.voices.clear();
    }

    /// kira's manager, if a device is open (tests drive a mock backend
    /// through it).
    pub fn manager_mut(&mut self) -> Option<&mut AudioManager<B>> {
        self.manager.as_mut()
    }
}
