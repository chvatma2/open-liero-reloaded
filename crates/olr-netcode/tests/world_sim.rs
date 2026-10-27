//! The real world under rollback: snapshots across terrain edits, state
//! transfer, the harness with bots-free scripted inputs (MW 1.0 on Castle
//! Strike) and colour maps that follow rollbacks (MW 1.0 on Dirt Level);
//! skipped with a notice without `OLR_CONTENT_DIR`.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use olr_content::{Content, MatchOptions, Player};
use olr_netcode::harness::{self, Factory, Observer, PeerPlan, Scenario};
use olr_netcode::netsim::Profile;
use olr_netcode::world_sim::{ConfirmedTick, TickRecorder, TickedTick};
use olr_netcode::{Driver, PeerId, Seat, Session, Simulation, TickInfo, WorldSim, input_delay};
use olr_sim::Keys;
use olr_terrain::{Area, ColorEdit, ColorMap};

fn content() -> Option<Content> {
    let Some(dir) = std::env::var("OLR_CONTENT_DIR").ok().filter(|d| std::path::Path::new(d).is_dir()) else {
        eprintln!("OLR_CONTENT_DIR not set or missing (run `cargo xtask fetch-olx-content`); skipping");
        return None;
    };
    Some(Content::open(std::path::Path::new(&dir)).expect("the content directory"))
}

/// MW 1.0 on Castle Strike: heavy carving. Built with the bots' loadouts,
/// then driven with scripted keys.
fn options(seed: u32) -> MatchOptions {
    MatchOptions::deathmatch(
        "olx-mw-1-0",
        "olx-level-castlestrike",
        seed,
        (0..8).map(|_| Player::Bot(Default::default())).collect(),
    )
}

fn world(c: &Content, seed: u32) -> olr_sim::World {
    c.build_match(&options(seed)).unwrap().world
}

/// MW 1.0 on Dirt Level, which digging and explosions carve (Castle
/// Strike's terrain is rock and air).
const DIRT_LEVEL: &str = "olx-level-dirt-level";

fn dirt_world(c: &Content, seed: u32) -> olr_sim::World {
    c.build_match(&MatchOptions { level_id: DIRT_LEVEL.into(), ..options(seed) }).unwrap().world
}

fn keys(t: u32) -> Vec<Keys> {
    (0..8u32)
        .map(|i| {
            Keys(if (t / 9 + i).is_multiple_of(4) { Keys::FIRE | Keys::RIGHT } else { Keys::FIRE | Keys::UP })
        })
        .collect()
}

#[test]
fn restore_undoes_terrain_edits_in_a_real_match() {
    let Some(c) = content() else { return };
    let mut a = WorldSim::new(world(&c, 1));
    let mut b = WorldSim::new(world(&c, 1));
    for t in 0..200 {
        a.tick(&keys(t));
        b.tick(&keys(t));
    }
    let s = a.save();
    for t in 200..400 {
        a.tick(&keys(t + 7)); // a different future
    }
    a.restore(&s);
    for t in 200..400 {
        a.tick(&keys(t));
        b.tick(&keys(t));
    }
    assert_eq!(a.world().checksum(), b.world().checksum());
    assert_eq!(a.world(), b.world());
}

/// The presentation learns what a restore put back.
#[test]
fn restore_reports_the_areas_it_put_back_in_a_real_match() {
    let Some(c) = content() else { return };
    let mut a = WorldSim::new(dirt_world(&c, 1));
    for t in 0..200 {
        a.tick(&keys(t));
    }
    let s = a.save();
    for t in 200..400 {
        a.tick(&keys(t + 7));
    }
    let carved = a.world().terrain().clone();
    assert!(a.take_restored_areas().is_empty(), "ticks restore nothing");
    a.restore(&s);
    let areas = a.take_restored_areas();
    let covered = |x: i64, y: i64| {
        areas
            .iter()
            .any(|r| (r.x0 as i64..r.x1 as i64).contains(&x) && (r.y0 as i64..r.y1 as i64).contains(&y))
    };
    let t = a.world().terrain();
    let (w, h) = (t.width() as i64, t.height() as i64);
    let mut put_back = 0;
    for (x, y) in (0..h).flat_map(|y| (0..w).map(move |x| (x, y))) {
        if carved.material(x, y) != t.material(x, y) {
            assert!(covered(x, y), "({x}, {y}) was put back unreported");
            put_back += 1;
        }
    }
    assert!(put_back > 0, "the other future carved");
}

