//! Byte identity of lit frames (issue #34): a bot match drawn lit on the
//! software adapter, frame by frame, and a hash of the exact bytes of each
//! frame's light grid (`Renderer::light_grid`, `Rgba16Float`) and lit
//! target (`Renderer::view_lit_target`, the composite before the upscale),
//! so a change can prove it leaves the picture bit-identical, stronger than
//! the render goldens' perceptual tolerance. `olr-cli frame-hashes` writes
//! them; `fixtures/render/lit-hashes.ron` is one lit match's, checked by
//! `tests/render_golden.rs` (`cargo xtask check --render`).

use anyhow::{Context as _, ensure};
use olr_content::Content;
use olr_present::{PresentSetup, Presentation, default_colour};
use olr_render::{Offscreen, RenderSettings, UpscaleFilter};
use olr_scene::{DrawList, Interpolation, SceneOptions, Viewport};
use serde::{Deserialize, Serialize};

use crate::assets::LevelAssets;
use crate::runner::{Match, Options};
use crate::spec::MatchSpec;

/// FNV-1a (64 bits) of `bytes`: every step is a bijection of the state, so
/// two inputs of one length that differ in a single byte (one texel's
/// channel) never hash alike.
pub fn hash_bytes(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3))
}

/// A hash as the fixture writes it: 16 hex digits.
pub fn hex(hash: u64) -> String {
    format!("{hash:016x}")
}

/// One drawn frame.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameHash {
    /// The match's tick drawn (the state after it).
    pub tick: u32,
    /// The world's checksum then: a frame whose world differs is another
    /// match (the simulation or the bots changed), not another picture.
    pub world: String,
    /// The light grid's bytes, [`hash_bytes`].
    pub light: String,
    /// The lit target's bytes.
    pub lit: String,
    /// The glowing discs (play's lights) drawn: said, the world decides it.
    pub discs: usize,
}

/// What `olr-cli frame-hashes` writes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FrameHashes {
    /// The adapter that drew them: another rasteriser may round otherwise,
    /// so only the same adapter's hashes compare.
    pub adapter: String,
    pub spec: MatchSpec,
    /// The light grid's and the lit target's sizes (texels).
    pub light_size: (u32, u32),
    pub lit_size: (u32, u32),
    /// The last `frames.len()` ticks of the match, each drawn.
    pub frames: Vec<FrameHash>,
}

/// How two recordings compare.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Comparison {
    Same,
    /// Drawn on another adapter: not compared.
    OtherAdapter(String),
    /// The first difference, said.
    Differs(String),
}

/// `actual` against the `committed` recording.
pub fn compare(committed: &FrameHashes, actual: &FrameHashes) -> Comparison {
    if committed.adapter != actual.adapter {
        return Comparison::OtherAdapter(format!(
            "the hashes were recorded on {}, this is {}: not compared",
            committed.adapter, actual.adapter
        ));
    }
    if committed.spec != actual.spec || committed.frames.len() != actual.frames.len() {
        return Comparison::Differs("another match or frame count than the recording's".into());
    }
    if (committed.light_size, committed.lit_size) != (actual.light_size, actual.lit_size) {
        return Comparison::Differs(format!(
            "the light grid is {:?} and the lit target {:?} texels, not {:?} and {:?}",
            actual.light_size, actual.lit_size, committed.light_size, committed.lit_size
        ));
    }
    for (c, a) in committed.frames.iter().zip(&actual.frames) {
        if c.tick != a.tick || c.world != a.world {
            return Comparison::Differs(format!(
                "tick {}: the world differs (the simulation or the bots changed): another match, not \
                 another picture",
                a.tick
            ));
        }
        let what = match (c.light != a.light, c.lit != a.lit) {
            (true, true) => "the light grid and the lit target differ",
            (true, false) => "the light grid differs",
            (false, true) => "the lit target differs",
            (false, false) => continue,
        };
        return Comparison::Differs(format!("tick {}: {what}", a.tick));
    }
    Comparison::Same
}

