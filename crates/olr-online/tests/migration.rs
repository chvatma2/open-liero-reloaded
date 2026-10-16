//! Host migration over real connections on 127.0.0.1 (spec §3.7): the
//! successor takes over, the others rejoin it, the match ends and the
//! records agree. Every endpoint's candidates are on 127.0.0.1 (ruling
//! NT-1: `base_override` on the standby and the joins' ICE); every wait is
//! bounded, each test under 30 s.

use std::collections::BTreeMap;
use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

use olr_discovery::{PoolConfig, Region, TestRelay};
use olr_netcode::migration::{GIVE_UP_US, HOST_SILENT_US, REASK_US, Step, WINDOW_US};
use olr_online::migration::{ANSWER_OPEN_TIMEOUT, Connector, Failure, HOST_BACK, Planner, Route};
use olr_online::{
    HOST_LOST, Join, JoinStep, LostWhy, MigrationConfig, MigrationStep, OnlineEvent, StartError,
};
use olr_proto::SuccessorWire;

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
fn started(content: olr_content::Content) -> (Net, [usize; 2]) {
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
    // N88: the old host's player is withdrawn at the resume, at its first
    // unknown tick, alike on both.
    let g0 = ra.gone.iter().find(|g| g.1 == 0).expect("the old host's player is withdrawn");
    assert!(g0.0 >= held, "withdrawn at {} before what a survivor held, {held}", g0.0);
    assert_eq!(ra.gone, rb.gone);
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
        n.peers[b].peer().lobby().state().is_some_and(|s| s.phase == olr_proto::Phase::Gathering)
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
        n.peers[0]
            .has(|e| matches!(e, OnlineEvent::HostLost(t) if t.to_string().starts_with("Your game stopped")))
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
        n.peers[b].has(|e| matches!(e, OnlineEvent::HostLost(LostWhy::NobodyReachable(_))))
    });
    let took = froze.elapsed();
    // Ruling FR12c-4: his attempt's outcome was said (the successor's name,
    // her address and why), and his host lost names it.
    let port = net.peers[a].peer().standby_port().unwrap();
    let said = format!("Anna (directly at 127.0.0.1:{port}) did not answer before its turn ended");
    let logs: Vec<&String> = net.peers[b]
        .events
        .iter()
        .filter_map(|e| match e {
            OnlineEvent::MigrationLog(l) => Some(l),
            _ => None,
        })
        .collect();
    assert_eq!(logs, [&said], "once");
    let lost = net.peers[b].events.iter().find_map(|e| match e {
        OnlineEvent::HostLost(why) => Some(why.to_string()),
        _ => None,
    });
    assert_eq!(lost, Some(format!("{HOST_LOST} Last try: {said}.")));
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

/// Ruling FR12c-1 (final review A-C1, B-I2): a host and one player. A
/// 2.5 s blip on either side's link (under the transport's ~3 s cliff) is a
/// stall, as before plan 12c: Anna's turn opens her gate, but expecting
/// nobody she commits only to a known-gone host (its `Bye`), and the host,
/// with one remote member, is never cut off (SB-2). Anna goes back to the
/// host each time; the match plays to its end, both records alike. Before,
/// Anna took the match over at once and the host left it (`CUT_OFF`).
#[test]
fn a_blip_in_a_two_peer_match_is_a_stall() {
    let Some(content) = content() else { return };
    let mut net = Net::new(content, 0.3);
    let a = net.join_with_migration("Anna", direct());
    net.ready(&[a]);
    net.host().peer().lobby().start(7).unwrap();
    net.run_until(Duration::from_secs(10), "the match runs", |n| n.host_tick() > 300);
    let back = |e: &OnlineEvent| *e == OnlineEvent::Notice(HOST_BACK.into());
    for (k, who) in [a, 0].into_iter().enumerate() {
        net.outage(who, Duration::from_millis(2_500));
        net.run_until(Duration::from_secs(8), "Anna is back with the host", |n| {
            n.peers[a].count(back) == k + 1
                && n.peers[a].peer().match_session().is_some_and(|s| !s.suspended())
        });
    }
    assert!(net.peers[a].has(|e| *e == OnlineEvent::Migrating(MigrationStep::TakingOver)), "her turn came");
    for i in [0, a] {
        assert!(!net.peers[i].has(migrated), "{i}: {:?}", net.peers[i].events);
        assert!(!net.peers[i].has(|e| matches!(e, OnlineEvent::HostLost(_))), "{i}");
    }
    assert!(net.host().peer().is_host() && !net.peers[a].peer().is_host());
    net.run_until(Duration::from_secs(20), "the match ends and both make replays", |n| {
        [0, a].iter().all(|&i| n.peers[i].has(ended) && n.peers[i].has(replay_ready))
    });
    let rh = net.peers[0].peer().last_record().cloned().unwrap();
    let ra = net.peers[a].peer().last_record().cloned().unwrap();
    assert_eq!(rh.rows, ra.rows);
    assert_eq!(rh.checksums, ra.checksums);
}

/// Ruling FR12c-1 beside a spectator: a host, Anna playing and Cid
/// watching (both can host). Anna's 2.5 s blip is a stall: the host still
/// hears Cid, so it is not cut off, and Anna, whom nobody rejoins, goes back
/// to it. Before, the host left (it heard none of its players) and the
/// match moved.
#[test]
fn a_players_blip_beside_a_spectator_is_a_stall() {
    let Some(content) = content() else { return };
    let mut net = Net::new(content, 0.2);
    let a = net.join_with_migration("Anna", direct());
    let c = net.join_with_migration("Cid", direct());
    net.peers[c].peer().lobby().set_spectate(true);
    let c_key = net.peers[c].key;
    net.run_until(Duration::from_secs(10), "Cid watches", |n| {
        n.host().peer().lobby().state().is_some_and(|s| {
            s.members.iter().any(|m| m.key == c_key && m.seat == olr_proto::SeatWire::Spectator)
        })
    });
    net.ready(&[a]);
    net.host().peer().lobby().start(7).unwrap();
    net.run_until(Duration::from_secs(10), "the match runs", |n| n.host_tick() > 300);
    net.outage(a, Duration::from_millis(2_500));
    net.run_until(Duration::from_secs(8), "Anna is back with the host", |n| {
        n.peers[a].has(|e| *e == OnlineEvent::Notice(HOST_BACK.into()))
            && n.peers[a].peer().match_session().is_some_and(|s| !s.suspended())
    });
    for i in [0, a, c] {
        assert!(!net.peers[i].has(migrated), "{i}: {:?}", net.peers[i].events);
        assert!(!net.peers[i].has(|e| matches!(e, OnlineEvent::HostLost(_))), "{i}");
    }
    assert!(net.host().peer().is_host());
}

