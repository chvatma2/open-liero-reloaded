//! Host migration over real connections on 127.0.0.1 (spec §3.7): the
//! successor takes over, the others rejoin it, the match ends and the
//! records agree. Every endpoint's candidates are on 127.0.0.1 (ruling
//! NT-1: `base_override` on the standby and the joins' ICE); every wait is
//! bounded, each test under 30 s.

use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

use rl_online::{HOST_LOST, MigrationConfig, MigrationStep, OnlineEvent, StartError};

use crate::common::net::{Net, content, ended, replay_ready};

fn migrated(e: &OnlineEvent) -> bool {
    matches!(e, OnlineEvent::Migrated { .. })
}

/// `MigrationConfig::direct()` with every candidate on 127.0.0.1 (NT-1).
fn direct() -> MigrationConfig {
    let mut m = MigrationConfig::direct();
    if let Some(standby) = &mut m.standby {
        standby.ice.base_override = Some(Ipv4Addr::LOCALHOST);
    }
    m.ice.base_override = Some(Ipv4Addr::LOCALHOST);
    m
}

/// A host and two clients (Anna is welcomed before Bert joins: the first
/// successor) start a 0.1-minute match.
fn started(content: rl_content::Content) -> (Net, [usize; 2]) {
    let mut net = Net::new(content, 0.1);
    let a = net.join_with_migration("Anna", direct());
    let b = net.join_with_migration("Bert", direct());
    net.run_until(Duration::from_secs(10), "both joined", |n| n.members() == 3);
    net.ready(&[a, b]);
    net.host().peer().lobby().start(7).unwrap();
    net.run_until(Duration::from_secs(10), "the match runs", |n| n.host_tick() > 300);
    (net, [a, b])
}

#[test]
fn a_frozen_host_hands_the_match_to_the_first_player() {
    let Some(content) = content() else { return };
    let (mut net, [a, b]) = started(content);
    // Both clients can host and report it.
    assert!(net.peers[a].peer().standby_port().is_some() && net.peers[b].peer().standby_port().is_some());
    let order = net.host().peer().lobby().succession();
    assert_eq!(order.len(), 2, "{order:?}");
    assert!(order.iter().all(|e| e.can_host && e.direct.is_some()), "{order:?}");
    net.peers[0].frozen = true;
    let froze = Instant::now();
    // What both survivors confirmed at the freeze (the host's last advance
    // may never have left it, B5).
    let held =
        [a, b].map(|i| net.peers[i].peer().match_session().unwrap().frontier()).into_iter().min().unwrap();
    net.run_until(Duration::from_secs(8), "both resume under Anna", |n| {
        n.peers[a].has(|e| matches!(e, OnlineEvent::Migrated { me: true, .. }))
            && n.peers[b].has(|e| matches!(e, OnlineEvent::Migrated { host, me: false } if host == "Anna"))
    });
    // N66: 2 s of silence, then at most 3.5 s on loopback with real clocks.
    assert!(froze.elapsed() < Duration::from_millis(2_000 + 3_500), "{:?}", froze.elapsed());
    assert!(
        net.peers[b].has(|e| matches!(e, OnlineEvent::Migrating(MigrationStep::Moving(n)) if n == "Anna"))
    );
    assert!(net.peers[a].has(|e| *e == OnlineEvent::Migrating(MigrationStep::TakingOver)));
    assert!(net.peers[a].peer().is_host());
    assert!(net.peers[b].has(|e| matches!(e, OnlineEvent::Connected { .. })), "the new link's path");
    net.run_until(Duration::from_secs(20), "the match ends and both make replays", |n| {
        [a, b].iter().all(|&i| n.peers[i].has(ended) && n.peers[i].has(replay_ready))
    });
    let ra = net.peers[a].peer().last_record().cloned().unwrap();
    let rb = net.peers[b].peer().last_record().cloned().unwrap();
    assert_eq!(ra.rows, rb.rows);
    assert_eq!(ra.checksums, rb.checksums);
    // Every input the survivors held at the freeze is kept, as the frozen
    // host had it (spec §3.2: final is final).
    let old = net.peers[0].p.as_ref().unwrap().match_session().unwrap();
    for t in 0..held {
        assert_eq!(Some(ra.rows[t as usize]), old.input_log().row(t), "tick {t}");
    }
    // The old host's player stands still after the migration; the bot
    // (slot 3) still plays, and so does Anna (slot 1, her own keys ahead by
    // her input delay, the bots' at the tick: the driver is called twice).
    let after = &ra.rows[held as usize + 300..];
    assert!(after.iter().all(|row| row[0] == 0));
    assert!(after.iter().any(|row| row[3] != 0), "the bot moved to Anna");
    assert!(after.iter().any(|row| row[1] != 0), "Anna plays on");
    assert!(after.iter().any(|row| row[2] != 0), "Bert plays on");
    assert!(!net.peers[a].has(|e| matches!(e, OnlineEvent::DesyncBundle(_))));
    assert!(!net.peers[b].has(|e| matches!(e, OnlineEvent::DesyncBundle(_))));
    // Back in Anna's lobby: Anna is its host, the old host is no member.
    net.run_until(Duration::from_secs(10), "the results, then gathering", |n| {
        n.peers[b].peer().lobby().state().is_some_and(|s| s.phase == rl_proto::Phase::Gathering)
    });
    let host_key = net.peers[0].key;
    let state = net.peers[b].peer().lobby().state().cloned().unwrap();
    assert!(state.members.iter().all(|m| m.key != host_key));
    assert!(!matches!(net.peers[a].peer().lobby().can_start(), Err(StartError::NotHost)));
    assert!(matches!(net.peers[b].peer().lobby().can_start(), Err(StartError::NotHost)));
    assert_eq!(net.host_index(), Some(0), "the frozen host still thinks it hosts");
}

