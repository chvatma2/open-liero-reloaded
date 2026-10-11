//! OLX's pathfinder (`searchpath_base`, `src/common/CWormBot.cpp:272-878`,
//! notes §1.4): an A*-like search whose nodes are maximal rock-free
//! rectangles, made resumable across ticks under a work budget instead of
//! running in a thread.
//!
//! Deliberate changes (notes §4.2, §4.3): areas come from the shared
//! [`AreaCache`] instead of being grown anew for every search; the open set
//! is a heap that breaks ties by insertion order (OLX's `std::set` drops an
//! area whose priority ties with a queued one); `simplifyPath`'s dead second
//! loop is left out; the areas per search and the path's nodes are capped.

use alloc::collections::BinaryHeap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::cmp::Reverse;

use rl_math::Vec2;
use rl_terrain::Terrain;

use super::areas::{AreaCache, Grower, Rect};
use super::checks::{center, center_i, check_checkpoint, connector, dist_between, length, v};
use super::path;
use super::trace::worm_line_clear;
use crate::budget::{Afford, Budget};

/// The most areas one search visits.
pub const MAX_SEARCH_AREAS: usize = 1024;
/// The most nodes a path has (OLX's 50 px splitting included).
pub const MAX_PATH_NODES: usize = 256;

const NONE: u32 = u32::MAX;
const OPEN_CAPACITY: usize = 4 * MAX_SEARCH_AREAS;
/// The checklist's spacing and depth (`checklistRowHeight`, `ColWidth`).
const CHECK_STEP: i32 = 5;
/// The most pixels one checkpoint reads: at most 9 traces of 6 pixels.
const CHECKPOINT_WORST: u64 = 9 * 6;

/// Where a search is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchStatus {
    /// No search was started (or its result was taken).
    Idle,
    /// It needs more ticks.
    Pending,
    /// [`Search::path`] holds the path.
    Found,
    Failed(SearchError),
}

/// Why a search found no path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchError {
    /// The start is outside the map or in no area (`findPath` returns NULL).
    NoStartArea,
    /// Every reachable area was tried.
    NoPath,
    /// [`MAX_SEARCH_AREAS`] areas were visited.
    TooManyAreas,
    /// The shared cache is full.
    CacheFull,
    /// The path would have more than [`MAX_PATH_NODES`] nodes.
    PathTooLong,
    /// The cache is for a level of another size.
    WrongLevel,
}

#[derive(Clone, Copy, Debug)]
struct Node {
    rect: Rect,
    pred: u32,
    /// `dist_from_source`.
    dist: f64,
    version: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Open {
    /// `expected_min_total_dist`'s bits (non-negative, so they order like it).
    f: u64,
    seq: u32,
    node: u32,
    version: u32,
}

/// A checklist point: its distance² to the target, its insertion order,
/// where it is and its outward step.
#[derive(Clone, Copy, Debug)]
struct Check {
    d2: i64,
    order: u32,
    x: i32,
    y: i32,
    dist: (i32, i32),
}

#[derive(Clone, Copy, Debug)]
enum Phase {
    Idle,
    Start,
    Pop,
    Goal(u32),
    Expand {
        node: u32,
        next: u32,
    },
    /// Growing a new area at `at`; then `resume` (a node and its next
    /// checkpoint), or the first pop for the start area.
    Grow {
        grower: Grower,
        at: (i32, i32),
        pred: u32,
        resume: Option<(u32, u32)>,
    },
    Simplify {
        o: u32,
        j: u32,
        count: u32,
        len: f32,
        last: u32,
    },
    Done,
    Failed(SearchError),
}

/// One bot's pathfinder. It allocates when made, never while searching.
#[derive(Clone, Debug)]
pub struct Search {
    cache: Arc<AreaCache>,
    from: (i32, i32),
    to: (i32, i32),
    phase: Phase,
    nodes: Vec<Node>,
    /// Per cache area: the search generation and node that visited it.
    node_of: Vec<(u32, u32)>,
    generation: u32,
    open: BinaryHeap<Reverse<Open>>,
    seq: u32,
    /// The checkpoints of the area being expanded.
    checks: Vec<Check>,
    raw: Vec<Vec2>,
    next: Vec<u16>,
    path: Vec<Vec2>,
}

impl Search {
    pub fn new(cache: Arc<AreaCache>) -> Self {
        let (w, h) = (cache.width() as usize, cache.height() as usize);
        Search {
            node_of: alloc::vec![(0, 0); cache.capacity()],
            cache,
            from: (0, 0),
            to: (0, 0),
            phase: Phase::Idle,
            nodes: Vec::with_capacity(MAX_SEARCH_AREAS),
            generation: 0,
            open: BinaryHeap::with_capacity(OPEN_CAPACITY),
            seq: 0,
            checks: Vec::with_capacity((2 * w + 2 * h) / CHECK_STEP as usize + 8),
            raw: Vec::with_capacity(MAX_PATH_NODES),
            next: Vec::with_capacity(MAX_PATH_NODES),
            path: Vec::with_capacity(MAX_PATH_NODES),
        }
    }

