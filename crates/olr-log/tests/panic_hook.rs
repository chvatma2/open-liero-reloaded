//! The log's panic hook and the `log` facade (process-global state: a
//! test binary of its own, one test).

use olr_log::{Clock, LevelFilter, Options, Stamp};

struct Fixed;

impl Clock for Fixed {
    fn now(&self) -> Stamp {
        Stamp { unix_ms: 1_792_584_187_045, offset_secs: 0 }
    }
}

#[test]
fn panics_and_records_reach_the_file() {
    let tmp = tempfile::tempdir().unwrap();
    let options = Options {
        dir: Some(tmp.path().join("logs")),
        level: LevelFilter::Info,
        home: Some(tmp.path().to_path_buf()),
        header: vec!["olr 1.2.3".into(), "os: test".into()],
    };
    let path = olr_log::init(options, Box::new(Fixed)).expect("a run file");
    assert_eq!(olr_log::path().as_deref(), Some(path.as_path()));
    log::info!(target: "olr_game::shell", "content in {}", tmp.path().join("packs").display());
    log::debug!(target: "olr_game::shell", "not at info");
    log::info!(target: "wgpu_core", "another crate's info");
    log::warn!(target: "wgpu_core", "another crate's warning");
    let caught = std::panic::catch_unwind(|| panic!("boom\nsecond line"));
    assert!(caught.is_err());
    let text = std::fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines[0].ends_with("info  olr: olr 1.2.3"), "{text}");
    assert!(lines[1].ends_with("info  olr: os: test"), "{text}");
    assert!(text.contains("info  shell: content in ~/packs\n"), "{text}");
    assert!(!text.contains("not at info") && !text.contains("another crate's info"), "{text}");
    assert!(text.contains("warn  wgpu_core: another crate's warning"), "{text}");
    let at = format!("error panic: thread 'panics_and_records_reach_the_file' panicked at {}:", file!());
    let panic_line = lines.iter().position(|l| l.contains(&at)).unwrap_or_else(|| panic!("{text}"));
    assert!(lines[panic_line].ends_with(": boom"), "{text}");
    assert_eq!(lines[panic_line + 1], "    second line", "{text}");
    olr_log::end();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.trim_end().ends_with(olr_log::CLEAN_END), "{text}");
    log::warn!(target: "olr", "after the end: the terminal alone");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
}