#[test]
fn a_leaving_host_is_followed_at_once() {
    let Some(content) = content() else { return };
    let (mut net, [a, b]) = started(content);
    net.host().peer().leave(); // its Byes go out with leave's own endpoint poll
    let left = Instant::now();
    net.peers[0].frozen = true;
    net.run_until(Duration::from_secs(5), "both resume", |n| {
        n.peers[a].has(migrated) && n.peers[b].has(migrated)
    });
    assert!(left.elapsed() < Duration::from_millis(1_500), "no 2 s wait (N76): {:?}", left.elapsed());
    // A7: the lobby's `Bye` did not end them.
    for i in [a, b] {
        assert!(
            !net.peers[i].has(|e| matches!(e, OnlineEvent::Disconnected(_))),
            "{:?}",
            net.peers[i].events
        );
    }
}

#[test]
fn a_stalled_host_steps_down_and_the_match_moves() {
    let Some(content) = content() else { return };
    let (mut net, [a, b]) = started(content);
    net.peers[0].frozen = true;
    std::thread::sleep(Duration::from_millis(1_700)); // nobody polls: the host's own stall
    net.peers[0].frozen = false;
    net.run_until(Duration::from_secs(8), "the host steps down and the match moves", |n| {
        n.peers[0].has(|e| matches!(e, OnlineEvent::HostLost(t) if t.starts_with("Your game stopped")))
            && n.peers[a].has(migrated)
            && n.peers[b].has(migrated)
    });
    assert!(net.peers[0].peer().match_session().is_none(), "no match left on the old host");
    // Its replay of what it confirmed is made, as at any end.
    net.run_until(Duration::from_secs(5), "the old host's replay", |n| n.peers[0].has(replay_ready));
    assert_eq!(net.host_index(), Some(0), "its lobby stays open, without them");
    net.run_until(Duration::from_secs(5), "the old host's lobby is empty", |n| n.members() == 1);
}

