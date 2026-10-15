//! Time sync and the lagging-player rule (spec §3.2) with the toy simulation.

use std::collections::BTreeMap;

use rl_netcode::harness::{self, HOST, MigrationPlan, Outage, PeerPlan, Scenario};
use rl_netcode::netsim::Profile;
use rl_netcode::toy::ToyFactory;
use rl_netcode::{Event, FILL_WINDOW, MAX_PREDICTION, Seat, Session, ToySim, input_delay};

fn plan(id: u32, slot: u8, profile: Profile, ppm: i32) -> PeerPlan {
    PeerPlan {
        id,
        seat: Seat::Player(slot),
        profile,
        clock_ppm: ppm,
        input_delay: input_delay(profile.ping_ms),
        frame_us: 6_944,
        join_tick: None,
        freeze: None,
    }
}

fn scenario(seed: u64, peers: Vec<PeerPlan>, ticks: u32) -> Scenario {
    Scenario {
        seed,
        slots: 2 + peers.len() as u8,
        peers,
        ticks,
        settle_ticks: 300,
        outages: vec![],
        expect_no_stalls: false,
        corrupt: vec![],
        host_freeze: None,
        exits: vec![],
        migration: None,
        profile_changes: vec![],
    }
}

fn run(sc: &Scenario) -> harness::Report {
    let mut f = ToyFactory { slots: sc.slots, seed: sc.seed };
    let r = harness::run(sc, &mut f).unwrap();
    harness::verdict(sc, &r, &mut f).unwrap();
    r
}

#[test]
fn drift_is_corrected() {
    // Clocks 0.5 % fast and slow: without adjustment they would drift 30 ticks a minute apart.
    let sc = scenario(1, vec![plan(1, 2, Profile::GOOD, 5_000), plan(2, 3, Profile::GOOD, -5_000)], 6_000);
    let r = run(&sc);
    for p in [1u32, 2] {
        assert_eq!(r.stats[&HOST].fills, 0, "nobody's input was filled");
        let m = r.stats[&p].margin.unwrap();
        assert!((0..=3).contains(&m), "peer {p} margin {m}");
    }
}

#[test]
fn players_within_the_window_never_stall() {
    for profile in [Profile::LAN, Profile::GOOD, Profile::TYPICAL, Profile::BAD] {
        for seed in 1..=10 {
            let peers = (0..4).map(|i| plan(1 + i, 2 + i as u8, profile, (i as i32 - 2) * 1_000)).collect();
            let mut sc = scenario(seed, peers, 2_000);
            sc.expect_no_stalls = true;
            run(&sc);
        }
    }
}

#[test]
fn late_inputs_lose_to_the_fill() {
    // Peer 1 loses its link for 0.5 s: the host fills its slot; every peer, peer 1 included, ends with the fill.
    let mut sc = scenario(4, vec![plan(1, 2, Profile::GOOD, 0), plan(2, 3, Profile::GOOD, 0)], 1_500);
    sc.outages.push(Outage { peer: 1, from_tick: 500, until_tick: 550, one_way: false });
    let r = run(&sc);
    assert!(r.stats[&HOST].fills > 0);
    assert!(r.events[&1].iter().any(|e| matches!(e, Event::Late { .. })));
    assert!(r.stats[&1].own_corrections > 0);
    assert_eq!(r.stats[&2].stalls, 0, "the other player never waited");
}

#[test]
fn a_frozen_client_catches_up() {
    let mut p = plan(1, 2, Profile::GOOD, 0);
    p.freeze = Some((600, 1_000)); // 4 s without a single frame
    let sc = scenario(6, vec![p, plan(2, 3, Profile::GOOD, 0)], 2_000);
    let r = run(&sc);
    assert!(r.stats[&1].jumps >= 1);
    assert_eq!(r.stats[&2].stalls, 0);
    assert!(r.events[&HOST].iter().any(|e| matches!(e, Event::PeerLate { peer: 1 })));
}

#[test]
fn a_silent_peer_times_out() {
    let mut sc = scenario(8, vec![plan(1, 2, Profile::GOOD, 0)], 2_000);
    sc.outages.push(Outage { peer: 1, from_tick: 300, until_tick: 1_400, one_way: false });
    let mut f = ToyFactory { slots: sc.slots, seed: sc.seed };
    let r = harness::run(&sc, &mut f).unwrap();
    assert!(r.events[&HOST].iter().any(|e| matches!(e, Event::PeerTimedOut { peer: 1 })));
    assert!(r.events[&1].iter().any(|e| matches!(e, Event::PeerTimedOut { peer: HOST })));
}

