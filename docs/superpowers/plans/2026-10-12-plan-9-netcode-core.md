# Plan 9: Netcode Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The rollback netcode of the networking spec (§3, without host migration and presentation), proven by an in-process simulated network: whole online matches with bots over bad networks run deterministically from a seed, every failure reproducible with one command.

**Architecture:** The simulation gains cheap snapshots (a terrain undo journal), a fast sync checksum and a validated world-state transfer form. Two new `no_std` crates sit above it: `rl-proto` (the wire format) and `rl-netcode` (a sans-IO rollback session, generic over a `Simulation` trait, plus a deterministic network model). `rl-cli` gets a World-backed harness (`net-sim`), desync bundles (`net-repro`) and feel numbers; a new `rl-detcheck` tool compares checksum streams on Linux, WASM (WASI under Node.js) and Windows.

**Tech Stack:** Rust 1.99 (edition 2024), `no_std` + `alloc`, serde (no_std derive) + postcard (exact pin) + miniz_oxide (exact pin), proptest, divan, clap, Node.js `node:wasi` for the WASM leg, the plan 8 Windows interop runner.

**Spec:** `docs/superpowers/specs/2026-10-12-networking-design.md` (approved 2026-10-12). This plan covers spec §11's plan 9 row: §3.1-§3.3, §3.5, §3.6 (bots on the host and spectators; the session's input log is what plan 10 records as the replay), §2.2 (the fingerprint), §9.1-§9.3, §9.5, §9.6. Host migration (§3.7) is plan 12; presentation during rollback (§3.4) is plan 10, where the game consumes the session.

## Global Constraints

- The simulation's behaviour must not change: `fixtures/oracle` traces, `fixtures/matches` goldens and `fixtures/render` goldens stay byte-identical; `World::checksum` keeps its exact value; `World::tick` stays allocation-free when rollback is not enabled (`crates/rl-sim/tests/no_alloc.rs`).
- Simulation-layer crates stay `#![no_std]` and build for `wasm32-unknown-unknown`; `rl-proto` and `rl-netcode` are layer 4, `#![no_std]` + `alloc`, in the `no_std` and `wasm` lists of `xtask/layers.ron`; `same_layer` gets `("rl-netcode", "rl-proto")`.
- Never panic on network input or on a received world state (spec §5.4); every decoder is fuzzed with proptest (`cases: 256, failure_persistence: None`, the repo convention).
- Deterministic output: no `HashMap` iteration, no wall clock in logic; the session takes time as an argument (`now_us: u64`).
- 100 ticks per second (`TICK_US = 10_000`); one `Keys` byte per worm per tick; at most 32 slots (`rl_sim::MAX_WORMS`).
- Spec §3.2 numbers: fill window 12 ticks; input delay `d = min(3, floor(rtt_ms / 40))`; speed adjustment at most 1 %; checksum every 10 confirmed ticks; "connection problem" after 3 s of missed inputs; drop after 10 s silence.
- Spec §3.1 budget: at most 24 KB/s received per client at 32 players, including 48 bytes of per-datagram overhead (DTLS + SCTP + UDP + IPv4 headers).
- Spec §3.3 budget: restore plus 12 re-simulated ticks within 2 ms p99 at the reference configuration (16 players, Castle Strike, Classic), within 4 ms at the 4096 × 4096 envelope point.
- New external dependencies: none beyond what the workspace already pins (serde, postcard `=1.1.3`, miniz_oxide `=0.9.1`, proptest, divan). System packages: `sudo apt install nodejs` (for the WASM leg) and `sudo apt install qemu-user gcc-aarch64-linux-gnu` (for the arm64 leg), each skipped with a notice when missing; `rustup target add wasm32-wasip1 aarch64-unknown-linux-gnu` (added to `rust-toolchain.toml`).
- Every new crate has a `CLAUDE.md`; every ported-free networking decision is recorded in `docs/porting/STATUS.md` under a new `## Networking decisions (plan 9)` section as `- **N<k>:** ...`; `CHANGELOG.md` gets bullets under `## Unreleased` → `### Added`.
- Run `cargo xtask check` before every commit; commit on `main`, never push. Commit trailers:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_019821i4KFSbhZ4mHYULrhEV
  ```

## Decisions this plan makes (recorded as STATUS.md N-rulings in Task 12)

- **N1 – client prediction limit 28 ticks** (amended during Task 7 from 20, measured). The spec's 12-tick window is the host's fill deadline (§3.2). A client may predict up to `MAX_PREDICTION = 28` ticks past its confirmed tick before it stalls: a client's frontier waits for the slowest client's inputs to reach the host (up to the 12-tick fill window under loss bursts) on top of its own two one-way legs minus its input delay. Measured in the `bad` profile (10 seeds, 4 clients), stall events by limit: 20 → 310, 22 → 68, 24 → 14, 26 → 3, 28 → 0. A stall freezes the player's game, which is worse than a deep correction, so the limit is 28 (snapshot ring of 29). The §3.3 budget is measured at 12 re-simulated ticks (the usual depth) and reported at 28.
- **N2 – input delay fixed per match.** `d` is chosen once, at match start, from the round-trip time (or pinned by the player), not changed mid-match: changing it mid-match leaves a tick with two inputs or none.
- **N3 – profiles are pings.** A profile's latency is the round-trip time; each direction takes half, with half the jitter.
- **N4 – the sync checksum.** Rollback compares `World::sync_checksum`, which equals `World::checksum` except that the terrain enters as an incrementally maintained, order-independent cell hash (O(1) to read) instead of FNV over every cell (570 µs at Castle Strike, about 13 ms at 4096²). `World::checksum` (replays, goldens, oracle) is unchanged.
- **N5 – world-state transfer by serde.** The world's dynamic state crosses the network as `rl_sim::WorldState` (serde-derived, postcard, deflated), decoded into a world built from the same match setup, and validated before use. The game mode transfers its own state through two new `GameMode` methods.
- **N6 – disconnected slots idle.** In plan 9, when a peer is dropped the host fills its slot with `Keys(0)` until the match ends; what happens to the worm (removal, a bot taking over) is decided with the lobby in plan 10.
- **N7 – time sync by arrival margin.** The host measures, for each client, how many ticks before it needs them the client's inputs arrive, and reports the median of the last 16 samples; the client runs up to 1 % faster or slower to hold it at 1 tick. A client more than 8 ticks off jumps (catches up up to 10 ticks per call, or holds).

## Review Focus

1. **A malicious host's world state** (spec §5.4: strangers host): a crafted `WorldState` with out-of-range indices, NaN positions or oversized buffers must be refused by `World::load_state`, and a state it accepts must tick without panicking. Pinned by Task 3's `corrupt_states_never_panic` proptest (which also ticks every accepted state).
2. **Inputs arriving after their tick was filled** (spec §3.2's lagging-player rule): the late real input is dropped, the client is told, and every peer (the late one included) ends with the host's filled value. Pinned by Task 7's `late_inputs_lose_to_the_fill`.
3. **A peer that falls far behind or runs far ahead** (clock drift, a frozen laptop): it must stall rather than predict past `MAX_PREDICTION`, and catch up without desyncing. Pinned by Task 7's `a_frozen_client_catches_up` and `drift_is_corrected`.
4. **Garbage on the wire** (any byte string from any peer): every decoder and `Session::receive` returns an error, counts it and never panics. Pinned by Task 4's `decode_never_panics` and Task 6's `receive_never_panics`.
5. **Rollback across terrain edits** (explosions carving and dirt placed in mispredicted ticks): after restore and re-simulation the terrain must be exactly the confirmed timeline's. Pinned by Task 2's `restore_undoes_terrain_edits` (synthetic), Task 8's `restore_undoes_terrain_edits_in_a_real_match` (MW 1.0 on Castle Strike) and Task 9's World-backed verdict.

---

## File structure

| File | Responsibility |
|---|---|
| `crates/rl-terrain/src/journal.rs` (new) | The edit journal (pre-images) and the incremental cell hash. |
| `crates/rl-terrain/src/lib.rs`, `dirt.rs` | Record pre-images and update the hash in the three mutators. |
| `crates/rl-sim/src/snapshot.rs` (new) | `Snapshot`, `World::{enable_rollback, save, restore, forget_before}`. |
| `crates/rl-sim/src/checksum.rs` | `World::sync_checksum` sharing the field walk with `checksum`. |
| `crates/rl-sim/src/state.rs` (new) | `WorldState`, `World::{state, load_state}`, `StateError`, validation. |
| `crates/rl-sim/src/mode.rs` | `GameMode::{save_state, load_state}`. |
| `crates/rl-modes/src/deathmatch.rs` | Deathmatch's state transfer. |
| `crates/rl-proto/` (new crate) | Messages, the frame delta codec, limits, the fingerprint (`build.rs`). |
| `crates/rl-netcode/src/sim.rs` | The `Simulation` trait and `ToySim`. |
| `crates/rl-netcode/src/netsim.rs` | The deterministic network model (links, profiles, clocks). |
| `crates/rl-netcode/src/inputs.rs` | The final-input table and predictions. |
| `crates/rl-netcode/src/session.rs` | The rollback session (host, client, spectator). |
| `crates/rl-netcode/src/clock.rs` | Tick pacing, speed adjustment, arrival margins. |
| `crates/rl-netcode/src/sync.rs` | Checksums, desync detection, state transfer. |
| `crates/rl-netcode/src/world_sim.rs` | `Simulation` for `rl_sim::World` (snapshots, encoding). |
| `crates/rl-netcode/src/harness.rs` | The generic N-peer harness, verdict and invariants. |
| `crates/rl-cli/src/netsim.rs`, `net_sim_cmd.rs` | World-backed harness with bots, `rl-cli net-sim`, failure files. |
| `crates/rl-cli/src/bundle.rs`, `net_repro_cmd.rs` | Desync bundles and `rl-cli net-repro`. |
| `crates/rl-cli/src/feel.rs` | Feel numbers. |
| `crates/rl-detcheck/` (new crate) | Plays replays, prints checksum streams (native, WASI, Windows). |
| `tools/wasi-run.mjs` (new) | Runs a WASI binary under Node.js. |
| `xtask/src/determinism.rs`, `xtask/src/net.rs` (new) | The determinism step and the feel gate. |
| `fixtures/net/cases/*.ron`, `fixtures/net/feel.ron` (new) | Kept failure cases, feel baselines. |

---
### Task 1: rl-terrain: the edit journal and the cell hash

**Files:**
- Create: `crates/rl-terrain/src/journal.rs`
- Modify: `crates/rl-terrain/src/lib.rs` (struct `Terrain`, `carve_hole`), `crates/rl-terrain/src/dirt.rs` (`place_dirt`, `place_green_dirt`), `crates/rl-terrain/CLAUDE.md`
- Test: `crates/rl-terrain/src/journal.rs` (inline `#[cfg(test)]`), `crates/rl-terrain/tests/journal.rs` (proptest)

**Interfaces:**
- Consumes: nothing new.
- Produces (used by Task 2):
  ```rust
  pub struct JournalMark(u64);          // Copy, Eq, Ord, Debug
  pub enum JournalError { NotRecording, Forgotten }
  impl Terrain {
      pub fn start_journal(&mut self);
      pub fn journal_mark(&self) -> Option<JournalMark>;
      pub fn undo_to(&mut self, mark: JournalMark) -> Result<(), JournalError>;
      pub fn forget_before(&mut self, mark: JournalMark);
      pub fn cell_hash(&self) -> u64;
      pub fn journal_bytes(&self) -> usize;
  }
  impl Default for Terrain   // 0 × 0, no journal
  ```

Context: `Terrain { width, height, cells: Vec<u8> }` is one material byte per world pixel; only `carve_hole` (`lib.rs`), `place_dirt` and `place_green_dirt` (`dirt.rs`) write `cells`. Rollback (spec §3.3) must restore the terrain in proportion to what changed, so each of these three records the clipped rectangle's old bytes before writing, when the journal is on. The journal also keeps an order-independent hash of the cells, updated per changed cell, so a sync checksum (Task 2) reads it in O(1).

The cell hash is `Σ mix((i << 8) | cell[i])` over every cell index `i`, wrapping, with `mix` the splitmix64 finaliser. A changed cell subtracts its old term and adds its new one.

- [ ] **Step 1: Write the failing tests**

`crates/rl-terrain/src/journal.rs` (module skeleton plus tests; the implementation comes in Step 3):

```rust
//! Rollback support (networking spec §3.3): the terrain's edit journal and
//! its incrementally kept cell hash.
//!
//! While the journal is on, every write through `carve_hole`, `place_dirt`
//! and `place_green_dirt` first records the old bytes of the clipped
//! rectangle it may change; `undo_to` puts them back newest first, so a
//! restore costs what changed since the mark, not the map's size. The cell
//! hash is the wrapping sum of `mix((index << 8) | material)` over all
//! cells: order-independent, so one changed cell updates it in O(1).

use alloc::collections::VecDeque;
use alloc::boxed::Box;

use crate::Terrain;

/// A position in the journal: the count of edits recorded since
/// `start_journal`. Marks only grow, so they survive `forget_before`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct JournalMark(pub(crate) u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JournalError {
    /// `start_journal` was never called.
    NotRecording,
    /// The mark is older than what `forget_before` kept.
    Forgotten,
}

/// One recorded edit: a clipped rectangle of the map and how many of the
/// journal's bytes (its old cells, row-major) belong to it.
#[derive(Clone, Debug)]
pub(crate) struct Edit {
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
}

#[derive(Clone, Debug)]
pub(crate) struct Journal {
    edits: VecDeque<Edit>,
    bytes: VecDeque<u8>,
    /// Edits dropped from the front by `forget_before`.
    base: u64,
    hash: u64,
}

/// The splitmix64 finaliser.
pub(crate) fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

pub(crate) fn term(i: usize, cell: u8) -> u64 {
    mix(((i as u64) << 8) | cell as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use rl_data::image::{Rgba, RgbaImage};
    use rl_data::level::{Material, MaterialGrid};

    use crate::Stamp;

    fn dirt(w: u32, h: u32) -> Terrain {
        Terrain::from_grid(&MaterialGrid::new(w, h, Material::Dirt))
    }

    fn square(n: u32) -> Stamp {
        let mut img = RgbaImage::new(n, n);
        for y in 0..n {
            for x in 0..n {
                img.set(x, y, Rgba::MAGENTA);
            }
        }
        Stamp::from_image(&img)
    }

    #[test]
    fn undo_restores_the_cells_and_the_hash() {
        let mut t = dirt(40, 30);
        t.start_journal();
        let before = t.clone();
        let hash = t.cell_hash();
        let mark = t.journal_mark().unwrap();
        t.carve_hole(&square(8), 10, 10);
        t.carve_hole(&square(8), 0, 0); // clipped at the corner
        t.place_dirt(&square(4), 10, 10);
        assert_ne!(t, before);
        assert_ne!(t.cell_hash(), hash);
        t.undo_to(mark).unwrap();
        assert_eq!(t, before);
        assert_eq!(t.cell_hash(), hash);
    }

    #[test]
    fn the_tracked_hash_equals_the_hash_from_scratch() {
        let mut t = dirt(40, 30);
        t.start_journal();
        t.carve_hole(&square(8), 5, 7);
        t.place_dirt(&square(4), 6, 6);
        let tracked = t.cell_hash();
        let mut fresh = t.clone();
        fresh.journal = None;
        assert_eq!(fresh.cell_hash(), tracked);
    }

    #[test]
    fn marks_survive_forgetting_and_old_marks_are_refused() {
        let mut t = dirt(20, 20);
        t.start_journal();
        let m0 = t.journal_mark().unwrap();
        t.carve_hole(&square(4), 5, 5);
        let m1 = t.journal_mark().unwrap();
        let at_m1 = t.clone();
        t.carve_hole(&square(4), 12, 12);
        t.forget_before(m1);
        assert_eq!(t.undo_to(m0), Err(JournalError::Forgotten));
        t.undo_to(m1).unwrap();
        assert_eq!(t, at_m1);
    }

    #[test]
    fn without_a_journal_nothing_is_recorded() {
        let mut t = dirt(20, 20);
        assert_eq!(t.journal_mark(), None);
        t.carve_hole(&square(4), 5, 5);
        assert_eq!(t.journal_bytes(), 0);
        assert_eq!(t.undo_to(JournalMark(0)), Err(JournalError::NotRecording));
    }

    #[test]
    fn equality_ignores_the_journal() {
        let a = dirt(10, 10);
        let mut b = a.clone();
        b.start_journal();
        assert_eq!(a, b);
        let _ = vec![0u8]; // keep `vec` used in no_std test builds
    }
}
```

`crates/rl-terrain/tests/journal.rs` (proptest; `proptest` becomes a dev-dependency of rl-terrain):

```rust
//! Random edit sequences: undoing to any mark restores that moment exactly,
//! and the tracked cell hash always equals the hash from scratch.

use proptest::prelude::*;
use rl_data::image::{Rgba, RgbaImage};
use rl_data::level::{Material, MaterialGrid};
use rl_terrain::{GreenMask, Stamp, Terrain};

#[derive(Clone, Debug)]
enum Op {
    Carve(u32, i32, i32),
    Dirt(u32, i32, i32),
    Green(i32, i32),
    Mark,
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        (1u32..12, -10i32..70, -10i32..50).prop_map(|(n, x, y)| Op::Carve(n, x, y)),
        (1u32..12, -10i32..70, -10i32..50).prop_map(|(n, x, y)| Op::Dirt(n, x, y)),
        (-10i32..70, -10i32..50).prop_map(|(x, y)| Op::Green(x, y)),
        Just(Op::Mark),
    ]
}

fn square(n: u32) -> Stamp {
    let mut img = RgbaImage::new(n, n);
    for y in 0..n {
        for x in 0..n {
            img.set(x, y, if (x + y) % 3 == 0 { Rgba::BLACK } else { Rgba::MAGENTA });
        }
    }
    Stamp::from_image(&img)
}

fn green() -> GreenMask {
    let mut img = RgbaImage::new(5, 5);
    for y in 0..5 {
        for x in 0..5 {
            img.set(x, y, Rgba::new(0, 255, 0, 255));
        }
    }
    GreenMask::from_image(&img)
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn undo_to_any_mark_restores_it(ops in prop::collection::vec(op(), 1..40), pick in any::<prop::sample::Index>()) {
        let mut grid = MaterialGrid::new(60, 40, Material::Dirt);
        for x in 0..60 { grid.set(x, 20, Material::Rock); }
        let mut t = Terrain::from_grid(&grid);
        t.start_journal();
        let mut marks = vec![(t.journal_mark().unwrap(), t.clone())];
        let mut seed = 7i32;
        for o in &ops {
            match *o {
                Op::Carve(n, x, y) => { t.carve_hole(&square(n), x, y); }
                Op::Dirt(n, x, y) => { t.place_dirt(&square(n), x, y); }
                Op::Green(x, y) => { t.place_green_dirt(&green(), x, y, &mut || { seed = seed.wrapping_mul(31) + 1; seed & 3 }); }
                Op::Mark => marks.push((t.journal_mark().unwrap(), t.clone())),
            }
            let mut fresh = t.clone();
            fresh.stop_journal_for_tests();
            prop_assert_eq!(fresh.cell_hash(), t.cell_hash());
        }
        let (mark, at) = &marks[pick.index(marks.len())];
        t.undo_to(*mark).unwrap();
        prop_assert_eq!(&t, at);
        let mut fresh = t.clone();
        fresh.stop_journal_for_tests();
        prop_assert_eq!(fresh.cell_hash(), t.cell_hash());
    }
}
```

`MaterialGrid::set`, `Rgba::new` and `GreenMask::from_image` must exist; check with `cargo xtask api` (`target/api/rl_data.txt`, `target/api/rl_terrain.txt`) and use the real names if they differ (for example a green colour constant the existing `dirt.rs` tests use). `stop_journal_for_tests` is a `#[doc(hidden)] pub fn` that sets the journal to `None` so the hash is computed from scratch; it exists only for these tests.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo nextest run -p rl-terrain`
Expected: compile errors: `start_journal`, `journal_mark`, `undo_to`, `cell_hash` not found.

- [ ] **Step 3: Implement**

In `lib.rs`: `mod journal; pub use journal::{JournalError, JournalMark};`. `Terrain` gets a field `journal: Option<Box<journal::Journal>>`. Replace `#[derive(Clone, Debug, PartialEq, Eq)]` by `#[derive(Clone, Debug, Default)]` and a manual `PartialEq` + `Eq` comparing `width`, `height` and `cells` only (document: "the journal is bookkeeping, not terrain"). `from_grid` sets `journal: None`.

In `journal.rs`, the `Terrain` methods:

```rust
impl Terrain {
    /// Starts recording edits and tracking the cell hash (idempotent).
    pub fn start_journal(&mut self) {
        if self.journal.is_none() {
            let hash = self.scratch_hash();
            self.journal = Some(Box::new(Journal { edits: VecDeque::new(), bytes: VecDeque::new(), base: 0, hash }));
        }
    }

    #[doc(hidden)]
    pub fn stop_journal_for_tests(&mut self) {
        self.journal = None;
    }

    pub fn journal_mark(&self) -> Option<JournalMark> {
        self.journal.as_ref().map(|j| JournalMark(j.base + j.edits.len() as u64))
    }

    /// Puts back every cell changed since `mark`, newest edit first.
    pub fn undo_to(&mut self, mark: JournalMark) -> Result<(), JournalError> {
        let width = self.width as usize;
        let j = self.journal.as_mut().ok_or(JournalError::NotRecording)?;
        if mark.0 < j.base {
            return Err(JournalError::Forgotten);
        }
        while j.base + j.edits.len() as u64 > mark.0 {
            let e = j.edits.pop_back().expect("counted above");
            let (w, h) = ((e.x1 - e.x0) as usize, (e.y1 - e.y0) as usize);
            // The edit's bytes are the last w * h of the journal, row-major.
            for row in (0..h).rev() {
                for col in (0..w).rev() {
                    let old = j.bytes.pop_back().expect("an edit's bytes are recorded with it");
                    let i = (e.y0 as usize + row) * width + e.x0 as usize + col;
                    let cur = self.cells[i];
                    if cur != old {
                        j.hash = j.hash.wrapping_sub(term(i, cur)).wrapping_add(term(i, old));
                        self.cells[i] = old;
                    }
                }
            }
        }
        Ok(())
    }

    /// Drops the records older than `mark` (they can no longer be undone).
    pub fn forget_before(&mut self, mark: JournalMark) {
        let Some(j) = self.journal.as_mut() else { return };
        while j.base < mark.0 {
            let Some(e) = j.edits.pop_front() else { break };
            let n = ((e.x1 - e.x0) * (e.y1 - e.y0)) as usize;
            j.bytes.drain(..n);
            j.base += 1;
        }
    }

    /// The cell hash: tracked while the journal is on, else computed.
    pub fn cell_hash(&self) -> u64 {
        match &self.journal {
            Some(j) => j.hash,
            None => self.scratch_hash(),
        }
    }

    /// Bytes the journal holds (pre-images), for the session's statistics.
    pub fn journal_bytes(&self) -> usize {
        self.journal.as_ref().map_or(0, |j| j.bytes.len())
    }

    fn scratch_hash(&self) -> u64 {
        self.cells.iter().enumerate().fold(0u64, |h, (i, &c)| h.wrapping_add(term(i, c)))
    }

    /// Records the old cells of `[x0, x1) × [y0, y1)` before a write (no-op
    /// without a journal or for an empty rectangle).
    pub(crate) fn record(&mut self, x0: u32, y0: u32, x1: u32, y1: u32) {
        let width = self.width as usize;
        let Some(j) = self.journal.as_mut() else { return };
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        for y in y0..y1 {
            let row = y as usize * width;
            j.bytes.extend(&self.cells[row + x0 as usize..row + x1 as usize]);
        }
        j.edits.push_back(Edit { x0, y0, x1, y1 });
    }

    /// Writes one cell, keeping the tracked hash.
    #[inline]
    pub(crate) fn set_cell(&mut self, i: usize, v: u8) {
        let old = self.cells[i];
        if old != v {
            if let Some(j) = self.journal.as_mut() {
                j.hash = j.hash.wrapping_sub(term(i, old)).wrapping_add(term(i, v));
            }
            self.cells[i] = v;
        }
    }
}
```

In the three mutators, call `self.record(..)` with the clipped rectangle right before the loop, and replace every `self.cells[i] = ...` by `self.set_cell(i, ...)`:
- `carve_hole`: after both `clip` calls succeed, `self.record(x as u32, y as u32, (x + cw) as u32, (y + ch) as u32);`.
- `place_dirt` and `place_green_dirt`: after `Clip::new`, if `clip.x0 < clip.x1 && clip.y0 < clip.y1`, `self.record(clip.x0 as u32, clip.y0 as u32, clip.x1 as u32, clip.y1 as u32);`. The loops borrow `self.cells` through `clip.cells(self.width)` only for indices, so `set_cell` compiles; if the borrow checker objects, collect nothing — iterate the same index ranges directly.

Behaviour without a journal is unchanged: `set_cell` writes the same values (an unchanged write is skipped, which writes nothing different), `record` returns at once.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo nextest run -p rl-terrain && cargo nextest run -p rl-sim --test no_alloc && cargo nextest run -p rl-cli --test golden`
Expected: all pass (the goldens and the allocation test prove behaviour and allocation-freedom are unchanged).

- [ ] **Step 5: Document and commit**

Add to `crates/rl-terrain/CLAUDE.md` a bullet: "**Journal** (`journal.rs`, networking spec §3.3): off by default; `start_journal` records each mutator's clipped pre-image and keeps the order-independent cell hash. Every new write to `cells` must go through `set_cell` after `record`, or rollback breaks." Run `cargo xtask check`, then:

```bash
git add crates/rl-terrain
git commit -m "rl-terrain: an edit journal and an incremental cell hash for rollback"
```

---

### Task 2: rl-sim: snapshots and the sync checksum

**Files:**
- Create: `crates/rl-sim/src/snapshot.rs`, `crates/rl-sim/tests/snapshot.rs`, `crates/rl-sim/tests/common/world.rs` (the synthetic world of `benches/tick.rs`, shared)
- Modify: `crates/rl-sim/src/lib.rs` (module, re-exports), `crates/rl-sim/src/checksum.rs`, `crates/rl-sim/benches/tick.rs` (use the shared helper, add `save_restore`), `crates/rl-sim/CLAUDE.md`
- Test: `crates/rl-sim/tests/snapshot.rs`; content variant in `crates/rl-netcode/tests/world_sim.rs` (Task 8)

**Interfaces:**
- Consumes (Task 1): `Terrain::{start_journal, journal_mark, undo_to, forget_before, cell_hash, journal_bytes}`, `JournalMark`, `JournalError`, `Terrain: Default`.
- Produces (Tasks 3, 8):
  ```rust
  pub struct Snapshot { /* world without terrain, journal mark */ }
  impl Snapshot { pub fn server_frame(&self) -> u64; }
  pub enum RestoreError { Journal(rl_terrain::JournalError), NotThisWorld }
  impl World {
      pub fn enable_rollback(&mut self);
      pub fn rollback_enabled(&self) -> bool;
      pub fn save(&mut self) -> Snapshot;
      pub fn restore(&mut self, s: &Snapshot) -> Result<(), RestoreError>;
      pub fn forget_before(&mut self, s: &Snapshot);
      pub fn sync_checksum(&self) -> u64;
      pub fn journal_bytes(&self) -> usize;
  }
  ```

Context: `World: Clone` (~1.25 MB at Castle Strike, of which 742 KB terrain, 480 KB projectile pool; about 20 µs). A snapshot is the world without its terrain plus the terrain journal's mark; a restore undoes the terrain to the mark and copies the rest back. `World::checksum` hashes the terrain with byte-wise FNV (570 µs at Castle Strike); `sync_checksum` is the same walk with `terrain.cell_hash()` instead (decision N4).

- [ ] **Step 1: Share the synthetic world**

Move `pellet`, `sim_mod` and `world(worms, w, h)` from `benches/tick.rs` into `crates/rl-sim/tests/common/world.rs` (unchanged bodies, `pub fn`), and make the bench include it with `#[path = "../tests/common/world.rs"] mod world;`. The bench results must not change (`cargo bench -p rl-sim --bench tick` compiles; no re-bless).

