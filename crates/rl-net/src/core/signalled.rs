//! `Core`'s signalled setups (plan 12b, N48-N50): the socket's
//! candidates and STUN, the ICE offer and answer (PF12b-2's pending
//! table), and each link's path (N49) and `Connected` report.

use super::*;
use crate::candidates::MAX_CANDIDATES;

impl Core {
    /// The socket's candidates: `base` is where every datagram is taken as arriving; `hosts` (base first) are offered.
    pub fn set_base(&mut self, base: SocketAddr, hosts: Vec<SocketAddr>) {
        self.base = Some(base);
        self.hosts = vec![base];
        for h in hosts {
            if !self.hosts.contains(&h) {
                self.hosts.push(h);
            }
        }
    }

    /// STUN servers asked from this socket now and every STUN_EVERY (two answers give NatBehaviour).
    pub fn stun(&mut self, servers: Vec<SocketAddr>, now: Instant) {
        self.probe.start(servers, now);
    }

    /// The router mapping's external address (a server-reflexive candidate
    /// at it), or `None`.
    pub fn set_mapped(&mut self, mapped: Option<SocketAddr>) {
        self.mapped = mapped;
    }

    /// Our candidates now; `srflx` is the latest STUN answer (a changed
    /// router address is offered from the next round on).
    pub fn local(&self) -> LocalCandidates {
        LocalCandidates {
            base: self.base,
            host: self.hosts.clone(),
            srflx: self.probe.srflx(),
            mapped: self.mapped,
            relayed: Vec::new(),
        }
    }

    pub fn nat(&self) -> NatBehaviour {
        self.probe.nat(self.base)
    }

    /// STUN answered (or STUN_WAIT passed) — and, from Task 3, TURN too.
    pub fn gathered(&self, now: Instant) -> bool {
        self.probe.gathered(now)
    }

    /// What a signalled link offers: the host candidates, and the
    /// server-reflexive and mapped addresses on the base (unless equal to
    /// it).
    /// They fit the far side's `sanitize` (at most `MAX_CANDIDATES`): the
    /// reflexive ones (and, from Task 3, relayed) first, then the host
    /// candidates, base first, as many as are left room.
    pub(super) fn link_candidates(&self) -> LinkCandidates {
        let mut c = LinkCandidates::default();
        if let Some(base) = self.base {
            for addr in [self.probe.srflx(), self.mapped].into_iter().flatten() {
                if addr != base && addr.is_ipv4() && !c.srflx.iter().any(|&(a, _)| a == addr) {
                    c.srflx.push((addr, base));
                }
            }
        }
        let room = MAX_CANDIDATES.saturating_sub(c.srflx.len() + c.relayed.len());
        c.host = self.hosts.iter().copied().take(room).collect();
        c
    }

    /// Client, signalled: an offerer link with every local candidate.
    pub fn offer_ice(&mut self, peer_hint: SocketAddr, now: Instant) -> (SetupId, String) {
        let (link, sdp) = Link::offerer_with(&self.link_candidates(), now);
        let base = self.base.unwrap_or((Ipv4Addr::UNSPECIFIED, 0).into());
        (self.add(peer_hint, base, link, now, Some(RemoteCandidates::default())), sdp)
    }

    /// Client: the host's answer and its mapped hint (no F-G rewrite).
    /// The answer is sanitized as an offer is (N50).
    pub fn accept_answer_ice(
        &mut self,
        setup: SetupId,
        answer: &str,
        mapped_hint: Option<SocketAddr>,
    ) -> Result<(), LinkError> {
        let answer = sanitize(answer);
        let Some(p) = self.pending.iter_mut().find(|p| p.setup == setup) else {
            return Err(LinkError::Closed);
        };
        p.link.accept_answer(&answer)?;
        p.theirs = remote_candidates(&answer, mapped_hint);
        Ok(())
    }

