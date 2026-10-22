//! `olr-audio`: OLX's OpenAL mix, the voice limits, silence without a
//! device, playing on kira's mock backend, and decoding every imported
//! sound (`OLR_CONTENT_DIR`, skipped with a notice when it is missing).

use std::path::PathBuf;

use olr_audio::kira::backend::Backend;
use olr_audio::kira::backend::Renderer;
use olr_audio::kira::backend::mock::{MockBackend, MockBackendSettings};
use olr_audio::{
    Admit, Audio, DEFAULT_VOLUME, GLOBAL_GAIN, GLOBAL_VOICES, Mix, SOURCE_GAIN, Voice, Voices, attenuation,
    decibels, is_global, max_plays, mix,
};
use olr_content::Content;
use olr_data::manifest::PackKind;
use olr_math::Vec2;
use olr_sim::{Event, EventKind, GAME_SOUNDS, SoundId, WeaponType};

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

#[test]
fn positional_gain_matches_olx_openal_defaults() {
    let l = Vec2::new(500.0, 300.0);
    let at = |dx: f32, dy: f32| Vec2::new(l.x + dx, l.y + dy);
    // Inverse distance, clamped: d = sqrt(dx² + dy² + 20²), 1 within 50 px,
    // then 50 / (50 + 2 (d - 50)).
    assert_eq!(attenuation(l, l), 1.0);
    assert_eq!(attenuation(l, at(45.0, 0.0)), 1.0);
    let d = (160.0f32 * 160.0 + 400.0).sqrt();
    assert!(close(attenuation(l, at(160.0, 0.0)), 50.0 / (50.0 + 2.0 * (d - 50.0))));
    assert!(close(attenuation(l, at(160.0, 0.0)), 0.18349)); // the 320-px view's side edge
    assert!(close(attenuation(l, at(0.0, -160.0)), 0.18349));
    let far = attenuation(l, at(3000.0, 0.0));
    assert!(far > 0.0 && far < 0.01, "no culling, just quiet: {far}");

    // Positional: listener gain (volume) × 0.7 × attenuation; pan dx / d.
    assert_eq!(SOURCE_GAIN, 0.7);
    assert_eq!(DEFAULT_VOLUME, 0.7);
    let m = mix(l, l, SOURCE_GAIN, DEFAULT_VOLUME);
    assert!(close(m.amplitude, 0.49) && m.pan == 0.0, "{m:?}");
    let m = mix(l, at(160.0, 0.0), SOURCE_GAIN, 1.0);
    assert!(close(m.amplitude, 0.7 * 0.18349), "{m:?}");
    assert!(close(m.pan, 160.0 / d), "{m:?}");
    assert!(close(mix(l, at(-160.0, 0.0), SOURCE_GAIN, 1.0).pan, -160.0 / d));
    assert_eq!(mix(l, at(0.0, 200.0), SOURCE_GAIN, 1.0).pan, 0.0);
    assert!(close(mix(l, at(20.0, 0.0), SOURCE_GAIN, 1.0).pan, 20.0 / 800.0f32.sqrt()));

    // Global (THROW, PICKUP): at the listener, gain 1.0, centred.
    assert!(is_global(SoundId::THROW) && is_global(SoundId::PICKUP));
    assert!(!is_global(SoundId::BUMP) && !SoundId::DEATH.iter().any(|d| is_global(*d)));
    assert!(!is_global(SoundId(GAME_SOUNDS.len() as u16)));
    assert_eq!(mix(l, l, GLOBAL_GAIN, DEFAULT_VOLUME), Mix { amplitude: 0.7, pan: 0.0 });
    // The amplitude never exceeds AL_MAX_GAIN, and the volume is clamped.
    assert_eq!(mix(l, l, GLOBAL_GAIN, 3.0).amplitude, 1.0);
    assert_eq!(mix(l, l, GLOBAL_GAIN, -1.0).amplitude, 0.0);

    // kira volumes are decibels; nothing is -60 dB (kira's silence).
    assert_eq!(decibels(1.0), 0.0);
    assert!((decibels(0.5) + 6.0206).abs() < 1e-3);
    assert_eq!(decibels(0.0), -60.0);
    assert_eq!(decibels(f32::NAN), -60.0);
}