#[test]
fn a_frozen_host_stalls_its_clients_who_recover() {
    // The host stops for 2 s (a hitch, a suspended laptop): no frames, its
    // clock stands still, arrivals wait. Its clients stall at the
    // prediction limit, then play on with it.
    let sc = Scenario {
        host_freeze: Some((1_000, 1_200)),
        ..scenario(7, vec![plan(1, 2, Profile::GOOD, 0), plan(2, 3, Profile::TYPICAL, 0)], 3_000)
    };
    let r = run(&sc);
    assert!(r.dropped.is_empty(), "dropped {:?}", r.dropped);
    for p in [1u32, 2] {
        let stalls: Vec<u32> = r.events[&p]
            .iter()
            .filter_map(|e| match e {
                Event::Stalled { tick } => Some(*tick),
                _ => None,
            })
            .collect();
        assert!(!stalls.is_empty(), "peer {p} waited for the host");
        let last = *stalls.last().unwrap();
        assert!(last < 1_600, "peer {p} still stalled at tick {last}: {stalls:?}");
    }
}

/// Each stall episode of `peer`: the host's tick when it stalled and when
/// it ran a tick again (`None`: never), as rl-cli's `StallTracker` sees it.
#[derive(Default)]
struct Episodes {
    peer: u32,
    host_tick: u32,
    open: Option<(u32, u32)>,
    done: Vec<(u32, Option<u32>)>,
}

impl harness::Observer<rl_netcode::ToySim> for Episodes {
    fn on_event(&mut self, peer: u32, e: &Event, s: &mut rl_netcode::Session<rl_netcode::ToySim>) {
        if peer == self.peer
            && let Event::Stalled { .. } = e
        {
            self.open = Some((self.host_tick, s.current_tick()));
        }
    }

    fn on_frame(&mut self, peer: u32, s: &mut rl_netcode::Session<rl_netcode::ToySim>) {
        if peer == HOST {
            self.host_tick = s.current_tick();
        } else if peer == self.peer
            && let Some((from, at)) = self.open
            && s.current_tick() > at
        {
            self.done.push((from, Some(self.host_tick)));
            self.open = None;
        }
    }
}

/// Ruling SK-2: a player's link that drops for 0.6-4.6 s stalls it at the
/// prediction limit; once the link is back it plays on within
/// `MAX_PREDICTION` ticks of the host. Two defects stalled it again after:
/// - the inputs it ran while catching up reached the host late by its
///   deficit, and the first margin after the catch-up jumped its pace ahead
///   by it, into the prediction limit (lan, 63 ticks after a 4.65 s outage);
/// - the host sent what it lacked one `MAX_FRAMES` datagram per round trip
///   (`resend`), so a backlog of `n` frames took `n / 64` round trips
///   (typical, 30 ticks after a 2.15 s outage; 2.3 s after a 6 s one on
///   bad).
#[test]
fn a_player_cut_off_plays_on_once_its_link_is_back() {
    let mut late = Vec::new();
    for profile in [Profile::LAN, Profile::GOOD, Profile::TYPICAL, Profile::BAD] {
        for seed in 1..=12u64 {
            let len = 60 + (seed as u32 * 77) % 400;
            let (from, until) = (600, 600 + len);
            let ppm = [0, 500, -2_000][seed as usize % 3];
            let mut sc =
                scenario(seed, vec![plan(1, 2, profile, ppm), plan(2, 3, profile, -ppm)], until + 600);
            sc.outages.push(Outage { peer: 2, from_tick: from, until_tick: until, one_way: false });
            let mut f = ToyFactory { slots: sc.slots, seed };
            let mut obs = Episodes { peer: 2, ..Episodes::default() };
            let r = harness::run_observed(&sc, &mut f, &mut obs).unwrap();
            harness::verdict(&sc, &r, &mut f).unwrap();
            if let Some(open) = obs.open {
                obs.done.push((open.0, None));
            }
            let limit = until
                + rl_netcode::MAX_PREDICTION.max(profile.outage_recovery_ticks(len, sc.peers[1].frame_us));
            for (start, end) in obs.done {
                if start + rl_netcode::MAX_PREDICTION < from || end.is_none_or(|e| e > limit) {
                    late.push((profile.name, seed, len, start, end, limit));
                }
            }
        }
    }
    assert!(late.is_empty(), "stalls past the outage (profile, seed, ticks, from, until, limit): {late:?}");
}

