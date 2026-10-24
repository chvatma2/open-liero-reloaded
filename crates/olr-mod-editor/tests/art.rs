//! Issue #19: the Art and Sounds sections and the Look group, on a copy of
//! Classic. Through the panels (`egui_kittest`): a frame painted in colour
//! and glow and saved, the PNG changed only there and the mask read back
//! (`paint_a_frame_and_save_it`); the projectile's and weapon's Look. On
//! the state: new images, frames added and removed (asked), renames,
//! imports through the browser, a replaced image, removal refused while
//! used, sounds imported, played and removed; a bad image or mask never
//! crashes.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use olr_data::image::Rgba;
use olr_edit::mods::{self, art};
use olr_format::image_io;
use olr_format::look::{LightSource, ReachSource};
use olr_mod_editor::ui::labels;
use olr_mod_editor::{
    ArtAction, BrowseAction, Dialog, FileEntry, Import, Item, ModAction as A, ModCommand as C,
};
use olr_vfs::VPath;

use crate::common::{CLASSIC, editor, installed};
use crate::flows::{App, click, harness, type_into};

const MISSILE: &str = "gfx/missile.png";

fn bytes_of(p: &olr_edit::pack::Pack, path: &str) -> Vec<u8> {
    p.other.get(&VPath::new(path).unwrap()).cloned().unwrap_or_default()
}

/// The screen point of pixel `(x, y)` of the frame on the canvas.
fn at(h: &Harness<'static, App>, x: i32, y: i32) -> egui::Pos2 {
    let s = &h.state().state;
    let rect = h.state().data.art_canvas().expect("the canvas was drawn");
    let side = s.art_doc().unwrap().side() as f32;
    let (z, (px, py)) = (s.tools().zoom, s.tools().pan);
    let o = rect.center() - egui::vec2(side * z / 2.0, side * z / 2.0) + egui::vec2(px, py);
    let o = egui::pos2(o.x.round(), o.y.round());
    o + egui::vec2((x as f32 + 0.5) * z, (y as f32 + 0.5) * z)
}

/// A click of the canvas at pixel `(x, y)`: a stroke of one dab.
fn dab(h: &mut Harness<'static, App>, x: i32, y: i32) {
    let pos = at(h, x, y);
    h.event(egui::Event::PointerMoved(pos));
    h.run();
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        h.run();
    }
}

/// The pixels of two same-sized images that differ.
fn differing(a: &[u8], b: &[u8]) -> Vec<(u32, u32)> {
    let (a, b) = (image_io::decode_rgba(a).unwrap(), image_io::decode_rgba(b).unwrap());
    assert_eq!((a.width(), a.height()), (b.width(), b.height()));
    let mut out = Vec::new();
    for y in 0..a.height() {
        for x in 0..a.width() {
            if a.get(x, y) != b.get(x, y) {
                out.push((x, y));
            }
        }
    }
    out
}

