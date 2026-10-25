//! The level look (spec §Data, plan 15a Task 2): `level.ron`'s `look` and
//! `Light` objects, the four `look/` images, their limits and hostile files.

use olr_data::image::RgbaImage;
use olr_data::level::Material;
use olr_format::image_io;
use olr_format::levels::{
    Composition, Element, ElementGlow, ElementKind, ElementMaterial, GlowBeam, LevelObject, LevelSource,
    check_elements, check_glow_groups, compile_look, read_level_files, write_level_files,
};
use olr_format::look::LookSource;
use olr_format::ron_io::{from_canonical, to_canonical};
use olr_format::{FormatError, canonical_text};
use olr_look::{Animation, GlowColour, Reach};
use olr_terrain::{GlassMap, GlowBeamMap, GlowLayers};
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
        glass: None,
        glow_beams: None,
        haze: None,
        distant: Vec::new(),
        surface: None,
        distant_surfaces: Vec::new(),
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
fn glass_map_round_trips() {
    // The background glass design: `look/glass.png`, RGBA at art size.
    let mut src = level(None);
    let mut g = GlassMap::none((8, 6));
    g.texels[4..8].copy_from_slice(&[235, 165, 50, 15]);
    src.glass = Some(g);
    let fs = files(&src);
    let names: Vec<String> =
        fs.files().map(|(p, _)| p.to_string()).filter(|p| p.starts_with("look/")).collect();
    assert_eq!(names, ["look/glass.png"]);
    assert!(olr_format::levels::file_paths(&src).iter().any(|p| p == "look/glass.png"));
    assert_eq!(read_level_files(&fs).unwrap(), src);
    // None writes no file.
    assert!(!files(&level(None)).files().any(|(p, _)| p.to_string() == "look/glass.png"));
    // A map of another size is refused on write.
    let mut bad = src.clone();
    bad.glass = Some(GlassMap::none((8, 5)));
    assert!(err(&bad).contains("look/glass.png"), "{}", err(&bad));
}

#[test]
fn a_misfit_glass_map_is_dropped_when_playing() {
    let src = level(None);
    let fs = files(&src).with("look/glass.png", image_io::encode_rgba(&RgbaImage::new(8, 5)));
    let e = read_level_files(&fs).unwrap_err().to_string();
    assert!(e.contains("look/glass.png"), "{e}");
    let (back, warnings) = olr_format::levels::read_level_files_lenient(&fs).unwrap();
    assert_eq!(back.glass, None);
    assert!(
        warnings.iter().any(|w| w.contains("look/glass.png") && w.contains("plays without its glass")),
        "{warnings:?}"
    );
}

#[test]
fn filtered_glow_colour_compiles() {
    let text = |colour: &str| {
        format!(
            "(name: \"Lit\", size: (4, 3), art_scale: 2, theme: \"olx-theme-dirt\", objects: [Spawn(at: (1, 1))], \
             look: (glow_groups: [(id: \"g\", colour: {colour}, reach: Pixels(10))]))"
        )
    };
    let src = level(Some(&text("Filtered(\"#c8d4f0\")")));
    let look = compile_look(&src).unwrap().expect("a look");
    assert_eq!(look.groups[0].colour, GlowColour::Filtered([200, 212, 240]));
    let fs = files(&level(None)).with("level.ron", text("Filtered(\"#c8d4f080\")").into_bytes());
    let e = read_level_files(&fs).unwrap_err().to_string();
    assert!(e.contains("colour"), "{e}");
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
    assert!(!look.projectiles.contains_key("w.s"));
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
    assert!(look.explosion.is_none());
}

