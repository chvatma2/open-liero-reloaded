//! A headless Claude Code session with a watchdog: no stream event for
//! `idle`, or more than `budget` of running time, interrupts it.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use crate::limit::{self, Reset};
use crate::stream::{self, Line};

/// How long an interrupted session may take to stop before it is killed.
const GRACE: Duration = Duration::from_secs(60);
/// Lines of stderr kept for a crash report.
const STDERR_TAIL: usize = 40;

#[derive(Clone, Debug)]
pub struct SessionSpec {
    pub claude_bin: String,
    pub config_dir: PathBuf,
    pub workdir: PathBuf,
    /// Every stream line is appended here.
    pub log: PathBuf,
    pub session_id: String,
    /// `None` resumes the session.
    pub prompt: Option<String>,
    pub idle: Duration,
    /// Running time left before the ceiling.
    pub budget: Duration,
    /// Where the session's process id (its process group's) is written
    /// while it runs.
    pub pid_file: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Finished { is_error: bool, text: String },
    Limit(Reset),
    Idle,
    Ceiling,
    Crashed(String),
}

const FLAGS: [&str; 9] = [
    "--model",
    "opus",
    "--effort",
    "high",
    "--permission-mode",
    "bypassPermissions",
    "--output-format",
    "stream-json",
    "--verbose",
];

pub fn args(spec: &SessionSpec) -> Vec<String> {
    let mut out: Vec<String> = match &spec.prompt {
        Some(prompt) => vec!["-p".into(), prompt.clone(), "--session-id".into(), spec.session_id.clone()],
        None => vec![
            "-p".into(),
            "Continue the task where you left off.".into(),
            "--resume".into(),
            spec.session_id.clone(),
        ],
    };
    out.extend(FLAGS.iter().map(|f| f.to_string()));
    out
}

pub fn envs(spec: &SessionSpec) -> Vec<(String, String)> {
    [
        ("CLAUDE_CONFIG_DIR", spec.config_dir.display().to_string()),
        ("CLAUDE_CODE_PROMPT_CACHE_TTL", "1h".into()),
        ("CLAUDE_CODE_AUTO_COMPACT_WINDOW", "400k".into()),
        ("CLAUDE_CODE_DISABLE_AUTO_MEMORY", "1".into()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

/// A fresh session id.
pub fn new_session_id() -> Result<String> {
    Ok(std::fs::read_to_string("/proc/sys/kernel/random/uuid")?.trim().to_string())
}

fn command(spec: &SessionSpec) -> Command {
    // The test stand-in is a bash script; the real session runs at the
    // lowest CPU and I/O priority so the PC stays responsive.
    let mut cmd = if spec.claude_bin.ends_with(".sh") {
        let mut c = Command::new("bash");
        c.arg(&spec.claude_bin);
        c
    } else {
        let mut c = Command::new("nice");
        c.args(["-n", "19", "ionice", "-c", "3", &spec.claude_bin]);
        c
    };
    cmd.args(args(spec))
        .envs(envs(spec))
        .current_dir(&spec.workdir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Its own process group, so an interrupt reaches cargo and the tests too.
        .process_group(0);
    cmd
}

/// Kills the process group a session left in `pid_file` (an interrupted
/// runner's), if any, so a resume never runs beside it.
pub fn stop_left_over(pid_file: &std::path::Path) {
    if let Some(pid) = std::fs::read_to_string(pid_file).ok().and_then(|p| p.trim().parse::<u32>().ok()) {
        let _ = Command::new("kill").args(["-KILL", "--", &format!("-{pid}")]).status();
    }
    let _ = std::fs::remove_file(pid_file);
}

/// Interrupts the session's process group, then kills it after [`GRACE`].
fn interrupt(child: &mut Child) {
    let group = format!("-{}", child.id());
    let _ = Command::new("kill").args(["-INT", "--", &group]).status();
    let t = Instant::now();
    while t.elapsed() < GRACE {
        if let Ok(Some(_)) = child.try_wait() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = Command::new("kill").args(["-KILL", "--", &group]).status();
    let _ = child.wait();
}

/// Runs the session to its end or the watchdog's; returns its outcome and
/// how long it ran.
pub fn run(spec: &SessionSpec) -> Result<(Outcome, Duration)> {
    let mut log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&spec.log)
        .with_context(|| format!("cannot open {}", spec.log.display()))?;
    let mut child = command(spec).spawn().with_context(|| format!("cannot start {}", spec.claude_bin))?;
    let started = Instant::now();
    if let Some(pid_file) = &spec.pid_file {
        std::fs::write(pid_file, child.id().to_string())?;
    }

    let (tx, rx) = mpsc::channel();
    let stdout = child.stdout.take().expect("piped");
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let mut stderr = child.stderr.take().expect("piped");
    let stderr = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        text
    });

    let mut last_result = None;
    let stopped = loop {
        let left = spec.budget.saturating_sub(started.elapsed());
        match rx.recv_timeout(spec.idle.min(left)) {
            Ok(line) => {
                writeln!(log, "{line}")?;
                if let Line::Result { is_error, text } = stream::scan(&line) {
                    last_result = Some((is_error, text));
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                interrupt(&mut child);
                break Some(if started.elapsed() >= spec.budget { Outcome::Ceiling } else { Outcome::Idle });
            }
            Err(RecvTimeoutError::Disconnected) => break None,
        }
    };
    let status = child.wait()?;
    let active = started.elapsed();
    if let Some(pid_file) = &spec.pid_file {
        let _ = std::fs::remove_file(pid_file);
    }
    let stderr = stderr.join().unwrap_or_default();
    if let Some(outcome) = stopped {
        return Ok((outcome, active));
    }
    let outcome = match last_result {
        Some((true, text)) if limit::parse(&text).is_some() => {
            Outcome::Limit(limit::parse(&text).expect("checked"))
        }
        Some((is_error, text)) => Outcome::Finished { is_error, text },
        None => {
            let lines: Vec<&str> = stderr.lines().collect();
            let tail = lines[lines.len().saturating_sub(STDERR_TAIL)..].join("\n");
            Outcome::Crashed(format!("{status}, no result line; stderr:\n{tail}"))
        }
    };
    Ok((outcome, active))
}
