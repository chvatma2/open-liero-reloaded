//! Host migration over real connections on 127.0.0.1 (spec §3.7): the
//! successor takes over, the others rejoin it, the match ends and the
//! records agree. Every endpoint's candidates are on 127.0.0.1 (ruling
//! NT-1: `base_override` on the standby and the joins' ICE); every wait is
//! bounded, each test under 30 s.

use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

use rl_online::migration::HOST_BACK;
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
    // What each survivor confirmed at the freeze (the host's last advance
    // may never have left it, B5).
    let held = [a, b].map(|i| net.peers[i].peer().match_session().unwrap().frontier());
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
    // Every input each survivor held at the freeze is kept, as the frozen
    // host had it (spec §3.2: final is final).
    let old = net.peers[0].p.as_ref().unwrap().match_session().unwrap();
    for (i, &upto) in held.iter().enumerate() {
        for t in 0..upto {
            assert_eq!(Some(ra.rows[t as usize]), old.input_log().row(t), "survivor {i}, tick {t}");
        }
    }
    let held = held.into_iter().max().unwrap();
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
    net.run_until(Duration::from_secs(5), "both begin", |n| {
        [a, b].iter().all(|&i| n.peers[i].has(|e| matches!(e, OnlineEvent::Migrating(_))))
    });
    // N76: at once, not after `HOST_SILENT_US`.
    assert!(left.elapsed() < Duration::from_millis(1_900), "no 2 s wait (N76): {:?}", left.elapsed());
    net.run_until(Duration::from_secs(5), "both resume", |n| {
        n.peers[a].has(migrated) && n.peers[b].has(migrated)
    });
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
    // 2 s of silence, then `GIVE_UP_US`: not before; the upper bound only
    // guards against a hang.
    assert!(took >= Duration::from_millis(11_900) && took < Duration::from_millis(13_500), "{took:?}");
    assert!(net.peers[b].has(|e| *e == OnlineEvent::Migrating(MigrationStep::Finding)));
    net.run_until(Duration::from_secs(5), "its replay", |n| n.peers[b].has(replay_ready));
    assert!(net.peers[b].peer().is_closed());
}

#[test]
fn a_successors_own_outage_does_not_take_the_match() {
    // N69: Anna's own link is cut for 2.5 s while the host is fine (well
    // under the transport's ~3 s cliff, past which the host's datagrams
    // come back only seconds later). Anna finds the host silent, opens its
    // standby (its turn: the first successor), nobody comes, the link
    // returns: Anna cancels and stays a client.
    let Some(content) = content() else { return };
    let (mut net, [a, b]) = started(content);
    let cut = Instant::now();
    net.outage(a, Duration::from_millis(2_500)); // test hook: drop everything on Anna's endpoint both ways
    net.run_until(Duration::from_secs(8), "Anna is back with the host", |n| {
        n.peers[a].has(|e| *e == OnlineEvent::Notice(HOST_BACK.into()))
            && n.peers[a].peer().match_session().is_some_and(|s| !s.suspended())
    });
    // Called off within about a second of the outage's end, in her turn.
    assert!(cut.elapsed() < Duration::from_millis(4_000), "{:?}", cut.elapsed());
    assert!(net.peers[a].has(|e| *e == OnlineEvent::Migrating(MigrationStep::TakingOver)));
    assert!(!net.peers[a].has(|e| matches!(e, OnlineEvent::Migrating(MigrationStep::Moving(_)))));
    assert!(!net.peers[a].has(migrated) && !net.peers[b].has(migrated));
    assert!(net.host().peer().is_host());
    assert!(!net.peers[a].peer().is_host());
}

/// Bert and Cid, the survivors, confirmed the same rows and checksums.
fn records_agree(net: &mut Net, [b, c]: [usize; 2]) {
    net.run_until(Duration::from_secs(25), "the match ends and both make replays", |n| {
        [b, c].iter().all(|&i| n.peers[i].has(ended) && n.peers[i].has(replay_ready))
    });
    let rb = net.peers[b].peer().last_record().cloned().unwrap();
    let rc = net.peers[c].peer().last_record().cloned().unwrap();
    assert_eq!(rb.rows, rc.rows);
    assert_eq!(rb.checksums, rc.checksums);
    for i in [b, c] {
        assert!(!net.peers[i].has(|e| matches!(e, OnlineEvent::DesyncBundle(_))));
    }
}

