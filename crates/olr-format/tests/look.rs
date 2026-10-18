//! The level look (spec §Data, plan 15a Task 2): `level.ron`'s `look` and
//! `Light` objects, the four `look/` images, their limits and hostile files.

use olr_data::image::RgbaImage;
use olr_data::level::Material;
use olr_format::image_io;
use olr_format::levels::{LevelObject, LevelSource, compile_look, read_level_files, write_level_files};
use olr_format::{FormatError, canonical_text};
use olr_look::{Animation, GlowColour, Reach};
use olr_terrain::GlowLayers;
use olr_vfs::MemFs;

const SPEC_LEVEL: &str = r##"(
    name: "Lit",
    size: (4, 3),
    art_scale: 2,
    theme: "olx-theme-dirt",
    objects: [
        Spawn(at: (1, 1)),
        Light(at: (2, 1), colour: "#ffbe78", intensity: 120, reach: Pixels(160),
              radius: 2, animation: Flicker(speed: 8, depth: 25)),
    ],
    look: (
        ambient: (colour: "#ffffff", intensity: 30),
        max_light: 100,
        light_steps: 0,
        softness: 0,
        grade: (exposure: 0, contrast: 100, saturation: 100, tint: "#ffffff"),
        glow_groups: [
            (id: "sky", colour: Tint("#fff0d2"), intensity: 100, reach: Unlimited, animation: Steady),
            (id: "lava", colour: Own, intensity: 150, reach: Pixels(96), animation: Flicker(speed: 6, depth: 20)),
        ],
    ),
)
"##;

fn level(look: Option<&str>) -> LevelSource {
    let (w, h, s) = (4u32, 3u32, 2u32);
    let mut src = LevelSource {
        name: "Lit".into(),
        notes: String::new(),
        size: (w, h),
        art_scale: s,
        theme: "olx-theme-dirt".into(),
        objects: vec![LevelObject::spawn((1, 1))],
        material: vec![Material::Dirt; (w * h) as usize],
        front: RgbaImage::new(w * s, h * s),
        back: RgbaImage::new(w * s, h * s),
        thumb: None,
        look: None,
        glow: None,
    };
    if let Some(text) = look {
        let fs = files(&src).with("level.ron", text.as_bytes().to_vec());
        src = read_level_files(&fs).unwrap();
    }
    src
}

fn files(src: &LevelSource) -> MemFs {
    let mut fs = MemFs::new();
    write_level_files(&mut fs, src).unwrap();
    fs
}

fn glow(src: &LevelSource) -> GlowLayers {
    let art = (src.size.0 * src.art_scale, src.size.1 * src.art_scale);
    let mut g = GlowLayers::empty(art, src.size);
    g.front[3] = 200;
    g.back[5] = 9;
    g.groups[1] = 1;
    g.groups_back[2] = 1;
    g
}

#[test]
fn the_spec_example_round_trips() {
    let src = level(Some(SPEC_LEVEL));
    let look = compile_look(&src).unwrap().expect("a look");
    assert_eq!(look.ambient.intensity, 30);
    assert_eq!(look.groups.len(), 2);
    assert_eq!(look.groups[0].colour, GlowColour::Tint([255, 240, 210]));
    assert_eq!(look.groups[1].reach, Reach::Pixels(96));
    assert_eq!(look.groups[1].animation, Animation::Flicker { speed: 6, depth: 20 });
    assert_eq!(look.lights.len(), 1);
    let l = &look.lights[0];
    assert_eq!((l.at, l.radius, l.light.colour, l.light.intensity), ((2, 1), 2, [255, 190, 120], 120));
    // Canonical text drops the defaults and reads back to itself.
    let canonical = canonical_text("level.ron", SPEC_LEVEL).unwrap();
    assert_eq!(canonical_text("level.ron", &canonical).unwrap(), canonical);
    for gone in ["max_light", "light_steps", "softness", "contrast", "radius", "animation: Steady", "colour: Own"] {
        assert!(!canonical.contains(gone), "{gone} in {canonical}");
    }
    // Written and read again: the same level.
    let back = read_level_files(&files(&src)).unwrap();
    assert_eq!(back, src);
}

