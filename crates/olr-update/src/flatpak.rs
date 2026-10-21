//! The Linux release's Flatpak repository (issue #4): its address, its
//! branches, the files that subscribe to it, what the sandbox says of the
//! running build (`/.flatpak-info`), the commands the player is shown,
//! and the Flatpak portal's update progress in words.
//!
//! `stable` holds the final releases, `beta` the final releases and the
//! pre-releases (`cargo xtask publish` commits a final release to both).
//! Bundles from before the repository installed branch `master`.

use crate::release::Channel;

/// The application ID (`tools/flatpak/`, `olr-game`'s `window::APP_ID`).
pub const APP_ID: &str = "io.github.openlieroreloaded.OpenLieroReloaded";
/// The repository on GitHub Pages: every bundle carries it for good.
pub const REPO_URL: &str = "https://openlieroreloaded.github.io/flatpak/";
/// The branch of final releases.
pub const STABLE: &str = "stable";
/// The branch of final releases and pre-releases.
pub const BETA: &str = "beta";
/// The repository's title (its summary, the `.flatpakrepo` and refs).
pub const TITLE: &str = "Open Liero Reloaded";
/// The runtimes come from Flathub.
pub const RUNTIME_REPO: &str = "https://dl.flathub.org/repo/flathub.flatpakrepo";
/// The `.flatpakrepo` file's name in the repository.
pub const REPO_FILE: &str = "open-liero-reloaded.flatpakrepo";

/// The branch a channel's builds are on.
pub fn branch_of(channel: Channel) -> &'static str {
    match channel {
        Channel::Stable => STABLE,
        Channel::PreRelease => BETA,
    }
}

/// The branches a release goes to: a final release to both, a
/// pre-release to `beta`.
pub fn branches_for(prerelease: bool) -> &'static [&'static str] {
    if prerelease { &[BETA] } else { &[STABLE, BETA] }
}

/// The `.flatpakref` file subscribing to `branch` (served as
/// `<REPO_URL><branch>.flatpakref`).
pub fn ref_file_name(branch: &str) -> String {
    format!("{branch}.flatpakref")
}

/// The base64 of an armored OpenPGP key's binary form (what `GPGKey=`
/// holds): its body lines, without the headers and the checksum.
pub fn gpg_key_base64(armored: &str) -> Option<String> {
    let start = armored.find("-----BEGIN PGP PUBLIC KEY BLOCK-----")?;
    let mut body = String::new();
    let mut in_body = false;
    for line in armored[start..].lines().skip(1) {
        let line = line.trim();
        if line.starts_with("-----END") {
            return (!body.is_empty()).then_some(body);
        }
        if !in_body {
            // Armor headers end at the first blank line.
            in_body = line.is_empty();
            continue;
        }
        if line.starts_with('=') {
            continue;
        }
        body.push_str(line);
    }
    None
}

/// The `.flatpakref` installing `branch` from the repository.
pub fn flatpakref(branch: &str, gpg_base64: &str) -> String {
    format!(
        "[Flatpak Ref]\nTitle={TITLE}\nName={APP_ID}\nBranch={branch}\nUrl={REPO_URL}\n\
         SuggestRemoteName=openlieroreloaded\nIsRuntime=false\nRuntimeRepo={RUNTIME_REPO}\nGPGKey={gpg_base64}\n"
    )
}

/// The `.flatpakrepo` adding the repository as a remote.
pub fn flatpakrepo(gpg_base64: &str) -> String {
    format!(
        "[Flatpak Repo]\nTitle={TITLE}\nUrl={REPO_URL}\n\
         Comment=Open Liero Reloaded's builds: branch stable (final releases) and beta (with pre-releases)\n\
         GPGKey={gpg_base64}\n"
    )
}

/// What the sandbox says of the running build (`/.flatpak-info`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlatpakInfo {
    pub app_id: String,
    /// `stable`, `beta`, or an older bundle's `master`.
    pub branch: String,
    /// Installed per user (`Some(true)`), system-wide (`Some(false)`), or
    /// unknown.
    pub user: Option<bool>,
}

/// `/.flatpak-info`'s text read: `None` when it names no application.
pub fn parse_info(text: &str) -> Option<FlatpakInfo> {
    let mut section = "";
    let (mut app_id, mut branch, mut app_path) = (None, None, None);
    for line in text.lines() {
        let line = line.trim();
        if let Some(s) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            section = s;
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        match (section, k.trim()) {
            ("Application", "name") => app_id = Some(v.trim().to_string()),
            ("Instance", "branch") => branch = Some(v.trim().to_string()),
            ("Instance", "app-path") => app_path = Some(v.trim().to_string()),
            _ => {}
        }
    }
    let user = app_path.map(|p| {
        if p.starts_with("/var/lib/flatpak/") {
            Some(false)
        } else if p.contains("/.local/share/flatpak/") {
            Some(true)
        } else {
            None
        }
    });
    Some(FlatpakInfo {
        app_id: app_id.filter(|a| !a.is_empty())?,
        branch: branch.unwrap_or_else(|| "master".to_string()),
        user: user.flatten(),
    })
}

impl FlatpakInfo {
    /// The channel its branch defines (`None`: an older bundle's `master`,
    /// on no repository).
    pub fn channel(&self) -> Option<Channel> {
        match self.branch.as_str() {
            STABLE => Some(Channel::Stable),
            BETA => Some(Channel::PreRelease),
            _ => None,
        }
    }

