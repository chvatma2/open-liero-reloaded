//! A bug report from the main menu (issue #3): what the player wrote, an
//! automatic section (the version, the system, the GPU, the display, the
//! settings in effect), the current and the previous run's logs, and a
//! replay if the player attaches one.
//!
//! Pure: the app gathers the facts ([`SystemFacts`] → [`sections`], the
//! logs → [`LogAttachment`], into a [`ReportContext`]); [`Report::new`]
//! assembles them with the player's text, and the payload
//! ([`Report::json`], the replay's bytes beside it) and the preview
//! ([`Report::preview`] and each log's text) are made from that one value,
//! so the preview shows exactly what is sent. Nothing here sends: the
//! Report screen's Send is `UiAction::SendReport`, which the app carries
//! out off the UI thread.
//!
//! Left out on purpose: key bindings, the online settings, profiles, the
//! identity and any path holding the home directory (the logs show it as
//! `~`; `olr-log`'s redaction, applied again by the app). Player names
//! reach a report only inside an attached replay, and the preview lists
//! them.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use olr_content::Catalog;
use olr_replay::Replay;
use serde::Serialize;

use crate::settings::{FrameRate, Settings, UpscaleFilter};

/// The payload's format (the relay refuses any other).
pub const REPORT_FORMAT: u32 = 1;
/// The most characters of each of the player's texts (the relay's cap).
pub const MAX_TEXT_CHARS: usize = 8_000;
/// The most bytes of a log sent: its end (the relay's cap is a little more).
pub const LOG_TAIL_BYTES: usize = 256 * 1024;
/// The largest replay that can be attached (the whole request stays under
/// the relay's 1 MiB with both logs).
pub const MAX_REPLAY_BYTES: u64 = 384 * 1024;
/// The most replays the picker lists (the newest).
pub const MAX_REPLAY_CHOICES: usize = 50;

/// One line of the automatic section: a name and its value.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Fact {
    pub name: String,
    pub value: String,
}

/// A group of facts under a title ("System", "GPU", …).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Section {
    pub title: String,
    pub facts: Vec<Fact>,
}

impl Section {
    fn new(title: &str, facts: Vec<(&str, String)>) -> Section {
        let facts = facts.into_iter().map(|(n, v)| Fact { name: n.to_string(), value: v }).collect();
        Section { title: title.to_string(), facts }
    }
}

/// How a run's log ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LogEnd {
    /// The run is still going (this one, or another game open).
    Running,
    Clean,
    /// Without its clean last line: a crash or a kill.
    Unclean,
}

impl LogEnd {
    pub fn words(self) -> &'static str {
        match self {
            LogEnd::Running => "still running",
            LogEnd::Clean => "ended cleanly",
            LogEnd::Unclean => "ended unexpectedly (a crash or a kill)",
        }
    }
}

/// A run's log as sent: its last [`LOG_TAIL_BYTES`] at most.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LogAttachment {
    /// "This run" or "The previous run".
    pub name: String,
    /// The log's file name (a time and a process id).
    pub file: String,
    pub ended: LogEnd,
    /// The bytes left out from its start.
    pub cut: u64,
    pub text: String,
}

impl LogAttachment {
    /// The log `file` named `name` from its whole `bytes`: the end that
    /// fits [`LOG_TAIL_BYTES`], from a line's start, `home` shown as `~`.
    pub fn new(name: &str, file: &str, ended: LogEnd, bytes: &[u8], home: Option<&str>) -> LogAttachment {
        let (cut, tail) = tail(bytes, LOG_TAIL_BYTES);
        let text = String::from_utf8_lossy(tail);
        let text = match home {
            Some(h) if h.len() > 1 => text.replace(h, "~"),
            _ => text.into_owned(),
        };
        LogAttachment { name: name.to_string(), file: file.to_string(), ended, cut: cut as u64, text }
    }

    /// One line of the preview: name, file, end and size.
    pub fn summary(&self) -> String {
        let cut = if self.cut > 0 {
            format!(
                ", its last {} KiB (the first {} KiB left out)",
                kib(self.text.len() as u64),
                kib(self.cut)
            )
        } else {
            format!(", {} KiB", kib(self.text.len() as u64))
        };
        format!("{}: {}, {}{cut}", self.name, self.file, self.ended.words())
    }
}

/// The last `max` bytes of `bytes` at most, from a line's start when one
/// begins in them; and how many bytes were left out.
fn tail(bytes: &[u8], max: usize) -> (usize, &[u8]) {
    if bytes.len() <= max {
        return (0, bytes);
    }
    let from = bytes.len() - max;
    let start = bytes[from..].iter().position(|&b| b == b'\n').map_or(from, |i| from + i + 1);
    (start, &bytes[start..])
}

