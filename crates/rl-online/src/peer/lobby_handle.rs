//! The lobby's actions on an [`OnlinePeer`]: the host's own, or a client's
//! requests to its host.

use super::*;

/// The lobby's actions, for the host or a client.
pub struct LobbyHandle<'a> {
    pub(super) peer: &'a mut OnlinePeer,
}

impl LobbyHandle<'_> {
    /// The lobby as last known (a client's once welcomed).
    pub fn state(&self) -> Option<&LobbyState> {
        match &self.peer.side {
            Side::Host(h) => Some(h.lobby.state()),
            Side::Client(c) => c.lobby.state(),
        }
    }

    /// The host's lobby revision: it changes whenever [`LobbyHandle::state`]
    /// does (`LobbyHost::revision`; `None` on a client).
    pub fn revision(&self) -> Option<u64> {
        match &self.peer.side {
            Side::Host(h) => Some(h.lobby.revision()),
            Side::Client(_) => None,
        }
    }

    /// This peer's member id.
    pub fn me(&self) -> Option<u8> {
        self.peer.me()
    }

    /// Whether the host could start the match now (a client cannot).
    pub fn can_start(&self) -> Result<(), StartError> {
        match &self.peer.side {
            _ if self.peer.game.is_some() => Err(StartError::Running),
            Side::Host(h) => h.lobby.can_start().map_err(StartError::Lobby),
            Side::Client(_) => Err(StartError::NotHost),
        }
    }

    /// Moves a member to the spectators or back to the players (the
    /// host's; a client's does nothing: no admin command moves seats).
    pub fn move_seat(&mut self, member: u8, spectate: bool) {
        self.act(|h| h.move_seat(member, spectate), |_| {});
    }

    pub fn set_ready(&mut self, ready: bool) {
        self.act(|h| h.set_my_ready(ready), |c| c.set_ready(ready));
    }

    /// Watch the next matches (or play them again).
    pub fn set_spectate(&mut self, spectate: bool) {
        let me = self.me();
        self.act(
            |h| {
                if let Some(me) = me {
                    h.move_seat(me, spectate);
                }
            },
            |c| c.set_spectate(spectate),
        );
    }

    /// The player's look; its skin's hash comes from this peer's content
    /// (filled in now when known, else once hashed).
    pub fn set_look(&mut self, look: Look) {
        if self.peer.closed {
            return;
        }
        let look = self.peer.content.set_look(look);
        self.act(|h| h.set_my_look(look.clone()), |c| c.set_look(look.clone()));
    }

    /// The weapons for the next match (`None`: random).
    pub fn set_loadout(&mut self, loadout: Option<[i32; 5]>) {
        self.act(|h| h.set_my_loadout(loadout), |c| c.set_loadout(loadout));
    }

    pub fn chat(&mut self, text: &str) {
        self.act(|h| h.chat(text), |c| c.chat(text));
    }

    /// The next match's setup: the host's own, an admin client's request.
    pub fn set_setup(&mut self, setup: MatchSetupWire) -> Result<(), LobbyError> {
        let mut result = Ok(());
        self.act(|h| result = h.set_setup(setup.clone()), |c| c.admin(AdminCommand::Settings(setup.clone())));
        result
    }

    /// Kicks (and bans, by key: spec §5.2) a member: the host's own action,
    /// an admin client's request by the member's key.
    pub fn kick(&mut self, member: u8, ban: bool) {
        let key = self.state().and_then(|s| s.members.iter().find(|m| m.id == member)).map(|m| m.key);
        self.act(
            |h| h.kick(member, ban),
            |c| {
                if let Some(key) = key {
                    c.admin(if ban { AdminCommand::Ban(key) } else { AdminCommand::Kick(key) });
                }
            },
        );
    }

    /// Starts the match on `seed` when every player is ready (the host's).
    pub fn start(&mut self, seed: u32) -> Result<(), StartError> {
        self.peer.host_start(seed, false)
    }

    /// Starts the match on `seed`; players not ready watch it (spec §7.2).
    pub fn force_start(&mut self, seed: u32) -> Result<(), StartError> {
        self.peer.host_start(seed, true)
    }

    /// Ends the running match now (the host's; an admin's `/next` on a
    /// dedicated server): the results of the last confirmed tick go out as
    /// at a match's end, and the replay is made of the confirmed ticks.
    /// Whether a match was ended.
    pub fn end_match(&mut self) -> bool {
        self.peer.host_end_now()
    }

    /// A notice to one member only (the host's; a dedicated server's answer
    /// to a command). Nothing for an unknown member or on a client.
    pub fn tell(&mut self, member: u8, text: &str) {
        let Side::Host(h) = &self.peer.side else { return };
        if self.peer.closed {
            return;
        }
        let (Some(peer), Some(text)) = (h.lobby.peer_of(member), sanitize_chat(text)) else { return };
        let notice = ToClient::Notice(Notice::Message(text));
        send_tagged(&mut self.peer.endpoint, peer, TAG_TO_CLIENT, &encode_to_client(&notice));
    }

    /// The address a member's signalling connection came from (the host's).
    pub fn addr_of(&self, member: u8) -> Option<SocketAddr> {
        let Side::Host(h) = &self.peer.side else { return None };
        h.lobby.peer_of(member).and_then(|peer| h.addrs.get(&peer)).copied()
    }

    /// Bans a key for the lobby's life (the host's; a dedicated server's
    /// bans file): refused at its next join.
    pub fn ban_key(&mut self, key: PublicKey) {
        self.act(|h| h.ban_key(key), |_| {});
    }

    /// Bans an address for the lobby's life (the host's): the endpoint's
    /// gate refuses it.
    pub fn ban_ip(&mut self, ip: IpAddr) {
        if let Side::Host(h) = &mut self.peer.side {
            h.banned.lock().unwrap_or_else(|e| e.into_inner()).insert(ip);
        }
        self.act(|h| h.ban_ip(ip), |_| {});
    }

    /// Kicks a member and bans its key and its address (the host's own
    /// Ban button). A member reached through a TURN relay is banned by key
    /// only (N59), and `OnlineEvent::Notice` says so.
    pub fn ban_with_address(&mut self, member: u8) {
        self.ban_with_address_by(member, None);
    }

    /// As [`LobbyHandle::ban_with_address`], asked by the admin `by` (a
    /// server's `/banip`; `None`: the host's own or a server command): N59's
    /// notice goes to that admin.
    pub fn ban_with_address_by(&mut self, member: u8, by: Option<u8>) {
        // N59 decides by the path now, not as it was at `Connected` (B-M1).
        self.peer.sync_paths();
        self.act(|h| h.ban_with_address(member, by), |_| {});
    }

    /// Lifts a key's ban (the host's); whether it was banned.
    pub fn unban_key(&mut self, key: &PublicKey) -> bool {
        let mut lifted = false;
        self.act(|h| lifted = h.unban_key(key), |_| {});
        lifted
    }

    /// Lifts an address's ban, in the gate too (the host's); whether it
    /// was banned.
    pub fn unban_ip(&mut self, ip: IpAddr) -> bool {
        if let Side::Host(h) = &mut self.peer.side {
            h.banned.lock().unwrap_or_else(|e| e.into_inner()).remove(&ip);
        }
        let mut lifted = false;
        self.act(|h| lifted = h.unban_ip(ip), |_| {});
        lifted
    }

    /// The host's bans, keys and addresses, in order (a client's are empty).
    pub fn bans(&self) -> (Vec<PublicKey>, Vec<IpAddr>) {
        match &self.peer.side {
            Side::Host(h) => {
                (h.lobby.banned_keys().copied().collect(), h.lobby.banned_ips().copied().collect())
            }
            Side::Client(_) => (Vec::new(), Vec::new()),
        }
    }

    fn act(&mut self, host: impl FnOnce(&mut LobbyHost), client: impl FnOnce(&mut LobbyClient)) {
        if self.peer.closed {
            return;
        }
        match &mut self.peer.side {
            Side::Host(h) => host(&mut h.lobby),
            Side::Client(c) => {
                client(&mut c.lobby);
                self.peer.flush_client();
            }
        }
    }
}
