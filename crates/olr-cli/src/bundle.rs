//! Desync bundles (networking spec §3.5, §9.6): what a peer saw when the
//! host reported that its simulation differs, so a desync (always our bug)
//! becomes a test. [`reproduce`] replays the recorded inputs on this
//! machine and names the first tick and world field that differ.

use anyhow::{Context as _, ensure};
use olr_content::Content;
use olr_netcode::harness::{Factory as _, HOST};
use olr_netcode::{Simulation as _, WorldSim};
use olr_sim::{Keys, WorldState};
use serde::{Deserialize, Serialize};

use crate::netsim::{NetSpec, WorldFactory};

pub const BUNDLE_MAGIC: [u8; 4] = *b"RLDS";
/// 2: `NetSpec` gained its migration fields (plan 12c). 3: withdrawals
/// (`DesyncBundle::gone`, plan 12d).
pub const BUNDLE_VERSION: u16 = 3;

/// A peer's record of a desync.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DesyncBundle {
    /// How to rebuild the start world (plan 10 replaces this by the lobby's match setup).
    pub spec: NetSpec,
    pub peer: u32,
    /// `olr_proto::FINGERPRINT` of the build that recorded it.
    pub fingerprint: u64,
    /// The recording machine, such as `x86_64-linux`.
    pub platform: String,
    /// Final input rows from tick 0.
    pub inputs: Vec<[u8; 32]>,
    pub local_checksums: Vec<(u32, u64)>,
    /// The mismatching tick and the host's checksum.
    pub host: (u32, u64),
    /// This peer's confirmed state when the desync was reported.
    pub local_state: Option<(u32, Vec<u8>)>,
    /// The state the host sent.
    pub host_state: Option<(u32, Vec<u8>)>,
    /// The withdrawals (plan 12d) below the inputs' end, `(tick, slot)` by
    /// tick then slot (`InputLog::gone_marks`): [`reproduce`] withdraws each
    /// before its tick.
    pub gone: Vec<(u32, u8)>,
}

impl DesyncBundle {
    /// The magic, the version (u16 LE) and the postcard body.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = BUNDLE_MAGIC.to_vec();
        out.extend_from_slice(&BUNDLE_VERSION.to_le_bytes());
        out.extend(postcard::to_allocvec(self).expect("a bundle serializes"));
        out
    }

    pub fn decode(b: &[u8]) -> anyhow::Result<Self> {
        ensure!(b.len() >= 6 && b[..4] == BUNDLE_MAGIC, "not a desync bundle (bad magic)");
        let version = u16::from_le_bytes([b[4], b[5]]);
        ensure!(version == BUNDLE_VERSION, "bundle version {version}, this build reads {BUNDLE_VERSION}");
        postcard::from_bytes(&b[6..]).context("decoding the bundle's body")
    }
}

/// What a replay found.
#[derive(Clone, Debug, PartialEq)]
pub struct Repro {
    /// The first checksum tick at which this machine's replay differs from
    /// the recording peer's own checksums.
    pub first_local_divergence: Option<u32>,
    /// Where the peer's state differs from the replay's: the first
    /// differing path of every differing field of the world state, in
    /// [`FIELDS`] order, comma separated (`worms[3].health, ents`).
    pub local_diff: Option<String>,
    /// The same for the host's state.
    pub host_diff: Option<String>,
    /// `local` and/or `host` when that state's tick is past the recorded
    /// inputs, so the replay cannot reach it.
    pub uncovered: Vec<&'static str>,
}

impl Repro {
    /// Whether the replay agrees with everything the bundle holds.
    pub fn is_clean(&self) -> bool {
        self.first_local_divergence.is_none()
            && self.local_diff.is_none()
            && self.host_diff.is_none()
            && self.uncovered.is_empty()
    }
}

/// The fields of `WorldState`, in its declaration (wire) order: the order
/// differences are listed in. A test keeps it equal to the real field set.
pub const FIELDS: &[&str] = &[
    "width",
    "height",
    "cells",
    "setup_worms",
    "worms",
    "latches",
    "time",
    "physics_time",
    "server_frame",
    "substeps",
    "shots",
    "last_shots",
    "projectiles",
    "rand",
    "mt",
    "ents",
    "rot_bumped",
    "bonuses",
    "mode",
    "over",
    "over_frame",
    "game_over_sent",
    "gone",
];