    pub fn cache(&self) -> &Arc<AreaCache> {
        &self.cache
    }

    /// Starts a search from `from` to `to`, dropping any other.
    pub fn start(&mut self, from: (i32, i32), to: (i32, i32)) {
        self.generation = self.generation.wrapping_add(1);
        if self.generation == 0 {
            self.node_of.iter_mut().for_each(|e| *e = (0, 0));
            self.generation = 1;
        }
        (self.from, self.to) = (from, to);
        self.nodes.clear();
        self.open.clear();
        self.path.clear();
        self.phase = Phase::Start;
    }

    /// Forgets the search and its path.
    pub fn cancel(&mut self) {
        self.path.clear();
        self.phase = Phase::Idle;
    }

    /// Where the running (or last) search starts and ends.
    pub fn endpoints(&self) -> ((i32, i32), (i32, i32)) {
        (self.from, self.to)
    }

    pub fn status(&self) -> SearchStatus {
        match self.phase {
            Phase::Idle => SearchStatus::Idle,
            Phase::Done => SearchStatus::Found,
            Phase::Failed(e) => SearchStatus::Failed(e),
            _ => SearchStatus::Pending,
        }
    }

    /// The path found, from the start to the target (empty unless
    /// [`SearchStatus::Found`]).
    pub fn path(&self) -> &[Vec2] {
        if matches!(self.phase, Phase::Done) { &self.path } else { &[] }
    }

    /// How many areas this search has visited.
    pub fn areas_visited(&self) -> usize {
        self.nodes.len()
    }

    /// Searches on until done or until `b` is spent.
    pub fn step(&mut self, t: &Terrain, b: &mut Budget) -> SearchStatus {
        if !self.cache.same_size(t) && !matches!(self.phase, Phase::Idle) {
            self.phase = Phase::Failed(SearchError::WrongLevel);
        }
        loop {
            match self.phase {
                Phase::Idle | Phase::Done | Phase::Failed(_) => return self.status(),
                Phase::Start => {
                    if b.afford(u64::from(self.scan_cost())) != Afford::Yes {
                        return SearchStatus::Pending;
                    }
                    self.phase = Phase::Pop;
                    self.add_area_node(self.from, NONE, None, b);
                }
                Phase::Pop => match self.pop() {
                    Some(n) => self.phase = Phase::Goal(n),
                    None if self.nodes.is_empty() => self.phase = Phase::Failed(SearchError::NoStartArea),
                    None => self.phase = Phase::Failed(SearchError::NoPath),
                },
                Phase::Goal(n) => {
                    let c = center(self.nodes[n as usize].rect);
                    match worm_line_clear(t, b, v(self.to), c) {
                        None => return SearchStatus::Pending,
                        Some(true) => self.build_path(n),
                        Some(false) => {
                            self.fill_checks(n);
                            self.phase = Phase::Expand { node: n, next: 0 };
                        }
                    }
                }
                Phase::Expand { node, next } => {
                    let Some(&Check { x, y, dist, .. }) = self.checks.get(next as usize) else {
                        self.phase = Phase::Pop;
                        continue;
                    };
                    if b.afford(CHECKPOINT_WORST + u64::from(self.scan_cost())) != Afford::Yes {
                        return SearchStatus::Pending;
                    }
                    self.phase = Phase::Expand { node, next: next + 1 };
                    if let Some(p) = check_checkpoint(t, b, (x, y), dist) {
                        self.add_area_node(p, node, Some((node, next + 1)), b);
                    }
                }
                Phase::Grow { mut grower, at, pred, resume } => match grower.step(t, b) {
                    None => {
                        self.phase = Phase::Grow { grower, at, pred, resume };
                        return SearchStatus::Pending;
                    }
                    Some(r) => {
                        self.phase = match resume {
                            Some((node, next)) => Phase::Expand { node, next },
                            None => Phase::Pop,
                        };
                        // Another bot sharing the cache may have added an
                        // area holding `at` meanwhile: take it, not a copy.
                        let found = match b.afford(u64::from(self.scan_cost())) {
                            Afford::Yes => self.find(at, b),
                            _ => None,
                        };
                        if let Some(a) = found {
                            self.visit_cached(a, at, pred);
                        } else if r.is_area() {
                            match self.cache.insert(r) {
                                Some(a) => self.visit(a, r, pred),
                                None => self.phase = Phase::Failed(SearchError::CacheFull),
                            }
                        }
                    }
                },
                Phase::Simplify { .. } => {
                    if !self.simplify(t, b) {
                        return SearchStatus::Pending;
                    }
                    path::split_up(&self.raw, &self.next, &mut self.path, MAX_PATH_NODES);
                    self.phase = Phase::Done;
                }
            }
        }
    }

