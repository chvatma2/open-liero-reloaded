//! Player profiles (spec §8.3, §9.4 `profiles.ron`): a name, a skin and a
//! colour, and optionally the player's own key bindings; OLX's player menu
//! (`src/client/DeprecatedGUI/Menu_Player.cpp`) creates, edits and deletes
//! them.

use std::fmt;

use olr_input::Bindings;
use olr_sim::MAX_SLOTS;
use serde::{Deserialize, Serialize};

/// The version `profiles.ron` is written with; any other is refused.
pub const PROFILES_VERSION: u32 = 1;

/// The longest name, in characters.
/// OLX: src/client/DeprecatedGUI/Menu_Player.cpp:130 (`TXM_SETMAX, 20`).
pub const MAX_NAME_CHARS: usize = 20;

/// The most profiles a file may hold (more are dropped with a warning).
pub const MAX_PROFILES: usize = 64;

/// The colours new profiles take in turn: OLX's local players' (after
/// the first's green: white, strong blue, yellow, then green again).
/// OLX: src/client/ProfileSystem.cpp:69-75 (`DefaultLocalProfiles`)
pub const NEW_COLOURS: [[u8; 3]; 4] = [[255, 255, 255], [20, 90, 255], [255, 220, 0], [0, 177, 0]];

/// The longest weapon name a profile remembers, in bytes (OLX's
/// `profiles.dat` keeps 64: `ProfileSystem.cpp:289`).
pub const MAX_WEAPON_NAME: usize = 64;

/// OLX's weapon slots of a new profile (`ProfileSystem.cpp:374-380`), by
/// name: the weapon selection starts from them where the mod has them.
pub const DEFAULT_WEAPONS: [&str; MAX_SLOTS] =
    ["minigun", "super shotgun", "blaster", "gauss gun", "big nuke"];

/// OLX's bot profiles' colours, CPU 1 to CPU 7 (`AddDefaultBotPlayers`).
/// OLX: src/client/ProfileSystem.cpp:47-59.
pub const CPU_COLOURS: [[u8; 3]; 7] =
    [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 0, 255], [0, 255, 255], [128, 128, 128], [128, 255, 0]];

/// Colours closer than this (RGB distance) to the human's are not given
/// to a bot, so the player's worm stands out.
pub const BOT_COLOUR_DISTANCE: f32 = 120.0;

/// The colours of `n` bots playing against a human of colour `human`:
/// OLX's [`CPU_COLOURS`] in turn, without those close to the human's
/// (all of them if every one is), again from the first after the last.
/// Cosmetic: no simulation state.
pub fn bot_colours(human: [u8; 3], n: usize) -> Vec<[u8; 3]> {
    let dist = |c: [u8; 3]| {
        let d = |i: usize| f32::from(c[i]) - f32::from(human[i]);
        (d(0) * d(0) + d(1) * d(1) + d(2) * d(2)).sqrt()
    };
    let far: Vec<[u8; 3]> = CPU_COLOURS.into_iter().filter(|&c| dist(c) >= BOT_COLOUR_DISTANCE).collect();
    let pool = if far.is_empty() { CPU_COLOURS.to_vec() } else { far };
    (0..n).map(|i| pool[i % pool.len()]).collect()
}

/// Why a profile name is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameError {
    /// Nothing but spaces (OLX's `ReportInvalidPlayerName`).
    Empty,
    /// More than [`MAX_NAME_CHARS`] characters.
    TooLong,
    /// A control character (a tab, a line break, ...).
    Control,
    /// Another profile has it (ignoring case).
    Taken,
}

impl fmt::Display for NameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NameError::Empty => write!(f, "A name is needed."),
            NameError::TooLong => write!(f, "At most {MAX_NAME_CHARS} characters."),
            NameError::Control => write!(f, "No tabs or line breaks."),
            NameError::Taken => write!(f, "Another profile has this name."),
        }
    }
}

/// One player.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub name: String,
    /// A skin pack id or OLX skin name (`olr_content::Content::skin_or_default`).
    pub skin_id: String,
    /// The worm's colour (RGB).
    pub colour: [u8; 3],
    /// The player's own keys; `None` uses the options' bindings.
    pub bindings: Option<Bindings>,
    /// The weapons chosen last, by name, slot by slot (OLX's
    /// `sWeaponSlots`): the weapon selection starts from them.
    pub weapons: Vec<String>,
}

impl Default for Profile {
    /// OLX's first local player: "OpenLieroXor", green, the default skin.
    /// OLX: src/client/ProfileSystem.cpp:69-75 (`DefaultLocalProfiles`).
    fn default() -> Self {
        Profile {
            name: "OpenLieroXor".into(),
            skin_id: "default".into(),
            colour: [0, 177, 0],
            bindings: None,
            weapons: DEFAULT_WEAPONS.map(String::from).to_vec(),
        }
    }
}

/// Every profile and the one playing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profiles {
    pub list: Vec<Profile>,
    /// An index into `list`.
    pub selected: usize,
}

impl Default for Profiles {
    fn default() -> Self {
        Profiles { list: vec![Profile::default()], selected: 0 }
    }
}

impl Profiles {
    /// The profile playing (the first if `selected` is out of range).
    pub fn selected(&self) -> Option<&Profile> {
        self.list.get(self.selected).or(self.list.first())
    }

