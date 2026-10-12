//! The lobby's client (networking spec §5.2, §7.2): answers the host's
//! challenge with its key, look and password proof, follows the lobby's
//! state and sends the player's actions. Sans-IO.

use alloc::string::String;
use alloc::vec::Vec;

use rl_proto::{
    AdminCommand, LobbyState, Look, MAX_MISSING, PublicKey, ToClient, ToHost, valid_pack_id, valid_setup,
};

use crate::identity::{Identity, challenge_message, password_proof};
use crate::model::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    /// No transport yet.
    Idle,
    /// The transport is open: waiting for the challenge.
    Connected,
    /// Hello and Prove sent: waiting for the welcome.
    Proving,
    Joined,
    /// Refused, kicked, left or disconnected.
    Closed,
}

/// The lobby's client.
pub struct LobbyClient {
    me: Identity,
    look: Look,
    password: Option<String>,
    expected_host: Option<PublicKey>,
    stage: Stage,
    host_key: Option<PublicKey>,
    you: Option<u8>,
    state: Option<LobbyState>,
    out: Vec<ToHost>,
    events: Vec<ClientEvent>,
}

impl LobbyClient {
    /// A client joining as `me` with `look` (sanitized), the lobby's
    /// password if it has one, and the host's key when known (an invite
    /// carries it; a direct connection passes `None`): a challenge naming
    /// another key is not answered.
    pub fn new(
        me: Identity,
        look: Look,
        password: Option<String>,
        expected_host: Option<PublicKey>,
    ) -> LobbyClient {
        LobbyClient {
            me,
            look: sanitize_look(&look),
            password,
            expected_host,
            stage: Stage::Idle,
            host_key: None,
            you: None,
            state: None,
            out: Vec::new(),
            events: Vec::new(),
        }
    }

    /// The transport opened: wait for the host's challenge.
    pub fn connected(&mut self) {
        if self.stage == Stage::Idle {
            self.stage = Stage::Connected;
        }
    }

    /// The transport closed.
    pub fn disconnected(&mut self) {
        self.close();
    }

    /// A message from the host. `aux` is fresh randomness for signing the
    /// challenge. The challenge is answered with `Hello` (with the password
    /// proof when the host asks for one; an empty password without one) and
    /// `Prove` at once.
    pub fn receive(&mut self, msg: ToClient, aux: [u8; 32]) {
        match (self.stage, msg) {
            (Stage::Connected, ToClient::Challenge { host_key, .. })
                if self.expected_host.is_some_and(|k| k != host_key) =>
            {
                self.stage = Stage::Closed;
                self.events.push(ClientEvent::WrongHost { key: host_key });
            }
            (Stage::Connected, ToClient::Challenge { nonce, host_key, needs_password }) => {
                let password = needs_password
                    .then(|| password_proof(&nonce, self.password.as_deref().unwrap_or_default()));
                self.out.push(ToHost::Hello { key: self.me.public(), look: self.look.clone(), password });
                let signature = self.me.sign(&challenge_message(&nonce, &host_key), aux);
                self.out.push(ToHost::Prove { signature });
                self.host_key = Some(host_key);
                self.stage = Stage::Proving;
            }
            (Stage::Connected | Stage::Proving, ToClient::Refused(why)) => {
                self.events.push(ClientEvent::Refused(why));
                self.stage = Stage::Closed;
            }
            (Stage::Proving, ToClient::Welcome { you, lobby })
                if !lobby.members.iter().any(|m| m.id == you) =>
            {
                self.close();
            }
            (Stage::Proving, ToClient::Welcome { you, lobby }) => {
                self.you = Some(you);
                self.state = Some(lobby);
                self.stage = Stage::Joined;
                self.events.push(ClientEvent::Welcome { you });
            }
            (Stage::Joined, msg) => self.receive_joined(msg),
            // Out of order or after the end: a host bug, nothing to act on.
            _ => {}
        }
    }

