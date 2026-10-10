//! Following a path: which node the bot heads for (`AI_MoveToTarget`'s node
//! advance, `src/common/CWormBot.cpp:3578-3627`, notes §1.5), and the
//! checks it makes before jumping.

use alloc::vec::Vec;

use rl_math::{AbsTime, Vec2};
use rl_sim::SimSettings;
use rl_terrain::Terrain;

use super::search::MAX_PATH_NODES;
use super::trace::worm_line_clear;
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

    /// The node advance (`CWormBot.cpp:3578-3627`): within 10 px of the
    /// current node, go on to the next; then head for the farthest of the
    /// next [`LOOKAHEAD`] nodes a worm line reaches. If none is reached, at
    /// most once a second, go back to the nearest earlier node (or this one)
    /// a worm line reaches. Gives the node to head for.
    pub fn advance(&mut self, pos: Vec2, t: &Terrain, b: &mut Budget, now: AbsTime) -> Option<Vec2> {
        let node = self.current()?;
        if (node - pos).length2() <= 100.0 && self.current + 1 < self.nodes.len() {
            self.current += 1;
        }
        let mut seen = false;
        let end = (self.current + LOOKAHEAD).min(self.nodes.len() - 1);
        for n in self.current + 1..=end {
            match worm_line_clear(t, b, self.nodes[n], pos) {
                None => break,
                Some(true) => {
                    self.current = n;
                    seen = true;
                }
                Some(false) => {}
            }
        }
        let due = self.last_go_back.is_none_or(|t0| now.0.saturating_sub(t0.0) >= 1000);
        if !seen && due {
            for n in (self.current.saturating_sub(LOOKAHEAD)..=self.current).rev() {
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
        self.current()
    }
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