    /// `name` trimmed, if a profile may have it: not empty, at most
    /// [`MAX_NAME_CHARS`] characters, no control character, and no other
    /// profile's name (ignoring case; profile `except` is the one renamed).
    ///
    /// OLX: src/client/DeprecatedGUI/Menu_Player.cpp:364-377, :609-622
    /// (trimmed, not empty), :130 (`TXM_SETMAX, 20`). OLX keeps duplicate
    /// names and drops non-ASCII characters; this keeps Unicode and refuses
    /// duplicates, so a name picks one profile (`docs/porting/STATUS.md`).
    pub fn check_name(&self, name: &str, except: Option<usize>) -> Result<String, NameError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(NameError::Empty);
        }
        if name.chars().count() > MAX_NAME_CHARS {
            return Err(NameError::TooLong);
        }
        if name.chars().any(char::is_control) {
            return Err(NameError::Control);
        }
        let lower = name.to_lowercase();
        let taken = self
            .list
            .iter()
            .enumerate()
            .any(|(i, p)| Some(i) != except && p.name.trim().to_lowercase() == lower);
        if taken {
            return Err(NameError::Taken);
        }
        Ok(name.to_string())
    }

    /// Renames profile `i` if [`Profiles::check_name`] allows `name`; a
    /// refused name leaves it as it was.
    pub fn rename(&mut self, i: usize, name: &str) -> Result<(), NameError> {
        let name = self.check_name(name, Some(i))?;
        if let Some(p) = self.list.get_mut(i) {
            p.name = name;
        }
        Ok(())
    }

    /// Adds a profile ("Player N", the first free N; the default skin; the
    /// next of [`NEW_COLOURS`]) and returns its index. At [`MAX_PROFILES`]
    /// it adds none and returns the last.
    pub fn add(&mut self) -> usize {
        if self.list.len() >= MAX_PROFILES {
            return self.list.len() - 1;
        }
        let n = (self.list.len() + 1..).find(|n| self.check_name(&format!("Player {n}"), None).is_ok());
        let name = format!("Player {}", n.unwrap_or(self.list.len() + 1));
        let colour = NEW_COLOURS[(self.list.len() + NEW_COLOURS.len() - 1) % NEW_COLOURS.len()];
        self.list.push(Profile { name, colour, ..Profile::default() });
        self.list.len() - 1
    }

    /// Removes profile `i`, unless it is the last one; the profile playing
    /// stays selected (the first when it was the one removed).
    pub fn remove(&mut self, i: usize) -> bool {
        if self.list.len() <= 1 || i >= self.list.len() {
            return false;
        }
        self.list.remove(i);
        self.selected = match self.selected {
            s if s == i => 0,
            s if s > i => s - 1,
            s => s,
        };
        true
    }

    /// Makes the list usable: at least one profile and at most
    /// [`MAX_PROFILES`], names without control characters, trimmed, not
    /// empty and at most [`MAX_NAME_CHARS`] long, a skin named, own keys
    /// that reach every action and leave the game's keys alone
    /// ([`crate::settings::sanitize_bindings`]), at most five remembered
    /// weapons of sane names, `selected` in range. One warning per fix.
    pub fn sanitize(&mut self) -> Vec<String> {
        let mut w = Vec::new();
        if self.list.len() > MAX_PROFILES {
            w.push(format!("{} profiles: the first {MAX_PROFILES} are kept", self.list.len()));
            self.list.truncate(MAX_PROFILES);
        }
        if self.list.is_empty() {
            w.push("no profiles: the default one is added".into());
            self.list.push(Profile::default());
        }
        for (i, p) in self.list.iter_mut().enumerate() {
            let trimmed: String = p.name.chars().filter(|c| !c.is_control()).collect();
            let mut name: String = trimmed.trim().chars().take(MAX_NAME_CHARS).collect();
            if name.is_empty() {
                name = format!("Player {}", i + 1);
            }
            if name != p.name {
                w.push(format!("profile {}: name {:?} is now {name:?}", i + 1, p.name));
                p.name = name;
            }
            if p.skin_id.trim().is_empty() {
                w.push(format!("profile {}: no skin: the default", i + 1));
                p.skin_id = Profile::default().skin_id;
            }
            if let Some(b) = &mut p.bindings {
                let fixed = crate::settings::sanitize_bindings(b);
                w.extend(fixed.into_iter().map(|x| format!("profile {}: {x}", i + 1)));
            }
            let sane = |n: &String| n.len() <= MAX_WEAPON_NAME && !n.chars().any(char::is_control);
            if p.weapons.len() > MAX_SLOTS || !p.weapons.iter().all(sane) {
                w.push(format!("profile {}: odd remembered weapons: forgotten", i + 1));
                p.weapons.truncate(MAX_SLOTS);
                p.weapons.retain(sane);
            }
        }
        if self.selected >= self.list.len() {
            w.push(format!("selected profile {} does not exist: the first", self.selected + 1));
            self.selected = 0;
        }
        w
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// OLX's seven bot colours in turn; none close to the human's.
    #[test]
    fn bot_colours_follow_olx_and_skip_the_humans() {
        // The default human green (0, 177, 0): OLX's green bot is skipped.
        let c = bot_colours(Profile::default().colour, 8);
        assert!(!c.contains(&[0, 255, 0]), "{c:?}");
        assert_eq!(
            c[..6],
            [[255, 0, 0], [0, 0, 255], [255, 0, 255], [0, 255, 255], [128, 128, 128], [128, 255, 0]]
        );
        assert_eq!(c[6], [255, 0, 0], "then again from the first");
        // A red human: no red bot; seven bots differ from each other while
        // there are colours enough.
        let c = bot_colours([250, 10, 10], 6);
        assert!(!c.contains(&[255, 0, 0]));
        let mut d = c.clone();
        d.dedup();
        assert_eq!(d.len(), 6);
        assert_eq!(bot_colours([1, 2, 3], 7), CPU_COLOURS);
        assert!(bot_colours([0, 0, 0], 0).is_empty());
    }
}
