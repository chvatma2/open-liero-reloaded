//! A router on 127.0.0.1 for the port-mapping tests (feature `test-hooks`):
//! an SSDP responder, a tiny HTTP server for UPnP-IGD (a `WANIPConnection:1`
//! description and SCPD; SOAP `GetExternalIPAddress`, `AddPortMapping` and
//! `DeletePortMapping`) and one UDP socket answering PCP and NAT-PMP, all on
//! threads. `Misbehaviour` makes it lie, stall or forget (review focus 2).

use std::collections::{BTreeMap, BTreeSet};
use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::Method;

/// How the fake router misbehaves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Misbehaviour {
    Honest,
    /// SSDP answers; the HTTP server accepts connections and never answers.
    HttpSilent,
    /// PCP and NAT-PMP grant the first new UDP mapping another external
    /// port than the one suggested (TCP's), honestly afterwards.
    OtherUdpPort,
    /// PCP refuses every mapping (`NOT_AUTHORIZED`); NAT-PMP, if on, works.
    PcpRefuses,
    /// Every method names 10.0.0.1 as the external address (a double NAT).
    PrivateExternal,
    /// A mapping's renewal is refused and the mapping dropped.
    RenewRefused,
    /// garbage in every SSDP, HTTP and PCP/NAT-PMP answer
    Garbage,
}

/// A fake router's mapping: who made it and where it leads.
#[derive(Clone, Copy, Debug)]
struct Entry {
    method: Method,
    internal: SocketAddrV4,
}

#[derive(Default)]
struct Table {
    /// By (UDP, external port).
    mappings: BTreeMap<(bool, u16), Entry>,
    /// External ports another client holds, for both protocols.
    held: BTreeSet<u16>,
    /// `OtherUdpPort` has moved a UDP mapping.
    moved_udp: bool,
}

/// What the router does with a PCP or NAT-PMP request.
enum Outcome {
    Granted(u16),
    Refused,
}

impl Table {
    fn taken(&self, udp: bool, port: u16) -> bool {
        self.held.contains(&port) || self.mappings.contains_key(&(udp, port))
    }

    /// The external port of `method`'s mapping of `udp` to `internal`.
    fn find(&self, method: Method, udp: bool, internal: SocketAddrV4) -> Option<u16> {
        self.mappings
            .iter()
            .find(|(k, e)| k.0 == udp && e.method == method && e.internal == internal)
            .map(|(k, _)| k.1)
    }

    /// A PCP or NAT-PMP mapping request: a renewal refreshes (or, under
    /// `RenewRefused`, drops) the client's mapping; a new one takes the
    /// suggested port or the next free one.
    fn grant(
        &mut self,
        method: Method,
        m: Misbehaviour,
        udp: bool,
        internal: SocketAddrV4,
        suggested: u16,
    ) -> Outcome {
        if let Some(port) = self.find(method, udp, internal) {
            if m == Misbehaviour::RenewRefused {
                self.mappings.remove(&(udp, port));
                return Outcome::Refused;
            }
            return Outcome::Granted(port);
        }
        let mut port = if suggested == 0 { internal.port() } else { suggested };
        if udp && m == Misbehaviour::OtherUdpPort && !self.moved_udp {
            self.moved_udp = true;
            port = port.wrapping_add(1000);
        }
        for _ in 0..=u16::MAX {
            if port != 0 && !self.taken(udp, port) {
                self.mappings.insert((udp, port), Entry { method, internal });
                return Outcome::Granted(port);
            }
            port = port.wrapping_add(1);
        }
        Outcome::Refused
    }

    fn delete(&mut self, method: Method, udp: bool, internal: SocketAddrV4) {
        if let Some(port) = self.find(method, udp, internal) {
            self.mappings.remove(&(udp, port));
        }
    }
}

struct Config {
    upnp: bool,
    pmp: bool,
    pcp: bool,
    external: Ipv4Addr,
    m: Misbehaviour,
    /// `serving`'s answer to every SSDP and HTTP request.
    answer: Option<Vec<u8>>,
    http: SocketAddr,
    started: Instant,
}

