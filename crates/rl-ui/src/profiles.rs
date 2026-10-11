//! Player profiles (spec §8.3, §9.4 `profiles.ron`): a name, a skin and a
//! colour, and optionally the player's own key bindings.

use rl_input::Bindings;
use serde::{Deserialize, Serialize};

/// The version `profiles.ron` is written with; any other is refused.
pub const PROFILES_VERSION: u32 = 1;

/// The longest name, in characters.
/// OLX: src/client/DeprecatedGUI/Menu_Player.cpp:130 (`TXM_SETMAX, 20`).
pub const MAX_NAME_CHARS: usize = 20;

/// The most profiles a file may hold (more are dropped with a warning).
pub const MAX_PROFILES: usize = 64;

/// One player.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub name: String,
    /// A skin pack id or OLX skin name (`rl_content::Content::skin_or_default`).
    pub skin_id: String,
    /// The worm's colour (RGB).
    pub colour: [u8; 3],
    /// The player's own keys; `None` uses the options' bindings.
    pub bindings: Option<Bindings>,
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

    /// Makes the list usable: at least one profile and at most
    /// [`MAX_PROFILES`], names trimmed, not empty and at most
    /// [`MAX_NAME_CHARS`] long, a skin named, `selected` in range. One
    /// warning per fix.
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
            let trimmed = p.name.trim();
            let mut name: String = trimmed.chars().take(MAX_NAME_CHARS).collect();
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
        }
        if self.selected >= self.list.len() {
            w.push(format!("selected profile {} does not exist: the first", self.selected + 1));
            self.selected = 0;
        }
        w
    }
}