- [ ] **Step 2: Write the failing tests**

`crates/rl-sim/tests/snapshot.rs`:

```rust
//! Snapshots (networking spec §3.3): restoring and re-simulating gives the
//! straight run's world exactly, terrain included, and the sync checksum
//! tracks the state like `checksum` does.

#[path = "common/world.rs"]
mod world;

use rl_sim::{Keys, TickEvents, World};

/// Fires constantly with a moving aim, so pellets fly and carve.
fn keys(t: u32, n: usize) -> Vec<Keys> {
    (0..n)
        .map(|i| {
            let mut k = Keys::FIRE;
            if (t / 7 + i as u32) % 2 == 0 { k |= Keys::UP } else { k |= Keys::DOWN }
            if (t / 31 + i as u32) % 3 == 0 { k |= Keys::LEFT }
            Keys(k)
        })
        .collect()
}

fn run(w: &mut World, from: u32, to: u32, ev: &mut TickEvents) {
    for t in from..to {
        w.tick(&keys(t, 8), ev);
    }
}

#[test]
fn restore_then_resimulate_matches_the_straight_run() {
    let mut ev = TickEvents::new();
    let mut straight = world::world(8, 400, 300);
    run(&mut straight, 0, 300, &mut ev);

    let mut w = world::world(8, 400, 300);
    w.enable_rollback();
    run(&mut w, 0, 100, &mut ev);
    let snap = w.save();
    // A different future, then back.
    for _ in 100..180 {
        w.tick(&[Keys(Keys::FIRE | Keys::JUMP); 8], &mut ev);
    }
    w.restore(&snap).unwrap();
    assert_eq!(w.server_frame(), snap.server_frame());
    run(&mut w, 100, 300, &mut ev);
    assert_eq!(w.checksum(), straight.checksum());
    assert_eq!(w, straight);
}

#[test]
fn restore_undoes_terrain_edits() {
    let mut ev = TickEvents::new();
    let mut w = world::world(8, 400, 300);
    w.enable_rollback();
    run(&mut w, 0, 50, &mut ev);
    let before = w.terrain().clone();
    let snap = w.save();
    run(&mut w, 50, 250, &mut ev);
    assert_ne!(w.terrain(), &before, "the fixture must carve");
    w.restore(&snap).unwrap();
    assert_eq!(w.terrain(), &before);
}

#[test]
fn snapshots_without_rollback_copy_the_terrain() {
    let mut ev = TickEvents::new();
    let mut w = world::world(4, 200, 150);
    run(&mut w, 0, 20, &mut ev);
    let snap = w.save();
    let at = w.clone();
    run(&mut w, 20, 120, &mut ev);
    w.restore(&snap).unwrap();
    assert_eq!(w, at);
}

#[test]
fn sync_checksum_follows_the_state() {
    let mut ev = TickEvents::new();
    let mut a = world::world(4, 200, 150);
    a.enable_rollback();
    let mut b = world::world(4, 200, 150);
    assert_eq!(a.sync_checksum(), b.sync_checksum(), "tracked and scratch hashes agree");
    run(&mut a, 0, 60, &mut ev);
    assert_ne!(a.sync_checksum(), b.sync_checksum());
    run(&mut b, 0, 60, &mut ev);
    assert_eq!(a.sync_checksum(), b.sync_checksum());
    b.set_health(1, 3.0);
    assert_ne!(a.sync_checksum(), b.sync_checksum());
}

#[test]
fn forgetting_keeps_newer_snapshots_usable() {
    let mut ev = TickEvents::new();
    let mut w = world::world(8, 400, 300);
    w.enable_rollback();
    let old = w.save();
    run(&mut w, 0, 40, &mut ev);
    let newer = w.save();
    let at = w.clone();
    run(&mut w, 40, 90, &mut ev);
    w.forget_before(&newer);
    assert!(w.restore(&old).is_err());
    w.restore(&newer).unwrap();
    assert_eq!(w, at);
}
```

`Keys::FIRE` etc. are `u8` constants and `Keys(pub u8)`; adjust the `|=` expressions if `Keys` offers named constructors. `World::set_health(i, f32)` exists (setup mutator). If the synthetic level has no dirt to carve, make `world()`'s helper fill the map's lower half with dirt (it is test code; the bench uses the same helper, so check the bench's numbers do not move by more than noise — if they would, add a `world_with_dirt` variant instead and leave `world` unchanged).

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo nextest run -p rl-sim --test snapshot`
Expected: compile errors: `enable_rollback`, `save`, `restore`, `sync_checksum` not found.

- [ ] **Step 4: Implement `snapshot.rs`**

```rust
//! Snapshots for rollback (networking spec §3.3).

use core::mem;

use rl_terrain::{JournalError, JournalMark};

use crate::World;

/// The world at one moment: everything but the terrain, which is kept as a
/// mark in the terrain's journal when rollback is on (a full copy otherwise).
#[derive(Clone, Debug)]
pub struct Snapshot {
    world: World,
    mark: Option<JournalMark>,
}

impl Snapshot {
    /// The server frame the snapshot was taken at (before that tick runs).
    pub fn server_frame(&self) -> u64 {
        self.world.server_frame
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestoreError {
    Journal(JournalError),
    /// A snapshot taken without rollback restored into a world with it, or the reverse.
    NotThisWorld,
}

impl World {
    /// Turns on the terrain journal: snapshots then skip the terrain.
    pub fn enable_rollback(&mut self) {
        self.terrain.start_journal();
    }

    pub fn rollback_enabled(&self) -> bool {
        self.terrain.journal_mark().is_some()
    }

    pub fn save(&mut self) -> Snapshot {
        match self.terrain.journal_mark() {
            Some(mark) => {
                let terrain = mem::take(&mut self.terrain);
                let world = self.clone();
                self.terrain = terrain;
                Snapshot { world, mark: Some(mark) }
            }
            None => Snapshot { world: self.clone(), mark: None },
        }
    }

    pub fn restore(&mut self, s: &Snapshot) -> Result<(), RestoreError> {
        match (s.mark, self.rollback_enabled()) {
            (Some(mark), true) => {
                self.terrain.undo_to(mark).map_err(RestoreError::Journal)?;
                let terrain = mem::take(&mut self.terrain);
                self.clone_from(&s.world);
                self.terrain = terrain;
                Ok(())
            }
            (None, false) => {
                self.clone_from(&s.world);
                Ok(())
            }
            _ => Err(RestoreError::NotThisWorld),
        }
    }

    /// Drops the journal older than `s`: snapshots older than it can no
    /// longer be restored.
    pub fn forget_before(&mut self, s: &Snapshot) {
        if let Some(mark) = s.mark {
            self.terrain.forget_before(mark);
        }
    }

    pub fn journal_bytes(&self) -> usize {
        self.terrain.journal_bytes()
    }
}
```

`World` derives `Clone` and so has `clone_from` (the default clones); if `World` implements `Clone` manually, use `*self = s.world.clone()`. Re-export from `lib.rs`: `pub use snapshot::{RestoreError, Snapshot};`.

- [ ] **Step 5: Implement `sync_checksum`**

In `checksum.rs`, move the body of `checksum` after the terrain lines into `fn hash_state(&self, terrain_hash: u64) -> u64`, which starts `let mut h = Fnv::new(); h.u32(terrain.width()); h.u32(terrain.height()); h.u64(terrain_hash);` and continues exactly as today. Then:

```rust
pub fn checksum(&self) -> u64 {
    self.hash_state(self.terrain.material_hash())
}

/// [`World::checksum`] with the terrain as its cell hash
/// ([`rl_terrain::Terrain::cell_hash`]): O(1) to read while rollback is on,
/// so the session can compare worlds every 10 ticks (networking spec §3.5).
/// Not equal to `checksum`; equal between equal worlds whether or not
/// rollback is on.
pub fn sync_checksum(&self) -> u64 {
    self.hash_state(self.terrain.cell_hash())
}
```

Keep the exhaustive destructuring inside `hash_state`.

- [ ] **Step 6: Add the bench**

In `benches/tick.rs` add:

```rust
/// One snapshot and one restore of a Castle-Strike-sized world with rollback on.
#[divan::bench]
fn save_restore(b: divan::Bencher) {
    let mut w = world::world(16, 1350, 550);
    w.enable_rollback();
    let mut ev = rl_sim::TickEvents::new();
    b.bench_local(|| {
        let s = w.save();
        w.tick(&[rl_sim::Keys(rl_sim::Keys::FIRE); 16], &mut ev);
        w.restore(&s).unwrap();
    });
}
```

It is not gated yet (Task 10 blesses the baselines).

- [ ] **Step 7: Run the tests**

Run: `cargo nextest run -p rl-sim && cargo nextest run -p rl-cli --test golden && cargo bench -p rl-sim --bench tick -- save_restore`
Expected: all pass; the bench prints a time (expect tens of µs).

- [ ] **Step 8: Document and commit**

`crates/rl-sim/CLAUDE.md`: a bullet "**Rollback** (`snapshot.rs`, networking spec §3.3): `enable_rollback` + `save`/`restore`/`forget_before`; `sync_checksum` is `checksum` with the terrain's cell hash. Snapshots carry no events; the caller re-runs ticks for them." `cargo xtask check`, then:

```bash
git add crates/rl-sim
git commit -m "rl-sim: snapshots and a sync checksum for rollback"
```

---
### Task 3: rl-sim + rl-modes: world-state transfer

**Files:**
- Create: `crates/rl-sim/src/state.rs`, `crates/rl-sim/tests/state.rs`, `crates/rl-modes/tests/state.rs`
- Modify: `crates/rl-math/Cargo.toml` + `src/` (serde derives on `AbsTime`, `TimeDiff`, `Vec2`, `GlibcRand`, `Mt19937`), `crates/rl-sim/Cargo.toml` (serde, postcard, miniz_oxide), the rl-sim state types (`worm.rs`, `rope.rs`, `projectile.rs`, `shots.rs`, `bonus.rs`, `input.rs`, `rng.rs`, `referee.rs`, `mode.rs`), `crates/rl-sim/src/lib.rs`, `crates/rl-modes/Cargo.toml` (serde, postcard), `crates/rl-modes/src/deathmatch.rs`, both crates' `CLAUDE.md`
- Test: `crates/rl-sim/tests/state.rs` (synthetic world, `OracleMode`), `crates/rl-modes/tests/state.rs` (Deathmatch)

**Interfaces:**
- Consumes (Task 2): `World::{save, restore}` are not needed; `World: PartialEq`, `World::checksum`, `World::set_health`.
- Produces (Tasks 8, 10):
  ```rust
  #[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
  pub struct WorldState { /* private fields */ }
  impl WorldState {
      pub fn encode(&self) -> Vec<u8>;                         // postcard, then deflate
      pub fn decode(bytes: &[u8]) -> Result<WorldState, StateError>;
      pub const MAX_INFLATED: usize = 64 * 1024 * 1024;
      #[doc(hidden)] pub fn to_raw(&self) -> Vec<u8>;           // postcard only (tests)
      #[doc(hidden)] pub fn from_raw(bytes: &[u8]) -> Result<WorldState, StateError>;
  }
  pub enum StateError { Decode, TooLarge, Terrain, Worms, Projectiles, Buffers, Bonuses, Rng, Mode(ModeStateError) }
  impl World {
      pub fn state(&self) -> WorldState;
      pub fn load_state(&mut self, s: WorldState) -> Result<(), StateError>;
  }
  // rl-sim mode.rs
  pub enum ModeStateError { Unsupported, Decode, Invalid }
  trait GameMode {
      fn save_state(&self) -> Vec<u8> { Vec::new() }
      fn load_state(&mut self, bytes: &[u8]) -> Result<(), ModeStateError> { Err(ModeStateError::Unsupported) }
  }
  ```

Context: decision N5. A world built from the same match setup (settings, mod, level, holes, green mask, worm count, pool capacity, game mode with its settings) receives the other peer's dynamic state: spectators who arrive mid-match and peers repairing a desync (spec §3.5, §3.6). The state comes from a host that may be a stranger: `load_state` validates everything that could index out of bounds or grow a buffer past its capacity, before changing anything, and is all-or-nothing.

What `WorldState` holds: exactly what `World::checksum` hashes (see `checksum.rs`'s destructuring): the terrain (width, height and the cell bytes), worms, latches, both clocks, the server frame, the sub-step count, the shot list and last shots, the projectile pool (all slots: freed slots keep stale values that are hashed), the draws' two generators, entity counter and `rot_bumped`, both bonus arrays and their queues, the referee (the mode as `save_state` bytes, the command list's length state, `over`, `over_frame`, `game_over_sent`). Left out, as in the checksum: settings, sim mod, holes, green mask, events, the spawn work counter, the action tally, scratch and spare buffers. Make `WorldState`'s field list an exhaustive destructuring of `World` in `World::state` (like `checksum`), so a new `World` field does not compile until it is transferred or listed as setup-only.

- [ ] **Step 1: Add the serde derives**

Workspace `serde` is `default-features = false, features = ["derive", "alloc"]`. Add `serde.workspace = true` to rl-math, rl-sim and rl-modes, `postcard.workspace = true` and `miniz_oxide.workspace = true` to rl-sim, `postcard.workspace = true` to rl-modes. Derive `serde::Serialize, serde::Deserialize` on every type `WorldState` reaches (follow the compile errors). Arrays longer than 32 elements have no serde impl: give `Mt19937` manual `Serialize`/`Deserialize` impls through a private mirror `struct MtWire { state: Vec<u32>, index: u64, calls: u64 }` whose deserialisation fails unless `state.len() == 624`. `GlibcRand { r: [i32; 31], .. }` derives directly. Do not derive serde on `World` itself, `SimSettings`, `SimMod` or `Stamp`.

- [ ] **Step 2: Write the failing tests**

`crates/rl-sim/tests/state.rs`:

```rust
//! World-state transfer (networking spec §3.5, §3.6): a world built from the
//! same setup takes another world's state and then evolves identically; a
//! corrupted or hostile state is refused, never a panic.

#[path = "common/world.rs"]
mod world;

use proptest::prelude::*;
use rl_sim::{Keys, StateError, TickEvents, World, WorldState};

fn keys(t: u32) -> Vec<Keys> {
    (0..8u32).map(|i| Keys(if (t + i) % 5 < 3 { Keys::FIRE | Keys::LEFT } else { Keys::UP | Keys::JUMP })).collect()
}

fn played(ticks: u32) -> World {
    let mut w = world::world(8, 400, 300);
    let mut ev = TickEvents::new();
    for t in 0..ticks {
        w.tick(&keys(t), &mut ev);
    }
    w
}

#[test]
fn a_loaded_state_evolves_identically() {
    let src = played(150);
    let bytes = src.state().encode();
    let mut dst = world::world(8, 400, 300);
    dst.load_state(WorldState::decode(&bytes).unwrap()).unwrap();
    assert_eq!(dst.checksum(), src.checksum());
    assert_eq!(dst, src);
    let (mut a, mut b) = (src, dst);
    let mut ev = TickEvents::new();
    for t in 150..350 {
        a.tick(&keys(t), &mut ev);
        b.tick(&keys(t), &mut ev);
    }
    assert_eq!(a.checksum(), b.checksum());
}

#[test]
fn a_state_for_another_level_is_refused() {
    let src = played(10);
    let mut dst = world::world(8, 300, 300);
    let before = dst.clone();
    assert_eq!(dst.load_state(src.state()), Err(StateError::Terrain));
    assert_eq!(dst, before, "a refused state changes nothing");
}

#[test]
fn a_state_with_more_worms_is_refused() {
    let src = played(10);
    let mut dst = world::world(4, 400, 300);
    assert_eq!(dst.load_state(src.state()), Err(StateError::Worms));
}

#[test]
fn garbage_is_refused() {
    assert_eq!(WorldState::decode(&[0x78, 0x9c, 0xff, 0xff]), Err(StateError::Decode));
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() })]

    /// Hostile host: random edits of the raw (uncompressed) encoding.
    #[test]
    fn corrupt_states_never_panic(
        edits in prop::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..8),
        cut in any::<prop::sample::Index>(),
    ) {
        let mut raw = played(120).state().to_raw();
        for (i, b) in &edits {
            let i = i.index(raw.len());
            raw[i] = *b;
        }
        raw.truncate(raw.len() - cut.index(raw.len() / 8 + 1));
        if let Ok(s) = WorldState::from_raw(&raw) {
            let mut w = world::world(8, 400, 300);
            if w.load_state(s).is_ok() {
                // Accepted states must tick without panicking.
                let mut ev = TickEvents::new();
                for t in 0..50 {
                    w.tick(&keys(t), &mut ev);
                }
            }
        }
    }
}
```

`crates/rl-modes/tests/state.rs`: build a Deathmatch world the way `tests/deathmatch.rs` does (reuse its helpers through `#[path]` or a `tests/common` module), play 300 ticks with keys that make worms fire and die (copy the key pattern used there), then: `state()` → `encode` → `decode` → `load_state` into a freshly prepared Deathmatch world of the same setup; assert `PartialEq` and equal checksums, then 300 more ticks on both and equal checksums. A second test: `Deathmatch::load_state` with bytes whose board has 40 entries returns `Err(ModeStateError::Invalid)`; with truncated bytes `Err(ModeStateError::Decode)`.

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo nextest run -p rl-sim --test state; cargo nextest run -p rl-modes --test state`
Expected: compile errors (`WorldState`, `state`, `load_state` missing).

- [ ] **Step 4: Implement `GameMode` state**

In `mode.rs` add `ModeStateError { Unsupported, Decode, Invalid }` and the two default methods (shown in Interfaces). `OracleMode` implements both: `save_state` → empty, `load_state` → `Ok(())` only for empty bytes, else `Err(Invalid)`. In `deathmatch.rs`, a private `#[derive(Serialize, Deserialize)] struct DeathmatchState` with every field of `Deathmatch` except `settings`, plus the chaos rotation's dynamic part if `settings.respawn_loadout` is `ChaosCycle` (its `index` and `count`: they change during the match; check `ChaosCycle`'s definition and transfer whatever `checksum` hashes). `save_state` = `postcard::to_allocvec`; `load_state` = `postcard::from_bytes` (→ `Decode`), then validate (`board.len() <= MAX_WORMS`, every `Score::worm` in `0..MAX_WORMS`, chaos indices below the cycle's weapon count, else `Invalid`), then assign. The board keeps its `MAX_WORMS` capacity (assign into the existing `Vec` with `clear` + `extend`).