#[test]
fn nobody_to_take_over_is_host_lost_with_a_replay() {
    let Some(content) = content() else { return };
    let mut net = Net::new(content, 0.5); // longer than the 12 s give-up
    let a = net.join_with_migration("Anna", direct());
    let b = net.join_with_migration("Bert", MigrationConfig { standby: None, ..direct() });
    net.run_until(Duration::from_secs(10), "both joined", |n| n.members() == 3);
    net.ready(&[a, b]);
    net.host().peer().lobby().start(7).unwrap();
    net.run_until(Duration::from_secs(10), "the match runs", |n| n.host_tick() > 200);
    assert!(net.peers[b].peer().standby_port().is_none(), "N79: no standby, never a successor");
    net.peers[0].frozen = true;
    net.peers[a].frozen = true; // Anna, the only successor, is gone too
    let froze = Instant::now();
    net.run_until(Duration::from_secs(14), "Bert gives up", |n| {
        n.peers[b].has(|e| *e == OnlineEvent::HostLost(HOST_LOST.into()))
    });
    let took = froze.elapsed();
    assert!(took >= Duration::from_millis(11_900) && took < Duration::from_millis(13_000), "{took:?}");
    assert!(net.peers[b].has(|e| *e == OnlineEvent::Migrating(MigrationStep::Finding)));
    net.run_until(Duration::from_secs(5), "its replay", |n| n.peers[b].has(replay_ready));
    assert!(net.peers[b].peer().is_closed());
}

#[test]
fn a_successors_own_outage_does_not_take_the_match() {
    // N69: Anna's own link is cut for 3 s while the host is fine. Anna finds
    // the host silent, opens its standby (its turn: the first successor),
    // nobody comes, the link returns: Anna cancels and stays a client.
    let Some(content) = content() else { return };
    let (mut net, [a, b]) = started(content);
    net.outage(a, Duration::from_secs(3)); // test hook: drop everything on Anna's endpoint both ways
    net.run_until(Duration::from_secs(8), "Anna is back with the host", |n| {
        n.peers[a].has(|e| matches!(e, OnlineEvent::Migrating(_)))
            && n.peers[a].peer().match_session().is_some_and(|s| !s.suspended())
    });
    assert!(!net.peers[a].has(migrated) && !net.peers[b].has(migrated));
    assert!(net.host().peer().is_host());
    assert!(!net.peers[a].peer().is_host());
}

/// D8: a successor whose window ends before anyone rejoined it closes its
/// gate, and the joins it answered fail at once (`NotHost`): their peers
/// lose contact and move to the next window. Anna, the first successor,
/// hears nobody on UDP (an outage longer than everything): Bert and Cid
/// reach her signalling (contact) but no link can open. Without the check
/// they keep trying Anna until the give-up (host lost); with it Bert takes
/// over in his window and Cid follows.
#[test]
fn an_uncommitted_successor_closes_at_its_windows_end() {
    let Some(content) = content() else { return };
    let mut net = Net::new(content, 0.5);
    let a = net.join_with_migration("Anna", direct());
    let b = net.join_with_migration("Bert", direct());
    let c = net.join_with_migration("Cid", direct());
    net.ready(&[a, b, c]);
    net.host().peer().lobby().start(7).unwrap();
    net.run_until(Duration::from_secs(10), "the match runs", |n| n.host_tick() > 200);
    net.peers[0].frozen = true;
    net.outage(a, Duration::from_secs(60));
    let froze = Instant::now();
    net.run_until(Duration::from_secs(16), "Bert hosts and Cid follows", |n| {
        n.peers[b].has(|e| matches!(e, OnlineEvent::Migrated { me: true, .. }))
            && n.peers[c].has(|e| matches!(e, OnlineEvent::Migrated { host, me: false } if host == "Bert"))
    });
    // Past Anna's window (2 s of silence, then her 5 s).
    assert!(froze.elapsed() > Duration::from_secs(7), "{:?}", froze.elapsed());
    for i in [b, c] {
        assert!(
            net.peers[i]
                .has(|e| matches!(e, OnlineEvent::Migrating(MigrationStep::Moving(n)) if n == "Anna"))
        );
        assert!(!net.peers[i].has(|e| matches!(e, OnlineEvent::HostLost(_))));
    }
    assert!(net.peers[b].peer().is_host());
    assert!(!net.peers[a].peer().is_host(), "Anna never took the match");
}
