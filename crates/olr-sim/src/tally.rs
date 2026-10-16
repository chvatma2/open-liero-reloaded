//! What a match's projectiles did, counted for coverage reports (spec
//! §7.1: every action kind the mods use must be triggered), and where
//! their worm hits landed, for the hit invariant. Not state: neither ever
//! makes two worlds unequal, and the checksum leaves them out.

use olr_data::mod_def::{ActionKind, ProjectileDef};

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

/// The worm hits of projectiles (`CProjectile::ProjWormColl`) and
/// explosions (`CClient::Explosion`'s injuries), each checked against the
/// struck worm's position at that moment (`Worm::pos`, which the newest
/// recorded entry equals): `stale` counts the hits whose source was outside
/// the hit geometry of that position (a projectile: `|dx|` or `|dy|` not
/// below `4 + radius`, in whole pixels as the test truncates them; an
/// explosion: farther than 5 px). With [`crate::SimSettings::olx_quirks`]
/// off a hit is never stale. For tests over whole matches; not state.
#[derive(Clone, Copy, Debug, Default)]
pub struct HitAudit {
    pub projectile_hits: u64,
    pub projectile_stale: u64,
    pub explosion_hits: u64,
    pub explosion_stale: u64,
    /// The worm the last stale hit struck (`None` before any).
    pub last_stale_worm: Option<i32>,
}

impl PartialEq for HitAudit {
    /// Always equal: an audit is not state.
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl HitAudit {
    /// A projectile struck `worm`, from within its current hit geometry
    /// or not.
    pub(crate) fn projectile(&mut self, worm: i32, current: bool) {
        self.projectile_hits = self.projectile_hits.saturating_add(1);
        self.projectile_stale = self.projectile_stale.saturating_add(u64::from(!current));
        self.stale_on(worm, current);
    }

    /// An explosion injured `worm`, from within 5 px of it or not.
    pub(crate) fn explosion(&mut self, worm: i32, current: bool) {
        self.explosion_hits = self.explosion_hits.saturating_add(1);
        self.explosion_stale = self.explosion_stale.saturating_add(u64::from(!current));
        self.stale_on(worm, current);
    }

    fn stale_on(&mut self, worm: i32, current: bool) {
        if !current {
            self.last_stale_worm = Some(worm);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::HitAudit;

    #[test]
    fn the_audit_names_the_worm_of_the_last_stale_hit() {
        let mut a = HitAudit::default();
        a.projectile(1, true);
        assert_eq!(a.last_stale_worm, None, "a current hit is not stale");
        a.projectile(2, false);
        a.explosion(3, true);
        assert_eq!(a.last_stale_worm, Some(2));
        a.explosion(0, false);
        assert_eq!((a.projectile_stale, a.explosion_stale, a.last_stale_worm), (1, 1, Some(0)));
    }
}