#[test]
fn paint_a_frame_and_save_it() {
    let Some(state) = editor() else { return };
    let original = bytes_of(&installed(CLASSIC).unwrap(), MISSILE);
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("my-classic");
    std::fs::create_dir_all(&dir).unwrap();
    let mut h = harness(state, dir.clone());

    // The image (the Art list's entry is below the weapons, out of the
    // window): its users and frames.
    h.get_by_label(&labels::image(MISSILE));
    h.state_mut().state.update(A::Select(Item::Art(MISSILE.into())));
    h.run();
    assert_eq!(*h.state().state.item(), Item::Art(MISSILE.into()));
    h.get_by_label_contains("Bazooka's shot");
    assert_eq!(h.state().state.art_doc().unwrap().frames(), 18);
    click(&mut h, &labels::frame(2));
    assert_eq!(h.state().state.tools().frame, 2);

    // Red at (5, 5) of the third frame: one step, that pixel alone.
    type_into(&mut h, labels::HEX, "#ff0000");
    assert_eq!(h.state().state.tools().colour, Rgba::rgb(255, 0, 0));
    dab(&mut h, 5, 5);
    let painted = bytes_of(h.state().state.pack(), MISSILE);
    assert_eq!(differing(&original, &painted), [(2 * 12 + 5, 5)]);
    let img = image_io::decode_rgba(&painted).unwrap();
    assert_eq!(img.get(29, 5), Rgba::rgb(255, 0, 0));
    assert_eq!(h.state().state.tools().recent, [Rgba::rgb(255, 0, 0)]);
    h.get_by_label(labels::UNSAVED);

    // Its glow, at full and half strength: the mask beside the image.
    click(&mut h, labels::TARGET_GLOW);
    dab(&mut h, 5, 5);
    h.get_by_label("50 %").click();
    h.run();
    dab(&mut h, 4, 4);
    let glow = art::glow(h.state().state.pack(), MISSILE).unwrap();
    let lit: Vec<(usize, u8)> = glow.iter().copied().enumerate().filter(|(_, g)| *g > 0).collect();
    let opaque = image_io::decode_rgba(&original).unwrap().get(28, 4).a != 0;
    let mut want = vec![(5 * 216 + 29, 255)];
    if opaque {
        want.insert(0, (4 * 216 + 28, 128));
    }
    assert_eq!(lit, want, "glow only on opaque pixels");
    // The painting was three steps; the colour image did not change for glow.
    assert_eq!(bytes_of(h.state().state.pack(), MISSILE), painted);

    // Saved and read back: the PNG changed only there, the mask kept.
    click(&mut h, labels::SAVE);
    assert!(h.query_by_label(labels::UNSAVED).is_none(), "{}", h.state().state.status());
    let saved = olr_edit::pack::load(&dir).unwrap();
    assert_eq!(differing(&original, &bytes_of(&saved, MISSILE)), [(29, 5)]);
    assert_eq!(art::glow(&saved, MISSILE).unwrap(), glow);
    assert!(mods::check(&saved).iter().all(|i| !i.is_error()));

    // Undo takes the glow and the colour back, to Classic's own bytes.
    for _ in 0..3 {
        click(&mut h, labels::UNDO);
    }
    assert_eq!(bytes_of(h.state().state.pack(), MISSILE), original);
    assert!(art::glow(h.state().state.pack(), MISSILE).unwrap().iter().all(|&g| g == 0));
    assert_eq!(
        h.state().state.art_doc().unwrap().frame(2).px[5 * 12 + 5].r,
        image_io::decode_rgba(&original).unwrap().get(29, 5).r
    );
}

