//! NAT-PMP (RFC 6886) and PCP (RFC 6887): the codec (sans-IO, fuzzed in
//! `tests/portmap.rs`) and the `rl-net pcp` thread, which tries PCP, then
//! NAT-PMP after any PCP failure, on the gateway's port 5351.

use std::io::ErrorKind;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::time::{Duration, Instant};

use super::{Ctx, Grant, MapError, Method, PMP_DEADLINE, PMP_LIFETIME, Router, try_router};

/// The NAT-PMP external address request (RFC 6886 §3.2): version 0, op 0.
pub fn pmp_external_request() -> [u8; 2] {
    [0, 0]
}

/// A NAT-PMP mapping request (RFC 6886 §3.3): version 0, op 1 (UDP) or 2
/// (TCP), reserved, internal port, suggested external port, lifetime in
/// seconds (0 deletes).
pub fn pmp_map_request(udp: bool, internal: u16, external: u16, lifetime: u32) -> [u8; 12] {
    let mut b = [0u8; 12];
    b[1] = if udp { 1 } else { 2 };
    b[4..6].copy_from_slice(&internal.to_be_bytes());
    b[6..8].copy_from_slice(&external.to_be_bytes());
    b[8..12].copy_from_slice(&lifetime.to_be_bytes());
    b
}

/// A NAT-PMP answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PmpResponse {
    External { result: u16, ip: Ipv4Addr },
    Map { udp: bool, result: u16, internal: u16, external: u16, lifetime: u32 },
}

fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_be_bytes([b[i], b[i + 1]])
}

fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

/// Reads a NAT-PMP answer (RFC 6886 §3.2, §3.3): version 0, op 128 + the
/// request's, a 16-bit result, seconds since the epoch, then the external
/// address (12 bytes in all) or the mapping (16 bytes). Anything else,
/// shorter ones included, is `None`.
pub fn parse_pmp(bytes: &[u8]) -> Option<PmpResponse> {
    match bytes {
        [0, 128, ..] if bytes.len() >= 12 => Some(PmpResponse::External {
            result: u16_at(bytes, 2),
            ip: Ipv4Addr::new(bytes[8], bytes[9], bytes[10], bytes[11]),
        }),
        [0, op @ (129 | 130), ..] if bytes.len() >= 16 => Some(PmpResponse::Map {
            udp: *op == 129,
            result: u16_at(bytes, 2),
            internal: u16_at(bytes, 8),
            external: u16_at(bytes, 10),
            lifetime: u32_at(bytes, 12),
        }),
        _ => None,
    }
}

/// PCP's version and MAP opcode (RFC 6887 §7, §11).
const PCP_VERSION: u8 = 2;
const PCP_MAP: u8 = 1;
/// The largest PCP message (RFC 6887 §7).
const PCP_MAX: usize = 1100;
/// The PCP result that names a busy external port (RFC 6887 §7.4).
const CANNOT_PROVIDE_EXTERNAL: u8 = 11;

/// `ip` as an IPv4-mapped IPv6 address (RFC 6887 §5).
fn mapped(ip: Ipv4Addr) -> [u8; 16] {
    let mut b = [0u8; 16];
    b[10] = 0xff;
    b[11] = 0xff;
    b[12..].copy_from_slice(&ip.octets());
    b
}

/// The IPv4 address in an IPv4-mapped IPv6 one.
fn unmapped(b: &[u8]) -> Option<Ipv4Addr> {
    (b.len() == 16 && b[..10].iter().all(|&x| x == 0) && b[10..12] == [0xff, 0xff])
        .then(|| Ipv4Addr::new(b[12], b[13], b[14], b[15]))
}

/// A PCP MAP request (RFC 6887 §7.1, §11.1): version 2, opcode MAP, the
/// lifetime (0 deletes), the client's address; then the nonce, the protocol
/// (6 TCP, 17 UDP), the internal port and the suggested external address
/// (0.0.0.0: no preference). 60 bytes.
pub fn pcp_map_request(
    nonce: [u8; 12],
    client: Ipv4Addr,
    protocol: u8,
    internal: u16,
    suggested: SocketAddrV4,
    lifetime: u32,
) -> Vec<u8> {
    let mut b = Vec::with_capacity(60);
    b.extend([PCP_VERSION, PCP_MAP, 0, 0]);
    b.extend(lifetime.to_be_bytes());
    b.extend(mapped(client));
    b.extend(nonce);
    b.extend([protocol, 0, 0, 0]);
    b.extend(internal.to_be_bytes());
    b.extend(suggested.port().to_be_bytes());
    b.extend(mapped(*suggested.ip()));
    b
}

