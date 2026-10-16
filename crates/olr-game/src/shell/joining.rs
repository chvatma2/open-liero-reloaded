//! Joining and letting go (plan 12b, N46, ruling NT-2; split from
//! `online.rs`): how a join reaches its host (an address, an invite, a
//! listed game, through the relays), a typed host name looked up off the
//! UI thread, and the router mappings of the peers let go, deleted on
//! threads and waited for at exit.

use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use olr_discovery::{Invite, PoolConfig};
use olr_net::endpoint::{EndpointConfig, IceConfig};
use olr_online::join::join_relays;
use olr_online::{Join, MigrationConfig, NostrJoin, OnlinePeer, PeerRole, Shutdown};
use olr_ui::{FindingSettings, Password, is_invite};

use super::{Outcome, Shell};
use crate::finding::{local_relays, region, relays_of, stun_of, turn_of};
use crate::net::Net;

impl Shell {
    /// The peers let go whose router mapping is still being deleted.
    pub fn unmapping_pending(&self) -> usize {
        self.unmapping.len()
    }

    /// The router mappings of the peers let go so far, being deleted
    /// (ruling NT-2): the exit waits for them once the window closed
    /// ([`Unmapping::wait`], at most [`UNMAP_WAIT`]).
    pub fn take_unmapping(&mut self) -> Unmapping {
        Unmapping(std::mem::take(&mut self.unmapping))
    }

    /// Joining from the Multiplayer screen (N46): `addr` is `host[:port]`
    /// as typed (the port defaults to the options' or `--port`; a name is
    /// looked up on a thread, [`Shell::poll_resolving`]), or an invite code
    /// (`rlx1…`: its address first, then through its relays and this
    /// player's; its key expected, N42); none: through the relays alone to
    /// `expected_host` (a listed game without an address). With
    /// `expected_host` (a listed game's), a host of another key is refused
    /// (N37), and an address that never answers falls back to the relays.
    /// A typed address or a LAN game joins directly only.
    pub(super) fn start_client(
        &mut self,
        addr: Option<String>,
        password: Option<Password>,
        expected_host: Option<[u8; 32]>,
    ) -> anyhow::Result<()> {
        self.leave_net();
        self.data.joining = None;
        let finding = self.ui.settings().online.finding.clone();
        let own = relays_of(&finding);
        let join = match addr {
            Some(text) if is_invite(&text) => {
                let invite = decode_invite(&text)?;
                let relays = join_relays(&invite.relay_urls(), &own);
                let join = Join { direct: invite.addr, nostr: nostr_join(&finding, invite.key, relays) };
                return self.start_join(join, password, Some(invite.key));
            }
            Some(text) => {
                let default_port = self.port.filter(|&p| p != 0).unwrap_or(self.ui.settings().online.port);
                let Some(to) = literal(&text, default_port) else {
                    // A name: looked up off the UI thread (it may take seconds).
                    self.data.joining = Some(format!("Looking up {}…", text.trim()));
                    self.resolving = Some(Resolving::start(text, default_port, password, expected_host));
                    return Ok(());
                };
                listed_join(&finding, to?, expected_host)
            }
            None => {
                let key =
                    expected_host.ok_or_else(|| anyhow::anyhow!("no address and no host key to join"))?;
                Join { direct: None, nostr: nostr_join(&finding, key, join_relays(&[], &own)) }
            }
        };
        self.start_join(join, password, expected_host)
    }

