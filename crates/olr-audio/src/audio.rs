//! The output device (kira), the decoded sounds, and playing a tick's
//! `Sound` events and its live copies' sounds (issue #92).

use std::fmt::Debug;
use std::io::Cursor;
use std::panic::AssertUnwindSafe;
use std::time::Duration;

use kira::backend::Backend;
use kira::sound::PlaybackState;
use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle};
use kira::track::MainTrackBuilder;
use kira::{AudioManager, AudioManagerSettings, Decibels, DefaultBackend, Panning, Tween};
use olr_content::{CorePack, LiveSounds, LoadedLevel, LoadedMod};
use olr_math::Vec2;
use olr_sim::{Event, EventKind, GAME_SOUND_PREFIX, SoundId, TickEvents};

use crate::mix::{DEFAULT_VOLUME, GLOBAL_GAIN, Mix, SOURCE_GAIN, decibels, mix, mix_nearest, nearest};
use crate::voices::{Admit, GLOBAL_VOICES, MAX_LOOPS, Voice, Voices};

/// How long a voice a death sound takes the place of fades out; so too a
/// loop that stops and one that starts (issue #92: no click).
const STEAL_FADE: Duration = Duration::from_millis(5);

/// The short fade of [`STEAL_FADE`].
fn fade() -> Tween {
    Tween { duration: STEAL_FADE, ..Tween::default() }
}

/// An asset sound looping for a live copy (issue #92), as
/// [`Audio::loops`] reports it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayingLoop {
    /// The copy, an index into the level's live copies.
    pub copy: u16,
    /// The sound, an index into the level's sounds.
    pub sound: u16,
    /// Where it is heard from (world px): its copy's centre at the last
    /// [`Audio::play_live`].
    pub source: Vec2,
    /// What it plays at now.
    pub mix: Mix,
}

struct LoopVoice {
    info: PlayingLoop,
    handle: StaticSoundHandle,
}

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