/// A PCP MAP answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PcpResponse {
    pub version: u8,
    pub result: u8,
    pub lifetime: u32,
    pub protocol: u8,
    pub internal: u16,
    pub external: SocketAddrV4,
}

/// Reads a PCP MAP answer echoing `nonce` (RFC 6887 §7.2, §11.1): at least
/// 60 bytes, at most 1100, version 2 with the R bit and opcode MAP, the
/// assigned external address IPv4-mapped (0.0.0.0 in a refusal).
///
/// An answer of another version (a NAT-PMP-only gateway's version 0
/// `UNSUPP_VERSION`, RFC 6887 §9) carries no nonce: it is read as its
/// version and result (byte 3 in both protocols) alone, the rest zero.
pub fn parse_pcp(bytes: &[u8], nonce: &[u8; 12]) -> Option<PcpResponse> {
    let unspecified = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0);
    match bytes {
        [version, op, _, result, ..] if *version != PCP_VERSION && op & 0x80 != 0 => Some(PcpResponse {
            version: *version,
            result: *result,
            lifetime: 0,
            protocol: 0,
            internal: 0,
            external: unspecified,
        }),
        [PCP_VERSION, op, _, result, ..]
            if *op == 0x80 | PCP_MAP && (60..=PCP_MAX).contains(&bytes.len()) =>
        {
            if bytes[24..36] != nonce[..] {
                return None;
            }
            let ip = match unmapped(&bytes[44..60]) {
                Some(ip) => ip,
                None if *result != 0 => Ipv4Addr::UNSPECIFIED,
                None => return None,
            };
            Some(PcpResponse {
                version: PCP_VERSION,
                result: *result,
                lifetime: u32_at(bytes, 4),
                protocol: bytes[36],
                internal: u16_at(bytes, 40),
                external: SocketAddrV4::new(ip, u16_at(bytes, 42)),
            })
        }
        _ => None,
    }
}

/// A PCP result code's name (RFC 6887 §7.4).
fn pcp_result(code: u8) -> String {
    let name = match code {
        1 => "UNSUPP_VERSION",
        2 => "NOT_AUTHORIZED",
        3 => "MALFORMED_REQUEST",
        4 => "UNSUPP_OPCODE",
        5 => "UNSUPP_OPTION",
        6 => "MALFORMED_OPTION",
        7 => "NETWORK_FAILURE",
        8 => "NO_RESOURCES",
        9 => "UNSUPP_PROTOCOL",
        10 => "USER_EX_QUOTA",
        11 => "CANNOT_PROVIDE_EXTERNAL",
        12 => "ADDRESS_MISMATCH",
        13 => "EXCESSIVE_REMOTE_PEERS",
        _ => return format!("result {code}"),
    };
    name.into()
}

/// A NAT-PMP result code's meaning (RFC 6886 §3.5).
fn pmp_result(code: u16) -> String {
    let name = match code {
        1 => "unsupported version",
        2 => "not authorized",
        3 => "network failure",
        4 => "out of resources",
        5 => "unsupported opcode",
        _ => return format!("result {code}"),
    };
    name.into()
}

/// The waits after each send of a request (N51): 250, 500 and 1000 ms.
const RETRIES: [u64; 3] = [250, 500, 1000];
/// The waits for a deletion's answer: it is sent twice at most.
const DELETE_RETRIES: [u64; 2] = [200, 200];
/// The longest a receive blocks before the thread looks at the board.
const TICK: Duration = Duration::from_millis(50);

/// Sends `req` to the gateway (the socket is connected to it, so only its
/// datagrams arrive) and waits for an answer `accept` takes, resending after
/// each of `waits` (ms), until `deadline` or `going` says stop.
fn exchange<T>(
    sock: &UdpSocket,
    req: &[u8],
    waits: &[u64],
    deadline: Instant,
    going: &dyn Fn() -> bool,
    accept: impl Fn(&[u8]) -> Option<T>,
) -> Result<T, String> {
    let mut buf = [0u8; PCP_MAX + 1];
    for &wait in waits {
        if Instant::now() >= deadline {
            break;
        }
        if let Err(e) = sock.send(req) {
            return Err(io_why(&e));
        }
        let until = (Instant::now() + Duration::from_millis(wait)).min(deadline);
        loop {
            if !going() {
                return Err("stopped".into());
            }
            let now = Instant::now();
            if now >= until {
                break;
            }
            if sock.set_read_timeout(Some((until - now).min(TICK))).is_err() {
                return Err("the socket failed".into());
            }
            match sock.recv(&mut buf) {
                Ok(n) => {
                    if let Some(t) = accept(&buf[..n]) {
                        return Ok(t);
                    }
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
                    ) => {}
                Err(e) => return Err(io_why(&e)),
            }
        }
    }
    Err("no answer".into())
}