    /// Starts the peer joining as `join` says (the ICE settings for a join
    /// through Nostr: STUN from the game's socket, this player's TURN
    /// servers, N48, N52).
    fn start_join(
        &mut self,
        join: Join,
        password: Option<Password>,
        expected_host: Option<[u8; 32]>,
    ) -> anyhow::Result<()> {
        if join.direct.is_none() && join.nostr.is_none() {
            anyhow::bail!("{NO_WAY_TO_JOIN}");
        }
        let (id, look) = self.me()?;
        let content = self.open_content()?;
        let finding = &self.ui.settings().online.finding;
        let ice = match &join.nostr {
            Some(_) => IceConfig { stun: stun_of(finding), turn: turn_of(finding), ..IceConfig::default() },
            None => IceConfig::default(),
        };
        let what = match (join.direct, &join.nostr) {
            (Some(to), None) => format!("{to}"),
            (Some(to), Some(_)) => format!("{to}, then through the relays"),
            (None, _) => "through the relays".to_string(),
        };
        let migration = self.migration_config();
        let role = PeerRole::Client { join, password: password.map(|p| p.0), expected_host, migration };
        let endpoint =
            EndpointConfig { port: 0, impair: self.impair, ice: self.ice(ice), ..EndpointConfig::default() };
        let peer = OnlinePeer::start(role, endpoint, id, look, content, None, self.peer_content())?;
        println!("olr-game: joining {what}");
        self.net = Some(Net::new(peer, false));
        Ok(())
    }

    /// How a client this shell starts takes part in host migration (plan
    /// 12c, N81): a standby on a free port (no port mapping, the settings'
    /// STUN, local addresses when every relay is local; none with
    /// `test_no_standby`, N79), the settings' relays to listen on once it
    /// hosts and to join a successor through (none: direct joins only),
    /// those joins asking the settings' STUN and the player's TURN servers
    /// in the settings' region; every ICE setting through [`Shell::ice`]
    /// (tests: 127.0.0.1 alone, C4). Off (`test_migration(false)`, D7):
    /// `MigrationConfig::off()`.
    fn migration_config(&self) -> MigrationConfig {
        if !self.migration {
            return MigrationConfig::off();
        }
        let finding = &self.ui.settings().online.finding;
        let relays = relays_of(finding);
        let allow_local = local_relays(&relays);
        let standby_ice =
            IceConfig { stun: stun_of(finding), port_mapping: false, allow_local, ..IceConfig::default() };
        let standby = (!self.no_standby).then(|| EndpointConfig {
            port: 0,
            impair: self.impair,
            ice: self.ice(standby_ice),
            ..EndpointConfig::default()
        });
        MigrationConfig {
            standby,
            relays: (!relays.is_empty()).then(|| PoolConfig { relays, ..PoolConfig::default() }),
            ice: self.ice(IceConfig {
                stun: stun_of(finding),
                turn: turn_of(finding),
                ..IceConfig::default()
            }),
            region: region(finding.region),
            turn_from_nostr: true,
            allow_local,
            enabled: true,
        }
    }

    /// The typed host name looked up (a frame of `net_frame`): its join
    /// starts, or the menus hear why not.
    pub(super) fn poll_resolving(&mut self, out: &mut Vec<Outcome>) {
        let Some(r) = &self.resolving else { return };
        let found = match r.rx.try_recv() {
            Ok(found) => found,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err(anyhow::anyhow!("{}: the lookup failed", r.typed)),
        };
        let Some(r) = self.resolving.take() else { return };
        let finding = self.ui.settings().online.finding.clone();
        let started = found.and_then(|to| {
            self.start_join(listed_join(&finding, to, r.expected_host), r.password, r.expected_host)
        });
        self.connected(started.map(|()| false), out);
    }

    /// Forgets the router mappings whose deletion is over (review Minor
    /// 3: the list stays short), warning once for one the router did not
    /// confirm. Never waits.
    pub(super) fn prune_unmapping(&mut self) {
        let mut unconfirmed = false;
        self.unmapping.retain_mut(|s| match s.poll() {
            None => true,
            Some(confirmed) => {
                unconfirmed |= !confirmed;
                false
            }
        });
        if unconfirmed {
            eprintln!("warning: the router did not confirm deleting a port mapping");
        }
    }
}

/// The invite code `text`, which the menus took for one (`is_invite`): a
/// code with a typo in its `rlx1` prefix is damaged, not "not an invite
/// code".
fn decode_invite(text: &str) -> Result<Invite, olr_discovery::InviteError> {
    Invite::decode(text).map_err(|e| match e {
        olr_discovery::InviteError::NotInvite => olr_discovery::InviteError::Damaged,
        e => e,
    })
}