struct Inner {
    cfg: Config,
    table: Mutex<Table>,
    stop: AtomicBool,
}

impl Inner {
    fn table(&self) -> MutexGuard<'_, Table> {
        self.table.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn epoch(&self) -> u32 {
        u32::try_from(self.cfg.started.elapsed().as_secs()).unwrap_or(u32::MAX)
    }

    fn external(&self) -> Ipv4Addr {
        if self.cfg.m == Misbehaviour::PrivateExternal {
            Ipv4Addr::new(10, 0, 0, 1)
        } else {
            self.cfg.external
        }
    }

    fn garbage(&self) -> bool {
        self.cfg.m == Misbehaviour::Garbage
    }
}

/// A router on 127.0.0.1: SSDP, HTTP and PCP/NAT-PMP sockets on ports the
/// system picks. Dropping it stops and joins its threads.
pub struct FakeGateway {
    ssdp: SocketAddr,
    pmp: SocketAddr,
    inner: Arc<Inner>,
    threads: Vec<JoinHandle<()>>,
}

/// How often the threads look at the stop flag.
const TICK: Duration = Duration::from_millis(5);

impl FakeGateway {
    /// A router speaking UPnP-IGD, NAT-PMP and PCP as asked, with the
    /// external address `external` (unless `PrivateExternal`). Its SSDP
    /// socket exists either way; without UPnP it never answers.
    pub fn start(upnp: bool, pmp: bool, pcp: bool, external: Ipv4Addr, m: Misbehaviour) -> FakeGateway {
        FakeGateway::launch(upnp, pmp, pcp, external, m, None)
    }

    /// Fuzzing: SSDP and HTTP answer every request with `answer`; PCP/NAT-PMP stay silent.
    /// Every `$HTTP` in `answer` becomes the HTTP server's address, so an
    /// SSDP answer can lead to it; each SSDP answer is followed by an empty
    /// datagram, which ends igd-next's search at once once the device fails
    /// (it otherwise waits out its timeout for another device).
    pub fn serving(answer: Vec<u8>) -> FakeGateway {
        FakeGateway::launch(true, false, false, Ipv4Addr::UNSPECIFIED, Misbehaviour::Honest, Some(answer))
    }

    fn launch(
        upnp: bool,
        pmp: bool,
        pcp: bool,
        external: Ipv4Addr,
        m: Misbehaviour,
        answer: Option<Vec<u8>>,
    ) -> FakeGateway {
        let local = (Ipv4Addr::LOCALHOST, 0);
        let ssdp = UdpSocket::bind(local).expect("bind the fake's SSDP socket");
        let pmp_sock = UdpSocket::bind(local).expect("bind the fake's PCP/NAT-PMP socket");
        let http = TcpListener::bind(local).expect("bind the fake's HTTP server");
        let http_addr = http.local_addr().expect("the HTTP server's address");
        let answer = answer.map(|a| replace(&a, b"$HTTP", http_addr.to_string().as_bytes()));
        let inner = Arc::new(Inner {
            cfg: Config { upnp, pmp, pcp, external, m, answer, http: http_addr, started: Instant::now() },
            table: Mutex::new(Table::default()),
            stop: AtomicBool::new(false),
        });
        let addrs = (ssdp.local_addr().expect("SSDP address"), pmp_sock.local_addr().expect("PCP address"));
        let spawn = |name: &str, f: Box<dyn FnOnce() + Send>| {
            std::thread::Builder::new().name(name.into()).spawn(f).expect("spawn a fake gateway thread")
        };
        let threads = vec![
            spawn("fake ssdp", {
                let inner = Arc::clone(&inner);
                Box::new(move || serve_ssdp(&inner, &ssdp))
            }),
            spawn("fake pcp", {
                let inner = Arc::clone(&inner);
                Box::new(move || serve_pmp(&inner, &pmp_sock))
            }),
            spawn("fake http", {
                let inner = Arc::clone(&inner);
                Box::new(move || serve_http(&inner, &http))
            }),
        ];
        FakeGateway { ssdp: addrs.0, pmp: addrs.1, inner, threads }
    }

