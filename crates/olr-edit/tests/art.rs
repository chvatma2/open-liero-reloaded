//! A mod's images, glow masks and sounds (issue #19) on a copy of
//! Classic: users, import, replace, glow, rename and remove.

use olr_edit::mods::{self, ModError, art};
use olr_edit::pack::{self, Pack};
use olr_format::image_io;
use olr_vfs::VPath;

use crate::common::content;

fn classic() -> Option<Pack> {
    let root = content()?;
    Some(pack::load(&root.join("olx-classic")).unwrap())
}

fn bytes(p: &Pack, path: &str) -> Option<Vec<u8>> {
    p.other.get(&VPath::new(path).unwrap()).cloned()
}

fn refused(e: ModError) -> String {
    match e {
        ModError::Refused(m) => m,
        e => panic!("not a refusal: {e}"),
    }
}

fn png(w: u32, h: u32) -> Vec<u8> {
    image_io::encode_rgba(&olr_data::image::RgbaImage::new(w, h))
}

/// The bazooka's missile names its users for people, and is refused
/// removal while used, the refusal listing them.
#[test]
fn users_are_named_and_a_used_asset_stays() {
    let Some(mut p) = classic() else { return };
    let u = art::users(mods::source(&p).unwrap(), "gfx/missile.png");
    assert!(
        u.iter().any(|u| u.label == "Bazooka's shot" && u.path == "weapons/bazooka.fire.shot.projectile.gfx.file"),
        "{u:?}"
    );
    let s = art::users(mods::source(&p).unwrap(), "sfx/bazooka.ogg");
    assert!(s.iter().any(|u| u.label == "Bazooka's fire sound"), "{s:?}");
    let e = refused(art::remove(&mut p, "gfx/missile.png").unwrap_err());
    assert!(e.contains("in use by") && e.contains("Bazooka's shot"), "{e}");
    let e = refused(art::remove(&mut p, "sfx/bazooka.ogg").unwrap_err());
    assert!(e.contains("Bazooka's fire sound"), "{e}");
    assert!(bytes(&p, "gfx/missile.png").is_some() && bytes(&p, "sfx/bazooka.ogg").is_some());
    assert!(art::images(&p).contains(&"gfx/missile.png".to_string()));
    assert!(art::sounds(&p).contains(&"sfx/bazooka.ogg".to_string()));
}

/// Import checks what it adds; an unused file is removed with its mask.
#[test]
fn imports_are_checked_and_unused_files_removed() {
    let Some(mut p) = classic() else { return };
    art::import_image(&mut p, "gfx/new.png", &png(12, 4)).unwrap();
    assert_eq!(art::image_frames(&bytes(&p, "gfx/new.png").unwrap()).unwrap(), 3);
    for (path, b, why) in [
        ("gfx/new.png", png(4, 4), "exists"),
        ("gfx/tall.png", png(4, 8), "at least as wide"),
        ("gfx/bad.png", b"not a png".to_vec(), "not a PNG"),
        ("gfx/x.glow.png", png(4, 4), "not .glow.png"),
        ("gfx/a/b.png", png(4, 4), "without folders"),
        ("sfx/x.png", png(4, 4), "gfx/<name>.png"),
    ] {
        let e = refused(art::import_image(&mut p, path, &b).unwrap_err());
        assert!(e.contains(why), "{path}: {e}");
    }
    let ogg = bytes(&p, "sfx/bazooka.ogg").unwrap();
    art::import_sound(&mut p, "sfx/new.ogg", &ogg).unwrap();
    let e = refused(art::import_sound(&mut p, "sfx/text.ogg", b"hello").unwrap_err());
    assert!(e.contains("WAV or Ogg"), "{e}");
    art::set_glow(&mut p, "gfx/new.png", Some(&[255; 48])).unwrap();
    assert!(bytes(&p, "gfx/new.glow.png").is_some());
    // Unused, the image and its mask are warned about; used, neither.
    let warned = |p: &Pack, f: &str| mods::check(p).iter().any(|i| i.file.as_deref() == Some(f));
    assert!(mods::check(&p).iter().all(|i| !i.is_error()));
    assert!(warned(&p, "gfx/new.png") && warned(&p, "gfx/new.glow.png"));
    let mut used = p.clone();
    mods::set(&mut used, "weapons/bazooka.fire.shot.projectile.gfx.file", "gfx/new.png").unwrap();
    assert!(!warned(&used, "gfx/new.png") && !warned(&used, "gfx/new.glow.png"));
    art::remove(&mut p, "gfx/new.png").unwrap();
    art::remove(&mut p, "sfx/new.ogg").unwrap();
    assert!(bytes(&p, "gfx/new.png").is_none() && bytes(&p, "gfx/new.glow.png").is_none());
    assert!(bytes(&p, "sfx/new.ogg").is_none());
}