#[test]
fn an_inline_projectile_look_compiles_under_its_key() {
    let w = weapon("", "", ", look: (glow: 40)");
    let look = compile_mod_look(&read_mod_files(&mod_fs(&w)).unwrap()).unwrap();
    assert_eq!(look.projectiles.keys().collect::<Vec<_>>(), ["w.s"]);
    assert_eq!(look.projectiles["w.s"].glow, Some(40));
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

// ---- Explosion lights (issue #15) ----

const BLUE_EXPLOSION: &str =
    r##"look: (explosion: (colour: "#80b0ff", intensity: 400, radius: 10, reach: Pixels(120))),"##;

const MOD_EXPLOSION: &str = r##"(name: "T", weapons: ["w"], look: (explosion: (colour: "#ff4020", intensity: 150,
    radius: 4, reach: Pixels(40), animation: Pulse(period_ms: 100, low: 50, high: 100), phase_ms: 20)))"##;

#[test]
fn explosion_lights_round_trip_and_compile() {
    let w = weapon(BLUE_EXPLOSION, "", ", look: (glow: 40)");
    let fs = mod_fs(&w).with("mod.ron", MOD_EXPLOSION);
    let src = read_mod_files(&fs).unwrap();
    // The simulation's mod is the one without the lights.
    let plain = read_mod_files(&mod_fs(&weapon("", "", ", look: (glow: 40)"))).unwrap();
    assert_eq!(compile_mod(&src).unwrap(), compile_mod(&plain).unwrap());
    let look = compile_mod_look(&src).unwrap();
    // The projectile's own: the engine's flash keys, fitted to the explosion.
    let own = look.projectiles["w"].explosion.as_ref().unwrap();
    assert_eq!(
        (own.light.colour, own.light.intensity, own.light.reach),
        ([128, 176, 255], 400, Reach::Pixels(120))
    );
    assert_eq!((own.radius, own.fitted), (10, true));
    assert_eq!(own.light, olr_look::engine::explosion_flash([128, 176, 255], 400, 120));
    // The child has a look but no explosion light: the mod's applies.
    assert!(look.projectiles["w.s"].explosion.is_none());
    let wide = look.explosion.as_ref().unwrap();
    assert_eq!(
        (wide.light.colour, wide.light.intensity, wide.radius, wide.fitted),
        ([255, 64, 32], 150, 4, false)
    );
    assert_eq!(wide.light.phase_ms, Some(20));
    assert!(matches!(
        wide.light.animation,
        olr_look::Animation::Pulse { period_ms: 100, low: 50, high: 100, .. }
    ));
    // Canonical text keeps both, and reads back the same.
    for (path, text) in [("weapons/w.ron", w.as_str()), ("mod.ron", MOD_EXPLOSION)] {
        let canonical = canonical_text(path, text).unwrap();
        assert_eq!(canonical_text(path, &canonical).unwrap(), canonical);
        assert!(canonical.contains("explosion"), "{canonical}");
    }
    let mut out = MemFs::new();
    olr_format::mods::write_mod_files(&mut out, &src).unwrap();
    assert_eq!(read_mod_files(&out).unwrap(), src);
}

