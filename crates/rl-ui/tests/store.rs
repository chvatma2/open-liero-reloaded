//! Settings and profiles on disk (spec §9.4, §21 G2; plan 7 Review Focus 1):
//! versioned RON written atomically; a corrupt, future-version or
//! out-of-range file gives defaults (or the values fixed) and a warning,
//! and the bad file is kept as `.bak`, never lost and never a panic.

use std::fs;
use std::path::Path;

use rl_input::{Action, Bindings, KeyCode};
use rl_ui::{
    BotLevel, FrameRate, HudStyle, PROFILES_VERSION, Profile, Profiles, SETTINGS_VERSION, Settings, Store,
    UpscaleFilter, WindowPreset,
};

fn changed_settings() -> Settings {
    let mut s = Settings::default();
    s.video.window = WindowPreset::W1920x1080;
    s.video.fullscreen = true;
    s.video.frame_rate = FrameRate::Uncapped;
    s.video.filter = UpscaleFilter::Nearest;
    s.video.interpolation = false;
    s.video.ui_scale = 1.25;
    s.audio.master = 0.5;
    s.audio.sfx = 0.75;
    s.audio.music = 0.1;
    s.input.keys.push((Action::Jump, KeyCode::Space));
    s.gameplay.hud = HudStyle::Classic;
    s.gameplay.match_prefs.bots = 7;
    s.gameplay.match_prefs.difficulty = BotLevel::Xtreme;
    s.gameplay.match_prefs.rules.kill_limit = 30;
    s.gameplay.match_prefs.rules.time_limit = 2.5;
    s.gameplay.match_prefs.level_id = "olx-level-castlestrike".into();
    s
}

fn changed_profiles() -> Profiles {
    let mut p = Profiles::default();
    p.list.push(Profile {
        name: "Second".into(),
        skin_id: "olx-skin-zombie".into(),
        colour: [20, 90, 255],
        bindings: Some(Bindings::olx()),
    });
    p.selected = 1;
    p
}

fn read(p: &Path) -> String {
    fs::read_to_string(p).unwrap()
}

#[test]
fn settings_round_trip_and_version() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path().join("data"));
    // A first start: no directory, no files, defaults and no warning.
    let first = store.load_settings();
    assert_eq!(first.value, Settings::default());
    assert!(first.warnings.is_empty(), "{:?}", first.warnings);
    assert_eq!(store.load_profiles().value, Profiles::default());

    let s = changed_settings();
    store.save_settings(&s).unwrap();
    let p = changed_profiles();
    store.save_profiles(&p).unwrap();
    let path = dir.path().join("data/settings.ron");
    let text = read(&path);
    assert!(text.contains(&format!("version: {SETTINGS_VERSION}")), "{text}");
    assert!(read(&dir.path().join("data/profiles.ron")).contains(&format!("version: {PROFILES_VERSION}")));
    // Atomic writes leave no temporary file behind.
    let names: Vec<_> =
        fs::read_dir(dir.path().join("data")).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(names.len(), 2, "{names:?}");

    let mut again = Store::open(dir.path().join("data"));
    let loaded = again.load_settings();
    assert_eq!(loaded.value, s);
    assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
    assert_eq!(again.load_profiles().value, p);
    // Saving again writes the same bytes (deterministic output).
    store.save_settings(&loaded.value).unwrap();
    assert_eq!(read(&path), text);

    // A file of this version that leaves fields out gets their defaults.
    fs::write(&path, format!("(version: {SETTINGS_VERSION}, settings: (video: (fullscreen: true)))"))
        .unwrap();
    let partial = store.load_settings();
    assert!(partial.value.video.fullscreen);
    assert_eq!(partial.value.audio, Settings::default().audio);
    assert!(partial.warnings.is_empty(), "{:?}", partial.warnings);
}