/// Why a game is not joined: no address, and no relays to join it through
/// (an invite without either, or a listed game while the settings name
/// no relay).
pub const NO_WAY_TO_JOIN: &str =
    "this game has no address, and there are no relays to join it through (set relays in Options → Online)";

/// The exit waits at most this long for the router to delete the port
/// mappings (N51, ruling NT-2), once the window closed.
pub const UNMAP_WAIT: Duration = Duration::from_secs(1);

/// The router mappings of the peers a shell let go, being deleted on
/// threads (ruling NT-2).
#[must_use = "wait on it before the process exits, or the router may keep the mapping"]
pub struct Unmapping(Vec<Shutdown>);

impl Unmapping {
    /// Waits until every mapping is deleted, at most `limit` in all: `true`
    /// when the router confirmed each (or there was none).
    pub fn wait(self, limit: Duration) -> bool {
        let until = Instant::now() + limit;
        // Each waited for, also after one the router did not confirm.
        let mut all = true;
        for s in self.0 {
            all &= s.wait(until);
        }
        all
    }
}

/// A typed host name looked up on a thread (`olr-game resolve`; a lookup
/// may take seconds), and the join waiting for it.
pub(super) struct Resolving {
    rx: Receiver<anyhow::Result<SocketAddr>>,
    typed: String,
    password: Option<Password>,
    expected_host: Option<[u8; 32]>,
}

impl Resolving {
    fn start(
        typed: String,
        default_port: u16,
        password: Option<Password>,
        expected_host: Option<[u8; 32]>,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let name = typed.clone();
        let spawned = std::thread::Builder::new().name("olr-game resolve".into()).spawn(move || {
            // A send fails only once the join was given up.
            let _ = tx.send(resolve(&name, default_port));
        });
        if let Err(e) = spawned {
            let (tx, failed) = mpsc::channel();
            let _ = tx.send(Err(anyhow::anyhow!("{typed}: cannot look it up: {e}")));
            return Resolving { rx: failed, typed, password, expected_host };
        }
        Resolving { rx, typed, password, expected_host }
    }
}

/// A join through Nostr to `host` on `relays` (N46: an invite's hints
/// first, then the player's, at most 8: `join_relays`), the player's
/// region for announced TURN relays, local addresses allowed when every
/// relay is local (PF12-8); `None` without relays.
fn nostr_join(finding: &FindingSettings, host: [u8; 32], relays: Vec<String>) -> Option<NostrJoin> {
    (!relays.is_empty()).then(|| NostrJoin {
        host,
        allow_local: local_relays(&relays),
        pool: PoolConfig { relays, ..PoolConfig::default() },
        region: region(finding.region),
        turn_from_nostr: true,
    })
}

/// The join of an address (N46): a listed game's (`expected_host`) directly
/// first, then through the player's relays; a typed address or a LAN
/// game's directly only.
fn listed_join(finding: &FindingSettings, to: SocketAddr, expected_host: Option<[u8; 32]>) -> Join {
    let nostr = expected_host.and_then(|key| nostr_join(finding, key, join_relays(&[], &relays_of(finding))));
    Join { direct: Some(to), nostr }
}

/// Why an IPv6 address is not joined: the host listens on IPv4 only and
/// the client's UDP socket is IPv4.
pub const IPV6_UNSUPPORTED: &str = "IPv6 is not supported yet";

/// The address `typed` (`host[:port]`, `default_port` when it has none); a
/// name is looked up (blocking: never on the UI thread, [`Resolving`]) and
/// its first IPv4 address taken. An IPv6 literal (`::1`, `[::1]:23400`) or
/// a name with IPv6 addresses only is refused ([`IPV6_UNSUPPORTED`]).
pub fn resolve(typed: &str, default_port: u16) -> anyhow::Result<SocketAddr> {
    if let Some(a) = literal(typed, default_port) {
        return a;
    }
    let typed = typed.trim();
    let with_port = if typed.contains(':') { typed.to_string() } else { format!("{typed}:{default_port}") };
    let addrs = with_port.to_socket_addrs().map_err(|e| anyhow::anyhow!("{typed}: {e}"))?;
    choose_address(typed, addrs)
}

