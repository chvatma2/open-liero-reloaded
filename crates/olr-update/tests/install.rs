//! The portable folder's update in a temporary directory: staging a
//! release zip, the swap, an interruption at every step and the next
//! start's recovery, and the zips refused.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::Path;

use olr_update::install::{
    MANIFEST_FILE, Plan, Recovery, WORK_DIR, check_writable, claim, manifest_text, recover, stage_zip, swap,
};

const EXE: &str = "olr.exe";
const TOP: &str = "open-liero-reloaded-0.2.1";

/// Every file below `dir` (the work directory left out) and its text.
fn tree(dir: &Path) -> BTreeMap<String, String> {
    fn walk(dir: &Path, prefix: &str, out: &mut BTreeMap<String, String>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let e = e.unwrap();
            let name = e.file_name().into_string().unwrap();
            if prefix.is_empty() && name == WORK_DIR {
                continue;
            }
            let rel = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
            if e.file_type().unwrap().is_dir() {
                walk(&e.path(), &rel, out);
            } else {
                out.insert(rel, std::fs::read_to_string(e.path()).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, "", &mut out);
    out
}

fn write(dir: &Path, files: &[(&str, &str)]) {
    for (f, text) in files {
        let p = dir.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
}

/// The installed old release (its manifest) and the player's own files.
fn old_folder(game: &Path) {
    let release = [
        (EXE, "old exe"),
        ("olr-server.exe", "old server"),
        ("content/packs/a.ron", "old a"),
        ("docs/gone.md", "old doc"),
    ];
    write(game, &release);
    let listed: Vec<String> =
        release.iter().map(|(f, _)| f.to_string()).chain([MANIFEST_FILE.into()]).collect();
    std::fs::write(game.join(MANIFEST_FILE), manifest_text(&listed)).unwrap();
    write(game, &[("my-notes.txt", "mine"), ("content/packs/mine/level.ron", "my level")]);
}

/// A release zip of `files` under the folder `top`.
fn zip_of(path: &Path, top: &str, files: &[(&str, &str)]) {
    let mut z = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (f, text) in files {
        z.start_file(format!("{top}/{f}"), o).unwrap();
        z.write_all(text.as_bytes()).unwrap();
    }
    z.finish().unwrap();
}

const NEW: [(&str, &str); 4] = [
    (EXE, "new exe"),
    ("olr-server.exe", "new server"),
    ("content/packs/a.ron", "new a"),
    ("content/packs/b.ron", "new b"),
];

/// The folder after the update: the new release, its manifest, the
/// player's files; the old release's other files gone.
fn expected() -> BTreeMap<String, String> {
    let mut want: BTreeMap<String, String> =
        NEW.iter().map(|(f, t)| (f.to_string(), t.to_string())).collect();
    let listed: Vec<String> = NEW.iter().map(|(f, _)| f.to_string()).chain([MANIFEST_FILE.into()]).collect();
    want.insert(MANIFEST_FILE.into(), manifest_text(&listed));
    want.insert("my-notes.txt".into(), "mine".into());
    want.insert("content/packs/mine/level.ron".into(), "my level".into());
    want
}

/// A folder with the old release and the new one staged; gives the plan.
fn staged(dir: &Path) -> (std::path::PathBuf, Plan) {
    let game = dir.join("game");
    std::fs::create_dir_all(&game).unwrap();
    old_folder(&game);
    let zip = dir.join("release.zip");
    zip_of(&zip, TOP, &NEW);
    check_writable(&game).unwrap();
    let files = stage_zip(&zip, &game, TOP, EXE).unwrap();
    // Staging changes nothing in the folder.
    assert_eq!(tree(&game).get(EXE).map(String::as_str), Some("old exe"));
    (game.clone(), Plan::new(&game, "0.2.1", &files, EXE))
}

#[test]
fn a_swap_installs_the_release_and_keeps_the_players_files() {
    let dir = tempfile::tempdir().unwrap();
    let (game, plan) = staged(dir.path());
    assert_eq!(plan.install[0], EXE, "the program goes first");
    assert_eq!(plan.remove, ["docs/gone.md"]);
    swap(&game, &plan, &mut |_| Ok(())).unwrap();
    assert_eq!(tree(&game), expected());
    assert!(!game.join("docs").exists(), "an emptied folder goes");
    // The old program waits in the work directory until the next start.
    assert_eq!(std::fs::read_to_string(game.join(WORK_DIR).join("old").join(EXE)).unwrap(), "old exe");
    assert_eq!(recover(&game, EXE), Recovery::Cleaned);
    assert!(!game.join(WORK_DIR).exists());
    assert_eq!(recover(&game, EXE), Recovery::Nothing);
}

#[test]
fn an_interrupted_swap_is_finished_at_the_next_start() {
    let dir = tempfile::tempdir().unwrap();
    let (game, plan) = staged(dir.path());
    let mut steps = Vec::new();
    swap(&game, &plan, &mut |s| {
        steps.push(s.to_string());
        Ok(())
    })
    .unwrap();
    assert!(steps.len() > 8, "{steps:?}");
    for (k, at) in steps.iter().enumerate() {
        let dir = tempfile::tempdir().unwrap();
        let (game, plan) = staged(dir.path());
        let mut n = 0;
        let r = swap(&game, &plan, &mut |_| {
            n += 1;
            if n > k { Err(std::io::Error::other("power cut")) } else { Ok(()) }
        });
        assert!(r.is_err(), "interrupted before {at}");
        // The program is there to start, but between its two renames.
        let gap = at == &format!("in {EXE}");
        assert_eq!(game.join(EXE).exists(), !gap, "olr.exe interrupted before {at}");
        let recovered = recover(&game, EXE);
        if at == "journal" {
            // Not begun: the folder is the old release, the staging gone.
            assert_eq!(recovered, Recovery::Cleaned);
            assert_eq!(tree(&game).get(EXE).map(String::as_str), Some("old exe"));
            assert!(game.join("docs/gone.md").exists());
        } else {
            let exe_swapped = steps[..k].iter().any(|s| s == &format!("in {EXE}"));
            assert_eq!(
                recovered,
                Recovery::Finished { version: "0.2.1".into(), restart: !exe_swapped },
                "interrupted before {at}"
            );
            assert_eq!(tree(&game), expected(), "interrupted before {at}");
        }
        assert!(!game.join(WORK_DIR).exists());
    }
}

#[test]
fn a_damaged_journal_leaves_the_folder_alone() {
    let dir = tempfile::tempdir().unwrap();
    let (game, _) = staged(dir.path());
    std::fs::write(game.join(WORK_DIR).join("swap.txt"), "olr-swap 1\nversion 0.2.1\ninstall olr.exe\n")
        .unwrap();
    assert!(matches!(recover(&game, EXE), Recovery::Failed(_)));
    assert_eq!(tree(&game).get(EXE).map(String::as_str), Some("old exe"));
    std::fs::write(game.join(WORK_DIR).join("swap.txt"), "olr-swap 1\nversion 0.2.1\ninstall ../x\nend\n")
        .unwrap();
    assert!(matches!(recover(&game, EXE), Recovery::Failed(_)));
}

#[test]
fn a_folder_without_a_manifest_loses_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let game = dir.path().join("game");
    write(&game, &[(EXE, "old exe"), ("old.txt", "?")]);
    let zip = dir.path().join("r.zip");
    zip_of(&zip, TOP, &NEW);
    let files = stage_zip(&zip, &game, TOP, EXE).unwrap();
    let plan = Plan::new(&game, "0.2.1", &files, EXE);
    assert!(plan.remove.is_empty());
    swap(&game, &plan, &mut |_| Ok(())).unwrap();
    let t = tree(&game);
    assert_eq!(t.get("old.txt").map(String::as_str), Some("?"));
    assert_eq!(t.get(EXE).map(String::as_str), Some("new exe"));
    assert!(t.contains_key(MANIFEST_FILE));
}

#[test]
fn bad_zips_are_refused_before_anything_moves() {
    let dir = tempfile::tempdir().unwrap();
    let game = dir.path().join("game");
    write(&game, &[(EXE, "old exe")]);
    type Case<'a> = (&'a str, &'a str, &'a [(&'a str, &'a str)]);
    let cases: [Case; 5] = [
        ("traversal", TOP, &[(EXE, "x"), ("../evil.txt", "x")]),
        ("work dir", TOP, &[(EXE, "x"), (".olr-update/swap.txt", "x")]),
        ("no program", TOP, &[("olr-server.exe", "x")]),
        ("another folder", "open-liero-reloaded-9.9.9", &[(EXE, "x")]),
        ("drive", TOP, &[(EXE, "x"), ("C:/evil.txt", "x")]),
    ];
    for (what, top, files) in cases {
        let zip = dir.path().join(format!("{what}.zip"));
        zip_of(&zip, top, files);
        assert!(stage_zip(&zip, &game, TOP, EXE).is_err(), "{what}");
        assert!(!dir.path().join("evil.txt").exists());
    }
    let not_zip = dir.path().join("x.zip");
    std::fs::write(&not_zip, b"PK not really").unwrap();
    assert!(stage_zip(&not_zip, &game, TOP, EXE).is_err());
    assert_eq!(tree(&game).get(EXE).map(String::as_str), Some("old exe"));
}

#[cfg(unix)]
#[test]
fn a_read_only_folder_is_not_writable() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = tempfile::tempdir().unwrap();
    let game = dir.path().join("game");
    write(&game, &[(EXE, "x")]);
    std::fs::set_permissions(&game, std::fs::Permissions::from_mode(0o555)).unwrap();
    let probe = std::fs::write(game.join("root-probe"), b"");
    if probe.is_ok() {
        // Root writes anywhere: nothing to test.
        return;
    }
    assert!(check_writable(&game).is_err());
    std::fs::set_permissions(&game, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn an_earlier_swaps_running_program_does_not_block_the_next() {
    let dir = tempfile::tempdir().unwrap();
    let (game, plan) = staged(dir.path());
    // `old/` cannot be removed (here: a file; on Windows: the program
    // the last update replaced, still running).
    std::fs::write(game.join(WORK_DIR).join("old"), "running").unwrap();
    swap(&game, &plan, &mut |_| Ok(())).unwrap();
    assert_eq!(tree(&game), expected());
    assert_eq!(std::fs::read_to_string(game.join(WORK_DIR).join("old-1").join(EXE)).unwrap(), "old exe");
}

#[test]
fn a_program_that_cannot_move_in_is_put_back() {
    let dir = tempfile::tempdir().unwrap();
    let (game, plan) = staged(dir.path());
    let staged_exe = game.join(WORK_DIR).join("new").join(EXE);
    // The new program vanishes (a virus scanner) after the old moved aside.
    let e = swap(&game, &plan, &mut |s| {
        if s == format!("in {EXE}") {
            std::fs::remove_file(&staged_exe)?;
        }
        Ok(())
    })
    .unwrap_err();
    assert!(e.contains("moving in"), "{e}");
    assert_eq!(std::fs::read_to_string(game.join(EXE)).unwrap(), "old exe", "the folder keeps a program");
}

#[test]
fn one_game_at_a_time_updates_the_folder() {
    let dir = tempfile::tempdir().unwrap();
    let (game, _plan) = staged(dir.path());
    let held = claim(&game).unwrap().expect("free");
    assert!(claim(&game).unwrap().is_none(), "another copy's install waits");
    // Another copy starting leaves the staged files alone.
    assert_eq!(recover(&game, EXE), Recovery::Busy);
    assert!(game.join(WORK_DIR).join("new").join(EXE).exists());
    drop(held);
    assert_eq!(recover(&game, EXE), Recovery::Cleaned);
}
