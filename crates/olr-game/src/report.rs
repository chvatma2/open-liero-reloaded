//! Bug reports from the main menu (issue #3): the relay's address, what a
//! report gathers (the system, the GPU and display the app passes in, the
//! settings in effect, the current and previous run's logs) and its
//! sending, on a thread of its own, as one multipart POST: the report's
//! JSON (`olr_ui::report::Report::json`, part `report`) and, when one is
//! attached, the replay file unchanged (part `replay`). The relay
//! (`tools/bug-relay/`) answers `{"ok":true,"url":…}` or
//! `{"ok":false,"error":…}`, which [`outcome`] puts in the player's words.
//! Nothing is sent but on the Report screen's Send.

pub mod http;

use std::path::Path;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

use olr_ui::report::{
    DisplayFacts, GpuFacts, LogAttachment, LogEnd, OsFacts, Report, ReportContext, SystemFacts,
    VideoOverrides,
};

/// The relay's address a build carries (`cargo xtask dist` passes it on
/// from its own environment), and the variable that overrides it at run
/// time (a local mock in tests). Without either, Report a bug is hidden.
pub const URL_VAR: &str = "OLR_REPORT_URL";

/// The build's relay, if it was given one.
pub const BUILD_URL: Option<&str> = option_env!("OLR_REPORT_URL");

/// How long a send may take: each connect, then the exchange.
pub const TIMEOUTS: http::Timeouts =
    http::Timeouts { connect: Duration::from_secs(10), exchange: Duration::from_secs(45) };

/// What the player is told when the relay counts too many reports.
pub const RATE_LIMITED: &str = "Too many reports right now, please try again later.";

/// The relay's address: `OLR_REPORT_URL` at run time, else the build's;
/// `None` (Report a bug hidden) without either.
pub fn endpoint(runtime: Option<String>) -> Option<String> {
    let given = |u: &str| (!u.trim().is_empty()).then(|| u.trim().to_string());
    runtime.as_deref().and_then(given).or_else(|| BUILD_URL.and_then(given))
}

/// What only the window knows: the GPU, the display, the window system.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Environment {
    pub gpu: Option<GpuFacts>,
    pub display: Option<DisplayFacts>,
    /// "Wayland", "X11", "Windows", …
    pub window_system: Option<String>,
}

/// The command line's video flags, as the report marks them.
pub fn overridden(o: &crate::video::Overrides) -> VideoOverrides {
    VideoOverrides {
        fullscreen: o.fullscreen.is_some(),
        frame_rate: o.frame_rate.is_some(),
        filter: o.filter.is_some(),
        interpolation: o.interpolation.is_some(),
        hud_layout: o.hud_layout.is_some(),
    }
}

/// The automatic section of this machine and run.
pub fn facts(env: Environment, settings: &olr_ui::Settings, overrides: VideoOverrides) -> SystemFacts {
    let s = crate::logging::System::read();
    SystemFacts {
        version: crate::VERSION.to_string(),
        os: OsFacts {
            os: s.os,
            arch: s.arch,
            distro: s.distro,
            kernel: s.kernel,
            window_system: env.window_system,
            session: s.session,
            wsl: s.wsl,
            flatpak: s.flatpak || std::env::var_os("FLATPAK_ID").is_some(),
        },
        gpu: env.gpu,
        display: env.display,
        settings: settings.clone(),
        overrides,
    }
}

/// The logs a report carries: this run's (`ours`, flushed first) and the
/// one before it in the same directory, each's end; what could not be
/// read is a warning, not sent.
pub fn logs(ours: Option<&Path>, home: Option<&Path>) -> (Vec<LogAttachment>, Vec<String>) {
    let home = olr_log::redact::home_text(home);
    let mut out = Vec::new();
    let mut warnings = Vec::new();
    let Some(ours) = ours else {
        warnings.push("This run writes no log (no data directory): none is attached.".to_string());
        return (out, warnings);
    };
    olr_log::flush();
    let mut attach = |name: &str, path: &Path, ended: LogEnd| match std::fs::read(path) {
        Ok(bytes) => {
            let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            out.push(LogAttachment::new(name, &file, ended, &bytes, home.as_deref()));
        }
        Err(e) => warnings.push(format!("{name}'s log could not be read ({e}): it is left out.")),
    };
    attach("This run", ours, LogEnd::Running);
    if let Some(dir) = ours.parent()
        && let Some(previous) = olr_log::run::previous(dir, ours)
    {
        let ended = match olr_log::run::ended(&previous) {
            Some(olr_log::RunEnd::Clean) => LogEnd::Clean,
            Some(olr_log::RunEnd::Running) => LogEnd::Running,
            Some(olr_log::RunEnd::Unclean) | None => LogEnd::Unclean,
        };
        attach("The previous run", &previous, ended);
    }
    (out, warnings)
}

/// The report's context: [`facts`]' sections and [`logs`].
pub fn context(facts: &SystemFacts, ours: Option<&Path>, home: Option<&Path>) -> ReportContext {
    let (logs, warnings) = logs(ours, home);
    ReportContext { sections: olr_ui::report::sections(facts), logs, warnings }
}