/// Plays `spec` (the runner's `advance`, unsettled, as `render-match`) and
/// draws each of its last `frames` ticks lit on `gpu` at `Viewport::GAME`,
/// 2 output pixels a world pixel, hashing the light grid and the lit target
/// after each.
pub fn record(
    content: &Content,
    spec: &MatchSpec,
    frames: u32,
    gpu: &Offscreen,
) -> anyhow::Result<FrameHashes> {
    ensure!(frames > 0 && frames <= spec.ticks, "{frames} frames of a {}-tick match", spec.ticks);
    let assets = LevelAssets::load(content, &spec.mod_id, &spec.level_id)?;
    let mut game = Match::with_assets(&assets, spec, &Options::default())?;
    let viewport = Viewport::GAME;
    let setup = PresentSetup {
        core: content.core(),
        m: &assets.m,
        level: &assets.level,
        theme: &assets.theme,
        viewport,
        options: SceneOptions { interpolation: Interpolation::Latest, show_health_bars: false },
        seed: spec.seed,
    };
    let mut present = Presentation::new(&setup, game.world());
    let skin = content.skin_or_default("olx-skin-default");
    for i in 0..game.world().worms().len() {
        present.add_worm(&skin, default_colour(i), i == 0, false);
    }
    let mut renderer = gpu.renderer(RenderSettings { filter: UpscaleFilter::IntegerNearest });
    let size = ((viewport.width * 2).max(1) as u32, (viewport.height * 2).max(1) as u32);
    let output = gpu.output_texture(size);
    let view = output.create_view(&wgpu::TextureViewDescriptor::default());
    let mut list = DrawList::new();
    let mut out = FrameHashes {
        adapter: format!(
            "{} ({:?}, {:?})",
            gpu.adapter_info().name,
            gpu.adapter_info().backend,
            gpu.adapter_info().device_type
        ),
        spec: spec.clone(),
        light_size: (0, 0),
        lit_size: (0, 0),
        frames: Vec::new(),
    };
    let first = spec.ticks - frames;
    for tick in 0..spec.ticks {
        game.advance();
        present.on_tick(game.world(), game.events());
        if tick < first {
            continue;
        }
        present.build(game.world(), 1.0, &mut list);
        present.upload(&mut renderer);
        let look =
            present.look().with_context(|| format!("{} has no look: nothing lit to hash", spec.level_id))?;
        renderer.render_lit(&list, Some(look), &view, [0, 0, size.0, size.1]);
        let (grid, _) = renderer.light_grid().context("a lit frame has a light grid")?;
        let lit = renderer.view_lit_target(0).context("a lit frame has a lit target")?;
        out.light_size = (grid.width(), grid.height());
        out.lit_size = (lit.width(), lit.height());
        out.frames.push(FrameHash {
            tick,
            world: hex(game.world().checksum()),
            light: hex(hash_bytes(&gpu.read_bytes(grid))),
            lit: hex(hash_bytes(&gpu.read_bytes(lit))),
            discs: look.emitters.len(),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 4 × 3 RGBA8 picture with every byte its own.
    fn picture() -> Vec<u8> {
        (0..48u8).map(|b| b.wrapping_mul(37)).collect()
    }

    #[test]
    fn the_same_bytes_hash_alike() {
        let (a, b) = (picture(), picture());
        assert_eq!(hash_bytes(&a), hash_bytes(&b));
        assert_eq!(hex(hash_bytes(&a)).len(), 16);
        assert_eq!(hash_bytes(&[]), 0xcbf2_9ce4_8422_2325, "FNV-1a's offset basis");
    }

    /// One texel changed by the least step in any channel, anywhere, or
    /// two texels swapped: another hash.
    #[test]
    fn a_one_texel_change_changes_the_hash() {
        let base = hash_bytes(&picture());
        for i in 0..48 {
            let mut p = picture();
            p[i] = p[i].wrapping_add(1);
            assert_ne!(hash_bytes(&p), base, "byte {i}");
        }
        let mut p = picture();
        p.swap(0, 4);
        p.swap(1, 5);
        p.swap(2, 6);
        p.swap(3, 7);
        assert_ne!(hash_bytes(&p), base, "two texels swapped");
    }

    fn hashes(frames: &[(&str, &str, &str)]) -> FrameHashes {
        FrameHashes {
            adapter: "lavapipe".into(),
            spec: MatchSpec::new("m", "l", 2, 1, 10),
            light_size: (8, 8),
            lit_size: (4, 4),
            frames: frames
                .iter()
                .enumerate()
                .map(|(i, &(w, l, t))| FrameHash {
                    tick: i as u32,
                    world: w.into(),
                    light: l.into(),
                    lit: t.into(),
                    discs: 0,
                })
                .collect(),
        }
    }

    #[test]
    fn comparisons_name_the_first_difference() {
        let a = hashes(&[("w0", "l0", "t0"), ("w1", "l1", "t1")]);
        assert_eq!(compare(&a, &a.clone()), Comparison::Same);
        let light = hashes(&[("w0", "l0", "t0"), ("w1", "lx", "t1")]);
        assert_eq!(compare(&a, &light), Comparison::Differs("tick 1: the light grid differs".into()));
        let lit = hashes(&[("w0", "l0", "tx"), ("w1", "lx", "t1")]);
        assert_eq!(compare(&a, &lit), Comparison::Differs("tick 0: the lit target differs".into()));
        let Comparison::Differs(world) = compare(&a, &hashes(&[("wx", "lx", "tx"), ("w1", "l1", "t1")]))
        else {
            panic!("a world that differs")
        };
        assert!(world.starts_with("tick 0: the world differs"), "{world}");
        let other = FrameHashes { adapter: "llvmpipe".into(), ..light };
        assert!(matches!(compare(&a, &other), Comparison::OtherAdapter(_)));
        let sized = FrameHashes { lit_size: (5, 4), ..a.clone() };
        assert!(matches!(compare(&a, &sized), Comparison::Differs(_)));
        assert!(matches!(compare(&a, &hashes(&[("w0", "l0", "t0")])), Comparison::Differs(_)));
    }
}