fn kib(bytes: u64) -> u64 {
    bytes.div_ceil(1024)
}

/// An attached replay as the report describes it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ReplayInfo {
    pub file: String,
    /// The file's date, UTC.
    pub date: String,
    #[serde(rename = "mod")]
    pub mod_name: String,
    pub level: String,
    /// The worms' names the replay holds (sent with it, unchanged).
    pub worms: Vec<String>,
    pub bytes: u64,
}

impl ReplayInfo {
    /// The picker's line: date, mod, level and size.
    pub fn line(&self) -> String {
        format!("{}  ·  {}  ·  {}  ·  {} KiB", self.date, self.mod_name, self.level, kib(self.bytes))
    }
}

/// A replay's bytes, shared (its `Debug` shows the size, not the bytes).
#[derive(Clone, PartialEq, Eq)]
pub struct ReplayBytes(pub Arc<Vec<u8>>);

impl std::fmt::Debug for ReplayBytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ReplayBytes({} bytes)", self.0.len())
    }
}

/// A saved replay the player may attach.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayChoice {
    pub path: PathBuf,
    pub info: ReplayInfo,
    /// Its bytes, read when listed (so what is sent is what was shown),
    /// or why it cannot be attached.
    pub bytes: Result<ReplayBytes, String>,
}

/// The replays of `dir` the picker offers, newest first (by the file's
/// date, then name), at most [`MAX_REPLAY_CHOICES`]: each read and
/// decoded for its mod, level and worms (names from `catalog`, else the
/// pack ids); one too large or unreadable is listed with why.
pub fn replay_choices(dir: &Path, catalog: &Catalog) -> Vec<ReplayChoice> {
    let mut files: Vec<(u64, String, PathBuf, u64)> = crate::replays::list(dir)
        .into_iter()
        .filter_map(|f| {
            let meta = std::fs::metadata(&f.path).ok()?;
            let secs = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs());
            Some((secs, f.name, f.path, meta.len()))
        })
        .collect();
    files.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    files.truncate(MAX_REPLAY_CHOICES);
    files.into_iter().map(|(secs, name, path, len)| choice(path, name, secs, len, catalog)).collect()
}

fn choice(path: PathBuf, file: String, secs: u64, len: u64, catalog: &Catalog) -> ReplayChoice {
    let mut info = ReplayInfo {
        file,
        date: utc_minute(secs),
        mod_name: "?".into(),
        level: "?".into(),
        worms: Vec::new(),
        bytes: len,
    };
    if len > MAX_REPLAY_BYTES {
        let why = format!("too large to attach ({} KiB, at most {} KiB)", kib(len), kib(MAX_REPLAY_BYTES));
        return ReplayChoice { path, info, bytes: Err(why) };
    }
    let bytes = match read_at_most(&path, MAX_REPLAY_BYTES) {
        Ok(b) => b,
        Err(e) => return ReplayChoice { path, info, bytes: Err(e) },
    };
    match Replay::decode(&bytes) {
        Ok(r) => {
            let packs = &r.setup.packs;
            info.mod_name =
                catalog.mod_info(&packs.mod_pack.id).map_or(packs.mod_pack.id.clone(), |m| m.name.clone());
            info.level = catalog.level(&packs.level.id).map_or(packs.level.id.clone(), |l| l.name.clone());
            info.worms = r.setup.looks.iter().map(|l| l.name.clone()).collect();
            info.bytes = bytes.len() as u64;
            ReplayChoice { path, info, bytes: Ok(ReplayBytes(Arc::new(bytes))) }
        }
        Err(e) => ReplayChoice { path, info, bytes: Err(format!("not a replay this build reads: {e}")) },
    }
}

/// The file's bytes, refusing more than `max` (it may have grown since it
/// was listed).
fn read_at_most(path: &Path, max: u64) -> Result<Vec<u8>, String> {
    use std::io::Read as _;
    let mut out = Vec::new();
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    file.take(max + 1).read_to_end(&mut out).map_err(|e| e.to_string())?;
    if out.len() as u64 > max {
        return Err(format!("too large to attach (more than {} KiB)", kib(max)));
    }
    Ok(out)
}

/// `secs` since the Unix epoch as `2026-10-21 14:03 UTC`.
pub fn utc_minute(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Howard Hinnant's `civil_from_days`.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02} {:02}:{:02} UTC", rem / 3600, rem % 3600 / 60)
}