    fn receive_joined(&mut self, msg: ToClient) {
        let event = match msg {
            ToClient::Lobby(state) if !state.members.iter().any(|m| Some(m.id) == self.you) => {
                return self.close();
            }
            ToClient::Lobby(state) => {
                self.state = Some(state.clone());
                ClientEvent::Lobby(state)
            }
            ToClient::Chat { from, text } => ClientEvent::Chat { from, text },
            ToClient::Notice(n) => ClientEvent::Notice(n),
            ToClient::Start(s) => ClientEvent::Start(s),
            ToClient::Ended(e) => ClientEvent::Ended(e),
            ToClient::Kicked { banned } => {
                self.stage = Stage::Closed;
                ClientEvent::Kicked { banned }
            }
            ToClient::Bye => return self.close(),
            ToClient::Challenge { .. } | ToClient::Welcome { .. } | ToClient::Refused(_) => return,
        };
        self.events.push(event);
    }

    fn close(&mut self) {
        if self.stage != Stage::Closed {
            self.stage = Stage::Closed;
            self.events.push(ClientEvent::Closed);
        }
    }

    /// Drains the messages to send.
    pub fn outgoing(&mut self) -> Vec<ToHost> {
        core::mem::take(&mut self.out)
    }

    /// Drains the events.
    pub fn events(&mut self) -> Vec<ClientEvent> {
        core::mem::take(&mut self.events)
    }

    // ---- the player's actions (sent once joined) ----

    fn send(&mut self, msg: ToHost) {
        if self.stage == Stage::Joined {
            self.out.push(msg);
        }
    }

    /// The look (sanitized); used for joining too.
    pub fn set_look(&mut self, look: Look) {
        self.look = sanitize_look(&look);
        self.send(ToHost::SetLook(self.look.clone()));
    }

    pub fn set_ready(&mut self, ready: bool) {
        self.send(ToHost::Ready(ready));
    }

    pub fn set_spectate(&mut self, spectate: bool) {
        self.send(ToHost::Spectate(spectate));
    }

    pub fn set_loadout(&mut self, loadout: Option<[i32; 5]>) {
        self.send(ToHost::Loadout(loadout));
    }

    /// A chat line, cleaned and cut to 200 characters (nothing for a blank one).
    pub fn chat(&mut self, text: &str) {
        if let Some(text) = sanitize_chat(text) {
            self.send(ToHost::Chat(text));
        }
    }

    /// The setup's pack ids this client lacks (invalid ids dropped, at most
    /// `MAX_MISSING`).
    pub fn have(&mut self, missing: Vec<String>) {
        let missing = missing.into_iter().filter(|m| valid_pack_id(m)).take(MAX_MISSING).collect();
        self.send(ToHost::Have { missing });
    }

    /// An admin command (the host refuses it from a non-admin key); a
    /// setup the wire cannot carry is not sent.
    pub fn admin(&mut self, cmd: AdminCommand) {
        if let AdminCommand::Settings(setup) = &cmd
            && !valid_setup(setup)
        {
            return;
        }
        self.send(ToHost::Admin(cmd));
    }

    /// Says goodbye; the caller closes the transport after sending.
    pub fn leave(&mut self) {
        if matches!(self.stage, Stage::Connected | Stage::Proving | Stage::Joined) {
            self.out.push(ToHost::Bye);
        }
        self.stage = Stage::Closed;
    }

    // ---- queries ----

    /// The lobby, once welcomed.
    pub fn state(&self) -> Option<&LobbyState> {
        self.state.as_ref()
    }

    /// Whether this client is a member now (welcomed, not gone since).
    pub fn joined(&self) -> bool {
        self.stage == Stage::Joined
    }

    /// This client's member id, once welcomed (kept after leaving).
    pub fn me(&self) -> Option<u8> {
        self.you
    }

    /// The key the host's challenge named.
    pub fn host_key(&self) -> Option<PublicKey> {
        self.host_key
    }
}