#[test]
fn new_images_frames_renames_and_imports() {
    let Some(mut s) = editor() else { return };
    s.update(A::AskNewImage(None));
    assert_eq!(s.dialog(), Some(&Dialog::NewImage { copy_of: None }));
    s.update(A::NewImage { name: "tip".into(), side: 6, frames: 2, copy_of: None });
    assert_eq!(s.dialog(), None);
    assert_eq!(*s.item(), Item::Art("gfx/tip.png".into()));
    assert_eq!(s.art_doc().unwrap().frames(), 2);
    // An unused image is a warning, not an error.
    assert_eq!(s.errors(), 0);

    // Add frame asks, then makes three; undo makes two again.
    s.update(A::Art(ArtAction::AddFrame));
    assert_eq!(s.dialog(), Some(&Dialog::FrameCount { image: "gfx/tip.png".into(), before: 2, after: 3 }));
    s.update(A::Confirm(true));
    assert_eq!(s.art_doc().unwrap().frames(), 3);
    assert!(s.status().contains("Play"), "{}", s.status());
    s.update(A::Art(ArtAction::RemoveFrame));
    s.update(A::Confirm(false));
    assert_eq!(s.art_doc().unwrap().frames(), 3, "cancelled");
    s.update(A::Undo);
    assert_eq!(s.art_doc().unwrap().frames(), 2);

    // A copy keeps the glow.
    s.update(A::Art(ArtAction::Target(olr_mod_editor::art::Target::Glow)));
    s.update(A::Art(ArtAction::Tool(olr_mod_editor::art::Tool::Pencil)));
    s.update(A::Art(ArtAction::Press { x: 0, y: 0 }));
    s.update(A::Art(ArtAction::Release));
    assert!(art::glow(s.pack(), "gfx/tip.png").unwrap().iter().all(|&g| g == 0), "no glow on a clear pixel");
    s.update(A::Art(ArtAction::Target(olr_mod_editor::art::Target::Colour)));
    s.update(A::Art(ArtAction::Press { x: 0, y: 0 }));
    s.update(A::Art(ArtAction::Release));
    s.update(A::Art(ArtAction::Target(olr_mod_editor::art::Target::Glow)));
    s.update(A::Art(ArtAction::Press { x: 0, y: 0 }));
    s.update(A::Art(ArtAction::Release));
    assert_eq!(art::glow(s.pack(), "gfx/tip.png").unwrap()[0], 255);
    s.update(A::AskNewImage(Some("gfx/tip.png".into())));
    s.update(A::NewImage {
        name: "tip2.png".into(),
        side: 1,
        frames: 1,
        copy_of: Some("gfx/tip.png".into()),
    });
    assert_eq!(*s.item(), Item::Art("gfx/tip2.png".into()));
    assert_eq!(art::glow(s.pack(), "gfx/tip2.png"), art::glow(s.pack(), "gfx/tip.png"));

    // Renamed with its mask.
    s.update(A::AskRename("gfx/tip2.png".into()));
    s.update(A::RenameAsset { from: "gfx/tip2.png".into(), to: "gfx/point".into() });
    assert_eq!(*s.item(), Item::Art("gfx/point.png".into()));
    assert!(s.asset("gfx/point.glow.png").is_some() && s.asset("gfx/tip2.glow.png").is_none());

    // Import PNG… through the browser: a folder listed, entered, a file picked.
    let cmds = s.update(A::Import(Import::Image));
    let C::List(home) = &cmds[0] else { panic!("{cmds:?}") };
    let home = home.clone();
    let e = |n: &str, d| FileEntry { name: n.into(), is_dir: d };
    s.update(A::Listed { dir: home.clone(), entries: Ok(vec![e("art", true), e("notes.txt", false)]) });
    assert_eq!(s.browser().unwrap().shown().len(), 1, "folders and PNGs only");
    let cmds = s.update(A::Browse(BrowseAction::Pick("art".into())));
    assert_eq!(cmds, [C::List(home.join("art"))]);
    s.update(A::Listed { dir: home.join("art"), entries: Ok(vec![e("My Shot.png", false)]) });
    s.update(A::Browse(BrowseAction::Select("My Shot.png".into())));
    let cmds = s.update(A::Browse(BrowseAction::Choose));
    assert_eq!(cmds, [C::ReadFile { import: Import::Image, path: home.join("art").join("My Shot.png") }]);
    assert_eq!((s.dialog(), s.browser().is_none()), (None, true));
    let png = art::blank_image(4, 3).unwrap();
    s.update(A::FileRead { import: Import::Image, name: "My Shot.png".into(), bytes: png.clone() });
    assert_eq!(*s.item(), Item::Art("gfx/my-shot.png".into()));
    // The same name again gets a number.
    s.update(A::FileRead { import: Import::Image, name: "My Shot.png".into(), bytes: png.clone() });
    assert_eq!(*s.item(), Item::Art("gfx/my-shot-2.png".into()));
    // The browser opens where the last file came from.
    let cmds = s.update(A::Import(Import::Sound));
    assert_eq!(cmds, [C::List(home.join("art"))]);
    s.update(A::Confirm(false));
    assert!(s.browser().is_none());

    // A bad file is refused, never a crash.
    s.update(A::FileRead { import: Import::Image, name: "bad.png".into(), bytes: b"not a png".to_vec() });
    assert!(matches!(s.dialog(), Some(Dialog::Error(e)) if e.contains("bad.png")), "{:?}", s.dialog());
    s.update(A::Confirm(true));
    // Replace with PNG…: the same frame count at once, another asked first.
    s.update(A::FileRead {
        import: Import::Replace("gfx/my-shot.png".into()),
        name: "x.png".into(),
        bytes: art::blank_image(5, 3).unwrap(),
    });
    assert_eq!(s.art_doc().unwrap().size(), (15, 5));
    s.update(A::FileRead {
        import: Import::Replace("gfx/my-shot.png".into()),
        name: "x.png".into(),
        bytes: art::blank_image(5, 1).unwrap(),
    });
    assert_eq!(
        s.dialog(),
        Some(&Dialog::FrameCount { image: "gfx/my-shot.png".into(), before: 3, after: 1 })
    );
    s.update(A::Confirm(true));
    assert_eq!(s.art_doc().unwrap().frames(), 1);
}

