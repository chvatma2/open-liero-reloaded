# olr-audio

Plays the simulation's `Sound` events through kira 0.12.5 the way OLX's
OpenAL driver does (spec §8.4, §19 E1/E12; presentation note §8), and a
level's live copies' sounds (issue #92).

- **Layer 5, uses `std`.** Depends on `olr-sim`, `olr-content`, `olr-math`,
  `kira` (cpal backend; OGG/Vorbis and WAV decoding, no realtime-priority
  features), `log` (a decoder's panic, caught, is an `error`: the game's run
  log, `olr-log`). `#![forbid(unsafe_code)]`. No GPU and no window: it runs on
  any thread.
- **Mix** (`mix.rs`, pure): positional sounds at `SOURCE_GAIN` 0.7 with
  OpenAL's `AL_INVERSE_DISTANCE_CLAMPED` (reference 50, rolloff 2) for a
  listener `LISTENER_DISTANCE` (20 px) out at the view centre; pan `dx / d`.
  The master volume is OLX's `Audio.Volume / 100` (default 0.7). THROW and
  PICKUP (`is_global`) start at the listener with gain 1.0 (OLX's `play`).
  The game passes only the player's own THROW sounds (`olr-game`'s
  `throw_sound`: OLX plays none for a bot's or a remote worm's throw).
  **Several listeners** (issue #27, split screen; a deliberate deviation,
  STATUS): `nearest(listeners, source)` is the listener nearest to a
  source (the first of equally near ones), `mix_nearest` mixes from it;
  with one listener it is `mix`. OLX's OpenAL hears viewport 0 only.
- **Voices** (`voices.rs`, pure): `max_plays` is OLX's `maxplaying` (throw
  4, other game sounds 2, mod sounds 10), `GLOBAL_VOICES` 256 (OpenAL
  Soft's default source count); `admit` says `Start`, `Steal(victim)` or
  `Skip` and changes nothing: a start beyond either limit is skipped, like OLX, except a death
  sound's, which takes the place of the oldest voice of that sound (else,
  at the global limit, the oldest voice that is not a death's; push
  order), so every death is heard (the user's ruling, STATUS); `Audio::play`
  starts the new voice first, then `replace`s the victim and stops it
  with a 5 ms fade (a start that fails takes nothing). Sources stay where they started and every play call re-mixes the
  live voices for the moved listener (OpenAL recomputes as the listener
  follows the view); `follow_nearest(&[listener])` re-mixes each voice
  from the listener now nearest to it.
- **Device** (`audio.rs`): `Audio::new()` opens kira's default backend;
  when it fails the game runs silently (`is_silent`) and
  `take_warnings()` says so once (E12). `Audio<B>` is generic over kira's
  backend: tests use `MockBackend` and a failing one. `load(&LoadedMod,
  &CorePack)` decodes the mod's sounds when content loads (by
  `SimMod::sounds()` index; `olx:` names fall back to the core pack); an
  undecodable sound is a warning and plays nothing. `play_events` once per
  tick with the view centre, or `play_events_heard` (`play_heard` for any
  events) with every local view's centre (`olr_present::Presentation::
  listeners`, what the game passes): each positional sound starts once,
  mixed from the nearest view, nothing doubled; THROW and PICKUP at the
  first listener; no listeners keep the last ones; `stop_all` on a restart. `set_volume`
  re-mixes the voices playing at once (OLX's `SetSoundVolume`), so the
  options apply it live.
  `preview(bytes)` (issue #19, the mod editor's Sounds) decodes and plays
  one sound file at the listener (global gain, the master volume), apart
  from the voices and the loaded sounds, stopping a preview still playing
  (`previewing`); one that does not decode is an `Err`.
- **Live copies' sounds** (issue #92, STATUS.md "Live assets"; no OLX
  counterpart): `load_level_sounds(&LoadedLevel)` decodes the level's
  live sounds (`LoadedLive::source.sounds`, by their index in the level's
  sounds; `load_level_sound_files(names and bytes)` underneath, what the
  tests use) once per level load, apart from the mod's `SoundId`s: one
  that does not decode plays nothing and is a warning, as a mod sound;
  every voice and loop stops and the last level's sounds go. The bytes
  were checked at load (`olr_pack::sounds`' `MAX_SOUND_BYTES`,
  `MAX_SOUND_SECONDS`) and the decoder's panic is caught. `play_live(&
  olr_content::LiveSounds, listeners)` once a presented tick, after
  `play_heard`: its `starts` play once each at their copy's centre
  (`SOURCE_GAIN`, `mix_nearest`, the master volume), at most
  `ASSET_MAX_PLAYS` (4) one-shots of one sound (a start beyond it is
  skipped, never a steal: `Voices::admit_asset`); its `loops` are what
  the presented world plays: the `MAX_LOOPS` (16) nearest to a listener
  (squared distance, ties in copy order) loop (kira's `loop_region(0..)`,
  a 5-ms fade in), keyed by copy and sound (a next state looping the same
  sound plays on), re-mixed every call from their copy's place now and
  the nearest listener; one no longer wanted (a state change, a hidden
  copy, others nearer) stops with `STEAL_FADE`'s 5 ms (no click) and
  starts again when it is wanted again. Loops are counted against
  `GLOBAL_VOICES` with every voice (`Voices::set_reserved`): a start or
  a loop past it waits. `stop_all` stops the loops too and `stop_loops`
  only them (a held game: the pause); the world's start again at the
  next `play_live`. Silent audio plays none. `loops()`
  (`PlayingLoop { copy, sound, source, mix }`) and `asset_one_shots()`
  are what plays, for tests. The game's sounds, their limits and their
  mixing are as before.
- Beams fire silently: the data model gives beam weapons no sound and the
  simulation emits none for their shots.
- **WSL:** sound needs `libasound2-plugins` and `~/.asoundrc` with
  `pcm.!default pulse` / `ctl.!default pulse` (WSLg's PulseServer).

Test: `cargo nextest run -p olr-audio` (pure maths and limits, a mock
device, silence without one, `a_preview_plays_a_sound_file_alone`,
issue #27's `each_sound_is_heard_from_the_nearest_listener`; issue
#92's `an_entry_sound_plays_once_at_its_copy`,
`a_loop_runs_while_its_state_lasts_and_stops_on_a_change`,
`a_loop_follows_its_moving_copy`, `a_loop_is_heard_from_the_nearest_listener`,
`asset_sounds_respect_their_limits`, `an_undecodable_level_sound_is_a_warning`; `decodes_every_mod_sound` needs
`OLR_CONTENT_DIR`). The real device: `cargo run -p olr-audio --example
device_check` (exit 3 means no device opened); `cargo xtask full` runs it.
