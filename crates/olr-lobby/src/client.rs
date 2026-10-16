//! The lobby's client (networking spec §5.2, §7.2): answers the host's
//! challenge with its key and look, then, once the host proved its key,
//! with its own proof and the password's; follows the lobby's state and
//! sends the player's actions. Sans-IO.

use alloc::string::String;
use alloc::vec::Vec;

use olr_proto::{
    AdminCommand, LobbyState, Look, MAX_MISSING, MAX_STANDBY_RELAYS, MIN_STANDBY_PORT, PublicKey, Refusal,
    SuccessorWire, ToClient, ToHost, valid_pack_id, valid_relay_url, valid_setup, valid_succession,
};

use crate::identity::{
    DtlsFingerprints, Identity, challenge_message, host_proof_message, password_proof, verify,
};
use crate::model::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    /// No transport yet.
    Idle,
    /// The transport is open: waiting for the challenge.
    Connected,
    /// Hello sent: waiting for the host's proof of its key.
    Hello,
    /// Prove sent: waiting for the welcome.
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
    /// The link's DTLS fingerprints as this end saw them (`connected`).
    fingerprints: DtlsFingerprints,
    stage: Stage,
    host_key: Option<PublicKey>,
    /// The challenge's nonce and the one this client sent in `Hello`,
    /// until the host's proof arrives.
    nonces: Option<([u8; 32], [u8; 32])>,
    /// The challenge asked for the password (proven in `Prove`).
    needs_password: bool,
    you: Option<u8>,
    state: Option<LobbyState>,
    /// The standby endpoint to report (`standby`).
    standby: Option<(u16, Vec<String>)>,
    /// The host's latest succession order.
    succession: Vec<SuccessorWire>,
    out: Vec<ToHost>,
    events: Vec<ClientEvent>,
}

impl LobbyClient {
    /// A client joining as `me` with `look` (sanitized), the lobby's
    /// password if it has one, and the host's key when known (an invite
    /// carries it; a direct connection passes `None`): a challenge naming
    /// another key is not answered.
    ///
    /// Whatever `expected_host`, the host must prove the key its challenge
    /// names (`HostProof`, N37) over this client's nonce and both DTLS
    /// fingerprints before this client proves its own. Without
    /// `expected_host` that catches a man in the middle (its certificates
    /// change the fingerprints), not an impostor answering with its own
    /// key: `expected_host` is for that.
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
            fingerprints: ([0; 32], [0; 32]),
            stage: Stage::Idle,
            host_key: None,
            nonces: None,
            needs_password: false,
            you: None,
            state: None,
            standby: None,
            succession: Vec::new(),
            out: Vec::new(),
            events: Vec::new(),
        }
    }

    /// The transport opened with the DTLS `fingerprints` this end saw
    /// (offer side's, answer side's): wait for the host's challenge.
    pub fn connected(&mut self, fingerprints: DtlsFingerprints) {
        if self.stage == Stage::Idle {
            self.fingerprints = fingerprints;
            self.stage = Stage::Connected;
        }
    }

    /// The transport closed.
    pub fn disconnected(&mut self) {
        self.close();
    }

    /// A message from the host. `aux` is fresh randomness: the challenge's
    /// is this client's nonce in `Hello`, the host proof's signs the
    /// challenge. The challenge is answered with `Hello`; the host's
    /// `HostProof` with `Prove` when it verifies (with the password proof
    /// when the challenge asked for one; an empty password without one),
    /// else `WrongHost` and nothing more. A password refusal before the
    /// host's proof closes, unbelieved.
    pub fn receive(&mut self, msg: ToClient, aux: [u8; 32]) {
        match (self.stage, msg) {
            (Stage::Connected, ToClient::Challenge { host_key, .. })
                if self.expected_host.is_some_and(|k| k != host_key) =>
            {
                self.stage = Stage::Closed;
                self.events.push(ClientEvent::WrongHost { key: host_key });
            }
            (Stage::Connected, ToClient::Challenge { nonce, host_key, needs_password }) => {
                let hello = ToHost::Hello { key: self.me.public(), look: self.look.clone(), nonce: aux };
                self.out.push(hello);
                self.host_key = Some(host_key);
                self.nonces = Some((nonce, aux));
                self.needs_password = needs_password;
                self.stage = Stage::Hello;
            }
            (Stage::Hello, ToClient::HostProof { signature }) => {
                let (Some(host_key), Some((nonce, mine))) = (self.host_key, self.nonces.take()) else {
                    return self.close();
                };
                let message = host_proof_message(&mine, &self.me.public(), &self.fingerprints);
                if !verify(&host_key, &message, &signature) {
                    self.stage = Stage::Closed;
                    self.events.push(ClientEvent::WrongHost { key: host_key });
                    return;
                }
                let signature = self.me.sign(&challenge_message(&nonce, &host_key, &self.fingerprints), aux);
                // Only now, to a host that proved its key: the password's
                // proof, bound to this key and this link.
                let password = self.needs_password.then(|| {
                    password_proof(
                        &nonce,
                        &self.me.public(),
                        &self.fingerprints,
                        self.password.as_deref().unwrap_or_default(),
                    )
                });
                self.out.push(ToHost::Prove { signature, password });
                self.stage = Stage::Proving;
            }
            // A password refusal from a host that has not proven its key is
            // not believed: it would prompt the player to type the password
            // for an impostor.
            (Stage::Connected | Stage::Hello, ToClient::Refused(Refusal::Password)) => self.close(),
            (Stage::Connected | Stage::Hello | Stage::Proving, ToClient::Refused(why)) => {
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
                // Again after every welcome: a new host knows none.
                if let Some((port, relays)) = self.standby.clone() {
                    self.out.push(ToHost::Standby { port, relays });
                }
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
            ToClient::Challenge { .. }
            | ToClient::HostProof { .. }
            | ToClient::Welcome { .. }
            | ToClient::Refused(_) => return,
            ToClient::Succession(list) => {
                if valid_succession(&list) {
                    self.succession = list;
                }
                return;
            }
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

    /// The pack ids this client lacks of the host's content list
    /// `revision` (invalid ids dropped, at most `MAX_MISSING`).
    pub fn have(&mut self, revision: u32, missing: Vec<String>) {
        let missing = missing.into_iter().filter(|m| valid_pack_id(m)).take(MAX_MISSING).collect();
        self.send(ToHost::Have { revision, missing });
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

    /// Reports this member's standby endpoint (N63): the `port` it would
    /// listen on and the `relays` it would listen on as host. Sent now if
    /// joined, else after the welcome, and again after every welcome. A port
    /// below `MIN_STANDBY_PORT` is not reported; invalid relays are dropped
    /// and at most `MAX_STANDBY_RELAYS` kept.
    pub fn standby(&mut self, port: u16, relays: Vec<String>) {
        if port < MIN_STANDBY_PORT {
            return;
        }
        let relays: Vec<String> =
            relays.into_iter().filter(|r| valid_relay_url(r)).take(MAX_STANDBY_RELAYS).collect();
        self.standby = Some((port, relays.clone()));
        self.send(ToHost::Standby { port, relays });
    }

    /// Says goodbye; the caller closes the transport after sending.
    pub fn leave(&mut self) {
        if matches!(self.stage, Stage::Connected | Stage::Hello | Stage::Proving | Stage::Joined) {
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

    /// The host's latest succession order (empty before the first).
    pub fn succession(&self) -> &[SuccessorWire] {
        &self.succession
    }

    /// The key the host's challenge named (proven once joined).
    pub fn host_key(&self) -> Option<PublicKey> {
        self.host_key
    }
}