/// The operating system as the app reads it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OsFacts {
    /// `std::env::consts::OS` and `ARCH`.
    pub os: String,
    pub arch: String,
    /// os-release's `PRETTY_NAME` (the host's under Flatpak).
    pub distro: Option<String>,
    pub kernel: Option<String>,
    /// The window system the game runs on: Wayland, X11, Windows, …
    pub window_system: Option<String>,
    /// `XDG_SESSION_TYPE`, the desktop's.
    pub session: Option<String>,
    pub wsl: bool,
    pub flatpak: bool,
}

/// The adapter and surface the game draws with (`olr-game`'s `Gpu`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GpuFacts {
    pub name: String,
    pub backend: String,
    pub device_type: String,
    /// wgpu's `driver` and `driver_info`.
    pub driver: String,
    pub driver_info: String,
    pub present_mode: String,
    pub format: String,
}

/// The window and its monitor as winit reports them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DisplayFacts {
    pub scale_factor: f64,
    /// The window's inner size, physical pixels.
    pub window: (u32, u32),
    /// The current monitor's size in physical pixels, if known.
    pub monitor: Option<(u32, u32)>,
    /// Its refresh rate in millihertz, if known.
    pub refresh_mhz: Option<u32>,
}

/// Which video settings the command line set for this run
/// (`olr-game`'s `video::Overrides`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VideoOverrides {
    pub fullscreen: bool,
    pub frame_rate: bool,
    pub filter: bool,
    pub interpolation: bool,
    pub hud_layout: bool,
}

/// Everything the automatic section is made of.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SystemFacts {
    /// The game's version (`olr_game::VERSION`).
    pub version: String,
    pub os: OsFacts,
    /// `None`: no window yet (said as unknown).
    pub gpu: Option<GpuFacts>,
    pub display: Option<DisplayFacts>,
    /// The settings in effect (the command line's flags applied).
    pub settings: Settings,
    pub overrides: VideoOverrides,
}

fn yes_no(b: bool) -> String {
    if b { "yes" } else { "no" }.to_string()
}

fn or_unknown(v: Option<&String>) -> String {
    v.cloned().unwrap_or_else(|| "unknown".to_string())
}

/// `value`, marked when the command line set it.
fn flagged(value: String, flag: bool) -> String {
    if flag { format!("{value} (set on the command line)") } else { value }
}

fn percent(v: f32) -> String {
    format!("{} %", (v * 100.0).round())
}

/// The automatic section, in order: Game, System, GPU, Display, Video,
/// Audio, Gameplay.
pub fn sections(f: &SystemFacts) -> Vec<Section> {
    let os = &f.os;
    let mut out = vec![
        Section::new("Game", vec![("Version", f.version.clone())]),
        Section::new(
            "System",
            vec![
                ("OS", format!("{} {}", os.os, os.arch)),
                ("Distribution", or_unknown(os.distro.as_ref())),
                ("Kernel", or_unknown(os.kernel.as_ref())),
                ("Window system", or_unknown(os.window_system.as_ref())),
                ("Desktop session", or_unknown(os.session.as_ref())),
                ("WSL", yes_no(os.wsl)),
                ("Flatpak", yes_no(os.flatpak)),
            ],
        ),
    ];
    out.push(match &f.gpu {
        Some(g) => Section::new(
            "GPU",
            vec![
                ("Adapter", g.name.clone()),
                ("Backend", g.backend.clone()),
                ("Device type", g.device_type.clone()),
                ("Driver", g.driver.clone()),
                ("Driver info", g.driver_info.clone()),
                ("Present mode", g.present_mode.clone()),
                ("Surface format", g.format.clone()),
            ],
        ),
        None => Section::new("GPU", vec![("Adapter", "unknown (no window)".to_string())]),
    });
    out.push(match &f.display {
        Some(d) => Section::new(
            "Display",
            vec![
                ("Scale factor", format!("{}", d.scale_factor)),
                ("Window", format!("{} × {}", d.window.0, d.window.1)),
                ("Monitor", d.monitor.map_or("unknown".to_string(), |(w, h)| format!("{w} × {h}"))),
                (
                    "Refresh rate",
                    d.refresh_mhz
                        .map_or("unknown".to_string(), |m| format!("{}.{:03} Hz", m / 1000, m % 1000)),
                ),
            ],
        ),
        None => Section::new("Display", vec![("Window", "unknown (no window)".to_string())]),
    });
    let v = &f.settings.video;
    let o = f.overrides;
    out.push(Section::new(
        "Video settings",
        vec![
            ("Window size", v.window.label().to_string()),
            ("Fullscreen", flagged(yes_no(v.fullscreen), o.fullscreen)),
            (
                "Frame rate",
                flagged(
                    match v.frame_rate {
                        FrameRate::VSync => "VSync",
                        FrameRate::Uncapped => "Uncapped",
                    }
                    .to_string(),
                    o.frame_rate,
                ),
            ),
            (
                "Upscale filter",
                flagged(
                    match v.filter {
                        UpscaleFilter::Nearest => "Nearest",
                        UpscaleFilter::Sharp => "Sharp",
                    }
                    .to_string(),
                    o.filter,
                ),
            ),
            (
                "Interpolation",
                flagged(if v.interpolation { "on" } else { "off" }.to_string(), o.interpolation),
            ),
            ("UI scale", format!("{}", v.ui_scale)),
            ("HUD layout", flagged(v.hud_layout.label().to_string(), o.hud_layout)),
        ],
    ));
    let a = &f.settings.audio;
    out.push(Section::new(
        "Audio settings",
        vec![("Master", percent(a.master)), ("Effects", percent(a.sfx)), ("Music", percent(a.music))],
    ));
    let prefs =
        ron::to_string(&f.settings.gameplay.match_prefs).unwrap_or_else(|e| format!("unwritable: {e}"));
    out.push(Section::new("Gameplay settings", vec![("Local play's match", prefs)]));
    out
}

