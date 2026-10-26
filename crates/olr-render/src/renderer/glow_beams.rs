//! Glow beams (the directional-glow design, spec
//! `docs/superpowers/specs/2026-10-25-directional-glow-design.md`): a
//! level's directional glow patches, each one beam the composite draws
//! (`glow_beams_at` in `composite.wgsl`), and the beam map the cascades
//! weight a directional cell's light by (`beam_cone` in `gi_cascade.wgsl`).
//! No OLX counterpart.
//!
//! The sources are derived ([`olr_look::glow_beams::derive`]) when a level
//! is uploaded, its maps are set anew or the editor uploads glow or art over
//! it (`ColorMap::beams_generation`);
//! each frame packs them with their groups' cast into a uniform.

use olr_data::level::Material;
use olr_look::LookFrame;
use olr_look::glow_beams::{BeamLayer, GlowBeamSource, MAX_GLOW_BEAMS, derive};
use olr_terrain::{ColorMap, Rect};

/// A beam's strength per unit of its group's cast (intensity 1000 at glow
/// 100 gives the reference headlights' 0.8).
pub(super) const BEAM_SCALE: f32 = 0.08;

/// Floats a beam takes in the uniform (`GlowBeam` in `composite.wgsl`):
/// at, dir, half, tan(spread), reach, group (negative for a backdrop
/// patch's), light, region.
pub(super) const BEAM_FLOATS: usize = 12;

/// The beams uniform's bytes: [`MAX_GLOW_BEAMS`] beams.
pub(super) const BEAMS_BYTES: u64 = (MAX_GLOW_BEAMS * BEAM_FLOATS * 4) as u64;

/// The longest reach a beam is drawn to, world pixels (hostile content:
/// the composite's shadow march is bounded by [`BEAM_MAX_STEPS`], four
/// times it); `check` allows reaches to 65 535.
pub(super) const MAX_BEAM_REACH: f32 = 512.0;

/// The most pixels the composite's shadow walk visits (`beam_sees` in
/// `composite.wgsl`, given it by [`wgsl`]); a walk that runs out counts as
/// blocked, so [`walk_reach`] keeps every beam within it.
pub(super) const BEAM_MAX_STEPS: u32 = 2048;

/// The widest spread a beam is drawn at, radians (a half-angle of 90° is a
/// half-plane: its tangent stays finite).
const MAX_SPREAD: f32 = 89.5 * core::f32::consts::PI / 180.0;

/// The composite's prelude: the uniform's array length.
pub(super) fn wgsl() -> String {
    format!("const MAX_GLOW_BEAMS: i32 = {MAX_GLOW_BEAMS};\nconst BEAM_MAX_STEPS: i32 = {BEAM_MAX_STEPS};\n")
}

/// The longest reach a beam of half-width `half` and spread tangent `tan`
/// is drawn to so the shadow walk from any pixel it lights to any point
/// across its source stays within [`BEAM_MAX_STEPS`] (final review M2: a
/// wide beam at a long reach ran out of steps and ended in a hard dark
/// cut; cut short, its falloff fades it out). A lit pixel lies at most
/// `reach` along and `half + reach (tan + 0.03) + 0.5` aside (the edge's
/// penumbra), a source point half a pixel behind the face and `half`
/// aside; a grid walk takes at most √2 times that distance plus a pixel
/// at each end. At most [`MAX_BEAM_REACH`]; never negative.
pub(super) fn walk_reach(half: f32, tan: f32) -> f32 {
    let steps = BEAM_MAX_STEPS as f32 / core::f32::consts::SQRT_2 - 2.0 * half.max(0.0) - 4.0;
    (steps / (1.03 + tan.max(0.0))).clamp(0.0, MAX_BEAM_REACH)
}

/// The layouts and the frame's uniform a level's bind groups use.
pub(super) struct BeamBinding<'a> {
    /// The cascades' group 1: the beam map.
    pub(super) cascade_layout: &'a wgpu::BindGroupLayout,
    /// The composite's group 1: the beams uniform and the haze map.
    pub(super) composite_layout: &'a wgpu::BindGroupLayout,
    pub(super) uniform: &'a wgpu::Buffer,
}

