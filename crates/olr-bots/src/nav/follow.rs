//! Following a path: which node the bot heads for (`AI_MoveToTarget`'s node
//! advance, `src/common/CWormBot.cpp:3578-3627`, notes §1.5), and the
//! checks it makes before jumping.

use alloc::vec::Vec;

use olr_math::{AbsTime, Vec2};
use olr_sim::SimSettings;
use olr_terrain::Terrain;

use super::search::MAX_PATH_NODES;
use super::trace::{worm_line_clear, worm_line_worst};
use crate::budget::Budget;

/// How many later nodes are checked for a worm line each tick (OLX checks
/// them all; notes §4.3).
pub const LOOKAHEAD: usize = 8;

/// A path being followed and the node the bot heads for.
#[derive(Clone, Debug)]
pub struct Follower {
    nodes: Vec<Vec2>,
    current: usize,
    /// `fLastGoBack`.
    last_go_back: Option<AbsTime>,
}

impl Default for Follower {
    fn default() -> Self {
        Self::new()
    }
}

impl Follower {
    pub fn new() -> Self {
        Follower { nodes: Vec::with_capacity(MAX_PATH_NODES), current: 0, last_go_back: None }
    }

    /// Follows `path` from its first node (`NEW_psCurrentNode = NEW_psPath`).
    pub fn set(&mut self, path: &[Vec2]) {
        self.nodes.clear();
        self.nodes.extend(path.iter().take(self.nodes.capacity()));
        self.current = 0;
    }

    pub fn clear(&mut self) {
        self.nodes.clear();
        self.current = 0;
    }

    pub fn nodes(&self) -> &[Vec2] {
        &self.nodes
    }

    /// The node headed for, if there is a path.
    pub fn current(&self) -> Option<Vec2> {
        self.nodes.get(self.current).copied()
    }

    pub fn current_index(&self) -> usize {
        self.current
    }

    /// Heads for node `i` (`NEW_psCurrentNode = node`), if there is one.
    pub fn head_for(&mut self, i: usize) {
        if i < self.nodes.len() {
            self.current = i;
        }
    }

    /// Adds `p` after the last node (`AI_CreatePath`'s path extension), if
    /// there is room; gives whether it did.
    pub fn push(&mut self, p: Vec2) -> bool {
        let room = self.nodes.len() < self.nodes.capacity();
        if room {
            self.nodes.push(p);
        }
        room
    }

    /// The node advance (`CWormBot.cpp:3578-3627`), then the jump check
    /// for the node it heads for: within 10 px of the current node, go on
    /// to the next; then head for the farthest of the next [`LOOKAHEAD`]
    /// nodes a worm line reaches. If none is reached, at most once a
    /// second, go back to the nearest earlier node (or this one) a worm
    /// line reaches.
    ///
    /// The jump check (`isJumpingGivingDisadvantage`) always keeps its
    /// share: a node is looked at only when the budget still fits that
    /// node's worm line and then the jump check of whichever node the bot
    /// would head for. A scan cut short by the budget does not go back.
    pub fn advance(&mut self, pos: Vec2, t: &Terrain, b: &mut Budget, now: AbsTime, s: &SimSettings) -> Step {
        let Some(node) = self.current() else { return Step::default() };
        if (node - pos).length2() <= 100.0 && self.current + 1 < self.nodes.len() {
            self.current += 1;
        }
        let rise = pos + Vec2::new(0.0, jump_rise(s));
        let jump_cost = |n: Vec2| worm_line_worst(n, rise);
        let mut reserved = jump_cost(self.nodes[self.current]);
        let (mut seen, mut cut) = (false, false);
        let end = (self.current + LOOKAHEAD).min(self.nodes.len() - 1);
        for n in self.current + 1..=end {
            let need =
                worm_line_worst(self.nodes[n], pos).saturating_add(reserved.max(jump_cost(self.nodes[n])));
            if need > u64::from(b.left()) {
                cut = true;
                break;
            }
            match worm_line_clear(t, b, self.nodes[n], pos) {
                None => {
                    cut = true;
                    break;
                }
                Some(true) => {
                    self.current = n;
                    reserved = jump_cost(self.nodes[n]);
                    seen = true;
                }
                Some(false) => {}
            }
        }
        let due = self.last_go_back.is_none_or(|t0| now.0.saturating_sub(t0.0) >= 1000);
        if !seen && !cut && due {
            for n in (self.current.saturating_sub(LOOKAHEAD)..=self.current).rev() {
                let need = worm_line_worst(self.nodes[n], pos)
                    .saturating_add(reserved.max(jump_cost(self.nodes[n])));
                if need > u64::from(b.left()) {
                    break;
                }
                match worm_line_clear(t, b, self.nodes[n], pos) {
                    None => break,
                    Some(true) => {
                        if n != self.current {
                            self.current = n;
                            self.last_go_back = Some(now);
                        }
                        break;
                    }
                    Some(false) => {}
                }
            }
        }
        let node = self.current();
        let jump_loses_node = node.and_then(|n| jump_loses_node(n, pos, s, t, b));
        Step { node, jump_loses_node, cut }
    }