/// What the app gathered for a report: the automatic section and the logs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReportContext {
    pub sections: Vec<Section>,
    pub logs: Vec<LogAttachment>,
    /// What could not be gathered (a log unreadable): shown on the
    /// screen, not sent.
    pub warnings: Vec<String>,
}

/// A report as it is sent: [`Report::json`] and, when a replay is
/// attached, its bytes ([`Report::replay_bytes`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Report {
    pub format: u32,
    /// What happened (required) and what the player expected.
    pub happened: String,
    pub expected: String,
    pub sections: Vec<Section>,
    pub logs: Vec<LogAttachment>,
    pub replay: Option<ReplayInfo>,
    /// The attached replay's file, unchanged.
    #[serde(skip)]
    pub replay_bytes: Option<ReplayBytes>,
}

/// The player's text as sent: line ends unified, trimmed, at most
/// [`MAX_TEXT_CHARS`] characters.
pub fn clean_text(text: &str) -> String {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    text.trim().chars().take(MAX_TEXT_CHARS).collect()
}

impl Report {
    /// The report of the player's texts with `context` and the `replay`
    /// chosen (one that cannot be attached is left out).
    pub fn new(
        happened: &str,
        expected: &str,
        context: &ReportContext,
        replay: Option<&ReplayChoice>,
    ) -> Report {
        let replay = replay.and_then(|c| c.bytes.as_ref().ok().map(|b| (c.info.clone(), b.clone())));
        Report {
            format: REPORT_FORMAT,
            happened: clean_text(happened),
            expected: clean_text(expected),
            sections: context.sections.clone(),
            logs: context.logs.clone(),
            replay: replay.as_ref().map(|(i, _)| i.clone()),
            replay_bytes: replay.map(|(_, b)| b),
        }
    }

    /// Whether it can be sent: "What happened" is not empty.
    pub fn is_ready(&self) -> bool {
        !self.happened.is_empty()
    }

    /// The payload's JSON (the replay's bytes go beside it).
    pub fn json(&self) -> String {
        // Strings, numbers and lists only: serde_json cannot fail on them.
        serde_json::to_string(self).unwrap_or_default()
    }

    /// The preview of everything sent but the logs' text (shown each under
    /// its summary): the player's texts, the automatic section, the
    /// replay with its worms' names, the logs' summaries.
    pub fn preview(&self) -> String {
        let mut s = String::new();
        let block = |s: &mut String, title: &str, text: &str| {
            s.push_str(title);
            s.push('\n');
            if text.is_empty() {
                s.push_str("  (empty)\n");
            }
            for line in text.lines() {
                s.push_str("  ");
                s.push_str(line);
                s.push('\n');
            }
            s.push('\n');
        };
        block(&mut s, "What happened", &self.happened);
        block(&mut s, "What you expected", &self.expected);
        for section in &self.sections {
            s.push_str(&section.title);
            s.push('\n');
            for f in &section.facts {
                s.push_str(&format!("  {}: {}\n", f.name, f.value));
            }
            s.push('\n');
        }
        match &self.replay {
            None => s.push_str("Replay\n  none attached\n\n"),
            Some(r) => {
                s.push_str(&format!("Replay\n  {} ({} KiB), sent unchanged\n", r.file, kib(r.bytes)));
                s.push_str(&format!("  Date: {}\n  Mod: {}\n  Level: {}\n", r.date, r.mod_name, r.level));
                s.push_str(&format!("  Worms' names in it: {}\n\n", r.worms.join(", ")));
            }
        }
        s.push_str("Logs\n");
        if self.logs.is_empty() {
            s.push_str("  none\n");
        }
        for l in &self.logs {
            s.push_str(&format!("  {}\n", l.summary()));
        }
        s
    }
}