    /// Where its SSDP socket listens (`MapRequest::ssdp`).
    pub fn ssdp(&self) -> SocketAddr {
        self.ssdp
    }

    /// Where its PCP/NAT-PMP socket listens (`MapRequest::pmp_port`, gateway 127.0.0.1).
    pub fn pmp(&self) -> SocketAddr {
        self.pmp
    }

    /// The mappings it holds: method, UDP (else TCP), external port, internal address.
    pub fn mappings(&self) -> Vec<(Method, bool /* udp */, u16 /* external */, SocketAddrV4)> {
        let rank = |m: Method| match m {
            Method::Upnp => 0,
            Method::NatPmp => 1,
            Method::Pcp => 2,
        };
        let mut all: Vec<_> = self
            .inner
            .table()
            .mappings
            .iter()
            .map(|(&(udp, port), e)| (e.method, udp, port, e.internal))
            .collect();
        all.sort_by_key(|&(m, udp, port, internal)| (rank(m), udp, port, internal));
        all
    }

    /// Another client holds `external` for both protocols: UPnP refuses it
    /// (718), PCP and NAT-PMP grant another port.
    pub fn hold(&self, external: u16) {
        self.inner.table().held.insert(external);
    }
}

impl Drop for FakeGateway {
    fn drop(&mut self) {
        self.inner.stop.store(true, Ordering::Relaxed);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

/// `bytes` with every `from` replaced by `to`.
fn replace(bytes: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i..].starts_with(from) {
            out.extend_from_slice(to);
            i += from.len();
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

fn serve_ssdp(inner: &Inner, sock: &UdpSocket) {
    let _ = sock.set_read_timeout(Some(TICK));
    let mut buf = [0u8; 2048];
    while !inner.stop.load(Ordering::Relaxed) {
        let Ok((_, from)) = sock.recv_from(&mut buf) else { continue };
        if let Some(answer) = &inner.cfg.answer {
            let _ = sock.send_to(answer, from);
            let _ = sock.send_to(&[], from);
            continue;
        }
        if !inner.cfg.upnp {
            continue;
        }
        let location = format!("LOCATION: http://{}/desc.xml\r\n", inner.cfg.http);
        let head = "HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age=120\r\n\
            ST: urn:schemas-upnp-org:device:InternetGatewayDevice:1\r\n\
            USN: uuid:fake-gateway::urn:schemas-upnp-org:device:InternetGatewayDevice:1\r\nEXT:\r\n";
        if inner.garbage() {
            // A junk line before a real LOCATION (so HTTP is reached), then
            // a datagram that is not UTF-8.
            let _ =
                sock.send_to(format!("HTTP/1.1 200 OK\r\n\x01\x02 junk\r\n{location}\r\n").as_bytes(), from);
            let _ = sock.send_to(&[0xff, 0xfe, 0x00], from);
        } else {
            let _ = sock.send_to(format!("{head}{location}\r\n").as_bytes(), from);
        }
    }
}

fn serve_pmp(inner: &Inner, sock: &UdpSocket) {
    let _ = sock.set_read_timeout(Some(TICK));
    let mut buf = [0u8; 1100];
    while !inner.stop.load(Ordering::Relaxed) {
        let Ok((n, from)) = sock.recv_from(&mut buf) else { continue };
        let SocketAddr::V4(from) = from else { continue };
        if inner.cfg.answer.is_some() {
            continue;
        }
        if let Some(answer) = answer_pmp(inner, &buf[..n], *from.ip()) {
            let _ = sock.send_to(&answer, from);
        }
    }
}

/// The answer to a PCP (version 2) or NAT-PMP (version 0) request from `source`.
fn answer_pmp(inner: &Inner, req: &[u8], source: Ipv4Addr) -> Option<Vec<u8>> {
    let cfg = &inner.cfg;
    let epoch = inner.epoch().to_be_bytes();
    match *req.first()? {
        2 if cfg.pcp => answer_pcp(inner, req),
        // RFC 6887 §9: a NAT-PMP-only server answers PCP with version 0, UNSUPP_VERSION.
        2 if cfg.pmp => {
            let op = req.get(1).copied().unwrap_or(0) & 0x7f;
            let mut a = vec![0, 0x80 | op, 0, 1];
            a.extend(epoch);
            Some(a)
        }
        0 if cfg.pmp => {
            let op = *req.get(1)?;
            let mut a = vec![0, 0x80 | op, 0, 0];
            a.extend(epoch);
            match op {
                0 => a.extend(inner.external().octets()),
                1 | 2 if req.len() >= 12 => {
                    let udp = op == 1;
                    let internal = SocketAddrV4::new(source, u16::from_be_bytes([req[4], req[5]]));
                    let suggested = u16::from_be_bytes([req[6], req[7]]);
                    let lifetime = u32::from_be_bytes([req[8], req[9], req[10], req[11]]);
                    let mut table = inner.table();
                    let (result, port, lifetime) = if lifetime == 0 {
                        table.delete(Method::NatPmp, udp, internal);
                        (0u16, 0u16, 0u32)
                    } else {
                        match table.grant(Method::NatPmp, cfg.m, udp, internal, suggested) {
                            Outcome::Granted(port) => (0, port, lifetime.min(7200)),
                            Outcome::Refused => (2, 0, 0),
                        }
                    };
                    a[2..4].copy_from_slice(&result.to_be_bytes());
                    a.extend(internal.port().to_be_bytes());
                    a.extend(port.to_be_bytes());
                    a.extend(lifetime.to_be_bytes());
                }
                _ => return None,
            }
            if inner.garbage() {
                a.pop();
            }
            Some(a)
        }
        _ => None,
    }
}

/// A PCP MAP answer (RFC 6887 §7.2, §11.1).
fn answer_pcp(inner: &Inner, req: &[u8]) -> Option<Vec<u8>> {
    if req.len() < 60 || req[1] != 1 {
        return None;
    }
    let lifetime = u32::from_be_bytes([req[4], req[5], req[6], req[7]]);
    let client = Ipv4Addr::new(req[20], req[21], req[22], req[23]);
    let nonce = &req[24..36];
    let protocol = req[36];
    let internal = SocketAddrV4::new(client, u16::from_be_bytes([req[40], req[41]]));
    let suggested = u16::from_be_bytes([req[42], req[43]]);
    let udp = protocol == 17;
    let (result, port, lifetime) = if inner.cfg.m == Misbehaviour::PcpRefuses {
        (2u8, 0u16, 0u32)
    } else if lifetime == 0 {
        inner.table().delete(Method::Pcp, udp, internal);
        (0, 0, 0)
    } else {
        match inner.table().grant(Method::Pcp, inner.cfg.m, udp, internal, suggested) {
            Outcome::Granted(port) => (0, port, lifetime.min(7200)),
            Outcome::Refused => (2, 0, 0),
        }
    };
    let mut a = vec![2, 0x81, 0, result];
    a.extend(lifetime.to_be_bytes());
    a.extend(inner.epoch().to_be_bytes());
    a.extend([0u8; 12]);
    if inner.garbage() {
        a.extend(nonce.iter().map(|b| !b));
    } else {
        a.extend(nonce);
    }
    a.extend([protocol, 0, 0, 0]);
    a.extend(internal.port().to_be_bytes());
    a.extend(port.to_be_bytes());
    let ip = if result == 0 && port != 0 { inner.external() } else { Ipv4Addr::UNSPECIFIED };
    a.extend([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff]);
    a.extend(ip.octets());
    Some(a)
}

fn serve_http(inner: &Inner, listener: &TcpListener) {
    if listener.set_nonblocking(true).is_err() {
        return;
    }
    // `HttpSilent`'s connections, held open and never answered.
    let mut silent = Vec::new();
    while !inner.stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                if inner.cfg.m == Misbehaviour::HttpSilent && inner.cfg.answer.is_none() {
                    silent.push(stream);
                } else {
                    serve_one(inner, stream);
                }
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(2)),
            Err(_) => std::thread::sleep(TICK),
        }
    }
}

