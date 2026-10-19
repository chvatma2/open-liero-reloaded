//! Flicker regions (plan 15c, spec "My calls"): each separately drawn
//! region of a `Flicker` group's pixels flickers with its own phase. A
//! region is 4-connected world pixels of one group; its id is 1 + the
//! row-major index of its first pixel, folded into 1..=65 535 (0: none).
//! Only the groups that flicker are labelled (`mask`), so a sky of a steady
//! group never costs a flood fill.

/// The region map of one level: ids per world pixel, labelled for `mask`.
#[derive(Debug)]
pub(super) struct Regions {
    /// World pixels.
    size: (u32, u32),
    /// The group map, one byte per world pixel (0 no group).
    groups: Vec<u8>,
    /// The region id per world pixel.
    pub(super) ids: Vec<u16>,
    /// The groups (`v - 1` for group value `v`) labelled.
    mask: [bool; 255],
    /// Visit stamps (`stamp` this fill), so no fill clears a whole map.
    seen: Vec<u32>,
    stamp: u32,
    stack: Vec<u32>,
    region: Vec<u32>,
}

/// A region's id from its first pixel's index.
fn id_of(first: u32) -> u16 {
    (first % 65_535 + 1) as u16
}

impl Regions {
    /// A level of `size` world pixels whose group map is `groups` (none:
    /// no glow), labelled for no group yet.
    pub(super) fn new(size: (u32, u32), groups: Option<&[u8]>) -> Regions {
        let n = size.0 as usize * size.1 as usize;
        let groups = match groups {
            Some(g) if g.len() == n => g.to_vec(),
            _ => vec![0; n],
        };
        Regions {
            size,
            groups,
            ids: vec![0; n],
            mask: [false; 255],
            seen: vec![0; n],
            stamp: 0,
            stack: Vec::new(),
            region: Vec::new(),
        }
    }

    /// Labels the whole map for `mask` when it is not the one labelled;
    /// whether it did (the caller uploads the whole map).
    pub(super) fn set_mask(&mut self, mask: &[bool; 255]) -> bool {
        if &self.mask == mask {
            return false;
        }
        self.mask = *mask;
        self.ids.iter_mut().for_each(|v| *v = 0);
        self.next_stamp();
        for i in 0..self.ids.len() as u32 {
            self.fill(i);
        }
        true
    }

    /// Takes world rectangle `(x, y, w, h)` of `groups` (the whole map's)
    /// and labels again every region touching it or its border: the world
    /// rectangle (x, y, w, h) whose ids changed, or `None`.
    pub(super) fn update(&mut self, groups: &[u8], r: (u32, u32, u32, u32)) -> Option<(u32, u32, u32, u32)> {
        let (w, h) = self.size;
        if groups.len() != self.groups.len() || w == 0 || h == 0 {
            return None;
        }
        let (x0, y0) = (r.0.min(w), r.1.min(h));
        let (x1, y1) = (r.0.saturating_add(r.2).min(w), r.1.saturating_add(r.3).min(h));
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        for y in y0..y1 {
            let a = (y * w + x0) as usize..(y * w + x1) as usize;
            self.groups[a.clone()].copy_from_slice(&groups[a]);
        }
        // The rectangle and its border: a region the edit cut keeps a pixel
        // beside it.
        let (bx0, by0, bx1, by1) =
            (x0.saturating_sub(1), y0.saturating_sub(1), (x1 + 1).min(w), (y1 + 1).min(h));
        for y in y0..y1 {
            for x in x0..x1 {
                self.ids[(y * w + x) as usize] = 0;
            }
        }
        let mut changed = (x0, y0, x1, y1);
        self.next_stamp();
        for y in by0..by1 {
            for x in bx0..bx1 {
                if let Some((a, b, c, d)) = self.fill(y * w + x) {
                    changed = (changed.0.min(a), changed.1.min(b), changed.2.max(c), changed.3.max(d));
                }
            }
        }
        Some((changed.0, changed.1, changed.2 - changed.0, changed.3 - changed.1))
    }

