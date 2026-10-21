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
    ],
    look: (
        ambient: (colour: "#ffffff", intensity: 30),
        max_light: 100,
        light_steps: 0,
        softness: 0,
        light_soak: 4,
        solid_floor: (colour: "#c0c0ff", intensity: 40),
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
        clear: None,
        distant: Vec::new(),
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
    assert_eq!(
        (look.light_soak, look.solid_floor.colour, look.solid_floor.intensity),
        (4, [192, 192, 255], 40)
    );
    assert_eq!(look.groups.len(), 2);
    assert_eq!(look.groups[0].colour, GlowColour::Tint([255, 240, 210]));
    assert_eq!(look.groups[1].reach, Reach::Pixels(96));
    assert_eq!(look.groups[1].animation, Animation::Flicker { speed: 6, depth: 20 });
    // Canonical text drops the defaults and reads back to itself.
    let canonical = canonical_text("level.ron", SPEC_LEVEL).unwrap();
    assert_eq!(canonical_text("level.ron", &canonical).unwrap(), canonical);
    for gone in ["max_light", "light_steps", "softness", "contrast", "animation: Steady", "colour: Own"] {
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
    let text =
        String::from_utf8(fs.files().find(|(p, _)| p.as_str() == "level.ron").unwrap().1.to_vec()).unwrap();
    assert!(!text.contains("look"), "{text}");
    assert_eq!(compile_look(&src).unwrap(), None);
}

#[test]
fn glow_images_round_trip() {
    let mut src = level(Some(SPEC_LEVEL));
    src.glow = Some(glow(&src));
    let fs = files(&src);
    let names: Vec<String> =
        fs.files().map(|(p, _)| p.to_string()).filter(|p| p.starts_with("look/")).collect();
    assert_eq!(names, ["look/glow-back.png", "look/glow.png", "look/groups-back.png", "look/groups.png"]);
    assert_eq!(olr_format::levels::file_paths(&src).iter().filter(|p| p.starts_with("look/")).count(), 4);
    let back = read_level_files(&fs).unwrap();
    assert_eq!(back, src);
    let mut again = MemFs::new();
    write_level_files(&mut again, &back).unwrap();
    assert_eq!(again, fs);
}

#[test]
fn clear_image_round_trips_on_its_own() {
    // A level without a look or glow may still have clear solids.
    let mut src = level(None);
    src.clear = Some((0..12).map(|i| if i % 3 == 0 { 255 } else { 0 }).collect());
    let fs = files(&src);
    let names: Vec<String> =
        fs.files().map(|(p, _)| p.to_string()).filter(|p| p.starts_with("look/")).collect();
    assert_eq!(names, ["look/clear.png"]);
    assert!(olr_format::levels::file_paths(&src).iter().any(|p| p == "look/clear.png"));
    assert_eq!(read_level_files(&fs).unwrap(), src);
    // The wrong number of flags is refused on write.
    let mut bad = src.clone();
    bad.clear = Some(vec![0; 11]);
    assert!(err(&bad).contains("look/clear.png"), "{}", err(&bad));
}

#[test]
fn a_clear_image_of_the_wrong_size_fails_strictly_and_warns_leniently() {
    let src = level(None);
    let fs = files(&src).with("look/clear.png", image_io::encode_grey(5, 3, &[0; 15]));
    let e = read_level_files(&fs).unwrap_err().to_string();
    assert!(e.contains("look/clear.png"), "{e}");
    let (back, warnings) = olr_format::levels::read_level_files_lenient(&fs).unwrap();
    assert_eq!(back.clear, None);
    assert!(
        warnings.iter().any(|w| w.contains("look/clear.png") && w.contains("clear solids")),
        "{warnings:?}"
    );
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
        ("light_soak: 4", "light_soak: 13", "look.light_soak"),
        ("intensity: 40)", "intensity: 101)", "look.solid_floor.intensity"),
        ("exposure: 0", "exposure: 201", "look.grade.exposure"),
        ("contrast: 100", "contrast: 201", "look.grade.contrast"),
        ("saturation: 100", "saturation: 201", "look.grade.saturation"),
        ("intensity: 150", "intensity: 1001", "look.glow_groups[1].intensity"),
        ("reach: Pixels(96)", "reach: Pixels(4097)", "look.glow_groups[1].reach"),
        ("id: \"lava\"", "id: \"Lava!\"", "look.glow_groups[1].id"),
        ("id: \"lava\"", "id: \"sky\"", "used twice"),
        ("depth: 20)", "depth: 101)", "look.glow_groups[1].animation.depth"),
        ("speed: 6,", "speed: 0,", "look.glow_groups[1].animation.speed"),
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
    look.glow_groups =
        (0..256).map(|i| olr_format::look::GlowGroupSource { id: format!("g{i}"), ..g.clone() }).collect();
    assert!(err(&src).contains("256 groups"), "{}", err(&src));
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
    // 0 is no group and 1..=n name the look's n groups: n + 1 is past them.
    groups[7] = 3;
    let fs = good.clone().with("look/groups.png", image_io::encode_grey(4, 3, &groups));
    assert!(read(&fs).unwrap_err().contains("group 3"));
    groups[7] = 2;
    let fs = good.clone().with("look/groups.png", image_io::encode_grey(4, 3, &groups));
    read(&fs).unwrap();
    // A 16-bit grey image is read as 8-bit, never a panic (Review Focus 1).
    let wide = {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, 8, 6);
            enc.set_color(png::ColorType::Grayscale);
            enc.set_depth(png::BitDepth::Sixteen);
            let mut w = enc.write_header().unwrap();
            w.write_image_data(&[0x80; 8 * 6 * 2]).unwrap();
        }
        out
    };
    let fs = good.clone().with("look/glow.png", wide);
    assert_eq!(read_level_files(&fs).unwrap().glow.unwrap().front[0], 0x80);
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
        ("Flicker(speed: 6, depth: 20)", "Pulse(period_ms: 0, low: 1, high: 2)"),
        ("reach: Pixels(96)", "reach: Pixels(0)"),
        (
            "Flicker(speed: 6, depth: 20)",
            &format!(
                "Keys(keys: [{}])",
                (0..65).map(|i| format!("({i}, 10)")).collect::<Vec<_>>().join(", ")
            ),
        ),
        ("Flicker(speed: 6, depth: 20)", "Keys(keys: [(10, 1), (5, 1)])"),
        ("Flicker(speed: 6, depth: 20)", "Keys(keys: [])"),
    ] {
        let text = SPEC_LEVEL.replacen(from, to, 1);
        let fs = good.clone().with("level.ron", text.into_bytes());
        let e = match read(&fs) {
            Err(e) => e,
            Ok(s) => err(&s),
        };
        assert!(e.contains("look.glow_groups[1]"), "{to}: {e}");
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
    let b = olr_pack::content_hash(&fs.clone().with("look/glow.png", image_io::encode_grey(8, 6, &[1; 48])))
        .unwrap();
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
    assert!(look.weapons.is_empty());
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

#[test]
fn the_default_look_is_the_empty_file() {
    use olr_format::look::LookSource;
    let empty: LookSource = olr_format::ron_io::from_canonical("level.ron", "()").unwrap();
    assert_eq!(empty, LookSource::default());
    assert_eq!(olr_format::ron_io::to_canonical("level.ron", &LookSource::default()).unwrap(), "()\n");
}

#[test]
fn solids_defaults_are_the_stop_1_verdict() {
    // Soak 8 and a white floor at 30 % (plan 15a Stop 1b) when a look says nothing.
    let src = level(Some("(name: \"Lit\", size: (4, 3), art_scale: 2, theme: \"olx-theme-dirt\", look: ())"));
    let look = compile_look(&src).unwrap().expect("a look");
    assert_eq!((look.light_soak, look.solid_floor.colour, look.solid_floor.intensity), (8, [255; 3], 30));
    // The defaults are left out of canonical text.
    let text = canonical_text("level.ron", "(name: \"Lit\", size: (4, 3), art_scale: 2, theme: \"olx-theme-dirt\", look: (light_soak: 8, solid_floor: (intensity: 30)))").unwrap();
    assert!(!text.contains("light_soak") && !text.contains("solid_floor"), "{text}");
}

/// A 64 × 32 open level of white art (art scale 1) with `look` (a
/// `level.ron` text's look), for the lit thumbnails.
fn white_level(look: &str) -> LevelSource {
    let (w, h) = (64u32, 32u32);
    let mut white = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            white.set(x, y, olr_data::image::Rgba::rgb(255, 255, 255));
        }
    }
    let src = LevelSource {
        name: "Thumb".into(),
        notes: String::new(),
        size: (w, h),
        art_scale: 1,
        theme: "olx-theme-dirt".into(),
        objects: Vec::new(),
        material: vec![Material::Background; (w * h) as usize],
        front: white.clone(),
        back: white,
        thumb: None,
        look: None,
        glow: None,
        clear: None,
        distant: Vec::new(),
    };
    let text = format!("(name: \"Thumb\", size: (64, 32), theme: \"olx-theme-dirt\", {look})");
    read_level_files(&files(&src).with("level.ron", text.into_bytes())).unwrap()
}

