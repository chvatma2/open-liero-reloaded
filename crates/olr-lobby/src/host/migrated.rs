//! The migrated lobby (plan 12c, N72): a successor's lobby, built from the
//! old host's last `LobbyState` and succession order, and the members'
//! rejoins by key.

use alloc::collections::{BTreeSet, VecDeque};
use alloc::vec::Vec;
use core::net::{IpAddr, Ipv4Addr};

use olr_proto::{LobbyState, Notice, Path, Phase, SeatWire, SuccessorWire, ToClient, valid_succession};

use super::{LobbyHost, Rec, Standby};
use crate::identity::Identity;
use crate::model::*;

impl LobbyHost {
    /// A successor's lobby (N72), built from the old lobby's `state` and
    /// `succession` with `cfg`: the wire carries no `HostConfig`, so the
    /// online layer gives the state's name, `max_players` = the match's
    /// player slots, `max_spectators` = `MAX_SPECTATORS` and the admins
    /// from the members' marks; its password is ignored (there is none).
    ///
    /// `match_ids` are the running match's member ids (its `MatchStartWire`
    /// slots): all stay reserved, so the id of a player who left before the
    /// migration is not given to anyone while its slot plays.
    ///
    /// `key` must be a member's in both (else `LobbyError::NotASuccessor`);
    /// the online layer promotes only an entry that is `can_host` (a
    /// follower has no world to host from).
    /// That member is the host's own (ready, an admin), keeping its seat; a
    /// spectator successor hosts as a spectator for the rest of the match
    /// and is seated at the next gathering like any member who wants to
    /// play. The old host's member is gone. Every other succession member
    /// is kept with its id, look, seat and admin mark, away until it
    /// rejoins; a member the succession does not name is gone too. The
    /// phase is the state's. The lobby is closed: only the succession's keys
    /// join (strangers are refused `Closed`), without a password; the online
    /// layer does not announce it.
    pub fn migrated(
        key: Identity,
        state: &LobbyState,
        succession: &[SuccessorWire],
        match_ids: &[u8],
        cfg: HostConfig,
    ) -> Result<LobbyHost, LobbyError> {
        let me_key = key.public();
        if !valid_succession(succession)
            || !succession.iter().any(|s| s.key == me_key)
            || !state.members.iter().any(|m| m.key == me_key)
        {
            return Err(LobbyError::NotASuccessor);
        }
        let cfg = HostConfig { password: None, ..cfg };
        let mut host = LobbyHost::new(cfg, key, None, state.setup.clone())?;
        host.phase = state.phase;
        // The running match's ids stay reserved, the old host's included.
        host.in_match = state
            .members
            .iter()
            .filter(|m| matches!(m.seat, SeatWire::Player { .. }))
            .map(|m| m.id)
            .chain(match_ids.iter().copied())
            .collect();
        host.next_id = state
            .members
            .iter()
            .map(|m| m.id)
            .chain(match_ids.iter().copied())
            .max()
            .map_or(0, |id| id.wrapping_add(1));
        for m in &state.members {
            let Some(s) = succession.iter().find(|s| s.key == m.key && s.member == m.id) else { continue };
            let mine = m.key == me_key;
            // What the old host knew of its standby, as far as the wire
            // carried it.
            let standby = (s.can_host || s.direct.is_some() || !s.relays.is_empty())
                .then(|| Standby { port: s.direct.map(|(_, port)| port), relays: s.relays.clone() });
            host.members.push(Rec {
                id: m.id,
                peer: None,
                addr: None,
                path: None,
                key: m.key,
                look: m.look.clone(),
                seat: m.seat,
                // A spectator successor is seated at the next gathering; any
                // other spectator keeps watching until it says otherwise.
                wants_play: mine || m.seat != SeatWire::Spectator,
                said_ready: mine || m.ready,
                missing: if mine { Vec::new() } else { m.missing.clone() },
                have_revision: None,
                known: true,
                download: None,
                download_next: None,
                download_at: None,
                awaiting_content: !mine && m.seat == SeatWire::Waiting,
                loadout: None,
                admin: m.admin || host.cfg.admins.contains(&m.key),
                ping_ms: None,
                chat: VecDeque::new(),
                looks: VecDeque::new(),
                look_held: None,
                // The state does not say who got the `Start`: a player did,
                // and a spectator the old host listed as `can_host` was
                // one from the start (a spectator without a standby that
                // was too is not waited for: the limit of the wire).
                in_start: m.seat != SeatWire::Waiting && (m.seat != SeatWire::Spectator || s.can_host),
                inherited_addr: s.direct.map(|(ip, _)| IpAddr::V4(Ipv4Addr::from(ip))).filter(|_| !mine),
                standby: if mine { None } else { standby },
                standby_at: None,
                standby_held: None,
            });
            if mine {
                host.me = Some(m.id);
                let i = host.members.len() - 1;
                host.members[i].admin = true;
            }
        }
        host.closed = Some(succession.iter().map(|s| s.key).collect::<BTreeSet<_>>());
        host.refresh();
        host.sent_succession = Some(host.succession());
        Ok(host)
    }

    /// Members of a migrated lobby that have not rejoined, in join order.
    pub fn away(&self) -> Vec<u8> {
        self.members.iter().filter(|m| m.peer.is_none() && Some(m.id) != self.me).map(|m| m.id).collect()
    }

    /// The away members the new host waits for (N68): those holding a slot
    /// of the match, or a spectator seat from its start; not followers
    /// (mid-match spectators and waiting members hold neither).
    pub fn away_in_match(&self) -> Vec<u8> {
        self.members
            .iter()
            .filter(|m| m.peer.is_none() && Some(m.id) != self.me)
            .filter(|m| m.in_start)
            .map(|m| m.id)
            .collect()
    }

    /// The resume (N68): the away members leave (`Notice(Left)`,
    /// `HostEvent::Left`). Their keys may still join, as mid-match joiners.
    pub fn drop_away(&mut self) {
        for id in self.away() {
            self.drop_member(id, super::Gone::Left);
        }
    }

    /// The away member `i` proved its key on a new link: it takes its old
    /// id and seat back and is welcomed with the current state.
    pub(super) fn rejoin(&mut self, peer: olr_netcode::PeerId, addr: IpAddr, path: Path, i: usize) {
        let id = self.members[i].id;
        self.links.insert(peer, super::Link::Member(id));
        let known = self.content_revision.is_none();
        let m = &mut self.members[i];
        m.peer = Some(peer);
        m.addr = Some(addr);
        m.inherited_addr = None;
        m.path = Some(path);
        m.known = known;
        m.have_revision = None;
        self.publish();
        self.out.push((peer, ToClient::Welcome { you: id, lobby: self.state.clone() }));
        // The succession comes with the next drain when it changed (its
        // address is the link's now); else the rejoiner gets the one in
        // force now. Its standby stays what the old host knew until it
        // reports again.
        if matches!(self.phase, Phase::Starting | Phase::Playing { .. }) {
            let list = self.succession();
            if self.sent_succession.as_ref() == Some(&list) {
                self.out.push((peer, ToClient::Succession(list)));
            }
        }
        self.flush_except(Some(peer));
        self.broadcast(&ToClient::Notice(Notice::Joined(id)));
        self.events.push(HostEvent::Rejoined(id));
    }
}
