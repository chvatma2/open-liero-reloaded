//! A client's standing migration relays (ruling FR12c-5, final review
//! B-I3): from its match's start, a pool on its migration relays is
//! connected and the announced TURN relays are fetched, so a migration
//! through the relays pays only the handshake and ICE, not a pool's
//! connects and the TURN notes' wait (1 to 4 s) at every attempt.
//!
//! It subscribes to nothing keyed to this peer (N80: no key is revealed
//! before its turn); the TURN notes' subscription names a kind and a tag
//! only, and ends once they are in (`TURN_AFTER_EOSE` after the first
//! `EOSE`, `TURN_AT_MOST` at the latest), fetched again every
//! [`TURN_REFRESH`] (a note lives 180 s).
//!
//! An attempt through the relays borrows the pool ([`MigrationRelays::lend`])
//! when it asks on relays the pool has, and gives it back when it ends;
//! the successor's listener borrows it in its turn and sends it back when
//! its gate closes (at the commit it keeps it: the host listens there).
//! The idle cost: one WebSocket per relay (at most `MAX_JOIN_RELAYS`, 8,
//! each pinged after 30 s of quiet) and one TURN request at the start and
//! every 2 minutes. Dropped (at the match's end, the peer's) on a thread.

use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use rl_discovery::{Pool, PoolConfig, PoolEvent, Region, TurnList};

use crate::join::{
    Lent, MAX_TURN, TURN_AFTER_EOSE, TURN_AT_MOST, TURN_SUB, drop_pool, join_relays, unix_now,
};
use crate::migration::MigrationConfig;

/// The TURN notes are fetched again this often while the match runs.
pub(crate) const TURN_REFRESH: Duration = Duration::from_secs(120);

/// A client's migration pool, connected from its match's start.
pub(crate) struct MigrationRelays {
    /// The pool's relays: the migration's own, each once, at most
    /// `MAX_JOIN_RELAYS`.
    cfg: PoolConfig,
    /// `None` while lent.
    pool: Option<Pool>,
    /// Where a listener sends a lent pool back.
    back: Sender<Pool>,
    returned: Receiver<Pool>,
    /// The announced TURN relays (`None`: not asked for).
    turn: Option<TurnList>,
    region: Region,
    /// The fetch under way: when it began, and its first `EOSE`.
    fetching: Option<(Instant, Option<Instant>)>,
    /// When the last fetch ended.
    fetched: Option<Instant>,
}

impl MigrationRelays {
    /// The pool on `m`'s relays, connecting from now, and the first TURN
    /// fetch; `None` without migration or relays.
    pub(crate) fn start(m: &MigrationConfig, now: Instant) -> Option<MigrationRelays> {
        let own = m.relays.as_ref().filter(|_| m.enabled)?;
        let relays = join_relays(&own.relays, &[]);
        if relays.is_empty() {
            return None;
        }
        let cfg = PoolConfig { relays, ..own.clone() };
        let turn = m.turn_from_nostr.then(|| {
            let mut list = TurnList::new();
            list.allow_local(m.allow_local);
            list
        });
        let (back, returned) = mpsc::channel();
        let mut r = MigrationRelays {
            pool: Some(Pool::start(cfg.clone())),
            cfg,
            back,
            returned,
            turn,
            region: m.region,
            fetching: None,
            fetched: None,
        };
        r.fetch(now);
        Some(r)
    }

    /// The pool's configuration: what a listener on it listens on.
    pub(crate) fn config(&self) -> &PoolConfig {
        &self.cfg
    }

    fn fetch(&mut self, now: Instant) {
        if let (Some(_), Some(pool)) = (&self.turn, &mut self.pool) {
            pool.subscribe(TURN_SUB, vec![TurnList::filter(unix_now())]);
            self.fetching = Some((now, None));
        }
    }

    /// The fetch under way ends (its subscription with it).
    fn end_fetch(&mut self, now: Instant) {
        if self.fetching.take().is_some() {
            if let Some(pool) = &mut self.pool {
                pool.unsubscribe(TURN_SUB);
            }
            self.fetched = Some(now);
        }
    }

    /// Every poll: a pool sent back is taken, the pool's news drained (the
    /// TURN notes taken), a fetch ended or begun.
    pub(crate) fn poll(&mut self, now: Instant) {
        while let Ok(pool) = self.returned.try_recv() {
            self.give_back(pool);
        }
        let Some(pool) = &mut self.pool else { return };
        let mut got = Vec::new();
        pool.poll(&mut got);
        let unix = unix_now();
        for e in got {
            match e {
                PoolEvent::Eose { sub, .. } if sub == TURN_SUB => {
                    if let Some((_, eose)) = &mut self.fetching {
                        eose.get_or_insert(now);
                    }
                }
                PoolEvent::Event { event, .. } => {
                    if let Some(list) = &mut self.turn {
                        list.offer(&event, unix);
                    }
                }
                _ => {}
            }
        }
        if let Some((began, eose)) = self.fetching
            && (eose.is_some_and(|t| now >= t + TURN_AFTER_EOSE) || now >= began + TURN_AT_MOST)
        {
            self.end_fetch(now);
        }
        if self.fetching.is_none() && self.fetched.is_some_and(|t| now >= t + TURN_REFRESH) {
            self.fetch(now);
        }
    }

    /// The pool for a join asking on `relays`, with the TURN relays picked
    /// (a fetch under way ends with what it has); `None` while lent, or
    /// when the join asks a relay the pool lacks (it makes its own then).
    pub(crate) fn lend(&mut self, relays: &[String], now: Instant) -> Option<Lent> {
        let same = |a: &str, b: &str| a.trim_end_matches('/') == b.trim_end_matches('/');
        if self.pool.is_none() || !relays.iter().all(|r| self.cfg.relays.iter().any(|k| same(k, r))) {
            return None;
        }
        self.end_fetch(now);
        let unix = unix_now();
        let picks = self.turn.as_mut().map_or_else(Vec::new, |list| {
            list.expire(unix);
            list.pick(self.region, MAX_TURN)
        });
        let pool = self.pool.take()?;
        Some(Lent { pool, relays: self.cfg.relays.clone(), picks })
    }

    /// The pool for the successor's listener in its turn, and where to send
    /// it back; `None` while lent.
    pub(crate) fn lend_to_listener(&mut self, now: Instant) -> Option<(Pool, Sender<Pool>)> {
        self.end_fetch(now);
        Some((self.pool.take()?, self.back.clone()))
    }

    /// A lent pool, back.
    pub(crate) fn give_back(&mut self, pool: Pool) {
        if self.pool.is_none() {
            self.pool = Some(pool);
        } else {
            drop_pool(pool);
        }
    }
}

impl Drop for MigrationRelays {
    fn drop(&mut self) {
        if let Some(pool) = self.pool.take() {
            drop_pool(pool);
        }
        for pool in self.returned.try_iter() {
            drop_pool(pool);
        }
    }
}