/// Replays the bundle's inputs from the start world, withdrawing each
/// recorded slot before its tick (as the harness's offline replay does).
pub fn reproduce(content: &Content, b: &DesyncBundle) -> anyhow::Result<Repro> {
    ensure!(!b.inputs.is_empty(), "the bundle holds no inputs");
    ensure!((1..=32).contains(&b.spec.slots), "{} slots: a match has 1 to 32", b.spec.slots);
    let mut sim: WorldSim = WorldFactory::new(content, &b.spec)?.sim(HOST);
    let slots = usize::from(b.spec.slots);
    let sums: std::collections::BTreeMap<u32, u64> = b.local_checksums.iter().copied().collect();
    let mut uncovered = Vec::new();
    let mut states: Vec<(u32, &[u8], bool)> = Vec::new();
    for (state, local) in [(&b.local_state, true), (&b.host_state, false)] {
        if let Some((tick, bytes)) = state {
            if (*tick as usize) < b.inputs.len() {
                states.push((*tick, bytes, local));
            } else {
                uncovered.push(if local { "local" } else { "host" });
            }
        }
    }
    let (mut first_local_divergence, mut local_diff, mut host_diff) = (None, None, None);
    for (t, row) in b.inputs.iter().enumerate() {
        let t = t as u32;
        // Plan 12d: the withdrawals at `t`, before its tick (a slot out of
        // range is the world's to ignore: the bundle is untrusted).
        for &(_, slot) in b.gone.iter().filter(|g| g.0 == t) {
            sim.withdraw(slot);
        }
        let keys: Vec<Keys> = row[..slots].iter().map(|&k| Keys(k)).collect();
        sim.tick(&keys);
        if t % 10 == 9
            && first_local_divergence.is_none()
            && sums.get(&t).is_some_and(|&c| c != sim.checksum())
        {
            first_local_divergence = Some(t);
        }
        for &(_, bytes, local) in states.iter().filter(|s| s.0 == t) {
            let want = serde_json::to_value(sim.world().state())?;
            let got =
                WorldState::decode(bytes).map_err(|e| anyhow::anyhow!("the state after tick {t}: {e:?}"))?;
            let diff = state_difference(&want, &serde_json::to_value(&got)?);
            if local {
                local_diff = diff;
            } else {
                host_diff = diff;
            }
        }
    }
    Ok(Repro { first_local_divergence, local_diff, host_diff, uncovered })
}

/// The first differing path of every differing top-level field, in
/// [`FIELDS`] order (fields it does not know after them, sorted), joined by
/// `, `; `None` if equal.
fn state_difference(a: &serde_json::Value, b: &serde_json::Value) -> Option<String> {
    let mut extra: Vec<&str> = [a, b]
        .iter()
        .filter_map(|v| v.as_object())
        .flat_map(|o| o.keys().map(String::as_str))
        .filter(|k| !FIELDS.contains(k))
        .collect();
    extra.sort_unstable();
    extra.dedup();
    let paths: Vec<String> = FIELDS
        .iter()
        .copied()
        .chain(extra)
        .filter_map(|k| match (a.get(k), b.get(k)) {
            (Some(x), Some(y)) => diff(x, y, k.to_string()),
            (None, None) => None,
            _ => Some(k.to_string()),
        })
        .collect();
    (!paths.is_empty()).then(|| paths.join(", "))
}

/// The path of the first difference between `a` and `b` (objects by sorted
/// key, arrays by index; a length difference is at `<path>.len`), such as
/// `worms[1].health`.
pub fn first_difference(a: &serde_json::Value, b: &serde_json::Value) -> Option<String> {
    diff(a, b, String::new())
}

fn diff(a: &serde_json::Value, b: &serde_json::Value, path: String) -> Option<String> {
    use serde_json::Value::{Array, Object};
    match (a, b) {
        (Object(x), Object(y)) => {
            let mut keys: Vec<&String> = x.keys().chain(y.keys()).collect();
            keys.sort();
            keys.dedup();
            for k in keys {
                let p = if path.is_empty() { k.clone() } else { format!("{path}.{k}") };
                match (x.get(k), y.get(k)) {
                    (Some(u), Some(v)) => {
                        if let Some(d) = diff(u, v, p) {
                            return Some(d);
                        }
                    }
                    _ => return Some(p),
                }
            }
            None
        }
        (Array(x), Array(y)) => {
            for (i, (u, v)) in x.iter().zip(y).enumerate() {
                if let Some(d) = diff(u, v, format!("{path}[{i}]")) {
                    return Some(d);
                }
            }
            (x.len() != y.len()).then(|| format!("{path}.len"))
        }
        _ => (a != b).then_some(path),
    }
}
