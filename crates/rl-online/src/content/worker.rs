//! The content worker: one background thread that archives packs to serve,
//! hashes packs and installs downloads, off the caller's thread.

use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use rl_content::DownloadCache;
use rl_data::manifest::PackKind;
use rl_vfs::DirFs;
use sha2::{Digest, Sha256};

/// How long dropping a `Worker` waits for its thread to finish its job.
const JOIN_BOUND: Duration = Duration::from_secs(2);

/// A pack's archive, ready to serve: its bytes and their SHA-256 (hex).
#[derive(Debug, PartialEq, Eq)]
pub struct PackArchive {
    pub bytes: Vec<u8>,
    pub sha: String,
}

impl PackArchive {
    pub fn new(bytes: Vec<u8>) -> PackArchive {
        let sha = hex::encode(Sha256::digest(&bytes));
        PackArchive { bytes, sha }
    }
}

/// Work for the worker.
#[derive(Clone, Debug)]
pub enum Job {
    /// Archive the pack in `dir`, whose content hash must be `hash`.
    Archive { hash: String, dir: PathBuf },
    /// Install the complete partial download of `hash`
    /// (`DownloadCache::install`; the cache is locked only to start and to
    /// finish, not while it verifies).
    Install { hash: String, id: String, kind: PackKind, limit: u64 },
    /// The content hash of pack `id` in `dir`.
    Hash { id: String, dir: PathBuf },
}

/// A job's result; errors in words.
#[derive(Clone, Debug)]
pub enum Done {
    Archived { hash: String, archive: Result<Arc<PackArchive>, String> },
    Installed { hash: String, dir: Result<PathBuf, String> },
    Hashed { id: String, hash: Result<String, String> },
}

/// The worker thread. Jobs run one at a time, in order; `take` returns the
/// results so far. Dropping it stops the thread after its current job.
pub struct Worker {
    jobs: Option<Sender<Job>>,
    done: Receiver<Done>,
    /// Answers jobs when the thread could not be started.
    fallback: Sender<Done>,
    stop: Arc<AtomicBool>,
    exited: Receiver<()>,
    thread: Option<JoinHandle<()>>,
}

impl Worker {
    /// The worker; without a cache (`None`: downloads are off) it archives
    /// and hashes, and every install fails.
    pub fn start(cache: Option<Arc<Mutex<DownloadCache>>>) -> Worker {
        let (jobs, job_rx) = mpsc::channel::<Job>();
        let (done_tx, done) = mpsc::channel();
        let (exit_tx, exited) = mpsc::channel::<()>();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let thread_done = done_tx.clone();
        let thread = std::thread::Builder::new()
            .name("rl-online-content".into())
            .spawn(move || {
                let _exit = exit_tx; // dropped when the thread ends
                while let Ok(job) = job_rx.recv() {
                    if thread_stop.load(Ordering::Relaxed) {
                        break;
                    }
                    // A panic in a parser of untrusted content fails its
                    // job, not the thread.
                    let backup = job.clone();
                    let done = std::panic::catch_unwind(AssertUnwindSafe(|| run(cache.as_deref(), job)))
                        .unwrap_or_else(|panic| failed(backup, &panic_message(panic.as_ref())));
                    if thread_done.send(done).is_err() {
                        break;
                    }
                }
            })
            .ok();
        Worker { jobs: thread.is_some().then_some(jobs), done, fallback: done_tx, stop, exited, thread }
    }

    pub fn submit(&self, job: Job) {
        if let Some(jobs) = &self.jobs
            && jobs.send(job.clone()).is_ok()
        {
            return;
        }
        let _ = self.fallback.send(failed(job, "the content worker is not running"));
    }

    /// The results of the jobs finished since the last call, in order.
    pub fn take(&self) -> Vec<Done> {
        self.done.try_iter().collect()
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.jobs = None;
        if let Some(thread) = self.thread.take() {
            // The thread ends after its current job; a long one is left to
            // finish on its own (its result is dropped).
            if matches!(self.exited.recv_timeout(JOIN_BOUND), Err(RecvTimeoutError::Disconnected)) {
                let _ = thread.join();
            }
        }
    }
}

fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    let what = panic
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("unknown");
    format!("the content worker failed: {what}")
}

fn failed(job: Job, why: &str) -> Done {
    match job {
        Job::Archive { hash, .. } => Done::Archived { hash, archive: Err(why.into()) },
        Job::Install { hash, .. } => Done::Installed { hash, dir: Err(why.into()) },
        Job::Hash { id, .. } => Done::Hashed { id, hash: Err(why.into()) },
    }
}

/// A `Job::Hash` of this id panics, for the tests.
#[cfg(test)]
const PANIC_ID: &str = "test-panic";

fn run(cache: Option<&Mutex<DownloadCache>>, job: Job) -> Done {
    #[cfg(test)]
    if matches!(&job, Job::Hash { id, .. } if id == PANIC_ID) {
        panic!("a test panic");
    }
    match job {
        Job::Archive { hash, dir } => {
            let archive = archive(&hash, DirFs::new(dir));
            Done::Archived { hash, archive }
        }
        Job::Install { hash, id, kind, limit } => {
            let dir = match cache {
                Some(cache) => install(cache, &hash, &id, kind, limit),
                None => Err("there is no download cache".to_string()),
            };
            Done::Installed { hash, dir }
        }
        Job::Hash { id, dir } => {
            let hash = rl_pack::content_hash(&DirFs::new(dir)).map_err(|e| e.to_string());
            Done::Hashed { id, hash }
        }
    }
}

fn archive(hash: &str, src: DirFs) -> Result<Arc<PackArchive>, String> {
    let bytes = rl_pack::archive::encode(&src).map_err(|e| e.to_string())?;
    // The pack may have changed since it was hashed: never serve other content.
    let content = rl_pack::content_hash(&src).map_err(|e| e.to_string())?;
    if content != hash {
        return Err(format!("the pack changed: its content is {content}, not {hash}"));
    }
    Ok(Arc::new(PackArchive::new(bytes)))
}

fn install(
    cache: &Mutex<DownloadCache>,
    hash: &str,
    id: &str,
    kind: PackKind,
    limit: u64,
) -> Result<PathBuf, String> {
    let lock = || cache.lock().map_err(|_| "the download cache failed".to_string());
    let pending = lock()?.begin_install(hash, id, kind, limit).map_err(|e| e.to_string())?;
    let verified = pending.verify();
    lock()?.finish_install(pending, verified).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn wait(worker: &Worker, n: usize) -> Vec<Done> {
        let until = Instant::now() + Duration::from_secs(10);
        let mut done = vec![];
        while done.len() < n {
            assert!(Instant::now() < until, "the worker did not finish: {done:?}");
            done.extend(worker.take());
            std::thread::sleep(Duration::from_millis(2));
        }
        done
    }

    #[test]
    fn a_panicking_job_fails_and_the_worker_goes_on() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Arc::new(Mutex::new(DownloadCache::open(dir.path()).unwrap()));
        let worker = Worker::start(Some(cache));
        worker.submit(Job::Hash { id: PANIC_ID.into(), dir: dir.path().into() });
        worker.submit(Job::Hash { id: "after".into(), dir: dir.path().into() });
        let done = wait(&worker, 2);
        assert!(
            matches!(&done[0], Done::Hashed { id, hash: Err(e) } if id == PANIC_ID && e.contains("a test panic")),
            "{done:?}"
        );
        // The thread lives on.
        assert!(matches!(&done[1], Done::Hashed { id, hash: Ok(_) } if id == "after"), "{done:?}");
    }
}