#[test]
fn removal_is_refused_while_used() {
    let Some(mut s) = editor() else { return };
    s.update(A::Select(Item::Art(MISSILE.into())));
    s.update(A::RemoveAsset(MISSILE.into()));
    let Some(Dialog::Error(why)) = s.dialog().cloned() else { panic!("{:?}", s.dialog()) };
    assert!(why.contains("Bazooka's shot") && why.contains(".gfx.file"), "{why}");
    assert!(s.asset(MISSILE).is_some() && !s.can_undo());
    s.update(A::Confirm(true));
    // Unused, it goes, with its mask; undo brings both back.
    s.update(A::AskNewImage(None));
    s.update(A::NewImage { name: "spare".into(), side: 4, frames: 1, copy_of: None });
    let path = "gfx/spare.png";
    assert_eq!(*s.item(), Item::Art(path.into()));
    // A name taken is refused, the dialog left open.
    s.update(A::AskNewImage(None));
    s.update(A::NewImage { name: "spare".into(), side: 4, frames: 1, copy_of: None });
    assert!(s.status().contains("exists"), "{}", s.status());
    s.update(A::Confirm(false));
    s.update(A::Art(ArtAction::Press { x: 1, y: 1 }));
    s.update(A::Art(ArtAction::Release));
    s.update(A::Art(ArtAction::Target(olr_mod_editor::art::Target::Glow)));
    s.update(A::Art(ArtAction::Press { x: 1, y: 1 }));
    s.update(A::Art(ArtAction::Release));
    assert!(s.asset("gfx/spare.glow.png").is_some());
    s.update(A::RemoveAsset(path.into()));
    assert!(s.asset(path).is_none() && s.asset("gfx/spare.glow.png").is_none() && s.dialog().is_none());
    assert_ne!(*s.item(), Item::Art(path.into()));
    s.update(A::Undo);
    assert!(s.asset(path).is_some() && s.asset("gfx/spare.glow.png").is_some());
}

/// A short mono 16-bit WAV.
fn wav(n: u32) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + n * 2).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&8000u32.to_le_bytes());
    b.extend_from_slice(&16000u32.to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&(n * 2).to_le_bytes());
    for i in 0..n {
        b.extend_from_slice(&(((i as f32 * 0.3).sin() * 6000.0) as i16).to_le_bytes());
    }
    b
}

#[test]
fn sounds_are_imported_played_and_removed() {
    let Some(mut s) = editor() else { return };
    let boom = wav(400);
    s.update(A::FileRead { import: Import::Sound, name: "Big Boom.WAV".into(), bytes: boom.clone() });
    assert_eq!(*s.item(), Item::Sounds);
    assert_eq!(s.asset("sfx/big-boom.wav"), Some(&boom[..]));
    assert_eq!(
        s.update(A::PlaySound("sfx/big-boom.wav".into())),
        [C::PlaySound { path: "sfx/big-boom.wav".into(), bytes: boom.clone() }]
    );
    // Not a sound: refused.
    s.update(A::FileRead { import: Import::Sound, name: "x.ogg".into(), bytes: b"not a sound".to_vec() });
    assert!(matches!(s.dialog(), Some(Dialog::Error(_))));
    s.update(A::Confirm(true));
    // Used by the bazooka's fire: not removed; renamed, the field follows.
    s.update(A::Set { path: "weapons/bazooka.fire.sound".into(), value: "\"sfx/big-boom.wav\"".into() });
    s.update(A::RemoveAsset("sfx/big-boom.wav".into()));
    assert!(
        matches!(s.dialog(), Some(Dialog::Error(e)) if e.contains("Bazooka's fire sound")),
        "{:?}",
        s.dialog()
    );
    s.update(A::Confirm(true));
    s.update(A::RenameAsset { from: "sfx/big-boom.wav".into(), to: "sfx/boom.wav".into() });
    assert_eq!(s.value("weapons/bazooka.fire.sound").as_deref(), Some("\"sfx/boom.wav\""));
    assert!(s.asset("sfx/big-boom.wav").is_none());
}

