//! The run log's start (issue #2, `olr-log`): its level, its header and
//! its directory, `<data>/logs`.

use std::path::{Path, PathBuf};

use olr_log::LevelFilter;

/// The log's level: `--log`, else `OLR_LOG` (`env`), else `info`; and a
/// warning for a variable that names no level.
pub fn level(flag: Option<LevelFilter>, env: Option<&str>) -> (LevelFilter, Option<String>) {
    if let Some(l) = flag {
        return (l, None);
    }
    match env.filter(|v| !v.trim().is_empty()) {
        None => (LevelFilter::Info, None),
        Some(v) => match olr_log::parse_level(v) {
            Some(l) => (l, None),
            None => (
                LevelFilter::Info,
                Some(format!("{}={v:?} is no level (error, warn, info, debug): info", olr_log::LEVEL_VAR)),
            ),
        },
    }
}

/// What the system line is made of (read by [`System::read`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct System {
    /// `std::env::consts::OS` and `ARCH`.
    pub os: String,
    pub arch: String,
    /// os-release's `PRETTY_NAME` (the host's under Flatpak).
    pub distro: Option<String>,
    /// The kernel's release (Linux).
    pub kernel: Option<String>,
    pub wsl: bool,
    pub flatpak: bool,
    /// `XDG_SESSION_TYPE` (`wayland`, `x11`): the desktop's, not yet the
    /// window system the game takes (logged with the window).
    pub session: Option<String>,
}

impl System {
    /// This machine's.
    pub fn read() -> System {
        let kernel = crate::wsl::osrelease().trim().to_string();
        let flatpak = Path::new("/.flatpak-info").exists();
        let release = ["/run/host/os-release", "/etc/os-release", "/usr/lib/os-release"]
            .iter()
            .filter(|p| flatpak || !p.starts_with("/run/host"))
            .find_map(|p| std::fs::read_to_string(p).ok());
        System {
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            distro: release.as_deref().and_then(pretty_name),
            wsl: crate::wsl::is_wsl(&kernel),
            kernel: (!kernel.is_empty()).then_some(kernel),
            flatpak,
            session: std::env::var("XDG_SESSION_TYPE").ok().filter(|s| !s.is_empty()),
        }
    }

    /// `os: linux x86_64, Ubuntu 24.04 LTS, kernel 6.6.87.2-microsoft-standard-WSL2, WSL, Flatpak, wayland session`.
    pub fn line(&self) -> String {
        let mut parts = vec![format!("{} {}", self.os, self.arch)];
        parts.extend(self.distro.clone());
        parts.extend(self.kernel.as_ref().map(|k| format!("kernel {k}")));
        if self.wsl {
            parts.push("WSL".into());
        }
        if self.flatpak {
            parts.push("Flatpak".into());
        }
        parts.extend(self.session.as_ref().map(|s| format!("{s} session")));
        format!("os: {}", parts.join(", "))
    }
}

/// os-release's `PRETTY_NAME`, unquoted.
fn pretty_name(text: &str) -> Option<String> {
    let value = text.lines().find_map(|l| l.strip_prefix("PRETTY_NAME="))?;
    let value = value.trim().trim_matches(['"', '\'']).trim();
    (!value.is_empty()).then(|| value.to_string())
}

/// What changed between two saves of the settings, a line a part: the
/// video, audio and match values in full; the keys and the online part
/// only named (relay and TURN entries may hold servers' credentials).
pub fn settings_changes(old: &olr_ui::Settings, new: &olr_ui::Settings) -> Vec<String> {
    let mut out = Vec::new();
    if old.video != new.video {
        out.push(format!("video {:?}", new.video));
    }
    if old.audio != new.audio {
        out.push(format!("audio {:?}", new.audio));
    }
    if old.gameplay != new.gameplay {
        out.push(format!("gameplay {:?}", new.gameplay));
    }
    if old.input != new.input {
        out.push("the key bindings changed".into());
    }
    if old.online != new.online {
        out.push("the online settings changed".into());
    }
    out
}

/// Starts the run log in `<data>/logs` (`None`: the terminal alone, said
/// once): the version and the system first. Returns the run's file.
pub fn start(data: Option<&Path>, level: LevelFilter) -> Option<PathBuf> {
    let header = vec![format!("olr {}", crate::VERSION), System::read().line()];
    let options =
        olr_log::Options { dir: data.map(olr_log::logs_dir), level, home: std::env::home_dir(), header };
    let path = olr_log::init(options, Box::new(olr_log::SystemClock::new()));
    if data.is_none() {
        log::warn!("no data directory: the log goes to the terminal alone");
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_level_is_the_flags_then_the_variables() {
        assert_eq!(level(Some(LevelFilter::Debug), Some("error")), (LevelFilter::Debug, None));
        assert_eq!(level(None, Some("DEBUG")), (LevelFilter::Debug, None));
        assert_eq!(level(None, None), (LevelFilter::Info, None));
        assert_eq!(level(None, Some(" ")), (LevelFilter::Info, None));
        let (l, w) = level(None, Some("loud"));
        assert_eq!(l, LevelFilter::Info);
        assert!(w.is_some_and(|w| w.contains("OLR_LOG=\"loud\"")));
    }

    #[test]
    fn settings_changes_are_named() {
        let old = olr_ui::Settings::default();
        assert!(settings_changes(&old, &old).is_empty());
        let mut new = old.clone();
        new.video.fullscreen = !new.video.fullscreen;
        new.online.port = new.online.port.wrapping_add(1);
        let lines = settings_changes(&old, &new);
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(lines[0].starts_with("video VideoSettings {"), "{lines:?}");
        assert_eq!(lines[1], "the online settings changed");
    }

    #[test]
    fn the_system_line_names_what_was_found() {
        let s = System {
            os: "linux".into(),
            arch: "x86_64".into(),
            distro: pretty_name("NAME=Ubuntu\nPRETTY_NAME=\"Ubuntu 24.04.1 LTS\"\n"),
            kernel: Some("6.6.87.2-microsoft-standard-WSL2".into()),
            wsl: true,
            flatpak: true,
            session: Some("wayland".into()),
        };
        assert_eq!(
            s.line(),
            "os: linux x86_64, Ubuntu 24.04.1 LTS, kernel 6.6.87.2-microsoft-standard-WSL2, WSL, Flatpak, wayland session"
        );
        let w = System { os: "windows".into(), arch: "x86_64".into(), ..System::default() };
        assert_eq!(w.line(), "os: windows x86_64");
    }
}