/// A voice that plays until told it stopped, and remembers its last mix.
struct Fake {
    playing: bool,
    mix: Option<Mix>,
    /// Which voice it is, for the steal tests.
    tag: u32,
}

impl Voice for Fake {
    fn playing(&self) -> bool {
        self.playing
    }

    fn set_mix(&mut self, mix: Mix) {
        self.mix = Some(mix);
    }
}

fn fake() -> Fake {
    tagged(0)
}

fn tagged(tag: u32) -> Fake {
    Fake { playing: true, mix: None, tag }
}

/// Admits sound `id` into `v` and starts it as voice `tag`: `Ok(None)`
/// started, `Ok(Some(tag))` started in that voice's place, `Err(())`
/// skipped.
fn start(v: &mut Voices<Fake>, id: SoundId, tag: u32) -> Result<Option<u32>, ()> {
    match v.admit(id) {
        Admit::Start => {
            v.push(id, Vec2::ZERO, SOURCE_GAIN, tagged(tag));
            Ok(None)
        }
        Admit::Steal(victim) => Ok(Some(v.replace(victim, id, Vec2::ZERO, SOURCE_GAIN, tagged(tag)).tag)),
        Admit::Skip => Err(()),
    }
}

#[test]
fn voice_limits() {
    // OLX's maxplaying: throw 4, pickup, bump and deaths 2, mod sounds 10.
    assert_eq!(max_plays(SoundId::THROW), 4);
    assert_eq!(max_plays(SoundId::PICKUP), 2);
    assert_eq!(max_plays(SoundId::BUMP), 2);
    assert!(SoundId::DEATH.iter().all(|d| max_plays(*d) == 2));
    assert_eq!(max_plays(SoundId(6)), 10);
    assert_eq!(max_plays(SoundId(u16::MAX)), 10);

    let p = Vec2::ZERO;
    let mut v = Voices::new(GLOBAL_VOICES);
    let fill = |v: &mut Voices<Fake>, id: SoundId| {
        let mut n = 0;
        while matches!(v.admit(id), Admit::Start) {
            v.push(id, p, SOURCE_GAIN, fake());
            n += 1;
        }
        n
    };
    assert_eq!(fill(&mut v, SoundId::THROW), 4);
    assert_eq!(fill(&mut v, SoundId::PICKUP), 2);
    assert_eq!(fill(&mut v, SoundId(7)), 10);
    assert_eq!(v.len(), 16);
    // Each death sample has its own limit (what a third does: below).
    for d in [SoundId::DEATH[0], SoundId::DEATH[1]] {
        for _ in 0..2 {
            assert!(matches!(v.admit(d), Admit::Start));
            v.push(d, p, SOURCE_GAIN, fake());
        }
    }
    assert_eq!(v.len(), 20);

    // A stopped voice no longer counts once pruned.
    v.voices_mut().next().expect("a voice").playing = false;
    assert!(matches!(v.admit(SoundId::THROW), Admit::Skip));
    v.prune();
    assert_eq!((v.playing(SoundId::THROW), v.len()), (3, 19));
    assert!(matches!(v.admit(SoundId::THROW), Admit::Start));

    // The global limit: distinct mod sounds stop at it.
    let mut v = Voices::new(5);
    for i in 0..5 {
        assert!(matches!(v.admit(SoundId(10 + i)), Admit::Start));
        v.push(SoundId(10 + i), p, SOURCE_GAIN, fake());
    }
    assert!(matches!(v.admit(SoundId(20)), Admit::Skip));

    // Sources stay put while the listener follows the view.
    let mut v = Voices::new(GLOBAL_VOICES);
    v.push(SoundId(9), Vec2::new(0.0, 0.0), SOURCE_GAIN, fake());
    v.follow(Vec2::new(160.0, 0.0), 1.0);
    let m = v.voices_mut().next().and_then(|f| f.mix).expect("re-mixed");
    assert_eq!(m, mix(Vec2::new(160.0, 0.0), Vec2::ZERO, SOURCE_GAIN, 1.0));
    assert!(m.pan < -0.9 && m.amplitude < 0.15, "{m:?}");
}