fn io_why(e: &std::io::Error) -> String {
    match e.kind() {
        // An ICMP port unreachable (Linux reports it on a connected socket,
        // Windows as a reset).
        ErrorKind::ConnectionRefused | ErrorKind::ConnectionReset => "the gateway's port is closed".into(),
        _ => format!("{e}"),
    }
}

/// A UDP socket connected to the gateway's PCP/NAT-PMP port.
fn open(gateway: Ipv4Addr, port: u16) -> std::io::Result<UdpSocket> {
    let sock = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
    sock.connect((gateway, port))?;
    Ok(sock)
}

/// The `rl-net pcp` thread: PCP, then NAT-PMP after any PCP failure.
pub(crate) fn run(ctx: Ctx) {
    let Some(gateway) = ctx.req.gateway else {
        ctx.fail("PCP: no default gateway; NAT-PMP: no default gateway".into());
        return;
    };
    let sock = match open(gateway, ctx.req.pmp_port) {
        Ok(sock) => sock,
        Err(e) => {
            ctx.fail(format!("PCP and NAT-PMP: no socket toward the gateway ({e})"));
            return;
        }
    };
    let client = match sock.local_addr() {
        Ok(SocketAddr::V4(a)) => *a.ip(),
        _ => *ctx.req.local.ip(),
    };
    let deadline = ctx.started + PMP_DEADLINE;
    let lifetime = ctx.req.lifetime_or(PMP_LIFETIME);
    let mut pcp = Pcp {
        sock: &sock,
        client,
        lifetime,
        nonces: [nonce(), nonce()],
        external: None,
        deadline,
        renewal: false,
    };
    let pcp_why = match try_router(&mut pcp, &ctx) {
        Ok(()) => return,
        Err(why) => why,
    };
    if !ctx.live() {
        ctx.fail(format!("PCP: {pcp_why}; NAT-PMP: not tried"));
        return;
    }
    let mut nat = NatPmp { sock: &sock, lifetime, external: None, deadline, renewal: false };
    let pmp_why = match nat.external_address(&ctx).and_then(|()| try_router(&mut nat, &ctx)) {
        Ok(()) => return,
        Err(why) => why,
    };
    ctx.fail(format!("PCP: {pcp_why}; NAT-PMP: {pmp_why}"));
}

/// A fresh PCP mapping nonce (RFC 6887 §11.1) from the system's randomness;
/// if that fails, from the clock (the nonce guards against stale answers,
/// not attackers on the LAN).
fn nonce() -> [u8; 12] {
    crate::random_bytes::<12>().unwrap_or_else(|_| {
        let t =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
        let mut b = [0u8; 12];
        b.copy_from_slice(&t.to_le_bytes()[..12]);
        b
    })
}

/// Whether a request goes on: a first mapping while the thread may still
/// win, a renewal until the mapper is dropped.
fn going(renewal: bool, ctx: &Ctx) -> impl Fn() -> bool + '_ {
    move || if renewal { !ctx.stopped() } else { ctx.live() }
}

struct Pcp<'a> {
    sock: &'a UdpSocket,
    client: Ipv4Addr,
    lifetime: u32,
    /// Per protocol (TCP, UDP): a mapping keeps its nonce for renewals and
    /// its deletion (one for every local port this client maps: PCP keys a
    /// mapping by its internal address, port and protocol).
    nonces: [[u8; 12]; 2],
    external: Option<Ipv4Addr>,
    deadline: Instant,
    renewal: bool,
}

impl Pcp<'_> {
    fn request(&self, udp: bool, internal: u16, external: u16, lifetime: u32) -> (Vec<u8>, [u8; 12], u8) {
        let nonce = self.nonces[usize::from(udp)];
        let protocol = if udp { 17 } else { 6 };
        let suggested = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, external);
        (pcp_map_request(nonce, self.client, protocol, internal, suggested, lifetime), nonce, protocol)
    }
}