    /// Goes back one node (the stuck handling's `psPrev`), if it can.
    pub fn back(&mut self) {
        self.current = self.current.saturating_sub(1);
    }

    /// How many nodes lie after the current one up to the last.
    pub fn nodes_left(&self) -> usize {
        self.nodes.len().saturating_sub(self.current + 1)
    }

    /// The last node.
    pub fn last(&self) -> Option<Vec2> {
        self.nodes.last().copied()
    }
}

/// What [`Follower::advance`] found.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Step {
    /// The node to head for, if there is a path.
    pub node: Option<Vec2>,
    /// Whether a jump now would lose sight of it (`None`: not checked, the
    /// budget is spent).
    pub jump_loses_node: Option<bool>,
    /// Whether the budget cut the scan short.
    pub cut: bool,
}

/// `estimateYDiffAfterJump(0.3)` (`CWormBot.cpp:3316-3322`), with OLX's bug:
/// the jump force is read for the gravity too, so it is about -22 px with
/// Classic's -65.
pub fn jump_rise(s: &SimSettings) -> f32 {
    let force = s.worm_jump_force;
    let grav = s.worm_jump_force;
    grav * 0.3 * 0.3 * 0.5 + force * 0.3
}

/// `isJumpingGivingDisadvantage` (`CWormBot.cpp:3324-3333`): a jump loses
/// the node when no worm line reaches it from where the jump would get to.
/// `None` when the budget does not allow the check this tick.
pub fn jump_loses_node(node: Vec2, pos: Vec2, s: &SimSettings, t: &Terrain, b: &mut Budget) -> Option<bool> {
    worm_line_clear(t, b, node, pos + Vec2::new(0.0, jump_rise(s))).map(|clear| !clear)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;
    use olr_data::level::{Material, MaterialGrid};

    fn terrain(rock: impl Fn(u32, u32) -> bool) -> Terrain {
        let cells: Vec<Material> = (0..400 * 100)
            .map(|i| if rock(i % 400, i / 400) { Material::Rock } else { Material::Background })
            .collect();
        Terrain::from_grid(&MaterialGrid::from_cells(400, 100, cells).unwrap())
    }

    /// Task 4 review: the lookahead must leave room for the jump check, or
    /// a bot under a ledge never jumps.
    #[test]
    fn the_jump_check_is_never_starved() {
        let t = terrain(|_, _| false);
        let s = SimSettings::lx56();
        let nodes: Vec<Vec2> = (1..20).map(|i| Vec2::new(20.0 * i as f32, 50.0)).collect();
        for limit in [120, 200, 400, 900, 3000] {
            let mut f = Follower::new();
            f.set(&nodes);
            let mut b = Budget::new(limit);
            let step = f.advance(Vec2::new(10.0, 50.0), &t, &mut b, AbsTime(1000), &s);
            assert!(step.jump_loses_node.is_some(), "budget {limit}: the jump check ran");
            assert!(b.used() <= limit);
        }
    }

    /// Task 4 review: when the budget cuts the forward scan short, the bot
    /// does not go back to an earlier node.
    #[test]
    fn a_scan_cut_short_does_not_go_back() {
        // A rock pillar at x 45..=48 between the bot and nodes 2 and 3.
        let t = terrain(|x, _| (45..=48).contains(&x));
        let s = SimSettings::lx56();
        let nodes =
            [Vec2::new(5.0, 50.0), Vec2::new(20.0, 50.0), Vec2::new(60.0, 50.0), Vec2::new(390.0, 50.0)];
        let pos = Vec2::new(30.0, 50.0);
        let mut f = Follower::new();
        f.set(&nodes);
        f.head_for(2);
        let step = f.advance(pos, &t, &mut Budget::new(600), AbsTime(5000), &s);
        assert!(step.cut, "node 3 is too far for this budget");
        assert_eq!(f.current_index(), 2, "no go-back after a cut scan");
        // With the budget for the whole scan it goes back to node 1.
        let step = f.advance(pos, &t, &mut Budget::new(8000), AbsTime(5000), &s);
        assert!(!step.cut);
        assert_eq!(f.current_index(), 1);
    }
}
