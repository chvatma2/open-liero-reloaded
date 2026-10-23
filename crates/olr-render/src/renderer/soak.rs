//! Light soak by material (issue #57): an opaque solid texel soaks by its
//! own surface material's `light_soak`, else by the level's. "Its
//! material" is the front surface map's value at its world pixel (where
//! rock or dirt stands the front rules, as for heat; the soak only reads
//! opaque solids). No OLX counterpart.
//!
//! The soak's passes read a per-material radius table ([`SoakTable`], a
//! uniform) and the front map: the heat's own surface texture when the
//! level heats (its red channel), else an `R8Uint` map of its own at world
//! size, cropped as the solid map. One copy of the map either way. The map
//! is the level's and does not change with the terrain; the solid map the
//! soak reads beside it follows every dirty rectangle, so what digs, or
//! adds dirt, changes the soak at once. With no material setting a soak
//! there is none of this: the soak runs today's shader, bytes and passes.

use olr_look::SurfaceMaterial;

use super::heat::{plain_texture, view};

/// The table's entries: one a material value (a byte, 0 none).
const ENTRIES: usize = 256;

/// The level's soak in the table: the material sets none.
const LEVEL: i32 = -1;

/// The soak's radius by material value: `-1` the level's, else 0 to
/// [`olr_look::MAX_SOAK`]; value 0 (no material) the level's.
pub(super) type SoakTable = Vec<i32>;

/// The level's materials' soak table, `None` when none sets its own
/// soak (then nothing changes).
pub(super) fn soak_table(materials: &[SurfaceMaterial]) -> Option<SoakTable> {
    let mut table = vec![LEVEL; ENTRIES];
    let mut any = false;
    for (i, m) in materials.iter().take(olr_look::MAX_MATERIALS).enumerate() {
        if let Some(s) = m.light_soak() {
            table[i + 1] = i32::from(s);
            any = true;
        }
    }
    any.then_some(table)
}

/// The front material of each world pixel of `texels` (the heat's
/// `Rg8Uint` surface texels: front, back).
fn front_bytes(texels: &[u8]) -> Vec<u8> {
    texels.iter().step_by(2).copied().collect()
}

/// The soak's group 1 (`look/soak.wgsl`'s material prelude): its table
/// and the front map.
pub(super) fn layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    let frag = wgpu::ShaderStages::FRAGMENT;
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("olr-render material soak"),
        entries: &[
            super::entry(0, frag, super::UNIFORM),
            super::entry(
                1,
                frag,
                wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Uint,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
            ),
        ],
    })
}

/// The WGSL the material soak's pipelines read (group 1): the table and
/// the front map, and `soak_radius` by them. The plain pipelines' twin
/// gives the level's soak alone.
pub(super) const MATERIAL_PRELUDE: &str = "
@group(1) @binding(0) var<uniform> soak_table: array<vec4<i32>, 64>;
@group(1) @binding(1) var front_tex: texture_2d<u32>;

// World pixel `w`'s soak (issue #57): its front material's own, else the
// level's.
fn soak_radius(w: vec2<i32>) -> i32 {
    let size = vec2<i32>(textureDimensions(front_tex));
    if any(w < vec2<i32>(0)) || any(w >= size) {
        return level_soak();
    }
    let v = i32(textureLoad(front_tex, w, 0).r);
    let s = soak_table[v >> 2u][v & 3];
    return select(clamp(s, 0, MAX_SOAK), level_soak(), s < 0);
}
";

/// The plain soak's `soak_radius`: the level's soak everywhere (today's).
pub(super) const PLAIN_PRELUDE: &str = "
fn soak_radius(w: vec2<i32>) -> i32 {
    return level_soak();
}
";

/// A level's material soak on the GPU: made only when a material sets
/// its own soak and the surface maps fit the level.
pub(super) struct SoakTextures {
    table: wgpu::Buffer,
    /// The front map, when the heat has no surface texture to lend (kept
    /// with the bind group that reads it).
    _front: Option<wgpu::Texture>,
    /// The soak passes' group 1.
    pub(super) bind: wgpu::BindGroup,
}

impl SoakTextures {
    /// The soak of a level of `world` pixels (the textures `size`, the
    /// solid map's crop): `heat_surface` the heat's surface texture when
    /// there is one, else a front map made from `texels`.
    pub(super) fn new(
        device: &wgpu::Device,
        queue: &super::uploads::Queue,
        layout: &wgpu::BindGroupLayout,
        table: &[i32],
        (world, size): ((u32, u32), (u32, u32)),
        heat_surface: Option<&wgpu::TextureView>,
        texels: &[u8],
    ) -> SoakTextures {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("olr-render soak table"),
            size: (ENTRIES * 4) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&buffer, 0, bytemuck::cast_slice(table));
        let front = heat_surface.is_none().then(|| {
            let t = plain_texture(
                device,
                "olr-render soak front map",
                size,
                wgpu::TextureFormat::R8Uint,
                wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            );
            let (w, h) = (size.0.min(world.0), size.1.min(world.1));
            let bytes = front_bytes(texels);
            if w > 0 && h > 0 && bytes.len() as u64 >= u64::from(world.0) * u64::from(world.1) {
                queue.write_texture(
                    t.as_image_copy(),
                    &bytes,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(world.0),
                        rows_per_image: Some(h),
                    },
                    wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                );
            }
            t
        });
        let own = front.as_ref().map(view);
        let front_view = heat_surface.or(own.as_ref()).expect("one of the two");
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("olr-render material soak"),
            layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(front_view) },
            ],
        });
        SoakTextures { table: buffer, _front: front, bind }
    }

    /// New materials' soaks (issue #45's live preview): the next frame
    /// soaks by them.
    pub(super) fn set_table(&self, queue: &super::uploads::Queue, table: &[i32]) {
        queue.write_buffer(&self.table, 0, bytemuck::cast_slice(table));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn material(soak: Option<u8>) -> SurfaceMaterial {
        SurfaceMaterial { id: "m".into(), heat: None, light_soak: soak }
    }

    #[test]
    fn only_materials_with_a_soak_make_a_table() {
        assert_eq!(soak_table(&[]), None);
        assert_eq!(soak_table(&[material(None), material(None)]), None);
        let t = soak_table(&[material(None), material(Some(2)), material(Some(200))]).unwrap();
        assert_eq!(t.len(), ENTRIES);
        assert_eq!(&t[..4], &[LEVEL, LEVEL, 2, olr_look::MAX_SOAK.into()], "0 none, a hostile soak clamped");
        assert!(t[4..].iter().all(|&s| s == LEVEL));
        assert_eq!(front_bytes(&[1, 2, 3, 4, 5, 6]), vec![1, 3, 5]);
    }
}