#[test]
fn the_look_group_edits_lights() {
    let Some(state) = editor() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let mut h = harness(state, tmp.path().to_path_buf());
    click(&mut h, "Bazooka");
    click(&mut h, &labels::node("Shot"));
    click(&mut h, labels::GO_PROJECTILE);
    let look = "weapons/bazooka.fire.shot.projectile.look";
    assert_eq!(h.state().state.value(look).as_deref(), Some("None"));
    click(&mut h, labels::LIGHT);
    type_into(&mut h, &labels::look(labels::LIGHT, "intensity (%)"), "80");
    type_into(&mut h, &labels::look(labels::LIGHT, "colour"), "#ffa040");
    type_into(&mut h, &labels::look(labels::LIGHT, "reach (px)"), "44");
    let light: Option<LightSource> =
        olr_mod_editor::state::get_as(h.state().state.src(), &format!("{look}.light")).flatten();
    let light = light.expect("a light");
    assert_eq!((light.intensity, light.reach), (80, ReachSource::Pixels(44)));
    assert_eq!(light.colour.to_hex(), "#ffa040");
    click(&mut h, labels::HAS_GLOW);
    assert_eq!(h.state().state.value(&format!("{look}.glow")).as_deref(), Some("100"));
    click(&mut h, labels::EXPLOSION);
    assert!(h.state().state.value(&format!("{look}.explosion")).is_some_and(|v| v.contains("radius")));
    // Every step undoes; the mod checks clean.
    assert_eq!(h.state().state.errors(), 0, "{:?}", h.state().state.issues());

    // The weapon's muzzle light.
    click(&mut h, &labels::node("Bazooka"));
    click(&mut h, labels::MUZZLE_LIGHT);
    let muzzle = h.state().state.value("weapons/bazooka.look").unwrap_or_default();
    assert!(muzzle.contains("muzzle_light"), "{muzzle}");
    click(&mut h, labels::MUZZLE_LIGHT);
    assert_eq!(h.state().state.value("weapons/bazooka.look").as_deref(), Some("None"));
}

