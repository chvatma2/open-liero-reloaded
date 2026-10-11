//! OLX's weapon selection before the match (spec §21 G4;
//! `CWormHumanInputHandler::initWeaponSelection` and
//! `doWeaponSelectionFrame`, `src/common/CWormHuman.cpp:702-1000`): five
//! slots, then Random and Done. Up and down move between the rows; left
//! and right change the row's weapon to the next one the restrictions
//! enable (six at a time while Select weapon is held); Fire on Random
//! draws random weapons (`CWorm::GetRandomWeapons`); releasing Fire on
//! Done ends the selection. The game's own keys drive it, keyboard or
//! gamepad, as OLX's controls do; the screen also takes the mouse.
//!
//! Pure logic: the screen (`screens/weapon_select.rs`) draws it, the app
//! gives it the mod's weapons ([`WeaponMenu`]) and the keys held.

use rl_bots::{BotRng, get_random_weapons};
use rl_sim::{Keys, MAX_SLOTS};

/// OLX's auto-repeat of a held direction (the stick's, `WeaponSelStickAxis`,
/// `CWormHuman.cpp:752-785`; keys repeat at the system's rate): the first
/// repeat after 0.4 s, then every 0.12 s.
pub const REPEAT_DELAY: f64 = 0.4;
pub const REPEAT_EVERY: f64 = 0.12;

/// `cSelWeapon` held multiplies a change (`CWormHuman.cpp:880`).
pub const FAST_STEP: i32 = 6;

/// The weapons a match offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WeaponMenu {
    /// The mod's weapon names, by id.
    pub names: Vec<String>,
    /// Whether the restrictions enable each (`WeaponRules::is_enabled`).
    pub enabled: Vec<bool>,
    /// `findEnabledWeapon`: what a slot gets when its remembered weapon is
    /// not enabled (-1: none is).
    pub fallback: i32,
    /// Seeds the Random button.
    pub seed: u32,
}

impl WeaponMenu {
    /// Whether there is a choice: OLX skips the dialog when the
    /// restrictions enable one weapon or none (`CWormHuman.cpp:740-747`).
    pub fn needs_choice(&self) -> bool {
        self.enabled.iter().filter(|&&e| e).count() > 1
    }

    fn is_enabled(&self, id: i32) -> bool {
        usize::try_from(id).ok().and_then(|i| self.enabled.get(i)).copied().unwrap_or(false)
    }

    /// Weapon `id`'s name ("-" for none).
    pub fn name(&self, id: i32) -> &str {
        usize::try_from(id).ok().and_then(|i| self.names.get(i)).map_or("-", String::as_str)
    }
}

/// The rows: the slots, then Random, then Done.
pub const RANDOM_ROW: usize = MAX_SLOTS;
pub const DONE_ROW: usize = MAX_SLOTS + 1;
const ROWS: usize = MAX_SLOTS + 2;

/// A selection in progress.
#[derive(Clone, Debug)]
pub struct WeaponSelect {
    slots: [i32; MAX_SLOTS],
    row: usize,
    rng: BotRng,
    /// The keys held last time ([`WeaponSelect::on_keys`]).
    held: u8,
    /// A direction held since, and when it last stepped.
    repeat: Option<(u8, f64, bool)>,
}

impl WeaponSelect {
    /// The selection starting from the profile's `remembered` weapon names
    /// (ignoring ASCII case, as `FindWeaponId`); a slot whose weapon the
    /// mod lacks or the restrictions do not enable gets `menu.fallback`.
    ///
    /// OLX: src/common/CWormHuman.cpp:722-730.
    pub fn new(menu: &WeaponMenu, remembered: &[String]) -> WeaponSelect {
        let slots = std::array::from_fn(|i| {
            let name = remembered.get(i).map_or("", String::as_str);
            let id = if name.is_empty() {
                -1
            } else {
                menu.names.iter().position(|n| n.eq_ignore_ascii_case(name)).map_or(-1, |p| p as i32)
            };
            if menu.is_enabled(id) { id } else { menu.fallback }
        });
        WeaponSelect { slots, row: 0, rng: BotRng::from_seed(menu.seed), held: 0, repeat: None }
    }

