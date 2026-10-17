//! Pins a digest of the runtime data the loaders hand to the simulation and
//! the renderer for every bundled pack (plan 13c Task 1), so the pack format
//! switch can be shown to leave it byte-identical. One line per pack, by
//! kind then id: its SHA-256 over what `olr_content::Content` loads, with
//! today's load-time colour keying (`RgbaImage::key_transparent`, once per
//! image, as `olr-scene` and `olr-fx` apply it) on mod and core images.

use std::collections::BTreeSet;
use std::fmt::Write;
use std::path::Path;

use olr_content::{Content, placeholder_image};
use olr_data::image::{Rgba, RgbaImage};
use olr_data::manifest::PackKind;
use olr_data::mod_def::ProjectileGfx;
use olr_vfs::DirFs;
use sha2::{Digest, Sha256};

const COLOURS: [(u8, u8, u8); 3] = [(255, 0, 0), (96, 128, 252), (10, 200, 30)];

/// The bundled content (`OLR_CONTENT_DIR`), or `None` with a notice.
fn content_dir() -> Option<String> {
    let dir = std::env::var("OLR_CONTENT_DIR").ok()?;
    if Path::new(&dir).is_dir() {
        Some(dir)
    } else {
        eprintln!("skip: {dir} not found (run `cargo xtask fetch-olx-content`)");
        None
    }
}

fn keyed(img: &RgbaImage) -> RgbaImage {
    let mut img = img.clone();
    img.key_transparent();
    img
}

/// A tagged, length-prefixed hash input, so no two fields run together.
struct Digest256(Sha256);

impl Digest256 {
    fn new() -> Self {
        Digest256(Sha256::new())
    }
    fn bytes(&mut self, tag: &str, b: &[u8]) {
        self.0.update((tag.len() as u64).to_le_bytes());
        self.0.update(tag.as_bytes());
        self.0.update((b.len() as u64).to_le_bytes());
        self.0.update(b);
    }
    fn text(&mut self, tag: &str, s: &str) {
        self.bytes(tag, s.as_bytes());
    }
    fn image(&mut self, tag: &str, img: &RgbaImage) {
        self.text(tag, &format!("{}x{}", img.width(), img.height()));
        self.bytes(tag, img.as_raw());
    }
    fn hex(self) -> String {
        self.0.finalize().iter().fold(String::new(), |mut s, b| {
            write!(s, "{b:02x}").unwrap();
            s
        })
    }
}

fn mod_digest(content: &Content, id: &str) -> String {
    let mut d = Digest256::new();
    let sim = content.sim_mod(id).unwrap();
    d.text("sim", &format!("{sim:?}"));
    let def = olr_pack::read_mod(&DirFs::new(content.pack_dir(id).unwrap())).unwrap().def;
    d.text("def", &ron::ser::to_string(&def).unwrap());
    let loaded = content.load_mod(id).unwrap();
    // Exactly the images the renderer uses: `SceneAssets::new` keys each
    // projectile image file once (a missing one is a 4x4 placeholder); files no
    // projectile references never reach it.
    let files: BTreeSet<&str> = def
        .projectiles
        .values()
        .filter_map(|p| match &p.gfx {
            ProjectileGfx::Image { file, .. } => Some(file.as_str()),
            _ => None,
        })
        .collect();
    for file in files {
        let img = loaded.images.get(file).cloned().unwrap_or_else(|| placeholder_image(4, 4));
        d.image(&format!("image:{file}"), &keyed(&img));
    }
    for (name, bytes) in &loaded.sounds {
        d.bytes(&format!("sound:{name}"), bytes);
    }
    d.hex()
}

fn level_digest(content: &Content, id: &str) -> String {
    let mut d = Digest256::new();
    let l = content.load_level(id).unwrap();
    let cells: Vec<u8> = l.material.cells().iter().map(|m| *m as u8).collect();
    d.text("name", &l.name);
    d.text("art_scale", &l.art_scale.to_string());
    d.text("theme", &l.theme);
    for (i, sp) in l.spawns.iter().enumerate() {
        d.text(&format!("spawn:{i}"), &format!("{sp:?}"));
    }
    d.text("size", &format!("{}x{}", l.material.width(), l.material.height()));
    d.bytes("material", &cells);
    d.image("front", &l.front);
    d.image("back", &l.back);
    d.hex()
}