    /// Host, signalled: admit, sanitize, an answerer link with every local candidate; Answer or Refused.
    /// With MAX_SIGNALLED_PENDING setups pending, the oldest one that has received no ICE connectivity
    /// check (STUN binding request) from its peer is closed to make room (PF12b-2); if every one has, Refused { Busy }.
    /// The evicted setup's `SetupFailed { why: Timeout }` comes with the next poll.
    pub fn answer_ice(&mut self, offer: &Signal, mapped_hint: Option<SocketAddr>, now: Instant) -> Signal {
        let refused = |reason| Signal::Refused { reason };
        if let Err(why) = self.admit(offer) {
            return refused(why);
        }
        let Signal::Offer { sdp, .. } = offer else { return refused(Refusal::Malformed) };
        let signalled = self.pending.iter().filter(|p| p.signalled).count();
        let victim = if signalled >= MAX_SIGNALLED_PENDING {
            // Setup ids grow: the smallest is the oldest.
            let silent = self.pending.iter().filter(|p| p.signalled && !p.checked).min_by_key(|p| p.setup);
            match silent {
                Some(p) => Some(p.setup),
                None => return refused(Refusal::Busy),
            }
        } else {
            None
        };
        let sdp = sanitize(sdp);
        let Ok((link, answer)) = Link::answerer_with(&self.link_candidates(), &sdp, now) else {
            return refused(Refusal::Malformed);
        };
        if let Some(i) = victim.and_then(|v| self.pending.iter().position(|p| p.setup == v)) {
            let peer = self.pending.remove(i);
            let (mut out, mut events) = (std::mem::take(&mut self.outbox), std::mem::take(&mut self.events));
            self.fail_setup(peer, Why::Timeout, now, &mut out, &mut events);
            (self.outbox, self.events) = (out, events);
        }
        let base = self.base.unwrap_or((Ipv4Addr::UNSPECIFIED, 0).into());
        let addr = (Ipv4Addr::UNSPECIFIED, 0).into();
        self.add(addr, base, link, now, Some(remote_candidates(&sdp, mapped_hint)));
        Signal::Answer { sdp: answer }
    }

    /// The path of an open (reported) peer's link: its selected pair's
    /// class, re-classified as the pair changes (`Direct` for a
    /// TCP-signalled link).
    pub fn path(&self, peer: PeerId) -> Option<Path> {
        self.open.get(&peer).filter(|p| p.reported).and_then(|p| p.path)
    }

    /// The link's selected candidate pair (none under the `suppress_stats`
    /// test hook).
    pub(super) fn selected(&self, peer: &Peer) -> Option<(SocketAddr, SocketAddr)> {
        #[cfg(feature = "test-hooks")]
        if self.cfg.suppress_stats {
            return None;
        }
        peer.link.selected_pair()
    }

    /// The path of a peer's link: its selected pair's class (N49), or
    /// `HolePunched` without one; `Direct` for a TCP-signalled link.
    pub(super) fn classify(&self, peer: &Peer) -> Path {
        match (peer.signalled, peer.pair) {
            (false, _) => Path::Direct,
            (true, Some((local, remote))) => classify(local, remote, &self.local(), &peer.theirs),
            (true, None) => Path::HolePunched,
        }
    }

    /// Reports an opened peer `Connected` (once) with its path, then the
    /// events that waited for it.
    /// A signalled peer's address: its selected pair's remote address,
    /// else the latest source its link took a datagram from (never the
    /// unspecified placeholder once it has sent one: the lobby's IP bans
    /// key on it), else the hint.
    pub(super) fn locate(&self, peer: &mut Peer) {
        if !peer.signalled {
            return;
        }
        if let Some(addr) = peer.pair.map(|(_, remote)| remote).or(peer.from) {
            peer.addr = addr;
        }
    }

    pub(super) fn report(&self, peer: &mut Peer, events: &mut Vec<CoreEvent>) {
        let Some(id) = peer.id else { return };
        if peer.reported {
            return;
        }
        peer.reported = true;
        peer.pair = self.selected(peer);
        let path = self.classify(peer);
        peer.path = Some(path);
        self.locate(peer);
        events.push(CoreEvent::Net(NetEvent::Connected { peer: id, addr: peer.addr, path }));
        events.extend(peer.held.drain(..).map(CoreEvent::Net));
    }
}
