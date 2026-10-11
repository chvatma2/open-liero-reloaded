//! Theme packs: `theme.ron`, `holes/<n>.png`, `stones/<n>.png`, `misc/<n>.png`
//! (numbered from 1), `front_tile.png`, `back_tile.png`, and optionally
//! `green_ball.png`.

use rl_data::image::RgbaImage;
use rl_data::manifest::{PackKind, PackManifest};
use rl_data::theme::{Theme, ThemeDef};
use rl_vfs::{ContentSink, ContentSource, VPath};

use crate::image_io::{decode_rgba, encode_rgba};
use crate::ron_io::{read_ron, write_ron};
use crate::{MANIFEST, PackError, read_manifest, write_manifest};

pub const THEME_FILE: &str = "theme.ron";
const GREEN_BALL: &str = "green_ball.png";

#[derive(Clone, Debug, PartialEq)]
pub struct ThemePack {
    pub manifest: PackManifest,
    pub theme: Theme,
}

fn put(sink: &mut dyn ContentSink, file: &str, img: &RgbaImage) -> Result<(), PackError> {
    sink.write(&VPath::new(file)?, &encode_rgba(img))?;
    Ok(())
}

fn get(src: &dyn ContentSource, file: &str) -> Result<RgbaImage, PackError> {
    decode_rgba(&crate::ron_io::read_file(src, &VPath::new(file)?)?)
        .map_err(|message| PackError::Image { file: file.into(), message })
}

pub fn write_theme(sink: &mut dyn ContentSink, pack: &ThemePack) -> Result<(), PackError> {
    if pack.manifest.kind != PackKind::Theme {
        return Err(PackError::invalid(MANIFEST, "manifest kind is not Theme"));
    }
    let t = &pack.theme;
    let counts = (t.holes.len() as u32, t.stones.len() as u32, t.misc.len() as u32);
    if counts != (t.def.holes, t.def.stones, t.def.misc) {
        return Err(PackError::invalid(THEME_FILE, "image counts do not match theme.ron"));
    }
    write_manifest(sink, &pack.manifest)?;
    write_ron(sink, THEME_FILE, &t.def)?;
    for (dir, imgs) in [("holes", &t.holes), ("stones", &t.stones), ("misc", &t.misc)] {
        for (i, img) in imgs.iter().enumerate() {
            put(sink, &format!("{dir}/{}.png", i + 1), img)?;
        }
    }
    put(sink, "front_tile.png", &t.front_tile)?;
    put(sink, "back_tile.png", &t.back_tile)?;
    if let Some(img) = &t.green_ball {
        put(sink, GREEN_BALL, img)?;
    }
    Ok(())
}

pub fn read_theme(src: &dyn ContentSource) -> Result<ThemePack, PackError> {
    let manifest = read_manifest(src)?;
    if manifest.kind != PackKind::Theme {
        return Err(PackError::invalid(MANIFEST, "manifest kind is not Theme"));
    }
    let def: ThemeDef = read_ron(src, THEME_FILE)?;
    let list = |dir: &str, n: u32| {
        (1..=n).map(|i| get(src, &format!("{dir}/{i}.png"))).collect::<Result<Vec<_>, _>>()
    };
    let theme = Theme {
        holes: list("holes", def.holes)?,
        stones: list("stones", def.stones)?,
        misc: list("misc", def.misc)?,
        front_tile: get(src, "front_tile.png")?,
        back_tile: get(src, "back_tile.png")?,
        green_ball: match src.exists(&VPath::new(GREEN_BALL)?) {
            true => Some(get(src, GREEN_BALL)?),
            false => None,
        },
        def,
    };
    Ok(ThemePack { manifest, theme })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mod_pack::tests::manifest;
    use rl_vfs::MemFs;

    #[test]
    fn roundtrip() {
        let img = RgbaImage::new(2, 2);
        let pack = ThemePack {
            manifest: manifest(PackKind::Theme),
            theme: Theme {
                def: ThemeDef { name: "Dirt".into(), holes: 2, stones: 1, misc: 0 },
                holes: vec![img.clone(), img.clone()],
                stones: vec![img.clone()],
                misc: vec![],
                front_tile: img.clone(),
                back_tile: img.clone(),
                green_ball: None,
            },
        };
        let mut fs = MemFs::new();
        write_theme(&mut fs, &pack).unwrap();
        assert!(fs.exists(&VPath::new("holes/2.png").unwrap()));
        assert!(!fs.exists(&VPath::new("green_ball.png").unwrap()));
        assert_eq!(read_theme(&fs).unwrap(), pack);
        let pack = ThemePack { theme: Theme { green_ball: Some(img), ..pack.theme }, ..pack };
        let mut fs = MemFs::new();
        write_theme(&mut fs, &pack).unwrap();
        assert!(fs.exists(&VPath::new("green_ball.png").unwrap()));
        assert_eq!(read_theme(&fs).unwrap(), pack);
    }
}
