//! What a headless match exercised (spec §7.1): weapons fired, projectile
//! actions triggered, bonuses collected, rope attachments and kills by
//! weapon. The required lists come from the mod itself: every weapon that
//! reports its shots, and every action kind a projectile can apply.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use olr_data::mod_def::ActionKind;
use olr_sim::{ActionTally, BonusItem, EventKind, SimMod, TickEvents, WeaponType, World, projectile_actions};

/// Counts of what one match or several (merged) did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Coverage {
    /// Shots (`WeaponFired`) by weapon id.
    pub fired: BTreeMap<i32, u64>,
    /// Projectile actions applied, by kind name (`ActionKind`'s, `Other`
    /// for any other value).
    pub actions: BTreeMap<String, u64>,
    /// Bonuses picked up: `health`, or the weapon id held.
    pub bonuses: BTreeMap<String, u64>,
    /// Ropes that caught on the map, and on a worm.
    pub rope_on_map: u64,
    pub rope_on_worm: u64,
    /// Kills by the killer's selected weapon when the victim died (the
    /// simulation does not record which weapon dealt the blow), suicides
    /// and deaths with nobody to credit apart.
    pub kills: BTreeMap<i32, u64>,
    pub suicides: u64,
    pub other_deaths: u64,
    /// Events by kind, in [`EVENT_KINDS`] order.
    pub events: [u64; EVENT_KINDS.len()],
    /// Each worm's rope attached after the last tick seen.
    attached: Vec<bool>,
}

/// The kinds of `olr_sim::EventKind`, as the report names them.
pub const EVENT_KINDS: [&str; 12] = [
    "Sound",
    "TerrainEdit",
    "Explosion",
    "SpawnCosmetic",
    "WeaponFired",
    "Damage",
    "Kill",
    "BonusPickup",
    "Spawned",
    "Message",
    "GameOver",
    "LiveState",
];

fn event_index(kind: &EventKind) -> usize {
    match kind {
        EventKind::Sound { .. } => 0,
        EventKind::TerrainEdit { .. } => 1,
        EventKind::Explosion { .. } => 2,
        EventKind::SpawnCosmetic { .. } => 3,
        EventKind::WeaponFired { .. } => 4,
        EventKind::Damage { .. } => 5,
        EventKind::Kill { .. } => 6,
        EventKind::BonusPickup { .. } => 7,
        EventKind::Spawned { .. } => 8,
        EventKind::Message(_) => 9,
        EventKind::GameOver(_) => 10,
        EventKind::LiveState { .. } => 11,
    }
}

/// The name of an action kind, as the report lists it.
pub fn action_name(kind: ActionKind) -> String {
    match kind {
        ActionKind::Other(_) => "Other".into(),
        k => format!("{k:?}"),
    }
}

impl Coverage {
    /// Adds a tick: `events` are its events, `world` the world after it.
    pub fn observe(&mut self, world: &World, events: &TickEvents) {
        for e in events {
            self.events[event_index(&e.kind)] += 1;
            match e.kind {
                EventKind::WeaponFired { weapon, .. } => *self.fired.entry(weapon).or_default() += 1,
                EventKind::BonusPickup { kind, .. } => {
                    let key = match kind {
                        BonusItem::Health => "health".to_string(),
                        BonusItem::Weapon(id) => format!("weapon {id}"),
                    };
                    *self.bonuses.entry(key).or_default() += 1;
                }
                EventKind::Kill { victim, killer } if killer >= 0 && killer != victim => {
                    let held = world
                        .worms()
                        .get(killer as usize)
                        .and_then(|w| w.slots.get(usize::try_from(w.current_weapon).ok()?).map(|s| s.weapon));
                    *self.kills.entry(held.unwrap_or(-1)).or_default() += 1;
                }
                EventKind::Kill { victim, killer } if killer == victim => self.suicides += 1,
                EventKind::Kill { .. } => self.other_deaths += 1,
                _ => {}
            }
        }
        self.attached.resize(world.worms().len(), false);
        for (was, w) in self.attached.iter_mut().zip(world.worms()) {
            let now = w.rope.attached;
            if now && !*was {
                if w.rope.player_attached >= 0 {
                    self.rope_on_worm += 1;
                } else {
                    self.rope_on_map += 1;
                }
            }
            *was = now;
        }
    }

    /// Adds the actions a match's world applied (at its end).
    pub fn add_actions(&mut self, tally: &ActionTally) {
        for (kind, n) in tally.iter().filter(|&(_, n)| n > 0) {
            *self.actions.entry(action_name(kind)).or_default() += n;
        }
    }

