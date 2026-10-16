//! The file format: [`crate::MAGIC`], [`crate::FORMAT_VERSION`] (`u16`,
//! little-endian), then the replay in postcard (varint integers,
//! little-endian floats bit for bit). The keys are stored by worm, each
//! worm's column after the other, deflated: a worm's keys repeat (held, or
//! in patterns such as a bot's jump taps), so the columns deflate well; keys
//! that do not cost at most about one byte per worm and tick.

use alloc::format;
use alloc::vec::Vec;

use olr_math::AbsTime;
use olr_sim::{Keys, MAX_SLOTS, MAX_WORMS};
use serde::{Deserialize, Serialize};

use crate::mirror::{CallW, ModeW, PacksW, RespawnW, SettingsW, WormW};
use crate::{
    DecodeError, FORMAT_VERSION, HEADER_BYTES, MAGIC, MAX_SETUP_CALLS, MAX_TICKS, Replay, ReplaySetup,
};

#[derive(Serialize, Deserialize)]
struct SetupW {
    packs: PacksW,
    settings: SettingsW,
    start: u64,
    seed: u32,
    worms: Vec<WormW>,
    pool_capacity: u32,
    bonus_weapons: Option<Vec<i32>>,
    calls: Vec<CallW>,
    viewer: Option<u8>,
}

#[derive(Serialize, Deserialize)]
struct Wire {
    setup: SetupW,
    mode: ModeW,
    ticks: u32,
    /// Keys per row.
    width: u8,
    /// The keys by worm (worm 0's for every tick, then worm 1's, ...),
    /// deflated.
    keys: Vec<u8>,
    checksums: Vec<(u32, u64)>,
    withdrawals: Vec<(u32, u8)>,
}

/// The deflate level: the best (bot keys deflate to about a quarter).
const LEVEL: u8 = 9;

/// The keys by worm: worm 0's for every tick, then worm 1's, ...
fn columns(r: &Replay) -> Vec<u8> {
    (0..r.width()).flat_map(|i| (0..r.ticks()).map(move |t| r.row(t)[i].0)).collect()
}

impl Replay {
    /// The replay in the file format.
    pub fn encode(&self) -> Vec<u8> {
        let s = &self.setup;
        let wire = Wire {
            setup: SetupW {
                packs: (&s.packs).into(),
                settings: (&s.settings).into(),
                start: s.start.0,
                seed: s.seed,
                worms: s.worms.iter().map(WormW::from).collect(),
                // `World::new` refuses pools above 65 536 slots.
                pool_capacity: u32::try_from(s.pool_capacity).unwrap_or(u32::MAX),
                bonus_weapons: s.bonus_weapons.clone(),
                calls: s.calls.iter().map(|&c| c.into()).collect(),
                viewer: s.viewer,
            },
            mode: (&self.mode).into(),
            ticks: self.ticks(),
            // At most `MAX_WORMS` (32): a recorder keeps no more.
            width: u8::try_from(self.width()).unwrap_or(u8::MAX),
            keys: miniz_oxide::deflate::compress_to_vec(&columns(self), LEVEL),
            checksums: self.checksums.clone(),
            withdrawals: self.withdrawals.clone(),
        };
        let mut out = Vec::from(MAGIC);
        out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        // Postcard fails only on types it cannot encode (sequences of
        // unknown length); these types have none.
        postcard::to_extend(&wire, out).expect("postcard encodes every replay")
    }

    /// Whether `bytes` start as a replay this build reads: [`MAGIC`] and
    /// [`FORMAT_VERSION`] (the first [`HEADER_BYTES`] bytes; the rest is not
    /// read, so a menu can list files cheaply).
    pub fn check_header(bytes: &[u8]) -> Result<(), DecodeError> {
        if bytes.get(..4) != Some(&MAGIC[..]) {
            return Err(DecodeError::Magic);
        }
        let Some(&[a, b]) = bytes.get(4..HEADER_BYTES) else {
            return Err(DecodeError::Malformed("no format version".into()));
        };
        let found = u16::from_le_bytes([a, b]);
        if found != FORMAT_VERSION {
            return Err(DecodeError::Version { found, supported: FORMAT_VERSION });
        }
        Ok(())
    }

