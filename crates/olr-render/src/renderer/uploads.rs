//! Counted uploads (issue #35): the renderer's own writes to the device go
//! through [`Queue`], which counts them, so a benchmark (`olr-cli
//! quick-bench`) can show what a change saves. Counting is a few relaxed
//! atomic adds a write; it never changes what is written.

use std::ops::Deref;
use std::sync::atomic::{AtomicU64, Ordering};

/// What the renderer has written to the device since it was made
/// ([`super::Renderer::uploads`]): texture writes and the bytes of their
/// texels, buffer writes and their bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Uploads {
    pub texture_writes: u64,
    pub texture_bytes: u64,
    pub buffer_writes: u64,
    pub buffer_bytes: u64,
}

impl Uploads {
    /// The writes made since `earlier` (a reading of the same renderer).
    pub fn since(self, earlier: Uploads) -> Uploads {
        Uploads {
            texture_writes: self.texture_writes.saturating_sub(earlier.texture_writes),
            texture_bytes: self.texture_bytes.saturating_sub(earlier.texture_bytes),
            buffer_writes: self.buffer_writes.saturating_sub(earlier.buffer_writes),
            buffer_bytes: self.buffer_bytes.saturating_sub(earlier.buffer_bytes),
        }
    }
}

/// The device's queue, counting the texture and buffer writes made through
/// it (anything else is the queue's own, through `Deref`).
pub(crate) struct Queue {
    queue: wgpu::Queue,
    counts: [AtomicU64; 4],
}

impl Deref for Queue {
    type Target = wgpu::Queue;

    fn deref(&self) -> &wgpu::Queue {
        &self.queue
    }
}

impl Queue {
    pub(crate) fn new(queue: wgpu::Queue) -> Queue {
        Queue { queue, counts: Default::default() }
    }

    fn add(&self, writes: usize, bytes: u64) {
        self.counts[writes].fetch_add(1, Ordering::Relaxed);
        self.counts[writes + 1].fetch_add(bytes, Ordering::Relaxed);
    }

    /// `wgpu::Queue::write_texture`, counted: its extent's texels' bytes.
    pub(crate) fn write_texture(
        &self,
        texture: wgpu::TexelCopyTextureInfo,
        data: &[u8],
        layout: wgpu::TexelCopyBufferLayout,
        size: wgpu::Extent3d,
    ) {
        let texel = texture.texture.format().block_copy_size(None).unwrap_or(0);
        let texels = u64::from(size.width) * u64::from(size.height) * u64::from(size.depth_or_array_layers);
        self.add(0, texels * u64::from(texel));
        self.queue.write_texture(texture, data, layout, size);
    }

    /// `wgpu::Queue::write_buffer`, counted.
    pub(crate) fn write_buffer(&self, buffer: &wgpu::Buffer, offset: wgpu::BufferAddress, data: &[u8]) {
        self.add(2, data.len() as u64);
        self.queue.write_buffer(buffer, offset, data);
    }

    pub(crate) fn uploads(&self) -> Uploads {
        let at = |i: usize| self.counts[i].load(Ordering::Relaxed);
        Uploads { texture_writes: at(0), texture_bytes: at(1), buffer_writes: at(2), buffer_bytes: at(3) }
    }
}