/// Every player's stall episodes, as rl-cli's `StallTracker` sees them
/// (the host's tick when each began and when the player ran a tick
/// again), and the host's last tick filling one slot.
#[derive(Default)]
struct Watch {
    host_tick: u32,
    open: BTreeMap<u32, (u32, u32)>,
    done: Vec<(u32, u32, Option<u32>)>,
    slot: u8,
    last_fill: u32,
}

impl harness::Observer<ToySim> for Watch {
    fn on_event(&mut self, peer: u32, e: &Event, s: &mut Session<ToySim>) {
        if let Event::Stalled { .. } = e {
            self.open.insert(peer, (self.host_tick, s.current_tick()));
        }
    }

    fn on_frame(&mut self, peer: u32, s: &mut Session<ToySim>) {
        if peer == HOST {
            self.host_tick = s.current_tick();
            if s.current_tick().saturating_sub(s.input_log().known_upto(self.slot)) >= FILL_WINDOW {
                self.last_fill = s.current_tick();
            }
        }
        if let Some(&(from, at)) = self.open.get(&peer)
            && s.current_tick() > at
        {
            self.done.push((peer, from, Some(self.host_tick)));
            self.open.remove(&peer);
        }
    }
}

/// Ruling SK-4 (net-soak's `bad-13s4c1v-seed2917608641`): with migration
/// on, a player cut off for over `HOST_SILENT_US` finds its host silent and
/// suspends; its link back, it hears the host again and unsuspends (N69).
/// The host ran on meanwhile, so the player catches up the pause, as one
/// cut off without migration does, and the host stops filling its slot
/// within the outage's recovery (`Profile::outage_recovery_ticks`).
/// `unsuspend` restarted the pace at the player's own stale tick instead:
/// it played on seconds behind until a fresh median of margins jumped it,
/// its inputs reaching the host that late, and on `bad` every other player
/// stalled on the host's fills past the recovery (5 clients, 2.3-3.5 s
/// outages, drifting clocks: the fills ended 13-21 ticks past it).
#[test]
fn a_player_back_from_its_hosts_silence_catches_up_the_pause() {
    let mut late = Vec::new();
    for seed in 1..=6u64 {
        let len = 230 + (seed as u32 * 37) % 120;
        let (from, until) = (600, 600 + len);
        let peers = (0..5)
            .map(|i| plan(1 + i, 2 + i as u8, Profile::BAD, [2_000, -2_000, 500][i as usize % 3]))
            .collect();
        let mut sc = scenario(seed, peers, until + 600);
        sc.migration = Some(MigrationPlan::default());
        sc.outages.push(Outage { peer: 3, from_tick: from, until_tick: until, one_way: false });
        let mut f = ToyFactory { slots: sc.slots, seed };
        let mut obs = Watch { slot: 4, ..Watch::default() };
        let r = harness::run_observed(&sc, &mut f, &mut obs).unwrap();
        harness::verdict(&sc, &r, &mut f).unwrap();
        assert_eq!(r.hosts, vec![HOST], "seed {seed}: the match stayed with its host");
        assert!(
            r.events[&3].iter().any(|e| matches!(e, Event::HostSilent)),
            "seed {seed}: the cut-off player found its host silent"
        );
        let limit = until + MAX_PREDICTION.max(Profile::BAD.outage_recovery_ticks(len, sc.peers[2].frame_us));
        if obs.last_fill > limit {
            late.push(format!(
                "seed {seed}: the host filled peer 3's slot until {} (limit {limit})",
                obs.last_fill
            ));
        }
        for (&peer, &(start, _)) in &obs.open {
            obs.done.push((peer, start, None));
        }
        for &(peer, start, end) in &obs.done {
            if start + MAX_PREDICTION >= from && start <= limit && end.is_none_or(|e| e > limit) {
                late.push(format!(
                    "seed {seed}: peer {peer} stalled from {start} until {end:?} (limit {limit})"
                ));
            }
        }
    }
    assert!(late.is_empty(), "past the outage's recovery: {late:#?}");
}
