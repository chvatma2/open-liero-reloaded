//! What a match's projectiles did, counted for coverage reports (spec
//! §7.1: every action kind the mods use must be triggered). Not state: it
//! never makes two worlds unequal, and the checksum leaves it out.

use rl_data::mod_def::{ActionKind, ProjectileDef};

use crate::actions::timer_has_action;

/// One bucket per named [`ActionKind`], and one for every `Other` value.
pub const ACTION_KINDS: usize = 27;

/// The kinds in bucket order; the last stands for every `Other` value.
const KINDS: [ActionKind; ACTION_KINDS] = [
    ActionKind::Bounce,
    ActionKind::Explode,
    ActionKind::Injure,
    ActionKind::Carve,
    ActionKind::Dirt,
    ActionKind::GreenDirt,
    ActionKind::Disappear,
    ActionKind::Nothing,
    ActionKind::Disappear2,
    ActionKind::GoThrough,
    ActionKind::InjureProj,
    ActionKind::PlaySound,
    ActionKind::InjureWorm,
    ActionKind::ChangeRadius,
    ActionKind::OverwriteOwnSpeed,
    ActionKind::MultiplyOwnSpeed,
    ActionKind::DiffOwnSpeed,
    ActionKind::OverwriteTargetSpeed,
    ActionKind::MultiplyTargetSpeed,
    ActionKind::DiffTargetSpeed,
    ActionKind::HeadingToNextWorm,
    ActionKind::HeadingToOwner,
    ActionKind::HeadingToNextOtherWorm,
    ActionKind::HeadingToNextEnemyWorm,
    ActionKind::HeadingToNextTeamMate,
    ActionKind::HeadTargetToUs,
    ActionKind::Other(-1),
];

fn bucket(kind: ActionKind) -> usize {
    match kind {
        ActionKind::Other(_) => ACTION_KINDS - 1,
        k => KINDS.iter().position(|&x| x == k).unwrap_or(ACTION_KINDS - 1),
    }
}

/// How many times each action kind was applied (`Proj_Action::applyTo`),
/// for the timer, a terrain hit or a worm hit alike.
#[derive(Clone, Copy, Debug, Default)]
pub struct ActionTally([u64; ACTION_KINDS]);

impl PartialEq for ActionTally {
    /// Always equal: a tally is not state.
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl ActionTally {
    /// How many times `kind` was applied (any `Other` value counts as all of them).
    pub fn count(&self, kind: ActionKind) -> u64 {
        self.0[bucket(kind)]
    }

    /// Every bucket with its count, in [`ActionKind`] order (`Other(-1)`
    /// stands for every `Other` value).
    pub fn iter(&self) -> impl Iterator<Item = (ActionKind, u64)> + '_ {
        KINDS.iter().copied().zip(self.0.iter().copied())
    }

    pub(crate) fn record(&mut self, kind: ActionKind) {
        let n = &mut self.0[bucket(kind)];
        *n = n.saturating_add(1);
    }
}

/// The actions projectile `def` can apply, as `LX56ProjectileHandler_doFrame`
/// (`src/common/PhysicsLX56_Projectiles.cpp:1330-1375`) passes them to
/// `applyTo`: its timer's when the timer has one (`Proj_LX56Timer::hasAction`),
/// then its hit's and its player hit's (applied whatever their kind).
pub fn projectile_actions(def: &ProjectileDef) -> impl Iterator<Item = ActionKind> + '_ {
    let timer = def.timer.action.as_ref().filter(|_| timer_has_action(&def.timer)).map(|a| a.kind);
    timer.into_iter().chain([def.hit.kind, def.player_hit.kind])
}