impl Router for Pcp<'_> {
    fn method(&self) -> Method {
        Method::Pcp
    }

    fn map(&mut self, udp: bool, internal: u16, external: u16, ctx: &Ctx) -> Result<Grant, MapError> {
        let (req, nonce, protocol) = self.request(udp, internal, external, self.lifetime);
        let going = going(self.renewal, ctx);
        let r = exchange(self.sock, &req, &RETRIES, self.deadline, &going, |b| {
            parse_pcp(b, &nonce)
                .filter(|r| r.version != PCP_VERSION || (r.protocol == protocol && r.internal == internal))
        })
        .map_err(MapError::Silent)?;
        if r.version != PCP_VERSION {
            return Err(MapError::Fatal(match r.version {
                0 => "the gateway speaks NAT-PMP only".into(),
                v => format!("the gateway speaks version {v}"),
            }));
        }
        match r.result {
            0 => {}
            CANNOT_PROVIDE_EXTERNAL => {
                return Err(MapError::Conflict(format!("port {external}: {}", pcp_result(r.result))));
            }
            code => return Err(MapError::Fatal(format!("refused ({})", pcp_result(code)))),
        }
        if r.lifetime == 0 || r.external.port() == 0 || r.external.ip().is_unspecified() {
            return Err(MapError::Fatal("an empty grant".into()));
        }
        self.external.get_or_insert(*r.external.ip());
        Ok(Grant { port: r.external.port(), lifetime: r.lifetime })
    }

    fn unmap(&mut self, udp: bool, internal: u16, _external: u16) -> bool {
        let (req, nonce, protocol) = self.request(udp, internal, 0, 0);
        let end = Instant::now() + Duration::from_millis(DELETE_RETRIES.iter().sum());
        let answer = exchange(self.sock, &req, &DELETE_RETRIES, end, &|| true, |b| {
            parse_pcp(b, &nonce).filter(|r| r.protocol == protocol && r.internal == internal)
        });
        matches!(answer, Ok(r) if r.version == PCP_VERSION && r.result == 0)
    }

    fn external_ip(&self) -> Option<Ipv4Addr> {
        self.external
    }

    fn renewing(&mut self) {
        self.renewal = true;
        self.deadline = Instant::now() + PMP_DEADLINE;
    }
}

struct NatPmp<'a> {
    sock: &'a UdpSocket,
    lifetime: u32,
    external: Option<Ipv4Addr>,
    deadline: Instant,
    renewal: bool,
}

impl NatPmp<'_> {
    /// Asks the gateway's external address (RFC 6886 §3.2).
    fn external_address(&mut self, ctx: &Ctx) -> Result<(), String> {
        let going = || ctx.live();
        let r =
            exchange(
                self.sock,
                &pmp_external_request(),
                &RETRIES,
                self.deadline,
                &going,
                |b| match parse_pmp(b) {
                    Some(PmpResponse::External { result, ip }) => Some((result, ip)),
                    _ => None,
                },
            )?;
        match r {
            (0, ip) if !ip.is_unspecified() => {
                self.external = Some(ip);
                Ok(())
            }
            (0, _) => Err("the gateway has no external address".into()),
            (code, _) => Err(format!("refused ({})", pmp_result(code))),
        }
    }
}