    fn scope(&self) -> &'static str {
        match self.user {
            Some(true) => " --user",
            Some(false) => " --system",
            None => "",
        }
    }

    /// The command that updates it outside the game.
    pub fn update_command(&self) -> String {
        format!("flatpak update{} {}//{}", self.scope(), self.app_id, self.branch)
    }

    /// The command that installs `to`'s branch from the repository and
    /// uninstalls this one (two installed branches would both stay and
    /// keep updating).
    pub fn switch_command(&self, to: Channel) -> String {
        let scope = self.scope();
        format!(
            "flatpak install{scope} -y --from {REPO_URL}{} && flatpak uninstall{scope} -y {}//{}",
            ref_file_name(branch_of(to)),
            self.app_id,
            self.branch
        )
    }
}

/// An `UpdateMonitor.Progress` signal (the portal's `status`: 0 running,
/// 1 nothing to update, 2 done, 3 failed).
#[derive(Clone, Debug, PartialEq)]
pub enum PortalProgress {
    /// `op` of `n_ops` operations, `progress` percent into it: the whole's
    /// fraction.
    Running(f32),
    UpToDate,
    Done,
    Failed {
        error: String,
        message: String,
    },
}

impl PortalProgress {
    pub fn of(status: u32, n_ops: u32, op: u32, progress: u32, error: &str, message: &str) -> PortalProgress {
        match status {
            0 => {
                let n = n_ops.max(1) as f32;
                let within = progress.min(100) as f32 / 100.0;
                PortalProgress::Running(((op.min(n_ops) as f32 + within) / n).clamp(0.0, 1.0))
            }
            1 => PortalProgress::UpToDate,
            2 => PortalProgress::Done,
            _ => PortalProgress::Failed { error: error.to_string(), message: message.to_string() },
        }
    }

    /// The portal refused because the update asks for more permissions
    /// than the installed build has: only the software centre or
    /// `flatpak update` can install it.
    pub fn needs_permissions(&self) -> bool {
        match self {
            PortalProgress::Failed { error, message } => {
                let m = message.to_lowercase();
                error.ends_with("PermissionDenied")
                    || error.ends_with("AccessDenied")
                    || error.ends_with("NotAllowed")
                    || m.contains("permission")
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = include_str!("../../../tools/flatpak/olr-flatpak.gpg");

    #[test]
    fn the_key_is_its_armor_body() {
        let b64 = gpg_key_base64(KEY).unwrap();
        assert!(b64.starts_with("mQINBGrYaisBEADZEHnSUGVd0HKoRDQ5lvHE0cBFtt"));
        assert!(b64.ends_with("vgM/GtnsqlPD"));
        assert!(!b64.contains('=') || !b64.contains("h1t6"));
        assert_eq!(gpg_key_base64("nothing"), None);
        let r = flatpakref(BETA, &b64);
        assert!(r.contains("Branch=beta\n") && r.contains(&format!("Url={REPO_URL}\n")));
        assert!(r.contains(&format!("Name={APP_ID}\n")) && r.contains(&format!("GPGKey={b64}\n")));
        assert!(flatpakrepo(&b64).starts_with("[Flatpak Repo]\n"));
    }

    #[test]
    fn the_info_file_names_the_branch_and_the_installation() {
        let user = "[Application]\nname=io.github.openlieroreloaded.OpenLieroReloaded\nruntime=runtime/x\n\n\
                    [Instance]\nbranch=beta\napp-path=/home/p/.local/share/flatpak/app/x/x86_64/beta/abc/files\n";
        let info = parse_info(user).unwrap();
        assert_eq!(info.branch, "beta");
        assert_eq!(info.user, Some(true));
        assert_eq!(info.channel(), Some(Channel::PreRelease));
        assert_eq!(
            info.switch_command(Channel::Stable),
            "flatpak install --user -y --from https://openlieroreloaded.github.io/flatpak/stable.flatpakref \
             && flatpak uninstall --user -y io.github.openlieroreloaded.OpenLieroReloaded//beta"
        );
        assert_eq!(
            info.update_command(),
            "flatpak update --user io.github.openlieroreloaded.OpenLieroReloaded//beta"
        );
        let system =
            "[Application]\nname=a.b.C\n[Instance]\nbranch=master\napp-path=/var/lib/flatpak/app/a\n";
        let info = parse_info(system).unwrap();
        assert_eq!((info.user, info.channel()), (Some(false), None));
        assert!(info.switch_command(Channel::Stable).contains("install --system -y"));
        assert_eq!(parse_info("[Instance]\nbranch=beta\n"), None);
        assert_eq!(parse_info("").map(|i| i.branch), None);
    }

    #[test]
    fn portal_progress_in_words() {
        assert_eq!(PortalProgress::of(0, 2, 1, 50, "", ""), PortalProgress::Running(0.75));
        assert_eq!(PortalProgress::of(0, 0, 0, 0, "", ""), PortalProgress::Running(0.0));
        assert_eq!(PortalProgress::of(1, 0, 0, 0, "", ""), PortalProgress::UpToDate);
        assert_eq!(PortalProgress::of(2, 1, 1, 100, "", ""), PortalProgress::Done);
        let refused = PortalProgress::of(3, 0, 0, 0, "org.freedesktop.DBus.Error.AccessDenied", "x");
        assert!(refused.needs_permissions());
        let other = PortalProgress::of(3, 0, 0, 0, "org.freedesktop.Flatpak.Error.Aborted", "network down");
        assert!(!other.needs_permissions());
    }

    #[test]
    fn a_final_release_goes_to_both_branches() {
        assert_eq!(branches_for(false), [STABLE, BETA]);
        assert_eq!(branches_for(true), [BETA]);
        assert_eq!(branch_of(Channel::Stable), STABLE);
    }
}