- [ ] **Step 5: Implement `state.rs`**

- `World::state(&self) -> WorldState`: exhaustive destructuring of `World` (setup-only fields bound to `_`), copying each part; terrain as `(width, height, cells.to_vec())` — add `pub fn cells(&self) -> &[u8]` to `rl_terrain::Terrain` and `pub fn load_cells(&mut self, cells: &[u8]) -> bool` (false unless the length matches and every byte is a valid `Material` index; writes through `set_cell` so a journal's hash stays right, then clears the journal by calling `forget_before(journal_mark)` — a loaded terrain cannot be undone; document it).
- `encode`: `postcard::to_allocvec(self)` then `miniz_oxide::deflate::compress_to_vec(&raw, 6)`. `decode`: `miniz_oxide::inflate::decompress_to_vec_with_limit(bytes, Self::MAX_INFLATED)` (→ `Decode` on error; a limit hit → `TooLarge`), then `from_raw`. `from_raw`: `postcard::from_bytes` → `Decode`.
- `load_state(&mut self, s)`: first `validate(&s)` against `self` (nothing mutated on error), then assign every part, keeping each `Buf`'s capacity (`buf.clear(); buf.extend_from_slice(..)`), and `referee.mode.load_state(&s.mode)?` — validate the mode first too: call `load_state` on a `box_clone` of the mode, and swap it in only after everything else validated.
- `validate` checks, each mapping to a `StateError` variant:
  - `Terrain`: width and height equal `self`'s, cell count equals `width * height`, every cell a valid material.
  - `Worms`: `worms.len() <= MAX_WORMS` and `<= self.worms.capacity()`, `latches.len() == worms.len()`, every worm's `id` equals its index, slot count `<= MAX_SLOTS` (the loadout limit), every slot's weapon id is `-1` or below `sim_mod.weapons().len()`, `current_weapon` within the worm's slots (or the value an empty loadout uses), rope's `player_attached` in `-1..worms.len()`.
  - `Projectiles`: slot count equals `self`'s pool capacity, every used slot's projectile type below the mod's projectile count, owner in `-1..MAX_WORMS`, the pool's `first_unused`, `last_used` and `touched` within the capacity (read `projectile.rs` for their exact meaning and sentinel values).
  - `Buffers`: shots and last shots `<= MAX_SHOOTINGS`, every buffer `<=` the receiving `Buf`'s capacity, `rot_bumped.len()` equals the mod's projectile count, every shot's weapon id below the weapon count.
  - `Bonuses`: queue lengths within capacity, weapon ids of weapon bonuses below the weapon count.
  - `Rng`: `GlibcRand`'s `fptr`, `rptr` below 31; `Mt19937`'s `index <= 624`.
  - Then audit: `grep -n "as usize\]" crates/rl-sim/src/*.rs` and check every index computed from transferred state is covered. The proptest of Step 2 is the net: any panic it finds is a missing check here (fix the check, never the test).

- [ ] **Step 6: Run the tests**

Run: `cargo nextest run -p rl-math -p rl-sim -p rl-modes && cargo nextest run -p rl-cli --test golden && cargo build --target wasm32-unknown-unknown -p rl-sim -p rl-modes`
Expected: all pass; the goldens are unchanged; the WASM build succeeds.

- [ ] **Step 7: Document and commit**

CLAUDE.md bullets: rl-sim "**State transfer** (`state.rs`, networking spec §3.5): `World::state`/`load_state`; a new state field must be added to `WorldState` (the destructuring forces it) and validated if it indexes anything"; rl-modes "Deathmatch transfers its dynamic fields (`save_state`/`load_state`)". `cargo xtask check`, then:

```bash
git add crates/rl-math crates/rl-terrain crates/rl-sim crates/rl-modes
git commit -m "rl-sim: validated world-state transfer for spectators and desync repair"
```

---

### Task 4: rl-proto: the wire format and the fingerprint

**Files:**
- Create: `crates/rl-proto/{Cargo.toml, CLAUDE.md, build.rs, src/lib.rs, src/frames.rs, src/fingerprint_impl.rs, tests/fuzz.rs}`
- Modify: `Cargo.toml` (workspace dependency `rl-proto = { path = "crates/rl-proto" }`), `xtask/layers.ron` (layer 4, `no_std`, `wasm`)
- Test: inline tests in `frames.rs`, `tests/fuzz.rs`

**Interfaces:**
- Consumes: nothing from earlier tasks (keys are plain `u8` on the wire).
- Produces (Tasks 6-10):
  ```rust
  pub const PROTOCOL_VERSION: u16 = 1;
  pub const MAX_SLOTS: usize = 32;
  pub const MAX_DATAGRAM: usize = 1200;
  pub const MAX_CONTROL: usize = 40 * 1024;
  pub const MAX_FRAMES: usize = 64;
  pub const MAX_KEYS: usize = 64;
  pub const STATE_CHUNK: usize = 32 * 1024;
  pub const MAX_STATE: usize = 64 * 1024 * 1024;
  pub const OVERHEAD: usize = 48;         // per datagram on the wire (DTLS+SCTP+UDP+IPv4), for budgets
  pub const FINGERPRINT: u64;              // build.rs
  #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
  pub struct Stamp { pub sent_ms: u32, pub echo_ms: u32, pub held_ms: u16 }
  #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
  pub enum Datagram { Client(ClientInputs), Host(HostInputs) }
  pub struct ClientInputs { pub stamp: Stamp, pub tick: u32, pub ack: u32, pub first: u32, pub keys: Vec<u8> }
  pub struct HostInputs { pub stamp: Stamp, pub tick: u32, pub your_ack: u32, pub margin: i16, pub first: u32, pub frames: Vec<Frame> }
  pub struct Frame { pub known: u32, pub changed: u32, pub keys: Vec<u8> }
  pub enum Control {
      Checksum { tick: u32, hash: u64 },
      Late { tick: u32 },
      Desync { tick: u32, host_hash: u64 },
      StateBegin { tick: u32, len: u32, chunks: u16 },
      StateChunk { index: u16, bytes: Vec<u8> },
      Bye,
  }
  pub enum ProtoError { TooLong, Decode, Invalid }
  pub fn encode_datagram(d: &Datagram) -> Vec<u8>;
  pub fn decode_datagram(b: &[u8], slots: usize) -> Result<Datagram, ProtoError>;
  pub fn encode_control(c: &Control) -> Vec<u8>;
  pub fn decode_control(b: &[u8]) -> Result<Control, ProtoError>;
  // frames.rs
  pub fn encode_frames(first: u32, ticks: u32, slots: usize,
                       value: &dyn Fn(u32, usize) -> Option<u8>,
                       base: &dyn Fn(usize) -> Option<u8>) -> Vec<Frame>;
  pub fn decode_frames(first: u32, frames: &[Frame], slots: usize,
                       base: &dyn Fn(usize) -> Option<u8>,
                       out: &mut dyn FnMut(u32, usize, u8)) -> Result<(), ProtoError>;
  ```

Context: spec §3.1 (bandwidth), §5.4 (limits, never panic), §2.2 (fingerprint). `HostInputs.frames[k]` describes tick `first + k`: `known` is the bitmask of slots whose final input the host has for that tick; `changed ⊆ known` marks slots whose value differs from that slot's value at the previous tick, and `keys` holds one byte per set bit of `changed`, in slot order. A known slot not in `changed` repeats the previous tick's value: from the previous frame in this message if that frame knew the slot, otherwise (only for the first frame) from `base(slot)`, the receiver's final value at `first - 1`. The encoder may leave a slot out of `changed` only when the decoder can resolve it that way; tick `first - 1 = -1` (when `first == 0`) has every slot at `0`.

`ClientInputs.keys[k]` is the sender's own slot's input for tick `first + k`. `ack` is the first tick whose frame the sender lacks; `tick` is the sender's next tick to simulate. `Stamp` measures round trips: `sent_ms` is the sender's clock, `echo_ms` the last `sent_ms` it received from this peer and `held_ms` how long ago it received it.

- [ ] **Step 1: Scaffold the crate**

`Cargo.toml` like rl-replay's (template in the workspace), dependencies `serde.workspace = true`, `postcard.workspace = true`; dev-dependency `proptest.workspace = true`; `build = "build.rs"`. `src/lib.rs` starts:

```rust
//! The network wire format (networking spec §2, §3.1, §5.4): datagrams on
//! the unreliable inputs channel, messages on the reliable control channel,
//! the frame delta codec and the simulation fingerprint. Every decoder
//! bounds its input and never panics.
//!
//! `#![no_std]` + `alloc`.
#![no_std]

extern crate alloc;
```

`layers.ron`: `"rl-proto": 4,` in `layers`, and `"rl-proto"` in the `no_std` and `wasm` lists. Workspace `Cargo.toml`: the path dependency line.

- [ ] **Step 2: Write the failing tests**

In `frames.rs` (`#[cfg(test)] mod tests`):

```rust
use super::*;
use alloc::vec;
use alloc::vec::Vec;

/// A table of known inputs: `table[t][s]`.
fn table(rows: &[&[Option<u8>]]) -> Vec<Vec<Option<u8>>> {
    rows.iter().map(|r| r.to_vec()).collect()
}

fn round_trip(t: &[Vec<Option<u8>>], first: u32, base: &[Option<u8>]) -> Vec<Vec<Option<u8>>> {
    let slots = base.len();
    let frames = encode_frames(first, t.len() as u32, slots, &|tick, s| t[(tick - first) as usize][s], &|s| base[s]);
    let mut got = vec![vec![None; slots]; t.len()];
    decode_frames(first, &frames, slots, &|s| base[s], &mut |tick, s, v| got[(tick - first) as usize][s] = Some(v)).unwrap();
    got
}

#[test]
fn frames_round_trip_with_partial_knowledge() {
    let t = table(&[
        &[Some(1), Some(2), None],
        &[Some(1), Some(3), None],
        &[None, Some(3), Some(9)],
        &[Some(4), Some(3), Some(9)],
    ]);
    assert_eq!(round_trip(&t, 10, &[Some(1), Some(0), None]), t);
}

#[test]
fn unchanged_inputs_cost_no_key_bytes() {
    let t: Vec<Vec<Option<u8>>> = (0..8).map(|_| vec![Some(5); 32]).collect();
    let frames = encode_frames(100, 8, 32, &|tick, s| t[(tick - 100) as usize][s], &|_| Some(5));
    assert!(frames.iter().all(|f| f.keys.is_empty() && f.changed == 0));
}

#[test]
fn tick_zero_starts_from_zero_keys() {
    let t = table(&[&[Some(0), Some(7)]]);
    let frames = encode_frames(0, 1, 2, &|_, s| t[0][s], &|_| Some(0));
    assert_eq!(frames[0].changed, 0b10);
}

#[test]
fn an_unresolvable_repeat_is_refused() {
    // Slot 0 claims "unchanged" at the first frame, but the receiver lacks its base.
    let frames = vec![Frame { known: 0b1, changed: 0, keys: vec![] }];
    let r = decode_frames(5, &frames, 1, &|_| None, &mut |_, _, _| {});
    assert_eq!(r, Err(ProtoError::Invalid));
}

#[test]
fn inconsistent_frames_are_refused() {
    let bad = [
        Frame { known: 0b01, changed: 0b10, keys: vec![1] },      // changed outside known
        Frame { known: 0b11, changed: 0b11, keys: vec![1] },      // too few keys
        Frame { known: 1 << 5, changed: 1 << 5, keys: vec![1] },  // slot beyond `slots`
    ];
    for f in bad {
        assert_eq!(decode_frames(1, &[f], 2, &|_| Some(0), &mut |_, _, _| {}), Err(ProtoError::Invalid));
    }
}
```

`tests/fuzz.rs`:

```rust
use proptest::prelude::*;
use rl_proto::*;

fn sample_host() -> Datagram {
    Datagram::Host(HostInputs {
        stamp: Stamp { sent_ms: 1, echo_ms: 2, held_ms: 3 },
        tick: 500,
        your_ack: 490,
        margin: -2,
        first: 490,
        frames: (0..10).map(|i| Frame { known: 0xff, changed: 1 << (i % 8), keys: vec![i as u8] }).collect(),
    })
}

#[test]
fn datagrams_and_controls_round_trip() {
    let d = sample_host();
    assert_eq!(decode_datagram(&encode_datagram(&d), 8), Ok(d));
    let c = Control::StateChunk { index: 3, bytes: vec![7; 100] };
    assert_eq!(decode_control(&encode_control(&c)), Ok(c));
}

#[test]
fn limits_are_enforced() {
    assert_eq!(decode_datagram(&vec![0u8; MAX_DATAGRAM + 1], 32), Err(ProtoError::TooLong));
    let big = Control::StateChunk { index: 0, bytes: vec![0; STATE_CHUNK + 1] };
    assert_eq!(decode_control(&encode_control(&big)), Err(ProtoError::Invalid));
    let many = Datagram::Client(ClientInputs { stamp: Stamp::default(), tick: 0, ack: 0, first: 0, keys: vec![0; MAX_KEYS + 1] });
    assert_eq!(decode_datagram(&encode_datagram(&many), 32), Err(ProtoError::Invalid));
}

#[test]
fn the_fingerprint_is_set() {
    assert_ne!(FINGERPRINT, 0);
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn decode_never_panics(bytes in prop::collection::vec(any::<u8>(), 0..2000)) {
        let _ = decode_datagram(&bytes, 32);
        let _ = decode_control(&bytes);
    }

    #[test]
    fn mutated_datagrams_never_panic(edits in prop::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..6)) {
        let mut b = encode_datagram(&sample_host());
        for (i, v) in &edits { let i = i.index(b.len()); b[i] = *v; }
        if let Ok(Datagram::Host(h)) = decode_datagram(&b, 8) {
            let _ = decode_frames(h.first, &h.frames, 8, &|_| Some(0), &mut |_, _, _| {});
        }
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo nextest run -p rl-proto`
Expected: compile errors (items missing).

- [ ] **Step 4: Implement**

`lib.rs`: the constants, types (all `#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]`), `ProtoError` (`Clone, Copy, Debug, PartialEq, Eq`). Encoding is `postcard::to_allocvec` (cannot fail for these types; `expect` with a reason is acceptable on the encode side only). `decode_datagram`: `b.len() > MAX_DATAGRAM` → `TooLong`; `postcard::from_bytes` → `Decode`; then `validate_datagram(&d, slots)` → `Invalid` when: `keys.len() > MAX_KEYS`; `frames.len() > MAX_FRAMES`; any frame with `changed & !known != 0`, `known >> slots != 0` (for `slots < 32`), or `keys.len() != changed.count_ones()`. `decode_control`: `> MAX_CONTROL` → `TooLong`; decode; `StateChunk` bytes `> STATE_CHUNK`, `StateBegin` with `len as usize > MAX_STATE` or `chunks as usize != (len as usize).div_ceil(STATE_CHUNK)` → `Invalid`.

`frames.rs`:

```rust
use alloc::vec::Vec;

use crate::{Frame, ProtoError};

pub fn encode_frames(
    first: u32,
    ticks: u32,
    slots: usize,
    value: &dyn Fn(u32, usize) -> Option<u8>,
    base: &dyn Fn(usize) -> Option<u8>,
) -> Vec<Frame> {
    let mut out = Vec::with_capacity(ticks as usize);
    // What the receiver will know for the previous tick, per slot.
    let mut prev: [Option<u8>; 32] = [None; 32];
    for (s, p) in prev.iter_mut().enumerate().take(slots) {
        *p = if first == 0 { Some(0) } else { base(s) };
    }
    for k in 0..ticks {
        let tick = first + k;
        let (mut known, mut changed, mut keys) = (0u32, 0u32, Vec::new());
        let mut next = [None; 32];
        for s in 0..slots {
            if let Some(v) = value(tick, s) {
                known |= 1 << s;
                if prev[s] != Some(v) {
                    changed |= 1 << s;
                    keys.push(v);
                }
                next[s] = Some(v);
            }
        }
        prev = next;
        out.push(Frame { known, changed, keys });
    }
    out
}

pub fn decode_frames(
    first: u32,
    frames: &[Frame],
    slots: usize,
    base: &dyn Fn(usize) -> Option<u8>,
    out: &mut dyn FnMut(u32, usize, u8),
) -> Result<(), ProtoError> {
    let mut prev: [Option<u8>; 32] = [None; 32];
    for (s, p) in prev.iter_mut().enumerate().take(slots) {
        *p = if first == 0 { Some(0) } else { base(s) };
    }
    for (k, f) in frames.iter().enumerate() {
        let valid = slots >= 32 || f.known >> slots == 0;
        if !valid || f.changed & !f.known != 0 || f.keys.len() != f.changed.count_ones() as usize {
            return Err(ProtoError::Invalid);
        }
        let tick = first.checked_add(k as u32).ok_or(ProtoError::Invalid)?;
        let mut keys = f.keys.iter();
        let mut next = [None; 32];
        for s in 0..slots {
            if f.known & (1 << s) == 0 {
                continue;
            }
            let v = if f.changed & (1 << s) != 0 {
                *keys.next().expect("counted above")
            } else {
                prev[s].ok_or(ProtoError::Invalid)?
            };
            next[s] = Some(v);
            out(tick, s, v);
        }
        prev = next;
    }
    Ok(())
}
```

`out` may be called for a frame before a later frame of the same message turns out invalid; callers decode into a scratch list first and apply only on `Ok` (Task 6 does).

`fingerprint_impl.rs` (plain std-free Rust, `include!`d by `build.rs` and unit-tested from `lib.rs` under `#[cfg(test)]` via `#[path]`):

```rust
/// FNV-1a 64 over `(relative path, contents)` of every `.rs` file, sorted by path.
pub fn fingerprint(files: &[(&str, &[u8])]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |b: &[u8]| {
        for &x in b {
            h = (h ^ x as u64).wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    for (path, body) in files {
        eat(path.as_bytes());
        eat(&[0]);
        eat(body);
        eat(&[0]);
    }
    h
}
```

`build.rs`: walks `../rl-math/src`, `../rl-data/src`, `../rl-terrain/src`, `../rl-sim/src`, `../rl-modes/src` and this crate's `src/lib.rs` (relative to `CARGO_MANIFEST_DIR`), collects `.rs` files with paths relative to `crates/` using `/` separators, sorts them, calls `fingerprint`, writes `$OUT_DIR/fingerprint.rs` containing `0x{:016x}`, and prints `cargo:rerun-if-changed=` for each directory and file. In `lib.rs`: `pub const FINGERPRINT: u64 = include!(concat!(env!("OUT_DIR"), "/fingerprint.rs"));`. A unit test in `lib.rs` checks `fingerprint` changes when one byte of one file changes and when two files swap names. Line endings: read files as bytes and strip `\r` so a Windows checkout gives the same fingerprint.

- [ ] **Step 5: Run the tests**

Run: `cargo nextest run -p rl-proto && cargo build --target wasm32-unknown-unknown -p rl-proto && cargo xtask layers`
Expected: pass.

- [ ] **Step 6: Document and commit**

`crates/rl-proto/CLAUDE.md` (template: rl-replay's): purpose (spec §2, §3.1, §5.4), "**Layer 4, `#![no_std]` + `alloc`, wasm32.** Depends on serde and postcard only (keys are plain bytes).", the datagram and control messages, the frame delta rule, the limits and "never panic: every decoder bounds and validates; `tests/fuzz.rs`", the fingerprint ("changes whenever simulation source changes; peers only play with an equal fingerprint, spec §2.2"), `Test: cargo nextest run -p rl-proto`. `cargo xtask check`, then:

```bash
git add Cargo.toml xtask/layers.ron crates/rl-proto
git commit -m "rl-proto: the netcode wire format, frame deltas and the simulation fingerprint"
```

---
### Task 5: rl-netcode: the `Simulation` trait, `ToySim` and the network model

**Files:**
- Create: `crates/rl-netcode/{Cargo.toml, CLAUDE.md, src/lib.rs, src/sim.rs, src/netsim.rs, src/rng.rs}`, `crates/rl-netcode/tests/netsim.rs`
- Modify: `Cargo.toml` (workspace dependency `rl-netcode`), `xtask/layers.ron` (`"rl-netcode": 4`, `no_std`, `wasm`, and `same_layer` `("rl-netcode", "rl-proto")` with a comment "the session speaks the wire format")
- Test: `crates/rl-netcode/tests/netsim.rs`, inline tests in `sim.rs`

**Interfaces:**
- Consumes: `rl_sim::Keys` (rl-netcode depends on rl-sim), `rl_proto` (constants).
- Produces (Tasks 6-11):
  ```rust
  pub type PeerId = u16;
  #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
  pub enum Channel { Unreliable, Reliable }
  pub struct StateRejected;                         // Debug, PartialEq
  pub trait Simulation {
      type Snapshot;
      fn tick(&mut self, keys: &[Keys]);
      fn save(&mut self) -> Self::Snapshot;
      fn restore(&mut self, s: &Self::Snapshot);
      fn forget_before(&mut self, s: &Self::Snapshot);
      fn checksum(&self) -> u64;
      fn encode_state(&self) -> Vec<u8>;
      fn decode_state(&mut self, bytes: &[u8]) -> Result<(), StateRejected>;
  }
  pub struct ToySim { /* slots, positions, tick, hash */ }
  impl ToySim { pub fn new(slots: usize) -> Self; pub fn tick_count(&self) -> u32; }
  // rng.rs
  pub struct Rng(u64);   // splitmix64
  impl Rng { pub fn new(seed: u64) -> Self; pub fn next_u64(&mut self) -> u64; pub fn unit(&mut self) -> f64; pub fn below(&mut self, n: u64) -> u64; pub fn fork(&mut self) -> Rng; }
  // netsim.rs
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub struct Profile { pub name: &'static str, pub ping_ms: u32, pub jitter_ms: u32, pub loss: f64, pub burst: f64, pub duplicate: f64 }
  impl Profile {
      pub const LAN: Profile; pub const GOOD: Profile; pub const TYPICAL: Profile; pub const BAD: Profile; pub const AWFUL: Profile;
      pub const ALL: [Profile; 5];
      pub fn by_name(name: &str) -> Option<Profile>;
      pub fn rtt_ms(&self) -> u32;   // ping_ms
  }
  pub struct Network { /* now, links, in-flight heap, per-peer stats */ }
  #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
  pub struct LinkStats { pub datagrams: u64, pub bytes: u64, pub dropped: u64 }
  impl Network {
      pub fn new(seed: u64) -> Self;
      pub fn connect(&mut self, a: PeerId, b: PeerId, profile: Profile);
      pub fn set_profile(&mut self, a: PeerId, b: PeerId, profile: Profile);
      pub fn outage(&mut self, a: PeerId, b: PeerId, from_us: u64, until_us: u64);
      pub fn send(&mut self, now_us: u64, from: PeerId, to: PeerId, ch: Channel, bytes: Vec<u8>);
      pub fn next_delivery(&self) -> Option<u64>;
      pub fn deliver_due(&mut self, now_us: u64, out: &mut dyn FnMut(PeerId, PeerId, Channel, Vec<u8>));
      pub fn received(&self, peer: PeerId) -> LinkStats;   // what reached `peer`
      pub fn sent(&self, peer: PeerId) -> LinkStats;
  }
  ```

Context: spec §9.1. The model is deterministic from its seed (no clock, no OS randomness). Profiles (decision N3: a profile's latency is the ping, each direction takes half, with half the jitter):

| name | ping ms | jitter ms (±, on the ping) | loss | burst (mean lost run) | duplicate |
|---|---|---|---|---|---|
| `lan` | 1 | 0 | 0 | 1 | 0 |
| `good` | 30 | 5 | 0 | 1 | 0 |
| `typical` | 80 | 20 | 0.01 | 1 | 0 |
| `bad` | 150 | 50 | 0.05 | 2 | 0.01 |
| `awful` | 300 | 100 | 0.15 | 4 | 0.02 |

Unreliable datagrams: each direction keeps a Gilbert-Elliott state; in the bad state every datagram is lost; from good the chance to turn bad is `loss / (burst * (1 - loss))`, from bad the chance to recover is `1 / burst` (mean loss = `loss`, mean run = `burst`). A datagram that survives takes `ping/2 + uniform(-jitter/2, +jitter/2)` ms (at least 0), so jitter reorders; with chance `duplicate` a second copy follows with its own delay. Reliable messages arrive in order per direction: each takes `ping/2 + jitter draw`, plus `ping` for every loss draw it suffers (a retransmission), and never before the previous reliable message in that direction. During an outage no unreliable datagram arrives and reliable ones wait until the outage ends plus their one-way delay. Ties in delivery time break by send order.

- [ ] **Step 1: Scaffold**

`Cargo.toml`: dependencies `rl-sim.workspace = true`, `rl-proto.workspace = true`; dev-dependencies `proptest.workspace = true`. `lib.rs`:

```rust
//! The rollback netcode (networking spec §3): a sans-IO session that takes
//! messages and the time and returns messages and ticks to run, generic over
//! the [`Simulation`] it drives, and a deterministic network model to test
//! it in one process ([`netsim`]).
//!
//! `#![no_std]` + `alloc`.
#![no_std]

extern crate alloc;

pub mod netsim;
mod rng;
mod sim;

pub use rng::Rng;
pub use sim::{Simulation, StateRejected, ToySim};

/// A peer's address within one session.
pub type PeerId = u16;

/// The two data channels of every connection (spec §2.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Channel {
    /// Inputs: unordered, may be lost or duplicated.
    Unreliable,
    /// Everything else: ordered, never lost.
    Reliable,
}
```

- [ ] **Step 2: Write the failing tests**

In `sim.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn k(v: u8) -> Keys {
        Keys(v)
    }

    #[test]
    fn toy_sim_restores_and_transfers() {
        let mut a = ToySim::new(3);
        a.tick(&[k(1), k(2), k(4)]);
        let s = a.save();
        let h = a.checksum();
        a.tick(&[k(8), k(8), k(8)]);
        assert_ne!(a.checksum(), h);
        a.restore(&s);
        assert_eq!(a.checksum(), h);
        let mut b = ToySim::new(3);
        b.decode_state(&a.encode_state()).unwrap();
        assert_eq!(b.checksum(), a.checksum());
        assert_eq!(b.decode_state(&[1, 2, 3]), Err(StateRejected));
    }

    #[test]
    fn toy_sim_depends_on_every_key_and_the_order() {
        let mut a = ToySim::new(2);
        let mut b = ToySim::new(2);
        a.tick(&[k(1), k(2)]);
        b.tick(&[k(2), k(1)]);
        assert_ne!(a.checksum(), b.checksum());
    }
}
```

`tests/netsim.rs`:

```rust
use rl_netcode::netsim::{Network, Profile};
use rl_netcode::Channel;

fn run(profile: Profile, n: u64, ch: Channel) -> Vec<(u64, u64)> {
    let mut net = Network::new(42);
    net.connect(0, 1, profile);
    let mut got = Vec::new();
    for i in 0..n {
        let now = i * 1000;
        net.send(now, 0, 1, ch, i.to_le_bytes().to_vec());
        net.deliver_due(now, &mut |_, _, _, b| got.push((u64::from_le_bytes(b.try_into().unwrap()), now)));
    }
    let end = n * 1000 + 10_000_000;
    net.deliver_due(end, &mut |_, _, _, b| got.push((u64::from_le_bytes(b.try_into().unwrap()), end)));
    got
}

#[test]
fn loss_matches_the_profile() {
    let got = run(Profile::BAD, 100_000, Channel::Unreliable);
    let distinct: std::collections::BTreeSet<u64> = got.iter().map(|g| g.0).collect();
    let lost = 1.0 - distinct.len() as f64 / 100_000.0;
    assert!((lost - 0.05).abs() < 0.01, "lost {lost}");
    assert!(got.len() > distinct.len(), "duplicates appear");
}

#[test]
fn latency_is_half_the_ping() {
    let mut net = Network::new(7);
    net.connect(0, 1, Profile::TYPICAL);
    let mut total = 0u64;
    let mut count = 0u64;
    for i in 0..20_000u64 {
        let sent = i * 1000;
        net.send(sent, 0, 1, Channel::Unreliable, sent.to_le_bytes().to_vec());
        // Step finely so the delivery time is the true arrival time.
        for t in sent..sent + 1000 {
            if net.next_delivery().is_some_and(|d| d <= t) {
                net.deliver_due(t, &mut |_, _, _, b| {
                    total += t - u64::from_le_bytes(b.try_into().unwrap());
                    count += 1;
                });
            }
        }
    }
    net.deliver_due(u64::MAX / 2, &mut |_, _, _, _| {});
    let mean_ms = total as f64 / count as f64 / 1000.0;
    assert!((mean_ms - 40.0).abs() < 2.0, "mean one-way {mean_ms} ms");
}

#[test]
fn reliable_messages_arrive_in_order_and_all() {
    let got = run(Profile::AWFUL, 5_000, Channel::Reliable);
    let order: Vec<u64> = got.iter().map(|g| g.0).collect();
    assert_eq!(order, (0..5_000).collect::<Vec<_>>());
}

#[test]
fn the_same_seed_gives_the_same_deliveries() {
    assert_eq!(run(Profile::BAD, 3_000, Channel::Unreliable), run(Profile::BAD, 3_000, Channel::Unreliable));
}

#[test]
fn an_outage_drops_datagrams_and_delays_messages() {
    let mut net = Network::new(1);
    net.connect(0, 1, Profile::LAN);
    net.outage(0, 1, 0, 50_000);
    net.send(10_000, 0, 1, Channel::Unreliable, vec![1]);
    net.send(10_000, 0, 1, Channel::Reliable, vec![2]);
    let mut got = Vec::new();
    net.deliver_due(49_000, &mut |_, _, _, b| got.push(b[0]));
    assert!(got.is_empty());
    net.deliver_due(60_000, &mut |_, _, _, b| got.push(b[0]));
    assert_eq!(got, vec![2]);
}

#[test]
fn profiles_are_found_by_name() {
    for p in Profile::ALL {
        assert_eq!(Profile::by_name(p.name), Some(p));
    }
    assert_eq!(Profile::by_name("nope"), None);
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo nextest run -p rl-netcode`
Expected: compile errors.

- [ ] **Step 4: Implement**

`rng.rs`: splitmix64 (`next_u64`: `self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15)` then the finaliser), `unit` = `(next_u64() >> 11) as f64 / (1u64 << 53) as f64`, `below(n)` = `next_u64() % n` (bias irrelevant here), `fork` = `Rng::new(self.next_u64())`.

`sim.rs`:

```rust
use alloc::vec::Vec;

use rl_sim::Keys;

/// A rejected world state (networking spec §5.4: never panic, refuse).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StateRejected;

/// What the session drives. One tick takes one `Keys` per slot.
pub trait Simulation {
    type Snapshot;
    fn tick(&mut self, keys: &[Keys]);
    fn save(&mut self) -> Self::Snapshot;
    fn restore(&mut self, s: &Self::Snapshot);
    /// Snapshots older than `s` will not be restored again.
    fn forget_before(&mut self, s: &Self::Snapshot);
    /// Compared between peers every few confirmed ticks.
    fn checksum(&self) -> u64;
    fn encode_state(&self) -> Vec<u8>;
    fn decode_state(&mut self, bytes: &[u8]) -> Result<(), StateRejected>;
}

/// A tiny deterministic simulation for the session's own tests: each slot
/// walks by its keys, and a hash chains every tick's inputs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToySim {
    tick: u32,
    pos: Vec<i64>,
    hash: u64,
}