/// A death sound is never skipped (the user's ruling; OLX skips it): at its
/// own limit it takes the oldest voice of that sound; at the global limit
/// the oldest of that sound, else the oldest voice that is not a death's.
/// Oldest is push order. Every other sound is skipped as in OLX.
#[test]
fn a_death_sound_steals_and_others_are_skipped() {
    let p = Vec2::ZERO;
    let (d0, d1, d2) = (SoundId::DEATH[0], SoundId::DEATH[1], SoundId::DEATH[2]);
    let mut v = Voices::new(GLOBAL_VOICES);
    for tag in 0..2 {
        assert_eq!(start(&mut v, d0, tag), Ok(None));
    }
    // A steal changes nothing until the new voice has started: a start
    // that fails leaves the old voice playing, never silence.
    let Admit::Steal(victim) = v.admit(d0) else { panic!("a third death sound steals") };
    assert_eq!(v.victim(victim).tag, 0, "the oldest");
    assert_eq!((v.playing(d0), v.len()), (2, 2), "admitting a steal takes nothing yet");
    assert_eq!(start(&mut v, d0, 2), Ok(Some(0)), "a third takes the oldest's place");
    assert_eq!(v.playing(d0), 2, "the stolen voice no longer counts");
    assert_eq!(start(&mut v, d0, 3), Ok(Some(1)), "then the next oldest");
    for _ in 0..2 {
        v.push(SoundId::BUMP, p, SOURCE_GAIN, fake());
    }
    assert_eq!(start(&mut v, SoundId::BUMP, 9), Err(()), "any other sound at its limit is skipped");

    let mut v = Voices::new(4);
    for (id, tag) in [(SoundId(10), 10), (SoundId(11), 11), (d1, 20), (SoundId(12), 12)] {
        v.push(id, p, SOURCE_GAIN, tagged(tag));
    }
    assert_eq!(start(&mut v, SoundId(13), 13), Err(()), "a mod sound at the global limit is skipped");
    assert_eq!(start(&mut v, d1, 21), Ok(Some(20)), "at the global limit, the same sound's oldest");
    assert_eq!(start(&mut v, d2, 30), Ok(Some(10)), "else the oldest that is not a death's");
    assert_eq!(v.len(), 4);
    assert_eq!(start(&mut v, d0, 40), Ok(Some(11)));

    let mut v = Voices::new(2);
    v.push(d0, p, SOURCE_GAIN, tagged(0));
    v.push(d1, p, SOURCE_GAIN, tagged(1));
    assert_eq!(start(&mut v, d2, 2), Err(()), "only other deaths' voices: none is taken");
}

/// A mono 16-bit WAV of `n` samples at 8 kHz.
fn wav(n: u32) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + n * 2).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes()); // PCM
    b.extend_from_slice(&1u16.to_le_bytes()); // mono
    b.extend_from_slice(&8000u32.to_le_bytes());
    b.extend_from_slice(&16000u32.to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&(n * 2).to_le_bytes());
    for i in 0..n {
        let s = ((i as f32 * 0.2).sin() * 8000.0) as i16;
        b.extend_from_slice(&s.to_le_bytes());
    }
    b
}

fn names() -> Vec<String> {
    let mut n: Vec<String> = GAME_SOUNDS.iter().map(|s| s.to_string()).collect();
    n.extend(["sfx/gun.wav".to_string(), "sfx/broken.wav".to_string(), "sfx/missing.wav".to_string()]);
    n
}

