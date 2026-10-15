# rl-audio

Plays the simulation's `Sound` events through kira 0.12.5 the way OLX's
OpenAL driver does (spec §8.4, §19 E1/E12; presentation note §8).

- **Layer 5, uses `std`.** Depends on `rl-sim`, `rl-content`, `rl-math`,
  `kira` (cpal backend; OGG/Vorbis and WAV decoding, no realtime-priority
  features). `#![forbid(unsafe_code)]`. No GPU and no window: it runs on
  any thread.
- **Mix** (`mix.rs`, pure): positional sounds at `SOURCE_GAIN` 0.7 with
  OpenAL's `AL_INVERSE_DISTANCE_CLAMPED` (reference 50, rolloff 2) for a
  listener `LISTENER_DISTANCE` (20 px) out at the view centre; pan `dx / d`.
  The master volume is OLX's `Audio.Volume / 100` (default 0.7). THROW and
  PICKUP (`is_global`) start at the listener with gain 1.0 (OLX's `play`).
  The game passes only the player's own THROW sounds (`rl-game`'s
  `throw_sound`: OLX plays none for a bot's or a remote worm's throw).
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
  follows the view).
- **Device** (`audio.rs`): `Audio::new()` opens kira's default backend;
  when it fails the game runs silently (`is_silent`) and
  `take_warnings()` says so once (E12). `Audio<B>` is generic over kira's
  backend: tests use `MockBackend` and a failing one. `load(&LoadedMod,
  &CorePack)` decodes the mod's sounds when content loads (by
  `SimMod::sounds()` index; `olx:` names fall back to the core pack); an
  undecodable sound is a warning and plays nothing. `play_events` once per
  tick with the view centre; `stop_all` on a restart. `set_volume`
  re-mixes the voices playing at once (OLX's `SetSoundVolume`), so the
  options apply it live.
- Beams fire silently: the data model gives beam weapons no sound and the
  simulation emits none for their shots.
- **WSL:** sound needs `libasound2-plugins` and `~/.asoundrc` with
  `pcm.!default pulse` / `ctl.!default pulse` (WSLg's PulseServer).

Test: `cargo nextest run -p rl-audio` (pure maths and limits, a mock
device, silence without one; `decodes_every_mod_sound` needs
`RL_CONTENT_DIR`). The real device: `cargo run -p rl-audio --example
device_check` (exit 3 means no device opened); `cargo xtask full` runs it.