impl ToySim {
    pub fn new(slots: usize) -> Self {
        Self { tick: 0, pos: alloc::vec![0; slots], hash: 0xcbf2_9ce4_8422_2325 }
    }

    pub fn tick_count(&self) -> u32 {
        self.tick
    }
}

impl Simulation for ToySim {
    type Snapshot = ToySim;

    fn tick(&mut self, keys: &[Keys]) {
        for (i, p) in self.pos.iter_mut().enumerate() {
            let k = keys.get(i).copied().unwrap_or_default().0;
            *p += (k & 0x0f) as i64 - (k >> 4) as i64;
            self.hash = (self.hash ^ (k as u64 | (i as u64) << 8)).wrapping_mul(0x0000_0100_0000_01b3);
        }
        self.hash = (self.hash ^ self.tick as u64).wrapping_mul(0x0000_0100_0000_01b3);
        self.tick += 1;
    }

    fn save(&mut self) -> ToySim {
        self.clone()
    }

    fn restore(&mut self, s: &ToySim) {
        self.clone_from(s);
    }

    fn forget_before(&mut self, _: &ToySim) {}

    fn checksum(&self) -> u64 {
        self.pos.iter().fold(self.hash, |h, &p| (h ^ p as u64).wrapping_mul(0x0000_0100_0000_01b3))
    }

    fn encode_state(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.tick.to_le_bytes());
        out.extend_from_slice(&self.hash.to_le_bytes());
        out.extend_from_slice(&(self.pos.len() as u32).to_le_bytes());
        for p in &self.pos {
            out.extend_from_slice(&p.to_le_bytes());
        }
        out
    }

    fn decode_state(&mut self, b: &[u8]) -> Result<(), StateRejected> {
        let n = self.pos.len();
        if b.len() != 16 + 8 * n || u32::from_le_bytes(b[12..16].try_into().unwrap()) as usize != n {
            return Err(StateRejected);
        }
        self.tick = u32::from_le_bytes(b[0..4].try_into().unwrap());
        self.hash = u64::from_le_bytes(b[4..12].try_into().unwrap());
        for (i, p) in self.pos.iter_mut().enumerate() {
            *p = i64::from_le_bytes(b[16 + 8 * i..24 + 8 * i].try_into().unwrap());
        }
        Ok(())
    }
}
```

(The `try_into().unwrap()` calls follow a length check, so they cannot fail; keep them that way.)

`netsim.rs`: the profiles table above as `const`s (`burst: 1.0` means independent loss; GE transition chance from good is `loss / (burst * (1.0 - loss))`, guarded for `loss == 0`). `Network` holds `BTreeMap<(PeerId, PeerId), Link>` (one `Link` per direction: profile, GE state, last reliable delivery time, outage window, an `Rng` forked from the network's in `connect` order), an `alloc::collections::BinaryHeap<Reverse<(u64 deliver_at, u64 seq, PeerId from, PeerId to, Channel)>>` plus a `BTreeMap<u64 seq, Vec<u8>>` for payloads (or put the bytes in the heap entry with an `Ord` that ignores them), and per-peer `LinkStats` for sent and received. `send` to an unconnected pair panics in debug (`debug_assert!`) and drops in release — it is a harness programming error. `deliver_due` pops every entry with `deliver_at <= now` in `(deliver_at, seq)` order and counts bytes and datagrams in the receiver's stats.

- [ ] **Step 5: Run the tests**

Run: `cargo nextest run -p rl-netcode && cargo build --target wasm32-unknown-unknown -p rl-netcode && cargo xtask layers`
Expected: pass.

- [ ] **Step 6: Document and commit**

`crates/rl-netcode/CLAUDE.md`: purpose (spec §3, §9.1); "**Layer 4, `#![no_std]` + `alloc`, wasm32.** Depends on rl-sim (keys, the world adapter of `world_sim.rs`), rl-proto (same layer: the wire format)."; "**Sans-IO**: no sockets, no clock, no threads; time is an argument (`now_us`)."; the `Simulation` trait and `ToySim`; the network model and its profiles table; `Test: cargo nextest run -p rl-netcode`. `cargo xtask check`, then:

```bash
git add Cargo.toml xtask/layers.ron crates/rl-netcode
git commit -m "rl-netcode: the simulation trait, a toy simulation and a deterministic network model"
```

---
### Task 6: rl-netcode: the session core and the harness

**Files:**
- Create: `crates/rl-netcode/src/{session.rs, inputs.rs, harness.rs, toy.rs}`, `crates/rl-netcode/tests/session.rs`
- Modify: `crates/rl-netcode/src/lib.rs` (modules, re-exports), `crates/rl-netcode/CLAUDE.md`
- Test: inline tests in `inputs.rs`; `tests/session.rs` (toy simulation over the network model)

