//! The update notice and screen's data (issue #4), plain views the app
//! fills: whether this build checks for updates at all, the release
//! offered (its version, title, how it installs, and the notes as text of
//! every release it brings, issue #22), the
//! install's progress, and under Flatpak the branch the game runs from.
//! No networking here: the app (`olr-game`) checks GitHub, downloads,
//! verifies and installs.

/// The update screen's and the notice's data.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UpdateData {
    /// This build checks for updates (one `cargo xtask dist` made): else
    /// no update notice and no update option.
    pub enabled: bool,
    /// The installed build is a pre-release: "Include pre-releases"'
    /// default.
    pub build_prerelease: bool,
    /// Under Flatpak on a repository branch: what switching channel takes.
    pub flatpak: Option<FlatpakChannel>,
    /// The newer release in the player's channel.
    pub offer: Option<UpdateOffer>,
    pub status: UpdateStatus,
}

impl UpdateData {
    /// Whether "Include pre-releases" is on, given the saved setting.
    pub fn includes_prereleases(&self, setting: Option<bool>) -> bool {
        match &self.flatpak {
            Some(f) => f.beta,
            None => setting.unwrap_or(self.build_prerelease),
        }
    }

    /// An install runs or is done: Update does nothing more.
    pub fn busy(&self) -> bool {
        matches!(self.status, UpdateStatus::Working { .. } | UpdateStatus::Installed)
    }
}

/// The Flatpak branch the game runs from defines its channel; the game
/// cannot change it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FlatpakChannel {
    /// On `beta` (pre-releases included), else `stable`.
    pub beta: bool,
    /// The command installing the other branch and uninstalling this one.
    pub switch_command: String,
}

/// A newer release.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UpdateOffer {
    /// Its version (`0.2.0`, `0.2.1-dev.12.0abc123`).
    pub version: String,
    /// The release's title.
    pub title: String,
    /// The notes of every release in the player's channel newer than
    /// their build, up to this one, newest first (this one's alone when
    /// it is the next).
    pub changes: Vec<ReleaseNotes>,
    /// The older ones of those not in `changes` (over its caps).
    pub left_out: usize,
    /// Its page on GitHub (https).
    pub page: String,
    pub method: UpdateMethod,
}

/// One release's notes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReleaseNotes {
    /// Its title and version: `0.2.0 development build 318 (v0.2.0-dev.318)`.
    pub heading: String,
    /// Its notes as plain text (empty: none).
    pub notes: String,
}

impl UpdateOffer {
    /// The notes cover more than the offered release.
    pub fn many(&self) -> bool {
        self.changes.len() + self.left_out > 1
    }
}

/// How the offered release installs.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum UpdateMethod {
    /// Windows' portable folder: download, verify, swap in place.
    Portable,
    /// Flatpak on a repository branch: through the Flatpak portal.
    Portal,
    /// Flatpak from an older bundle (no repository): this command
    /// installs from the repository and removes the old build.
    Command(String),
    /// Nothing the game can install: the release page.
    #[default]
    Page,
}

/// How the install goes.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum UpdateStatus {
    #[default]
    Idle,
    /// Under way: the step in words, how far (0 to 1) when known.
    Working { step: String, progress: Option<f32> },
    /// Installed: a restart starts the new build.
    Installed,
    /// Failed: why. Nothing changed; Update tries again.
    Failed(String),
    /// The game folder cannot be written (why): the release page instead.
    NotWritable(String),
    /// Only outside the game: why, and the command to run.
    Outside { why: String, command: String },
}