#[test]
fn explosion_light_limits() {
    for (from, to, want) in [
        ("intensity: 400", "intensity: 1001", "fire.shot.projectile.look.explosion.intensity"),
        ("radius: 10", "radius: 0", "fire.shot.projectile.look.explosion.radius"),
        ("radius: 10", "radius: 17", "fire.shot.projectile.look.explosion.radius"),
        ("reach: Pixels(120)", "reach: Pixels(0)", "fire.shot.projectile.look.explosion.reach"),
        ("reach: Pixels(120)", "reach: Pixels(4097)", "fire.shot.projectile.look.explosion.reach"),
        ("reach: Pixels(120)", "reach: Unlimited", "must be `Pixels`"),
        ("\"#80b0ff\"", "\"#80b0ff80\"", "fire.shot.projectile.look.explosion.colour"),
        (
            "reach: Pixels(120)",
            "reach: Pixels(120), phase_ms: 5",
            "fire.shot.projectile.look.explosion.phase_ms",
        ),
        (
            "reach: Pixels(120)",
            "reach: Pixels(120), animation: Flicker(speed: 0, depth: 5)",
            "explosion.animation.speed",
        ),
    ] {
        let w = weapon(BLUE_EXPLOSION, "", "").replacen(from, to, 1);
        let src = read_mod_files(&mod_fs(&w)).unwrap();
        let e = compile_mod(&src).unwrap_err().to_string();
        assert!(e.contains("weapons/w.ron") && e.contains(want), "{to}: {e}");
        assert!(compile_mod_look(&src).is_err());
    }
    // The caps themselves pass.
    let w = weapon(BLUE_EXPLOSION, "", "").replacen("radius: 10", "radius: 16", 1).replacen(
        "Pixels(120)",
        "Pixels(4096)",
        1,
    );
    compile_mod_look(&read_mod_files(&mod_fs(&w)).unwrap()).unwrap();
    // The mod-wide one is checked as well, naming `mod.ron`.
    let fs =
        mod_fs(&weapon("", "", "")).with("mod.ron", MOD_EXPLOSION.replacen("radius: 4", "radius: 40", 1));
    let e = compile_mod(&read_mod_files(&fs).unwrap()).unwrap_err().to_string();
    assert!(e.contains("mod.ron") && e.contains("look.explosion.radius"), "{e}");
    // An unknown field is an error.
    let fs =
        mod_fs(&weapon("", "", "")).with("mod.ron", "(name: \"T\", weapons: [\"w\"], look: (flash: ()))");
    assert!(read_mod_files(&fs).is_err());
}

// ---- Heat (issue #42) ----

const HOT_PROJECTILE: &str =
    "look: (explosion_heat: (amount: 250, radius: 30), heat: (amount: 4, radius: 6)),";

const MOD_HEAT: &str = r#"(name: "T", weapons: ["w"], look: (explosion_heat: (amount: 40)))"#;

#[test]
fn heat_round_trips_compiles_and_falls_back_to_the_mods() {
    let w = weapon(HOT_PROJECTILE, "", ", look: (glow: 40)");
    let fs = mod_fs(&w).with("mod.ron", MOD_HEAT);
    let src = read_mod_files(&fs).unwrap();
    // Presentation only: the simulation's mod is the one without heat.
    let plain = read_mod_files(&mod_fs(&weapon("", "", ", look: (glow: 40)"))).unwrap();
    assert_eq!(compile_mod(&src).unwrap(), compile_mod(&plain).unwrap());
    let look = compile_mod_look(&src).unwrap();
    let own = &look.projectiles["w"];
    assert_eq!(own.explosion_heat, Some(olr_look::ExplosionHeatLook { amount: 250, radius: Some(30) }));
    assert_eq!(own.heat, Some(olr_look::ProjectileHeat { amount: 4, radius: 6 }));
    assert!(own.explosion.is_none(), "heat is not light");
    // The child has a look but no heat: the mod's explosion heat applies,
    // the engine's radius kept.
    let child = &look.projectiles["w.s"];
    assert!(child.explosion_heat.is_none() && child.heat.is_none());
    assert_eq!(look.explosion_heat, Some(olr_look::ExplosionHeatLook { amount: 40, radius: None }));
    // Canonical text keeps them and leaves the default amount out.
    for (path, text, has) in [
        ("weapons/w.ron", w.clone(), "heat: (\n"),
        ("mod.ron", MOD_HEAT.into(), "amount: 40"),
        ("mod.ron", MOD_HEAT.replacen("amount: 40", "radius: 9", 1), "radius: 9"),
    ] {
        let canonical = canonical_text(path, &text).unwrap();
        assert_eq!(canonical_text(path, &canonical).unwrap(), canonical);
        assert!(canonical.contains("explosion_heat") && canonical.contains(has), "{canonical}");
        assert!(!canonical.contains("amount: 100"), "{canonical}");
    }
    let default: olr_format::look::ExplosionHeatSource =
        olr_format::ron_io::from_canonical("mod.ron", "()").unwrap();
    assert_eq!((default.amount, default.radius), (100, None));
    assert_eq!(olr_format::ron_io::to_canonical("mod.ron", &default).unwrap(), "()\n");
    let mut out = MemFs::new();
    olr_format::mods::write_mod_files(&mut out, &src).unwrap();
    assert_eq!(read_mod_files(&out).unwrap(), src);
}