#[test]
fn state_transfer_in_a_real_match() {
    let Some(c) = content() else { return };
    let mut a = WorldSim::new(world(&c, 2));
    for t in 0..300 {
        a.tick(&keys(t));
    }
    let mut b = WorldSim::new(world(&c, 2));
    b.decode_state(&a.encode_state()).unwrap();
    assert_eq!(a.checksum(), b.checksum());
    let (w, h) = (b.world().terrain().width() as i32, b.world().terrain().height() as i32);
    assert_eq!(
        b.take_restored_areas(),
        [Area { x0: 0, y0: 0, x1: w, y1: h }],
        "a load replaces the whole map"
    );
    for t in 300..500 {
        a.tick(&keys(t));
        b.tick(&keys(t));
    }
    assert_eq!(a.world().checksum(), b.world().checksum());
    assert_eq!(b.decode_state(&[0, 1, 2]).map_err(|_| ()), Err(()));
}

/// Issue #86: a live level's world (`fixtures/live/levels/live-test` over
/// the imported content) transfers its copies' state with the rest.
#[test]
fn state_transfer_of_a_live_world() {
    let Some(dir) = std::env::var("OLR_CONTENT_DIR").ok().filter(|d| std::path::Path::new(d).is_dir()) else {
        eprintln!("OLR_CONTENT_DIR not set or missing; skipping");
        return;
    };
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/live/levels");
    let c = Content::open_layers(&[std::path::Path::new(&dir), &fixtures]).unwrap();
    let live_world = || {
        let o = MatchOptions { level_id: "olr-level-live-test".into(), ..options(4) };
        c.build_match(&o).unwrap().world
    };
    let mut src = live_world();
    assert_eq!(src.live().map(|l| l.len()), Some(6), "the fixture's live copies");
    src.set_live_state(1, 1).unwrap();
    src.set_live_value(3, 0, 5 * olr_sim::LIVE_SUBPIXELS + 3).unwrap();
    let mut a = WorldSim::new(src);
    for t in 0..200 {
        a.tick(&keys(t));
    }
    let mut b = WorldSim::new(live_world());
    assert_ne!(a.world().sync_checksum(), b.world().sync_checksum());
    b.decode_state(&a.encode_state()).unwrap();
    assert_eq!(a.world().sync_checksum(), b.world().sync_checksum());
    assert_eq!(a.world(), b.world());
    let console = b.world().live().unwrap().copy(1).unwrap();
    assert_eq!(console.state_name(), "Alarm");
}

/// Every peer builds the same world; drivers press scripted keys.
struct Worlds<'a> {
    content: &'a Content,
}

struct Scripted;

impl Driver<WorldSim> for Scripted {
    fn local_inputs(&mut self, _sim: &WorldSim, tick: u32, slots: &[u8], out: &mut [Keys]) {
        let all = keys(tick);
        for (&s, k) in slots.iter().zip(out.iter_mut()) {
            *k = all[s as usize];
        }
    }
}

impl Factory<WorldSim> for Worlds<'_> {
    fn sim(&mut self, _peer: PeerId) -> WorldSim {
        WorldSim::new(world(self.content, 3))
    }

    fn driver(&mut self, _peer: PeerId, _seat: Option<Seat>) -> Box<dyn Driver<WorldSim>> {
        Box::new(Scripted)
    }
}

#[test]
fn the_world_plays_over_the_network_with_a_late_spectator() {
    let Some(c) = content() else { return };
    let plan = |id: u32, seat, join_tick| PeerPlan {
        id,
        seat,
        profile: Profile::TYPICAL,
        clock_ppm: 0,
        input_delay: input_delay(Profile::TYPICAL.ping_ms),
        frame_us: 6_944,
        join_tick,
        freeze: None,
    };
    let peers = vec![
        plan(1, Seat::Player(6), None),
        plan(2, Seat::Player(7), None),
        plan(3, Seat::Spectator, Some(200)),
    ];
    let sc = Scenario {
        seed: 3,
        slots: 8,
        peers,
        ticks: 400,
        settle_ticks: 100,
        outages: vec![],
        expect_no_stalls: true,
        corrupt: vec![],
        host_freeze: None,
        exits: vec![],
        migration: None,
        profile_changes: vec![],
    };
    let mut f = Worlds { content: &c };
    let r = harness::run(&sc, &mut f).unwrap();
    harness::verdict(&sc, &r, &mut f).unwrap();
    assert!(r.stats[&1].rollbacks > 0);
    assert_eq!(r.stats[&3].states_loaded, 1);
}