/// Re-mixes every loop from its source for `listeners` and `volume`.
fn follow_loops(loops: &mut [LoopVoice], listeners: &[Vec2], volume: f32) {
    for v in loops {
        let m = mix_nearest(listeners, v.info.source, SOURCE_GAIN, volume);
        if m != v.info.mix {
            v.info.mix = m;
            v.handle.set_mix(m);
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
    /// The level's live copies' sounds (issue #92), by their index in the
    /// level's sounds; `None` plays nothing.
    level_sounds: Vec<Option<StaticSoundData>>,
    /// The asset loops playing (at most [`MAX_LOOPS`]).
    loops: Vec<LoopVoice>,
    /// The loops chosen this tick: distance and index (reused).
    chosen: Vec<(f32, usize)>,
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
            level_sounds: Vec::new(),
            loops: Vec::with_capacity(MAX_LOOPS),
            chosen: Vec::new(),
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
        follow_loops(&mut self.loops, &self.listeners, self.volume);
    }

    /// How many voices play (for the debug overlay), the asset loops
    /// included.
    pub fn playing(&self) -> usize {
        self.voices.len() + self.loops.len()
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

    /// Decodes `level`'s live copies' sounds (issue #92, `live/<k>.ogg`
    /// or `.wav`, checked against `olr_pack::sounds`' limits when the
    /// level loaded), indexed like the level's sounds, apart from the
    /// mod's: [`Audio::load_level_sound_files`] of them.
    pub fn load_level_sounds(&mut self, level: &LoadedLevel) {
        let Some(live) = &level.live else {
            self.load_level_sound_files(std::iter::empty());
            return;
        };
        let names = &live.source.file.sounds;
        self.load_level_sound_files(
            live.source.sounds.iter().enumerate().map(|(k, b)| (names.get(k).map_or("", String::as_str), &b[..])),
        );
    }

    /// The level sounds `files` (name for the warning, bytes), by index:
    /// stops every voice and loop and drops the last level's sounds. One
    /// that does not decode plays nothing and is a warning, as a mod
    /// sound. Silent audio decodes nothing. Decoding happens here, once
    /// per level load, never per tick.
    pub fn load_level_sound_files<'a>(&mut self, files: impl IntoIterator<Item = (&'a str, &'a [u8])>) {
        self.stop_all();
        self.level_sounds.clear();
        for (name, bytes) in files {
            let data = match &self.manager {
                Some(_) => match decode(bytes) {
                    Ok(d) => Some(d),
                    Err(e) => {
                        self.warnings.push(format!("level sound {name} does not decode, it plays nothing: {e}"));
                        None
                    }
                },
                None => None,
            };
            self.level_sounds.push(data);
        }
    }

    /// Whether level sound `k` was loaded and decoded.
    pub fn has_level_sound(&self, k: u16) -> bool {
        matches!(self.level_sounds.get(usize::from(k)), Some(Some(_)))
    }

    /// The asset loops playing (issue #92), in the order they started.
    pub fn loops(&self) -> impl Iterator<Item = &PlayingLoop> {
        self.loops.iter().map(|l| &l.info)
    }

    /// Each asset one-shot playing: its level sound and where it plays.
    pub fn asset_one_shots(&self) -> impl Iterator<Item = (u16, Vec2)> + '_ {
        self.voices.asset_sources()
    }

    /// Plays a presented tick's live copies' sounds (issue #92) for the
    /// listeners `listeners` (each local view's centre; none keep the last
    /// ones): call it once a tick, after [`Audio::play_heard`].
    ///
    /// - The loops follow `live.loops`: the [`MAX_LOOPS`] nearest to a
    ///   listener play (ties in their order), each re-mixed from its
    ///   copy's place now and heard from the nearest listener
    ///   ([`mix_nearest`], [`SOURCE_GAIN`], the master volume); one no
    ///   longer wanted (its state changed, its copy hidden, farther than
    ///   the others) stops with a 5-ms fade, a new one starts with one.
    ///   A loop keyed by copy and sound plays on while its copy's next
    ///   state loops the same sound.
    /// - The starts play once each where they are, like a game sound: at
    ///   most [`crate::ASSET_MAX_PLAYS`] of one sound at once, a start
    ///   beyond it skipped.
    /// - Loops and one-shots count against [`GLOBAL_VOICES`] with the
    ///   game's voices; a start or a loop past it waits (a loop starts at
    ///   a later tick when there is room).
    ///
    /// Silent audio plays nothing.
    pub fn play_live(&mut self, live: &LiveSounds, listeners: &[Vec2]) {
        if self.manager.is_none() {
            return;
        }
        if !listeners.is_empty() {
            self.listeners.clear();
            self.listeners.extend_from_slice(listeners);
        }
        self.voices.prune();
        // The loops wanted: the nearest to a listener.
        self.chosen.clear();
        for (i, l) in live.loops.iter().enumerate() {
            if self.has_level_sound(l.sound) {
                let near = nearest(&self.listeners, l.pos);
                let (dx, dy) = (l.pos.x - near.x, l.pos.y - near.y);
                self.chosen.push((dx * dx + dy * dy, i));
            }
        }
        self.chosen.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        self.chosen.truncate(MAX_LOOPS);
        // Those playing that are not wanted fade out; the others move.
        let (chosen, wanted) = (&self.chosen, &live.loops);
        self.loops.retain_mut(|v| {
            let found = chosen
                .iter()
                .map(|&(_, i)| &wanted[i])
                .find(|l| l.copy == v.info.copy && l.sound == v.info.sound && v.handle.playing());
            match found {
                Some(l) => {
                    v.info.source = l.pos;
                    true
                }
                None => {
                    v.handle.stop(fade());
                    false
                }
            }
        });
        self.voices.set_reserved(self.loops.len());
        // The wanted ones not playing start, while there is room.
        let Some(manager) = &mut self.manager else { return };
        for &(_, i) in &self.chosen {
            let l = wanted[i];
            if self.loops.iter().any(|v| v.info.copy == l.copy && v.info.sound == l.sound) {
                continue;
            }
            if !self.voices.has_room() {
                break;
            }
            let Some(Some(data)) = self.level_sounds.get(usize::from(l.sound)) else { continue };
            let m = mix_nearest(&self.listeners, l.pos, SOURCE_GAIN, self.volume);
            let data = data
                .loop_region(0.0..)
                .fade_in_tween(fade())
                .volume(Decibels(decibels(m.amplitude)))
                .panning(Panning(m.pan));
            let Ok(handle) = manager.play(data) else { continue };
            self.loops.push(LoopVoice {
                info: PlayingLoop { copy: l.copy, sound: l.sound, source: l.pos, mix: m },
                handle,
            });
            self.voices.set_reserved(self.loops.len());
        }
        self.voices.follow_nearest(&self.listeners, self.volume);
        follow_loops(&mut self.loops, &self.listeners, self.volume);
        // The one-shots.
        for s in &live.starts {
            let Some(Some(data)) = self.level_sounds.get(usize::from(s.sound)) else { continue };
            if self.voices.admit_asset(s.sound) != Admit::Start {
                continue;
            }
            let m = mix_nearest(&self.listeners, s.pos, SOURCE_GAIN, self.volume);
            let data = data.volume(Decibels(decibels(m.amplitude))).panning(Panning(m.pan));
            let Ok(handle) = manager.play(data) else { continue };
            self.voices.push_asset(s.sound, s.pos, SOURCE_GAIN, handle);
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

    /// Stops the asset loops with the short fade (the game is held: the
    /// pause, the weapon selection); the world's start again at the next
    /// [`Audio::play_live`].
    pub fn stop_loops(&mut self) {
        for mut l in self.loops.drain(..) {
            l.handle.stop(fade());
        }
        self.voices.set_reserved(0);
    }

    /// Stops every voice, the asset loops too (a restart; the loops a
    /// world plays start again at the next [`Audio::play_live`]).
    pub fn stop_all(&mut self) {
        for v in self.voices.voices_mut() {
            v.stop(Tween::default());
        }
        self.voices.clear();
        self.stop_loops();
    }

    /// kira's manager, if a device is open (tests drive a mock backend
    /// through it).
    pub fn manager_mut(&mut self) -> Option<&mut AudioManager<B>> {
        self.manager.as_mut()
    }
}