#[test]
fn a_level_without_a_look_writes_none() {
    let src = level(None);
    let fs = files(&src);
    let names: Vec<String> = fs.files().map(|(p, _)| p.to_string()).collect();
    assert_eq!(names, ["back.png", "front.png", "level.ron", "material.png"]);
    let text = String::from_utf8(fs.files().find(|(p, _)| p.as_str() == "level.ron").unwrap().1.to_vec()).unwrap();
    assert!(!text.contains("look"), "{text}");
    assert_eq!(compile_look(&src).unwrap(), None);
}

#[test]
fn glow_images_round_trip() {
    let mut src = level(Some(SPEC_LEVEL));
    src.glow = Some(glow(&src));
    let fs = files(&src);
    let names: Vec<String> = fs.files().map(|(p, _)| p.to_string()).filter(|p| p.starts_with("look/")).collect();
    assert_eq!(names, ["look/glow-back.png", "look/glow.png", "look/groups-back.png", "look/groups.png"]);
    assert_eq!(olr_format::levels::file_paths(&src).iter().filter(|p| p.starts_with("look/")).count(), 4);
    let back = read_level_files(&fs).unwrap();
    assert_eq!(back, src);
    let mut again = MemFs::new();
    write_level_files(&mut again, &back).unwrap();
    assert_eq!(again, fs);
}

fn err(src: &LevelSource) -> String {
    match compile_look(src) {
        Err(e) => e.to_string(),
        Ok(_) => match write_level_files(&mut MemFs::new(), src) {
            Err(e) => e.to_string(),
            Ok(()) => String::new(),
        },
    }
}

#[test]
fn limits_are_enforced() {
    let cases: &[(&str, &str, &str)] = &[
        ("intensity: 30)", "intensity: 101)", "look.ambient.intensity"),
        ("max_light: 100", "max_light: 201", "look.max_light"),
        ("max_light: 100", "max_light: 99", "look.max_light"),
        ("light_steps: 0", "light_steps: 1", "look.light_steps"),
        ("light_steps: 0", "light_steps: 33", "look.light_steps"),
        ("softness: 0", "softness: 5", "look.softness"),
        ("exposure: 0", "exposure: 201", "look.grade.exposure"),
        ("contrast: 100", "contrast: 201", "look.grade.contrast"),
        ("saturation: 100", "saturation: 201", "look.grade.saturation"),
        ("intensity: 150", "intensity: 1001", "look.glow_groups[1].intensity"),
        ("reach: Pixels(96)", "reach: Pixels(4097)", "look.glow_groups[1].reach"),
        ("id: \"lava\"", "id: \"Lava!\"", "look.glow_groups[1].id"),
        ("id: \"lava\"", "id: \"sky\"", "used twice"),
        ("depth: 20)", "depth: 101)", "look.glow_groups[1].animation.depth"),
        ("speed: 6,", "speed: 0,", "look.glow_groups[1].animation.speed"),
        ("intensity: 120", "intensity: 1001", "objects[1].intensity"),
        ("radius: 2", "radius: 17", "objects[1].radius"),
        ("radius: 2", "radius: 0", "objects[1].radius"),
        ("\"#ffbe78\"", "\"#ffbe7880\"", "objects[1].colour"),
    ];
    for (from, to, want) in cases {
        assert!(SPEC_LEVEL.contains(from), "{from}");
        let text = SPEC_LEVEL.replacen(from, to, 1);
        let fs = files(&level(None)).with("level.ron", text.into_bytes());
        let e = match read_level_files(&fs) {
            Err(e) => e.to_string(),
            Ok(src) => err(&src),
        };
        assert!(e.contains(want), "{to}: {e:?}");
    }
    // 256 groups.
    let mut src = level(Some(SPEC_LEVEL));
    let look = src.look.as_mut().unwrap();
    let g = look.glow_groups[0].clone();
    look.glow_groups = (0..256).map(|i| olr_format::look::GlowGroupSource { id: format!("g{i}"), ..g.clone() }).collect();
    assert!(err(&src).contains("256 groups"), "{}", err(&src));
    // 1025 lights.
    let mut src = level(Some(SPEC_LEVEL));
    let light = src.objects[1].clone();
    src.objects = vec![light; 1025];
    assert!(err(&src).contains("1025 lights"), "{}", err(&src));
}