    /// The weapon in each slot, by mod id.
    pub fn slots(&self) -> [i32; MAX_SLOTS] {
        self.slots
    }

    /// The row the cursor is on: a slot, [`RANDOM_ROW`] or [`DONE_ROW`].
    pub fn row(&self) -> usize {
        self.row
    }

    /// Puts the cursor on `row`.
    pub fn set_row(&mut self, row: usize) {
        self.row = row.min(ROWS - 1);
    }

    /// Moves the cursor `by` rows, wrapping.
    pub fn move_row(&mut self, by: i32) {
        self.row = (self.row as i32 + by).rem_euclid(ROWS as i32) as usize;
    }

    /// Changes the cursor's slot by `by` enabled weapons (nothing on
    /// Random and Done): OLX's loop, which stops when it comes back to
    /// where it started.
    ///
    /// OLX: src/common/CWormHuman.cpp:872-896.
    pub fn change(&mut self, menu: &WeaponMenu, by: i32) {
        if self.row < MAX_SLOTS {
            self.change_slot(menu, self.row, by);
        }
    }

    /// Changes slot `slot` by `by` enabled weapons.
    pub fn change_slot(&mut self, menu: &WeaponMenu, slot: usize, mut by: i32) {
        let n = menu.names.len() as i32;
        let Some(cur) = self.slots.get_mut(slot) else { return };
        if n == 0 {
            return;
        }
        let start = (*cur).clamp(-1, n - 1);
        let mut id = start.max(0);
        let step = by.signum();
        while by != 0 {
            id = (id + step).rem_euclid(n);
            if menu.is_enabled(id) {
                by -= step;
            }
            if (start < 0 && id == 0) || id == start {
                break;
            }
        }
        *cur = id;
    }

    /// Random weapons the restrictions enable (`CWorm::GetRandomWeapons`).
    pub fn random(&mut self, menu: &WeaponMenu) {
        self.slots = get_random_weapons(&menu.enabled, &mut self.rng);
    }

    /// The game's keys held now (`Keys` bits; keyboard and gamepad as the
    /// player bound them) at time `now` (seconds): steps on each press and
    /// on the auto-repeat of a held direction, Random on Fire's press, and
    /// on Done, Fire's release ends the selection: `true` (OLX checks
    /// `isUp` there so the release does not also fire in the game).
    pub fn on_keys(&mut self, menu: &WeaponMenu, held: Keys, now: f64) -> bool {
        let held = held.0;
        let pressed = held & !self.held;
        let released = self.held & !held;
        self.held = held;
        let dirs = Keys::UP | Keys::DOWN | Keys::LEFT | Keys::RIGHT;
        let mut steps = pressed & dirs;
        match self.repeat {
            Some((dir, _, _)) if held & dir == 0 => self.repeat = None,
            Some((dir, last, repeated)) => {
                let wait = if repeated { REPEAT_EVERY } else { REPEAT_DELAY };
                if now - last >= wait {
                    steps |= dir;
                    self.repeat = Some((dir, now, true));
                }
            }
            None => {}
        }
        if let Some(dir) =
            [Keys::UP, Keys::DOWN, Keys::LEFT, Keys::RIGHT].into_iter().find(|d| pressed & d != 0)
        {
            self.repeat = Some((dir, now, false));
        }
        let fast = if held & Keys::CHANGE != 0 { FAST_STEP } else { 1 };
        let change = i32::from(steps & Keys::RIGHT != 0) - i32::from(steps & Keys::LEFT != 0);
        if change != 0 {
            self.change(menu, change * fast);
        }
        let mv = i32::from(steps & Keys::DOWN != 0) - i32::from(steps & Keys::UP != 0);
        if mv != 0 {
            self.move_row(mv);
        }
        if pressed & Keys::FIRE != 0 && self.row == RANDOM_ROW {
            self.random(menu);
        }
        released & Keys::FIRE != 0 && self.row == DONE_ROW
    }
}