/// How the last Send went.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SendStatus {
    #[default]
    Idle,
    Sending,
    /// Filed: the issue's URL.
    Sent(String),
    /// Why not, in words for the player.
    Failed(String),
}

/// The Report screen's data: what the app gathered and what the player
/// typed and chose (kept while the menus run, so a failed send keeps it).
#[derive(Clone, Debug, Default)]
pub struct ReportData {
    /// The build has a relay to send to: Main shows "Report a bug".
    pub available: bool,
    /// The automatic section and the logs (the app's `PrepareReport`).
    pub context: Option<ReportContext>,
    /// The saved replays, newest first (read when the screen opens).
    pub replays: Vec<ReplayChoice>,
    pub happened: String,
    pub expected: String,
    /// "Attach a replay" (off by default).
    pub attach: bool,
    /// The replay picked (an index of `replays`).
    pub selected: Option<usize>,
    pub status: SendStatus,
}

impl ReportData {
    /// The screen opens: the replays listed again from `dir`, the newest
    /// that can be attached picked (the choice kept when it is still listed).
    pub fn enter(&mut self, dir: Option<&Path>, catalog: &Catalog) {
        let kept = self.chosen_path();
        self.replays = dir.map(|d| replay_choices(d, catalog)).unwrap_or_default();
        let newest = self.replays.iter().position(|r| r.bytes.is_ok());
        self.selected = kept.and_then(|p| self.replays.iter().position(|r| r.path == p)).or(newest);
        if matches!(self.status, SendStatus::Sent(_)) {
            self.status = SendStatus::Idle;
        }
    }

    fn chosen_path(&self) -> Option<PathBuf> {
        self.selected.and_then(|i| self.replays.get(i)).map(|r| r.path.clone())
    }

    /// The replay that goes with the report: attached and picked.
    pub fn chosen(&self) -> Option<&ReplayChoice> {
        self.selected.filter(|_| self.attach).and_then(|i| self.replays.get(i))
    }

    /// The report as it stands (`None` until the app gathered the context).
    pub fn report(&self) -> Option<Report> {
        let c = self.context.as_ref()?;
        Some(Report::new(&self.happened, &self.expected, c, self.chosen()))
    }

    /// The app: the send ended with the issue's URL (the texts are
    /// cleared, the replay unattached) or why not (all kept to retry).
    pub fn finished(&mut self, result: Result<String, String>) {
        match result {
            Ok(url) => {
                self.happened.clear();
                self.expected.clear();
                self.attach = false;
                self.status = SendStatus::Sent(url);
            }
            Err(why) => self.status = SendStatus::Failed(why),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_minutes() {
        assert_eq!(utc_minute(0), "1970-01-01 00:00 UTC");
        // 2026-10-21 12:03:07 UTC.
        assert_eq!(utc_minute(1_792_584_187), "2026-10-21 12:03 UTC");
        assert_eq!(utc_minute(951_782_400), "2000-02-29 00:00 UTC");
    }

    #[test]
    fn a_log_keeps_its_end_from_a_line_start() {
        let mut bytes = Vec::new();
        for i in 0..20_000 {
            bytes.extend_from_slice(format!("line {i} in /home/ann/x\n").as_bytes());
        }
        let l = LogAttachment::new("This run", "a.log", LogEnd::Running, &bytes, Some("/home/ann"));
        assert!(l.cut > 0);
        assert!(l.text.starts_with("line "), "{}", &l.text[..20]);
        assert!(l.text.ends_with("line 19999 in ~/x\n"));
        assert!(!l.text.contains("/home/ann"));
        let small = LogAttachment::new("This run", "a.log", LogEnd::Clean, b"one\ntwo\n", None);
        assert_eq!((small.cut, small.text.as_str()), (0, "one\ntwo\n"));
    }

    #[test]
    fn texts_are_trimmed_and_capped() {
        assert_eq!(clean_text("  a\r\nb\r "), "a\nb");
        assert_eq!(clean_text(&"x".repeat(MAX_TEXT_CHARS + 5)).chars().count(), MAX_TEXT_CHARS);
    }
}
