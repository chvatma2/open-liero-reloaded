//! Updating the game (issue #4): which release a build is offered and
//! how a download is trusted and installed, without any networking (the
//! game, `olr-game`, fetches; `cargo xtask publish` signs and publishes).
//!
//! - [`version`]: versions ordered by semver precedence, `dev` builds by
//!   their commit count.
//! - [`release`]: GitHub's release list, the channels, the newest release
//!   newer than the build (never a downgrade).
//! - [`manifest`]: the Windows zip's signed manifest (ed25519 over the
//!   version, the zip's name, size and SHA-256).
//! - [`install`]: the portable folder's swap, safe against interruption.
//! - [`flatpak`]: the Flatpak repository, `/.flatpak-info`, the commands
//!   shown, the portal's progress.
//! - [`notes`]: a release's Markdown notes as plain text.

pub mod flatpak;
pub mod install;
pub mod manifest;
pub mod notes;
pub mod release;
pub mod version;

pub use release::{Channel, Release};
pub use version::Version;

/// The channel a build checks: under Flatpak its branch's (`stable` or
/// `beta`), else the setting "Include pre-releases" (`None`: the build's
/// own).
pub fn channel(
    include_prereleases: Option<bool>,
    build: &Version,
    flatpak: Option<&flatpak::FlatpakInfo>,
) -> Channel {
    flatpak
        .and_then(flatpak::FlatpakInfo::channel)
        .unwrap_or_else(|| Channel::of_setting(include_prereleases, build))
}