**Interfaces:**
- Consumes: Task 4's `rl_proto::{Datagram, ClientInputs, HostInputs, Stamp, encode_datagram, decode_datagram, encode_frames, decode_frames, MAX_DATAGRAM, MAX_FRAMES, MAX_KEYS, OVERHEAD}`; Task 5's `Simulation`, `ToySim`, `Network`, `Profile`, `PeerId`, `Channel`, `Rng`.
- Produces (Tasks 7-11):
  ```rust
  pub const TICK_US: u64 = 10_000;
  pub const FILL_WINDOW: u32 = 12;
  pub const MAX_PREDICTION: u32 = 28;
  pub const CHECKSUM_EVERY: u32 = 10;
  pub const MAX_CATCHUP: u32 = 10;
  pub fn input_delay(rtt_ms: u32) -> u8;                 // min(3, rtt_ms / 40)
  #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum Seat { Player(u8), Spectator }
  #[derive(Clone, Debug)] pub struct Config { pub slots: u8, pub input_delay: u8, pub fill_window: u32, pub max_prediction: u32, pub checksum_every: u32 }
  impl Config { pub fn new(slots: u8, input_delay: u8) -> Config }
  #[derive(Clone, Debug)] pub enum Role { Host { peers: Vec<(PeerId, Seat)> }, Client { host: PeerId, slot: u8 }, Spectator { host: PeerId } }
  #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub struct TickInfo { pub tick: u32, pub resim: bool, pub all_final: bool }
  pub trait Driver<S> {
      fn local_inputs(&mut self, sim: &S, tick: u32, slots: &[u8], out: &mut [Keys]);
      fn simulated(&mut self, _sim: &S, _info: TickInfo) {}
  }
  #[derive(Clone, Debug, PartialEq, Eq)]
  pub enum Event {
      Rollback { from: u32, to: u32 },
      Stalled { tick: u32 },
      Malformed { peer: PeerId },
      Inconsistent { peer: PeerId, tick: u32, slot: u8 },
  }  // Tasks 7 and 8 add variants
  #[derive(Clone, Debug, Default)]
  pub struct Stats { pub rollbacks: u64, pub depth_hist: [u64; 64], pub resim_ticks: u64, pub stalls: u64,
                     pub own_corrections: u64, pub malformed: u64, pub rtt_ms: Option<u32>,
                     pub datagrams_in: u64, pub bytes_in: u64, pub datagrams_out: u64, pub bytes_out: u64 }
  pub struct Session<S: Simulation> { .. }
  impl<S: Simulation> Session<S> {
      pub fn new(cfg: Config, role: Role, sim: S, now_us: u64) -> Self;
      pub fn receive(&mut self, from: PeerId, ch: Channel, bytes: &[u8], now_us: u64);
      pub fn advance(&mut self, now_us: u64, driver: &mut dyn Driver<S>);
      pub fn transmit(&mut self, out: &mut dyn FnMut(PeerId, Channel, Vec<u8>));
      pub fn drain_events(&mut self) -> Vec<Event>;
      pub fn sim(&self) -> &S;
      pub fn sim_mut(&mut self) -> &mut S;
      pub fn current_tick(&self) -> u32;      // the next tick to simulate
      pub fn frontier(&self) -> u32;          // ticks below it have every slot's final input
      pub fn final_input(&self, tick: u32, slot: u8) -> Option<Keys>;
      pub fn input_log(&self) -> &InputLog;
      pub fn stats(&self) -> &Stats;
      pub fn config(&self) -> &Config;
  }
  // inputs.rs
  pub struct InputLog { /* per tick: [u8; 32] values, u32 known mask */ }
  impl InputLog {
      pub fn new(slots: u8) -> Self;
      pub fn get(&self, tick: u32, slot: u8) -> Option<u8>;
      pub fn set(&mut self, tick: u32, slot: u8, v: u8) -> SetResult;   // New | Same | Conflict | Gap
      pub fn known_upto(&self, slot: u8) -> u32;   // first tick this slot lacks
      pub fn frontier(&self) -> u32;               // min over slots
      pub fn predicted(&self, tick: u32, slot: u8) -> u8;  // final, else the last final before it, else 0
      pub fn row(&self, tick: u32) -> Option<[u8; 32]>;    // only below the frontier
      pub fn slots(&self) -> u8;
  }
  // harness.rs
  pub struct PeerPlan { pub id: PeerId, pub seat: Seat, pub profile: Profile, pub clock_ppm: i32, pub input_delay: u8, pub frame_us: u64, pub join_tick: Option<u32> }
  pub struct Outage { pub peer: PeerId, pub from_tick: u32, pub until_tick: u32 }
  pub struct Scenario { pub seed: u64, pub slots: u8, pub peers: Vec<PeerPlan>, pub ticks: u32, pub settle_ticks: u32,
                        pub outages: Vec<Outage>, pub expect_no_stalls: bool }
  pub trait Factory<S: Simulation> {
      fn sim(&mut self, peer: PeerId) -> S;
      fn driver(&mut self, peer: PeerId, seat: Option<Seat>) -> Box<dyn Driver<S>>;   // None = the host
  }
  pub struct Report { pub host_log: Vec<[u8; 32]>, pub sims: BTreeMap<PeerId, BTreeMap<u32, u64>>,
                      pub stats: BTreeMap<PeerId, Stats>, pub events: BTreeMap<PeerId, Vec<Event>>,
                      pub received: BTreeMap<PeerId, LinkStats>, pub duration_us: u64 }
  #[derive(Clone, Debug, PartialEq, Eq)] pub struct Failure { pub tick: u32, pub what: String }
  pub fn run<S: Simulation>(sc: &Scenario, f: &mut dyn Factory<S>) -> Result<Report, Failure>;
  pub fn verdict<S: Simulation>(sc: &Scenario, r: &Report, f: &mut dyn Factory<S>) -> Result<(), Failure>;
  pub const HOST: PeerId = 0;
  // toy.rs
  pub struct ToyFactory { pub slots: u8, pub seed: u64 }   // drivers press seeded pseudo-random keys
  impl Factory<ToySim> for ToyFactory
  ```

Context: spec §3.1-§3.3, §3.6. This task builds the session with the host's input ordering, prediction and rollback, the input delay, and spectators from tick 0; its clock simply runs at the peer's own pace (the time sync, the fill rule, stalls by prediction limit and timeouts come in Task 7; checksums and state transfer in Task 8). The harness drives N sessions in virtual time over the network model and checks the verdict (spec §9.1).

**How the session works** (implement exactly this; the tests pin it):

1. **Slots and seats.** `Config::slots` worms, slot `i` = worm `i`. The host's *local slots* are those no peer holds (its human and the bots, spec §3.6); a client's local slot is its own; a spectator has none. `Config::input_delay` (`d`) applies to clients; the host uses `0` for its local slots (decision N2: fixed per match).
2. **The input log.** `InputLog` holds, per tick from 0, one value per slot and a known mask. Every slot's knowledge is a contiguous prefix (`known_upto(slot)`): a value is final, and set only at `known_upto` (`SetResult::Gap` beyond it, `Same` or `Conflict` below it). The frontier is the minimum over slots: every tick below it is *confirmed*.
3. **Prediction.** The keys a peer uses for slot `s` at tick `t`: the final value if known; for a client's own slot, its own (provisional) input for `t` if generated; otherwise the slot's last final value before `t` (`predicted`), or `0` with none.
4. **Generating local inputs.** Before simulating a new tick `t`, the session calls `driver.local_inputs(sim, t + d, slots, out)` once (with the state before `t`); the host stores the results for its local slots as final at tick `t` (`d = 0`); a client stores its own slot's value as provisional at `t + d` and queues it to send. At the start a client's own ticks `0..d` hold `Keys(0)`, provisional, sent like any other.
5. **Simulating a new tick.** Save a snapshot (before `t`), build `keys[s]` per 3, `sim.tick(&keys)`, remember the keys used for `t` (for misprediction checks), `driver.simulated(sim, TickInfo { tick: t, resim: false, all_final })`, `current += 1`. A spectator simulates only below the frontier (never predicts).
6. **Rollback.** When a final value arrives for `(t, s)` with `t < current` and it differs from the keys used for `t` (including a client's own slot overridden by the host), set `rollback_from = min(rollback_from, t)`. At the start of the next `advance`, restore the snapshot before `rollback_from`, re-simulate `rollback_from..current` (calling `simulated` with `resim: true`), record the depth `current - rollback_from` in `depth_hist` (clamped to 63) and push `Event::Rollback { from, to: current }`. A client's own slot overridden counts in `own_corrections`.
7. **Snapshots.** Keep one per tick in `frontier..current` (the state before each unconfirmed tick, plus the one before the frontier tick, which is the state after the last confirmed tick). After each `advance`, drop the older ones and call `sim.forget_before(oldest kept)`.
8. **Host messages.** After each `advance` in which it simulated at least one tick, the host builds one `HostInputs` per connected peer: `first` = that peer's `ack` (from its last `ClientInputs`), frames for ticks `first..` up to the last tick the host knows anything for, at most `MAX_FRAMES` and trimmed until the encoded datagram fits `MAX_DATAGRAM`; `base(s)` = the final value at `first - 1`; `your_ack` = `known_upto(peer's slot)` (spectators: the frontier); `tick` = `current`; `margin = 0` (Task 7 fills it); `stamp` (below).
9. **Client messages.** After each `advance` in which it simulated or stalled, a client sends one `ClientInputs`: `first` = the host's last `your_ack`, its own provisional inputs from `first` on (at most `MAX_KEYS`), `ack` = its frontier, `tick` = `current`, a `stamp`. A spectator sends the same with no keys.
10. **Receiving.** `decode_datagram` (`Malformed` + `malformed += 1` on error, nothing else changes). Host from a client: append each key at `first + k` to the client's slot (`Gap` → ignore the rest; `Conflict` → ignore: the first value stays final), record the client's `ack`. Client or spectator from the host: decode the frames into a scratch list (`decode_frames` with `base(s) = log.get(first - 1, s)`, `0` for `first == 0`); on `Ok`, apply each value (`Conflict` → `Event::Inconsistent` — the host changed a final input, which cannot happen with a correct host), check mispredictions (6), and record `your_ack`.
11. **Stamps.** Every datagram carries `Stamp { sent_ms: now/1000, echo_ms, held_ms }` with `echo_ms` the last `sent_ms` received from that peer and `held_ms` the milliseconds since; on receipt, `rtt = now_ms - echo_ms - held_ms` (skip when no echo yet) updates `rtt_ms` as an exponential average (`rtt += (sample - rtt) / 8`).
12. **Pace (until Task 7).** `advance(now)` runs `floor((now - start_us) / TICK_US)` minus the ticks already run, at most `MAX_CATCHUP` per call; a non-spectator stops early (`Event::Stalled { tick }`, `stalls += 1`, once per call) if `current - frontier >= max_prediction`.
13. **Statistics.** `datagrams_in/bytes_in` count received payload bytes plus `OVERHEAD` per datagram; likewise out.

**The harness** (`run`): peer `HOST` (0) is the host; `sc.peers` are clients and spectators (`join_tick: None` = present from tick 0; mid-match joining comes in Task 8 and is rejected here with a `Failure`). Every peer is connected to the host with its plan's profile. Virtual time advances in 1 ms steps; at each step the network delivers what is due (each delivery → `receive`), then every peer whose next frame time has come advances (its local clock: `now * (1_000_000 + clock_ppm) / 1_000_000`) with its driver, transmits, and drains its events into the report; frames repeat every `frame_us` of local time. Outages call `Network::outage` for both directions from the tick's time to the end tick's time. The run ends when the host's `current` reaches `ticks + settle_ticks`; during the last `settle_ticks` every link switches to `Profile::LAN` so everything confirms. The driver wrapper records, for every `simulated` call with `tick % 10 == 9`, `sim.checksum()` into `sims[peer][tick]` (overwriting: the last simulation of a tick counts). `host_log` is the host's log rows below its frontier.

`verdict` checks, returning the first `Failure`:
- every peer's frontier reached `ticks`;
- every peer's log rows below `ticks` equal the host's (no two peers confirmed different inputs);
- an offline replay — `f.sim(HOST)` ticked with `host_log` rows `0..ticks` — gives, at each `t % 10 == 9 < ticks`, the checksum every peer recorded for `t` (spectators included);
- with `expect_no_stalls`, no player peer reported `Stalled`;
- no `Malformed` or `Inconsistent` events.

- [ ] **Step 1: Write the failing tests**

`inputs.rs` tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knowledge_is_a_prefix_and_final() {
        let mut log = InputLog::new(2);
        assert_eq!(log.set(1, 0, 5), SetResult::Gap);
        assert_eq!(log.set(0, 0, 5), SetResult::New);
        assert_eq!(log.set(1, 0, 6), SetResult::New);
        assert_eq!(log.set(1, 0, 6), SetResult::Same);
        assert_eq!(log.set(1, 0, 7), SetResult::Conflict);
        assert_eq!(log.known_upto(0), 2);
        assert_eq!(log.frontier(), 0);
        log.set(0, 1, 1);
        assert_eq!(log.frontier(), 1);
        assert_eq!(log.row(0), Some({ let mut r = [0; 32]; r[0] = 5; r[1] = 1; r }));
        assert_eq!(log.row(1), None);
    }

    #[test]
    fn prediction_repeats_the_last_final_input() {
        let mut log = InputLog::new(1);
        assert_eq!(log.predicted(3, 0), 0);
        log.set(0, 0, 9);
        log.set(1, 0, 4);
        assert_eq!(log.predicted(1, 0), 4);
        assert_eq!(log.predicted(7, 0), 4);
    }
}
```

`tests/session.rs`:

```rust
//! The session over the network model with the toy simulation (spec §9.1):
//! the verdict holds for every profile and many seeds.

use rl_netcode::harness::{self, Outage, PeerPlan, Scenario, HOST};
use rl_netcode::netsim::Profile;
use rl_netcode::toy::ToyFactory;
use rl_netcode::{input_delay, Seat};

fn scenario(seed: u64, profile: Profile, clients: u8, spectators: u8) -> Scenario {
    let slots = clients + 2; // two host-local slots (a human and a bot)
    let mut peers = Vec::new();
    for c in 0..clients {
        peers.push(PeerPlan {
            id: 1 + c as u16,
            seat: Seat::Player(2 + c),
            profile,
            clock_ppm: 0,
            input_delay: input_delay(profile.ping_ms),
            frame_us: 6_944,
            join_tick: None,
        });
    }
    for s in 0..spectators {
        peers.push(PeerPlan {
            id: 100 + s as u16,
            seat: Seat::Spectator,
            profile,
            clock_ppm: 0,
            input_delay: 0,
            frame_us: 16_667,
            join_tick: None,
        });
    }
    Scenario { seed, slots, peers, ticks: 1500, settle_ticks: 200, outages: Vec::new(), expect_no_stalls: false }
}

fn check(sc: &Scenario) {
    let mut f = ToyFactory { slots: sc.slots, seed: sc.seed };
    let report = harness::run(sc, &mut f).unwrap_or_else(|e| panic!("seed {}: {e:?}", sc.seed));
    harness::verdict(sc, &report, &mut f).unwrap_or_else(|e| panic!("seed {} {:?}: {e:?}", sc.seed, sc.peers[0].profile.name));
}

#[test]
fn every_profile_converges() {
    for profile in Profile::ALL {
        for seed in 1..=20 {
            check(&scenario(seed, profile, 3, 1));
        }
    }
}

#[test]
fn a_lone_host_plays_without_clients() {
    check(&scenario(1, Profile::LAN, 0, 0));
}

#[test]
fn rollbacks_happen_and_are_counted() {
    let sc = scenario(3, Profile::TYPICAL, 3, 0);
    let mut f = ToyFactory { slots: sc.slots, seed: sc.seed };
    let r = harness::run(&sc, &mut f).unwrap();
    let client = &r.stats[&1];
    assert!(client.rollbacks > 0);
    assert!(client.depth_hist.iter().sum::<u64>() == client.rollbacks);
    assert_eq!(r.stats[&HOST].own_corrections, 0);
}

#[test]
fn an_outage_recovers() {
    let mut sc = scenario(5, Profile::GOOD, 2, 0);
    sc.outages.push(Outage { peer: 1, from_tick: 400, until_tick: 450 });
    check(&sc);
}

#[test]
fn the_input_delay_formula() {
    assert_eq!([0, 39, 40, 80, 119, 150, 400].map(input_delay), [0, 0, 1, 2, 2, 3, 3]);
}
```

`receive_never_panics` (Review Focus 4) also goes in `tests/session.rs`:

```rust
use proptest::prelude::*;
use rl_netcode::{Channel, Config, Role, Session, ToySim};

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn receive_never_panics(packets in prop::collection::vec(prop::collection::vec(any::<u8>(), 0..1300), 1..20)) {
        let mut host = Session::new(Config::new(4, 0), Role::Host { peers: vec![(1, Seat::Player(1))] }, ToySim::new(4), 0);
        let mut client = Session::new(Config::new(4, 1), Role::Client { host: 0, slot: 1 }, ToySim::new(4), 0);
        for (i, p) in packets.iter().enumerate() {
            let ch = if i % 2 == 0 { Channel::Unreliable } else { Channel::Reliable };
            host.receive(1, ch, p, i as u64 * 1000);
            client.receive(0, ch, p, i as u64 * 1000);
        }
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo nextest run -p rl-netcode`
Expected: compile errors.

- [ ] **Step 3: Implement `inputs.rs`**

Store `rows: Vec<[u8; 32]>` and `known: Vec<u32>` grown on demand, plus `upto: [u32; 32]`. `set` checks `tick < upto[slot]` (Same/Conflict), `tick > upto[slot]` (Gap), else writes, sets the bit and increments `upto[slot]`. `frontier` = `upto[..slots].iter().min()` (`u32::MAX` never: with zero slots return the number of rows the session told it to cover — the session with no slots does not exist; `Config::new` asserts `slots >= 1`). `predicted(t, s)` = `get(t, s)` or the value at `upto[s] - 1` when `t >= upto[s]` (`0` when `upto[s] == 0`). `row(t)` = `Some(rows[t])` only for `t < frontier`.

- [ ] **Step 4: Implement `session.rs`**

Private state (one struct for all roles, the role deciding branches):

```rust
pub struct Session<S: Simulation> {
    cfg: Config,
    role: Role,
    sim: S,
    log: InputLog,
    /// A client's own slot's provisional inputs, by tick (from 0).
    own: Vec<u8>,
    /// The keys used for each tick in `frontier..current`, by tick - `used_base`.
    used: VecDeque<[u8; 32]>,
    used_base: u32,
    /// The state before each tick in `snap_base..=current`.
    snaps: VecDeque<S::Snapshot>,
    snap_base: u32,
    current: u32,
    rollback_from: Option<u32>,
    start_us: u64,
    /// Per remote peer (the host keeps one per client; others one for the host).
    peers: BTreeMap<PeerId, PeerState>,
    out: Vec<(PeerId, Channel, Vec<u8>)>,
    events: Vec<Event>,
    stats: Stats,
}

struct PeerState {
    seat: Seat,                 // the host's view; clients use Seat::Spectator for the host entry
    ack: u32,                   // host: the peer's frontier; client: the host's `your_ack`
    echo: Option<(u32, u64)>,   // (their sent_ms, when we received it in µs)
}
```

`advance(now, driver)`:

```rust
pub fn advance(&mut self, now_us: u64, driver: &mut dyn Driver<S>) {
    if let Some(from) = self.rollback_from.take() {
        self.resimulate(from, driver);
    }
    let due = self.due_ticks(now_us); // Task 6: elapsed ticks - ticks run, capped at MAX_CATCHUP
    let mut ran = 0;
    for _ in 0..due {
        if self.is_spectator() {
            if self.current >= self.log.frontier() { break; }
        } else if self.current - self.log.frontier().min(self.current) >= self.cfg.max_prediction {
            self.events.push(Event::Stalled { tick: self.current });
            self.stats.stalls += 1;
            break;
        }
        self.new_tick(driver);
        ran += 1;
    }
    self.ticks_run += ran;           // the pace counter (a field)
    self.trim_snapshots();
    if ran > 0 || due > 0 {
        self.queue_datagrams(now_us);
    }
}
```

`new_tick`: (4) local inputs, then (5). `resimulate(from)`: `snaps[from - snap_base]` restore; drop snapshots after it; for `t in from..current`: save snapshot, keys per (3), tick, overwrite `used[t]`, `simulated(.. resim: true ..)`. Keep `current` unchanged. `trim_snapshots`: while `snap_base < frontier` and more than one snapshot, pop front and advance `snap_base`; then `sim.forget_before(&snaps[0])`; trim `used` the same way.

`all_final` in `TickInfo` is true when every slot's final value was known for that tick when it was simulated.

`queue_datagrams`, `receive`: as described in 8-11. Use `rl_proto::encode_frames` with `value = |t, s| self.log.get(t, s)` for `t` in `first..first + n`, where `n` = number of ticks from `first` to the highest tick any slot knows (`max known_upto`), capped by `MAX_FRAMES`; encode, and while `encoded.len() > MAX_DATAGRAM` halve `n` and retry.

- [ ] **Step 5: Implement `harness.rs` and `toy.rs`**

As described above. `toy.rs`: `ToyFactory::sim(_)` = `ToySim::new(slots)`; `driver(peer, seat)` = a `ToyDriver { rng: Rng::new(seed ^ peer as u64 * 0x9e37) }` whose `local_inputs` writes, per local slot, a new random key with probability 1/8 and otherwise repeats that slot's previous key (realistic hold-and-change behaviour). The driver must not read wall time or anything else.

- [ ] **Step 6: Run the tests**

Run: `cargo nextest run -p rl-netcode`
Expected: all pass. `every_profile_converges` runs 100 toy scenarios; it must finish within a few seconds in the dev profile (if it is slower, profile `harness::run` — the 1 ms stepping loop should skip to the next frame or delivery time when nothing is due).

- [ ] **Step 7: Commit**

CLAUDE.md: a "**Session**" section summarising points 1-13 in a few bullets and "**Harness**" (`run`, `verdict`, `HOST`). `cargo xtask check`, then:

```bash
git add crates/rl-netcode
git commit -m "rl-netcode: the rollback session with host-ordered inputs, and its harness"
```

---
### Task 7: rl-netcode: time sync, the lagging-player rule and timeouts

**Files:**
- Create: `crates/rl-netcode/src/clock.rs`, `crates/rl-netcode/tests/timing.rs`
- Modify: `crates/rl-netcode/src/session.rs`, `crates/rl-netcode/src/harness.rs` (freezes; clock drift already exists), `crates/rl-netcode/tests/session.rs` (its `PeerPlan`s gain `freeze: None`), `crates/rl-netcode/CLAUDE.md`
- Test: inline tests in `clock.rs`; `tests/timing.rs`