/// The user's note (2026-10-19): a lit level's thumbnail shows its look.
/// Without a look it is the plain thumbnail.
#[test]
fn a_level_without_a_look_keeps_its_plain_thumbnail() {
    let l = white_level("");
    assert!(olr_format::levels::level_thumbnail(&l) == olr_format::levels::thumbnail(&l.front));
}

#[test]
fn a_dark_level_has_a_dark_thumbnail() {
    let dark = white_level("look: (ambient: (intensity: 10))");
    let t = olr_format::levels::level_thumbnail(&dark);
    let plain = olr_format::levels::thumbnail(&dark.front);
    assert_eq!((t.width(), t.height()), (plain.width(), plain.height()));
    // 64 × 32 fills 160 × 80 (bars of 8 above and below).
    let mid = t.get(80, 48);
    assert!(mid.r < 110 && mid.r > 60, "about 10 % light: {mid:?}");
    assert_eq!(t.get(80, 2).a, 0, "the bars stay clear");
}

#[test]
fn glow_shows_and_lights_around_it_in_the_thumbnail() {
    let mut l = white_level(
        "look: (ambient: (intensity: 5), glow_groups: [(id: \"lamp\", colour: Own, reach: Pixels(16))])",
    );
    let mut g = GlowLayers::empty((64, 32), (64, 32));
    for y in 12..20 {
        for x in 40..48 {
            let i = (y * 64 + x) as usize;
            (g.front[i], g.back[i], g.groups[i], g.groups_back[i]) = (255, 255, 1, 1);
        }
    }
    l.glow = Some(g);
    let t = olr_format::levels::level_thumbnail(&l);
    let (on, beside, far) = (t.get(110, 48), t.get(95, 48), t.get(10, 48));
    assert!(on.r > 240, "the glowing art at full: {on:?}");
    assert!(beside.r > far.r + 20, "its light spills around: {beside:?} {far:?}");
}

