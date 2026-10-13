//! Real transport on one machine (networking spec §9.4): `rl-server` in a
//! thread and headless bot clients over localhost through `rl-net`'s
//! impairment layer, judged by `TransportRun::verdict` (every client's
//! confirmed checksums equal the server's, equal replays, a replay
//! reproducing the server's checksums). Its own binary: `cargo xtask full`
//! runs it by name (`check::FULL_TESTS`) with `RL_FULL_TESTS` for the
//! full-tier tests. Classic on Dirt Level from `RL_CONTENT_DIR` (skipped
//! with a notice without it); every wait is bounded.

mod common;

use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::{content, content_dir, full_tier};
use rl_cli::transport::{SharedLog, TransportRun, TransportSpec, run_transport};
use rl_online::replay::MatchRecord;

/// Runs `spec` and panics with every failure and the server's log unless
/// the verdict holds.
fn pass(spec: &TransportSpec) -> TransportRun {
    let (Some(dir), Some(c)) = (content_dir(), content()) else { unreachable!("checked by the caller") };
    let run = run_transport(Path::new(&dir), spec).unwrap();
    let profile = spec.profile.as_deref().unwrap_or("none");
    eprintln!("{} players through {profile}: {:.1} s", spec.players, run.elapsed.as_secs_f32());
    if let Err(why) = run.verdict(&c, spec) {
        panic!("the verdict ({profile}):\n{why}\nthe server's log:\n{}", run.server_log);
    }
    run
}

/// Every player's bot pressed keys in its slot.
fn assert_played(run: &TransportRun) {
    for c in run.clients.iter().filter(|c| c.matches.iter().all(|m| m.slot.is_some())) {
        for m in &c.matches {
            let slot = usize::from(m.slot.unwrap());
            let rows = &m.record.as_ref().expect("a player's record").rows;
            assert!(rows.iter().any(|r| r[slot] != 0), "{}'s bot pressed no key in slot {slot}", c.name);
        }
    }
}

#[test]
fn two_bots_play_a_match_over_real_connections() {
    if content_dir().is_none() {
        return;
    }
    let data = tempfile::tempdir().unwrap();
    let spec = TransportSpec::new(2, 0.2, Some("typical"), data.path());
    let run = pass(&spec);
    assert_eq!(run.clients.len(), 2);
    let r = &run.server[0];
    assert!(r.rows.len() >= 1200, "the whole match: {} ticks", r.rows.len());
    let mut slots: Vec<Option<u8>> = run.clients.iter().map(|c| c.matches[0].slot).collect();
    slots.sort();
    assert_eq!(slots, [Some(0), Some(1)]);
    assert!(run.clients.iter().all(|c| c.matches[0].replay.is_some()), "every client made its replay");
    assert_played(&run);
}

/// `rl-cli net-client --bot` against a server in this process (the client
/// and one of the server's bots): it plays one match, prints its confirmed
/// checksums (the server's) and exits 0.
#[test]
fn the_net_client_command_plays_a_match_and_prints_its_checksums() {
    let (Some(dir), Some(_)) = (content_dir(), content()) else { return };
    let data = tempfile::tempdir().unwrap();
    let mut spec = TransportSpec::new(1, 0.05, None, data.path());
    spec.fill_bots = 2;
    let stop = Arc::new(AtomicBool::new(false));
    let records: Arc<Mutex<Vec<MatchRecord>>> = Arc::default();
    let port = Arc::new(Mutex::new(None));
    let log = SharedLog::default();
    let server = {
        let (config, data) = (spec.server_config(), data.path().to_path_buf());
        let content = rl_content::Content::open(Path::new(&dir)).unwrap();
        let (stop, records, port, mut log) =
            (Arc::clone(&stop), Arc::clone(&records), Arc::clone(&port), log.clone());
        std::thread::spawn(move || {
            rl_server::run_with(config, content, &data, &stop, &mut log, &mut |s| {
                *port.lock().unwrap() = Some(s.local_port());
                let mut r = records.lock().unwrap();
                if let Some(rec) = s.last_record().filter(|_| r.is_empty()) {
                    r.push(rec.clone());
                }
            })
        })
    };
    let begun = Instant::now();
    let port = loop {
        assert!(begun.elapsed() < Duration::from_secs(20), "the server listens");
        if let Some(p) = *port.lock().unwrap() {
            break p;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let mut child = Command::new(env!("CARGO_BIN_EXE_rl-cli"))
        .args(["net-client", "--join", &format!("127.0.0.1:{port}"), "--bot", "--name", "Cli"])
        .args(["--difficulty", "hard", "--matches", "1", "--impair", "good", "--timeout", "40"])
        .arg("--content")
        .arg(&dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        if begun.elapsed() > Duration::from_secs(90) {
            child.kill().unwrap();
            panic!("net-client did not exit within 90 s");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        status.success(),
        "net-client failed: {status}\n{stdout}\n{stderr}\nthe server's log:\n{}",
        log.text()
    );
    while records.lock().unwrap().is_empty() {
        assert!(begun.elapsed() < Duration::from_secs(100), "the server keeps the match's record");
        std::thread::sleep(Duration::from_millis(5));
    }
    stop.store(true, Ordering::Relaxed);
    server.join().unwrap().unwrap();
    let record = records.lock().unwrap()[0].clone();
    let header = format!("match 1: olx-classic on olx-level-dirt-level, seed {}, slot 0,", record.start.seed);
    assert!(stdout.contains(&header), "{header:?} in:\n{stdout}");
    let printed: Vec<(u32, u64)> = stdout
        .lines()
        .filter_map(|l| l.strip_prefix("  checksum "))
        .map(|l| {
            let (t, c) = l.split_once(' ').unwrap();
            (t.parse().unwrap(), u64::from_str_radix(c, 16).unwrap())
        })
        .collect();
    assert_eq!(printed, record.checksums, "the printed checksums are the server's");
}

#[test]
fn eight_bots_per_profile() {
    if !full_tier() || content_dir().is_none() {
        return;
    }
    for (i, profile) in ["good", "typical", "bad"].into_iter().enumerate() {
        let data = tempfile::tempdir().unwrap();
        let mut spec = TransportSpec::new(8, 0.5, Some(profile), data.path());
        spec.net_seed = 100 * (i as u64 + 1);
        let run = pass(&spec);
        assert_eq!(run.clients.len(), 8);
        assert_played(&run);
    }
}

#[test]
fn a_spectator_joins_mid_match_over_real_connections() {
    if !full_tier() || content_dir().is_none() {
        return;
    }
    let data = tempfile::tempdir().unwrap();
    let mut spec = TransportSpec::new(2, 0.2, Some("typical"), data.path());
    spec.spectators = vec![Duration::from_secs(4)];
    let run = pass(&spec);
    let watcher = &run.clients[2];
    assert_eq!(watcher.name, TransportSpec::spectator_name(0));
    let m = &watcher.matches[0];
    assert_eq!(m.slot, None, "it watches");
    assert!(m.record.is_none() && m.replay.is_none(), "it joined mid-match: no record from tick 0");
    let first = m.checksums.first().expect("its confirmed checksums").0;
    assert!(first >= 300, "from the state it joined with, not tick 0: tick {first}");
    assert_played(&run);
}
