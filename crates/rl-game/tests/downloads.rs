//! Content downloads in the game (plan 11 task 7): a joiner lacking the
//! level downloads it in the lobby into its cache (`Store::cache_dir`) and
//! plays the match from it; a replay of a level only the cache holds plays
//! back from the menus; the options clear the cache. Classic on Dirt Level
//! from `RL_CONTENT_DIR`; skipped with a notice without it.

use std::path::Path;
use std::time::Duration;

use rl_audio::Audio;
use rl_content::{Content, DownloadCache, MatchOptions, Player};
use rl_data::manifest::PackKind;
use rl_game::session::{Driver, Session};
use rl_game::setup;
use rl_game::shell::{Shell, ShellConfig};
use rl_online::content::PackArchive;
use rl_scene::Interpolation;
use rl_sim::Keys;
use rl_ui::{Profiles, Screen, Settings, Store, UiAction, UiState};
use tempfile::TempDir;

use crate::online::{
    LEVEL, MOD, confirmed, content_dir, frames_until, host, join, members, menus, start_match, viewport,
};

/// The packs a match of Classic on Dirt Level needs, and the default skin.
const MATCH_PACKS: [&str; 5] =
    ["olx-core", "olx-classic", "olx-theme-dirt", "olx-level-dirt-level", "olx-skin-default"];

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let path = e.path();
        if path.is_dir() {
            copy_dir(&path, &to.join(e.file_name()));
        } else {
            std::fs::copy(&path, to.join(e.file_name())).unwrap();
        }
    }
}

/// A content directory with [`MATCH_PACKS`] from `dir` but `leave_out`.
fn partial_content(dir: &Path, leave_out: &[&str]) -> TempDir {
    let to = tempfile::tempdir().unwrap();
    for id in MATCH_PACKS.iter().filter(|id| !leave_out.contains(id)) {
        copy_dir(&dir.join(id), &to.path().join(id));
    }
    to
}

/// Installs pack `id` of `content` in the download cache in `cache_dir`
/// as a download would (its archive, verified); returns its content hash.
fn install(cache_dir: &Path, content: &Content, id: &str, kind: PackKind) -> String {
    let hash = content.pack_hash(id, kind).unwrap();
    let src = rl_vfs::DirFs::new(content.pack_dir(id).unwrap());
    let archive = PackArchive::new(rl_pack::archive::encode(&src).unwrap());
    let mut cache = DownloadCache::open(cache_dir).unwrap();
    let size = archive.bytes.len() as u64;
    cache.append(&hash, &archive.sha, size, 0, &archive.bytes).unwrap();
    cache.install(&hash, id, kind, rl_content::archive_limits(kind)).unwrap();
    hash
}