/// A level's glow beams on the GPU.
pub(super) struct GlowBeams {
    /// The derived beams: none past [`MAX_GLOW_BEAMS`] (`check` refuses
    /// such a level).
    sources: Vec<GlowBeamSource>,
    /// Each world pixel's beam texel as shown (`Rg8Uint`: direction / 2 +
    /// 1, 0 none; the spread), the backdrop's where the pixel is background,
    /// the composite's elsewhere; at the look's world size (cropped) with a
    /// beam map, else 1 × 1 none.
    map: wgpu::Texture,
    /// The haze map (`R8Unorm`, 0 none to 1 the level's haze) at the
    /// look's world size with a map, else 1 × 1 at 1.
    haze: wgpu::Texture,
    /// The cascades' group 1.
    pub(super) cascade_bind: wgpu::BindGroup,
    /// The composite's group 1.
    pub(super) composite_bind: wgpu::BindGroup,
    /// Whether [`Self::map`] holds a level's beam map (else 1 × 1 none).
    mapped: bool,
    /// The `ColorMap::beams_generation` the textures hold.
    generation: Option<u32>,
    /// A rectangle's composed texels (reused).
    scratch: Vec<u8>,
}

fn texture(
    device: &wgpu::Device,
    label: &str,
    size: (u32, u32),
    format: wgpu::TextureFormat,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: size.0.max(1), height: size.1.max(1), depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

/// Writes `data` (`texel` bytes a texel, `w` texels a row) at `(x, y)` of
/// `texture`.
fn write(
    queue: &super::uploads::Queue,
    texture: &wgpu::Texture,
    data: &[u8],
    (x, y, w, h): Rect4,
    texel: u32,
) {
    if w == 0 || h == 0 || data.len() as u64 != u64::from(w) * u64::from(h) * u64::from(texel) {
        return;
    }
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d { x, y, z: 0 },
            aspect: wgpu::TextureAspect::All,
        },
        data,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * texel), rows_per_image: Some(h) },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
}

/// A rectangle as x, y, width, height.
type Rect4 = (u32, u32, u32, u32);

/// The beams of `map` (none without glow or a beam map, or past the cap).
fn sources(map: &ColorMap) -> Vec<GlowBeamSource> {
    let world = (map.material().width(), map.material().height());
    let (Some(beams), Some(glow)) = (map.glow_beams().filter(|b| b.fits(world)), map.glow_layers()) else {
        return Vec::new();
    };
    let (front, back) = map.art_layers();
    // The composite's beams only on rock: on background they are the
    // backdrop's (as baked), whose own beam that is.
    let cells = map.material().cells();
    let mut on_rock = beams.front.clone();
    for (texel, cell) in on_rock.as_chunks_mut::<2>().0.iter_mut().zip(cells) {
        if *cell != Material::Rock as u8 {
            *texel = [0, 0];
        }
    }
    let layer = |is_back: bool| BeamLayer {
        world,
        art_scale: map.art_scale(),
        groups: if is_back { &glow.groups_back } else { &glow.groups },
        glow: if is_back { &glow.back } else { &glow.front },
        art: if is_back { back } else { front },
        beams: if is_back { &beams.back } else { &on_rock },
    };
    let all = derive(&layer(false), &layer(true));
    if all.len() > MAX_GLOW_BEAMS { Vec::new() } else { all }
}

impl GlowBeams {
    /// No beams nor haze yet: 1 × 1 textures; [`GlowBeams::sync`] fills them.
    pub(super) fn new(device: &wgpu::Device, queue: &super::uploads::Queue, b: &BeamBinding) -> GlowBeams {
        let map = texture(device, "olr-render glow beams", (1, 1), wgpu::TextureFormat::Rg8Uint);
        let haze = texture(device, "olr-render haze", (1, 1), wgpu::TextureFormat::R8Unorm);
        write(queue, &map, &[0, 0], (0, 0, 1, 1), 2);
        write(queue, &haze, &[255], (0, 0, 1, 1), 1);
        let (cascade_bind, composite_bind) = Self::binds(device, b, &map, &haze);
        GlowBeams {
            sources: Vec::new(),
            map,
            haze,
            cascade_bind,
            composite_bind,
            mapped: false,
            generation: None,
            scratch: Vec::new(),
        }
    }