/// Issue #42: beside the explosion light, a projectile's explosion heat
/// (its amount, its own radius or the engine's) and its heat a tick, and
/// the mod's own explosion light and heat on the World panel, each within
/// its bounds, the mod checking clean.
#[test]
fn the_look_group_edits_heat() {
    use olr_format::look::{
        ExplosionHeatSource, ModWideLookSource, ProjectileHeatSource, ProjectileLookSource,
    };
    let Some(state) = editor() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let mut h = harness(state, tmp.path().to_path_buf());
    click(&mut h, "Bazooka");
    click(&mut h, &labels::node("Shot"));
    click(&mut h, labels::GO_PROJECTILE);
    let path = "weapons/bazooka.fire.shot.projectile";
    let look = |h: &Harness<'static, App>| -> ProjectileLookSource {
        olr_mod_editor::state::get_as(h.state().state.src(), &format!("{path}.look"))
            .flatten()
            .unwrap_or_default()
    };
    let heat = labels::EXPLOSION_HEAT;
    click(&mut h, heat);
    assert_eq!(
        look(&h).explosion_heat,
        Some(ExplosionHeatSource { amount: 100, radius: None }),
        "the engine's"
    );
    type_into(&mut h, &labels::look(heat, "amount (%)"), "250");
    click(&mut h, &labels::look(heat, labels::OWN_RADIUS));
    type_into(&mut h, &labels::look(heat, "radius (px)"), "30");
    assert_eq!(look(&h).explosion_heat, Some(ExplosionHeatSource { amount: 250, radius: Some(30) }));
    // The bounds hold: a radius past the engine's widest is the widest.
    type_into(&mut h, &labels::look(heat, "radius (px)"), "90");
    assert_eq!(look(&h).explosion_heat.unwrap().radius, Some(48));
    click(&mut h, labels::HEAT);
    type_into(&mut h, &labels::look(labels::HEAT, "amount a tick"), "7");
    type_into(&mut h, &labels::look(labels::HEAT, "radius (px)"), "3");
    assert_eq!(look(&h).heat, Some(ProjectileHeatSource { amount: 7, radius: 3 }));
    type_into(&mut h, &labels::look(labels::HEAT, "amount a tick"), "500");
    assert_eq!(look(&h).heat.unwrap().amount, 100, "at most the cap");
    assert!(look(&h).explosion.is_none(), "heat is not light");
    assert_eq!(h.state().state.errors(), 0, "{:?}", h.state().state.issues());
    // Switched off again, the look is none.
    click(&mut h, heat);
    click(&mut h, labels::HEAT);
    assert_eq!(h.state().state.value(&format!("{path}.look")).as_deref(), Some("None"));

    // The mod's own, on the World panel: `mod.ron`'s look.
    click(&mut h, labels::WORLD);
    let wide = |h: &Harness<'static, App>| h.state().state.src().look.clone().unwrap_or_default();
    click(&mut h, heat);
    type_into(&mut h, &labels::look(heat, "amount (%)"), "40");
    click(&mut h, labels::EXPLOSION);
    let w: ModWideLookSource = wide(&h);
    assert_eq!(w.explosion_heat, Some(ExplosionHeatSource { amount: 40, radius: None }));
    assert!(w.explosion.is_some_and(|e| e.radius == 7));
    assert_eq!(h.state().state.errors(), 0, "{:?}", h.state().state.issues());
    let compiled = olr_format::mods::compile_mod_look(h.state().state.src()).unwrap();
    assert_eq!(compiled.explosion_heat, Some(olr_look::ExplosionHeatLook { amount: 40, radius: None }));
    click(&mut h, heat);
    click(&mut h, labels::EXPLOSION);
    assert!(h.state().state.src().look.is_none(), "all default is none");
}

/// Issue #69: a Beam weapon's Look group has Beam light, Beam tip light
/// and Beam heat, each switched on from the engine's values in the beam's
/// colour, edited within the format's bounds and set whole as the
/// weapon's look; a projectile weapon has none of them.
#[test]
fn the_look_group_edits_beam() {
    use olr_format::look::{BeamHeatSource, WeaponLookSource};
    let Some(state) = editor() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let mut h = harness(state, tmp.path().to_path_buf());
    click(&mut h, "Bazooka");
    assert!(h.query_by_label(labels::MUZZLE_LIGHT).is_some());
    assert!(h.query_by_label(labels::BEAM_LIGHT).is_none(), "not on a projectile weapon");
    click(&mut h, "Laser");
    let look = |h: &Harness<'static, App>| -> WeaponLookSource {
        olr_mod_editor::state::get_as(h.state().state.src(), "weapons/laser.look")
            .flatten()
            .unwrap_or_default()
    };
    click(&mut h, labels::BEAM_LIGHT);
    let light = look(&h).beam.and_then(|b| b.light).expect("a beam light");
    assert_eq!((light.colour.to_hex().as_str(), light.intensity), ("#ff0000", 21), "the engine's, in red");
    assert_eq!(light.reach, ReachSource::Pixels(40));
    type_into(&mut h, &labels::look(labels::BEAM_LIGHT, "reach (px)"), "999");
    let reach = look(&h).beam.and_then(|b| b.light).unwrap().reach;
    assert_eq!(reach, ReachSource::Pixels(olr_look::engine::MAX_BEAM_REACH), "at most the cap");
    assert!(h.query_by_label(&labels::look(labels::BEAM_LIGHT, labels::UNLIMITED)).is_none());
    click(&mut h, labels::BEAM_TIP_LIGHT);
    let tip = look(&h).beam.and_then(|b| b.tip_light).expect("a tip light");
    assert_eq!(tip.reach, ReachSource::Pixels(90));
    assert!(tip.intensity > 250, "{tip:?}");
    click(&mut h, labels::BEAM_HEAT);
    assert_eq!(look(&h).beam.unwrap().heat, Some(BeamHeatSource { amount: 60, radius: 5 }), "the engine's");
    type_into(&mut h, &labels::look(labels::BEAM_HEAT, "amount a sub-step"), "0");
    type_into(&mut h, &labels::look(labels::BEAM_HEAT, "radius (px)"), "40");
    assert_eq!(
        look(&h).beam.unwrap().heat,
        Some(BeamHeatSource { amount: 0, radius: 16 }),
        "0 is none; 16 the cap"
    );
    assert_eq!(h.state().state.errors(), 0, "{:?}", h.state().state.issues());
    let compiled = olr_format::mods::compile_mod_look(h.state().state.src()).unwrap();
    let laser = h.state().state.src().weapons.iter().position(|(w, _)| w == "laser").unwrap();
    let beam = compiled.weapons[laser].as_ref().and_then(|w| w.beam.as_ref()).expect("compiled");
    assert_eq!(beam.heat, Some(olr_look::BeamHeat { amount: 0, radius: 16 }));
    // The muzzle light keeps the beam's; all switched off, the look is none.
    click(&mut h, labels::MUZZLE_LIGHT);
    assert!(look(&h).muzzle_light.is_some() && look(&h).beam.is_some());
    for part in [labels::MUZZLE_LIGHT, labels::BEAM_LIGHT, labels::BEAM_TIP_LIGHT, labels::BEAM_HEAT] {
        click(&mut h, part);
    }
    assert_eq!(h.state().state.value("weapons/laser.look").as_deref(), Some("None"));
}

