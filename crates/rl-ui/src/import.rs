//! The Import screen's background job (plan 7 task 7): an OLX game
//! directory found ([`rl_content::import::discover`]) and imported
//! ([`rl_content::import::import`], the pipeline `rl-cli import` runs) into
//! the user data's `packs/` on a thread of its own, so the menus never
//! wait. The thread reports over a channel; [`ImportJob::poll`] gathers
//! the reports into an [`ImportProgress`] the screen draws.
//!
//! - **Cancel** stops the import between two items; a pack is written
//!   hidden and renamed into place, so a cancel, a quit or a crash never
//!   leaves a half-written pack in `packs/` (`rl_content::import`).
//!   Dropping the job cancels it.
//! - **Errors are reports, never panics:** a path that is no OLX
//!   directory, a failed write (permission, full disk) and even a panic
//!   in an importer end the job with [`ImportResult::Failed`]; an item
//!   that fails is an entry with its error, and the rest still import.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::{JoinHandle, ThreadId};
use std::time::{Duration, Instant};

use rl_content::import::{self, ImportEntry, ImportOptions, Progress};

/// The revision recorded in a pack the game imported (`Origin::revision`).
pub const LOCAL_REVISION: &str = "local";

/// How an import ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportResult {
    /// It ran: `imported` packs written and `failed` items of the `tried`
    /// (of `total`; fewer when cancelled), with `warnings` over them.
    Done { imported: usize, failed: usize, warnings: usize, tried: usize, total: usize, cancelled: bool },
    /// It could not run or stopped: why.
    Failed(String),
}

/// What the Import screen shows of the import running or done.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportProgress {
    /// An import runs.
    pub running: bool,
    /// Its cancel was asked for.
    pub cancelling: bool,
    /// The OLX game directory found.
    pub gamedir: Option<PathBuf>,
    /// What the directory holds that is not imported.
    pub notes: Vec<String>,
    /// The item importing (from 0) of `total`, and its source path.
    pub index: usize,
    pub total: usize,
    pub current: Option<String>,
    /// Every item done so far, imported or failed, in order.
    pub entries: Vec<ImportEntry>,
    /// How it ended (`None` while it runs or before the first).
    pub result: Option<ImportResult>,
}

impl ImportProgress {
    /// A new import is starting.
    pub fn started() -> ImportProgress {
        ImportProgress { running: true, ..ImportProgress::default() }
    }

    /// The import could not start: `why`.
    pub fn failed(why: String) -> ImportProgress {
        ImportProgress { result: Some(ImportResult::Failed(why)), ..ImportProgress::default() }
    }

    /// Done items over all, 0 to 1.
    pub fn fraction(&self) -> f32 {
        if self.total == 0 { 0.0 } else { self.entries.len() as f32 / self.total as f32 }
    }
}

enum Msg {
    Worker(ThreadId),
    Found { gamedir: PathBuf, notes: Vec<String> },
    Item { index: usize, total: usize, source: String },
    Entry(ImportEntry),
    Finished(ImportResult),
}

/// An import running on its own thread.
pub struct ImportJob {
    rx: Receiver<Msg>,
    cancel: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    worker: Option<ThreadId>,
    finished: bool,
}

impl ImportJob {
    /// Starts importing the OLX game directory `path` (or its
    /// `share/gamedir`) into `packs` on a new thread; returns at once.
    pub fn spawn(path: PathBuf, packs: PathBuf) -> ImportJob {
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&cancel);
        let report = tx.clone();
        let thread = std::thread::Builder::new()
            .name("olx-import".into())
            .spawn(move || work(&path, &packs, &stop, &tx))
            .map_err(|e| {
                let _ = report.send(Msg::Finished(ImportResult::Failed(format!("starting the import: {e}"))));
            })
            .ok();
        ImportJob { rx, cancel, thread, worker: None, finished: false }
    }

    /// Asks the import to stop after the item it is on.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Gathers the thread's reports into `view`; `true` once, when the
    /// import has ended (`view.result` says how).
    pub fn poll(&mut self, view: &mut ImportProgress) -> bool {
        if self.finished {
            return false;
        }
        view.cancelling |= self.cancel.load(Ordering::Relaxed);
        loop {
            match self.rx.try_recv() {
                Ok(Msg::Worker(id)) => self.worker = Some(id),
                Ok(Msg::Found { gamedir, notes }) => {
                    view.gamedir = Some(gamedir);
                    view.notes = notes;
                }
                Ok(Msg::Item { index, total, source }) => {
                    (view.index, view.total, view.current) = (index, total, Some(source));
                }
                Ok(Msg::Entry(e)) => view.entries.push(e),
                Ok(Msg::Finished(r)) => return self.end(view, r),
                Err(TryRecvError::Empty) => return false,
                // The thread is gone without a word (it cannot panic past
                // `work`'s guard, but never wait for nothing).
                Err(TryRecvError::Disconnected) => {
                    return self.end(view, ImportResult::Failed("the import stopped unexpectedly".into()));
                }
            }
        }
    }

    fn end(&mut self, view: &mut ImportProgress, r: ImportResult) -> bool {
        self.finished = true;
        view.running = false;
        view.cancelling = false;
        view.current = None;
        view.result = Some(r);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        true
    }

    /// The import has ended (reported or not yet polled).
    pub fn is_finished(&self) -> bool {
        self.finished || self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }

    /// Cancels and waits up to `timeout` for the thread to end (the game
    /// quitting): whether it ended. A pack being written when the time is
    /// up stays hidden; the next import removes it.
    pub fn stop(&mut self, timeout: Duration) -> bool {
        self.cancel();
        let until = Instant::now() + timeout;
        while !self.is_finished() {
            if Instant::now() >= until {
                return false;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        true
    }

    /// The thread the import ran on, once it reported (tests: never the
    /// caller's).
    pub fn worker(&self) -> Option<ThreadId> {
        self.worker
    }
}

impl Drop for ImportJob {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// The import thread: find, import, report; a panic is a failure.
fn work(path: &Path, packs: &Path, stop: &AtomicBool, tx: &Sender<Msg>) {
    let _ = tx.send(Msg::Worker(std::thread::current().id()));
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(path, packs, stop, tx)));
    let result = run.unwrap_or_else(|_| ImportResult::Failed("the import failed (an internal error)".into()));
    let _ = tx.send(Msg::Finished(result));
}

fn run(path: &Path, packs: &Path, stop: &AtomicBool, tx: &Sender<Msg>) -> ImportResult {
    let found = match import::discover(path) {
        Ok(d) => d,
        Err(e) => return ImportResult::Failed(format!("{e:#}")),
    };
    let _ = tx.send(Msg::Found { gamedir: found.gamedir.clone(), notes: found.notes.clone() });
    let options = ImportOptions { revision: LOCAL_REVISION.into(), prune: false };
    let report = import::import(&found.gamedir, &found.plan, packs, &options, &mut |p| {
        let _ = match p {
            Progress::Item { index, total, source } => {
                tx.send(Msg::Item { index, total, source: source.into() })
            }
            Progress::Done { entry, .. } => tx.send(Msg::Entry(entry.clone())),
        };
        !stop.load(Ordering::Relaxed)
    });
    match report {
        Ok(r) => ImportResult::Done {
            imported: r.imported(),
            failed: r.failed(),
            warnings: r.warnings(),
            tried: r.entries.len(),
            total: r.total,
            cancelled: r.cancelled,
        },
        Err(e) => ImportResult::Failed(format!("{e:#}")),
    }
}