    /// The best queued node (lazily skipping superseded entries).
    fn pop(&mut self) -> Option<u32> {
        while let Some(Reverse(o)) = self.open.pop() {
            if self.nodes[o.node as usize].version == o.version {
                return Some(o.node);
            }
        }
        None
    }

    /// `addAreaNode(p, pred)` (`CWormBot.cpp:648-679`): a point outside the
    /// map is ignored; an area holding `p` is reached from `pred` if that
    /// is shorter; otherwise a new area is grown at `p` (then `resume`).
    fn add_area_node(&mut self, p: (i32, i32), pred: u32, resume: Option<(u32, u32)>, b: &mut Budget) {
        let (w, h) = (self.cache.width() as i64, self.cache.height() as i64);
        if p.0 < 0 || i64::from(p.0) >= w || p.1 < 0 || i64::from(p.1) >= h {
            return;
        }
        match self.find(p, b) {
            Some(a) => self.visit_cached(a, p, pred),
            None => {
                let grower = Grower::new(p, self.cache.width(), self.cache.height());
                self.phase = Phase::Grow { grower, at: p, pred, resume };
            }
        }
    }

    /// What a scan of the cache costs: a unit per 64 areas, so that a
    /// search does not get slower, unpaid, as the cache fills up.
    fn scan_cost(&self) -> u32 {
        (self.cache.len() / 64 + 1) as u32
    }

    /// [`AreaCache::find`], paid for (the caller checked it can pay).
    fn find(&self, p: (i32, i32), b: &mut Budget) -> Option<usize> {
        b.spend(self.scan_cost());
        self.cache.find(p.0, p.1)
    }

    fn visit_cached(&mut self, a: usize, p: (i32, i32), pred: u32) {
        let r = self.cache.get(a).unwrap_or(Rect { x0: p.0, y0: p.1, x1: p.0, y1: p.1 });
        self.visit(a, r, pred);
    }

    /// Area `a` (`r`) reached from node `pred`: a new node, or a shorter
    /// way to a known one (`expected_min_total_dist`, `setLastArea`).
    fn visit(&mut self, a: usize, r: Rect, pred: u32) {
        let Some(slot) = self.node_of.get(a).copied() else { return };
        let dist_via = |nodes: &[Node]| match nodes.get(pred as usize) {
            Some(p) => p.dist + dist_between(&r, &p.rect),
            None => 0.0,
        };
        let n = if slot.0 == self.generation {
            let n = slot.1;
            if pred == NONE {
                return;
            }
            let via = dist_via(&self.nodes);
            let node = &mut self.nodes[n as usize];
            if node.dist <= via {
                return;
            }
            node.pred = pred;
            node.dist = via;
            node.version += 1;
            n
        } else {
            if self.nodes.len() >= MAX_SEARCH_AREAS {
                self.phase = Phase::Failed(SearchError::TooManyAreas);
                return;
            }
            let dist = dist_via(&self.nodes);
            self.nodes.push(Node { rect: r, pred, dist, version: 0 });
            let n = (self.nodes.len() - 1) as u32;
            self.node_of[a] = (self.generation, n);
            n
        };
        if self.open.len() >= OPEN_CAPACITY {
            self.phase = Phase::Failed(SearchError::TooManyAreas);
            return;
        }
        let node = self.nodes[n as usize];
        let f = node.dist + length(center_i(node.rect), self.to);
        self.seq = self.seq.wrapping_add(1);
        self.open.push(Reverse(Open { f: f.to_bits(), seq: self.seq, node: n, version: node.version }));
    }