/// What a peer's presentation does with the terrain: the edits of every
/// tick simulated (re-simulated ones included), in order, since its last
/// frame.
#[derive(Default)]
struct Painted {
    edits: BTreeMap<PeerId, Vec<Paint>>,
}

enum Paint {
    Edit(ColorEdit),
    Lost(Area),
}

/// [`Scripted`], keeping each simulated tick's terrain edits for the
/// presentation.
struct Painter {
    peer: PeerId,
    painted: Rc<RefCell<Painted>>,
}

impl Driver<WorldSim> for Painter {
    fn local_inputs(&mut self, sim: &WorldSim, tick: u32, slots: &[u8], out: &mut [Keys]) {
        Scripted.local_inputs(sim, tick, slots, out);
    }

    fn simulated(&mut self, sim: &WorldSim, _info: TickInfo) {
        let mut p = self.painted.borrow_mut();
        let edits = p.edits.entry(self.peer).or_default();
        let events = sim.events();
        edits.extend(events.iter().filter_map(|e| olr_content::color_edit(&e.kind)).map(Paint::Edit));
        if let Some(a) = events.lost_area() {
            edits.push(Paint::Lost(olr_content::lost_area(a)));
        }
    }
}

struct PaintingWorlds<'a> {
    content: &'a Content,
    painted: Rc<RefCell<Painted>>,
}

impl Factory<WorldSim> for PaintingWorlds<'_> {
    fn sim(&mut self, _peer: PeerId) -> WorldSim {
        WorldSim::new(dirt_world(self.content, 3))
    }

    fn driver(&mut self, peer: PeerId, _seat: Option<Seat>) -> Box<dyn Driver<WorldSim>> {
        Box::new(Painter { peer, painted: self.painted.clone() })
    }
}

/// Every peer's colour map, kept as plan 10's game keeps it: after each
/// frame, the areas the session's restores and loads put back are rebuilt
/// from the terrain, then the frame's edits are applied in order.
struct Presentations {
    blank: ColorMap,
    maps: BTreeMap<PeerId, ColorMap>,
    painted: Rc<RefCell<Painted>>,
    rebuilt: usize,
}

impl Observer<WorldSim> for Presentations {
    fn on_frame(&mut self, peer: PeerId, session: &mut Session<WorldSim>) {
        let map = self.maps.entry(peer).or_insert_with(|| self.blank.clone());
        let areas = session.take_restored_areas();
        let terrain = session.sim().world().terrain();
        for &a in &areas {
            map.rebuild_area(a, terrain);
        }
        self.rebuilt += areas.len();
        for p in self.painted.borrow_mut().edits.remove(&peer).unwrap_or_default() {
            match p {
                Paint::Edit(e) => map.apply(e),
                Paint::Lost(a) => map.rebuild_area(a, terrain),
            }
        }
        let _ = map.take_dirty().count();
        // Before its first tick (a joining spectator's: before its state),
        // the setup's spawn holes are not reported yet.
        assert!(
            session.current_tick() == 0 || map.material() == terrain,
            "peer {peer}, tick {}: the colour map shows terrain the world no longer has",
            session.current_tick()
        );
    }
}

/// Rollbacks undo mispredicted carving; the colour maps follow, and a late
/// spectator's follows its loaded state.
#[test]
fn colour_maps_follow_rollbacks_and_loads() {
    let Some(c) = content() else { return };
    let level = c.load_level(DIRT_LEVEL).unwrap();
    let theme = c.load_theme(&level.theme).unwrap();
    let plan = |id: u32, seat, join_tick| PeerPlan {
        id,
        seat,
        profile: Profile::BAD,
        clock_ppm: 0,
        input_delay: input_delay(Profile::BAD.ping_ms),
        frame_us: 6_944,
        join_tick,
        freeze: None,
    };
    let sc = Scenario {
        seed: 5,
        slots: 8,
        peers: vec![
            plan(1, Seat::Player(5), None),
            plan(2, Seat::Player(6), None),
            plan(3, Seat::Player(7), None),
            plan(4, Seat::Spectator, Some(150)),
        ],
        ticks: 400,
        settle_ticks: 100,
        outages: vec![],
        expect_no_stalls: false,
        corrupt: vec![],
        host_freeze: None,
        exits: vec![],
        migration: None,
        profile_changes: vec![],
    };
    let painted = Rc::new(RefCell::new(Painted::default()));
    let mut f = PaintingWorlds { content: &c, painted: painted.clone() };
    let mut obs =
        Presentations { blank: level.color_map(&theme), maps: BTreeMap::new(), painted, rebuilt: 0 };
    let r = harness::run_observed(&sc, &mut f, &mut obs).unwrap();
    assert!(r.stats[&1].rollbacks > 0);
    assert_eq!(r.stats[&4].states_loaded, 1);
    assert!(obs.rebuilt > 1, "areas were put back: {}", obs.rebuilt);
    eprintln!("{} areas rebuilt", obs.rebuilt);
}