fn sound(id: SoundId, x: f32) -> Event {
    Event { substep: 0, death: false, kind: EventKind::Sound { id, pos: Vec2::new(x, 0.0) } }
}

/// A backend whose device never opens.
struct NoDevice;

impl Backend for NoDevice {
    type Settings = ();
    type Error = &'static str;

    fn setup(_: (), _: usize) -> Result<(Self, u32), Self::Error> {
        Err("no such device")
    }

    fn start(&mut self, _: Renderer) -> Result<(), Self::Error> {
        Err("no such device")
    }
}

#[test]
fn silent_when_no_device() {
    let mut a = Audio::<NoDevice>::with_backend(());
    assert!(a.is_silent());
    let w = a.take_warnings();
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].contains("silently") && w[0].contains("no such device"), "{w:?}");
    assert!(a.take_warnings().is_empty(), "said once");

    // Loading and playing work and do nothing.
    let bytes = wav(800);
    a.load_sounds(&names(), |_| Some(&bytes[..]));
    assert!(!a.has_sound(SoundId::THROW));
    a.play(&[sound(SoundId::THROW, 0.0), sound(SoundId(6), 10.0)], Vec2::ZERO);
    a.play_events(&olr_sim::TickEvents::new(), Vec2::ZERO);
    assert_eq!(a.playing(), 0);
    a.stop_all();
    assert!(a.take_warnings().is_empty());
    assert!(Audio::<NoDevice>::silent().is_silent());
}

#[test]
fn plays_events_on_a_mock_device() {
    let mut a = Audio::<MockBackend>::with_backend(MockBackendSettings { sample_rate: 8000 });
    assert!(!a.is_silent() && a.take_warnings().is_empty());
    let good = wav(800);
    let broken = b"RIFF not a wav".to_vec();
    a.load_sounds(&names(), |n| match n {
        "sfx/broken.wav" => Some(&broken[..]),
        "sfx/missing.wav" => None,
        _ => Some(&good[..]),
    });
    let w = a.take_warnings();
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].contains("sfx/broken.wav"), "{w:?}");
    assert!(a.has_sound(SoundId::THROW) && a.has_sound(SoundId(6)));
    assert!(!a.has_sound(SoundId(7)) && !a.has_sound(SoundId(8)) && !a.has_sound(SoundId(99)));

    // 6 throws, 12 gun shots, a bump 3000 px away, the broken and the missing
    // sound and an id the mod lacks: 4 + 10 + 1 voices.
    let mut ev: Vec<Event> = (0..6).map(|_| sound(SoundId::THROW, 900.0)).collect();
    ev.extend((0..12).map(|i| sound(SoundId(6), i as f32)));
    ev.extend([sound(SoundId::BUMP, 3000.0), sound(SoundId(7), 0.0), sound(SoundId(8), 0.0)]);
    ev.push(sound(SoundId(99), 0.0));
    ev.push(Event { substep: 1, death: false, kind: EventKind::Kill { victim: 0, killer: 1 } });
    a.play(&ev, Vec2::ZERO);
    assert_eq!(a.playing(), 15);
    // The next tick re-mixes them; of the same events only a second bump
    // fits under the limits.
    a.play(&ev, Vec2::new(100.0, 0.0));
    assert_eq!(a.playing(), 16);
    // Once kira has run them to the end they no longer count.
    let manager = a.manager_mut().expect("mock device");
    for _ in 0..2000 {
        manager.backend_mut().on_start_processing();
        manager.backend_mut().process();
    }
    a.play(&[], Vec2::ZERO);
    assert_eq!(a.playing(), 0);
    a.play(&ev[..1], Vec2::ZERO);
    assert_eq!(a.playing(), 1);
    a.stop_all();
    assert_eq!(a.playing(), 0);
}

