//! Golden match checksums (spec §7.2): fixed-seed bot matches whose
//! checksum streams are committed in `fixtures/matches/`. Any change of
//! behaviour shows as a difference; accept it with
//! `cargo xtask bless-regression --reason "<why>"` (which runs this test
//! with `RL_BLESS_REGRESSION` set, so it rewrites the streams).

mod common;

use std::path::PathBuf;

use common::*;
use rl_cli::golden::{Golden, first_difference};
use rl_cli::runner::{Options, play};

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/matches")
}

#[test]
fn golden_match_checksums() {
    let Some(c) = content() else { return };
    let bless = std::env::var_os("RL_BLESS_REGRESSION").is_some();
    let goldens = Golden::load_dir(&dir()).unwrap();
    assert!(goldens.len() >= 3, "fixtures/matches holds the golden matches");
    let mut diffs = Vec::new();
    for (path, golden) in goldens {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let run = play(&c, &golden.spec, &Options::default()).unwrap();
        assert!(run.failure().is_none(), "{name}: {}", run.summary());
        let fresh = Golden::of(&run);
        if bless {
            fresh.save(&path).unwrap();
        } else if let Some(d) = first_difference(&golden, &fresh) {
            diffs.push(format!("fixtures/matches/{name}: {d}"));
        }
    }
    assert!(
        diffs.is_empty(),
        "the matches no longer play as recorded:\n{}\nIf the change is intended, run \
         `cargo xtask bless-regression --reason \"<why>\"`",
        diffs.join("\n")
    );
}