/// Reads one HTTP request (head and a `Content-Length` body, at most 64 KiB)
/// and answers it, then closes the connection.
fn serve_one(inner: &Inner, mut stream: TcpStream) {
    if stream.set_nonblocking(false).is_err()
        || stream.set_read_timeout(Some(Duration::from_secs(1))).is_err()
    {
        return;
    }
    let mut req = Vec::new();
    let mut buf = [0u8; 4096];
    let head_end = loop {
        if let Some(i) = req.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        match stream.read(&mut buf) {
            Ok(n) if n > 0 && req.len() < 65536 => req.extend_from_slice(&buf[..n]),
            _ => return,
        }
    };
    let head = String::from_utf8_lossy(&req[..head_end]).into_owned();
    let length = head
        .lines()
        .find_map(|l| {
            let (name, value) = l.split_once(':')?;
            name.trim().eq_ignore_ascii_case("content-length").then(|| value.trim().parse::<usize>().ok())?
        })
        .unwrap_or(0)
        .min(65536);
    while req.len() < head_end + length {
        match stream.read(&mut buf) {
            Ok(n) if n > 0 => req.extend_from_slice(&buf[..n]),
            _ => break,
        }
    }
    let body = String::from_utf8_lossy(&req[head_end..]).into_owned();
    let answer = match &inner.cfg.answer {
        Some(answer) => answer.clone(),
        None if inner.garbage() => b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\n<root><de".to_vec(),
        None => respond(inner, &head, &body),
    };
    let _ = stream.write_all(&answer);
    let _ = stream.flush();
}

