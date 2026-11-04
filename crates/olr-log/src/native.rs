//! How Windows saw a run end that left no clean last line: a crash in
//! native code (a driver's, or a stack overflow) or a kill never reaches
//! the panic hook, but Windows' error reporting records the crash
//! (Application Error 1000: the module, the exception), a window that
//! stopped responding (Application Hang 1002) and a display driver reset
//! (Display 4101) in its event log. The next run reads them
//! (`wevtutil`, no unsafe code) for the run it reports, matched by the
//! log's pid and start, and logs what it finds ([`report_previous`]).
//! The XML is read here, on any platform, so it is tested.

use std::path::Path;

/// What is said when Windows recorded nothing of the run.
pub const NOTHING_RECORDED: &str = "Windows recorded no crash, hang or display driver reset of the \
                                    previous run (a kill, a power loss, or error reporting off)";

/// A finished run, from its log's name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    pub pid: u32,
    /// Its start in UTC, `YYYY-MM-DDTHH:MM:SS.mmm` (as the event log's
    /// times begin).
    pub start: String,
}

/// The run of a log named `YYYY-MM-DD_HH-MM-SS.mmmZ_<pid>[-n].log`.
pub fn run_of(log: &Path) -> Option<Run> {
    let stem = log.file_stem()?.to_str()?;
    let (stamp, rest) = stem.split_once("Z_")?;
    let pid = rest.split('-').next()?.parse().ok()?;
    let (day, time) = stamp.split_once('_')?;
    let time = time.replacen('-', ":", 2);
    if day.len() != 10 || time.len() != 12 {
        return None;
    }
    Some(Run { pid, start: format!("{day}T{time}") })
}

/// One event of the log's XML.
struct Event {
    provider: String,
    id: u32,
    /// `TimeCreated`'s `SystemTime`, UTC.
    time: String,
    /// Its data: named (Windows 11) or by position (Windows 10, name "").
    data: Vec<(String, String)>,
}

impl Event {
    /// The data `name`, else the one at `index` when the data has no names.
    fn get(&self, name: &str, index: usize) -> &str {
        if let Some((_, v)) = self.data.iter().find(|(n, _)| n == name) {
            return v;
        }
        match self.data.get(index) {
            Some((n, v)) if n.is_empty() => v,
            _ => "",
        }
    }

    /// Whether it happened at or after `start` (both UTC, ISO 8601: the
    /// strings compare as the times do, however many fractional digits).
    fn after(&self, start: &str) -> bool {
        !self.time.is_empty() && self.time.trim_end_matches('Z') >= start
    }
}

/// An attribute's value in a tag's text (either quote).
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let at = tag.find(&format!("{name}="))? + name.len() + 1;
    let quote = tag[at..].chars().next().filter(|c| *c == '\'' || *c == '"')?;
    let value = &tag[at + 1..];
    Some(&value[..value.find(quote)?])
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// The first `<tag…>`'s head and the text up to its `</tag>` (none for
/// `<tag…/>`), and where in `xml` it ends.
fn element<'a>(xml: &'a str, tag: &str) -> Option<(&'a str, &'a str, usize)> {
    let open = xml.find(&format!("<{tag}"))?;
    let head_end = open + xml[open..].find('>')?;
    let head = &xml[open..head_end];
    if head.ends_with('/') {
        return Some((head, "", head_end + 1));
    }
    let close = format!("</{tag}>");
    let body_len = xml[head_end + 1..].find(&close)?;
    Some((head, &xml[head_end + 1..head_end + 1 + body_len], head_end + 1 + body_len + close.len()))
}

/// The events of `wevtutil qe … /f:xml`'s output, in its order; damaged
/// ones are left out.
fn events(xml: &str) -> Vec<Event> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(at) = rest.find("<Event") {
        rest = &rest[at + "<Event".len()..];
        // `<Event …>` itself, not `<EventID>` or `<EventData>`.
        if !rest.starts_with([' ', '>']) {
            continue;
        }
        let Some(end) = rest.find("</Event>") else { break };
        let chunk = &rest[..end];
        rest = &rest[end..];
        let provider = element(chunk, "Provider").and_then(|(h, _, _)| attr(h, "Name")).unwrap_or("");
        let Some(id) = element(chunk, "EventID").and_then(|(_, b, _)| b.trim().parse().ok()) else {
            continue;
        };
        let time = element(chunk, "TimeCreated").and_then(|(h, _, _)| attr(h, "SystemTime")).unwrap_or("");
        let mut data = Vec::new();
        if let Some((_, mut rest, _)) = element(chunk, "EventData") {
            while let Some((head, body, end)) = element(rest, "Data") {
                data.push((attr(head, "Name").unwrap_or("").to_string(), unescape(body)));
                rest = &rest[end..];
            }
        }
        out.push(Event { provider: provider.to_string(), id, time: time.to_string(), data });
    }
    out
}

/// A hexadecimal process id, with or without `0x`.
fn hex_pid(s: &str) -> Option<u32> {
    let s = s.trim();
    u32::from_str_radix(s.strip_prefix("0x").unwrap_or(s), 16).ok()
}