/// The packs in the download cache in `cache_dir` (its `packs/`).
fn cached(cache_dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(cache_dir.join("packs"))
        .map(|d| d.filter_map(Result::ok).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    names.sort();
    names
}

/// A client whose content lacks the level joins: the lobby lists the
/// download (a "level" row, then "done"), the level lands in the client's
/// cache (`Store::cache_dir`), it is ready, plays the match (the same
/// start world: no refusal) and its match's assets load the level from
/// the cache (`setup::load_online` from the peer's content).
#[test]
fn a_joiner_without_the_level_downloads_and_plays() {
    let Some(dir) = content_dir() else { return };
    let partial = partial_content(&dir, &[LEVEL]);
    let data = tempfile::tempdir().unwrap();
    let cache_dir = Store::open(data.path()).cache_dir().unwrap();
    let mut h = menus(&dir, Store::in_memory(), "Hosty", 0.04);
    host(&mut h);
    let port = h.local_port().expect("the host's port");
    let mut c = menus(partial.path(), Store::open(data.path()), "Anna", 0.04);
    join(&mut c, port);
    frames_until("the client joins", Duration::from_secs(5), &mut [&mut h, &mut c], |s| {
        s[1].ui.screen() == Screen::Lobby && members(s[0]) == 2
    });
    let mut states = Vec::new();
    frames_until("the level downloads", Duration::from_secs(10), &mut [&mut h, &mut c], |s| {
        let Some(l) = &s[1].data.lobby else { return false };
        states.extend(l.downloads.iter().map(|d| (d.name.clone(), d.state.clone())));
        l.downloads.iter().any(|d| d.name == LEVEL && d.state == "done")
    });
    let lobby = c.data.lobby.clone().unwrap();
    let row = lobby.downloads.iter().find(|d| d.name == LEVEL).unwrap();
    assert_eq!(row.kind, "level");
    assert!(row.size > 0 && row.have == row.size, "{row:?}");
    assert!(lobby.prompt.is_none(), "a level is far below the prompt");
    assert!(states.iter().all(|(n, _)| n == LEVEL), "only the level is missing: {states:?}");
    assert_eq!(cached(&cache_dir).len(), 1, "the level is in the client's cache");
    frames_until("the client lacks nothing", Duration::from_secs(5), &mut [&mut h, &mut c], |s| {
        let me = |sh: &Shell, name: &str| {
            sh.data.lobby.as_ref().and_then(|l| l.members.iter().find(|m| m.name == name).cloned())
        };
        me(s[0], "Anna").is_some_and(|m| m.missing.is_empty())
            && me(s[1], "Anna").is_some_and(|m| m.missing.is_empty())
    });

    start_match(&mut h, &mut c);
    let level_dir = c.play.as_ref().unwrap().assets.content.pack_dir(LEVEL).unwrap().to_path_buf();
    assert!(level_dir.starts_with(&cache_dir), "the match's level is the download: {}", level_dir.display());
    frames_until("both confirm the match", Duration::from_secs(10), &mut [&mut h, &mut c], |s| {
        s.iter().all(|sh| confirmed(sh).is_some_and(|(t, _)| t > 50))
    });
    assert!(c.plays_online() && c.session().unwrap().local() == Some(1), "the client plays its slot");
}

/// Content over the prompt waits: the lobby asks "This game needs N MB"
/// with the download queued; Download lets it come (the prompt goes), and
/// it completes.
#[test]
fn big_downloads_wait_for_the_prompt() {
    let Some(dir) = content_dir() else { return };
    let partial = partial_content(&dir, &[LEVEL]);
    let data = tempfile::tempdir().unwrap();
    let mut h = menus(&dir, Store::in_memory(), "Hosty", 1.0);
    host(&mut h);
    let port = h.local_port().expect("the host's port");
    let mut c = menus(partial.path(), Store::open(data.path()), "Anna", 1.0);
    join(&mut c, port);
    c.with_peer(|p| p.test_prompt_limit(1)).expect("joining");
    frames_until("the prompt shows", Duration::from_secs(5), &mut [&mut h, &mut c], |s| {
        s[1].data.lobby.as_ref().is_some_and(|l| l.prompt.is_some())
    });
    let lobby = c.data.lobby.clone().unwrap();
    let row = lobby.downloads.iter().find(|d| d.name == LEVEL).expect("the level's row");
    assert_eq!(lobby.prompt, Some(row.size), "the level's archive is what is missing");
    let started = std::time::Instant::now();
    frames_until("a while", Duration::from_secs(2), &mut [&mut h, &mut c], |_| {
        started.elapsed() > Duration::from_millis(500)
    });
    let lobby = c.data.lobby.clone().unwrap();
    assert!(lobby.prompt.is_some());
    assert!(
        lobby.downloads.iter().all(|d| d.state == "queued"),
        "nothing comes unasked: {:?}",
        lobby.downloads
    );
    c.act(UiAction::LobbyDownload(true));
    assert_eq!(c.data.lobby.as_ref().unwrap().prompt, None, "the prompt goes at once");
    frames_until("the level downloads", Duration::from_secs(10), &mut [&mut h, &mut c], |s| {
        s[1].data
            .lobby
            .as_ref()
            .is_some_and(|l| l.downloads.iter().any(|d| d.name == LEVEL && d.state == "done"))
    });
    assert_eq!(c.data.lobby.as_ref().unwrap().prompt, None);
}

/// A replay of a level this install has only as a download (in the cache
/// of `Store::cache_dir`) plays back from the Replays screen, every
/// checksum matching; without the download it is refused, naming the level.
#[test]
fn a_replay_of_a_downloaded_level_plays_back() {
    let Some(dir) = content_dir() else { return };
    // A replay recorded with the whole content.
    let o = MatchOptions::deathmatch(MOD, LEVEL, 7, vec![Player::Human, Player::Bot(Default::default())]);
    let a = setup::load_match(&[dir.as_path()], &o, "default").unwrap();
    let mut s = Session::new(&a, viewport(), Interpolation::Linear, Audio::silent()).unwrap();
    for t in 0..300 {
        s.tick(crate::online::player_keys(t));
    }
    let end = s.world.checksum();
    let replay = s.take_replay().expect("a Deathmatch records");

    let partial = partial_content(&dir, &[LEVEL]);
    let shell = |data: &Path| {
        let ui = UiState::new(Settings::default(), Profiles::default(), 1);
        let cfg = ShellConfig {
            content: partial.path().to_path_buf(),
            interpolation: Interpolation::Linear,
            viewport: viewport(),
        };
        Shell::new(cfg, ui, Store::open(data), Audio::silent())
    };
    let play = |sh: &mut Shell, file: &Path| {
        sh.act(UiAction::Open(Screen::Replays));
        sh.act(UiAction::PlayReplay(file.to_path_buf()));
        sh.poll();
    };

    // Not downloaded: refused, naming the level.
    let bare = tempfile::tempdir().unwrap();
    let file = bare.path().join("match.rlrp");
    std::fs::write(&file, replay.encode()).unwrap();
    let mut sh = shell(bare.path());
    play(&mut sh, &file);
    assert_eq!(sh.ui.screen(), Screen::Replays);
    assert!(sh.ui.message().is_some_and(|m| m.contains(LEVEL)), "{:?}", sh.ui.message());

    // Downloaded: it plays back.
    let data = tempfile::tempdir().unwrap();
    let full = Content::open(&dir).unwrap();
    install(&Store::open(data.path()).cache_dir().unwrap(), &full, LEVEL, PackKind::Level);
    let mut sh = shell(data.path());
    play(&mut sh, &file);
    assert_eq!(sh.ui.screen(), Screen::InGame, "{:?}", sh.ui.message());
    for _ in 0..350 {
        sh.tick(Keys(0));
    }
    let s = sh.session().unwrap();
    let Driver::Replay(d) = &s.driver else { panic!("a replay") };
    assert!(d.is_done());
    assert_eq!(d.mismatch, None);
    assert_eq!(d.cursor.checked(), replay.checksums.len(), "every checksum checked");
    assert_eq!(s.world.checksum(), end, "the same world");

    // Downloaded by a game that ran on `downloads-2` (another game held
    // `downloads` then): found there too, while this shell holds `downloads`.
    let data = tempfile::tempdir().unwrap();
    let second = Store::open(data.path()).cache_dir().unwrap().with_file_name("downloads-2");
    install(&second, &full, LEVEL, PackKind::Level);
    let mut sh = shell(data.path());
    play(&mut sh, &file);
    assert_eq!(sh.ui.screen(), Screen::InGame, "{:?}", sh.ui.message());
}

/// A command-line replay (`--replay`, `setup::load_replay` with
/// `net::cache_dirs`) finds a downloaded level while a running game holds
/// that cache, and creates, locks and writes no cache itself.
#[test]
fn a_command_line_replay_reads_the_caches_without_opening_them() {
    let Some(dir) = content_dir() else { return };
    let o = MatchOptions::deathmatch(MOD, LEVEL, 3, vec![Player::Human, Player::Bot(Default::default())]);
    let a = setup::load_match(&[dir.as_path()], &o, "default").unwrap();
    let mut s = Session::new(&a, viewport(), Interpolation::Linear, Audio::silent()).unwrap();
    for t in 0..100 {
        s.tick(crate::online::player_keys(t));
    }
    let replay = s.take_replay().expect("a Deathmatch records");
    let partial = partial_content(&dir, &[LEVEL]);
    let file = partial.path().join("match.rlrp");
    std::fs::write(&file, replay.encode()).unwrap();

    let data = tempfile::tempdir().unwrap();
    let base = Store::open(data.path()).cache_dir().unwrap();
    let dirs = rl_game::net::cache_dirs(Some(base.clone()));
    let roots = [partial.path()];
    // No cache at all: refused, naming the level, and none made.
    let e = setup::load_replay(&roots, &file, "default", &dirs).err().expect("refused");
    assert!(format!("{e:#}").contains(LEVEL), "{e:#}");
    assert!(!base.parent().unwrap().exists(), "no cache directory is made");
    // The level downloaded; a game holds the cache while the replay loads.
    install(&base, &Content::open(&dir).unwrap(), LEVEL, PackKind::Level);
    let index = std::fs::read(base.join("index.ron")).unwrap();
    let game = DownloadCache::open(&base).unwrap();
    let a = setup::load_replay(&roots, &file, "default", &dirs).unwrap();
    assert!(a.content.pack_dir(LEVEL).unwrap().starts_with(&base), "the level from the cache");
    drop(game);
    assert_eq!(std::fs::read(base.join("index.ron")).unwrap(), index, "the index is not written");
    let made: Vec<_> =
        std::fs::read_dir(base.parent().unwrap()).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(made, ["downloads"], "no other cache is made");
}

/// The options show the download cache's size and Clear empties it.
/// While an online game is on (the options opened from a match's menu)
/// Clear is disabled with the reason, and the command is refused.
#[test]
fn the_shell_clears_the_download_cache() {
    let Some(dir) = content_dir() else { return };
    let data = tempfile::tempdir().unwrap();
    let cache_dir = Store::open(data.path()).cache_dir().unwrap();
    let full = Content::open(&dir).unwrap();
    install(&cache_dir, &full, LEVEL, PackKind::Level);
    let mut sh = menus(&dir, Store::open(data.path()), "Anna", 1.0);

    // Online: a match of the host and a bot, its menu, the options.
    host(&mut sh);
    frames_until("the lobby shows", Duration::from_secs(3), &mut [&mut sh], |s| s[0].data.lobby.is_some());
    let mut setup = sh.data.lobby.as_ref().unwrap().setup.clone();
    setup.bots = 1;
    sh.act(UiAction::LobbySetup(setup));
    sh.act(UiAction::LobbyReady(true));
    frames_until("the host can start", Duration::from_secs(5), &mut [&mut sh], |s| {
        s[0].data.lobby.as_ref().is_some_and(|l| l.can_start.is_ok())
    });
    sh.act(UiAction::LobbyStart);
    frames_until("the match plays", Duration::from_secs(5), &mut [&mut sh], |s| s[0].plays_online());
    sh.act(UiAction::Back);
    sh.act(UiAction::Open(Screen::Options));
    assert_eq!(sh.ui.screen(), Screen::Options);
    sh.poll();
    let view = sh.data.download_cache.clone().expect("a download cache");
    assert!(view.bytes > 0, "{view:?}");
    assert!(view.clear.is_err(), "no Clear while online: {view:?}");
    sh.act(UiAction::ClearDownloadCache);
    assert_eq!(cached(&cache_dir).len(), 1, "refused while online");

    // Back in the menus: Clear empties it.
    sh.act(UiAction::Back);
    sh.act(UiAction::LeaveMatch);
    assert!(!sh.online_active());
    sh.act(UiAction::Back);
    sh.act(UiAction::Open(Screen::Options));
    sh.poll();
    assert!(sh.data.download_cache.clone().unwrap().clear.is_ok());
    sh.act(UiAction::ClearDownloadCache);
    assert!(cached(&cache_dir).is_empty(), "cleared: {:?}", cached(&cache_dir));
    sh.poll();
    assert_eq!(sh.data.download_cache.clone().unwrap().bytes, 0);
}

/// Review Minor 4: the Options screen reads the cache's sizes (a directory
/// listing and a file per partial) at most once a second, not every frame.
#[test]
fn the_options_read_the_cache_at_most_once_a_second() {
    let Some(dir) = content_dir() else { return };
    let data = tempfile::tempdir().unwrap();
    let mut sh = menus(&dir, Store::open(data.path()), "Anna", 1.0);
    sh.act(UiAction::Open(Screen::Options));
    let start = std::time::Instant::now();
    sh.poll();
    let shown = |sh: &Shell| sh.data.download_cache.clone().expect("a download cache").bytes;
    let before = shown(&sh);
    // A partial download appears.
    let cache = sh.download_cache().clone().expect("a download cache");
    cache.lock().unwrap().append(&"a".repeat(64), &"f".repeat(64), 2000, 0, &[0u8; 1000]).unwrap();
    let charge = rl_pack::archive::file_charge(1000) + rl_pack::archive::BLOCK;
    for _ in 0..10 {
        sh.poll();
    }
    if start.elapsed() < Duration::from_millis(900) {
        assert_eq!(shown(&sh), before, "not read again within the second");
    }
    std::thread::sleep(Duration::from_millis(1100));
    sh.poll();
    assert_eq!(shown(&sh), before + charge, "read again after it");
}
