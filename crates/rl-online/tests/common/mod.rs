//! Content and match starts for the tests: Classic on Dirt Level from
//! `RL_CONTENT_DIR` (skipped with a notice without it); partial copies of
//! it, a generated skin pack and download caches for the content tests.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rl_bots::BotConfig;
use rl_content::{Content, DEFAULT_PRESET, DownloadCache};
use rl_modes::DeathmatchSettings;
use rl_online::PeerContent;
use rl_online::matches::{bot_wire, rules_wire, setup_packs};
use rl_proto::{Look, MatchSetupWire, MatchStartWire, SlotWire};
use tempfile::TempDir;

pub const MOD: &str = "olx-classic";
pub const LEVEL: &str = "olx-level-dirt-level";

/// The packs a match of Classic on Dirt Level needs, and the default skin.
pub const MATCH_PACKS: [&str; 5] =
    ["olx-core", "olx-classic", "olx-theme-dirt", "olx-level-dirt-level", "olx-skin-default"];
/// The skin pack `with_test_skin` adds.
pub const TEST_SKIN: &str = "test-skin";

pub fn content_dir() -> PathBuf {
    PathBuf::from(std::env::var("RL_CONTENT_DIR").expect("RL_CONTENT_DIR (check `content()` first)"))
}

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

/// A content directory holding [`MATCH_PACKS`] from `RL_CONTENT_DIR` but
/// `leave_out`.
pub fn partial_content(leave_out: &[&str]) -> (TempDir, Content) {
    let src = content_dir();
    let dir = tempfile::tempdir().unwrap();
    for id in MATCH_PACKS.iter().filter(|id| !leave_out.contains(id)) {
        copy_dir(&src.join(id), &dir.path().join(id));
    }
    let content = Content::open(dir.path()).unwrap();
    (dir, content)
}

/// Adds the skin pack [`TEST_SKIN`] to the content directory `dir`: the
/// default skin under another id, one pixel changed (another hash).
pub fn with_test_skin(dir: &Path) -> Content {
    with_skin(dir, TEST_SKIN)
}

/// Adds a skin pack `id` to the content directory `dir` (in `skin-<id>`):
/// the default skin, one pixel changed (another hash).
pub fn with_skin(dir: &Path, id: &str) -> Content {
    let to = dir.join(format!("skin-{id}"));
    copy_dir(&content_dir().join("olx-skin-default"), &to);
    let manifest = std::fs::read_to_string(to.join("pack.ron")).unwrap();
    let manifest = manifest.replace("\"olx-skin-default\"", &format!("\"{id}\""));
    std::fs::write(to.join("pack.ron"), manifest).unwrap();
    let png = std::fs::read(to.join("skin.png")).unwrap();
    let img = rl_pack::image_io::decode_rgba(&png).unwrap();
    let mut pixels = img.as_raw().to_vec();
    pixels[0] ^= 0x55;
    pixels[3] = 255;
    let img = rl_data::image::RgbaImage::from_raw(img.width(), img.height(), pixels).unwrap();
    std::fs::write(to.join("skin.png"), rl_pack::image_io::encode_rgba(&img)).unwrap();
    Content::open(dir).unwrap()
}

/// A download cache of its own (in the returned directory).
pub fn cache() -> (TempDir, Arc<Mutex<DownloadCache>>) {
    let dir = tempfile::tempdir().unwrap();
    let cache = DownloadCache::open(dir.path()).unwrap();
    (dir, Arc::new(Mutex::new(cache)))
}

/// A peer's content configuration with `cache`.
pub fn peer_content(cache: &Arc<Mutex<DownloadCache>>) -> PeerContent {
    PeerContent { cache: Ok(Arc::clone(cache)), upload_rate: rl_online::content::HOST_RATE }
}

pub fn content() -> Option<Content> {
    let Some(dir) = std::env::var("RL_CONTENT_DIR").ok().filter(|d| std::path::Path::new(d).is_dir()) else {
        eprintln!("RL_CONTENT_DIR not set or missing (run `cargo xtask fetch-olx-content`); skipping");
        return None;
    };
    Some(Content::open(std::path::Path::new(&dir)).unwrap())
}

pub fn look(name: &str) -> Look {
    Look {
        name: name.into(),
        colour: [200, 80, 40],
        skin: "olx-skin-default".into(),
        skin_hash: rl_proto::NO_HASH.into(),
    }
}

/// Classic on Dirt Level, one bot, ending after `minutes`.
pub fn setup(content: &Content, minutes: f32) -> MatchSetupWire {
    MatchSetupWire {
        mod_id: MOD.into(),
        level_id: LEVEL.into(),
        packs: setup_packs(content, MOD, LEVEL).unwrap(),
        rules: rules_wire(&DeathmatchSettings { time_limit: minutes, ..DeathmatchSettings::default() }),
        bonuses: false,
        loading_time: 100,
        restrictions: DEFAULT_PRESET.into(),
        bots: vec![bot_wire(&BotConfig::default(), look("Bot"))],
    }
}

/// A start of `setup`: member 0 in slot 0, then every bot.
pub fn start(setup: MatchSetupWire, seed: u32) -> MatchStartWire {
    let mut slots = vec![SlotWire { member: Some(0), bot: None, look: look("Host"), loadout: None }];
    slots.extend(setup.bots.iter().enumerate().map(|(i, b)| SlotWire {
        member: None,
        bot: Some(i as u8),
        look: b.look.clone(),
        loadout: None,
    }));
    MatchStartWire { seed, setup, slots, input_delay: vec![(0, 0)], start_checksum: 0 }
}