/// Every peer's tick records, taken after each frame as plan 10's game takes them.
#[derive(Default)]
struct Records {
    recorders: BTreeMap<PeerId, TickRecorder>,
    ticked: BTreeMap<PeerId, Vec<TickedTick>>,
    confirmed: BTreeMap<PeerId, Vec<ConfirmedTick>>,
}

/// [`Scripted`], feeding its peer's [`TickRecorder`].
struct Recording {
    peer: PeerId,
    records: Rc<RefCell<Records>>,
}

impl Driver<WorldSim> for Recording {
    fn local_inputs(&mut self, sim: &WorldSim, tick: u32, slots: &[u8], out: &mut [Keys]) {
        Scripted.local_inputs(sim, tick, slots, out);
    }

    fn simulated(&mut self, sim: &WorldSim, info: TickInfo) {
        self.records.borrow_mut().recorders.entry(self.peer).or_default().simulated(sim, info);
    }
}

struct RecordingWorlds<'a> {
    content: &'a Content,
    records: Rc<RefCell<Records>>,
}

impl Factory<WorldSim> for RecordingWorlds<'_> {
    fn sim(&mut self, _peer: PeerId) -> WorldSim {
        WorldSim::new(dirt_world(self.content, 3))
    }

    fn driver(&mut self, peer: PeerId, _seat: Option<Seat>) -> Box<dyn Driver<WorldSim>> {
        Box::new(Recording { peer, records: self.records.clone() })
    }
}

struct Taker(Rc<RefCell<Records>>);

impl Observer<WorldSim> for Taker {
    fn on_frame(&mut self, peer: PeerId, session: &mut Session<WorldSim>) {
        let mut r = self.0.borrow_mut();
        let confirmed = session.confirmed();
        let rec = r.recorders.entry(peer).or_default();
        let (ticked, done) = (rec.take_ticked(), rec.take_confirmed(confirmed));
        assert!(done.iter().all(|t| t.tick < confirmed), "peer {peer}: a record at or past {confirmed}");
        r.ticked.entry(peer).or_default().extend(ticked);
        r.confirmed.entry(peer).or_default().extend(done);
    }
}

/// The records a peer that simulated only final inputs would make: the
/// host's confirmed inputs replayed offline.
fn final_timeline(c: &Content, host_log: &[[u16; 32]], ticks: u32) -> Vec<ConfirmedTick> {
    let mut sim = WorldSim::new(dirt_world(c, 3));
    let mut rec = TickRecorder::default();
    for t in 0..ticks {
        let keys: Vec<Keys> = host_log[t as usize][..8].iter().map(|&k| Keys(k)).collect();
        sim.tick(&keys);
        rec.simulated(&sim, TickInfo { tick: t, resim: false, all_final: true });
    }
    assert_eq!(rec.take_ticked().len(), ticks as usize);
    rec.take_confirmed(ticks)
}