fn theme_digest(content: &Content, id: &str) -> String {
    let mut d = Digest256::new();
    let t = content.load_theme(id).unwrap();
    d.text("name", &t.name);
    d.text("holes", &t.holes.len().to_string());
    for (i, h) in t.holes.iter().enumerate() {
        d.image(&format!("hole:{i}"), h);
    }
    match &t.green_ball {
        Some(g) => d.image("green_ball", g),
        None => d.text("green_ball", "none"),
    }
    d.image("front_tile", &t.front_tile);
    d.image("back_tile", &t.back_tile);
    d.hex()
}

/// Hashes the frames as the game cuts them: the sheet's spacing columns are
/// never drawn, and format 5 drops them.
fn skin_digest(content: &Content, id: &str) -> String {
    let mut d = Digest256::new();
    let s = content.load_skin(id).unwrap();
    d.text("def", &format!("{:?}", s.skin.def));
    for (r, g, b) in COLOURS {
        let sheet = s.colorized(Rgba::rgb(r, g, b));
        for (face, name) in [(olr_sim::Dir::Right, "right"), (olr_sim::Dir::Left, "left")] {
            for f in 0..olr_content::SHEET_FRAMES {
                d.image(&format!("frame:{r},{g},{b}:{name}:{f}"), &olr_content::skin_sprite(&sheet, f, face));
            }
        }
    }
    d.hex()
}

fn core_digest(content: &Content, id: &str) -> String {
    let mut d = Digest256::new();
    let core = content.core();
    let def = olr_pack::read_core(&DirFs::new(content.pack_dir(id).unwrap())).unwrap().def;
    for name in &def.sprites {
        d.image(&format!("sprite:{name}"), &keyed(core.sprite(name)));
    }
    for name in &def.sounds {
        match core.sound(name) {
            Some(b) => d.bytes(&format!("sound:{name}"), b),
            None => d.text(&format!("sound:{name}"), "none"),
        }
    }
    // `colours` is a BTreeMap: key order.
    for key in def.colours.keys() {
        d.text(&format!("colour:{key}"), &format!("{:?}", core.colour(key)));
    }
    d.hex()
}

#[test]
fn bundled_content_digest() {
    let Some(dir) = content_dir() else { return };
    let content = Content::open(Path::new(&dir)).unwrap();
    let mut out = String::new();
    let mut counts = [0usize; 5];
    for (i, (kind, label)) in [
        (PackKind::Mod, "mod"),
        (PackKind::Level, "level"),
        (PackKind::Theme, "theme"),
        (PackKind::Skin, "skin"),
        (PackKind::Core, "core"),
    ]
    .into_iter()
    .enumerate()
    {
        let mut ids = content.ids(kind);
        ids.sort();
        counts[i] = ids.len();
        for id in ids {
            let hash = match kind {
                PackKind::Mod => mod_digest(&content, &id),
                PackKind::Level => level_digest(&content, &id),
                PackKind::Theme => theme_digest(&content, &id),
                PackKind::Skin => skin_digest(&content, &id),
                PackKind::Core => core_digest(&content, &id),
            };
            writeln!(out, "{label} {id} {hash}").unwrap();
        }
    }
    assert!(counts[0] >= 2, "mods: {}", counts[0]);
    assert!(counts[1] >= 4, "levels: {}", counts[1]);
    assert!(counts[2] >= 1, "themes: {}", counts[2]);
    assert!(counts[3] >= 700, "skins: {}", counts[3]);
    assert!(counts[4] >= 1, "core packs: {}", counts[4]);
    insta::assert_snapshot!(out);
}

/// Dropping the keying at load (plan 13c Task 8) keeps the gibs and their
/// stains: keying before or after colorizing gives the same pixels.
#[test]
fn the_colorized_gibs_do_not_depend_on_when_they_are_keyed() {
    let Some(dir) = content_dir() else { return };
    let content = Content::open(Path::new(&dir)).unwrap();
    let raw = content.core().sprite("giblets");
    assert!(content.core().has_sprite("giblets"));
    for (r, g, b) in COLOURS {
        let c = Rgba::rgb(r, g, b);
        let early = keyed(&olr_fx::colorize_gibs(raw, c));
        let late = olr_fx::colorize_gibs(&keyed(raw), c);
        assert_eq!(early, late, "gibs differ for colour ({r},{g},{b})");
    }
}