#[test]
fn hostile_look_files_are_errors_never_panics() {
    let mut src = level(Some(SPEC_LEVEL));
    src.glow = Some(glow(&src));
    let good = files(&src);
    let read = |fs: &MemFs| read_level_files(fs).map_err(|e: FormatError| e.to_string());
    // The wrong size.
    let fs = good.clone().with("look/glow.png", image_io::encode_grey(3, 3, &[0; 9]));
    assert!(read(&fs).unwrap_err().contains("look/glow.png"));
    // Two of the four images.
    let mut fs = MemFs::new();
    for (p, b) in good.files() {
        if !p.as_str().starts_with("look/groups") {
            fs = fs.with(p.as_str(), b.to_vec());
        }
    }
    let e = read(&fs).unwrap_err();
    assert!(e.contains("look/groups.png"), "{e}");
    // A group index past the list.
    let mut groups = vec![0u8; 12];
    groups[7] = 200;
    let fs = good.clone().with("look/groups.png", image_io::encode_grey(4, 3, &groups));
    let e = read(&fs).unwrap_err();
    assert!(e.contains("look/groups.png") && e.contains("200") && e.contains("(3, 1)"), "{e}");
    // A colour image instead of grey, and garbage.
    let fs = good.clone().with("look/glow-back.png", image_io::encode_rgba(&RgbaImage::new(8, 6)));
    assert!(read(&fs).unwrap_err().contains("look/glow-back.png"));
    let fs = good.clone().with("look/glow.png", b"not a png".to_vec());
    assert!(read(&fs).unwrap_err().contains("look/glow.png"));
    // A bomb's header.
    let fs = good.clone().with("look/glow.png", image_io::declared_png(30_000, 30_000, false));
    assert!(read(&fs).is_err());
    // Animations past their limits.
    for (from, to) in [
        ("Flicker(speed: 8, depth: 25)", "Pulse(period_ms: 0, low: 1, high: 2)"),
        ("reach: Pixels(160)", "reach: Pixels(0)"),
        (
            "Flicker(speed: 8, depth: 25)",
            &format!("Keys(keys: [{}])", (0..65).map(|i| format!("({i}, 10)")).collect::<Vec<_>>().join(", ")),
        ),
        ("Flicker(speed: 8, depth: 25)", "Keys(keys: [(10, 1), (5, 1)])"),
        ("Flicker(speed: 8, depth: 25)", "Keys(keys: [])"),
    ] {
        let text = SPEC_LEVEL.replacen(from, to, 1);
        let fs = good.clone().with("level.ron", text.into_bytes());
        let e = match read(&fs) {
            Err(e) => e,
            Ok(s) => err(&s),
        };
        assert!(e.contains("objects[1]"), "{to}: {e}");
    }
}

#[test]
fn look_files_are_hashed_and_known() {
    let mut src = level(Some(SPEC_LEVEL));
    src.glow = Some(glow(&src));
    let manifest = b"(format_version: 5, id: \"lit\", name: \"Lit\", version: \"1\", kind: Level)\n".to_vec();
    let fs = files(&src).with("pack.ron", manifest);
    let diags = olr_pack::validate(&fs);
    assert!(diags.iter().all(|d| !d.to_string().contains("not read")), "{diags:?}");
    let a = olr_pack::content_hash(&fs).unwrap();
    let b = olr_pack::content_hash(&fs.clone().with("look/glow.png", image_io::encode_grey(8, 6, &[1; 48]))).unwrap();
    assert_ne!(a, b);
}

// ---- Mod looks (plan 15a Task 3) ----

use olr_format::mods::{compile_mod, compile_mod_look, read_mod_files};

const MOD: &str = "(name: \"T\", weapons: [\"w\"])";

fn weapon(projectile_look: &str, weapon_look: &str, spawn_look: &str) -> String {
    format!(
        r##"(
    name: "W",
    recharge: 0.5,
    drain: 30.0,
    between_shots_s: 0.25,
    fire: Projectile(
        shot: (
            speed: 100,
            projectile: (
                gfx: Pixels(["#948800"]),
                hit_terrain: [Explode(damage: 4), Spawn("s")],
                spawns: {{"s": (amount: 3, projectile: (gfx: Pixels([]){spawn_look}))}},
                {projectile_look}
            ),
        ),
    ),
    {weapon_look}
)
"##
    )
}

