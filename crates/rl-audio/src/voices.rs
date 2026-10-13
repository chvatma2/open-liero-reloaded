//! Which sounds may start, and the live voices' mix as the listener moves.
//! Pure: a voice is anything that says whether it still plays and takes a
//! new [`Mix`].

use rl_math::Vec2;
use rl_sim::{GAME_SOUNDS, SoundId};

use crate::mix::{Mix, mix};

/// A playing sound, as [`Voices`] sees it.
pub trait Voice {
    /// Whether it still plays (a stopped voice no longer counts).
    fn playing(&self) -> bool;
    /// Plays on at `mix`.
    fn set_mix(&mut self, mix: Mix);
}

/// How many copies of sound `id` may play at once: OLX's `maxplaying` for
/// its game sounds (throw 4, pickup, bump and the deaths 2) and 10 for a
/// mod's sounds. A start beyond it is skipped, but a death sound's
/// ([`Voices::admit`]).
/// OLX: src/game/Sounds.cpp:25-30, src/common/CGameScript.cpp:1185-1205,
/// src/sound/sfx.cpp:120-134.
pub fn max_plays(id: SoundId) -> usize {
    const GAME: [usize; GAME_SOUNDS.len()] = [4, 2, 2, 2, 2, 2];
    GAME.get(usize::from(id.0)).copied().unwrap_or(10)
}

/// How many voices may play at once in all: OpenAL Soft's default source
/// count, the limit OLX met (`alGenSources` fails, and the sound is
/// skipped). A start beyond it is skipped, but a death sound's
/// ([`Voices::admit`]).
pub const GLOBAL_VOICES: usize = 256;

/// What a new sound may do ([`Voices::admit`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admit {
    /// Start: it is below both limits.
    Start,
    /// Start in this voice's place ([`Voices::replace`] once the new voice
    /// has started; until then the victim plays on and counts).
    Steal(Victim),
    /// Do not start.
    Skip,
}

/// A live voice a death sound may take the place of ([`Admit::Steal`]):
/// valid until the voices change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Victim(usize);

struct Live<V> {
    sound: SoundId,
    /// Where the source is, in world px (a global sound's is the listener's
    /// position when it started, like OLX's).
    source: Vec2,
    gain: f32,
    voice: V,
}

/// The voices playing now.
pub struct Voices<V> {
    live: Vec<Live<V>>,
    limit: usize,
}

impl<V: Voice> Voices<V> {
    /// At most `limit` voices at once ([`GLOBAL_VOICES`]).
    pub fn new(limit: usize) -> Voices<V> {
        Voices { live: Vec::with_capacity(limit), limit }
    }

    /// How many voices play.
    pub fn len(&self) -> usize {
        self.live.len()
    }

    pub fn is_empty(&self) -> bool {
        self.live.is_empty()
    }

    /// How many copies of `sound` play.
    pub fn playing(&self, sound: SoundId) -> usize {
        self.live.iter().filter(|l| l.sound == sound).count()
    }

    /// Forgets the voices that stopped.
    pub fn prune(&mut self) {
        self.live.retain(|l| l.voice.playing());
    }

    /// Whether `sound` may start now: with fewer than [`max_plays`] copies
    /// of it and fewer than the limit in all, it starts; beyond either it
    /// is skipped, as OLX skips it, unless it is a death sound
    /// (`SoundId::DEATH`), which is never skipped while another voice can
    /// make room (the user's ruling, `docs/porting/STATUS.md`): it takes
    /// the place of the oldest voice of the same sound, else (at the
    /// global limit with none of its own) of the oldest voice that is not
    /// a death's. Nothing changes yet: the caller starts the new voice and
    /// then calls [`Voices::replace`], so a start that fails leaves the old
    /// voice playing. Oldest is push order, so it is deterministic.
    pub fn admit(&self, sound: SoundId) -> Admit {
        let own = self.playing(sound) >= max_plays(sound);
        if !own && self.live.len() < self.limit {
            return Admit::Start;
        }
        if !SoundId::DEATH.contains(&sound) {
            return Admit::Skip;
        }
        let same = self.live.iter().position(|l| l.sound == sound);
        let victim = if own {
            same
        } else {
            same.or_else(|| self.live.iter().position(|l| !SoundId::DEATH.contains(&l.sound)))
        };
        match victim {
            Some(i) => Admit::Steal(Victim(i)),
            None => Admit::Skip,
        }
    }

    /// The voice `victim` names.
    pub fn victim(&self, victim: Victim) -> &V {
        &self.live[victim.0].voice
    }

    /// A started death sound takes `victim`'s place: the victim leaves the
    /// live voices (it no longer counts) and is returned for the caller to
    /// stop, and the new voice is added as [`Voices::push`] adds it.
    pub fn replace(&mut self, victim: Victim, sound: SoundId, source: Vec2, gain: f32, voice: V) -> V {
        let old = self.live.remove(victim.0).voice;
        self.push(sound, source, gain, voice);
        old
    }

    /// Adds a started voice.
    pub fn push(&mut self, sound: SoundId, source: Vec2, gain: f32, voice: V) {
        self.live.push(Live { sound, source, gain, voice });
    }

    /// Re-mixes every voice for a listener now at `listener`: OpenAL
    /// sources stay where they started while the listener follows the view.
    pub fn follow(&mut self, listener: Vec2, volume: f32) {
        for l in &mut self.live {
            l.voice.set_mix(mix(listener, l.source, l.gain, volume));
        }
    }

    /// Drops every voice (the caller stops them first).
    pub fn clear(&mut self) {
        self.live.clear();
    }

    /// The voices, to stop them.
    pub fn voices_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.live.iter_mut().map(|l| &mut l.voice)
    }
}