**Interfaces:**
- Consumes: Task 6's session, harness, `Event`, `Stats`; `rl_proto::{Control, encode_control, decode_control}`.
- Produces:
  ```rust
  pub const TARGET_MARGIN: i32 = 1;          // ticks
  pub const MAX_ADJUST_PPM: i64 = 10_000;    // 1 %
  pub const JUMP_TICKS: i32 = 8;
  pub const LATE_WARNING_US: u64 = 3_000_000;
  pub const SILENCE_TIMEOUT_US: u64 = 10_000_000;
  // Event gains:
  Late { tick: u32 },              // client: the host filled our input for `tick`
  PeerLate { peer: PeerId },       // host: this peer's inputs missed the window for 3 s
  PeerTimedOut { peer: PeerId },   // nothing from this peer for 10 s
  // Stats gains: pub margin: Option<i16>, pub fills: u64, pub adjust_ppm: i64, pub jumps: u64
  impl<S: Simulation> Session<S> { pub fn peer_lost(&mut self, peer: PeerId); }
  // clock.rs
  pub struct Pace { .. }
  impl Pace {
      pub fn new(start_us: u64) -> Self;
      pub fn due(&mut self, now_us: u64, ticks_run: u32) -> u32;   // capped at MAX_CATCHUP
      pub fn set_adjust_ppm(&mut self, ppm: i64);                 // clamped to ±MAX_ADJUST_PPM
      pub fn shift(&mut self, ticks: i32);                        // a jump: move the timeline
  }
  pub struct Margins { .. }   // the host's per-client arrival margins
  impl Margins { pub fn push(&mut self, m: i32); pub fn median(&self) -> Option<i16>; }   // last 16 samples
  ```
  Harness additions: `PeerPlan` gains `pub freeze: Option<(u32, u32)>` (the peer stops advancing between these host ticks: a frozen laptop).

Context: spec §3.2 and decision N7. The host fills a client's input when the host's own `current` reaches `tick + fill_window` without it; clients pace themselves so their inputs arrive just before the host needs them.

**Behaviour:**

1. **Pace.** `Pace` converts local time into due ticks: `elapsed = (now - origin) * (1_000_000 + adjust_ppm) / 1_000_000`, integrated piecewise so a change of `adjust_ppm` affects only time after it (keep `(origin_us, ticks_at_origin)` and re-anchor on every change); `due = floor(elapsed / TICK_US) + ticks_at_origin + shift - ticks_run`, clamped to `0..=MAX_CATCHUP`. `shift(n)` adds `n` ticks to the timeline (negative holds the peer back). Replace Task 6's pacing by `Pace`.
2. **Margins (host).** When a `ClientInputs` brings a new input for tick `T` (the newest in the datagram that was `New`), the sample is `T as i32 - host.current as i32` (positive: arrived early). `Margins` keeps the last 16; every `HostInputs` to that client carries `margin = median` (0 with no samples).
3. **Adjusting (client).** On every `HostInputs`, `error = TARGET_MARGIN - margin`. If `|error| > JUMP_TICKS`: `pace.shift(error)` (a positive error means the client is late: it jumps ahead by catching up, `MAX_CATCHUP` ticks per call; negative holds it), `jumps += 1`, and the margin samples are stale — the client ignores margins until the host's `tick` field shows the host has seen inputs sent after the jump (track the client tick at the jump; ignore `HostInputs` whose `your_ack` is below it). Otherwise `adjust_ppm = clamp(error * 2_000, ±MAX_ADJUST_PPM)` (one tick of error → 0.2 % faster). Record `margin` and `adjust_ppm` in `Stats`.
4. **The fill rule (host).** At the start of each new host tick `t`, for every client slot `s` with `known_upto(s) + fill_window <= t`: set tick `known_upto(s)` to the slot's previous final value (`0` if none), `fills += 1`, remember the filled tick in a per-slot `BTreeSet` (pruned below the frontier). Repeat while the condition holds. When a client's datagram later brings a value for a filled tick, the host ignores the value and sends `Control::Late { tick }` on the reliable channel once per filled tick.
5. **Late (client).** On `Control::Late { tick }`: `Event::Late { tick }`. (The correction itself arrives as the host's frame for that tick and counts in `own_corrections` per Task 6.)
6. **Peer health (host).** If a client's slot was filled in each of the last 300 host ticks (3 s), push `Event::PeerLate { peer }` once (again only after it recovered). A peer from which nothing arrived for `SILENCE_TIMEOUT_US`: `Event::PeerTimedOut { peer }` once. The driver decides; `peer_lost(peer)` (also callable directly) marks the slot abandoned: from then on the host fills it with `Keys(0)` at every new tick without waiting (decision N6) and stops sending to that peer. A client that hears nothing from the host for `SILENCE_TIMEOUT_US` pushes `PeerTimedOut { peer: host }`.
7. **Clients never wait for the host's fill.** A client's stall rule stays `current - frontier >= max_prediction`.

- [ ] **Step 1: Write the failing tests**

`clock.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pace_runs_at_one_tick_per_10_ms() {
        let mut p = Pace::new(0);
        assert_eq!(p.due(9_999, 0), 0);
        assert_eq!(p.due(10_000, 0), 1);
        assert_eq!(p.due(1_000_000, 1), MAX_CATCHUP);
    }

    #[test]
    fn adjusting_changes_only_the_future() {
        let mut p = Pace::new(0);
        assert_eq!(p.due(1_000_000, 0).min(100), 10); // capped per call
        let mut run = 0;
        for ms in 1..=1000u64 { run += p.due(ms * 1000, run); }
        assert_eq!(run, 100);
        p.set_adjust_ppm(10_000);
        for ms in 1001..=2000u64 { run += p.due(ms * 1000, run); }
        assert_eq!(run, 201);
    }

    #[test]
    fn shifting_jumps_and_holds() {
        let mut p = Pace::new(0);
        p.shift(5);
        assert_eq!(p.due(0, 0), 5);
        p.shift(-10);
        assert_eq!(p.due(100_000, 5), 0);
    }

    #[test]
    fn margins_take_the_median_of_the_last_16() {
        let mut m = Margins::default();
        assert_eq!(m.median(), None);
        for v in [5, -3, 1, 1, 1] { m.push(v); }
        assert_eq!(m.median(), Some(1));
        for _ in 0..16 { m.push(-4); }
        assert_eq!(m.median(), Some(-4));
    }
}
```

`tests/timing.rs`:

```rust
//! Time sync and the lagging-player rule (spec §3.2) with the toy simulation.

use rl_netcode::harness::{self, Outage, PeerPlan, Scenario, HOST};
use rl_netcode::netsim::Profile;
use rl_netcode::toy::ToyFactory;
use rl_netcode::{input_delay, Event, Seat};

fn plan(id: u16, slot: u8, profile: Profile, ppm: i32) -> PeerPlan {
    PeerPlan { id, seat: Seat::Player(slot), profile, clock_ppm: ppm, input_delay: input_delay(profile.ping_ms),
               frame_us: 6_944, join_tick: None, freeze: None }
}

fn scenario(seed: u64, peers: Vec<PeerPlan>, ticks: u32) -> Scenario {
    Scenario { seed, slots: 2 + peers.len() as u8, peers, ticks, settle_ticks: 300, outages: vec![], expect_no_stalls: false }
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
    for p in [1u16, 2] {
        assert_eq!(r.stats[&HOST].fills, 0, "nobody's input was filled");
        let m = r.stats[&p].margin.unwrap();
        assert!((0..=3).contains(&m), "peer {p} margin {m}");
    }
}

#[test]
fn players_within_the_window_never_stall() {
    for profile in [Profile::LAN, Profile::GOOD, Profile::TYPICAL, Profile::BAD] {
        for seed in 1..=10 {
            let mut sc = scenario(seed, (0..4).map(|i| plan(1 + i, 2 + i as u8, profile, (i as i32 - 2) * 1_000)).collect(), 2_000);
            sc.expect_no_stalls = true;
            run(&sc);
        }
    }
}

#[test]
fn late_inputs_lose_to_the_fill() {
    // Peer 1 loses its link for 0.5 s: the host fills its slot; every peer, peer 1 included, ends with the fill.
    let mut sc = scenario(4, vec![plan(1, 2, Profile::GOOD, 0), plan(2, 3, Profile::GOOD, 0)], 1_500);
    sc.outages.push(Outage { peer: 1, from_tick: 500, until_tick: 550 });
    let r = run(&sc);
    assert!(r.stats[&HOST].fills > 0);
    assert!(r.events[&1].iter().any(|e| matches!(e, Event::Late { .. })));
    assert!(r.stats[&1].own_corrections > 0);
    assert_eq!(r.stats[&2].stalls, 0, "the other player never waited");
}

#[test]
fn a_frozen_client_catches_up() {
    let mut p = plan(1, 2, Profile::GOOD, 0);
    p.freeze = Some((600, 900)); // 3 s without a single frame
    let sc = scenario(6, vec![p, plan(2, 3, Profile::GOOD, 0)], 2_000);
    let r = run(&sc);
    assert!(r.stats[&1].jumps >= 1);
    assert_eq!(r.stats[&2].stalls, 0);
    assert!(r.events[&HOST].iter().any(|e| matches!(e, Event::PeerLate { peer: 1 })));
}

#[test]
fn a_silent_peer_times_out() {
    let mut sc = scenario(8, vec![plan(1, 2, Profile::GOOD, 0)], 2_000);
    sc.outages.push(Outage { peer: 1, from_tick: 300, until_tick: 1_400 });
    let mut f = ToyFactory { slots: sc.slots, seed: sc.seed };
    let r = harness::run(&sc, &mut f).unwrap();
    assert!(r.events[&HOST].iter().any(|e| matches!(e, Event::PeerTimedOut { peer: 1 })));
    assert!(r.events[&1].iter().any(|e| matches!(e, Event::PeerTimedOut { peer: HOST })));
}
```

In `a_silent_peer_times_out` the verdict is not required (the harness calls `peer_lost` on `PeerTimedOut` events for the host, so the match continues, but the dropped client cannot converge); implement that harness reaction: on a host `PeerTimedOut { peer }`, call `host.peer_lost(peer)` and exclude that peer from the verdict (record it in `Report::dropped: BTreeSet<PeerId>`).

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo nextest run -p rl-netcode`
Expected: compile errors (`Pace`, `Margins`, new fields and events).

- [ ] **Step 3: Implement** `clock.rs` and the session changes 1-7. The `Report` gains `dropped`. `verdict` skips dropped peers.

- [ ] **Step 4: Run the tests**

Run: `cargo nextest run -p rl-netcode`
Expected: all pass, including Task 6's `every_profile_converges`. If `players_within_the_window_never_stall` fails for `bad`, look at the margin and depth numbers before changing constants; a constant change is a ruling to record (spec numbers: fill window 12; this plan's N1: prediction 28).

- [ ] **Step 5: Commit**

`cargo xtask check`, then:

```bash
git add crates/rl-netcode
git commit -m "rl-netcode: time sync, the host's fill rule and timeouts"
```

---

### Task 8: rl-netcode: checksums, desync repair, state transfer, spectators mid-match, and the World adapter

**Files:**
- Create: `crates/rl-netcode/src/sync.rs`, `crates/rl-netcode/src/world_sim.rs`, `crates/rl-netcode/tests/sync.rs`, `crates/rl-netcode/tests/world_sim.rs`
- Modify: `crates/rl-netcode/src/{session.rs, harness.rs, lib.rs}`, `crates/rl-netcode/Cargo.toml` (no new dependencies: the World encoding is rl-sim's; dev-dependencies `rl-content`, `rl-bots`, `rl-modes`), `crates/rl-netcode/CLAUDE.md`, the earlier tests that build a `Scenario` (they gain `corrupt: vec![]`)
- Test: `tests/sync.rs` (toy), `tests/world_sim.rs` (content: Castle Strike)

**Interfaces:**
- Consumes: Task 3's `WorldState::{encode, decode}`, `World::{state, load_state, enable_rollback, save, restore, forget_before, sync_checksum}`; Task 7's session.
- Produces (Tasks 9, 10):
  ```rust
  // Event gains:
  Desync { tick: u32, local: u64, remote: u64, peer: PeerId },   // host: peer's checksum differs; client: the host said so
  StateSent { peer: PeerId, tick: u32, bytes: usize },
  StateArrived { tick: u32, bytes: Vec<u8> },                     // client/spectator, before loading
  StateLoaded { tick: u32 },
  StateRejected { tick: u32 },
  // Stats gains: pub desyncs: u64, pub states_sent: u64, pub states_loaded: u64
  impl<S: Simulation> Session<S> {
      pub fn checksums(&self) -> &[(u32, u64)];          // own confirmed sync checksums, ascending
      pub fn encode_confirmed_state(&mut self) -> (u32, Vec<u8>);  // the state after the last confirmed tick
      pub fn add_spectator(&mut self, peer: PeerId, now_us: u64);  // host: a spectator arrives mid-match
  }
  pub enum Role { .., Joining { host: PeerId } }   // a spectator waiting for the host's state
  // world_sim.rs
  pub struct WorldSim { .. }
  impl WorldSim {
      pub fn new(world: rl_sim::World) -> Self;            // enables rollback
      pub fn world(&self) -> &rl_sim::World;
      pub fn world_mut(&mut self) -> &mut rl_sim::World;
      pub fn events(&self) -> &rl_sim::TickEvents;          // of the last simulated tick
  }
  impl Simulation for WorldSim { type Snapshot = rl_sim::Snapshot; .. }
  // harness.rs: PeerPlan.join_tick now supported for spectators; Report gains pub checksums: BTreeMap<PeerId, Vec<(u32, u64)>>
  ```

Context: spec §3.5, §3.6. Checksums use `Simulation::checksum` (for `WorldSim`, `sync_checksum`, decision N4).

**Behaviour:**

1. **Recording.** After simulating tick `t` (new or re-simulated) with `(t + 1) % checksum_every == 0`, store `pending[t] = sim.checksum()`. A rollback to `r` drops pending entries `>= r` (they are recomputed as the ticks re-run). At the end of `advance`, every pending `t` below the frontier moves to `checksums` (confirmed: the last simulation of a confirmed tick ran on the final inputs, since any differing final input forces a rollback first).
2. **Exchange.** Clients and spectators send `Control::Checksum { tick, hash }` for each newly confirmed entry. The host keeps its own confirmed checksums (the last 1024 ticks' worth) and peers' reported ones until its own exists; on comparison: equal → drop; different → `Event::Desync { tick, local, remote, peer }`, `desyncs += 1`, send `Control::Desync { tick, host_hash: local }`, then the state (3) — at most one repair in flight per peer.
3. **Sending a state.** `encode_confirmed_state`: restore the snapshot before the frontier (the state after tick `frontier - 1`), `encode_state()`, then re-simulate back to `current` (a rollback without an event; count `resim_ticks`). Send `Control::StateBegin { tick: frontier - 1, len, chunks }` and the chunks (`STATE_CHUNK` bytes each) on the reliable channel; `Event::StateSent`.
4. **Receiving a state.** Collect chunks (refuse — `StateRejected` and drop the transfer — when an index is out of range, a chunk repeats or the total exceeds `len`). Complete: `Event::StateArrived { tick, bytes }` (the driver may keep the bytes), then `sim.decode_state(&bytes)`: on `Err` → `Event::StateRejected { tick }`, keep the old state; on `Ok` → `StateLoaded { tick }`, `states_loaded += 1`, and the session's timeline restarts from it: drop all snapshots and used keys, set a *state floor* `floor = tick + 1` (ticks below it count as confirmed for rollback and checksum purposes even if their log rows are not all known yet; their frames are still recorded when they arrive, and never trigger a rollback), `snap_base = floor`, save a snapshot, then simulate `floor..max(current, floor)` predicted (a joining spectator sets `current = floor` and follows the frontier).
5. **Desync on the client.** `Control::Desync { tick, host_hash }` → `Event::Desync { tick, local: own checksum for tick (or 0), remote: host_hash, peer: host }`. The state follows.
6. **Spectators mid-match.** `Role::Joining { host }`: simulates nothing until a state loads; then it is a spectator. The host's `add_spectator(peer, now)` registers the peer with `Seat::Spectator`, its `ack` at the host's frontier, and sends the state (3). Frames for ticks at and after the state tick reach it through normal `HostInputs` (its `ack` comes from its `ClientInputs`, which it sends from the moment it is created).
7. **WorldSim.** `tick` = `world.tick(keys, &mut events)`; `save`/`restore`/`forget_before` = the `World` methods (`restore` returning an error is a session bug: `expect` with the message "a kept snapshot restores"); `checksum` = `sync_checksum`; `encode_state` = `world.state().encode()`; `decode_state` = `WorldState::decode` then `world.load_state`, mapping errors to `StateRejected`. `events()` returns the last tick's events (stale after a restore until the next tick — document it).

- [ ] **Step 1: Write the failing tests**

`tests/sync.rs`:

```rust
use rl_netcode::harness::{self, PeerPlan, Scenario, HOST};
use rl_netcode::netsim::Profile;
use rl_netcode::toy::ToyFactory;
use rl_netcode::{input_delay, Event, Seat};

fn plan(id: u16, seat: Seat, profile: Profile, join_tick: Option<u32>) -> PeerPlan {
    PeerPlan { id, seat, profile, clock_ppm: 0, input_delay: input_delay(profile.ping_ms), frame_us: 6_944, join_tick, freeze: None }
}

#[test]
fn confirmed_checksums_agree_and_are_exchanged() {
    let peers = vec![plan(1, Seat::Player(2), Profile::TYPICAL, None), plan(2, Seat::Spectator, Profile::GOOD, None)];
    let sc = Scenario { seed: 2, slots: 3, peers, ticks: 1_000, settle_ticks: 200, outages: vec![], expect_no_stalls: true, corrupt: vec![] };
    let mut f = ToyFactory { slots: 3, seed: 2 };
    let r = harness::run(&sc, &mut f).unwrap();
    harness::verdict(&sc, &r, &mut f).unwrap();
    let host = &r.checksums[&HOST];
    assert!(host.len() >= 99);
    assert_eq!(&r.checksums[&1][..99], &host[..99]);
    assert!(r.events.values().flatten().all(|e| !matches!(e, Event::Desync { .. })));
}

#[test]
fn a_spectator_joins_mid_match() {
    let peers = vec![plan(1, Seat::Player(2), Profile::TYPICAL, None), plan(7, Seat::Spectator, Profile::TYPICAL, Some(600))];
    let sc = Scenario { seed: 3, slots: 3, peers, ticks: 1_500, settle_ticks: 200, outages: vec![], expect_no_stalls: true, corrupt: vec![] };
    let mut f = ToyFactory { slots: 3, seed: 3 };
    let r = harness::run(&sc, &mut f).unwrap();
    harness::verdict(&sc, &r, &mut f).unwrap();
    assert!(r.events[&7].iter().any(|e| matches!(e, Event::StateLoaded { .. })));
    assert!(r.events[&HOST].iter().any(|e| matches!(e, Event::StateSent { peer: 7, .. })));
}
```

`verdict` must compare a mid-match spectator only from its state tick on (its `sims` start there). Desync repair with a planted fault is tested in Task 10 (it needs a fault hook in the World harness); add here a toy-level test that corrupts a client's `ToySim` through `Session::sim_mut` — give `Scenario` a field `pub corrupt: Vec<(PeerId, u32)>` (at the peer's first frame after host tick `t`, the harness calls a `Factory::corrupt(&mut S)` hook — `ToyFactory` adds 1 to slot 0's position) and assert: `Event::Desync` on the host within 3 × `CHECKSUM_EVERY` ticks of the corruption, `StateLoaded` on the client, and every checksum after the load equals the host's:

```rust
#[test]
fn a_desync_is_detected_and_repaired() {
    let peers = vec![plan(1, Seat::Player(2), Profile::GOOD, None)];
    let mut sc = Scenario { seed: 9, slots: 3, peers, ticks: 1_200, settle_ticks: 200, outages: vec![], expect_no_stalls: true, corrupt: vec![] };
    sc.corrupt.push((1, 500));
    let mut f = ToyFactory { slots: 3, seed: 9 };
    let r = harness::run(&sc, &mut f).unwrap();
    let desync = r.events[&HOST].iter().find_map(|e| match e { Event::Desync { tick, .. } => Some(*tick), _ => None }).unwrap();
    assert!((500..530).contains(&desync), "detected at {desync}");
    let loaded = r.events[&1].iter().find_map(|e| match e { Event::StateLoaded { tick } => Some(*tick), _ => None }).unwrap();
    let after = |v: &Vec<(u32, u64)>| v.iter().filter(|(t, _)| *t > loaded).copied().collect::<Vec<_>>();
    let (h, c) = (after(&r.checksums[&HOST]), after(&r.checksums[&1]));
    assert!(!c.is_empty());
    assert_eq!(&h[..c.len()], &c[..]);
}
```

(`verdict` is not called: the corrupted peer's pre-repair checksums differ by design; `verdict` gains a rule: with `corrupt` entries, a corrupted peer is compared only after its last `StateLoaded`.)

`tests/world_sim.rs` (content; skip with a notice when `RL_CONTENT_DIR` is unset, as `rl-cli`'s `tests/common` does):

```rust
//! The real world under rollback: snapshots across terrain edits, state
//! transfer and the harness with bots-free random inputs.

use rl_content::{Content, MatchOptions, Player};
use rl_netcode::{Simulation, WorldSim};
use rl_sim::Keys;

fn content() -> Option<Content> {
    let dir = std::env::var("RL_CONTENT_DIR").ok()?;
    Content::open(std::path::Path::new(&dir)).ok()
}

fn world(c: &Content, seed: u32) -> rl_sim::World {
    // MW 1.0 chaos bots on Castle Strike: heavy carving. Build with the bots'
    // loadouts, then drive with scripted keys.
    let mut o = MatchOptions::for_test("olx-mw-1-0", "olx-level-castlestrike", seed, 8);
    o.players = (0..8).map(|_| Player::Bot(Default::default())).collect();
    c.build_match(&o).unwrap().world
}

fn keys(t: u32) -> Vec<Keys> {
    (0..8u32).map(|i| Keys(if (t / 9 + i) % 4 == 0 { Keys::FIRE | Keys::RIGHT } else { Keys::FIRE | Keys::UP })).collect()
}

#[test]
fn restore_undoes_terrain_edits_in_a_real_match() {
    let Some(c) = content() else { return };
    let mut a = WorldSim::new(world(&c, 1));
    let mut b = WorldSim::new(world(&c, 1));
    for t in 0..200 { a.tick(&keys(t)); b.tick(&keys(t)); }
    let s = a.save();
    for t in 200..400 { a.tick(&keys(t + 7)); } // a different future
    a.restore(&s);
    for t in 200..400 { a.tick(&keys(t)); b.tick(&keys(t)); }
    assert_eq!(a.world().checksum(), b.world().checksum());
    assert_eq!(a.world(), b.world());
}

#[test]
fn state_transfer_in_a_real_match() {
    let Some(c) = content() else { return };
    let mut a = WorldSim::new(world(&c, 2));
    for t in 0..300 { a.tick(&keys(t)); }
    let mut b = WorldSim::new(world(&c, 2));
    b.decode_state(&a.encode_state()).unwrap();
    assert_eq!(a.checksum(), b.checksum());
    for t in 300..500 { a.tick(&keys(t)); b.tick(&keys(t)); }
    assert_eq!(a.world().checksum(), b.world().checksum());
}
```

`MatchOptions::for_test` does not exist: build `MatchOptions` the way `rl-cli`'s `MatchSpec::match_options` does (read `crates/rl-cli/src/spec.rs`) — copy the needed lines into the test, or, better, move `MatchSpec::match_options`'s body into `rl_content` as `MatchOptions::deathmatch(mod_id, level_id, seed, players)` and call it from both (a small refactor; `rl-cli`'s goldens prove it unchanged).

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo nextest run -p rl-netcode`
Expected: compile errors.

- [ ] **Step 3: Implement** `sync.rs` (recording, exchange, state transfer, chunk assembly) and `world_sim.rs`; wire them into `session.rs`; extend the harness (`join_tick` for spectators: the harness creates the peer's session with `Role::Joining` and calls `host.add_spectator` at that host tick; `corrupt`; `Report::checksums` from `session.checksums()`).

- [ ] **Step 4: Run the tests**

Run: `cargo nextest run -p rl-netcode`
Expected: all pass (content tests pass with `RL_CONTENT_DIR`, which `.cargo/config.toml` sets).

- [ ] **Step 5: Commit**

CLAUDE.md: "**Sync**" bullets (checksums every 10 confirmed ticks, the host compares, repairs by sending its confirmed state; the state floor) and "**WorldSim**". `cargo xtask check`, then:

```bash
git add crates/rl-content crates/rl-cli crates/rl-netcode
git commit -m "rl-netcode: checksums, desync repair and state transfer; the World adapter"
```

---
### Task 9: rl-cli: the World-backed harness, `rl-cli net-sim` and kept cases

**Files:**
- Create: `crates/rl-cli/src/netsim.rs`, `crates/rl-cli/src/net_sim_cmd.rs`, `crates/rl-cli/tests/net_sim.rs`, `crates/rl-cli/tests/net_cases.rs`, `fixtures/net/cases/.gitkeep`
- Modify: `crates/rl-cli/Cargo.toml` (`rl-netcode.workspace = true`, `rl-proto.workspace = true`), `crates/rl-cli/src/{lib.rs, main.rs}`, `crates/rl-cli/tests/common/mod.rs` (export `NET_FULL` helpers if needed), `xtask/src/bless_regression.rs` (route `net-*.ron`), `crates/rl-cli/CLAUDE.md`, `xtask/CLAUDE.md`
- Test: `tests/net_sim.rs` (check tier: two short runs; full tier: the grid), `tests/net_cases.rs`, a unit test in `xtask/src/bless_regression.rs`

**Interfaces:**
- Consumes: Task 8's `WorldSim`, `harness::{run, verdict, Scenario, PeerPlan, Outage, Factory, Report, Failure, HOST}`, `Profile`; `rl_content::{Content, MatchOptions, Player}` (and the `MatchOptions::deathmatch` helper if Task 8 added it); `rl_bots::{Bot, BotConfig, Difficulty, Personality, AreaCache}`.
- Produces (Tasks 10, 11):
  ```rust
  // rl_cli::netsim
  #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
  pub struct NetSpec {
      pub mod_id: String, pub level_id: String, pub seed: u32,
      pub slots: u8,                         // worms; clients take slots 0..clients, the host's bots the rest
      pub clients: u8,
      pub spectators: u8,
      #[serde(default)] pub join_tick: Option<u32>,   // spectators arrive then (None: from the start)
      pub profile: String,                   // a netsim profile name
      #[serde(default)] pub drift_ppm: i32,  // clients get -drift..+drift spread evenly
      pub ticks: u32,
      #[serde(default)] pub outages: Vec<(u16, u32, u32)>,
      #[serde(default)] pub net_seed: u64,
      #[serde(default)] pub difficulty: Option<String>,  // bots' difficulty, default "mixed"
  }
  impl NetSpec { pub fn stem(&self) -> String; pub fn scenario(&self) -> anyhow::Result<Scenario>; }
  pub struct WorldFactory<'c> { .. }
  impl<'c> WorldFactory<'c> { pub fn new(content: &'c Content, spec: &NetSpec) -> anyhow::Result<Self>; }
  impl Factory<WorldSim> for WorldFactory<'_>
  pub struct NetRun { pub spec: NetSpec, pub report: Report, pub verdict: Result<(), Failure> }
  pub fn run(content: &Content, spec: &NetSpec) -> anyhow::Result<NetRun>;
  #[derive(Serialize, Deserialize)] pub struct NetCase { pub spec: NetSpec, #[serde(default)] pub failure: Option<String> }
  pub fn write_failure(dir: &Path, run: &NetRun) -> anyhow::Result<PathBuf>;   // <dir>/net-<stem>.ron
  impl NetRun { pub fn summary(&self) -> String; }
  ```

Context: spec §9.1, §9.4's in-process part, §3.6 (bots think on the host only). Every peer builds its own world from the same `MatchOptions` (all players `Player::Bot` with the spec's difficulty, so loadouts are identical everywhere), with its own `AreaCache` (bots on different peers must not share one). Each peer *runs* only the bots of its local slots: the host the slots `clients..slots`, client `k` (peer id `1 + k`) slot `k`. A bot thinks with `Bot::think(&world, &events)`, where `events` are the peer's last simulated tick's (`WorldSim::events`), so clients' bots act like players producing inputs that travel with their input delay.

**Behaviour:**

- `NetSpec::scenario`: host = `HOST`, clients `1..=clients` with `Seat::Player(k)`, `input_delay(profile.ping_ms)`, `frame_us 6_944`, clock ppm spread evenly in `-drift_ppm..=drift_ppm`; spectators ids `100..`, `frame_us 16_667`, `join_tick` from the spec; `settle_ticks: 300`; `expect_no_stalls` = the profile is one of `lan`, `good`, `typical`, `bad`; outages from the spec; `seed = net_seed`.
- `WorldFactory::sim(peer)` → `WorldSim::new(content.build_match(&options)?.world)` (the build fails only on bad content: `expect` with the spec in the message is acceptable inside the factory because `WorldFactory::new` builds once up front to validate). `driver(peer, seat)` → a `BotDriver` holding the bots for that peer's local slots (`Bot::with_cache(slot, cfg, seed, &world, cache)` with the peer's own `AreaCache::new(world.terrain())`).
- `run` → `harness::run` then `harness::verdict`; a `harness::run` error is a failure too.
- `rl-cli net-sim` (`net_sim_cmd.rs`): `Args { #[command(flatten)] spec: NetSpecArgs (clap mirror of NetSpec with defaults: mod olx-classic, level olx-level-castlestrike, seed 1, slots 8, clients 3, spectators 1, profile typical, ticks 1000), --from <net case .ron>, --seeds <a..b> (runs each seed as net_seed and seed), --content, --failures (default target/failures) }`. Prints `NetRun::summary()` per run (profile, peers, ticks, rollbacks and depth p50/p99 per client, fills, stalls, desyncs, bytes/s received per client) and `ok`/`FAIL <what> at tick <t>`; on failure writes the case with `write_failure` and prints `wrote <path>; replay with: rl-cli net-sim --from <path>`. Exit status 1 when any run failed.
- `tests/net_cases.rs`: every `fixtures/net/cases/*.ron` (a `NetCase`) must pass its verdict now (kept failures are regression tests).
- `bless_regression::add_failure`: a source file whose name starts with `net-` is copied to `fixtures/net/cases/` instead of `fixtures/matches/` (same "exists already" error); `run` then runs `cargo test -p rl-cli --test net_cases` instead of re-recording goldens when only a net case was added. Unit test: `add_failure` with a `net-x.ron` source lands in the net cases directory.