/// Final review B-M2: a member waiting outside the match while it runs
/// (Bert, his download of the level held by the prompt) follows the match
/// to its new host when the host leaves, as a late rejoiner does: Anna
/// takes over, and Bert is her lobby's member, waiting, instead of ending
/// with "the host closed the lobby".
#[test]
fn a_member_waiting_outside_the_match_follows_it() {
    let Some(content) = content() else { return };
    let mut net = Net::new(content, 0.3);
    let a = net.join_with_migration("Anna", direct());
    let (dir, partial) = crate::common::partial_content(&[crate::common::net::LEVEL]);
    net.dirs.push(dir);
    let b = net.join_full(
        crate::common::net::identity(),
        crate::common::net::look("Bert"),
        partial,
        None,
        None,
        direct(),
    );
    net.peers[b].peer().test_prompt_limit(1000);
    net.run_until(Duration::from_secs(10), "Bert is told what he lacks", |n| {
        n.peers[b].has(|e| matches!(e, OnlineEvent::DownloadPrompt { .. })) && n.members() == 3
    });
    net.peers[b].peer().lobby().set_spectate(true);
    let b_key = net.peers[b].key;
    net.run_until(Duration::from_secs(10), "Bert would watch", |n| {
        n.host().peer().lobby().state().is_some_and(|s| {
            s.members.iter().any(|m| m.key == b_key && m.seat == olr_proto::SeatWire::Spectator)
        })
    });
    net.ready(&[a]);
    net.host().peer().lobby().start(7).unwrap();
    net.run_until(Duration::from_secs(10), "the match runs, Bert waits", |n| n.host_tick() > 300);
    assert!(net.peers[b].peer().match_session().is_none(), "Bert waits outside the match");
    net.host().peer().leave();
    net.peers[0].frozen = true;
    net.run_until(Duration::from_secs(10), "Anna hosts and Bert follows", |n| {
        n.peers[a].has(|e| matches!(e, OnlineEvent::Migrated { me: true, .. }))
            && n.peers[b].has(|e| matches!(e, OnlineEvent::Migrated { host, me: false } if host == "Anna"))
    });
    net.run_until(Duration::from_secs(5), "Bert in Anna's lobby", |n| {
        n.peers[a].peer().lobby().state().is_some_and(|s| s.members.iter().any(|m| m.key == b_key))
    });
    assert!(!net.peers[b].peer().is_closed());
    assert!(!net.peers[b].has(|e| matches!(e, OnlineEvent::Disconnected(_) | OnlineEvent::HostLost(_))));
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
fn started3(content: olr_content::Content, minutes: f32) -> (Net, [usize; 3]) {
    started3_at(content, minutes, 200)
}

/// The same, returned once the host is past `tick`.
fn started3_at(content: olr_content::Content, minutes: f32, tick: u32) -> (Net, [usize; 3]) {
    let mut net = Net::new(content, minutes);
    let a = net.join_with_migration("Anna", direct());
    let b = net.join_with_migration("Bert", direct());
    let c = net.join_with_migration("Cid", direct());
    net.ready(&[a, b, c]);
    net.host().peer().lobby().start(7).unwrap();
    net.run_until(Duration::from_secs(10), "the match runs", |n| n.host_tick() > tick);
    (net, [a, b, c])
}

/// A host and four clients (Anna, Bert, Cid, Dan), welcomed in order, in a
/// match of `minutes`, returned once the host is past `tick`. Ruling
/// FR12d-Q: a crashed host still counts as a member, so with one more
/// player out of reach the other three are a majority of five (of four, two
/// would be none).
fn started4_at(content: olr_content::Content, minutes: f32, tick: u32) -> (Net, [usize; 4]) {
    let mut net = Net::new(content, minutes);
    let a = net.join_with_migration("Anna", direct());
    let b = net.join_with_migration("Bert", direct());
    let c = net.join_with_migration("Cid", direct());
    let d = net.join_with_migration("Dan", direct());
    net.ready(&[a, b, c, d]);
    net.host().peer().lobby().start(7).unwrap();
    net.run_until(Duration::from_secs(10), "the match runs", |n| n.host_tick() > tick);
    (net, [a, b, c, d])
}

/// D8: a successor whose window ends before anyone rejoined it closes its
/// gate, and the joins it answered fail: their peers lose contact and move
/// to the next window. Anna, the first successor, hears nobody on UDP (an
/// outage longer than everything): Bert and Cid reach her signalling
/// (contact) but no link can open. Without losing that contact they keep
/// trying Anna until the give-up (host lost); with it Bert takes over in
/// his window, Cid and Dan follow (three of five members, a quorum: ruling
/// FR12d-Q), and their records agree.
#[test]
fn an_uncommitted_successor_closes_at_its_windows_end() {
    let Some(content) = content() else { return };
    let (mut net, [a, b, c, d]) = started4_at(content, 0.15, 200);
    net.peers[0].frozen = true;
    net.outage(a, Duration::from_secs(60));
    let froze = Instant::now();
    net.run_until(Duration::from_secs(16), "Bert hosts and Cid and Dan follow", |n| {
        n.peers[b].has(|e| matches!(e, OnlineEvent::Migrated { me: true, .. }))
            && [c, d].iter().all(|&i| {
                n.peers[i].has(|e| matches!(e, OnlineEvent::Migrated { host, me: false } if host == "Bert"))
            })
    });
    // Past Anna's window (2 s of silence, then her 5 s).
    assert!(froze.elapsed() > Duration::from_secs(7), "{:?}", froze.elapsed());
    for i in [b, c, d] {
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

/// Ruling FR12d-Q on the real path: the host freezes (no `Bye`: it still
/// counts) and Cid's network goes down for good, so Anna's turn gathers
/// only Bert: two of four members, no majority. She holds his welcome and
/// never hosts; her gate closes at her window's end, Bert's turn gathers
/// Anna (two of four again), and every player ends host lost, the ruling's
/// cost (from where they stand, Cid and the host may play on). Before,
/// Bert's rejoin committed Anna at once.
#[test]
fn a_successor_without_a_quorum_never_hosts() {
    let Some(content) = content() else { return };
    let (mut net, [a, b, c]) = started3(content, 0.3);
    net.peers[0].frozen = true;
    net.outage(c, Duration::from_secs(60));
    net.run_until(Duration::from_secs(20), "every player is host lost", |n| {
        [a, b, c].iter().all(|&i| n.peers[i].has(|e| matches!(e, OnlineEvent::HostLost(_))))
    });
    for i in [a, b] {
        assert!(
            net.peers[i].has(|e| *e == OnlineEvent::Migrating(MigrationStep::TakingOver)),
            "its turn came"
        );
        assert!(!net.peers[i].has(migrated), "nobody moved: {:?}", net.peers[i].events);
    }
    assert!(
        net.peers[b].has(|e| matches!(e, OnlineEvent::Migrating(MigrationStep::Moving(n)) if n == "Anna")),
        "Bert reached Anna"
    );
}

/// Ruling FR12d-Q1 on the real path: followers count in the quorum. A
/// host, one player (Anna) and two spectators that joined mid-match (Sam,
/// Tom); the host freezes (no `Bye`: it still counts). Anna with both
/// spectators back is three of four: she takes the match over and they
/// watch on under her. Before, followers counted for nothing: one of two,
/// all host lost.
#[test]
fn a_player_and_its_spectators_carry_on_when_the_host_crashes() {
    let Some(content) = content() else { return };
    let mut net = Net::new(content, 0.2);
    let a = net.join_with_migration("Anna", direct());
    net.run_until(Duration::from_secs(10), "Anna joined", |n| n.members() == 2);
    net.ready(&[a]);
    net.host().peer().lobby().start(7).unwrap();
    net.run_until(Duration::from_secs(10), "the match runs", |n| n.host_tick() > 200);
    let watchers = [net.join_with_migration("Sam", direct()), net.join_with_migration("Tom", direct())];
    net.run_until(Duration::from_secs(10), "both watch", |n| {
        watchers.iter().all(|&i| n.peers[i].has(|e| matches!(e, OnlineEvent::StateLoaded { .. })))
    });
    net.peers[0].frozen = true;
    net.run_until(Duration::from_secs(14), "Anna hosts and both watch on", |n| {
        n.peers[a].has(|e| matches!(e, OnlineEvent::Migrated { me: true, .. }))
            && watchers.iter().all(|&i| {
                n.peers[i].has(|e| matches!(e, OnlineEvent::Migrated { host, me: false } if host == "Anna"))
            })
    });
    for i in [a, watchers[0], watchers[1]] {
        assert!(!net.peers[i].has(|e| matches!(e, OnlineEvent::HostLost(_))));
    }
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
/// contact lapses `ANSWER_OPEN_TIMEOUT` (6 s, ruling SB-1) after the
/// answer, past her window's end, so they move on in Bert's window and
/// within the give-up, not to a false host lost once their setups time
/// out past it. Dan makes Bert's quorum (ruling FR12d-Q: three of five).
#[test]
fn a_successor_dead_after_its_answer_is_left() {
    let Some(content) = content() else { return };
    let (mut net, [a, b, c, d]) = started4_at(content, 0.5, 200);
    net.peers[0].frozen = true;
    let froze = Instant::now();
    net.outage(a, Duration::from_secs(60));
    net.run_until(Duration::from_secs(5), "all try Anna", |n| {
        [b, c, d].iter().all(|&i| {
            n.peers[i].has(|e| matches!(e, OnlineEvent::Migrating(MigrationStep::Moving(n)) if n == "Anna"))
        })
    });
    // Her answers leave within this.
    let t = Instant::now();
    net.run_until(Duration::from_secs(2), "Anna answers", |_| t.elapsed() > Duration::from_millis(700));
    net.peers[a].frozen = true;
    // Bert's window opens 7 s after the freeze, but his contact with Anna
    // holds him until it lapses, `ANSWER_OPEN_TIMEOUT` after her answer.
    net.run_until(ANSWER_OPEN_TIMEOUT + Duration::from_secs(2), "Bert takes over", |n| {
        n.peers[b].has(|e| *e == OnlineEvent::Migrating(MigrationStep::TakingOver))
    });
    let lapsed = t.elapsed();
    assert!(lapsed > ANSWER_OPEN_TIMEOUT - Duration::from_secs(1), "{lapsed:?} after her answers");
    // He resumes at his window's end (Anna, expected, never comes back),
    // before anyone's give-up.
    net.run_until(Duration::from_secs(8), "Bert hosts and Cid and Dan follow", |n| {
        n.peers[b].has(|e| matches!(e, OnlineEvent::Migrated { me: true, .. }))
            && [c, d].iter().all(|&i| {
                n.peers[i].has(|e| matches!(e, OnlineEvent::Migrated { host, me: false } if host == "Bert"))
            })
    });
    assert!(froze.elapsed() < Duration::from_secs(14), "{:?}", froze.elapsed());
    for i in [b, c, d] {
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

/// N69 for a follower: Anna's and Bert's own links drop for 2.8 s (the
/// host still hears Cid, so it hosts on: ruling SB-2 ends a host only when
/// it hears none of its players) while Bert's join to Anna is open (the
/// signalling's TCP answers) but its `Hello` held (`test_delay_rejoin`).
/// The old host comes back: Bert, in contact, keeps the migration, but
/// Anna calls hers off and closes the link; losing that contact, Bert goes
/// back to the host too, instead of running through the windows to a host
/// lost.
#[test]
fn a_follower_in_contact_when_the_host_returns_goes_back_too() {
    let Some(content) = content() else { return };
    let (mut net, [a, b, c]) = started3(content, 0.2);
    net.peers[b].peer().test_delay_rejoin(Duration::from_secs(3));
    let cut = Instant::now();
    net.outage(a, Duration::from_millis(2_800));
    net.outage(b, Duration::from_millis(2_800));
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
    for i in [a, b, c] {
        assert!(!net.peers[i].has(migrated), "{i}");
    }
    assert!(!net.peers[c].has(|e| matches!(e, OnlineEvent::Migrating(_))), "Cid heard the host all along");
    assert!(net.host().peer().is_host());
    assert!(!net.host().has(|e| matches!(e, OnlineEvent::HostLost(_))));
}

/// Ruling SB-2: the host's own network drops for longer than the silence
/// limit. It hears none of its players for `HOST_SILENT_US`, as they hear
/// nothing of it: they move to Anna and play on, while the old host ends
/// its match with `CUT_OFF` and its replay instead of playing on alone
/// (it did, filling the players' slots, until their links timed out).
#[test]
fn a_host_cut_off_from_its_players_ends_its_match() {
    let Some(content) = content() else { return };
    let (mut net, [a, b]) = started(content);
    let cut = Instant::now();
    net.outage(0, Duration::from_secs(6));
    net.run_until(Duration::from_secs(5), "the old host is cut off", |n| {
        n.peers[0].has(|e| *e == OnlineEvent::HostLost(LostWhy::CutOff))
    });
    let took = cut.elapsed();
    // The last datagrams heard came just before the cut.
    let limit = Duration::from_micros(HOST_SILENT_US) - Duration::from_millis(100);
    assert!(took >= limit && took < limit + Duration::from_secs(1), "{took:?}: at the silence limit");
    assert!(net.host().peer().match_session().is_none(), "its match view is over");
    net.run_until(Duration::from_secs(10), "both resume under Anna", |n| {
        n.peers[a].has(|e| matches!(e, OnlineEvent::Migrated { me: true, .. }))
            && n.peers[b].has(|e| matches!(e, OnlineEvent::Migrated { host, me: false } if host == "Anna"))
    });
    net.run_until(Duration::from_secs(5), "the old host's replay", |n| n.peers[0].has(replay_ready));
    for i in [a, b] {
        assert!(!net.peers[i].has(|e| matches!(e, OnlineEvent::HostLost(_))), "{i}");
    }
    records_agree(&mut net, [a, b]);
}

/// N72: a member welcomed after the new host's resume is a mid-match
/// joiner. Bert keeps contact with Anna but sends his `Hello` 6 s after his
/// link opened (`test_delay_rejoin`), so Anna, committed by Cid's and
/// Dan's rejoins (three of five members, ruling FR12d-Q), resumes without
/// him at her window's end. Bert is welcomed `Waiting`, drops his old
/// session, watches from a state, and plays the next match; his slot stays
/// `Keys(0)` from the resume.
#[test]
fn a_late_rejoiner_watches_from_a_state() {
    let Some(content) = content() else { return };
    let (mut net, [a, b, c, d]) = started4_at(content, 0.1, 120);
    let bert_slot = net.peers[b].peer().my_slot().unwrap() as usize;
    let bert_key = net.peers[b].key;
    net.peers[b].peer().test_delay_rejoin(Duration::from_secs(6));
    let marks = [a, b, c].map(|i| net.peers[i].events.len());
    net.peers[0].frozen = true;
    net.run_until(Duration::from_secs(10), "Anna resumes with Cid", |n| {
        n.peers[a].has(|e| matches!(e, OnlineEvent::Migrated { me: true, .. }))
            && n.peers[c].has(|e| matches!(e, OnlineEvent::Migrated { host, me: false } if host == "Anna"))
    });
    // Anna's tick at her resume (at least `R`): Bert's slot is `Keys(0)` from it.
    let resumed = net.peers[a].peer().match_session().unwrap().current_tick();
    assert!(!net.peers[b].has(migrated), "Bert is not back yet");
    net.run_until(Duration::from_secs(8), "Bert is welcomed as a mid-match joiner and watches", |n| {
        n.peers[b].events[marks[1]..].iter().any(|e| matches!(e, OnlineEvent::StateLoaded { .. }))
    });
    let state = net.peers[b].peer().lobby().state().cloned().unwrap();
    let me = state.members.iter().find(|m| m.key == bert_key).unwrap();
    assert!(
        matches!(me.seat, olr_proto::SeatWire::Waiting),
        "welcomed waiting for the next match: {:?}",
        me.seat
    );
    assert!(net.peers[b].peer().match_session().is_some_and(|s| s.input_log().origin() > 0), "from a state");
    assert_eq!(net.peers[b].started().last(), Some(&None), "it watches");
    // The migration still counts in the match it watches (N74).
    assert_eq!(net.peers[b].peer().migrations(), Some(1));
    // The match it played up to the freeze makes its replay (N18).
    net.run_until(Duration::from_secs(5), "Bert's replay of what he played", |n| {
        n.peers[b].events[marks[1]..].iter().any(replay_ready)
    });
    net.run_until(Duration::from_secs(20), "the match ends for all three", |n| {
        [a, b, c].iter().all(|&i| n.peers[i].events[marks[0].min(marks[1])..].iter().any(ended))
            && n.peers[a].has(replay_ready)
    });
    let ra = net.peers[a].peer().last_record().cloned().unwrap();
    assert!(ra.rows.len() > resumed as usize + 100, "{} rows, resumed at {resumed}", ra.rows.len());
    assert!(ra.rows[resumed as usize..].iter().all(|row| row[bert_slot] == 0), "Bert's slot stands still");
    // Gathering again: Bert takes a seat and plays Anna's next match.
    net.run_until(Duration::from_secs(14), "Bert is seated", |n| {
        n.peers[a].peer().lobby().state().is_some_and(|s| {
            s.phase == olr_proto::Phase::Gathering
                && s.members
                    .iter()
                    .any(|m| m.key == bert_key && matches!(m.seat, olr_proto::SeatWire::Player { .. }))
        })
    });
    for i in [b, c, d] {
        net.peers[i].peer().lobby().set_ready(true);
    }
    net.run_until(Duration::from_secs(10), "Anna can start", |n| {
        n.peers[a].peer().lobby().can_start().is_ok()
    });
    net.peers[a].peer().lobby().start(9).unwrap();
    net.run_until(Duration::from_secs(10), "the next match starts for Bert", |n| {
        n.peers[b].started().last().is_some_and(Option::is_some)
    });
    assert_eq!(net.peers[b].peer().migrations(), Some(0), "a new match counts afresh");
    for i in [a, b, c] {
        assert!(!net.peers[i].has(|e| matches!(e, OnlineEvent::HostLost(_))), "{:?}", net.peers[i].events);
    }
}

/// N74: a later migration uses the new host's succession, which lists
/// everyone but itself. The host freezes and Anna takes over; 4 s after her
/// resume Anna freezes too, and the match moves to Bert, Cid following.
#[test]
fn a_second_migration_moves_the_match_again() {
    let Some(content) = content() else { return };
    let (mut net, [a, b, c]) = started3(content, 0.2);
    net.peers[0].frozen = true;
    net.run_until(Duration::from_secs(8), "the match moves to Anna", |n| {
        n.peers[a].has(|e| matches!(e, OnlineEvent::Migrated { me: true, .. }))
            && [b, c].iter().all(|&i| {
                n.peers[i].has(|e| matches!(e, OnlineEvent::Migrated { host, me: false } if host == "Anna"))
            })
    });
    let anna = net.peers[a].key;
    let order = net.peers[a].peer().lobby().succession();
    assert!(order.iter().all(|e| e.key != anna), "the host is no successor: {order:?}");
    assert_eq!(
        order.iter().map(|e| e.key).collect::<Vec<_>>(),
        vec![net.peers[b].key, net.peers[c].key],
        "Bert, then Cid"
    );
    let t = Instant::now();
    net.run_until(Duration::from_secs(6), "4 s of Anna's hosting", |_| t.elapsed() > Duration::from_secs(4));
    let mark = net.peers[c].events.len();
    net.peers[a].frozen = true;
    net.run_until(Duration::from_secs(8), "the match moves to Bert", |n| {
        n.peers[b].has(|e| matches!(e, OnlineEvent::Migrated { me: true, .. }))
            && n.peers[c].events[mark..]
                .iter()
                .any(|e| matches!(e, OnlineEvent::Migrated { host, me: false } if host == "Bert"))
    });
    assert!(
        net.peers[c].events[mark..]
            .iter()
            .any(|e| matches!(e, OnlineEvent::Migrating(MigrationStep::Moving(n)) if n == "Bert"))
    );
    assert!(net.peers[b].peer().is_host());
    records_agree(&mut net, [b, c]);
}

/// `direct()` with a pool on `relay` (local relays allowed): the relays a
/// successor listens on in its turn, and its joins' second route.
fn relayed(relay: &TestRelay) -> MigrationConfig {
    MigrationConfig {
        relays: Some(PoolConfig { relays: vec![relay.url()], ..PoolConfig::default() }),
        allow_local: true,
        ..direct()
    }
}

fn listening(e: &OnlineEvent) -> bool {
    matches!(e, OnlineEvent::ListenRelay { status, .. } if status == "connected")
}

/// A host and two clients on a `TestRelay`, Bert's joins to successors
/// through the relays only (`test_ignore_direct`), in a 0.1-minute match.
fn started_relayed(content: olr_content::Content, relay: &TestRelay) -> (Net, [usize; 2]) {
    let mut net = Net::new(content, 0.1);
    let a = net.join_with_migration("Anna", relayed(relay));
    let b = net.join_with_migration("Bert", relayed(relay));
    net.peers[b].peer().test_ignore_direct();
    net.ready(&[a, b]);
    net.host().peer().lobby().start(7).unwrap();
    net.run_until(Duration::from_secs(10), "the match runs", |n| n.host_tick() > 300);
    (net, [a, b])
}

/// N73: Bert reaches Anna, his successor, through the relays: Anna listens
/// on hers once her turn comes (not before), answers on her standby, and
/// Bert's new link is to her standby.
#[test]
fn a_successor_is_reached_through_the_relays() {
    let Some(content) = content() else { return };
    let relay = TestRelay::start();
    let (mut net, [a, b]) = started_relayed(content, &relay);
    let order = net.host().peer().lobby().succession();
    assert!(order.iter().all(|e| e.relays == [relay.url()]), "each reports its relays: {order:?}");
    let port = net.peers[a].peer().standby_port().unwrap();
    assert!(!net.peers[a].has(listening), "Anna listens only once her turn comes");
    let marks = [a, b].map(|i| net.peers[i].events.len());
    net.peers[0].frozen = true;
    let froze = Instant::now();
    net.run_until(Duration::from_secs(12), "both resume under Anna", |n| {
        n.peers[a].has(|e| matches!(e, OnlineEvent::Migrated { me: true, .. }))
            && n.peers[b].has(|e| matches!(e, OnlineEvent::Migrated { host, me: false } if host == "Anna"))
    });
    // 2 s of silence, then the relays' few seconds (a hang guard).
    assert!(froze.elapsed() < Duration::from_secs(2 + 6), "{:?}", froze.elapsed());
    let after = &net.peers[b].events[marks[1]..];
    let steps: Vec<JoinStep> = after
        .iter()
        .filter_map(|e| match e {
            OnlineEvent::Joining(s) => Some(*s),
            _ => None,
        })
        .collect();
    let at = |s: JoinStep| steps.iter().position(|&t| t == s);
    assert!(at(JoinStep::Relays).is_some_and(|r| at(JoinStep::Asking).is_some_and(|q| r < q)), "{steps:?}");
    assert!(at(JoinStep::Direct).is_none(), "{steps:?}");
    assert!(net.peers[a].events[marks[0]..].iter().any(listening), "Anna listens in her turn");
    let migrated = after.iter().position(migrated).unwrap();
    assert!(
        after[migrated..]
            .iter()
            .any(|e| matches!(e, OnlineEvent::Connected { remote, .. } if remote.port() == port)),
        "Bert's path line names his link to Anna's standby (port {port}): {:?}",
        &after[migrated..]
    );
    net.run_until(Duration::from_secs(20), "the match ends and both make replays", |n| {
        [a, b].iter().all(|&i| n.peers[i].has(ended) && n.peers[i].has(replay_ready))
    });
    let ra = net.peers[a].peer().last_record().cloned().unwrap();
    let rb = net.peers[b].peer().last_record().cloned().unwrap();
    assert_eq!(ra.rows, rb.rows);
    assert_eq!(ra.checksums, rb.checksums);
}

/// Ruling FR12c-5 (final review B-I3): each client's migration relays
/// connect at its match's start and fetch the TURN notes there, asking for
/// nothing keyed to it before its turn (N80); a migration through the
/// relays reuses those pools: Anna's listener subscribes to her handshakes
/// on her open connection in her turn, Bert asks on his. The relay sees no
/// new connection during the migration, and Bert's closes at his match's
/// end (Anna's stays: she hosts and listens there).
#[test]
fn the_migration_relays_connect_at_the_match_start() {
    let Some(content) = content() else { return };
    let relay = TestRelay::start();
    let turned = MigrationConfig { turn_from_nostr: true, ..relayed(&relay) };
    let mut net = Net::new(content, 0.1);
    let a = net.join_with_migration("Anna", turned.clone());
    let b = net.join_with_migration("Bert", turned);
    net.peers[b].peer().test_ignore_direct();
    let keys = [a, b].map(|i| hex::encode(net.peers[i].key));
    assert_eq!(relay.connections(), 0, "no pool before the match");
    net.ready(&[a, b]);
    net.host().peer().lobby().start(7).unwrap();
    let mut subs: Vec<Vec<olr_discovery::Filter>> = Vec::new();
    let keyed = |subs: &[Vec<olr_discovery::Filter>], key: &str| {
        subs.iter().flatten().any(|f| f.tags.iter().any(|(c, v)| *c == 'p' && v.iter().any(|k| k == key)))
    };
    net.run_until(Duration::from_secs(10), "both pools connect and fetch the TURN notes", |n| {
        subs.extend(relay.subscriptions().into_iter().map(|s| s.1));
        relay.connections() == 2
            && subs.iter().flatten().filter(|f| f.kinds == [olr_discovery::KIND_TURN]).count() >= 2
            && relay.subscriptions().is_empty()
            && n.host_tick() > 300
    });
    assert!(!keys.iter().any(|k| keyed(&subs, k)), "nothing keyed to a client before its turn: {subs:?}");
    net.peers[0].frozen = true;
    net.run_until(Duration::from_secs(12), "both resume under Anna", |n| {
        subs.extend(relay.subscriptions().into_iter().map(|s| s.1));
        n.peers[a].has(|e| matches!(e, OnlineEvent::Migrated { me: true, .. }))
            && n.peers[b].has(|e| matches!(e, OnlineEvent::Migrated { host, me: false } if host == "Anna"))
    });
    assert!(keyed(&subs, &keys[0]), "Anna listened in her turn");
    assert!(
        net.peers[b].has(|e| *e == OnlineEvent::Joining(JoinStep::Asking)),
        "Bert asked through the relays"
    );
    assert_eq!(relay.connections(), 2, "the migration reused both pools");
    net.run_until(Duration::from_secs(20), "the match ends and Bert's pool closes", |n| {
        [a, b].iter().all(|&i| n.peers[i].has(ended) && n.peers[i].has(replay_ready))
            && relay.open_connections() == 1
    });
}

/// Carried from Task 6: a link through the relays opens slowly on a lossy
/// path (ICE's lost checks sent again 0.25, 0.75, 1.75, 3.75 s after the
/// first, then DTLS, then olr-net's wait for the pair's stats). Cid
/// commits Anna directly; Bert's datagrams black out for 2.5 s
/// from Anna's answer through the relays (`Connecting`), so his link opens
/// only on ICE's check sent again 3.75 s after it: Bert keeps his contact
/// (`ANSWER_OPEN_TIMEOUT`) and is welcomed by Anna, never hosting. With
/// Anna committed, her window is no limit: a link opening after it is
/// welcomed all the same (late), so the timeout is what is tested.
#[test]
fn a_slow_link_through_the_relays_is_waited_for() {
    let Some(content) = content() else { return };
    let relay = TestRelay::start();
    let mut net = Net::new(content, 0.1);
    let a = net.join_with_migration("Anna", relayed(&relay));
    let b = net.join_with_migration("Bert", relayed(&relay));
    let c = net.join_with_migration("Cid", direct());
    net.peers[b].peer().test_ignore_direct();
    net.ready(&[a, b, c]);
    net.host().peer().lobby().start(7).unwrap();
    net.run_until(Duration::from_secs(10), "the match runs", |n| n.host_tick() > 200);
    let mark = net.peers[b].events.len();
    net.peers[0].frozen = true;
    net.run_until(Duration::from_secs(6), "Anna answers Bert through the relays", |n| {
        n.peers[b].events[mark..].contains(&OnlineEvent::Joining(JoinStep::Connecting))
    });
    let answered = Instant::now();
    net.outage(b, Duration::from_millis(2_500));
    net.run_until(Duration::from_secs(10), "Bert is welcomed by Anna", |n| {
        n.peers[a].has(|e| matches!(e, OnlineEvent::Migrated { me: true, .. }))
            && n.peers[b].events[mark..]
                .iter()
                .any(|e| matches!(e, OnlineEvent::Migrated { host, me: false } if host == "Anna"))
    });
    let took = answered.elapsed();
    assert!(took > Duration::from_secs(2), "the link opened {took:?} after the answer: not a slow path");
    for i in [b, c] {
        assert!(!net.peers[i].has(|e| *e == OnlineEvent::Migrating(MigrationStep::TakingOver)), "{i} hosts");
        assert!(!net.peers[i].has(|e| matches!(e, OnlineEvent::HostLost(_))));
    }
    assert!(net.peers[c].has(|e| matches!(e, OnlineEvent::Migrated { host, me: false } if host == "Anna")));
    records_agree(&mut net, [a, c]);
}

/// Ruling SB-1 (net-soak's `soak-transport-125`): a direct link runs the
/// same handshake as a relayed one (ICE's lost checks sent again 0.25,
/// 0.75, 1.75, 3.75 s after the first, DTLS, olr-net's wait for the pair's
/// stats), so on a lossy path it opens seconds after the answer too.
/// Bert's datagrams black out for 2.5 s from his attempt at Anna (no
/// relays: direct only; the signalling's TCP answers): his link opens
/// only on ICE's check sent again 3.75 s after the first. He keeps his
/// contact and is welcomed by Anna; with 2 s for a direct link he lost
/// it, could not try Anna directly again in her window, and ended host
/// lost in his own.
#[test]
fn a_slow_direct_link_is_waited_for() {
    let Some(content) = content() else { return };
    let (mut net, [a, b, c]) = started3(content, 0.2);
    let mark = net.peers[b].events.len();
    net.peers[0].frozen = true;
    net.run_until(Duration::from_secs(6), "Bert tries Anna", |n| {
        n.peers[b].events[mark..].contains(&OnlineEvent::Migrating(MigrationStep::Moving("Anna".into())))
    });
    let tried = Instant::now();
    net.outage(b, Duration::from_millis(2_500));
    net.run_until(Duration::from_secs(10), "Bert is welcomed by Anna", |n| {
        n.peers[a].has(|e| matches!(e, OnlineEvent::Migrated { me: true, .. }))
            && n.peers[b].events[mark..]
                .iter()
                .any(|e| matches!(e, OnlineEvent::Migrated { host, me: false } if host == "Anna"))
    });
    let took = tried.elapsed();
    assert!(took > Duration::from_secs(2), "the link opened {took:?} after the attempt: not a slow path");
    for i in [b, c] {
        assert!(!net.peers[i].has(|e| *e == OnlineEvent::Migrating(MigrationStep::TakingOver)), "{i} hosts");
        assert!(!net.peers[i].has(|e| matches!(e, OnlineEvent::HostLost(_))));
    }
    records_agree(&mut net, [a, b]);
}

/// N74 on the real path: the match has counted three migrations (the test
/// hook); the host freezes and Anna takes over, Bert and Cid rejoin her
/// (`take_over` and `rejoin` count the fourth); Anna freezes too, and the
/// fifth is host lost at once, with a replay.
#[test]
fn the_fifth_migration_in_a_match_is_host_lost() {
    let Some(content) = content() else { return };
    let (mut net, [a, b, c]) = started3(content, 0.2);
    for i in [a, b, c] {
        net.peers[i].peer().test_set_migrations(3);
    }
    net.peers[0].frozen = true;
    net.run_until(Duration::from_secs(8), "the fourth migration, to Anna", |n| {
        n.peers[a].has(|e| matches!(e, OnlineEvent::Migrated { me: true, .. }))
            && [b, c].iter().all(|&i| n.peers[i].has(migrated))
    });
    for i in [a, b, c] {
        assert_eq!(net.peers[i].peer().migrations(), Some(4), "peer {i}");
    }
    let marks = [b, c].map(|i| net.peers[i].events.len());
    net.peers[a].frozen = true;
    net.run_until(Duration::from_secs(6), "the fifth is host lost", |n| {
        [b, c].iter().all(|&i| n.peers[i].has(|e| *e == OnlineEvent::HostLost(LostWhy::TooManyMigrations)))
    });
    for (k, i) in [b, c].into_iter().enumerate() {
        assert!(
            !net.peers[i].events[marks[k]..].iter().any(|e| matches!(e, OnlineEvent::Migrating(_))),
            "no fifth migration: {:?}",
            &net.peers[i].events[marks[k]..]
        );
    }
    net.run_until(Duration::from_secs(5), "their replays", |n| {
        [b, c].iter().enumerate().all(|(k, &i)| n.peers[i].events[marks[k]..].iter().any(replay_ready))
    });
}

/// A fake [`Connector`] for the planner: every join counted with when, to
/// whom and how (direct, or the relays' pool size); a join started is live
/// until discarded.
#[derive(Default)]
struct Fake {
    now: u64,
    /// `(when, member, direct, relays)`.
    joins: Vec<(u64, u8, bool, usize)>,
    live: usize,
    most_live: usize,
}

impl Connector for Fake {
    type Attempt = u8;

    fn connect(&mut self, entry: &SuccessorWire, join: Join) -> Option<u8> {
        let relays = join.nostr.as_ref().map_or(0, |n| n.pool.relays.len());
        self.joins.push((self.now, entry.member, join.direct.is_some(), relays));
        self.live += 1;
        self.most_live = self.most_live.max(self.live);
        Some(entry.member)
    }

    fn discard(&mut self, _: u8) {
        self.live -= 1;
    }
}

/// How a hostile succession's relays answer a join.
#[derive(Clone, Copy, PartialEq)]
enum Relays {
    Silent,
    NotHost,
    /// The join fails at once, not reached: asked again (FR12d-Q2).
    Unreached,
}

/// Review Focus 3: a hostile succession (48 entries, each with an address
/// and 2 relays) whose every direct connect is refused and whose relays
/// never answer, answer `NotHost` at once, or fail at once, over 10 s of a
/// fake clock: one attempt at a time, at most 8 relays a join, nothing
/// past the give-up, and the host lost. Each route of an entry is asked
/// once per window, but a route not reached (ruling FR12d-Q2, amending
/// N73) once per `REASK_US` at most within its window.
#[test]
fn a_hostile_succession_is_bounded() {
    hostile_succession(Relays::Silent);
    hostile_succession(Relays::NotHost);
    hostile_succession(Relays::Unreached);
}

fn hostile_succession(relays_answer: Relays) {
    let order: Vec<SuccessorWire> = (0..48u8)
        .map(|i| SuccessorWire {
            member: i,
            key: [i + 1; 32],
            direct: Some(([10, 0, 0, i], 2000 + u16::from(i))),
            relays: vec![format!("wss://a{i}.example"), format!("wss://b{i}.example")],
            can_host: true,
        })
        .collect();
    let own = PoolConfig {
        relays: (0..10).map(|i| format!("wss://own{i}.example")).collect(),
        ..Default::default()
    };
    let cfg = MigrationConfig { relays: Some(own), region: Region::Europe, ..MigrationConfig::direct() };
    let mut plan: Planner<u8> = Planner::new(order, vec![true; 48], None, &cfg, 0);
    let mut fake = Fake::default();
    let mut lost = None;
    for t in (0..=11_000_000).step_by(10_000) {
        fake.now = t;
        if plan.step(t, &mut fake) == Step::GiveUp {
            lost.get_or_insert(t);
        }
        // Every direct connect is refused at once; the relays never answer,
        // or say `NotHost` (a key the hostile host holds).
        match plan.attempt() {
            Some((_, Route::Direct)) => assert!(!plan.failed(t, Failure::Unreached, &mut fake)),
            Some((_, Route::Relays)) if relays_answer == Relays::NotHost => {
                assert!(!plan.failed(t, Failure::NotHost, &mut fake))
            }
            Some((_, Route::Relays)) if relays_answer == Relays::Unreached => {
                assert!(!plan.failed(t, Failure::Unreached, &mut fake))
            }
            _ => {}
        }
    }
    assert_eq!(lost, Some(GIVE_UP_US), "the host is lost at the give-up");
    assert_eq!(fake.most_live, 1, "one attempt at a time");
    assert_eq!(fake.live, 0, "none left after the give-up");
    assert!(fake.joins.iter().all(|&(t, ..)| t < GIVE_UP_US), "nothing past the give-up");
    assert!(fake.joins.iter().all(|&(.., relays)| relays <= 8), "{:?}", fake.joins);
    assert!(fake.joins.iter().any(|&(.., relays)| relays == 8), "the entry's 2 and 6 of its own");
    let mut routes: BTreeMap<(u64, u8, bool), Vec<u64>> = BTreeMap::new();
    for &(t, member, is_direct, _) in &fake.joins {
        routes.entry((t / WINDOW_US, member, is_direct)).or_default().push(t);
    }
    // Two windows in 10 s: the first two entries.
    assert!(routes.keys().all(|&(w, member, _)| u64::from(member) == w), "{routes:?}");
    if relays_answer == Relays::Unreached {
        // Asked again, each route at most once per `REASK_US`, within the
        // window: 5 a route a window.
        for times in routes.values() {
            assert!(times.windows(2).all(|p| p[1] - p[0] >= REASK_US), "{routes:?}");
        }
        assert!(routes.values().all(|t| t.len() > 1), "asked again: {routes:?}");
        assert_eq!(fake.joins.len(), 2 * 2 * (WINDOW_US / REASK_US) as usize, "{:?}", fake.joins);
    } else {
        // Each once directly, then once through the relays (a silent
        // relay join is never given up in its window; `NotHost` through
        // the relays is a refusal).
        assert!(routes.values().all(|t| t.len() == 1), "{routes:?}");
        assert_eq!(fake.joins.len(), 4, "{:?}", fake.joins);
    }
}

/// The bound on the real path (127.0.0.1): Anna, the first successor, is
/// gone with her standby, so nobody answers at its port. Bert, Cid and Dan
/// each try her address once in her window: the connect waits in the
/// held port's backlog unanswered until her turn ends (an attempt under
/// way is never doubled; ruling FR12d-Q2 asks again only after one that
/// failed), then the match moves to Bert in his (three of five members,
/// ruling FR12d-Q).
///
/// The port stays ours: a free ephemeral port could be taken by another
/// test's standby (its closed gate answers `NotHost`, asked again every
/// `RETRY_US`: 19 joins instead of one), so a listener that never accepts
/// holds it.
#[test]
fn a_successor_gone_costs_one_connect_each() {
    let Some(content) = content() else { return };
    let (mut net, [a, b, c, d]) = started4_at(content, 0.3, 200);
    let [anna, bert] = [a, b].map(|i| net.peers[i].key);
    let anna = net.peers[b].member_id(anna).unwrap();
    let bert = net.peers[c].member_id(bert).unwrap();
    let mark = net.peers[c].events.len();
    let port = net.peers[a].peer().standby_port().unwrap();
    net.peers[0].frozen = true;
    drop(net.peers[a].p.take());
    let held = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port))
        .unwrap_or_else(|e| panic!("Anna's freed port {port} was taken at once: {e}"));
    // Nobody else can stand at her address now.
    let usurper = olr_net::endpoint::Endpoint::host(
        olr_net::endpoint::EndpointConfig { port, ..Default::default() },
        Box::new(|_, _| Err(olr_proto::Refusal::NotHost)),
    );
    assert!(usurper.is_err(), "the held port is bound once");
    net.run_until(Duration::from_secs(16), "Bert hosts and Cid and Dan follow", |n| {
        n.peers[b].has(|e| matches!(e, OnlineEvent::Migrated { me: true, .. }))
            && [c, d].iter().all(|&i| {
                n.peers[i].has(|e| matches!(e, OnlineEvent::Migrated { host, me: false } if host == "Bert"))
            })
    });
    assert_eq!(net.peers[b].peer().test_joins(), [(anna, true)]);
    assert_eq!(net.peers[c].peer().test_joins(), [(anna, true), (bert, true)]);
    // Each successor's join is said afresh (the HUD's and the log's line).
    let direct = |e: &&OnlineEvent| **e == OnlineEvent::Joining(JoinStep::Direct);
    assert_eq!(net.peers[c].events[mark..].iter().filter(direct).count(), 2);
    drop(held);
}

/// Ruling SK-8: a spectator joining mid-match whose host crashes before its
/// state loads has no world to follow the migration with: it finds its
/// host silent and ends with `HostLost` (the text the UI already shows),
/// instead of waiting on its join for ever. Ruling FR12d-Q1: Cleo, a
/// member of the succession, counts in Anna's quorum though she cannot
/// come back, so Dan, a spectator watching already, makes it (three of
/// five; Anna and Bert alone are two of four).
#[test]
fn a_joiner_still_loading_when_its_host_crashes_is_host_lost() {
    let Some(content) = content() else { return };
    let (mut net, [a, _]) = started(content);
    let d = net.join_with_migration("Dan", direct());
    net.run_until(Duration::from_secs(10), "Dan watches", |n| {
        n.peers[d].has(|e| matches!(e, OnlineEvent::StateLoaded { .. }))
    });
    let c = net.join_with_migration("Cleo", direct());
    net.run_until(Duration::from_secs(10), "the joiner's match starts", |n| {
        n.peers[c].has(|e| matches!(e, OnlineEvent::MatchStarting { .. }))
    });
    // Crashed before it sent the joiner its state.
    net.peers[0].frozen = true;
    let crashed = Instant::now();
    assert!(net.peers[c].peer().match_session().is_some_and(|s| s.is_joining()), "still loading");
    net.run_until(Duration::from_secs(8), "the joiner is host lost", |n| {
        n.peers[c].has(|e| *e == OnlineEvent::HostLost(LostWhy::StillJoining))
    });
    let took = crashed.elapsed();
    let silent = Duration::from_micros(HOST_SILENT_US);
    // Its silence counts from the host's last datagram, just before the crash.
    assert!(
        took + Duration::from_millis(100) >= silent && took < silent + Duration::from_secs(2),
        "{took:?}"
    );
    assert!(!net.peers[c].has(|e| matches!(e, OnlineEvent::StateLoaded { .. })));
    assert!(!net.peers[c].has(|e| matches!(e, OnlineEvent::Migrating(_))), "it does not follow");
    net.run_until(Duration::from_secs(8), "the players move on", |n| n.peers[a].has(migrated));
}

/// N86, N88, Review Focus 2: Cid leaves while the match moves from a frozen
/// host to Anna; Anna and Bert remove Cid's worm and the old host's, their
/// records and replays agree, and only Cid is said to have left (the old
/// host goes silently, ruling PF12d-3). Nobody hears Cid's `Bye` (his host
/// is the frozen one), so he still counts as a member: Dan makes Anna's
/// quorum (ruling FR12d-Q: three of five).
#[test]
fn a_player_leaving_during_a_migration_is_removed_at_the_resume() {
    let Some(content) = content() else { return };
    let mut net = Net::new(content, 0.15);
    let a = net.join_with_migration("Anna", direct());
    let b = net.join_with_migration("Bert", direct());
    let c = net.join_with_migration("Cid", direct());
    let d = net.join_with_migration("Dan", direct());
    net.ready(&[a, b, c, d]);
    net.host().peer().lobby().start(7).unwrap();
    net.run_until(Duration::from_secs(10), "the match runs", |n| n.host_tick() > 300);
    let sc = net.peers[c].started()[0].unwrap();
    net.peers[0].frozen = true;
    net.run_until(Duration::from_secs(5), "Bert finds the host gone", |n| {
        n.peers[b].has(|e| matches!(e, OnlineEvent::Migrating(_)))
    });
    net.peers[c].peer().leave();
    net.peers[c].frozen = true;
    net.run_until(Duration::from_secs(12), "both resume under Anna", |n| {
        n.peers[a].has(migrated) && n.peers[b].has(migrated)
    });
    net.run_until(Duration::from_secs(25), "the match ends and both make replays", |n| {
        [a, b].iter().all(|&i| n.peers[i].has(ended) && n.peers[i].has(replay_ready))
    });
    let ra = net.peers[a].peer().last_record().cloned().unwrap();
    let rb = net.peers[b].peer().last_record().cloned().unwrap();
    assert_eq!(ra, rb);
    let slots: Vec<u8> = ra.gone.iter().map(|g| g.1).collect();
    assert!(slots.contains(&0) && slots.contains(&sc), "the old host and Cid: {:?}", ra.gone);
    for i in [a, b] {
        assert!(!net.peers[i].has(|e| matches!(e, OnlineEvent::DesyncBundle(_))));
        // Final review B-5: once.
        assert_eq!(net.peers[i].count(|e| matches!(e, OnlineEvent::Left(x) if x == "Cid")), 1);
        assert!(
            !net.peers[i]
                .has(|e| matches!(e, OnlineEvent::Left(x) | OnlineEvent::TimedOut(x) if x == "Host")),
            "the old host goes silently (B-7: never timed out either)"
        );
    }
}
