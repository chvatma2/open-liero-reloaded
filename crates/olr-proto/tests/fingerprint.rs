//! The simulation fingerprint (spec §2.2) covers every crate whose source
//! decides what peers compute together: the tick (olr-math, olr-data,
//! olr-terrain, olr-sim, olr-modes), the wire format (olr-proto) and the
//! session's semantics (olr-netcode: checksum cadence, acks, resends, the
//! world's checksum).

use std::path::{Path, PathBuf};

use crate::fingerprint_impl;

const HASHED: [&str; 7] =
    ["olr-math", "olr-data", "olr-terrain", "olr-sim", "olr-modes", "olr-netcode", "olr-proto"];

fn collect(dir: &Path, rel: &str, out: &mut Vec<(String, PathBuf)>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        let rel = format!("{rel}/{}", p.file_name().unwrap().to_string_lossy());
        if p.is_dir() {
            collect(&p, &rel, out);
        } else if rel.ends_with(".rs") {
            out.push((rel, p));
        }
    }
}

#[test]
fn the_fingerprint_hashes_the_simulation_and_session_sources() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut files = Vec::new();
    for c in HASHED {
        collect(&crates.join(c).join("src"), &format!("{c}/src"), &mut files);
    }
    files.sort();
    let bodies: Vec<(String, Vec<u8>)> = files
        .into_iter()
        .map(|(rel, p)| (rel, std::fs::read(p).unwrap().into_iter().filter(|&b| b != b'\r').collect()))
        .collect();
    let refs: Vec<(&str, &[u8])> = bodies.iter().map(|(a, b)| (a.as_str(), b.as_slice())).collect();
    assert_eq!(fingerprint_impl::fingerprint(&refs), olr_proto::FINGERPRINT);
}

/// Issue #86: the state transfer's live copies changed every state's
/// encoding.
#[test]
fn the_protocol_version_is_7() {
    assert_eq!(olr_proto::PROTOCOL_VERSION, 7);
}
