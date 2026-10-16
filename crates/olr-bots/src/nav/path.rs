//! The last step of a path search: OLX's 50 px splitting.

use alloc::vec::Vec;

use olr_math::Vec2;

/// OLX's `splitUpNodes` spacing.
pub const SPLIT_DIST: f32 = 50.0;

/// Walks `raw` from its first node along `next` (one past the end ends it)
/// into `out`, with `splitUpNodes` (`src/common/CWormBot.cpp:808-835`):
/// while a segment is longer than 50 px on either axis, a node is put 50 px
/// along its longer axis. At most `cap` nodes are written; when splitting
/// would leave no room for the path's own nodes, the rest is not split.
pub(crate) fn split_up(raw: &[Vec2], next: &[u16], out: &mut Vec<Vec2>, cap: usize) {
    out.clear();
    let chain = || {
        let mut i = (!raw.is_empty()).then_some(0usize);
        core::iter::from_fn(move || {
            let cur = i?;
            i = next.get(cur).map(|&n| usize::from(n)).filter(|&n| n > cur && n < raw.len());
            Some(raw[cur])
        })
    };
    let total = chain().count().min(cap);
    let mut nodes = chain().take(total);
    let Some(mut n) = nodes.next() else { return };
    out.push(n);
    for (k, b) in nodes.enumerate() {
        // Room left for splits, keeping room for this and the later nodes.
        let reserve = total - 1 - k;
        loop {
            let s1: f32 = if n.x > b.x { 1.0 } else { -1.0 };
            let s2: f32 = if n.y > b.y { 1.0 } else { -1.0 };
            let (ax, ay) = (s1 * (n.x - b.x), s2 * (n.y - b.y));
            if !(ax > SPLIT_DIST || ay > SPLIT_DIST) || out.len() + reserve >= cap {
                break;
            }
            n = if ax >= ay {
                Vec2::new(n.x - s1 * SPLIT_DIST, n.y - s1 * SPLIT_DIST * (n.y - b.y) / (n.x - b.x))
            } else {
                Vec2::new(n.x - s2 * SPLIT_DIST * (n.x - b.x) / (n.y - b.y), n.y - s2 * SPLIT_DIST)
            };
            out.push(n);
        }
        out.push(b);
        n = b;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_segments_get_a_node_every_50_px() {
        let raw = [Vec2::new(0.0, 0.0), Vec2::new(120.0, 60.0), Vec2::new(120.0, 0.0)];
        let mut out = Vec::with_capacity(16);
        split_up(&raw, &[1, 2, 3], &mut out, 16);
        assert_eq!(
            out,
            [
                Vec2::new(0.0, 0.0),
                Vec2::new(50.0, 25.0),
                Vec2::new(100.0, 50.0),
                Vec2::new(120.0, 60.0),
                Vec2::new(120.0, 10.0),
                Vec2::new(120.0, 0.0),
            ]
        );
        // Shortcut links skip nodes; a full buffer keeps the path's own nodes.
        split_up(&raw, &[2, 2, 3], &mut out, 16);
        assert_eq!(
            out,
            [Vec2::new(0.0, 0.0), Vec2::new(50.0, 0.0), Vec2::new(100.0, 0.0), Vec2::new(120.0, 0.0)]
        );
        split_up(&raw, &[1, 2, 3], &mut out, 4);
        assert_eq!(out.len(), 4);
        assert_eq!(out[3], Vec2::new(120.0, 0.0));
    }
}