/// A death sound at its limit takes the place of the oldest voice of that
/// sound, which stops (with a short fade) for it: every death is heard.
#[test]
fn a_death_sound_at_its_limit_steals_the_oldest_on_a_mock_device() {
    let mut a = Audio::<MockBackend>::with_backend(MockBackendSettings { sample_rate: 8000 });
    // One second; the mock device renders about 1/63 s a call.
    let good = wav(8000);
    a.load_sounds(&names(), |n| (n != "sfx/broken.wav" && n != "sfx/missing.wav").then_some(&good[..]));
    let run = |a: &mut Audio<MockBackend>, frames| {
        let manager = a.manager_mut().expect("mock device");
        for _ in 0..frames {
            manager.backend_mut().on_start_processing();
            manager.backend_mut().process();
        }
    };
    let death = sound(SoundId::DEATH[0], 0.0);
    a.play(&[death, death], Vec2::ZERO);
    run(&mut a, 20);
    a.play(&[death], Vec2::ZERO);
    assert_eq!(a.playing(), 2, "at most 2 of a death sound, as OLX");
    // The two first ones would have ended by now; the oldest was stopped
    // and the third plays on.
    run(&mut a, 50);
    a.play(&[], Vec2::ZERO);
    assert_eq!(a.playing(), 1, "the third death sound plays");
    // Three in one tick: the third takes the first's place.
    a.stop_all();
    a.play(&[death, death, death], Vec2::ZERO);
    assert_eq!(a.playing(), 2);
    run(&mut a, 5);
    let sounds = a.manager_mut().expect("mock device").main_track().num_sounds();
    assert_eq!(sounds, 2, "the stolen voice faded out and is gone");
}

fn content_dir() -> Option<PathBuf> {
    let dir = std::env::var_os("OLR_CONTENT_DIR").map(PathBuf::from)?;
    dir.join("olx-core").is_dir().then_some(dir)
}

#[test]
fn decodes_every_mod_sound() {
    let Some(dir) = content_dir() else {
        eprintln!("skipping: OLR_CONTENT_DIR has no imported content (cargo xtask fetch-olx-content)");
        return;
    };
    let content = Content::open(&dir).expect("content");
    let mods = content.ids(PackKind::Mod);
    assert!(mods.len() >= 2, "{mods:?}");
    let mut audio = Audio::<MockBackend>::with_backend(MockBackendSettings { sample_rate: 44100 });
    for id in mods {
        let m = content.load_mod(&id).expect("mod");
        audio.load(&m, content.core());
        let w = audio.take_warnings();
        assert!(w.is_empty(), "{id}: {w:?}");
        for (i, name) in m.sim.sounds().iter().enumerate() {
            assert!(audio.has_sound(SoundId(i as u16)), "{id}: {name} not decoded");
        }
        // Beam weapons have no firing sound (OLX plays none for them).
        for w in m.sim.weapons().iter().filter(|w| w.kind == WeaponType::Beam) {
            assert_eq!(w.sound, None, "{id}: beam {}", w.id);
        }
    }
}

/// Issue #19: the mod editor's preview plays one sound file at a time,
/// apart from the game's voices and sounds; a file that does not decode
/// says so, and without a device nothing plays.
#[test]
fn a_preview_plays_a_sound_file_alone() {
    let mut a = Audio::<MockBackend>::with_backend(MockBackendSettings { sample_rate: 8000 });
    let good = wav(800);
    a.load_sounds(&names(), |_| Some(&good[..]));
    a.preview(&wav(400)).unwrap();
    assert!(a.previewing());
    assert_eq!(a.playing(), 0, "not one of the game's voices");
    a.preview(&wav(400)).unwrap();
    assert!(a.previewing());
    let e = a.preview(b"RIFF not a wav").unwrap_err();
    assert!(!e.is_empty());
    assert!(a.has_sound(SoundId::THROW), "the game's sounds stay loaded");
    let mut silent = Audio::<MockBackend>::silent();
    assert!(silent.preview(&good).is_ok() && !silent.previewing());
}
