//! Settings and profiles on disk (spec §9.4, §21 G2; plan 7 Review Focus 1):
//! versioned RON written atomically; a corrupt or out-of-range file gives
//! defaults (or the values fixed) and a warning, and the bad file is kept
//! as `.bak`, never lost and never a panic; a newer game's file is left in
//! place, untouched.

use std::fs;
use std::path::Path;

use olr_input::{Action, Bindings, KeyCode};
use olr_ui::{
    BotLevel, FrameRate, PROFILES_VERSION, Profile, Profiles, SETTINGS_VERSION, Secret, Settings, Store,
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
        weapons: vec!["Bazooka".into(), "Laser".into()],
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

    // A file from before the Classic HUD was dropped (plan 7 task 6)
    // still loads: its `hud` is ignored, the rest kept, no backup made.
    let old = format!(
        "(version: {SETTINGS_VERSION}, settings: (video: (fullscreen: true), \
         gameplay: (hud: Classic, match_prefs: (bots: 5))))"
    );
    fs::write(&path, old).unwrap();
    let loaded = store.load_settings();
    assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
    assert!(loaded.value.video.fullscreen);
    assert_eq!(loaded.value.gameplay.match_prefs.bots, 5);
    assert!(!dir.path().join("data/settings.ron.bak").exists());

    // A file from before the Import screen was removed (plan 13c) still
    // loads: its `import` block is ignored, the rest kept, no backup.
    let old = format!(
        "(version: {SETTINGS_VERSION}, settings: (import: (last_path: \"/x\"), \
         video: (fullscreen: true)))"
    );
    fs::write(&path, old).unwrap();
    let loaded = store.load_settings();
    assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
    assert!(loaded.value.video.fullscreen);
    assert!(!dir.path().join("data/settings.ron.bak").exists());

    // v0.1.0's window of the desktop's size is fullscreen now (the v0.1
    // playtest): it loads as fullscreen over the default window size, the
    // rest kept, no warning, no backup.
    let old = format!(
        "(version: {SETTINGS_VERSION}, settings: (video: (window: Desktop, ui_scale: 1.5), \
         audio: (master: 0.25)))"
    );
    fs::write(&path, old).unwrap();
    let loaded = store.load_settings();
    assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
    let v = &loaded.value.video;
    assert_eq!((v.window, v.fullscreen, v.ui_scale), (WindowPreset::W1280x720, true, 1.5));
    assert_eq!(loaded.value.audio.master, 0.25);
    assert!(!dir.path().join("data/settings.ron.bak").exists());
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

    // A newer game's file: defaults and a warning naming the version; it
    // stays where it is, untouched (`a_newer_file_is_never_overwritten`).
    let future = format!("(version: {}, settings: (), hologram: true)", SETTINGS_VERSION + 1);
    fs::write(&path, &future).unwrap();
    let got = store.load_settings();
    assert_eq!(got.value, Settings::default());
    assert!(got.warnings[0].contains(&format!("version {}", SETTINGS_VERSION + 1)), "{:?}", got.warnings);
    assert_eq!(read(&bak), garbage);
    assert_eq!(read(&path), future);
    assert!(!data.join("settings.ron.1.bak").exists());

    // Not UTF-8, empty, no version, too large: all defaults with a backup.
    let huge = vec![b' '; olr_ui::store::MAX_FILE_BYTES + 1];
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
    // The fixed values are written back: the next start reads them, with
    // every other setting kept and no warning (the file is never lost).
    let next = Store::open(&data).load_settings();
    assert!(next.warnings.is_empty(), "{:?}", next.warnings);
    assert_eq!(next.value, got.value);

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
    assert_eq!(got.value.list[1].name.chars().count(), olr_ui::MAX_NAME_CHARS);
    let next = Store::open(&data).load_profiles();
    assert!(next.warnings.is_empty(), "{:?}", next.warnings);
    assert_eq!(next.value, got.value, "both profiles kept, the name shortened");
    assert_eq!(next.value.list.len(), 2);

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

/// The backups are bounded: with every slot taken, the oldest (`.bak`) is
/// dropped and the others move down one, so a bad file is still moved
/// aside and saves keep working.
#[test]
fn backups_rotate_past_the_limit() {
    use olr_ui::store::MAX_BACKUPS;
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path();
    let slot = |n: usize| {
        if n == 0 { data.join("settings.ron.bak") } else { data.join(format!("settings.ron.{n}.bak")) }
    };
    for n in 0..MAX_BACKUPS {
        fs::write(slot(n), format!("old {n}")).unwrap();
    }
    let path = data.join("settings.ron");
    fs::write(&path, "corrupt").unwrap();
    let mut store = Store::open(data);
    let got = store.load_settings();
    assert_eq!(got.value, Settings::default());
    assert!(!path.exists(), "moved aside: {:?}", got.warnings);
    assert_eq!(read(&slot(0)), "old 1", "the oldest is dropped");
    assert_eq!(read(&slot(MAX_BACKUPS - 2)), format!("old {}", MAX_BACKUPS - 1));
    assert_eq!(read(&slot(MAX_BACKUPS - 1)), "corrupt");
    assert_eq!(fs::read_dir(data).unwrap().count(), MAX_BACKUPS);
    store.save_settings(&Settings::default()).unwrap();
    assert_eq!(store.load_settings().value, Settings::default());
}

/// A file from a newer version of the game stays where it is and is never
/// overwritten, even after the settings change and are saved: this
/// session uses defaults and saves nothing to it, and says so (a notice
/// the main menu shows). Read again later (the newer game gone, the file
/// replaced), the store saves as usual.
#[test]
fn a_newer_file_is_never_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path();
    let settings = data.join("settings.ron");
    let profiles = data.join("profiles.ron");
    let future_s = format!("(version: {}, settings: (video: (holo: 3)))", SETTINGS_VERSION + 1);
    let future_p = format!("(version: {}, profiles: (list: [], selected: 0))", PROFILES_VERSION + 7);
    fs::write(&settings, &future_s).unwrap();
    fs::write(&profiles, &future_p).unwrap();
    let mut store = Store::open(data);
    assert!(store.notices().is_empty());
    let got = store.load_settings();
    assert_eq!(got.value, Settings::default());
    assert_eq!(got.warnings.len(), 1, "{:?}", got.warnings);
    assert!(got.warnings[0].contains("newer"), "{}", got.warnings[0]);
    let gotp = store.load_profiles();
    assert_eq!(gotp.value, Profiles::default());
    assert_eq!(
        store.notices(),
        vec![
            "settings.ron is from a newer version; using defaults and leaving it untouched \
             (changes are not saved)"
                .to_string(),
            "profiles.ron is from a newer version; using defaults and leaving it untouched \
             (changes are not saved)"
                .to_string(),
        ]
    );
    // Changed and saved, again and again: nothing is written over them.
    for _ in 0..2 {
        store.save_settings(&changed_settings()).unwrap();
        store.save_profiles(&changed_profiles()).unwrap();
    }
    assert_eq!(read(&settings), future_s);
    assert_eq!(read(&profiles), future_p);
    let names: Vec<String> =
        fs::read_dir(data).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    assert_eq!(names.len(), 2, "no backup, no temporary file: {names:?}");
    // A later session finds them as they were.
    let mut next = Store::open(data);
    assert_eq!(next.load_settings().value, Settings::default());
    assert_eq!(next.notices().len(), 1);
    // The file replaced by a readable one: read and saved as usual.
    fs::remove_file(&settings).unwrap();
    store.save_settings(&changed_settings()).unwrap();
    assert_eq!(store.load_settings().value, changed_settings());
    store.save_settings(&Settings::default()).unwrap();
    assert_eq!(store.load_settings().value, Settings::default());
    assert_eq!(store.notices().len(), 1, "profiles.ron is still the newer one");
}