/// Confirmed records come out once per tick, in order, only below the
/// confirmed tick, and are the final timeline's however often a tick was
/// simulated; the ticked records are every simulation, in order.
#[test]
fn tick_records_follow_rollbacks() {
    let Some(c) = content() else { return };
    let plan = |id: u32, seat| PeerPlan {
        id,
        seat,
        profile: Profile::BAD,
        clock_ppm: 0,
        input_delay: input_delay(Profile::BAD.ping_ms),
        frame_us: 6_944,
        join_tick: None,
        freeze: None,
    };
    let sc = Scenario {
        seed: 6,
        slots: 8,
        peers: vec![
            plan(1, Seat::Player(5)),
            plan(2, Seat::Player(6)),
            plan(3, Seat::Player(7)),
            plan(4, Seat::Spectator),
        ],
        ticks: 500,
        settle_ticks: 100,
        outages: vec![],
        expect_no_stalls: false,
        corrupt: vec![],
        host_freeze: None,
        exits: vec![],
        migration: None,
        profile_changes: vec![],
    };
    let records = Rc::new(RefCell::new(Records::default()));
    let mut f = RecordingWorlds { content: &c, records: records.clone() };
    let mut obs = Taker(records.clone());
    let r = harness::run_observed(&sc, &mut f, &mut obs).unwrap();
    harness::verdict(&sc, &r, &mut f).unwrap();
    assert!(r.stats[&1].rollbacks > 0);
    let want = final_timeline(&c, &r.host_log, sc.ticks);
    assert!(want.iter().any(|t| !t.events.is_empty()), "something happened");
    let records = records.borrow();
    for peer in [0u32, 1, 2, 3, 4] {
        let got = &records.confirmed[&peer];
        assert!(got.len() >= sc.ticks as usize, "peer {peer}: {} records", got.len());
        for (i, t) in got.iter().enumerate() {
            assert_eq!(t.tick, i as u32, "peer {peer}: once per tick, in order");
        }
        for (g, w) in got.iter().zip(&want) {
            assert_eq!(g, w, "peer {peer}, tick {}", w.tick);
        }
        // Every new tick once, re-simulations after it, in simulation order.
        let ticked = &records.ticked[&peer];
        let new: Vec<u32> = ticked.iter().filter(|t| !t.resim).map(|t| t.tick).collect();
        assert!(new.iter().enumerate().all(|(i, &t)| t == i as u32), "peer {peer}: new ticks in order");
        let resims = ticked.len() - new.len();
        assert_eq!(resims as u64, r.stats[&peer].resim_ticks, "peer {peer}");
        assert!(ticked.iter().all(|t| t.events.iter().all(|e| e.substep <= t.substeps)));
    }
    assert!(records.ticked[&1].iter().any(|t| t.resim));
}

/// After a loaded state, the records restart at it: the old timeline's
/// candidates up to its tick are dropped, and nothing up to it comes out.
#[test]
fn a_loaded_state_restarts_the_tick_records() {
    let Some(c) = content() else { return };
    let mut sim = WorldSim::new(dirt_world(&c, 1));
    let mut rec = TickRecorder::default();
    let info = |tick, resim| TickInfo { tick, resim, all_final: false };
    for t in 0..20 {
        sim.tick(&keys(t));
        rec.simulated(&sim, info(t, false));
    }
    let first: Vec<u32> = rec.take_confirmed(5).iter().map(|t| t.tick).collect();
    assert_eq!(first, vec![0, 1, 2, 3, 4]);
    // The state after tick 14 loads; the ticks after it run again.
    rec.state_loaded(14);
    for t in 15..20 {
        sim.tick(&keys(t));
        rec.simulated(&sim, info(t, true));
    }
    rec.simulated(&sim, info(10, true)); // before the state: not recorded
    let after: Vec<u32> = rec.take_confirmed(30).iter().map(|t| t.tick).collect();
    assert_eq!(after, vec![15, 16, 17, 18, 19], "a gap from 5 to 14");
}

/// A spectator simulates the confirmed ticks after their inputs arrive:
/// `confirmed()` can be ahead of its simulation, and those ticks still get
/// their records once simulated.
#[test]
fn ticks_confirmed_before_they_are_simulated_are_recorded() {
    let Some(c) = content() else { return };
    let mut sim = WorldSim::new(dirt_world(&c, 1));
    let mut rec = TickRecorder::default();
    let info = |tick| TickInfo { tick, resim: false, all_final: true };
    for t in 0..5 {
        sim.tick(&keys(t));
        rec.simulated(&sim, info(t));
    }
    let first: Vec<u32> = rec.take_confirmed(8).iter().map(|t| t.tick).collect();
    assert_eq!(first, vec![0, 1, 2, 3, 4]);
    for t in 5..8 {
        sim.tick(&keys(t));
        rec.simulated(&sim, info(t));
    }
    let later: Vec<u32> = rec.take_confirmed(8).iter().map(|t| t.tick).collect();
    assert_eq!(later, vec![5, 6, 7], "simulated after they were confirmed");
}
