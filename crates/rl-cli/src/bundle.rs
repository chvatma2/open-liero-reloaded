//! Desync bundles (networking spec §3.5, §9.6): what a peer saw when the
//! host reported that its simulation differs, so a desync (always our bug)
//! becomes a test. [`reproduce`] replays the recorded inputs on this
//! machine and names the first tick and world field that differ.

use anyhow::{Context as _, bail, ensure};
use rl_content::Content;
use rl_netcode::harness::{Factory as _, HOST};
use rl_netcode::{Simulation as _, WorldSim};
use rl_sim::{Keys, WorldState};
use serde::{Deserialize, Serialize};

use crate::netsim::{NetSpec, WorldFactory};

pub const BUNDLE_MAGIC: [u8; 4] = *b"RLDS";
pub const BUNDLE_VERSION: u16 = 1;

/// A peer's record of a desync.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DesyncBundle {
    /// How to rebuild the start world (plan 10 replaces this by the lobby's match setup).
    pub spec: NetSpec,
    pub peer: u16,
    /// `rl_proto::FINGERPRINT` of the build that recorded it.
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
    /// The first world field where the peer's state differs from the replay's.
    pub local_diff: Option<String>,
    /// The same for the host's state.
    pub host_diff: Option<String>,
}

impl Repro {
    /// Whether the replay agrees with everything the bundle holds.
    pub fn is_clean(&self) -> bool {
        self.first_local_divergence.is_none() && self.local_diff.is_none() && self.host_diff.is_none()
    }
}

/// Replays the bundle's inputs from the start world.
pub fn reproduce(content: &Content, b: &DesyncBundle) -> anyhow::Result<Repro> {
    let mut sim: WorldSim = WorldFactory::new(content, &b.spec)?.sim(HOST);
    let slots = usize::from(b.spec.slots);
    let sums: std::collections::BTreeMap<u32, u64> = b.local_checksums.iter().copied().collect();
    let mut states: Vec<(u32, &[u8], bool)> = Vec::new();
    for (state, local) in [(&b.local_state, true), (&b.host_state, false)] {
        if let Some((tick, bytes)) = state {
            ensure!(
                (*tick as usize) < b.inputs.len(),
                "a state after tick {tick} but inputs for only {} ticks",
                b.inputs.len()
            );
            states.push((*tick, bytes, local));
        }
    }
    let (mut first_local_divergence, mut local_diff, mut host_diff) = (None, None, None);
    for (t, row) in b.inputs.iter().enumerate() {
        let t = t as u32;
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
    if b.inputs.is_empty() {
        bail!("the bundle holds no inputs");
    }
    Ok(Repro { first_local_divergence, local_diff, host_diff })
}

/// The first difference between two serialized world states. The worms come
/// first: their fields are causes (a health, a position) where the counters
/// after them (`ents`, the generators) are effects, which sorted key order
/// would name before `worms`.
fn state_difference(a: &serde_json::Value, b: &serde_json::Value) -> Option<String> {
    if let (Some(x), Some(y)) = (a.get("worms"), b.get("worms"))
        && let Some(d) = diff(x, y, "worms".to_string())
    {
        return Some(d);
    }
    first_difference(a, b)
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