- [ ] **Step 1: Write the failing tests**

`tests/net_sim.rs`:

```rust
mod common;

use common::{content, full_tier};
use rl_cli::netsim::{run, NetSpec};

fn spec(profile: &str, seed: u32) -> NetSpec {
    NetSpec {
        mod_id: "olx-classic".into(),
        level_id: "olx-level-castlestrike".into(),
        seed,
        slots: 8,
        clients: 3,
        spectators: 1,
        join_tick: None,
        profile: profile.into(),
        drift_ppm: 2_000,
        ticks: 600,
        outages: vec![],
        net_seed: seed as u64,
        difficulty: None,
    }
}

fn pass(c: &rl_content::Content, s: &NetSpec) {
    let r = run(c, s).unwrap();
    if let Err(f) = &r.verdict {
        let path = rl_cli::netsim::write_failure(&common::failures_dir(), &r).unwrap();
        panic!("{} failed at tick {}: {} (rl-cli net-sim --from {})", s.stem(), f.tick, f.what, path.display());
    }
}

#[test]
fn bots_play_online_over_a_typical_and_a_bad_network() {
    let Some(c) = content() else { return };
    pass(&c, &spec("typical", 1));
    pass(&c, &spec("bad", 2));
}

#[test]
fn a_spectator_joins_a_real_match() {
    let Some(c) = content() else { return };
    let mut s = spec("typical", 3);
    s.join_tick = Some(300);
    pass(&c, &s);
}

#[test]
fn net_grid() {
    if !full_tier() { return; }
    let Some(c) = content() else { return };
    for profile in ["lan", "good", "typical", "bad", "awful"] {
        for seed in 1..=4 {
            let mut s = spec(profile, seed);
            s.ticks = 3_000;
            if seed % 2 == 0 { s.mod_id = "olx-mw-1-0".into(); }
            pass(&c, &s);
        }
    }
}
```

`common::failures_dir()` is the repo's `target/failures` (add it to `tests/common/mod.rs` next to `capture` if it is not there). Add `("rl-cli", "net_sim")` to `FULL_TESTS` in `xtask/src/check.rs` so `net_grid` runs in `full` (it skips itself in `check`).

`tests/net_cases.rs`:

```rust
mod common;

#[test]
fn kept_net_cases_pass() {
    let Some(c) = common::content() else { return };
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/net/cases");
    let mut paths: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "ron")).collect();
    paths.sort();
    for p in paths {
        let case: rl_cli::netsim::NetCase = ron::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        let r = rl_cli::netsim::run(&c, &case.spec).unwrap();
        assert!(r.verdict.is_ok(), "{}: {:?}", p.display(), r.verdict);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo nextest run -p rl-cli --test net_sim --test net_cases`
Expected: compile errors.

- [ ] **Step 3: Implement** `netsim.rs`, `net_sim_cmd.rs` (registered in `main.rs` as `NetSim(net_sim_cmd::Args)` with doc comment "Plays an online match in one process over a simulated network (bots on every peer) and checks that every peer agrees"), the `bless_regression` routing and its unit test.

- [ ] **Step 4: Run the tests and the command**

Run: `cargo nextest run -p rl-cli --test net_sim --test net_cases && cargo run -q -p rl-cli -- net-sim --profile bad --ticks 800`
Expected: tests pass; the command prints a summary and `ok`. Record the check-tier test time in the report; `bots_play_online_over_a_typical_and_a_bad_network` must stay under 60 s in the dev profile — if it is slower, shorten `ticks` (not below 400) and say so.

- [ ] **Step 5: Commit**

CLAUDE.md (rl-cli): the `net-sim` command, `NetSpec`, failure files `target/failures/net-*.ron`, `fixtures/net/cases/`. xtask/CLAUDE.md: `bless-regression --from target/failures/net-*.ron` keeps a net case; `full` runs `net_sim`'s grid. `cargo xtask check`, then:

```bash
git add crates/rl-cli xtask fixtures/net
git commit -m "rl-cli: online matches with bots over a simulated network (net-sim)"
```

---

### Task 10: desync bundles and `rl-cli net-repro`

**Files:**
- Create: `crates/rl-cli/src/bundle.rs`, `crates/rl-cli/src/net_repro_cmd.rs`, `crates/rl-cli/tests/net_repro.rs`
- Modify: `crates/rl-cli/src/{netsim.rs, lib.rs, main.rs}`, `crates/rl-cli/tests/net_sim.rs` (its `NetSpec`s gain `faults: vec![]`), `crates/rl-cli/CLAUDE.md`
- Test: `tests/net_repro.rs`

**Interfaces:**
- Consumes: Task 8's events (`Desync`, `StateArrived`, `StateLoaded`), `Session::{encode_confirmed_state, checksums, input_log}`; Task 9's `NetSpec`, `WorldFactory`, `run`; Task 3's `WorldState::decode` (serde `Serialize` for JSON).
- Produces:
  ```rust
  // rl_cli::bundle
  pub const BUNDLE_MAGIC: [u8; 4] = *b"RLDS";
  pub const BUNDLE_VERSION: u16 = 1;
  #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
  pub struct DesyncBundle {
      pub spec: NetSpec,                       // how to rebuild the start world (plan 10 replaces this by the lobby's match setup)
      pub peer: u16,
      pub fingerprint: u64,                    // rl_proto::FINGERPRINT
      pub platform: String,                    // e.g. "x86_64-linux"
      pub inputs: Vec<[u8; 32]>,               // final input rows from tick 0
      pub local_checksums: Vec<(u32, u64)>,
      pub host: (u32, u64),                    // the mismatching tick and the host's checksum
      pub local_state: Option<(u32, Vec<u8>)>, // this peer's confirmed state when the desync was reported
      pub host_state: Option<(u32, Vec<u8>)>,  // the state the host sent
  }
  impl DesyncBundle { pub fn encode(&self) -> Vec<u8>; pub fn decode(b: &[u8]) -> anyhow::Result<Self>; }
  pub struct Repro { pub first_local_divergence: Option<u32>, pub local_diff: Option<String>, pub host_diff: Option<String> }
  pub fn reproduce(content: &Content, b: &DesyncBundle) -> anyhow::Result<Repro>;
  pub fn first_difference(a: &serde_json::Value, b: &serde_json::Value) -> Option<String>;  // a JSON path like "worms[1].health"
  // NetSpec gains: #[serde(default)] pub faults: Vec<(u16 peer, u32 tick, u8 worm, f32 health)>
  // netsim::run writes bundles for clients that report Desync to <failures>/net-<stem>-peer<k>.rlds when asked:
  pub fn run_with(content: &Content, spec: &NetSpec, bundles: Option<&Path>) -> anyhow::Result<NetRun>;
  ```

Context: spec §3.5, §9.6. A desync is our bug; the bundle turns it into a test. `reproduce` rebuilds the start world, replays the final inputs computing `sync_checksum` at every `t % 10 == 9`, and reports: the first tick where the replay differs from `local_checksums` (the local machine's simulation differs from the recording peer's: nondeterminism or a platform difference); and, at the tick of each included state, the first JSON path where the replayed world's `state()` differs from the decoded state (`serde_json::to_value` on both `WorldState`s).

- [ ] **Step 1: Write the failing tests**

`tests/net_repro.rs`:

```rust
mod common;

use rl_cli::bundle::{first_difference, reproduce, DesyncBundle};
use rl_cli::netsim::{run_with, NetSpec};
use serde_json::json;

#[test]
fn json_paths_name_the_first_difference() {
    let a = json!({"worms": [{"health": 100.0}, {"health": 100.0}], "time": 5});
    let b = json!({"worms": [{"health": 100.0}, {"health": 3.0}], "time": 5});
    assert_eq!(first_difference(&a, &b).as_deref(), Some("worms[1].health"));
    assert_eq!(first_difference(&a, &a), None);
}

#[test]
fn a_planted_desync_is_repaired_and_pinpointed() {
    let Some(c) = common::content() else { return };
    let spec = NetSpec {
        mod_id: "olx-classic".into(), level_id: "olx-level-castlestrike".into(), seed: 4, slots: 6, clients: 2,
        spectators: 0, join_tick: None, profile: "good".into(), drift_ppm: 0, ticks: 800, outages: vec![],
        net_seed: 4, difficulty: None, faults: vec![(1, 400, 3, 7.0)],
    };
    let dir = tempfile::tempdir().unwrap();
    let r = run_with(&c, &spec, Some(dir.path())).unwrap();
    assert!(r.verdict.is_ok(), "repaired: {:?}", r.verdict);
    let path = std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|e| e == "rlds")).expect("a bundle was written");
    let b = DesyncBundle::decode(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(b.peer, 1);
    let repro = reproduce(&c, &b).unwrap();
    let t = repro.first_local_divergence.expect("the faulty peer's stream diverges from the replay");
    assert!((400..420).contains(&t), "diverged at {t}");
    let diff = repro.local_diff.expect("its state differs from the replay");
    assert!(diff.starts_with("worms[3]"), "{diff}");
    assert_eq!(repro.host_diff, None, "the host's state is the replay's");
}
```

The fault: at peer 1's first frame after host tick 400, the harness sets worm 3's health to 7.0 in that peer's world (`World::set_health`) — through `Factory::corrupt`-like hook from Task 8, generalised: `Scenario::corrupt` entries carry an index into `spec.faults`, and `WorldFactory::corrupt(sim, index)` applies `faults[index]`. Worm 3 belongs to the host's bots, so only peer 1 is wrong.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo nextest run -p rl-cli --test net_repro`
Expected: compile errors.

- [ ] **Step 3: Implement**

- `bundle.rs`: `encode` = magic + version (u16 LE) + postcard; `decode` checks both (an error names what is wrong). `reproduce`: `WorldFactory::new(content, &b.spec)?.sim(HOST)` as the replay world, tick it with `b.inputs` rows (stop at the last row), compare as described. `first_difference`: depth-first over objects (keys in sorted order) and arrays (index order; a length difference is reported at `path.len`), returning the first differing leaf's path (`a.b[2].c`).
- In `netsim.rs` (`run_with`): the harness reports events per peer as they happen (extend `harness::run` with an optional `&mut dyn FnMut(PeerId, &Event, &mut Session<S>)` observer, called right after each `drain_events` with the session; `run` passes a no-op). For a client: on `Event::Desync` start a bundle (spec, peer, fingerprint, platform from `std::env::consts::{ARCH, OS}`, inputs = the session's log rows below its frontier, local checksums, `host = (tick, remote)`, `local_state = Some(session.encode_confirmed_state())`); on `StateArrived` attach `host_state`; write it on `StateLoaded` (or at the end if no state came) as `<dir>/net-<stem>-peer<k>.rlds`. `NetRun` lists written bundles (`pub bundles: Vec<PathBuf>`).
- `rl-cli net-repro <bundle> [--content]` prints the `Repro` in three lines: `local stream: matches the replay` / `local stream: differs from tick T (this machine's simulation differs from the recording peer's)`, `local state at T: equal` / `local state at T differs first at <path>`, and the same for the host state. Exit status 1 if anything differs.

- [ ] **Step 4: Run the tests**

Run: `cargo nextest run -p rl-cli --test net_repro --test net_sim`
Expected: pass.

- [ ] **Step 5: Commit**

CLAUDE.md (rl-cli): "**Desync bundles** (`bundle.rs`): `.rlds` files from `net-sim` (and, in plan 10, real games); `rl-cli net-repro <file>` names the first diverging tick and world field." `cargo xtask check`, then:

```bash
git add crates/rl-cli crates/rl-netcode
git commit -m "rl-cli: desync bundles and net-repro"
```

---
### Task 11: feel numbers and their gate

**Files:**
- Create: `crates/rl-cli/src/feel.rs`, `crates/rl-cli/tests/feel.rs`, `xtask/src/net.rs`, `fixtures/net/feel.ron` (blessed in Step 5), `fixtures/bench/tool-output/feel.ron` (sample)
- Modify: `crates/rl-cli/src/{netsim.rs, net_sim_cmd.rs, lib.rs}` (`--feel <out.ron>`), `xtask/src/{main.rs, check.rs}` (`cargo xtask net-feel [--bless --reason]`, a `net-feel` step in `full`), `crates/rl-cli/CLAUDE.md`, `xtask/CLAUDE.md`
- Test: `tests/feel.rs` (metric definitions on a short run), unit tests in `xtask/src/net.rs` (gate rules)

**Interfaces:**
- Consumes: Task 9's harness observer, `NetSpec`, `run_with`; `Session::stats`, `Report::received`.
- Produces:
  ```rust
  // rl_cli::feel
  #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
  pub struct ProfileFeel {
      pub profile: String,
      pub rollback_p50: u32, pub rollback_p99: u32,          // ticks, over all clients' rollbacks
      pub correction_px_p99: f32,                            // other worms' visible jump, px, over corrections
      pub own_corrections_per_min: f32,                      // per client, the worst client
      pub stalls: u64,                                       // all players
      pub down_bytes_per_s: u32,                             // the worst client, with OVERHEAD per datagram
      pub up_bytes_per_s: u32,
  }
  #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
  pub struct FeelReport { pub note: String, pub profiles: Vec<ProfileFeel>, pub bandwidth32: ProfileFeel }
  pub fn feel_spec(profile: &str, seed: u32) -> NetSpec;   // 16 worms, 7 clients, Classic, Castle Strike, 3000 ticks
  pub fn bandwidth_spec() -> NetSpec;                      // 32 worms, 1 client, typical, 3000 ticks
  pub fn measure(content: &Content) -> anyhow::Result<FeelReport>;   // profiles × seeds 1..=2 in parallel threads
  ```

Context: spec §9.2. Every number is computed in virtual time from seeded runs, so it is the same on every machine: the baseline is not tied to a host. The gate:

- **Absolute rules** (always fail): `stalls == 0` for `lan`, `good`, `typical`, `bad`; `bandwidth32.down_bytes_per_s <= 24_576`.
- **Regression rules** (fail): a metric worse than its baseline by more than 10 % and more than its floor (rollback depth 1 tick, correction 1 px, own corrections 0.5/min, bytes 256 B/s). Better by the same margins: a notice suggesting a re-bless to lock it in.

**Definitions:**
- Rollback depth: every `Event::Rollback { from, to }` of every client: `to - from`; nearest-rank percentiles (the repo's convention).
- Correction: per client, at the end of each `advance` (a frame), remember the displayed tick `D = current - 1` and every *other* worm's position at `D` (the observer reads `WorldSim::world().worms()`). When a later frame's rollback re-simulated tick `D` (seen through the `simulated` callback with `resim: true` and `tick == D`), the correction is the largest distance between a worm's remembered and re-simulated positions; record it if it is above 0.
- Own corrections: `stats.own_corrections` per client, per minute of match time.
- Bytes: `Network::received(peer)` (payload + `OVERHEAD` per datagram) over the match's seconds; down = to a client, up = from a client.

- [ ] **Step 1: Write the failing tests**

`tests/feel.rs`:

```rust
mod common;