impl Router for NatPmp<'_> {
    fn method(&self) -> Method {
        Method::NatPmp
    }

    fn map(&mut self, udp: bool, internal: u16, external: u16, ctx: &Ctx) -> Result<Grant, MapError> {
        let req = pmp_map_request(udp, internal, external, self.lifetime);
        let going = going(self.renewal, ctx);
        let r = exchange(self.sock, &req, &RETRIES, self.deadline, &going, |b| match parse_pmp(b) {
            Some(PmpResponse::Map { udp: u, result, internal: i, external, lifetime })
                if u == udp && i == internal =>
            {
                Some((result, external, lifetime))
            }
            _ => None,
        })
        .map_err(MapError::Silent)?;
        match r {
            (0, port, lifetime) if port != 0 && lifetime != 0 => Ok(Grant { port, lifetime }),
            (0, ..) => Err(MapError::Fatal("an empty grant".into())),
            (code, ..) => Err(MapError::Fatal(format!("refused ({})", pmp_result(code)))),
        }
    }

    fn unmap(&mut self, udp: bool, internal: u16, _external: u16) -> bool {
        let req = pmp_map_request(udp, internal, 0, 0);
        let end = Instant::now() + Duration::from_millis(DELETE_RETRIES.iter().sum());
        // Only this deletion's answer counts: a late one to another request
        // must not stop its resend.
        let answer = exchange(self.sock, &req, &DELETE_RETRIES, end, &|| true, |b| match parse_pmp(b) {
            Some(PmpResponse::Map { udp: u, internal: i, result, .. }) if u == udp && i == internal => {
                Some(result)
            }
            _ => None,
        });
        answer == Ok(0)
    }

    fn external_ip(&self) -> Option<Ipv4Addr> {
        self.external
    }

    fn renewing(&mut self) {
        self.renewal = true;
        self.deadline = Instant::now() + PMP_DEADLINE;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nat_pmp_requests_and_responses_have_rfc_6886_layouts() {
        assert_eq!(pmp_external_request(), [0, 0]);
        assert_eq!(
            pmp_map_request(true, 23400, 23400, 7200),
            [0, 1, 0, 0, 0x5b, 0x68, 0x5b, 0x68, 0, 0, 0x1c, 0x20]
        );
        let ext = [0, 128, 0, 0, 0, 0, 0, 9, 203, 0, 113, 7];
        assert_eq!(
            parse_pmp(&ext),
            Some(PmpResponse::External { result: 0, ip: "203.0.113.7".parse().unwrap() })
        );
        let map = [0, 129, 0, 0, 0, 0, 0, 9, 0x5b, 0x68, 0x5b, 0x69, 0, 0, 0x1c, 0x20];
        assert_eq!(
            parse_pmp(&map),
            Some(PmpResponse::Map { udp: true, result: 0, internal: 23400, external: 23401, lifetime: 7200 })
        );
        assert_eq!(parse_pmp(&map[..15]), None);
    }

    #[test]
    fn pcp_map_round_trips_its_nonce() {
        let nonce = [7u8; 12];
        let req = pcp_map_request(
            nonce,
            "192.168.1.10".parse().unwrap(),
            17,
            23400,
            "0.0.0.0:23400".parse().unwrap(),
            7200,
        );
        assert_eq!(req.len(), 60);
        assert_eq!((req[0], req[1] & 0x7f), (2, 1));
        // A response: version 2, R|opcode, reserved, result 0, lifetime, epoch, reserved(12), then the MAP payload.
        let mut resp = vec![2, 0x81, 0, 0, 0, 0, 0x1c, 0x20, 0, 0, 0, 9];
        resp.extend([0u8; 12]);
        resp.extend(nonce);
        resp.extend([17, 0, 0, 0, 0x5b, 0x68, 0x5b, 0x68]);
        resp.extend([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff, 203, 0, 113, 7]);
        assert_eq!(
            parse_pcp(&resp, &nonce),
            Some(PcpResponse {
                version: 2,
                result: 0,
                lifetime: 7200,
                protocol: 17,
                internal: 23400,
                external: "203.0.113.7:23400".parse().unwrap()
            })
        );
        assert_eq!(parse_pcp(&resp, &[8u8; 12]), None, "another nonce");
    }

    #[test]
    fn a_nat_pmp_gateway_answers_pcp_with_its_version() {
        // RFC 6887 §9: version 0, op 128 + 1, result 1 (UNSUPP_VERSION), the epoch.
        let answer = [0, 129, 0, 1, 0, 0, 0, 9];
        let r = parse_pcp(&answer, &[7u8; 12]).unwrap();
        assert_eq!((r.version, r.result), (0, 1));
        assert_eq!(parse_pcp(&[0, 1, 0, 1], &[7u8; 12]), None, "a request, not an answer");
    }

    #[test]
    fn requests_carry_their_fields() {
        let req = pcp_map_request(
            [1; 12],
            Ipv4Addr::new(192, 168, 1, 10),
            6,
            23400,
            "198.51.100.1:23401".parse().unwrap(),
            60,
        );
        assert_eq!(&req[4..8], &60u32.to_be_bytes());
        assert_eq!(unmapped(&req[8..24]), Some(Ipv4Addr::new(192, 168, 1, 10)));
        assert_eq!(&req[24..36], &[1; 12]);
        assert_eq!((req[36], u16_at(&req, 40), u16_at(&req, 42)), (6, 23400, 23401));
        assert_eq!(unmapped(&req[44..60]), Some(Ipv4Addr::new(198, 51, 100, 1)));
        assert_eq!(pmp_map_request(false, 1, 2, 0)[1], 2);
    }
}