#[test]
fn identity_is_created_once_and_a_damaged_file_is_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let path = data.join("identity.ron");
    let mut store = Store::open(&data);

    let first = store.load_identity(|| [7; 32]);
    assert_eq!((first.value.0, first.warnings.is_empty()), ([7; 32], true));
    assert_eq!(
        read(&path).split_whitespace().collect::<String>(),
        format!("(version:1,secret:\"{}\",)", "07".repeat(32))
    );
    // The next load reads it; `fresh` is not asked.
    let again = store.load_identity(|| panic!("a second identity"));
    assert_eq!((again.value.0, again.warnings.is_empty()), ([7; 32], true));
    assert_eq!(Store::open(&data).load_identity(|| panic!("a second identity")).value.0, [7; 32]);

    // Damaged files (garbage, short, not hex, zero): new secret, one
    // warning, the old file moved to `.bak`, the new one saved.
    let zero = format!("(version: 1, secret: \"{}\")", "00".repeat(32));
    let not_hex = format!("(version: 1, secret: \"{}\")", "zz".repeat(32));
    for (n, bad) in
        ["not ron\u{0}", "(version: 1, secret: \"abcd\")", &zero, &not_hex].into_iter().enumerate()
    {
        fs::write(&path, bad).unwrap();
        let got = store.load_identity(|| [9; 32]);
        assert_eq!(got.value, Secret([9; 32]), "{bad}");
        assert_eq!(got.warnings.len(), 1, "{:?}", got.warnings);
        assert!(got.warnings[0].contains(".bak"), "{:?}", got.warnings);
        let bak = if n == 0 { "identity.ron.bak".to_string() } else { format!("identity.ron.{n}.bak") };
        assert_eq!(read(&data.join(bak)), bad);
        assert!(read(&path).contains(&"09".repeat(32)));
        assert_eq!(store.load_identity(|| panic!("again")).value.0, [9; 32]);
    }
}

#[test]
fn identity_without_a_data_directory_is_fresh_each_time() {
    let mut store = Store::in_memory();
    assert_eq!(store.load_identity(|| [3; 32]).value.0, [3; 32]);
}

#[cfg(unix)]
#[test]
fn identity_file_and_backup_are_private() {
    use std::os::unix::fs::PermissionsExt as _;
    let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let path = data.join("identity.ron");
    let mut store = Store::open(&data);
    store.load_identity(|| [7; 32]);
    assert_eq!(mode(&path), 0o600);
    // A damaged file made world-readable: its backup is made private.
    fs::write(&path, "garbage").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    store.load_identity(|| [9; 32]);
    assert_eq!(mode(&path), 0o600);
    assert_eq!(mode(&data.join("identity.ron.bak")), 0o600);
    // Debug never prints the secret.
    assert_eq!(format!("{:?}", Secret([9; 32])), "Secret(..)");
}

/// Plan 11: downloads go to the platform's cache directory's
/// `downloads/` (`<dir>/cache/downloads` for a data directory of one's
/// own, `--data-dir` and the tests); in memory there is none.
#[test]
fn the_download_cache_has_its_own_directory() {
    assert_eq!(Store::in_memory().cache_dir(), None);
    let tmp = tempfile::tempdir().unwrap();
    assert_eq!(Store::open(tmp.path()).cache_dir(), Some(tmp.path().join("cache").join("downloads")));
    let (platform, _) = Store::platform();
    if let (Some(data), Some(cache)) = (platform.dir(), platform.cache_dir()) {
        assert!(cache.ends_with("downloads"), "{}", cache.display());
        assert!(!cache.starts_with(data.join("packs")), "never among the imported packs");
    }
}