const DESC: &str = r#"<?xml version="1.0"?>
<root xmlns="urn:schemas-upnp-org:device-1-0">
<specVersion><major>1</major><minor>0</minor></specVersion>
<device>
<deviceType>urn:schemas-upnp-org:device:InternetGatewayDevice:1</deviceType>
<friendlyName>FakeGateway</friendlyName>
<deviceList><device>
<deviceType>urn:schemas-upnp-org:device:WANDevice:1</deviceType>
<deviceList><device>
<deviceType>urn:schemas-upnp-org:device:WANConnectionDevice:1</deviceType>
<serviceList><service>
<serviceType>urn:schemas-upnp-org:service:WANIPConnection:1</serviceType>
<serviceId>urn:upnp-org:serviceId:WANIPConn1</serviceId>
<SCPDURL>/scpd.xml</SCPDURL>
<controlURL>/ctl</controlURL>
<eventSubURL>/evt</eventSubURL>
</service></serviceList>
</device></deviceList>
</device></deviceList>
</device>
</root>"#;

/// An SCPD action with its `in` arguments.
fn action(name: &str, args: &[&str], out: &[&str]) -> String {
    let arg =
        |n: &&str, dir: &str| format!("<argument><name>{n}</name><direction>{dir}</direction></argument>");
    let args: String = args.iter().map(|n| arg(n, "in")).chain(out.iter().map(|n| arg(n, "out"))).collect();
    format!("<action><name>{name}</name><argumentList>{args}</argumentList></action>")
}

fn scpd() -> String {
    let add = [
        "NewRemoteHost",
        "NewExternalPort",
        "NewProtocol",
        "NewInternalPort",
        "NewInternalClient",
        "NewEnabled",
        "NewPortMappingDescription",
        "NewLeaseDuration",
    ];
    format!(
        "<?xml version=\"1.0\"?><scpd xmlns=\"urn:schemas-upnp-org:service-1-0\">\
         <specVersion><major>1</major><minor>0</minor></specVersion><actionList>{}{}{}</actionList></scpd>",
        action("AddPortMapping", &add, &[]),
        action("DeletePortMapping", &["NewRemoteHost", "NewExternalPort", "NewProtocol"], &[]),
        action("GetExternalIPAddress", &[], &["NewExternalIPAddress"]),
    )
}