/// A host and three clients, welcomed in order, in a match of `minutes`.
fn started3(content: rl_content::Content, minutes: f32) -> (Net, [usize; 3]) {
    let mut net = Net::new(content, minutes);
    let a = net.join_with_migration("Anna", direct());
    let b = net.join_with_migration("Bert", direct());
    let c = net.join_with_migration("Cid", direct());
    net.ready(&[a, b, c]);
    net.host().peer().lobby().start(7).unwrap();
    net.run_until(Duration::from_secs(10), "the match runs", |n| n.host_tick() > 200);
    (net, [a, b, c])
}

/// D8: a successor whose window ends before anyone rejoined it closes its
/// gate, and the joins it answered fail: their peers lose contact and move
/// to the next window. Anna, the first successor, hears nobody on UDP (an
/// outage longer than everything): Bert and Cid reach her signalling
/// (contact) but no link can open. Without losing that contact they keep
/// trying Anna until the give-up (host lost); with it Bert takes over in
/// his window, Cid follows, and their records agree.
#[test]
fn an_uncommitted_successor_closes_at_its_windows_end() {
    let Some(content) = content() else { return };
    let (mut net, [a, b, c]) = started3(content, 0.15);
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
    records_agree(&mut net, [b, c]);
}

/// D8, links open: Bert's join to Anna opens but its `Hello` comes only 6 s
/// later (`test_delay_rejoin`), past Anna's window. Anna, uncommitted at its
/// end, closes the link; Bert loses contact, takes over in his own window,
/// and Anna rejoins him. Without the gate closing, Bert would hold on to
/// Anna until the give-up.
#[test]
fn a_successors_unwelcomed_links_close_at_its_windows_end() {
    let Some(content) = content() else { return };
    let mut net = Net::new(content, 0.1);
    let a = net.join_with_migration("Anna", direct());
    let b = net.join_with_migration("Bert", direct());
    net.ready(&[a, b]);
    net.host().peer().lobby().start(7).unwrap();
    net.run_until(Duration::from_secs(10), "the match runs", |n| n.host_tick() > 300);
    net.peers[b].peer().test_delay_rejoin(Duration::from_secs(6));
    net.peers[0].frozen = true;
    net.run_until(Duration::from_secs(14), "Bert hosts and Anna follows", |n| {
        n.peers[b].has(|e| matches!(e, OnlineEvent::Migrated { me: true, .. }))
            && n.peers[a].has(|e| matches!(e, OnlineEvent::Migrated { host, me: false } if host == "Bert"))
    });
    assert!(net.peers[a].has(|e| *e == OnlineEvent::Migrating(MigrationStep::TakingOver)));
    assert!(
        net.peers[b].has(|e| matches!(e, OnlineEvent::Migrating(MigrationStep::Moving(n)) if n == "Anna"))
    );
    records_agree(&mut net, [a, b]);
}

/// A successor that dies after answering (Anna answers Bert's and Cid's
/// joins while cut off on UDP, then freezes: she never refuses them): the
/// contact lapses `ANSWER_OPEN_TIMEOUT` after the answer, so they move on
/// with the others at her window's end, not to a false host lost once
/// their setups time out past the give-up.
#[test]
fn a_successor_dead_after_its_answer_is_left() {
    let Some(content) = content() else { return };
    let (mut net, [a, b, c]) = started3(content, 0.5);
    net.peers[0].frozen = true;
    net.outage(a, Duration::from_secs(60));
    net.run_until(Duration::from_secs(5), "both try Anna", |n| {
        [b, c].iter().all(|&i| {
            n.peers[i].has(|e| matches!(e, OnlineEvent::Migrating(MigrationStep::Moving(n)) if n == "Anna"))
        })
    });
    // Her answers leave within this.
    let t = Instant::now();
    net.run_until(Duration::from_secs(2), "Anna answers", |_| t.elapsed() > Duration::from_millis(700));
    net.peers[a].frozen = true;
    net.run_until(Duration::from_secs(14), "Bert hosts and Cid follows", |n| {
        n.peers[b].has(|e| matches!(e, OnlineEvent::Migrated { me: true, .. }))
            && n.peers[c].has(|e| matches!(e, OnlineEvent::Migrated { host, me: false } if host == "Bert"))
    });
    for i in [b, c] {
        assert!(!net.peers[i].has(|e| matches!(e, OnlineEvent::HostLost(_))));
    }
}