#[test]
fn heat_limits() {
    let path = "fire.shot.projectile.look";
    for (from, to, want) in [
        ("amount: 250", "amount: 1001", format!("{path}.explosion_heat.amount")),
        ("radius: 30", "radius: 0", format!("{path}.explosion_heat.radius")),
        ("radius: 30", "radius: 49", format!("{path}.explosion_heat.radius")),
        ("amount: 4", "amount: 0", format!("{path}.heat.amount")),
        ("amount: 4", "amount: 101", format!("{path}.heat.amount")),
        ("radius: 6", "radius: 0", format!("{path}.heat.radius")),
        ("radius: 6", "radius: 17", format!("{path}.heat.radius")),
    ] {
        let w = weapon(HOT_PROJECTILE, "", "").replacen(from, to, 1);
        let src = read_mod_files(&mod_fs(&w)).unwrap();
        let e = compile_mod(&src).unwrap_err().to_string();
        assert!(e.contains("weapons/w.ron") && e.contains(&want), "{to}: {e}");
        assert!(compile_mod_look(&src).is_err());
        assert_eq!(olr_format::mods::mod_errors(&src).len(), 1, "{to}");
    }
    // The caps themselves pass, and 0 % is no heat at all.
    let w = weapon(HOT_PROJECTILE, "", "")
        .replacen("amount: 250", "amount: 1000", 1)
        .replacen("radius: 30", "radius: 48", 1)
        .replacen("amount: 4", "amount: 100", 1)
        .replacen("radius: 6", "radius: 16", 1);
    compile_mod_look(&read_mod_files(&mod_fs(&w)).unwrap()).unwrap();
    let w = weapon(HOT_PROJECTILE, "", "").replacen("amount: 250", "amount: 0", 1);
    let look = compile_mod_look(&read_mod_files(&mod_fs(&w)).unwrap()).unwrap();
    assert!(look.projectiles["w"].explosion_heat.unwrap().heat(4.0).is_none());
    // The mod-wide one is checked as well, naming `mod.ron`.
    let fs = mod_fs(&weapon("", "", "")).with("mod.ron", MOD_HEAT.replacen("amount: 40", "radius: 60", 1));
    let src = read_mod_files(&fs).unwrap();
    let e = compile_mod(&src).unwrap_err().to_string();
    assert!(e.contains("mod.ron") && e.contains("look.explosion_heat.radius"), "{e}");
    // A projectile's `heat` needs both fields, and an unknown one is an error.
    for bad in ["heat: (amount: 4)", "heat: (amount: 4, radius: 6, every: 2)"] {
        let w = weapon(&format!("look: ({bad}),"), "", "");
        assert!(read_mod_files(&mod_fs(&w)).is_err(), "{bad}");
    }
}