use rl_cli::feel::{measure_one, ProfileFeel};
use rl_cli::netsim::NetSpec;

#[test]
fn feel_numbers_make_sense_on_a_short_run() {
    let Some(c) = common::content() else { return };
    let mut spec = rl_cli::feel::feel_spec("typical", 1);
    spec.ticks = 600;
    let f: ProfileFeel = measure_one(&c, &spec).unwrap();
    assert!(f.rollback_p50 >= 1 && f.rollback_p99 <= 28, "{f:?}");
    assert!(f.rollback_p50 <= f.rollback_p99);
    assert_eq!(f.stalls, 0);
    assert!(f.down_bytes_per_s > 0 && f.down_bytes_per_s < 24_576);
    let lan = measure_one(&c, &NetSpec { profile: "lan".into(), ..spec }).unwrap();
    assert!(lan.rollback_p99 <= f.rollback_p99);
}
```

(`measure_one(content, spec) -> anyhow::Result<ProfileFeel>` is one run; `measure` aggregates two seeds per profile by taking, per metric, the worse of the two.)

In `xtask/src/net.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn feel(profile: &str, p99: u32, stalls: u64, down: u32) -> ProfileFeel {
        ProfileFeel { profile: profile.into(), rollback_p50: 3, rollback_p99: p99, correction_px_p99: 4.0,
                      own_corrections_per_min: 0.0, stalls, down_bytes_per_s: down, up_bytes_per_s: 900 }
    }

    #[test]
    fn stalls_up_to_bad_always_fail() {
        let base = feel("bad", 10, 0, 9_000);
        let now = feel("bad", 10, 1, 9_000);
        assert!(compare(&base, &now).failures.iter().any(|f| f.contains("stalls")));
        let awful = feel("awful", 10, 5, 9_000);
        assert!(compare(&awful, &awful).failures.is_empty());
    }

    #[test]
    fn small_changes_pass_and_large_ones_fail() {
        let base = feel("typical", 10, 0, 9_000);
        assert!(compare(&base, &feel("typical", 11, 0, 9_100)).failures.is_empty());
        assert!(!compare(&base, &feel("typical", 13, 0, 9_000)).failures.is_empty());
        assert!(!compare(&base, &feel("typical", 6, 0, 9_000)).notices.is_empty(), "improvements are noticed");
    }
}
```

`compare(base, now) -> Gate { failures: Vec<String>, notices: Vec<String> }` applies the rules to one profile; the bandwidth rule is checked separately on `bandwidth32`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo nextest run -p rl-cli --test feel; cargo test -p xtask net::`
Expected: compile errors.

- [ ] **Step 3: Implement**

`feel.rs` with the definitions above (use the Task 10 observer to collect positions and rollbacks). `rl-cli net-sim --feel <out.ron>` runs `measure` and writes the `FeelReport` (RON, pretty), printing one line per profile. `xtask/src/net.rs`: `cargo xtask net-feel` builds `rl-cli` in release (`cargo build --quiet --release -p rl-cli`), runs `target/release/rl-cli net-sim --feel target/net/feel.ron`, compares with `fixtures/net/feel.ron` (missing baseline: a notice, pass), prints `ok   <profile>` / `FAIL <profile>: <why>` lines and notices; `--bless --reason "<why>"` writes the baseline (with `note: "Seeded, virtual-time feel numbers (plan 9); host-independent."`) and logs the reason in `fixtures/blessings.log` via `crate::bless::log_blessing("net-feel", reason, &changed)`. In `check.rs`'s `full` block add `step("net-feel", ...)` after `stress`. Add the tool-output sample (a valid `FeelReport`) and a test in `rl-cli`'s `tests/tool_output.rs` that it parses (the repo's convention for RON read by xtask), and make `xtask/src/net.rs` read the report strictly (unknown fields refused).

- [ ] **Step 4: Run, measure, bless**

Run: `cargo nextest run -p rl-cli --test feel --test tool_output && cargo test -p xtask net:: && cargo xtask net-feel --bless --reason "plan 9: first feel baselines"`
Expected: tests pass; the bless prints every profile's numbers. Check them against the absolute rules (the command fails if a rule fails — then the netcode has a problem to fix, not the gate). Record the numbers in the task report. The `full` step must take under 3 minutes; if `measure` is slower, run the profiles in parallel threads (`std::thread::scope`, as `tests/grid.rs` does).

- [ ] **Step 5: Commit**

CLAUDE.md (rl-cli, xtask): the feel numbers, their definitions, the gate rules, `cargo xtask net-feel [--bless --reason]`. `cargo xtask check`, then:

```bash
git add crates/rl-cli xtask fixtures/net fixtures/bench/tool-output fixtures/blessings.log
git commit -m "net-feel: seeded feel numbers for every network profile, gated in full"
```

---

### Task 12: the rollback cost: benchmark and envelope

**Files:**
- Create: `crates/rl-cli/benches/rollback.rs`
- Modify: `crates/rl-cli/Cargo.toml` (`[[bench]] name = "rollback" harness = false`), `xtask/src/bench.rs` (`BENCHES` gains `("rl-cli", "rollback")`), `fixtures/bench/baselines.ron` (bless), `crates/rl-cli/src/stress.rs` or a new `crates/rl-cli/src/rollback_cost.rs` + `rl-cli rollback-cost` command, `xtask/src/envelope/{mod.rs, sweeps.rs, results.rs, report.rs}` (an `Axis::Rollback`), `fixtures/bench/envelope/<host>.ron` (bless), `docs/perf/envelope.md` (regenerated), `fixtures/bench/tool-output/rollback.ron`
- Test: the bench compiles and runs; envelope's existing unit tests plus one for the new axis's report row; `tests/tool_output.rs` for the new sample

**Interfaces:**
- Consumes: `WorldSim` (Task 8), the generated levels of plan 8 (`rl_cli::genlevel`, names like `gen-4096x4096-a1-s1`), `rl_content` matches.
- Produces: `rl-cli rollback-cost --level <id> [--mod] [--worms 16] [--depth 12] [--samples 300] --out <ron>` printing `RollbackCost { level, worms, depth, restore_us_p50, restore_us_p99, resim_us_p50, resim_us_p99, total_us_p99, snapshot_bytes, journal_bytes_max }`; the envelope axis `rollback` (points: Castle Strike 16 worms depth 12 and 28; `gen-4096x4096-a1-s1` 16 worms depth 12), its table in the report, and the budget checks: total p99 ≤ 2000 µs (reference, depth 12) and ≤ 4000 µs (4096², depth 12) flagged in the report like other breaking points.

Context: spec §3.3's budget, decision N1. A sample: play the match with bots to a random tick (warm-up 500 ticks, then one sample every 10 ticks), save, tick `depth` ticks with the bots' keys, then time `restore` and the `depth` re-simulated ticks (the replayed keys), separately. Nearest-rank percentiles.

- [ ] **Step 1: The bench** (`benches/rollback.rs`, divan, needs content; prints a skip notice without `RL_CONTENT_DIR` like `benches/game.rs`): `rollback_12_castlestrike` — Classic, Castle Strike, 16 bots' recorded keys (record 600 ticks once in setup), each iteration restores a snapshot taken 12 ticks back and re-simulates the 12 ticks. Run `cargo bench -p rl-cli --bench rollback`; expect well under 2 ms.

- [ ] **Step 2: The command** `rl-cli rollback-cost` as described, with a unit test of its percentile/report code on fake samples, and a tool-output sample + `tests/tool_output.rs` entry.

- [ ] **Step 3: The envelope axis**: add `Axis::Rollback` (in `Axis::ALL` and the clap value list), a sweep step running `rl-cli rollback-cost` in release for the three points, strict reading of its RON, a results field (lenient: old results files without it still read), a report table "Rollback" (points, restore and re-simulation p50/p99, total p99 against the budget), and the budget flags. Follow how the `map` axis was added in plan 8 (`xtask/src/envelope/`); the envelope's own unit tests show the patterns.

- [ ] **Step 4: Measure and bless**

Run: `cargo xtask bench --bless --reason "plan 9: the rollback benchmark" && cargo xtask envelope --only rollback --bless --reason "plan 9: the rollback axis" && cargo xtask envelope --report`
Expected: both budgets hold (if one does not, the report says so: record the numbers in the task report and keep going — the budget is information for plan 10, not a reason to stop). The envelope's hand-written summary (between the markers) gains one sentence: "Rollback: restoring and re-simulating 12 ticks costs <x> ms p99 at the reference configuration and <y> ms at 4096 × 4096 (budgets 2 and 4 ms)." with the measured numbers.

- [ ] **Step 5: Commit**

`cargo xtask check`, then:

```bash
git add crates/rl-cli xtask fixtures/bench docs/perf/envelope.md
git commit -m "envelope: the rollback cost (restore and re-simulation) with its budgets"
```

---

### Task 13: cross-platform determinism (Linux x86-64, WASM, Windows, Linux arm64)

**Files:**
- Create: `crates/rl-detcheck/{Cargo.toml, CLAUDE.md, src/main.rs}`, `tools/wasi-run.mjs`, `xtask/src/determinism.rs`
- Modify: `rust-toolchain.toml` (`targets` gains `"wasm32-wasip1"` and `"aarch64-unknown-linux-gnu"`), `.cargo/config.toml` (`[target.aarch64-unknown-linux-gnu] linker = "aarch64-linux-gnu-gcc"`), `xtask/layers.ron` (`"rl-detcheck": 6`), `xtask/src/{main.rs, check.rs, winbuild.rs}` (`EXES` and `build()` gain `rl-detcheck`), `xtask/CLAUDE.md`
- Test: unit tests in `xtask/src/determinism.rs` (stream comparison), the step itself

**Interfaces:**
- Consumes: `rl_replay::Replay::{decode, ..}`, `rl_content::Content::{open, world_for_replay}`, `World::checksum`; `crate::winbuild::{missing, build, stage, run, RUN_TIMEOUT}`.
- Produces: the binary `rl-detcheck --content <dir> [--every N] <replay.rlrp>...` printing `<file name> <tick> <checksum hex>` lines (tick 0 and every `N`, default 10, and the last tick), exit status 1 if a replay's own recorded checksums do not match; `cargo xtask determinism [--windows] [--arm64]`; a `determinism` step in `check` (WASM leg) and `determinism-windows` and `determinism-arm64` in `full`.

Context: spec §9.5. Rollback rests on identical simulation everywhere. The tool replays recorded replays (keys only, so bots' own maths are not under test: bots think on the host only) and prints the world checksum stream; xtask compares the native stream with the WASM one (a `wasm32-wasip1` build of the same tool, run by Node.js's built-in WASI) and, in `full`, the Windows one (the plan 8 interop runner).

- [ ] **Step 1: Spike: does the replay stack build for WASI?**

Run: `rustup target add wasm32-wasip1`, create the crate (Step 2), then `cargo build -p rl-detcheck --target wasm32-wasip1`. Expected: it builds (`rl-content`'s dependencies are pure Rust). If a dependency does not build for `wasm32-wasip1`, stop and report which (BLOCKED with the error): the plan then needs a ruling on the WASM leg.

- [ ] **Step 2: The tool**

`crates/rl-detcheck/Cargo.toml`: binary crate, dependencies `rl-content`, `rl-replay`, `rl-sim`, `anyhow`, `clap`. `src/main.rs`:

```rust
//! Prints the world checksum stream of replays, to compare platforms
//! (networking spec §9.5): native, WASI (`tools/wasi-run.mjs`) and Windows.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

#[derive(Parser)]
#[command(name = "rl-detcheck", about = "Prints replays' checksum streams")]
struct Args {
    #[arg(long, default_value = "content/packs")]
    content: PathBuf,
    #[arg(long, default_value_t = 10)]
    every: u32,
    replays: Vec<PathBuf>,
}

fn main() -> ExitCode {
    match run(&Args::parse()) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(a: &Args) -> anyhow::Result<bool> {
    let content = rl_content::Content::open(&a.content)?;
    let mut ok = true;
    for path in &a.replays {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let replay = rl_replay::Replay::decode(&std::fs::read(path)?)?;
        let mut world = content.world_for_replay(&replay)?;
        let mut events = rl_sim::TickEvents::new();
        let mut cursor = rl_replay::Cursor::default();
        println!("{name} 0 {:016x}", world.checksum());
        let mut tick = 0u32;
        loop {
            match cursor.step(&replay, &mut world, &mut events) {
                Ok(true) => {}
                Ok(false) => break,
                Err(m) => {
                    eprintln!("{name}: {m:?}");
                    ok = false;
                    break;
                }
            }
            tick += 1;
            if tick % a.every == 0 || tick == replay.ticks() {
                println!("{name} {tick} {:016x}", world.checksum());
            }
        }
    }
    Ok(ok)
}
```

Check `Cursor`'s constructor and `step`'s return contract with `cargo xtask api` (`target/api/rl_replay.txt`) and adapt.

`tools/wasi-run.mjs`:

```js
// Runs a wasm32-wasip1 binary with Node.js's WASI: node tools/wasi-run.mjs <file.wasm> <dir to preopen as /> [args...]
import { readFile } from 'node:fs/promises';
import { WASI } from 'node:wasi';
import { argv, exit } from 'node:process';

const [wasmPath, root, ...args] = argv.slice(2);
const wasi = new WASI({ version: 'preview1', args: ['rl-detcheck', ...args], preopens: { '/': root }, returnOnExit: true });
const module = await WebAssembly.compile(await readFile(wasmPath));
const instance = await WebAssembly.instantiate(module, wasi.getImportObject());
exit(wasi.start(instance));
```

The xtask passes the repository root as `root` and paths relative to it (`/content/packs`, `/target/determinism/x.rlrp`).

- [ ] **Step 3: The xtask** (`determinism.rs`)

1. Record the replays natively: for each golden spec in `fixtures/matches/*.ron` (in `check`: only `mw-castlestrike-8chaos`; with `--all`/in `full`: all three) run `cargo run --quiet --release -p rl-cli -- match <spec flags> --record target/determinism/<stem>.rlrp --no-verify` — read the spec flags from the golden's `spec` (`rl_cli::golden::Golden` is in rl-cli's library; xtask may depend on it, or shell out with the flags built from the RON fields).
2. Native stream: `cargo run --quiet --release -p rl-detcheck -- --content content/packs <replays>`.
3. WASM leg: skip with `skip determinism (<why>)` when `node --version` fails (notice: `sudo apt install nodejs`) or `rustup target list --installed` lacks `wasm32-wasip1`; else `cargo build --quiet --release -p rl-detcheck --target wasm32-wasip1` and `node tools/wasi-run.mjs target/wasm32-wasip1/release/rl-detcheck.wasm <repo root> --content /content/packs /target/determinism/<stem>.rlrp`.
4b. **arm64 leg** (`--arm64`, the `full` step `determinism-arm64`; the user asked for it on 2026-10-12 so a dedicated server or the game can later run on arm64 Linux): skip with `skip determinism-arm64 (<why>)` when `qemu-aarch64` is not on `PATH`, `aarch64-linux-gnu-gcc` is missing or `rustup target list --installed` lacks `aarch64-unknown-linux-gnu` (notice: `sudo apt install qemu-user gcc-aarch64-linux-gnu` and `rustup target add aarch64-unknown-linux-gnu`); else `cargo build --quiet --release -p rl-detcheck --target aarch64-unknown-linux-gnu` and run `qemu-aarch64 -L /usr/aarch64-linux-gnu target/aarch64-unknown-linux-gnu/release/rl-detcheck --content content/packs <replays>` (all three replays; QEMU is several times slower, so it is in `full` only). Compare with the native stream like the other legs.
4. Windows leg (`--windows`, the `full` step): skip with a notice when `winbuild::missing()` says why; else `winbuild::build()`, `stage(&release, content, &replays)`, `run(&stage, "rl-detcheck.exe", ["--content", "content", "replays/<stem>.rlrp", ..], &[], RUN_TIMEOUT)` and parse stdout.
5. Compare: `fn compare(a: &str, b: &str) -> Result<(), String>` — the first differing line (`<file> <tick>: native <x>, wasm <y>`), or a length difference. Unit tests: equal streams pass; a differing checksum names file and tick; a missing tail is reported.

`check.rs`: `step("determinism", ...)` after `test` in `check`; `step("determinism-windows", ...)` and `step("determinism-arm64", ...)` in `full`. The `check` step must take under 30 s warm (one replay, release builds cached); record the time in the task report.

- [ ] **Step 4: Run**

Run: `sudo apt install nodejs` is the user's step — if `node` is missing, the step prints the skip notice; verify that path, and tell the controller in the report so the user gets the apt command. Then `cargo xtask determinism && cargo xtask determinism --windows`.
Then `cargo xtask determinism --arm64` (if QEMU or the cross linker is missing, verify the skip notice and report the apt command for the user).
Expected: the streams match (or skips with notices). A mismatch is a real finding: stop and report it with the first differing tick (it would block rollback across platforms).

- [ ] **Step 5: Commit**

CLAUDE.md for rl-detcheck (layer 6, std; native, WASI, Windows, arm64 under QEMU; not a game tool) and xtask's (`determinism` in `check`, `determinism-windows` and `determinism-arm64` in `full`, the Node.js, QEMU and cross-linker requirements). `cargo xtask check`, then:

```bash
git add rust-toolchain.toml .cargo/config.toml xtask crates/rl-detcheck tools/wasi-run.mjs
git commit -m "determinism: compare checksum streams on Linux, WASM and Windows"
```

---

### Task 14: records: decisions, changelog, status

**Files:**
- Modify: `docs/porting/STATUS.md` (new section `## Networking decisions (plan 9)`), `CHANGELOG.md` (`## Unreleased` → `### Added`), `docs/superpowers/specs/2026-10-12-networking-design.md` (status line only: "Approved 2026-10-12; plan 9 implemented")
- Test: `cargo xtask check`

- [ ] **Step 1: STATUS.md** — the section with N1-N7 from this plan's "Decisions" (one bullet each, same wording, each citing `docs/superpowers/plans/2026-10-12-plan-9-netcode-core.md`), plus any rulings the controller recorded in the plan's ledger during execution that change behaviour (the controller supplies them in the dispatch).

- [ ] **Step 2: CHANGELOG.md** — under `### Added`, user-visible effects in the file's prose style:
  - "The netcode core for online play (not yet playable from the game): rollback with the host ordering inputs, spectators, desync detection and repair, tested by whole online matches with bots over simulated networks (`rl-cli net-sim`)."
  - "`rl-cli net-repro` reads a desync bundle and names the first tick and world field that differ."
  - "The simulation is checked to give identical results on Linux, in the browser's WebAssembly, on Windows and on arm64 Linux (`cargo xtask determinism`; the WebAssembly leg needs `sudo apt install nodejs`, the arm64 leg `sudo apt install qemu-user gcc-aarch64-linux-gnu`)."

- [ ] **Step 3: Commit**

`cargo xtask check`, then:

```bash
git add docs/porting/STATUS.md CHANGELOG.md docs/superpowers/specs/2026-10-12-networking-design.md
git commit -m "docs: plan 9's networking decisions and changelog"
```