const SPEC_PROJECTILE_LOOK: &str = r##"look: (
                    glow: 80,
                    light: (colour: "#ff8c28", intensity: 80, reach: Pixels(48), animation: Flicker(speed: 10, depth: 15)),
                    end_flash: (colour: "#ffc878", intensity: 200, reach: Pixels(96),
                                animation: Keys(keys: [(0, 100), (250, 0)], looped: false)),
                ),"##;

const SPEC_WEAPON_LOOK: &str = r##"look: (muzzle_light: (colour: "#ffdca0", intensity: 120, reach: Pixels(64),
                          animation: Keys(keys: [(0, 100), (60, 0)]))),"##;

fn mod_fs(w: &str) -> MemFs {
    MemFs::new().with("mod.ron", MOD).with("weapons/w.ron", w.to_string())
}

#[test]
fn the_spec_mod_example_round_trips() {
    let w = weapon(SPEC_PROJECTILE_LOOK, SPEC_WEAPON_LOOK, "");
    let src = read_mod_files(&mod_fs(&w)).unwrap();
    compile_mod(&src).unwrap();
    let look = compile_mod_look(&src).unwrap();
    let p = &look.projectiles["w"];
    assert_eq!(p.glow, Some(80));
    let light = p.light.as_ref().unwrap();
    assert_eq!((light.colour, light.intensity, light.reach), ([255, 140, 40], 80, Reach::Pixels(48)));
    let flash = p.end_flash.as_ref().unwrap();
    assert!(flash.animation.ended(251.0));
    assert_eq!(look.weapons.len(), 1);
    let muzzle = look.weapons[0].as_ref().unwrap().muzzle_light.as_ref().unwrap();
    assert_eq!(muzzle.intensity, 120);
    assert!(!look.projectiles.contains_key("w.spawn"));
    let canonical = canonical_text("weapons/w.ron", &w).unwrap();
    assert_eq!(canonical_text("weapons/w.ron", &canonical).unwrap(), canonical);
    assert!(!canonical.contains("looped"), "{canonical}");
}

#[test]
fn a_mod_without_looks_compiles_to_none() {
    let src = read_mod_files(&mod_fs(&weapon("", "", ""))).unwrap();
    let look = compile_mod_look(&src).unwrap();
    assert!(look.projectiles.is_empty());
    assert_eq!(look.weapons, vec![None]);
}

#[test]
fn an_inline_projectile_look_compiles_under_its_key() {
    let w = weapon("", "", ", look: (glow: 40)");
    let look = compile_mod_look(&read_mod_files(&mod_fs(&w)).unwrap()).unwrap();
    assert_eq!(look.projectiles.keys().collect::<Vec<_>>(), ["w.spawn"]);
    assert_eq!(look.projectiles["w.spawn"].glow, Some(40));
}

#[test]
fn mod_look_limits() {
    for (from, to, want) in [
        ("glow: 80", "glow: 101", "fire.shot.projectile.look.glow"),
        ("intensity: 80", "intensity: 1001", "fire.shot.projectile.look.light.intensity"),
        ("reach: Pixels(96)", "reach: Pixels(0)", "fire.shot.projectile.look.end_flash.reach"),
        ("intensity: 120", "intensity: 1001", "look.muzzle_light.intensity"),
    ] {
        let w = weapon(SPEC_PROJECTILE_LOOK, SPEC_WEAPON_LOOK, "").replacen(from, to, 1);
        let src = read_mod_files(&mod_fs(&w)).unwrap();
        let e = compile_mod(&src).unwrap_err().to_string();
        assert!(e.contains("weapons/w.ron") && e.contains(want), "{to}: {e}");
        assert!(compile_mod_look(&src).is_err());
    }
}

#[test]
fn bundled_mods_have_no_look() {
    let Some(root) = std::env::var_os("OLR_CONTENT_DIR").map(std::path::PathBuf::from) else { return };
    for id in ["olx-classic", "olx-mw-1-0"] {
        let src = read_mod_files(&olr_vfs::DirFs::new(root.join(id))).unwrap();
        let look = compile_mod_look(&src).unwrap();
        assert!(look.projectiles.is_empty() && look.weapons.iter().all(Option::is_none), "{id}");
    }
}