#[test]
fn bad_user_files_fall_back_to_defaults_and_keep_a_backup() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let mut store = Store::open(&data);
    store.save_settings(&changed_settings()).unwrap();
    let path = data.join("settings.ron");
    let bak = data.join("settings.ron.bak");

    // Corrupt: defaults, a warning, the file moved to `.bak` untouched.
    let garbage = "(version: 1, settings: (video: (window: Bogus\u{0}";
    fs::write(&path, garbage).unwrap();
    let got = store.load_settings();
    assert_eq!(got.value, Settings::default());
    assert_eq!(got.warnings.len(), 1, "{:?}", got.warnings);
    assert!(got.warnings[0].contains("settings.ron.bak"), "{}", got.warnings[0]);
    assert_eq!(read(&bak), garbage);
    assert!(!path.exists());
    // The next save writes a good file and leaves the backup alone.
    store.save_settings(&Settings::default()).unwrap();
    assert_eq!(read(&bak), garbage);

    // A newer game's file: defaults, a warning naming the version, and a
    // second backup (the first is kept).
    let future = format!("(version: {}, settings: (), hologram: true)", SETTINGS_VERSION + 1);
    fs::write(&path, &future).unwrap();
    let got = store.load_settings();
    assert_eq!(got.value, Settings::default());
    assert!(got.warnings[0].contains(&format!("version {}", SETTINGS_VERSION + 1)), "{:?}", got.warnings);
    assert_eq!(read(&bak), garbage);
    assert_eq!(read(&data.join("settings.ron.1.bak")), future);

    // Not UTF-8, empty, no version, too large: all defaults with a backup.
    let huge = vec![b' '; rl_ui::store::MAX_FILE_BYTES + 1];
    for bytes in [&[0xff, 0xfe, 0x00][..], b"", b"(settings: ())", &huge] {
        fs::write(&path, bytes).unwrap();
        let got = store.load_settings();
        assert_eq!(got.value, Settings::default());
        assert_eq!(got.warnings.len(), 1, "{:?}", got.warnings);
        assert!(!path.exists());
    }

    // Values out of range are fixed one by one (the rest is kept), with a
    // warning each, and the original is kept as a backup too.
    let mut odd = changed_settings();
    odd.video.ui_scale = 40.0;
    odd.audio.master = f32::NAN;
    odd.gameplay.match_prefs.bots = 0;
    odd.gameplay.match_prefs.rules.time_limit = f32::INFINITY;
    store.save_settings(&odd).unwrap();
    let saved = read(&path);
    let got = store.load_settings();
    assert_eq!(got.warnings.len(), 4, "{:?}", got.warnings);
    assert_eq!(got.value.video.ui_scale, 3.0);
    assert_eq!(got.value.audio.master, Settings::default().audio.master);
    assert_eq!(got.value.gameplay.match_prefs.bots, 1);
    assert_eq!(got.value.gameplay.match_prefs.rules.time_limit, 6.0);
    assert_eq!(got.value.gameplay.match_prefs.difficulty, BotLevel::Xtreme, "the rest is kept");
    assert!(fs::read_dir(&data).unwrap().any(|e| read(&e.unwrap().path()) == saved), "a backup");

    // Profiles: corrupt gives the default profile; an empty list or a
    // selection past its end is fixed.
    let ppath = data.join("profiles.ron");
    fs::write(&ppath, "((((").unwrap();
    let got = store.load_profiles();
    assert_eq!(got.value, Profiles::default());
    assert_eq!(read(&data.join("profiles.ron.bak")), "((((");
    fs::write(&ppath, format!("(version: {PROFILES_VERSION}, profiles: (list: [], selected: 9))")).unwrap();
    let got = store.load_profiles();
    assert_eq!(got.value.list.len(), 1);
    assert_eq!(got.value.selected, 0);
    assert!(!got.warnings.is_empty());
    let long = "x".repeat(300);
    let mut p = changed_profiles();
    p.list[1].name = long;
    store.save_profiles(&p).unwrap();
    let got = store.load_profiles();
    assert_eq!(got.value.list[1].name.chars().count(), rl_ui::MAX_NAME_CHARS);

    // A data directory that cannot be made (a file is in the way): loads
    // give defaults with no panic, saves are errors, never panics.
    fs::write(dir.path().join("blocked"), "a file").unwrap();
    let mut blocked = Store::open(dir.path().join("blocked/sub"));
    assert_eq!(blocked.load_settings().value, Settings::default());
    assert!(blocked.save_settings(&Settings::default()).is_err());
    // No data directory at all (no home): defaults, saves do nothing.
    let mut none = Store::in_memory();
    assert_eq!(none.load_profiles().value, Profiles::default());
    assert!(none.save_profiles(&Profiles::default()).is_ok());
}

/// A file that cannot be moved aside is never overwritten: the store
/// refuses to save over it until it is gone.
#[cfg(unix)]
#[test]
fn a_bad_file_that_cannot_be_backed_up_is_never_overwritten() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    fs::create_dir_all(&data).unwrap();
    let path = data.join("settings.ron");
    fs::write(&path, "corrupt").unwrap();
    // A read-only directory: the file cannot be renamed (root ignores it).
    fs::set_permissions(&data, fs::Permissions::from_mode(0o555)).unwrap();
    if fs::write(data.join("probe"), "").is_ok() {
        fs::set_permissions(&data, fs::Permissions::from_mode(0o755)).unwrap();
        return;
    }
    let mut store = Store::open(&data);
    let got = store.load_settings();
    assert_eq!(got.value, Settings::default());
    assert!(got.warnings[0].contains("kept"), "{:?}", got.warnings);
    fs::set_permissions(&data, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(store.save_settings(&Settings::default()).is_err());
    assert_eq!(read(&path), "corrupt");
}