/// The request's body, `multipart/form-data`: the report's JSON, then the
/// replay's bytes when one is attached. Gives the content type and body.
pub fn body(report: &Report) -> (String, Vec<u8>) {
    let json = report.json();
    let replay = report.replay_bytes.as_ref().map(|b| b.0.as_slice());
    let mut seed = std::collections::hash_map::RandomState::new();
    let boundary = loop {
        use std::hash::{BuildHasher, Hasher};
        let mut h = seed.build_hasher();
        h.write(json.as_bytes());
        let b = format!("olr-report-{:016x}{:016x}", h.finish(), seed.hash_one(json.len()));
        let inside = |bytes: &[u8]| bytes.windows(b.len()).any(|w| w == b.as_bytes());
        if !inside(json.as_bytes()) && !replay.is_some_and(inside) {
            break b;
        }
        seed = std::collections::hash_map::RandomState::new();
    };
    let mut out = Vec::with_capacity(json.len() + replay.map_or(0, <[u8]>::len) + 512);
    out.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"report\"\r\n\
             Content-Type: application/json; charset=utf-8\r\n\r\n"
        )
        .as_bytes(),
    );
    out.extend_from_slice(json.as_bytes());
    if let Some(bytes) = replay {
        out.extend_from_slice(
            format!(
                "\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"replay\"; filename=\"replay.olrp\"\r\n\
                 Content-Type: application/octet-stream\r\n\r\n"
            )
            .as_bytes(),
        );
        out.extend_from_slice(bytes);
    }
    out.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), out)
}

/// The relay's answer in the player's words: the issue's address, or why
/// not (each failure a message the screen shows in red).
pub fn outcome(answer: std::io::Result<http::Response>) -> Result<String, String> {
    let r = answer.map_err(|e| {
        format!("Could not reach the report server ({e}). Check your connection and try again.")
    })?;
    let json: Option<serde_json::Value> = serde_json::from_slice(&r.body).ok();
    let field = |k: &str| json.as_ref().and_then(|j| j.get(k)).and_then(|v| v.as_str()).map(str::to_string);
    if (200..300).contains(&r.status)
        && json.as_ref().and_then(|j| j.get("ok")).and_then(|v| v.as_bool()) == Some(true)
        && let Some(url) = field("url").filter(|u| u.starts_with("https://") || u.starts_with("http://"))
    {
        return Ok(url);
    }
    let error = field("error").unwrap_or_default();
    Err(match (error.as_str(), r.status) {
        ("rate_limited", _) | (_, 429) => RATE_LIMITED.to_string(),
        ("too_large", _) | (_, 413) => {
            "The report is too large to send. Leave the replay out and try again.".to_string()
        }
        ("invalid", _) | (_, 400) => {
            let why = field("detail").map(|d| format!(": {d}")).unwrap_or_default();
            format!("The report server refused the report as invalid{why}.")
        }
        ("upstream", _) | (_, 502..=504) => {
            "GitHub did not take the report just now. Please try again later.".to_string()
        }
        (_, s) => {
            format!("The report server answered {s} instead of filing the report. Please try again later.")
        }
    })
}

/// Sends `report` to `url` now (blocking): the issue's address or why not.
pub fn send_now(url: &str, report: &Report, timeouts: http::Timeouts) -> Result<String, String> {
    let url = http::parse_url(url).map_err(|e| format!("The report server's address is wrong: {e}."))?;
    let (content_type, body) = body(report);
    let agent = format!("olr/{}", crate::VERSION);
    outcome(http::post(&url, &content_type, &body, &agent, timeouts))
}

/// A send under way on its thread.
pub struct Sending {
    rx: Receiver<Result<String, String>>,
}

impl Sending {
    /// Sends `report` to `url` on the thread `olr-game report`: the menus
    /// never wait for it.
    pub fn start(url: String, report: Report, timeouts: http::Timeouts) -> Sending {
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new().name("olr-game report".into()).spawn(move || {
            let _ = tx.send(send_now(&url, &report, timeouts));
        });
        if let Err(e) = spawned {
            let (tx, failed) = mpsc::channel();
            let _ = tx.send(Err(format!("The report could not be sent ({e}). Please try again.")));
            return Sending { rx: failed };
        }
        Sending { rx }
    }

    /// How it went, once it ended.
    pub fn poll(&self) -> Option<Result<String, String>> {
        match self.rx.try_recv() {
            Ok(r) => Some(r),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                Some(Err("The report's sending stopped unexpectedly. Please try again.".to_string()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_endpoint_is_the_variables_then_the_builds() {
        assert_eq!(endpoint(Some(" http://127.0.0.1:9/r ".into())).as_deref(), Some("http://127.0.0.1:9/r"));
        assert_eq!(endpoint(Some("  ".into())), BUILD_URL.map(str::to_string));
        assert_eq!(endpoint(None), BUILD_URL.map(str::to_string));
    }

    #[test]
    fn answers_in_the_players_words() {
        let r = |status: u16, body: &str| Ok(http::Response { status, body: body.as_bytes().to_vec() });
        let url = "https://github.com/o/r/issues/7";
        assert_eq!(outcome(r(201, &format!(r#"{{"ok":true,"url":"{url}"}}"#))), Ok(url.to_string()));
        assert_eq!(outcome(r(429, r#"{"ok":false,"error":"rate_limited"}"#)), Err(RATE_LIMITED.to_string()));
        assert_eq!(outcome(r(429, "")), Err(RATE_LIMITED.to_string()));
        assert!(outcome(r(413, r#"{"ok":false,"error":"too_large"}"#)).unwrap_err().contains("too large"));
        let invalid = outcome(r(400, r#"{"ok":false,"error":"invalid","detail":"the replay is no replay"}"#));
        assert_eq!(
            invalid.unwrap_err(),
            "The report server refused the report as invalid: the replay is no replay."
        );
        assert!(outcome(r(502, r#"{"ok":false,"error":"upstream"}"#)).unwrap_err().contains("GitHub"));
        assert!(outcome(r(500, "<html>")).unwrap_err().contains("answered 500"));
        // A 200 without an address is no success.
        assert!(outcome(r(200, r#"{"ok":true}"#)).is_err());
        let refused = std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "connection refused");
        assert!(outcome(Err(refused)).unwrap_err().starts_with("Could not reach the report server"));
    }
}