/// A glow mask round-trips; one of another size is refused; all zero
/// removes it.
#[test]
fn a_glow_mask_round_trips() {
    let Some(mut p) = classic() else { return };
    let (w, h) = image_io::png_size(&bytes(&p, "gfx/missile.png").unwrap()).unwrap();
    let n = (w * h) as usize;
    assert_eq!(art::glow(&p, "gfx/missile.png").unwrap(), vec![0; n]);
    let mut values = vec![0; n];
    values[3] = 255;
    values[7] = 128;
    art::set_glow(&mut p, "gfx/missile.png", Some(&values)).unwrap();
    assert_eq!(art::glow(&p, "gfx/missile.png").unwrap(), values);
    let e = refused(art::set_glow(&mut p, "gfx/missile.png", Some(&values[1..])).unwrap_err());
    assert!(e.contains("values for a"), "{e}");
    let wrong = olr_format::mods::encode_glow_mask((w + 1, h), &vec![255; n + h as usize]);
    let e = refused(art::set_glow_png(&mut p, "gfx/missile.png", &wrong).unwrap_err());
    assert!(e.contains("not its image's"), "{e}");
    assert_eq!(art::glow(&p, "gfx/missile.png").unwrap(), values, "a refusal changes nothing");
    // Saved and loaded again: the same mask, the pack checks clean.
    let tmp = tempfile::tempdir().unwrap();
    pack::save(tmp.path(), &p).unwrap();
    let back = pack::load(tmp.path()).unwrap();
    assert_eq!(art::glow(&back, "gfx/missile.png").unwrap(), values);
    art::set_glow(&mut p, "gfx/missile.png", Some(&vec![0; n])).unwrap();
    assert!(bytes(&p, "gfx/missile.glow.png").is_none());
}

/// Replacing an image reports its frame count before and after; a new
/// size drops a mask that no longer fits.
#[test]
fn replacing_an_image_reports_its_frames() {
    let Some(mut p) = classic() else { return };
    let old = bytes(&p, "gfx/missile.png").unwrap();
    let (w, h) = image_io::png_size(&old).unwrap();
    art::set_glow(&mut p, "gfx/missile.png", Some(&vec![255; (w * h) as usize])).unwrap();
    let same = art::set_image(&mut p, "gfx/missile.png", &old).unwrap();
    assert!(!same.changes_play() && same.frames_after == w / h);
    assert!(bytes(&p, "gfx/missile.glow.png").is_some());
    let fewer = art::set_image(&mut p, "gfx/missile.png", &png(h * 2, h)).unwrap();
    assert!(fewer.changes_play() && fewer.frames_after == 2, "{fewer:?}");
    assert!(bytes(&p, "gfx/missile.glow.png").is_none(), "the mask no longer fits");
}

/// A rename moves the file, its mask and every field naming it.
#[test]
fn a_rename_follows_its_users() {
    let Some(mut p) = classic() else { return };
    let (w, h) = image_io::png_size(&bytes(&p, "gfx/missile.png").unwrap()).unwrap();
    art::set_glow(&mut p, "gfx/missile.png", Some(&vec![64; (w * h) as usize])).unwrap();
    let users = art::users(mods::source(&p).unwrap(), "gfx/missile.png");
    art::rename(&mut p, "gfx/missile.png", "gfx/rocket.png").unwrap();
    assert!(bytes(&p, "gfx/missile.png").is_none() && bytes(&p, "gfx/missile.glow.png").is_none());
    assert!(bytes(&p, "gfx/rocket.png").is_some() && bytes(&p, "gfx/rocket.glow.png").is_some());
    let after = art::users(mods::source(&p).unwrap(), "gfx/rocket.png");
    assert_eq!(after, users);
    assert_eq!(mods::get(mods::source(&p).unwrap(), &users[0].path).unwrap(), "\"gfx/rocket.png\"");
    art::rename(&mut p, "sfx/bazooka.ogg", "sfx/boom.ogg").unwrap();
    assert_eq!(
        mods::get(mods::source(&p).unwrap(), "weapons/bazooka.fire.sound").unwrap(),
        "\"sfx/boom.ogg\""
    );
    let e = refused(art::rename(&mut p, "sfx/boom.ogg", "sfx/exp2.ogg").unwrap_err());
    assert!(e.contains("exists"), "{e}");
    assert!(mods::check(&p).iter().all(|i| !i.is_error()));
}