/// The address `typed` names without a lookup (an IP address, with or
/// without a port: `default_port`), as [`resolve`] judges it; `None`: a
/// name to look up.
pub fn literal(typed: &str, default_port: u16) -> Option<anyhow::Result<SocketAddr>> {
    let typed = typed.trim();
    let literal = typed.parse::<SocketAddr>().ok().or_else(|| {
        typed
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<IpAddr>()
            .ok()
            .map(|ip| SocketAddr::new(ip, default_port))
    });
    literal.map(|a| choose_address(typed, [a]))
}

/// The address to join among those `typed` resolved to: the first IPv4
/// one; IPv6 alone is refused ([`IPV6_UNSUPPORTED`]).
pub fn choose_address(
    typed: &str,
    addrs: impl IntoIterator<Item = SocketAddr>,
) -> anyhow::Result<SocketAddr> {
    let mut ipv6 = false;
    for a in addrs {
        match a {
            SocketAddr::V4(_) => return Ok(a),
            SocketAddr::V6(v6) => match v6.ip().to_ipv4_mapped() {
                Some(v4) => return Ok(SocketAddr::new(v4.into(), a.port())),
                None => ipv6 = true,
            },
        }
    }
    if ipv6 {
        anyhow::bail!("{typed}: {IPV6_UNSUPPORTED}")
    }
    anyhow::bail!("{typed}: no address")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addrs(list: &[&str]) -> Vec<SocketAddr> {
        list.iter().map(|a| a.parse().unwrap()).collect()
    }

    /// Text the menus take for an invite code (olr-ui's `is_invite`: `rlx1`
    /// or a long run of letters and digits) without the `rlx1` prefix has
    /// a typo there: "damaged", never "not an invite code".
    #[test]
    fn a_code_with_a_typo_in_its_prefix_is_damaged() {
        let code = olr_discovery::invite([1; 32], &[], None, 23400).encode();
        let typo = code.replacen("rlx1", "rkx1", 1);
        assert!(olr_ui::is_invite(&typo));
        assert_eq!(decode_invite(&typo).unwrap_err().to_string(), "this invite code is damaged");
        assert_eq!(decode_invite(&code).unwrap().key, [1; 32]);
    }

    #[test]
    fn a_name_joins_its_ipv4_address_first() {
        let got =
            choose_address("host", addrs(&["[2001:db8::1]:23400", "192.0.2.7:23400", "192.0.2.8:23400"]));
        assert_eq!(got.unwrap(), "192.0.2.7:23400".parse().unwrap());
        let mapped = choose_address("host", addrs(&["[::ffff:192.0.2.9]:5"]));
        assert_eq!(mapped.unwrap(), "192.0.2.9:5".parse().unwrap(), "an IPv4-mapped address is IPv4");
    }

    #[test]
    fn an_ipv6_only_name_is_refused_clearly() {
        let err = choose_address("host", addrs(&["[2001:db8::1]:23400"])).unwrap_err();
        assert_eq!(err.to_string(), "host: IPv6 is not supported yet");
        let err = choose_address("host", addrs(&[])).unwrap_err();
        assert_eq!(err.to_string(), "host: no address");
    }

    #[test]
    fn an_ipv6_literal_is_refused_clearly() {
        for typed in ["::1", "[::1]", "[2001:db8::1]:23400"] {
            let err = resolve(typed, 23400).unwrap_err();
            assert_eq!(err.to_string(), format!("{typed}: IPv6 is not supported yet"));
        }
        assert_eq!(resolve("127.0.0.1", 23400).unwrap(), "127.0.0.1:23400".parse().unwrap());
        assert_eq!(resolve(" 127.0.0.1:99 ", 23400).unwrap(), "127.0.0.1:99".parse().unwrap());
        assert_eq!(resolve("localhost:5", 23400).unwrap(), "127.0.0.1:5".parse().unwrap());
    }
}