#[test]
fn bundled_mods_have_no_look() {
    let Some(root) = std::env::var_os("OLR_CONTENT_DIR").map(std::path::PathBuf::from) else { return };
    for id in ["olx-classic", "olx-mw-1-0"] {
        let src = read_mod_files(&olr_vfs::DirFs::new(root.join(id))).unwrap();
        let look = compile_mod_look(&src).unwrap();
        assert!(look.projectiles.is_empty() && look.weapons.iter().all(Option::is_none), "{id}");
        assert!(look.explosion.is_none() && look.explosion_heat.is_none() && src.look.is_none(), "{id}");
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
        glass: None,
        glow_beams: None,
        haze: None,
        distant: Vec::new(),
        surface: None,
        distant_surfaces: Vec::new(),
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

/// The background glass design: a `Filtered` group casts its colour by
/// the art's hue at full strength, so dim art lights as bright art.
#[test]
fn a_filtered_glow_lights_by_the_arts_hue_in_the_thumbnail() {
    let spill = |colour: &str| {
        let mut l = white_level(&format!(
            "look: (ambient: (intensity: 5), glow_groups: [(id: \"glass\", colour: {colour}, reach: Pixels(16))])"
        ));
        let mut g = GlowLayers::empty((64, 32), (64, 32));
        for y in 12..20 {
            for x in 40..48 {
                let i = (y * 64 + x) as usize;
                (g.front[i], g.back[i], g.groups[i], g.groups_back[i]) = (255, 255, 1, 1);
                l.front.set(x, y, olr_data::image::Rgba::rgb(40, 40, 40));
                l.back.set(x, y, olr_data::image::Rgba::rgb(40, 40, 40));
            }
        }
        l.glow = Some(g);
        olr_format::levels::level_thumbnail(&l).get(95, 48)
    };
    let (own, filtered) = (spill("Own"), spill("Filtered(\"#ffffff\")"));
    assert!(filtered.r > own.r + 20, "dim grey lights as white: {filtered:?} vs {own:?}");
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

/// Issue #69: a Beam weapon with `look`'s `beam` made of `beam`.
fn beam_weapon(beam: &str) -> String {
    format!(
        r##"(
    name: "W",
    recharge: 0.18,
    drain: 1.25,
    between_shots_s: 0.009,
    fire: Beam(colour: "#ff0000", worm_damage: 1, length: 999999),
    look: (beam: ({beam})),
)
"##
    )
}

const BEAM_LOOK: &str = r##"light: (colour: "#ff4020", intensity: 30, reach: Pixels(50)),
        tip_light: (colour: "#ffffff", intensity: 250, reach: Pixels(256)),
        heat: (amount: 0, radius: 16)"##;

/// Issue #69: a Beam weapon's own beam look round-trips through the files
/// and canonical text and compiles, the simulation's mod unchanged; each
/// part left out is the engine's.
#[test]
fn a_beam_look_round_trips_and_compiles() {
    let w = beam_weapon(BEAM_LOOK);
    let src = read_mod_files(&mod_fs(&w)).unwrap();
    let plain = read_mod_files(&mod_fs(&beam_weapon("").replace("    look: (beam: ()),\n", ""))).unwrap();
    assert_eq!(compile_mod(&src).unwrap(), compile_mod(&plain).unwrap(), "presentation only");
    let look = compile_mod_look(&src).unwrap();
    let beam = look.weapons[0].as_ref().unwrap().beam.as_ref().unwrap();
    let light = beam.light.as_ref().unwrap();
    assert_eq!((light.colour, light.intensity, light.reach), ([255, 64, 32], 30, Reach::Pixels(50)));
    assert_eq!(beam.tip_light.as_ref().unwrap().reach, Reach::Pixels(256));
    assert_eq!(beam.heat, Some(olr_look::BeamHeat { amount: 0, radius: 16 }));
    assert!(beam.heat.unwrap().part(1).is_none(), "0 heats nothing");
    let canonical = canonical_text("weapons/w.ron", &w).unwrap();
    assert_eq!(canonical_text("weapons/w.ron", &canonical).unwrap(), canonical);
    assert!(canonical.contains("tip_light") && canonical.contains("amount: 0"), "{canonical}");
    let mut out = MemFs::new();
    olr_format::mods::write_mod_files(&mut out, &src).unwrap();
    assert_eq!(read_mod_files(&out).unwrap(), src);
    // Left out, each is none: the engine's.
    let src = read_mod_files(&mod_fs(&beam_weapon("heat: (amount: 20, radius: 3)"))).unwrap();
    let look = compile_mod_look(&src).unwrap();
    let beam = look.weapons[0].as_ref().unwrap().beam.as_ref().unwrap();
    assert!(beam.light.is_none() && beam.tip_light.is_none());
}

/// Issue #69: a beam look's bounds, `Unlimited` refused, its heat's
/// fields required, and `beam` on a weapon that is not a Beam refused.
#[test]
fn beam_look_limits() {
    let path = "look.beam";
    for (from, to, want) in [
        ("Pixels(50)", "Pixels(0)", format!("{path}.light.reach")),
        ("Pixels(50)", "Pixels(257)", format!("{path}.light.reach")),
        ("Pixels(256)", "Unlimited", format!("{path}.tip_light.reach")),
        ("intensity: 250", "intensity: 1001", format!("{path}.tip_light.intensity")),
        ("amount: 0", "amount: 101", format!("{path}.heat.amount")),
        ("radius: 16", "radius: 0", format!("{path}.heat.radius")),
        ("radius: 16", "radius: 17", format!("{path}.heat.radius")),
    ] {
        let w = beam_weapon(BEAM_LOOK).replacen(from, to, 1);
        let src = read_mod_files(&mod_fs(&w)).unwrap();
        let e = compile_mod(&src).unwrap_err().to_string();
        assert!(e.contains("weapons/w.ron") && e.contains(&want), "{to}: {e}");
        assert!(compile_mod_look(&src).is_err());
        assert_eq!(olr_format::mods::mod_errors(&src).len(), 1, "{to}");
    }
    for bad in ["heat: (amount: 4)", "heat: (amount: 4, radius: 6, every: 2)", "glow: 3"] {
        assert!(read_mod_files(&mod_fs(&beam_weapon(bad))).is_err(), "{bad}");
    }
    let w = weapon("", "look: (beam: (heat: (amount: 20, radius: 3))),", "");
    let src = read_mod_files(&mod_fs(&w)).unwrap();
    let e = compile_mod(&src).unwrap_err().to_string();
    assert!(e.contains("weapons/w.ron") && e.contains("look.beam") && e.contains("Beam"), "{e}");
}

// Directional glow (the directional-glow design): `GlowBeam` on an element's
// glow, the two baked beam maps and the haze.

fn beam_element(id: &str, kind: ElementKind, beam: Option<GlowBeam>) -> Element {
    Element {
        id: id.into(),
        name: String::new(),
        visible: true,
        locked: false,
        folder: None,
        kind,
        glow: Some(ElementGlow { strength: 80, group: "g".into(), mask: false, beam }),
        clear: false,
        glass: None,
        distant: None,
        surface: None,
        back_surface: None,
        surface_map: false,
        back_surface_map: false,
    }
}

fn beam_look(reach: &str) -> LookSource {
    let text = format!("(glow_groups: [(id: \"g\", reach: {reach})])");
    from_canonical("level.ron", &text).unwrap()
}

fn beam_map(world: (u32, u32)) -> GlowBeamMap {
    let mut m = GlowBeamMap::none(world);
    m.front[0] = 84;
    m.front[1] = 15;
    m.back[2] = 1;
    m.back[3] = 90;
    m
}

#[test]
fn glow_beams_round_trip() {
    // An element's beam is written as `beam: (direction: 166, spread: 15)`.
    let beam = GlowBeam { direction: 166, spread: 15 };
    let mut c = Composition {
        elements: vec![beam_element("lamp", ElementKind::drawn(ElementMaterial::Rock), Some(beam))],
        folders: Vec::new(),
    };
    let text = to_canonical("composition.ron", &c).unwrap();
    assert!(text.contains("beam: (\n") && text.contains("direction: 166,"), "{text}");
    assert_eq!(from_canonical::<Composition>("composition.ron", &text).unwrap(), c);
    c.elements[0].glow.as_mut().unwrap().beam = None;
    let text = to_canonical("composition.ron", &c).unwrap();
    assert!(!text.contains("beam"), "{text}");

    // A level's beam maps write both files and read back equal.
    let mut src = level(None);
    src.glow_beams = Some(beam_map(src.size));
    let fs = files(&src);
    let names: Vec<String> =
        fs.files().map(|(p, _)| p.to_string()).filter(|p| p.starts_with("look/")).collect();
    assert_eq!(names, ["look/glow-beam-back.png", "look/glow-beam.png"]);
    let paths = olr_format::levels::file_paths(&src);
    assert!(
        paths.iter().any(|p| p == "look/glow-beam.png")
            && paths.iter().any(|p| p == "look/glow-beam-back.png")
    );
    assert_eq!(read_level_files(&fs).unwrap(), src);
    let m = src.glow_beams.as_ref().unwrap();
    assert_eq!(m.get(false, 0, 0), Some((166, 15)));
    assert_eq!(m.get(false, 1, 0), None);
    assert_eq!(m.get(true, 1, 0), Some((0, 90)));
    assert_eq!(m.get(true, 9, 9), None);
    // None writes neither file.
    assert!(!files(&level(None)).files().any(|(p, _)| p.to_string().contains("glow-beam")));
    // A map of another size is refused on write.
    let mut bad = src.clone();
    bad.glow_beams = Some(beam_map((4, 4)));
    assert!(err(&bad).contains("look/glow-beam"), "{}", err(&bad));

    // The haze percent and the haze map.
    let mut src = level(None);
    src.look = Some(LookSource { haze: 40, ..LookSource::default() });
    let fs = files(&src);
    let text = String::from_utf8(
        olr_vfs::ContentSource::read(&fs, &olr_vfs::VPath::new("level.ron").unwrap()).unwrap(),
    )
    .unwrap();
    assert!(text.contains("haze: 40"), "{text}");
    assert_eq!(read_level_files(&fs).unwrap(), src);
    assert_eq!(compile_look(&src).unwrap().unwrap().haze, 40);
    src.look = Some(LookSource::default());
    let text = String::from_utf8(
        olr_vfs::ContentSource::read(&files(&src), &olr_vfs::VPath::new("level.ron").unwrap()).unwrap(),
    )
    .unwrap();
    assert!(!text.contains("haze"), "{text}");
    src.haze = Some((0..12).map(|i| i * 20).collect());
    let fs = files(&src);
    assert!(fs.files().any(|(p, _)| p.to_string() == "look/haze.png"));
    assert!(olr_format::levels::file_paths(&src).iter().any(|p| p == "look/haze.png"));
    assert_eq!(read_level_files(&fs).unwrap(), src);
    src.haze = Some(vec![0; 11]);
    assert!(err(&src).contains("look/haze.png"), "{}", err(&src));
}

#[test]
fn glow_beam_limits() {
    let size = (4, 3);
    let check = |c: &Composition, look: &LookSource| {
        check_elements(c, size, 2).and_then(|_| check_glow_groups(c, Some(look))).err().map(|e| e.to_string())
    };
    let lamp = |material, beam| Composition {
        elements: vec![beam_element("lamp", ElementKind::drawn(material), Some(beam))],
        folders: Vec::new(),
    };
    let ok = GlowBeam { direction: 359, spread: 90 };
    let look = beam_look("Pixels(40)");
    for material in [ElementMaterial::Rock, ElementMaterial::Background] {
        assert_eq!(check(&lamp(material, ok), &look), None, "{material:?}");
    }
    for (beam, field) in [
        (GlowBeam { direction: 360, spread: 15 }, "glow.beam.direction"),
        (GlowBeam { direction: 0, spread: 0 }, "glow.beam.spread"),
        (GlowBeam { direction: 0, spread: 91 }, "glow.beam.spread"),
    ] {
        let e = check(&lamp(ElementMaterial::Rock, beam), &look).expect("refused");
        assert!(e.contains(&format!("elements[0].{field}")), "{e}");
    }
    // Dirt may not carry a beam.
    let e = check(&lamp(ElementMaterial::Dirt, ok), &look).expect("refused");
    assert!(e.contains("elements[0].glow.beam") && e.contains("dirt"), "{e}");
    // An image of dirt may not either.
    let mut c = lamp(ElementMaterial::Rock, ok);
    c.elements[0].kind = ElementKind::Image {
        material: ElementMaterial::Dirt,
        source: "a.png".into(),
        at: (0, 0),
        size: (2, 2),
        pixel_size: 1,
        colours: None,
        dither: olr_format::levels::Dither::None,
        alpha_threshold: 128,
        mask: false,
    };
    let e = check(&c, &look).expect("refused");
    assert!(e.contains("elements[0].glow.beam") && e.contains("dirt"), "{e}");
    // A distant element's glow stays undirected.
    let mut c = lamp(ElementMaterial::Background, ok);
    c.elements[0].distant = Some(50);
    let e = check(&c, &look).expect("refused");
    assert!(e.contains("elements[0].glow.beam") && e.contains("distant"), "{e}");
    // The group's reach must be Pixels.
    let e = check(&lamp(ElementMaterial::Rock, ok), &beam_look("Unlimited")).expect("refused");
    assert!(e.contains("elements[0]") && e.contains("lamp") && e.contains("Pixels"), "{e}");
    // Haze is a percent.
    let mut src = level(None);
    src.look = Some(LookSource { haze: 101, ..LookSource::default() });
    assert!(err(&src).contains("look.haze"), "{}", err(&src));
}

#[test]
fn hostile_glow_beam_maps() {
    let src = level(None);
    let grey_alpha = |w: u32, h: u32, f: &dyn Fn(usize) -> [u8; 2]| {
        let data: Vec<u8> = (0..(w * h) as usize).flat_map(f).collect();
        image_io::encode_grey_alpha(w, h, &data)
    };
    let zero = |_| [0u8, 0u8];
    let both = |front: Vec<u8>, back: Vec<u8>| {
        files(&src).with("look/glow-beam.png", front).with("look/glow-beam-back.png", back)
    };
    // The good pair reads.
    let good = both(grey_alpha(4, 3, &zero), grey_alpha(4, 3, &zero));
    assert!(read_level_files(&good).unwrap().glow_beams.is_some());
    // The wrong size, a byte 0 above 180, a spread above 90.
    let cases = [
        both(grey_alpha(5, 3, &zero), grey_alpha(4, 3, &zero)),
        both(grey_alpha(4, 3, &|i| if i == 2 { [181, 10] } else { [0, 0] }), grey_alpha(4, 3, &zero)),
        both(grey_alpha(4, 3, &zero), grey_alpha(4, 3, &|i| if i == 7 { [10, 91] } else { [0, 0] })),
        both(grey_alpha(4, 3, &|i| if i == 4 { [10, 0] } else { [0, 0] }), grey_alpha(4, 3, &zero)),
    ];
    for fs in &cases {
        let e = read_level_files(fs).unwrap_err().to_string();
        assert!(e.contains("look/glow-beam"), "{e}");
        let (back, warnings) = olr_format::levels::read_level_files_lenient(fs).unwrap();
        assert_eq!(back.glow_beams, None);
        assert!(warnings.iter().any(|w| w.contains("plays without its glow beams")), "{warnings:?}");
    }
    // One file alone is refused, and plays without.
    let one = files(&src).with("look/glow-beam.png", grey_alpha(4, 3, &zero));
    let e = read_level_files(&one).unwrap_err().to_string();
    assert!(e.contains("look/glow-beam-back.png"), "{e}");
    let (back, warnings) = olr_format::levels::read_level_files_lenient(&one).unwrap();
    assert_eq!(back.glow_beams, None);
    assert!(warnings.iter().any(|w| w.contains("plays without its glow beams")), "{warnings:?}");
    // A misfit haze map.
    let fs = files(&src).with("look/haze.png", image_io::encode_grey(5, 3, &[0; 15]));
    let e = read_level_files(&fs).unwrap_err().to_string();
    assert!(e.contains("look/haze.png"), "{e}");
    let (back, warnings) = olr_format::levels::read_level_files_lenient(&fs).unwrap();
    assert_eq!(back.haze, None);
    assert!(warnings.iter().any(|w| w.contains("plays without its haze map")), "{warnings:?}");
}