fn http(status: &str, body: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/xml; charset=\"utf-8\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

fn soap(body: &str) -> String {
    format!(
        "<?xml version=\"1.0\"?><s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
         s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\"><s:Body>{body}</s:Body></s:Envelope>"
    )
}

fn soap_ok(action: &str, inner: &str) -> Vec<u8> {
    let body = format!(
        "<u:{action}Response xmlns:u=\"urn:schemas-upnp-org:service:WANIPConnection:1\">{inner}</u:{action}Response>"
    );
    http("200 OK", &soap(&body))
}

fn soap_fault(code: u16, description: &str) -> Vec<u8> {
    let body = format!(
        "<s:Fault><faultcode>s:Client</faultcode><faultstring>UPnPError</faultstring><detail>\
         <UPnPError xmlns=\"urn:schemas-upnp-org:control-1-0\"><errorCode>{code}</errorCode>\
         <errorDescription>{description}</errorDescription></UPnPError></detail></s:Fault>"
    );
    http("500 Internal Server Error", &soap(&body))
}

/// The text of the first `<name>…</name>` in `body`.
fn arg<'a>(body: &'a str, name: &str) -> Option<&'a str> {
    let start = body.find(&format!("<{name}>"))? + name.len() + 2;
    let len = body[start..].find(&format!("</{name}>"))?;
    Some(body[start..start + len].trim())
}

fn respond(inner: &Inner, head: &str, body: &str) -> Vec<u8> {
    let line = head.lines().next().unwrap_or("");
    if line.starts_with("GET /desc.xml") {
        return http("200 OK", DESC);
    }
    if line.starts_with("GET /scpd.xml") {
        return http("200 OK", &scpd());
    }
    if !line.starts_with("POST /ctl") {
        return http("404 Not Found", "");
    }
    let action = head
        .lines()
        .find_map(|l| {
            let (name, value) = l.split_once(':')?;
            name.trim().eq_ignore_ascii_case("soapaction").then(|| value.trim().trim_matches('"').to_string())
        })
        .unwrap_or_default();
    let action = action.rsplit('#').next().unwrap_or("");
    let udp = arg(body, "NewProtocol") == Some("UDP");
    let port = arg(body, "NewExternalPort").and_then(|p| p.parse::<u16>().ok());
    match action {
        "GetExternalIPAddress" => {
            soap_ok(action, &format!("<NewExternalIPAddress>{}</NewExternalIPAddress>", inner.external()))
        }
        "AddPortMapping" => {
            let internal = arg(body, "NewInternalClient")
                .and_then(|ip| ip.parse::<Ipv4Addr>().ok())
                .zip(arg(body, "NewInternalPort").and_then(|p| p.parse::<u16>().ok()));
            let (Some(port), Some((ip, internal_port))) = (port, internal) else {
                return soap_fault(402, "Invalid Args");
            };
            let internal = SocketAddrV4::new(ip, internal_port);
            let mut table = inner.table();
            if table.held.contains(&port) {
                return soap_fault(718, "ConflictInMappingEntry");
            }
            match table.mappings.get(&(udp, port)) {
                Some(e) if e.method == Method::Upnp && e.internal == internal => {
                    if inner.cfg.m == Misbehaviour::RenewRefused {
                        table.mappings.remove(&(udp, port));
                        return soap_fault(606, "Action not authorized");
                    }
                }
                Some(_) => return soap_fault(718, "ConflictInMappingEntry"),
                None => {
                    table.mappings.insert((udp, port), Entry { method: Method::Upnp, internal });
                }
            }
            soap_ok(action, "")
        }
        "DeletePortMapping" => {
            let Some(port) = port else { return soap_fault(402, "Invalid Args") };
            match inner.table().mappings.remove(&(udp, port)) {
                Some(_) => soap_ok(action, ""),
                None => soap_fault(714, "NoSuchEntryInArray"),
            }
        }
        _ => soap_fault(401, "Invalid Action"),
    }
}