/// A broken image and a mask of the wrong size open, list and draw
/// without a crash: the image says why it does not paint, the mask is
/// noted and painting glow replaces it with one that fits.
#[test]
fn a_bad_image_or_mask_never_crashes() {
    let Some(mut pack) = crate::common::classic_copy() else { return };
    pack.other.insert(VPath::new("gfx/broken.png").unwrap(), b"\x89PNG\r\n\x1a\nnot really".to_vec());
    pack.other.insert(VPath::new("gfx/missile.glow.png").unwrap(), art::blank_image(3, 1).unwrap());
    let base = mods::source(&installed(CLASSIC).unwrap()).unwrap().clone();
    let state = olr_mod_editor::ModEditorState::new(pack, Some(base), Vec::new(), false).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let mut h = harness(state, tmp.path().to_path_buf());
    h.run();
    h.state_mut().state.update(A::Select(Item::Art("gfx/broken.png".into())));
    h.run();
    assert!(matches!(h.state().state.art(), Some(Err(_))), "the broken image does not paint");
    assert!(h.state().state.art_doc().is_none());
    h.state_mut().state.update(A::Art(ArtAction::Press { x: 1, y: 1 }));
    h.state_mut().state.update(A::Art(ArtAction::Release));
    h.state_mut().state.update(A::Select(Item::Art(MISSILE.into())));
    h.run();
    let doc = h.state().state.art_doc().expect("the missile paints");
    assert!(doc.mask_note().is_some(), "the mask that does not fit is noted");
    // Glow paints only where there is colour.
    let frame = doc.frame(0);
    let i = frame.px.iter().position(|p| p.a != 0).expect("an opaque pixel") as i32;
    let (x, y) = (i % frame.w as i32, i / frame.w as i32);
    let s = &mut h.state_mut().state;
    s.update(A::Art(ArtAction::Target(olr_mod_editor::art::Target::Glow)));
    s.update(A::Art(ArtAction::Press { x, y }));
    s.update(A::Art(ArtAction::Release));
    let (w, hh) = image_io::png_size(&bytes_of(s.pack(), MISSILE)).unwrap();
    let mask = bytes_of(s.pack(), "gfx/missile.glow.png");
    assert_eq!(image_io::png_size(&mask), Some((w, hh)), "the new mask fits");
    h.run();
}