    fn next_stamp(&mut self) {
        self.stamp = self.stamp.wrapping_add(1);
        if self.stamp == 0 {
            self.seen.iter_mut().for_each(|v| *v = 0);
            self.stamp = 1;
        }
    }

    /// Labels the region holding pixel `i` (when it is of a labelled group
    /// and not yet seen this stamp): its bounding box (x0, y0, x1, y1).
    fn fill(&mut self, i: u32) -> Option<(u32, u32, u32, u32)> {
        let v = self.groups[i as usize];
        if v == 0 || !self.mask[usize::from(v) - 1] || self.seen[i as usize] == self.stamp {
            return None;
        }
        let w = self.size.0;
        self.region.clear();
        self.stack.clear();
        self.stack.push(i);
        self.seen[i as usize] = self.stamp;
        while let Some(p) = self.stack.pop() {
            self.region.push(p);
            let (x, y) = (p % w, p / w);
            let visit = |q: u32, stack: &mut Vec<u32>, seen: &mut Vec<u32>| {
                if self.groups[q as usize] == v && seen[q as usize] != self.stamp {
                    seen[q as usize] = self.stamp;
                    stack.push(q);
                }
            };
            if x > 0 {
                visit(p - 1, &mut self.stack, &mut self.seen);
            }
            if x + 1 < w {
                visit(p + 1, &mut self.stack, &mut self.seen);
            }
            if y > 0 {
                visit(p - w, &mut self.stack, &mut self.seen);
            }
            if y + 1 < self.size.1 {
                visit(p + w, &mut self.stack, &mut self.seen);
            }
        }
        let first = self.region.iter().copied().min().unwrap_or(i);
        let id = id_of(first);
        let mut bb = (u32::MAX, u32::MAX, 0, 0);
        for &p in &self.region {
            self.ids[p as usize] = id;
            let (x, y) = (p % w, p / w);
            bb = (bb.0.min(x), bb.1.min(y), bb.2.max(x + 1), bb.3.max(y + 1));
        }
        Some(bb)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mask(groups: &[u8]) -> [bool; 255] {
        let mut m = [false; 255];
        for g in groups {
            m[usize::from(*g) - 1] = true;
        }
        m
    }

    #[test]
    fn regions_are_connected_pixels_of_a_flickering_group() {
        // Two blobs of group 1, one of group 2 touching the first.
        #[rustfmt::skip]
        let g = [
            1, 1, 0, 0, 1,
            0, 1, 2, 0, 1,
            0, 0, 2, 0, 0,
        ];
        let mut r = Regions::new((5, 3), Some(&g));
        assert!(r.set_mask(&mask(&[1])));
        assert!(!r.set_mask(&mask(&[1])), "the same mask labels nothing");
        #[rustfmt::skip]
        let want = [
            1, 1, 0, 0, 5,
            0, 1, 0, 0, 5,
            0, 0, 0, 0, 0,
        ];
        assert_eq!(r.ids, want);
        r.set_mask(&mask(&[1, 2]));
        assert_eq!(r.ids[7], 8, "group 2's own region");
    }

    #[test]
    fn an_edit_joins_and_splits_regions() {
        let mut g = vec![0u8; 7];
        g[1] = 1;
        g[5] = 1;
        let mut r = Regions::new((7, 1), Some(&g));
        r.set_mask(&mask(&[1]));
        assert_eq!((r.ids[1], r.ids[5]), (2, 6));
        // A bridge: one region, its first pixel's id.
        g[2..5].fill(1);
        assert_eq!(r.update(&g, (2, 0, 3, 1)), Some((1, 0, 5, 1)));
        assert!(r.ids[1..6].iter().all(|&v| v == 2), "{:?}", r.ids);
        // Cut in the middle: two again.
        g[3] = 0;
        r.update(&g, (3, 0, 1, 1));
        assert_eq!(&r.ids[1..6], &[2, 2, 0, 5, 5]);
        // Out of range and wrong sizes change nothing.
        assert_eq!(r.update(&g, (9, 0, 3, 1)), None);
        assert_eq!(r.update(&g[..3], (0, 0, 3, 1)), None);
    }
}