    /// Adds `other`'s counts (another match of the same mod).
    pub fn merge(&mut self, other: &Coverage) {
        fn add<K: Ord + Clone>(a: &mut BTreeMap<K, u64>, b: &BTreeMap<K, u64>) {
            for (k, n) in b {
                *a.entry(k.clone()).or_default() += n;
            }
        }
        add(&mut self.fired, &other.fired);
        add(&mut self.actions, &other.actions);
        add(&mut self.bonuses, &other.bonuses);
        add(&mut self.kills, &other.kills);
        self.rope_on_map += other.rope_on_map;
        self.rope_on_worm += other.rope_on_worm;
        self.suicides += other.suicides;
        self.other_deaths += other.other_deaths;
        for (a, b) in self.events.iter_mut().zip(other.events) {
            *a += b;
        }
    }

    /// Every shot fired.
    pub fn shots(&self) -> u64 {
        self.fired.values().sum()
    }

    /// The weapons a match must fire: all but specials, which are shot on
    /// the client only and report no `WeaponFired` (Classic and MW 1.0 have none).
    pub fn required_weapons(sim: &SimMod) -> BTreeSet<i32> {
        (0..sim.weapons().len() as i32)
            .filter(|&i| sim.weapons()[i as usize].kind != WeaponType::Special)
            .collect()
    }

    /// The action kinds a mod's projectiles can apply (`projectile_actions`),
    /// `Nothing` aside (it does nothing to see).
    pub fn required_actions(sim: &SimMod) -> BTreeSet<String> {
        sim.projectiles()
            .iter()
            .flat_map(|p| projectile_actions(&p.def))
            .filter(|&k| k != ActionKind::Nothing)
            .map(action_name)
            .collect()
    }

    /// The names of the required weapons never fired.
    pub fn missing_weapons(&self, sim: &SimMod) -> Vec<String> {
        Self::required_weapons(sim)
            .into_iter()
            .filter(|w| !self.fired.contains_key(w))
            .map(|w| sim.weapons()[w as usize].name.clone())
            .collect()
    }

    /// The required action kinds never triggered.
    pub fn missing_actions(&self, sim: &SimMod) -> Vec<String> {
        Self::required_actions(sim).into_iter().filter(|a| !self.actions.contains_key(a)).collect()
    }

    /// The report `olr-cli match --coverage` prints.
    pub fn report(&self, sim: &SimMod) -> String {
        let name = |id: i32| sim.weapon(id).map_or_else(|| format!("#{id}"), |w| w.name.clone());
        let mut s = String::new();
        let required = Self::required_weapons(sim);
        let fired = required.iter().filter(|w| self.fired.contains_key(w)).count();
        let _ = writeln!(s, "weapons fired: {fired} of {} ({} shots)", required.len(), self.shots());
        for (i, w) in sim.weapons().iter().enumerate() {
            let n = self.fired.get(&(i as i32)).copied().unwrap_or(0);
            let note = if required.contains(&(i as i32)) { "" } else { " (special: not reported)" };
            let _ = writeln!(s, "  {:>6}  {}{note}", n, w.name);
        }
        let required = Self::required_actions(sim);
        let hit = required.iter().filter(|a| self.actions.contains_key(*a)).count();
        let _ = writeln!(s, "actions triggered: {hit} of the {} the mod uses", required.len());
        let names: BTreeSet<&String> = required.iter().chain(self.actions.keys()).collect();
        for a in names {
            let n = self.actions.get(a).copied().unwrap_or(0);
            let note = if required.contains(a) { "" } else { " (not required)" };
            let _ = writeln!(s, "  {n:>6}  {a}{note}");
        }
        let _ = writeln!(s, "events:");
        for (name, n) in EVENT_KINDS.iter().zip(self.events) {
            let _ = writeln!(s, "  {n:>6}  {name}");
        }
        let _ = writeln!(s, "bonuses collected: {}", self.bonuses.values().sum::<u64>());
        for (k, n) in &self.bonuses {
            let what = match k.strip_prefix("weapon ").and_then(|id| id.parse().ok()) {
                Some(id) => format!("weapon: {}", name(id)),
                None => k.clone(),
            };
            let _ = writeln!(s, "  {n:>6}  {what}");
        }
        let _ =
            writeln!(s, "rope attachments: {} on the map, {} on worms", self.rope_on_map, self.rope_on_worm);
        let kills: u64 = self.kills.values().sum();
        let _ = writeln!(
            s,
            "kills: {kills} by the killer's selected weapon, {} suicides, {} uncredited deaths",
            self.suicides, self.other_deaths
        );
        for (&w, n) in &self.kills {
            let _ = writeln!(s, "  {n:>6}  {}", name(w));
        }
        s
    }
}