    /// `initChecklists` and `forEachChecklistItem` (`CWormBot.cpp:321-328`,
    /// `384-440`): points every 5 px along each edge, centred, nearest to
    /// the target first (ties: left, right, top, bottom, in edge order).
    fn fill_checks(&mut self, n: u32) {
        let r = self.nodes[n as usize].rect;
        let (sx, sy) = (r.x1 - r.x0, r.y1 - r.y0);
        let cols = (sx - 3) / CHECK_STEP + 1;
        let rows = (sy - 3) / CHECK_STEP + 1;
        let col_start = (sx - (cols - 1) * CHECK_STEP) / 2;
        let row_start = (sy - (rows - 1) * CHECK_STEP) / 2;
        self.checks.clear();
        let to = self.to;
        let mut order = 0;
        let mut push = |checks: &mut Vec<Check>, x: i32, y: i32, dist: (i32, i32)| {
            if checks.len() < checks.capacity() {
                let d2 = (i64::from(x) - i64::from(to.0)).pow(2) + (i64::from(y) - i64::from(to.1)).pow(2);
                checks.push(Check { d2, order, x, y, dist });
                order += 1;
            }
        };
        for (x, dx) in [(r.x0, -CHECK_STEP), (r.x1, CHECK_STEP)] {
            for i in 0..rows {
                push(&mut self.checks, x, r.y0 + row_start + i * CHECK_STEP, (dx, 0));
            }
        }
        for (y, dy) in [(r.y0, -CHECK_STEP), (r.y1, CHECK_STEP)] {
            for i in 0..cols {
                push(&mut self.checks, r.x0 + col_start + i * CHECK_STEP, y, (0, dy));
            }
        }
        self.checks.sort_unstable_by_key(|c| (c.d2, c.order));
    }

    /// `buildPath` (`CWormBot.cpp:593-615`): from the start,
    /// `centre(a1), centre(a1), conn(a2, a1), centre(a2), …, centre(an), target`.
    fn build_path(&mut self, last: u32) {
        self.raw.clear();
        self.raw.push(v(self.to));
        let mut a = last;
        for _ in 0..=self.nodes.len() {
            let Some(node) = self.nodes.get(a as usize).copied() else { break };
            let conn = match self.nodes.get(node.pred as usize) {
                Some(p) => connector(&node.rect, &p.rect),
                None => center_i(node.rect),
            };
            if self.raw.len() + 2 > MAX_PATH_NODES {
                self.phase = Phase::Failed(SearchError::PathTooLong);
                return;
            }
            self.raw.push(center(node.rect));
            self.raw.push(v(conn));
            a = node.pred;
        }
        self.raw.reverse();
        self.next.clear();
        self.next.extend((1..=self.raw.len()).map(|i| i as u16));
        self.phase = Phase::Simplify { o: 0, j: 0, count: 0, len: 0.0, last: 0 };
    }

    /// `simplifyPath`'s first loop (`CWormBot.cpp:838-862`), resumable: from
    /// each node of the (shortened) path, a node at least 3 ahead becomes
    /// its successor when it is closer than the summed squared lengths of
    /// the way there and a worm line reaches it. False when `b` ran out.
    fn simplify(&mut self, t: &Terrain, b: &mut Budget) -> bool {
        let Phase::Simplify { mut o, mut j, mut count, mut len, mut last } = self.phase else { return true };
        let n = self.raw.len() as u32;
        while o < n {
            if j >= n {
                o = u32::from(self.next[o as usize]);
                (j, count, len, last) = (o, 0, 0.0, o);
                continue;
            }
            let (pj, po, pl) = (self.raw[j as usize], self.raw[o as usize], self.raw[last as usize]);
            let new_len = len + (pj - pl).length2();
            let dist = (pj - po).length2();
            let mut kept = new_len;
            if count >= 3 && dist < new_len {
                match worm_line_clear(t, b, pj, po) {
                    None => {
                        self.phase = Phase::Simplify { o, j, count, len, last };
                        return false;
                    }
                    Some(true) => {
                        self.next[o as usize] = j as u16;
                        kept = dist;
                    }
                    Some(false) => {}
                }
            }
            (len, last, j, count) = (kept, j, j + 1, count + 1);
        }
        true
    }
}
