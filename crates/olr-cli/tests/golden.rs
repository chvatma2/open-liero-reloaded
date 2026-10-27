//! Golden match checksums (spec §7.2): fixed-seed bot matches whose
//! checksum streams are committed in `fixtures/matches/`, and (issue #89)
//! those on the live fixture level in `fixtures/matches-live/`, played on
//! `fixtures/live/levels` laid over the content. Any change of behaviour
//! shows as a difference; accept it with
//! `cargo xtask bless-regression --reason "<why>"` (which runs this test
//! with `OLR_BLESS_REGRESSION` set, so it rewrites the streams of both).

mod common;

use std::path::{Path, PathBuf};

use common::*;
use olr_cli::golden::{Golden, first_difference};
use olr_cli::runner::{Options, play};
use olr_cli::verify::verify;
use olr_content::Content;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

/// Plays every golden of `fixtures/<dir>` on `c` (and, with `check`,
/// verifies each: determinism and the replay), comparing or (blessing)
/// rewriting its stream.
fn goldens(c: &Content, dir: &str, at_least: usize, check: bool) {
    let bless = std::env::var_os("OLR_BLESS_REGRESSION").is_some();
    let goldens = Golden::load_dir(&fixtures().join(dir)).unwrap();
    assert!(goldens.len() >= at_least, "fixtures/{dir} holds the golden matches");
    let mut diffs = Vec::new();
    for (path, golden) in goldens {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let run = play(c, &golden.spec, &Options::default()).unwrap();
        assert!(run.failure().is_none(), "{name}: {}", run.summary());
        if check {
            if let Some(f) = verify(c, &run).unwrap() {
                panic!("{}", capture(&run, &f));
            }
            // The scripts ran: states changed, motions arrived, none
            // failed.
            let count = |kind: &str| {
                let i = olr_cli::coverage::EVENT_KINDS.iter().position(|k| *k == kind).unwrap();
                run.coverage.events[i]
            };
            assert!(
                count("LiveState") > 20 && count("LiveArrived") >= 10,
                "{name}: {:?}",
                run.coverage.events
            );
            assert_eq!(count("ScriptError"), 0, "{name}: a script failed");
        }
        let fresh = Golden::of(&run);
        if bless {
            fresh.save(&path).unwrap();
        } else if let Some(d) = first_difference(&golden, &fresh) {
            diffs.push(format!("fixtures/{dir}/{name}: {d}"));
        }
    }
    assert!(
        diffs.is_empty(),
        "the matches no longer play as recorded:\n{}\nIf the change is intended, run \
         `cargo xtask bless-regression --reason \"<why>\"`",
        diffs.join("\n")
    );
}

#[test]
fn golden_match_checksums() {
    let Some(c) = content() else { return };
    goldens(&c, "matches", 3, false);
}

/// Issue #89: the live goldens, on the fixture level's scripts and
/// motions, with `verify`'s replay and determinism.
#[test]
fn live_golden_match_checksums() {
    let Some(dir) = content_dir() else { return };
    let live = fixtures().join("live/levels");
    let c = Content::open_layers(&[live.as_path(), Path::new(&dir)]).expect("the layered content");
    goldens(&c, "matches-live", 1, true);
}