    fn binds(
        device: &wgpu::Device,
        b: &BeamBinding,
        map: &wgpu::Texture,
        haze: &wgpu::Texture,
    ) -> (wgpu::BindGroup, wgpu::BindGroup) {
        let view = |t: &wgpu::Texture| t.create_view(&wgpu::TextureViewDescriptor::default());
        let (map, haze) = (view(map), view(haze));
        let cascade = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("olr-render glow beams cascade"),
            layout: b.cascade_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&map),
            }],
        });
        let composite = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("olr-render glow beams composite"),
            layout: b.composite_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: b.uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&haze) },
            ],
        });
        (cascade, composite)
    }

    /// Whether the level has a beam map (the cascades weight by its cone).
    pub(super) fn has_map(&self) -> bool {
        self.mapped
    }

    /// Derives `map`'s beams and uploads its beam and haze maps when they
    /// are not the ones held (`ColorMap::beams_generation`), at `world`
    /// (the look's world size, cropped); whether they changed (the glow
    /// light is then made again).
    pub(super) fn sync(
        &mut self,
        device: &wgpu::Device,
        queue: &super::uploads::Queue,
        b: &BeamBinding,
        map: &ColorMap,
        world: (u32, u32),
    ) -> bool {
        if self.generation == Some(map.beams_generation()) {
            return false;
        }
        self.generation = Some(map.beams_generation());
        self.sources = sources(map);
        let level = (map.material().width(), map.material().height());
        // A beam map counts only with beams kept (none past the cap: no cones).
        let has_beams = map.glow_beams().is_some_and(|m| m.fits(level)) && !self.sources.is_empty();
        let haze = map.haze().filter(|h| h.len() as u64 == u64::from(level.0) * u64::from(level.1));
        let size = |on: bool| if on { world } else { (1, 1) };
        let sized = |t: &wgpu::Texture, s: (u32, u32)| (t.width(), t.height()) == (s.0.max(1), s.1.max(1));
        let (map_size, haze_size) = (size(has_beams), size(haze.is_some()));
        let remake = !sized(&self.map, map_size) || !sized(&self.haze, haze_size);
        self.mapped = has_beams;
        if remake {
            self.map = texture(device, "olr-render glow beams", map_size, wgpu::TextureFormat::Rg8Uint);
            self.haze = texture(device, "olr-render haze", haze_size, wgpu::TextureFormat::R8Unorm);
            (self.cascade_bind, self.composite_bind) = Self::binds(device, b, &self.map, &self.haze);
        }
        if has_beams {
            self.write(queue, map, Rect { x: 0, y: 0, w: level.0, h: level.1 });
        } else {
            write(queue, &self.map, &[0, 0], (0, 0, 1, 1), 2);
        }
        match haze {
            Some(h) => {
                let (w, hh) = (self.haze.width().min(level.0), self.haze.height().min(level.1));
                self.scratch.clear();
                for y in 0..hh as usize {
                    let row = y * level.0 as usize;
                    self.scratch.extend_from_slice(&h[row..row + w as usize]);
                }
                write(queue, &self.haze, &self.scratch, (0, 0, w, hh), 1);
            }
            None => write(queue, &self.haze, &[255], (0, 0, 1, 1), 1),
        }
        true
    }

    /// Writes world rectangle `r` of the beam map as `map` shows it: the
    /// backdrop's texel where the pixel is background, the composite's
    /// elsewhere (digging reveals the backdrop's beams). Nothing without a
    /// beam map.
    pub(super) fn write(&mut self, queue: &super::uploads::Queue, map: &ColorMap, r: Rect) {
        let level = (map.material().width(), map.material().height());
        let Some(beams) = map.glow_beams().filter(|b| b.fits(level)) else { return };
        if !self.mapped {
            return;
        }
        let (x0, y0) = (r.x.min(self.map.width()), r.y.min(self.map.height()));
        let x1 = r.x.saturating_add(r.w).min(self.map.width());
        let y1 = r.y.saturating_add(r.h).min(self.map.height());
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let cells = map.material().cells();
        self.scratch.clear();
        for y in y0..y1 {
            for x in x0..x1 {
                let i = y as usize * level.0 as usize + x as usize;
                let layer = if cells[i] == Material::Background as u8 { &beams.back } else { &beams.front };
                self.scratch.extend_from_slice(&layer[2 * i..2 * i + 2]);
            }
        }
        write(queue, &self.map, &self.scratch, (x0, y0, x1 - x0, y1 - y0), 2);
    }

    /// Packs the frame's beams into `out` ([`pack`]); returns the count.
    pub(super) fn pack(&self, look: &LookFrame, regions: &[u16], out: &mut Vec<f32>) -> u32 {
        pack(&self.sources, look, regions, out)
    }
}