#[test]
fn solids_never_fall_below_the_solid_floor_in_the_thumbnail() {
    let mut l = white_level("look: (ambient: (intensity: 0), solid_floor: (intensity: 40))");
    l.material = vec![Material::Rock; 64 * 32];
    let t = olr_format::levels::level_thumbnail(&l);
    let v = t.get(80, 48);
    // 40 % white light on white art is about sRGB 170.
    assert!((160..=180).contains(&v.r), "{v:?}");
}

/// Plan 15c: glowing pixels are a level's only light. A `Light` object is
/// an error naming it (compatibility breaks are fine at this stage).
#[test]
fn a_light_object_is_an_error() {
    let text = "(name: \"Lit\", size: (4, 3), art_scale: 2, theme: \"olx-theme-dirt\", objects: [\
                Light(at: (2, 1), colour: \"#ffffff\", intensity: 100, reach: Pixels(16))])";
    let fs = files(&level(None)).with("level.ron", text.as_bytes().to_vec());
    let e = read_level_files(&fs).unwrap_err().to_string();
    assert!(e.contains("level.ron") && e.contains("Light"), "{e}");
}

/// Plan 15c: `shadows` went with the point lights.
#[test]
fn shadows_is_an_unknown_field() {
    let text =
        "(name: \"Lit\", size: (4, 3), art_scale: 2, theme: \"olx-theme-dirt\", look: (shadows: Hard))";
    let fs = files(&level(None)).with("level.ron", text.as_bytes().to_vec());
    let e = read_level_files(&fs).unwrap_err().to_string();
    assert!(e.contains("shadows"), "{e}");
}