/// C1: the new host waits for the reports of the members back, even past
/// the moment it could resume. Anna's own link blacks out for 0.7 s just
/// before the host freezes, so Bert holds rows Anna lacks (the host's
/// lagging fill confirmed them without her): every one of them is kept.
#[test]
fn a_reporter_ahead_of_the_successor_keeps_its_rows() {
    let Some(content) = content() else { return };
    let (mut net, [a, b]) = started(content);
    net.outage(a, Duration::from_millis(700));
    let t0 = Instant::now();
    net.run_until(Duration::from_secs(3), "Bert gets ahead", |_| t0.elapsed() > Duration::from_millis(500));
    net.peers[0].frozen = true;
    let fa = net.peers[a].peer().match_session().unwrap().frontier();
    let fb = net.peers[b].peer().match_session().unwrap().frontier();
    assert!(fb > fa + 5, "Bert ahead: {fa} {fb}");
    net.run_until(Duration::from_secs(10), "both resume under Anna", |n| {
        n.peers[a].has(|e| matches!(e, OnlineEvent::Migrated { me: true, .. }))
            && n.peers[b].has(|e| matches!(e, OnlineEvent::Migrated { me: false, .. }))
    });
    net.run_until(Duration::from_secs(20), "the match ends and both make replays", |n| {
        [a, b].iter().all(|&i| n.peers[i].has(ended) && n.peers[i].has(replay_ready))
    });
    let ra = net.peers[a].peer().last_record().cloned().unwrap();
    let rb = net.peers[b].peer().last_record().cloned().unwrap();
    let old = net.peers[0].p.as_ref().unwrap().match_session().unwrap();
    for t in 0..fb {
        assert_eq!(Some(ra.rows[t as usize]), old.input_log().row(t), "tick {t} (anna {fa}, bert {fb})");
    }
    assert_eq!(ra.rows, rb.rows);
    assert_eq!(ra.checksums, rb.checksums);
    assert!(!net.peers[b].has(|e| matches!(e, OnlineEvent::DesyncBundle(_))));
}

/// A rejoined member that never reports (`test_withhold_reports`) holds the
/// new host only until its window's end plus `REPORT_WAIT` (C1's wait has a
/// deadline): Anna waits for Bert's report until then, then resumes.
#[test]
fn a_report_that_never_comes_holds_nobody_for_ever() {
    let Some(content) = content() else { return };
    let (mut net, [a, b]) = started(content);
    net.peers[b].peer().test_withhold_reports();
    net.peers[0].frozen = true;
    let froze = Instant::now();
    net.run_until(Duration::from_secs(14), "Anna resumes", |n| {
        n.peers[a].has(|e| matches!(e, OnlineEvent::Migrated { me: true, .. }))
    });
    // 2 s of silence, her 5 s window and the 1 s wait for the report: not
    // before it (the report was awaited), and no hang.
    let took = froze.elapsed();
    assert!(took > Duration::from_millis(7_500), "{took:?}");
    net.run_until(Duration::from_secs(5), "Bert resumes too", |n| n.peers[b].has(migrated));
}

/// N69 for a follower: the old host comes back (its own 2.8 s outage) while
/// Bert's join to Anna is open but its `Hello` held (`test_delay_rejoin`):
/// Bert, in contact, keeps the migration, but Anna calls hers off and
/// closes the link; losing that contact, Bert goes back to the host too,
/// instead of running through the windows to a host lost.
#[test]
fn a_follower_in_contact_when_the_host_returns_goes_back_too() {
    let Some(content) = content() else { return };
    let (mut net, [a, b]) = started(content);
    net.peers[b].peer().test_delay_rejoin(Duration::from_secs(3));
    let cut = Instant::now();
    net.outage(0, Duration::from_millis(2_800));
    net.run_until(Duration::from_secs(8), "both are back with the host", |n| {
        [a, b].iter().all(|&i| {
            n.peers[i].has(|e| *e == OnlineEvent::Notice(HOST_BACK.into()))
                && n.peers[i].peer().match_session().is_some_and(|s| !s.suspended())
        })
    });
    assert!(cut.elapsed() < Duration::from_secs(6), "{:?}", cut.elapsed());
    assert!(
        net.peers[b].has(|e| matches!(e, OnlineEvent::Migrating(MigrationStep::Moving(n)) if n == "Anna"))
    );
    assert!(!net.peers[a].has(migrated) && !net.peers[b].has(migrated));
    assert!(net.host().peer().is_host());
}