/// What an exception code means, for the common ones.
fn exception_name(code: &str) -> Option<&'static str> {
    Some(match code {
        "c0000005" => "an access violation",
        "c00000fd" => "a stack overflow",
        "c0000409" => "a fast fail: an abort or a stack buffer overrun",
        "c0000374" => "heap corruption",
        "c000001d" => "an illegal instruction",
        "c0000094" => "an integer division by zero",
        "80000003" => "a breakpoint",
        "e06d7363" => "a C++ exception",
        _ => return None,
    })
}

fn crash_line(e: &Event) -> String {
    let code = e.get("ExceptionCode", 6).trim().trim_start_matches("0x").to_ascii_lowercase();
    let mut line = format!("Windows recorded the previous run's crash: exception 0x{code}");
    if let Some(name) = exception_name(&code) {
        line.push_str(&format!(" ({name})"));
    }
    match e.get("ModuleName", 3).trim() {
        "" | "unknown" => line.push_str(" in an unknown module"),
        module => {
            line.push_str(&format!(" in {module}"));
            let version = e.get("ModuleVersion", 4).trim();
            if !version.is_empty() {
                line.push_str(&format!(" {version}"));
            }
        }
    }
    let offset = e.get("FaultingOffset", 7).trim().trim_start_matches("0x");
    if !offset.is_empty() {
        line.push_str(&format!(" at offset 0x{offset}"));
    }
    match e.get("ModulePath", 11).trim() {
        "" | "unknown" => {}
        path => line.push_str(&format!(" ({path})")),
    }
    line
}

/// What Windows recorded of `run` (whose program is `exe`, any case):
/// `application` is the Application log's events, `system` the System
/// log's, as `wevtutil qe … /f:xml` writes them. Each crash, hang and
/// display driver reset since the run's start is a line; none,
/// [`NOTHING_RECORDED`].
pub fn findings(application: &str, system: &str, exe: &str, run: &Run) -> Vec<String> {
    let mut out = Vec::new();
    for e in events(application) {
        if !e.after(&run.start) || !e.get("AppName", 0).trim().eq_ignore_ascii_case(exe) {
            continue;
        }
        let pid = if e.id == 1002 { e.get("ProcessId", 2) } else { e.get("ProcessId", 8) };
        if hex_pid(pid) != Some(run.pid) {
            continue;
        }
        match (e.provider.as_str(), e.id) {
            ("Application Error", 1000) => out.push(crash_line(&e)),
            ("Application Hang", 1002) => out.push(
                "Windows recorded the previous run as not responding: it hung and was closed".to_string(),
            ),
            _ => {}
        }
    }
    for e in events(system) {
        if e.provider == "Display" && e.id == 4101 && e.after(&run.start) {
            let driver = e.get("", 0).trim();
            let driver = if driver.is_empty() { String::new() } else { format!(" ({driver})") };
            let at = e.time.get(..19).unwrap_or(&e.time).replace('T', " ");
            out.push(format!(
                "Windows reset the display driver{driver} during the previous run, at {at} UTC: it stopped responding"
            ));
        }
    }
    if out.is_empty() {
        out.push(NOTHING_RECORDED.to_string());
    }
    out
}

/// Looks up how Windows saw the run of the previous log `log` end, on a
/// thread of its own (the game's start never waits for it), and warns
/// of each finding. Elsewhere it does nothing.
pub fn report_previous(log: &Path) {
    #[cfg(windows)]
    if let Some(run) = run_of(log) {
        let spawned = std::thread::Builder::new().name("olr-log windows".into()).spawn(move || {
            for line in windows::findings(&run) {
                log::warn!(target: "olr", "{line}");
            }
        });
        if let Err(e) = spawned {
            log::debug!(target: "olr", "cannot ask Windows how the previous run ended: {e}");
        }
    }
    #[cfg(not(windows))]
    let _ = log;
}

#[cfg(windows)]
mod windows {
    use std::os::windows::process::CommandExt as _;
    use std::process::{Command, Stdio};

    use super::Run;

    /// `CREATE_NO_WINDOW`: no console flashes up from the game.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const APPLICATION: &str = "*[System[Provider[@Name='Application Error' or @Name='Application Hang']]]";
    const SYSTEM: &str = "*[System[Provider[@Name='Display'] and (EventID=4101)]]";

    /// The newest `count` events of `channel` matching `query`, as XML.
    fn query(channel: &str, query: &str, count: u32) -> Result<String, String> {
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        let exe = std::path::Path::new(&root).join("System32").join("wevtutil.exe");
        let out = Command::new(&exe)
            .args(["qe", channel, &format!("/q:{query}"), &format!("/c:{count}"), "/rd:true", "/f:xml"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("{}: {e}", exe.display()))?;
        if !out.status.success() {
            return Err(format!("wevtutil {channel}: {}", out.status));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    pub(super) fn findings(run: &Run) -> Vec<String> {
        let exe = std::env::current_exe()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "olr.exe".into());
        let application = query("Application", APPLICATION, 50);
        let system = query("System", SYSTEM, 10);
        match (application, system) {
            (Ok(a), Ok(s)) => super::findings(&a, &s, &exe, run),
            (Err(e), _) | (_, Err(e)) => {
                vec![format!("cannot read how Windows saw the previous run end: {e}")]
            }
        }
    }
}
