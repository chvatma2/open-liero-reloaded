//! Computes the simulation fingerprint: a hash of the source of every crate
//! that decides what a tick does (spec §2.2).

use std::fs;
use std::path::{Path, PathBuf};

include!("src/fingerprint_impl.rs");

const DIRS: [&str; 5] = ["rl-math", "rl-data", "rl-terrain", "rl-sim", "rl-modes"];

fn collect(dir: &Path, rel: &str, out: &mut Vec<(String, PathBuf)>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        let rel = format!("{rel}/{name}");
        if p.is_dir() {
            collect(&p, &rel, out);
        } else if name.ends_with(".rs") {
            out.push((rel, p));
        }
    }
}

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets it"));
    let crates = manifest.parent().expect("crates dir").to_path_buf();
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    for d in DIRS {
        let dir = crates.join(d).join("src");
        println!("cargo:rerun-if-changed={}", dir.display());
        collect(&dir, &format!("{d}/src"), &mut files);
    }
    let lib = manifest.join("src/lib.rs");
    println!("cargo:rerun-if-changed={}", lib.display());
    files.push(("rl-proto/src/lib.rs".into(), lib));
    files.sort();
    let bodies: Vec<(String, Vec<u8>)> = files
        .into_iter()
        .map(|(rel, p)| {
            let body: Vec<u8> =
                fs::read(&p).unwrap_or_default().into_iter().filter(|&b| b != b'\r').collect();
            (rel, body)
        })
        .collect();
    let refs: Vec<(&str, &[u8])> = bodies.iter().map(|(a, b)| (a.as_str(), b.as_slice())).collect();
    let h = fingerprint(&refs);
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets it")).join("fingerprint.rs");
    fs::write(out, format!("0x{h:016x}")).expect("write fingerprint");
    println!("cargo:rerun-if-changed=build.rs");
}