    /// Reads a replay in the file format. Another format version, broken
    /// bytes or values no recorder writes are errors; it never panics, and
    /// it allocates at most what [`crate::MAX_TICKS`] ticks of
    /// [`MAX_WORMS`] worms need beyond the input's size.
    pub fn decode(bytes: &[u8]) -> Result<Replay, DecodeError> {
        Replay::check_header(bytes)?;
        let (wire, rest) = postcard::take_from_bytes::<Wire>(&bytes[HEADER_BYTES..])
            .map_err(|e| DecodeError::Malformed(format!("{e}")))?;
        if !rest.is_empty() {
            return Err(DecodeError::Malformed(format!("{} trailing bytes", rest.len())));
        }
        from_wire(wire)
    }
}

fn invalid<T>(m: alloc::string::String) -> Result<T, DecodeError> {
    Err(DecodeError::Invalid(m))
}

fn from_wire(w: Wire) -> Result<Replay, DecodeError> {
    let Wire { setup, mode, ticks, width, keys, checksums, withdrawals } = w;
    if ticks > MAX_TICKS {
        return invalid(format!("{ticks} ticks, at most {MAX_TICKS}"));
    }
    if usize::from(width) > MAX_WORMS {
        return invalid(format!("keys for {width} worms, at most {MAX_WORMS}"));
    }
    let mut prev = None;
    for &(t, _) in &checksums {
        if t > ticks || prev.is_some_and(|p| p >= t) {
            return invalid(format!("checksum at tick {t} out of order"));
        }
        prev = Some(t);
    }
    if withdrawals.len() > MAX_WORMS {
        return invalid(format!("{} withdrawals, at most {MAX_WORMS}", withdrawals.len()));
    }
    let mut seen = 0u32;
    let mut prev = 0;
    for &(t, worm) in &withdrawals {
        if t > ticks || t < prev {
            return invalid(format!("withdrawal at tick {t} out of order or past the end (tick {ticks})"));
        }
        if usize::from(worm) >= MAX_WORMS || seen & (1 << worm) != 0 {
            return invalid(format!("withdrawal of worm {worm}: no such worm, or withdrawn twice"));
        }
        seen |= 1 << worm;
        prev = t;
    }
    let SetupW { packs, settings, start, seed, worms, pool_capacity, bonus_weapons, calls, viewer } = setup;
    if worms.len() > MAX_WORMS {
        return invalid(format!("{} worms, at most {MAX_WORMS}", worms.len()));
    }
    if let Some(i) = worms.iter().position(|w| w.slots.len() > MAX_SLOTS) {
        return invalid(format!("worm {i}: more than {MAX_SLOTS} weapon slots"));
    }
    if let Some(v) = viewer.filter(|&v| usize::from(v) >= worms.len()) {
        return invalid(format!("viewer {v} is no worm of {}", worms.len()));
    }
    if calls.len() > MAX_SETUP_CALLS {
        return invalid(format!("{} setup calls, at most {MAX_SETUP_CALLS}", calls.len()));
    }
    if let ModeW::Deathmatch(d) = &mode
        && let RespawnW::ChaosCycle { worms, .. } = &d.respawn_loadout
        && worms.iter().any(|&w| usize::from(w) >= MAX_WORMS)
    {
        return invalid("chaos worm out of range".into());
    }
    let (width, size) = (usize::from(width), ticks as usize * usize::from(width));
    let columns = miniz_oxide::inflate::decompress_to_vec_with_limit(&keys, size)
        .map_err(|e| DecodeError::Malformed(format!("keys: {e}")))?;
    if columns.len() != size {
        return invalid(format!("{} keys for {ticks} ticks of {width} worms", columns.len()));
    }
    let mut keys = alloc::vec![Keys(0); size];
    if ticks > 0 {
        for (i, column) in columns.chunks_exact(ticks as usize).enumerate() {
            column.iter().enumerate().for_each(|(t, &k)| keys[t * width + i] = Keys(k));
        }
    }
    let setup = ReplaySetup {
        packs: packs.into(),
        settings: settings.into(),
        start: AbsTime(start),
        seed,
        worms: worms.into_iter().map(Into::into).collect(),
        pool_capacity: pool_capacity as usize,
        bonus_weapons,
        calls: calls.into_iter().map(Into::into).collect(),
        viewer,
    };
    // NaN, infinities and values outside OLX's ranges, before anything
    // computes with them (`World::new` and `Replay::world` check them again).
    setup.settings.validate().map_err(|e| DecodeError::Invalid(format!("settings: {}", e.0)))?;
    let mode: crate::ModeSpec = mode.into();
    mode.validate(&setup.settings).map_err(|e| DecodeError::Invalid(format!("mode: {e}")))?;
    Ok(Replay { setup, mode, checksums, withdrawals, ticks, width, keys })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mirror::SlotW;
    use alloc::vec;
    use olr_sim::{Loadout, SimSettings, Slot, WormSetup};

    /// A replay with no worms and `ticks` ticks, as the wire carries it.
    fn wire(ticks: u32, width: u8, keys: &[u8]) -> Wire {
        let r = |id: &str| crate::PackRef { id: id.into(), hash: id.into() };
        let packs = crate::Packs { mod_pack: r("m"), level: r("l"), theme: r("t") };
        let w = WormSetup {
            team: 0,
            loadout: Loadout::new(vec![Slot::ready(0)], &SimSettings::lx56()),
            spawn: None,
        };
        Wire {
            setup: SetupW {
                packs: (&packs).into(),
                settings: (&SimSettings::lx56()).into(),
                start: 100_000,
                seed: 1,
                worms: vec![(&w).into()],
                pool_capacity: 3000,
                bonus_weapons: None,
                calls: vec![],
                viewer: None,
            },
            mode: ModeW::Oracle,
            ticks,
            width,
            keys: miniz_oxide::deflate::compress_to_vec(keys, LEVEL),
            checksums: vec![],
            withdrawals: vec![],
        }
    }

    fn err(w: Wire) -> DecodeError {
        from_wire(w).unwrap_err()
    }

    #[test]
    fn decoding_checks_every_bound() {
        assert!(from_wire(wire(3, 2, &[1, 2, 3, 4, 5, 6])).is_ok());
        let r = from_wire(wire(3, 2, &[1, 2, 3, 4, 5, 6])).unwrap();
        assert_eq!(r.row(1), &[Keys(2), Keys(5)], "columns become rows");
        assert!(matches!(err(wire(MAX_TICKS + 1, 0, &[])), DecodeError::Invalid(_)));
        assert!(matches!(err(wire(1, 33, &[0; 33])), DecodeError::Invalid(_)));
        // Keys that inflate to more or fewer than ticks × width bytes.
        assert!(matches!(err(wire(3, 2, &[0; 7])), DecodeError::Malformed(_)));
        assert!(matches!(err(wire(3, 2, &[0; 5])), DecodeError::Invalid(_)));
        let mut w = wire(3, 0, &[]);
        w.checksums = vec![(2, 1), (2, 2)];
        assert!(matches!(err(w), DecodeError::Invalid(_)), "checksums out of order");
        let mut w = wire(3, 0, &[]);
        w.checksums = vec![(4, 1)];
        assert!(matches!(err(w), DecodeError::Invalid(_)), "checksum past the end");
        let mut w = wire(0, 0, &[]);
        w.setup.worms[0].slots = (0..=MAX_SLOTS).map(|_| SlotW::from(&Slot::ready(0))).collect();
        assert!(matches!(err(w), DecodeError::Invalid(_)), "too many slots");
        let mut w = wire(0, 0, &[]);
        w.setup.calls = vec![CallW::FindSpot; MAX_SETUP_CALLS + 1];
        assert!(matches!(err(w), DecodeError::Invalid(_)), "too many calls");
        let mut w = wire(0, 0, &[]);
        w.keys = vec![0xff; 4];
        assert!(matches!(err(w), DecodeError::Malformed(_)), "broken deflate stream");
        assert_eq!(from_wire(wire(0, 0, &[])).unwrap().ticks(), 0);
    }

    #[test]
    fn the_viewer_is_one_of_the_worms() {
        let mut w = wire(0, 0, &[]);
        w.setup.viewer = Some(0);
        assert_eq!(from_wire(w).unwrap().setup.viewer, Some(0));
        let mut w = wire(0, 0, &[]);
        w.setup.viewer = Some(1);
        assert!(matches!(err(w), DecodeError::Invalid(_)), "worm 1 of 1");
    }
}