/// Packs the frame's beams into `out` ([`BEAM_FLOATS`] each): each of
/// `sources` whose group `look` has, with a reach in world pixels, that
/// casts, its reach at most [`walk_reach`]. Its light is [`BEAM_SCALE`] × the group's cast × its colour (its
/// art's, normalised, for an own-colour or filtered group; white for a
/// tint, which the cast carries) × its mean glow. `regions` are the flicker
/// regions (a `Flicker` group's beam flickers as its patch's first pixel's
/// region does). Returns the count.
fn pack(sources: &[GlowBeamSource], look: &LookFrame, regions: &[u16], out: &mut Vec<f32>) -> u32 {
    out.clear();
    for s in sources {
        let Some(g) = usize::from(s.group).checked_sub(1).and_then(|i| look.groups.get(i)) else {
            continue;
        };
        if !(g.reach > 0.0 && g.reach < 1e8) {
            continue;
        }
        let colour = if g.own_colour { s.colour } else { [1.0; 3] };
        let light: [f32; 3] = std::array::from_fn(|i| BEAM_SCALE * g.cast[i] * colour[i] * s.glow);
        if !light.iter().any(|v| *v > 0.0) {
            continue;
        }
        let region = match g.flicker {
            Some(_) => regions.get(s.first as usize).copied().unwrap_or(0),
            None => 0,
        };
        let tan = s.spread.clamp(0.0, MAX_SPREAD).tan();
        let group = if s.back { -f32::from(s.group) } else { f32::from(s.group) };
        let at = out.len();
        let reach = g.reach.min(walk_reach(s.half, tan));
        out.extend([s.at[0], s.at[1], s.dir[0], s.dir[1], s.half, tan, reach, group]);
        out.extend([light[0], light[1], light[2], f32::from(region)]);
        // A NaN never reaches a shader.
        if !out[at..].iter().all(|v| v.is_finite()) {
            out.truncate(at);
        }
    }
    (out.len() / BEAM_FLOATS) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use olr_data::image::{Rgba, RgbaImage};
    use olr_data::level::MaterialGrid;
    use olr_terrain::{GlowBeamMap, GlowLayers, Terrain, ThemeArt};

    /// A 128 × 4 open level whose rows 0 and 2 glow in group 1, every
    /// `every`-th pixel directional (a patch each) in the backdrop, the
    /// composite's maps the backdrop's on background (as baked).
    fn map(every: u32) -> ColorMap {
        let (w, h) = (128, 4);
        let cells = vec![Material::Background; (w * h) as usize];
        let t = Terrain::from_grid(&MaterialGrid::from_cells(w, h, cells).unwrap());
        let mut art = RgbaImage::new(w, h);
        for x in 0..w {
            art.set(x, 0, Rgba::rgb(255, 128, 0));
            art.set(x, 2, Rgba::rgb(255, 128, 0));
        }
        let theme = ThemeArt { holes: Vec::new(), green_mask: None, front_tile: art.clone() };
        let mut m = ColorMap::new(&t, &art, &art, 1, theme);
        let mut g = GlowLayers::empty((w, h), (w, h));
        let mut b = GlowBeamMap::none((w, h));
        for y in [0, 2] {
            for x in (0..w).step_by(every as usize) {
                let i = (y * w + x) as usize;
                (g.front[i], g.back[i], g.groups[i], g.groups_back[i]) = (255, 255, 1, 1);
                (b.back[2 * i], b.back[2 * i + 1]) = (46, 30);
                (b.front[2 * i], b.front[2 * i + 1]) = (46, 30);
            }
        }
        assert!(m.set_glow(Some(g)) && m.set_glow_beams(Some(b)));
        m
    }

    fn look(cast: f32, reach: f32, own_colour: bool) -> LookFrame {
        let mut l = LookFrame::default();
        l.groups.push(olr_look::FrameGroup { cast: [cast; 3], own_colour, reach, ..Default::default() });
        l
    }

    /// Past [`MAX_GLOW_BEAMS`] a level has none (`check` refuses it); a
    /// background patch is one beam, the backdrop's (the composite's maps
    /// there are its copy).
    #[test]
    fn past_the_cap_a_level_has_no_beams() {
        // 32 patches a row: 64 beams, all the backdrop's.
        let s = sources(&map(4));
        assert_eq!(s.len(), MAX_GLOW_BEAMS);
        assert!(s.iter().all(|s| s.back));
        // 43 a row: none.
        assert!(sources(&map(3)).is_empty());
    }

    /// The light is BEAM_SCALE × cast × colour × glow; a group the frame
    /// lacks, an unlimited reach or no light packs none; a reach past
    /// MAX_BEAM_REACH is clamped; a backdrop beam's group is negative.
    #[test]
    fn beams_pack_their_groups_cast() {
        let mut sources = sources(&map(4));
        let mut out = Vec::new();
        assert_eq!(pack(&sources, &look(10.0, 60.0, true), &[], &mut out), 64);
        let first = &out[..BEAM_FLOATS];
        // Own colour: the art (255, 128, 0) normalised, × 0.8.
        let orange = olr_look::srgb_to_linear(128);
        assert!((first[8] - 0.8).abs() < 1e-5 && (first[9] - 0.8 * orange).abs() < 1e-5 && first[10] == 0.0);
        assert!((first[5] - 30f32.to_radians().tan()).abs() < 1e-5 && first[6] == 60.0 && first[7] == -1.0);
        // Hostile content: a reach of 65 535 is drawn to 512.
        pack(&sources, &look(10.0, 65535.0, true), &[], &mut out);
        assert_eq!(out[6], MAX_BEAM_REACH);
        // A tint: white × the cast.
        pack(&sources, &look(10.0, 60.0, false), &[], &mut out);
        assert!(out[8..11].iter().all(|v| (v - 0.8).abs() < 1e-5));
        assert_eq!(pack(&sources, &look(10.0, f32::INFINITY, true), &[], &mut out), 0);
        assert_eq!(pack(&sources, &look(0.0, 60.0, true), &[], &mut out), 0);
        assert_eq!(pack(&sources, &LookFrame::default(), &[], &mut out), 0);
        // A NaN source packs none.
        let mut one = sources.clone();
        one.truncate(1);
        one[0].at[0] = f32::NAN;
        assert_eq!(pack(&one, &look(10.0, 60.0, true), &[], &mut out), 0);
        assert!(out.is_empty());
        // A flickering group's beam takes its first pixel's region.
        let mut flicker = look(10.0, 60.0, true);
        flicker.groups[0].flicker = Some((5.0, 50.0));
        let mut regions = vec![0u16; 512];
        regions[4] = 77;
        pack(&sources, &flicker, &regions, &mut out);
        assert_eq!((out[11], out[BEAM_FLOATS + 11]), (0.0, 77.0));
        // A front source's group is positive.
        sources[0].back = false;
        pack(&sources, &look(10.0, 60.0, true), &[], &mut out);
        assert_eq!(out[7], 1.0);
    }

    /// Final review M2: a wide beam's reach is cut so the composite's
    /// shadow walk (`BEAM_MAX_STEPS` pixels) reaches its far end: the beam
    /// fades out within the walk rather than ending in a hard dark cut. A
    /// beam whose walk fits keeps its reach.
    #[test]
    fn a_wide_beam_fades_within_the_walk() {
        let mut one = sources(&map(4));
        one.truncate(1);
        let mut out = Vec::new();
        for spread in 1..=90 {
            for half in [0.5, 2.5, 10.0, 40.0] {
                one[0].spread = (spread as f32).to_radians();
                one[0].half = half;
                assert_eq!(pack(&one, &look(10.0, 65535.0, true), &[], &mut out), 1);
                let (tan, reach) = (out[5], out[6]);
                let walk = reach * core::f32::consts::SQRT_2 * (1.0 + tan);
                assert!(walk <= 2048.0, "spread {spread}, half {half}: reach {reach} walks {walk}");
                assert!(reach > 0.0 && reach <= MAX_BEAM_REACH);
            }
        }
        // Spread 30: the walk fits, the reach as asked (or 512).
        one[0].spread = 30f32.to_radians();
        one[0].half = 2.5;
        pack(&one, &look(10.0, 60.0, true), &[], &mut out);
        assert_eq!(out[6], 60.0);
        pack(&one, &look(10.0, 65535.0, true), &[], &mut out);
        assert_eq!(out[6], MAX_BEAM_REACH);
        // Spread 80 at reach 500: cut.
        one[0].spread = 80f32.to_radians();
        pack(&one, &look(10.0, 500.0, true), &[], &mut out);
        assert!(out[6] < 250.0 && out[6] == walk_reach(2.5, out[5]), "{}", out[6]);
    }
}