/// Issue #10: `occlusion` is absent by default, round-trips (its default
/// reach left out of canonical text) and refuses values out of range.
#[test]
fn occlusion_round_trips_and_is_bounded() {
    assert_eq!(compile_look(&level(Some(SPEC_LEVEL))).unwrap().unwrap().occlusion, None, "absent: none");
    let with = |o: &str| {
        SPEC_LEVEL.replacen("max_light: 100,", &format!("occlusion: {o},\n        max_light: 100,"), 1)
    };
    let src = level(Some(&with("(strength: 60, reach: 20)")));
    let look = compile_look(&src).unwrap().unwrap();
    assert_eq!(look.occlusion, Some(olr_look::Occlusion { strength: 60, reach: 20 }));
    let fs = files(&src);
    assert_eq!(read_level_files(&fs).unwrap(), src);
    let text = canonical_text("level.ron", &with("(strength: 60, reach: 12)")).unwrap();
    assert!(text.contains("occlusion: (\n            strength: 60,\n        ),\n"), "{text}");
    let src = level(Some(&with("(strength: 40)")));
    let o = compile_look(&src).unwrap().unwrap().occlusion;
    assert_eq!(o, Some(olr_look::Occlusion { strength: 40, reach: 12 }), "the default reach");
    for (o, want) in [
        ("(strength: 0)", "look.occlusion.strength"),
        ("(strength: 101)", "look.occlusion.strength"),
        ("(strength: 50, reach: 1)", "look.occlusion.reach"),
        ("(strength: 50, reach: 33)", "look.occlusion.reach"),
        ("(strength: 50, reach: 70000)", "level.ron"),
        ("(reach: 12)", "strength"),
        ("(strength: 50, depth: 3)", "depth"),
    ] {
        let fs = files(&level(None)).with("level.ron", with(o).into_bytes());
        let e = match read_level_files(&fs) {
            Err(e) => e.to_string(),
            Ok(src) => err(&src),
        };
        assert!(e.contains(want), "{o}: {e:?}");
    }
}

/// Issue #10: the occlusion is in `level.ron`, so it changes the pack's
/// content hash.
#[test]
fn occlusion_changes_the_content_hash() {
    let manifest = b"(format_version: 5, id: \"lit\", name: \"Lit\", version: \"1\", kind: Level)\n".to_vec();
    let src = level(Some(SPEC_LEVEL));
    let fs = files(&src).with("pack.ron", manifest);
    let a = olr_pack::content_hash(&fs).unwrap();
    let mut occluded = src.clone();
    occluded.look.as_mut().unwrap().occlusion =
        Some(olr_format::look::OcclusionSource { strength: 60, reach: 12 });
    let mut fs2 = fs.clone();
    write_level_files(&mut fs2, &occluded).unwrap();
    assert_eq!(read_level_files(&fs2).unwrap(), occluded);
    assert_ne!(olr_pack::content_hash(&fs2).unwrap(), a);
}

/// Issue #10: the lit thumbnail leaves the occlusion out (as it does soak
/// and softness).
#[test]
fn the_thumbnail_leaves_occlusion_out() {
    let mut plain = white_level("look: (ambient: (intensity: 50))");
    let mut occluded = white_level("look: (ambient: (intensity: 50), occlusion: (strength: 100))");
    for l in [&mut plain, &mut occluded] {
        l.material =
            (0..64 * 32).map(|i| if i % 64 < 20 { Material::Rock } else { Material::Background }).collect();
    }
    assert!(olr_format::levels::level_thumbnail(&plain) == olr_format::levels::level_thumbnail(&occluded));
}
