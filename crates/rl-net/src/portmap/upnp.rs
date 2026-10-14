//! UPnP-IGD (N51, ruling PM-1): our own client, the `rl-net upnp` thread.
//! A LAN device must never crash or exhaust the game, so every step is
//! bounded and nothing recurses:
//! - **SSDP:** an M-SEARCH with `MX: 1`, re-sent within the window; a reply
//!   counts only when its `LOCATION` host is its own (non-global) sender,
//!   and a bad reply is skipped, never fatal.
//! - **HTTP/1.1:** connect, read and overall deadlines, never a proxy (the
//!   environment is ignored), no names, no redirects; heads up to 8 KiB and
//!   bodies up to 64 KiB, framed by `Content-Length`, chunks or the close.
//! - **The description:** a flat scan for a `WANIPConnection:1`/`:2` or
//!   `WANPPPConnection:1` service's `controlURL`, resolved against the
//!   `URLBase` or the `LOCATION`, on the device's own IP only.
//! - **SOAP:** `GetExternalIPAddress`, `AddPortMapping` and
//!   `DeletePortMapping`, arguments escaped, answers scanned flat.
//!
//! The parsers (`parse_ssdp`, `parse_http`, `control_urls`, `soap_result`)
//! are sans-IO and fuzzed in `tests/portmap.rs`.

use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream, UdpSocket};
use std::time::{Duration, Instant};

use super::{Ctx, Grant, MapError, Method, Router, UPNP_DEADLINE, UPNP_LIFETIME, tidy, try_router};

/// The SSDP search window when the request names none.
pub const SEARCH: Duration = Duration::from_secs(2);
/// The largest HTTP head we read.
pub const HEAD_CAP: usize = 8 * 1024;
/// The largest HTTP body we read (real descriptions are 2-20 KB).
pub const BODY_CAP: usize = 64 * 1024;
/// The most raw bytes one answer may take (head, body and chunk framing).
const RAW_CAP: usize = HEAD_CAP + BODY_CAP + 16 * 1024;
/// The longest a read blocks before the thread looks at the board.
const TICK: Duration = Duration::from_millis(50);
/// A deletion's own deadline.
const DELETE_WAIT: Duration = Duration::from_millis(450);
/// The longest one device's description and external address may take.
const DEVICE_WAIT: Duration = Duration::from_secs(2);
/// Distinct devices (`LOCATION`s) tried per search.
const MAX_DEVICES: usize = 4;
/// Connection services tried per device.
const MAX_SERVICES: usize = 4;
/// The longest URL path we send.
const MAX_PATH: usize = 512;

/// The connection services we drive, in their SOAP namespaces.
pub const SERVICES: [&str; 3] = [
    "urn:schemas-upnp-org:service:WANIPConnection:1",
    "urn:schemas-upnp-org:service:WANIPConnection:2",
    "urn:schemas-upnp-org:service:WANPPPConnection:1",
];

const M_SEARCH: &[u8] =
    b"M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nMAN: \"ssdp:discover\"\r\nMX: 1\r\n\
ST: urn:schemas-upnp-org:device:InternetGatewayDevice:1\r\n\r\n";

/// An HTTP address on a device: its IPv4 address and port, and a path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Location {
    pub addr: SocketAddrV4,
    pub path: String,
}

/// A connection service's control URL and its service type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Control {
    pub url: Location,
    pub service: &'static str,
}

/// The first index of `needle` in `hay`.
fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

fn trim_cr(line: &[u8]) -> &[u8] {
    line.strip_suffix(b"\r").unwrap_or(line)
}

/// `http://a.b.c.d[:port][/path]`: an IPv4 literal only (no names, so no
/// DNS), port 80 by default; the path must be printable without spaces.
pub fn parse_url(url: &str) -> Option<Location> {
    let scheme = url.get(..7)?;
    if !scheme.eq_ignore_ascii_case("http://") {
        return None;
    }
    let rest = url.get(7..)?;
    let (hostport, path) = match rest.find('/') {
        Some(i) => (rest.get(..i)?, rest.get(i..)?),
        None => (rest, "/"),
    };
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) => (h, p.parse::<u16>().ok()?),
        None => (hostport, 80),
    };
    let ip: Ipv4Addr = host.parse().ok()?;
    let path = path.split('#').next().unwrap_or("/");
    if port == 0 || !valid_path(path) {
        return None;
    }
    Some(Location { addr: SocketAddrV4::new(ip, port), path: path.to_string() })
}

fn valid_path(path: &str) -> bool {
    path.starts_with('/') && path.len() <= MAX_PATH && path.bytes().all(|b| b.is_ascii_graphic())
}

/// Reads an SSDP answer from `from`: an `HTTP/1.x 200` head naming a
/// `LOCATION` whose host is `from`'s own IP, which must not be global (a
/// router on the LAN). Anything else is `None`: the caller skips it.
pub fn parse_ssdp(bytes: &[u8], from: SocketAddr) -> Option<Location> {
    let SocketAddr::V4(from) = from else { return None };
    if rl_proto::is_global(IpAddr::V4(*from.ip())) {
        return None;
    }
    let mut lines = bytes.split(|&b| b == b'\n').map(trim_cr);
    let status = lines.next()?;
    let mut words = status.split(|&b| b == b' ');
    if !words.next()?.starts_with(b"HTTP/1.") || words.next()? != b"200" {
        return None;
    }
    for line in lines {
        if line.is_empty() {
            break;
        }
        let Ok(line) = std::str::from_utf8(line) else { continue };
        let Some((name, value)) = line.split_once(':') else { continue };
        if name.trim().eq_ignore_ascii_case("location") {
            let location = parse_url(value.trim())?;
            return (location.addr.ip() == from.ip()).then_some(location);
        }
    }
    None
}

/// Where reading an HTTP answer stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HttpParse {
    /// More bytes are needed.
    Incomplete,
    Done {
        status: u16,
        body: Vec<u8>,
    },
    /// Not an answer we take, and why.
    Bad(&'static str),
}

/// Reads an HTTP/1.x answer from the bytes so far (`eof`: the peer closed):
/// a head of at most `HEAD_CAP`, then a body of at most `BODY_CAP` framed by
/// `Transfer-Encoding: chunked`, `Content-Length` or the close.
pub fn parse_http(bytes: &[u8], eof: bool) -> HttpParse {
    let Some(end) = find(&bytes[..bytes.len().min(HEAD_CAP + 4)], b"\r\n\r\n") else {
        return if bytes.len() > HEAD_CAP {
            HttpParse::Bad("a head over 8 KiB")
        } else if eof {
            HttpParse::Bad("a cut head")
        } else {
            HttpParse::Incomplete
        };
    };
    let (head, body) = (&bytes[..end], &bytes[end + 4..]);
    let mut lines = head.split(|&b| b == b'\n').map(trim_cr);
    let mut words = lines.next().unwrap_or(&[]).split(|&b| b == b' ');
    if !words.next().unwrap_or(&[]).starts_with(b"HTTP/1.") {
        return HttpParse::Bad("not an HTTP answer");
    }
    let status = match words.next().map(std::str::from_utf8) {
        Some(Ok(s)) if s.len() == 3 => match s.parse::<u16>() {
            Ok(code @ 100..=599) => code,
            _ => return HttpParse::Bad("a bad status"),
        },
        _ => return HttpParse::Bad("a bad status"),
    };
    let (mut chunked, mut length) = (false, None);
    for line in lines {
        let Ok(line) = std::str::from_utf8(line) else { continue };
        let Some((name, value)) = line.split_once(':') else { continue };
        let (name, value) = (name.trim(), value.trim());
        if name.eq_ignore_ascii_case("transfer-encoding") {
            chunked |= value.to_ascii_lowercase().contains("chunked");
        } else if name.eq_ignore_ascii_case("content-length") && length.is_none() {
            match value.parse::<usize>() {
                Ok(n) => length = Some(n),
                Err(_) => return HttpParse::Bad("a bad Content-Length"),
            }
        }
    }
    if chunked {
        return match dechunk(body) {
            Ok(Some(body)) => HttpParse::Done { status, body },
            Ok(None) if eof => HttpParse::Bad("a cut chunked body"),
            Ok(None) => HttpParse::Incomplete,
            Err(why) => HttpParse::Bad(why),
        };
    }
    match length {
        Some(n) if n > BODY_CAP => HttpParse::Bad("a body over 64 KiB"),
        Some(n) if body.len() >= n => HttpParse::Done { status, body: body[..n].to_vec() },
        Some(_) if eof => HttpParse::Bad("a cut body"),
        Some(_) => HttpParse::Incomplete,
        None if body.len() > BODY_CAP => HttpParse::Bad("a body over 64 KiB"),
        None if eof => HttpParse::Done { status, body: body.to_vec() },
        None => HttpParse::Incomplete,
    }
}

/// A chunked body (RFC 9112 §7.1), iteratively: `Ok(None)` while cut short.
/// Chunk extensions and trailers are ignored.
fn dechunk(mut b: &[u8]) -> Result<Option<Vec<u8>>, &'static str> {
    let mut out = Vec::new();
    loop {
        let Some(i) = find(&b[..b.len().min(1024)], b"\r\n") else {
            return if b.len() > 1024 { Err("a bad chunk size") } else { Ok(None) };
        };
        let line = &b[..i];
        let size_text = line.split(|&c| c == b';').next().unwrap_or(line).trim_ascii();
        if size_text.is_empty() || size_text.len() > 8 {
            return Err("a bad chunk size");
        }
        let mut size = 0usize;
        for &c in size_text {
            let digit = match c {
                b'0'..=b'9' => c - b'0',
                b'a'..=b'f' => c - b'a' + 10,
                b'A'..=b'F' => c - b'A' + 10,
                _ => return Err("a bad chunk size"),
            };
            size = size * 16 + usize::from(digit);
        }
        b = &b[i + 2..];
        if size == 0 {
            return Ok(Some(out));
        }
        if size > BODY_CAP - out.len() {
            return Err("a body over 64 KiB");
        }
        if b.len() < size + 2 {
            return Ok(None);
        }
        if &b[size..size + 2] != b"\r\n" {
            return Err("a chunk without its CRLF");
        }
        out.extend_from_slice(&b[..size]);
        b = &b[size + 2..];
    }
}

/// The local name of a tag: after its namespace prefix.
fn local(name: &[u8]) -> &[u8] {
    match name.iter().rposition(|&c| c == b':') {
        Some(i) => &name[i + 1..],
        None => name,
    }
}

/// The first element named `name` (any namespace prefix) at or after byte
/// `from` of `xml`: its content's start and end, and where its end tag
/// ends. A flat scan — no tree, no recursion — so nesting costs nothing; a
/// nested element of the same name closes at the first end tag.
pub fn find_element(xml: &str, name: &str, from: usize) -> Option<(usize, usize, usize)> {
    let b = xml.as_bytes();
    let is_end = |c: u8| matches!(c, b' ' | b'\t' | b'\r' | b'\n' | b'>' | b'/');
    let mut i = from;
    while i < b.len() {
        let open = i + b.get(i..)?.iter().position(|&c| c == b'<')?;
        let start = open + 1;
        let name_end = start + b[start..].iter().position(|&c| is_end(c)).unwrap_or(b.len() - start);
        let tag = &b[start..name_end];
        i = start;
        if tag.first().is_some_and(|c| matches!(c, b'/' | b'?' | b'!')) || local(tag) != name.as_bytes() {
            continue;
        }
        let gt = name_end + b[name_end..].iter().position(|&c| c == b'>')?;
        if b[gt - 1] == b'/' && gt > name_end {
            return Some((gt + 1, gt + 1, gt + 1));
        }
        let mut k = gt + 1;
        while let Some(off) = find(&b[k..], b"</") {
            let close = k + off + 2;
            let close_end = close + b[close..].iter().position(|&c| is_end(c)).unwrap_or(b.len() - close);
            if local(&b[close..close_end]) == name.as_bytes() {
                let after = close_end + b[close_end..].iter().position(|&c| c == b'>').map_or(0, |p| p + 1);
                return Some((gt + 1, k + off, after));
            }
            k = close;
        }
        return None;
    }
    None
}

/// The text of the first element named `name`, unescaped and trimmed.
pub fn element_text(xml: &str, name: &str) -> Option<String> {
    let (start, end, _) = find_element(xml, name, 0)?;
    Some(unescape(xml.get(start..end)?.trim()))
}

/// XML's five entities and numeric references; anything else stays as is.
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(semi) = rest.get(..rest.len().min(12)).and_then(|r| r.find(';')) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..semi];
        let c = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix("#x")
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse::<u32>().ok()))
                .and_then(char::from_u32),
        };
        match c {
            Some(c) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Escapes text for an XML element.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

/// The connection services in a device description fetched from
/// `location`, in document order (at most 4): each `<service>` whose
/// `<serviceType>` is one of `SERVICES`, with its `<controlURL>` resolved
/// against the `<URLBase>` (when on the device's IP) or `location`. A
/// control URL on another host is skipped.
pub fn control_urls(xml: &str, location: &Location) -> Vec<Control> {
    let base = element_text(xml, "URLBase")
        .and_then(|u| parse_url(&u))
        .filter(|u| u.addr.ip() == location.addr.ip())
        .unwrap_or_else(|| location.clone());
    let mut out = Vec::new();
    let mut i = 0;
    while out.len() < MAX_SERVICES {
        let Some((start, end, after)) = find_element(xml, "service", i) else { break };
        i = after.max(i + 1);
        let Some(block) = xml.get(start..end) else { continue };
        let Some(kind) = element_text(block, "serviceType") else { continue };
        let Some(service) = SERVICES.iter().find(|s| **s == kind) else { continue };
        let Some(url) = element_text(block, "controlURL").and_then(|c| resolve(&c, &base, location)) else {
            continue;
        };
        out.push(Control { url, service });
    }
    out
}

/// A URL from a description: absolute (on the device's IP only),
/// absolute-path (on the base's host) or relative to the base's directory.
fn resolve(url: &str, base: &Location, location: &Location) -> Option<Location> {
    if url.get(..7).is_some_and(|s| s.eq_ignore_ascii_case("http://")) {
        return parse_url(url).filter(|u| u.addr.ip() == location.addr.ip());
    }
    let path = if url.starts_with('/') {
        url.to_string()
    } else {
        let dir = base.path.rfind('/').map_or("/", |i| &base.path[..=i]);
        format!("{dir}{url}")
    };
    valid_path(&path).then_some(Location { addr: base.addr, path })
}

/// A SOAP call that did not succeed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SoapError {
    /// No usable answer: unreachable, silent, cut, oversized or malformed.
    Transport(String),
    /// The device answered with a UPnP error code (or none), and its words.
    Refused { code: Option<u16>, text: String },
}

impl std::fmt::Display for SoapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SoapError::Transport(why) => f.write_str(why),
            SoapError::Refused { code: Some(code), text } => write!(f, "error {code} ({text})"),
            SoapError::Refused { code: None, text } => f.write_str(text),
        }
    }
}

/// Reads a SOAP answer to `action`: HTTP 200 with its `<actionResponse>`
/// gives the response element's content; otherwise the UPnP `errorCode`
/// and `errorDescription`, if any.
pub fn soap_result(status: u16, body: &str, action: &str) -> Result<String, SoapError> {
    if status == 200 {
        let name = format!("{action}Response");
        return match find_element(body, &name, 0) {
            Some((start, end, _)) => Ok(body.get(start..end).unwrap_or("").to_string()),
            None => Err(SoapError::Refused { code: None, text: format!("an answer without {name}") }),
        };
    }
    let code = element_text(body, "errorCode").and_then(|c| c.parse::<u16>().ok());
    let text = element_text(body, "errorDescription").map_or_else(|| format!("HTTP {status}"), |t| tidy(&t));
    Err(SoapError::Refused { code, text })
}

/// One HTTP/1.1 exchange with a device, never through a proxy: the answer's
/// status and body, and our end's address. Bounded by `deadline` (connect,
/// writes and reads) and `going`, and by `HEAD_CAP`/`BODY_CAP`.
fn http(
    to: SocketAddrV4,
    request: &[u8],
    deadline: Instant,
    going: &dyn Fn() -> bool,
) -> Result<(u16, Vec<u8>, Ipv4Addr), String> {
    let left = |now: Instant| deadline.checked_duration_since(now).filter(|d| !d.is_zero());
    let late = || "no answer within the deadline".to_string();
    let mut stream = TcpStream::connect_timeout(&SocketAddr::V4(to), left(Instant::now()).ok_or_else(late)?)
        .map_err(|e| e.to_string())?;
    let ours = match stream.local_addr() {
        Ok(SocketAddr::V4(a)) => *a.ip(),
        _ => Ipv4Addr::UNSPECIFIED,
    };
    stream.set_write_timeout(Some(left(Instant::now()).ok_or_else(late)?)).map_err(|e| e.to_string())?;
    stream.write_all(request).map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        if !going() {
            return Err("stopped".into());
        }
        let wait = left(Instant::now()).ok_or_else(late)?.min(TICK);
        stream.set_read_timeout(Some(wait)).map_err(|e| e.to_string())?;
        let eof = match stream.read(&mut buf) {
            Ok(0) => true,
            Ok(n) => {
                raw.extend_from_slice(&buf[..n]);
                false
            }
            Err(e)
                if matches!(
                    e.kind(),
                    ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
                ) =>
            {
                continue;
            }
            Err(e) => return Err(e.to_string()),
        };
        match parse_http(&raw, eof) {
            HttpParse::Done { status, body } => return Ok((status, body, ours)),
            HttpParse::Bad(why) => return Err(why.into()),
            HttpParse::Incomplete if raw.len() > RAW_CAP => return Err("an answer over 64 KiB".into()),
            HttpParse::Incomplete => {}
        }
    }
}

fn get(url: &Location) -> Vec<u8> {
    format!("GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n", url.path, url.addr).into_bytes()
}

/// A SOAP request for `action` on `control` with `args` (escaped).
fn soap_request(control: &Control, action: &str, args: &[(&str, String)]) -> Vec<u8> {
    let args: String =
        args.iter().map(|(name, value)| format!("<{name}>{}</{name}>", escape(value))).collect();
    let body = format!(
        "<?xml version=\"1.0\"?>\r\n<s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
         s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\"><s:Body><u:{action} xmlns:u=\"{}\">\
         {args}</u:{action}></s:Body></s:Envelope>\r\n",
        control.service
    );
    format!(
        "POST {} HTTP/1.1\r\nHost: {}\r\nContent-Type: text/xml; charset=\"utf-8\"\r\n\
         SOAPAction: \"{}#{action}\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        control.url.path,
        control.url.addr,
        control.service,
        body.len()
    )
    .into_bytes()
}

fn soap(
    control: &Control,
    action: &str,
    args: &[(&str, String)],
    deadline: Instant,
    going: &dyn Fn() -> bool,
) -> Result<String, SoapError> {
    let (status, body, _) = http(control.url.addr, &soap_request(control, action, args), deadline, going)
        .map_err(SoapError::Transport)?;
    soap_result(status, &String::from_utf8_lossy(&body), action)
}

/// A router found by the search: its connection service, its external
/// address and our address toward it.
struct Device {
    control: Control,
    external: Ipv4Addr,
    ours: Ipv4Addr,
}

/// The `rl-net upnp` thread.
pub(crate) fn run(ctx: Ctx) {
    if let Err(why) = attempt(&ctx) {
        ctx.fail(format!("UPnP: {why}"));
    }
}

fn attempt(ctx: &Ctx) -> Result<(), String> {
    let device = search(ctx)?;
    if !ctx.live() {
        return Ok(());
    }
    let ip = match ctx.req.local.ip() {
        ip if ip.is_unspecified() => device.ours,
        ip => *ip,
    };
    if ip.is_unspecified() {
        return Err("no local address toward the router".into());
    }
    let mut router = Upnp {
        control: device.control,
        local: SocketAddrV4::new(ip, ctx.req.local.port()),
        lease: ctx.req.lifetime_or(UPNP_LIFETIME),
        external: device.external,
        description: ctx.req.description,
        deadline: ctx.deadline,
        renewal: false,
    };
    try_router(&mut router, ctx)
}

/// The local address the system routes toward `to` from.
fn route_to(to: Ipv4Addr) -> Option<Ipv4Addr> {
    let sock = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    sock.connect((to, 1900)).ok()?;
    match sock.local_addr().ok()? {
        SocketAddr::V4(a) if !a.ip().is_unspecified() => Some(*a.ip()),
        _ => None,
    }
}

/// The SSDP search: M-SEARCH at 0, ¼ and ½ of the window; every reply that
/// names its sender is tried (description, then `GetExternalIPAddress`) until
/// one works or the window ends.
fn search(ctx: &Ctx) -> Result<Device, String> {
    let req = &ctx.req;
    let bind = match req.local.ip() {
        ip if !ip.is_unspecified() => *ip,
        _ => req.gateway.and_then(route_to).unwrap_or(Ipv4Addr::UNSPECIFIED),
    };
    let sock = UdpSocket::bind((bind, 0)).map_err(|e| format!("no SSDP socket ({e})"))?;
    let _ = sock.set_multicast_ttl_v4(2);
    let start = Instant::now();
    let end = (start + req.search).min(ctx.deadline);
    let resends = [Duration::ZERO, req.search / 4, req.search / 2];
    let mut sent = 0;
    let mut tried: Vec<Location> = Vec::new();
    let mut why = "no router answered the search".to_string();
    let mut buf = [0u8; 1500];
    loop {
        if !ctx.live() {
            return Err("stopped".into());
        }
        let now = Instant::now();
        if now >= end {
            return Err(why);
        }
        if sent < resends.len() && now >= start + resends[sent] {
            let _ = sock.send_to(M_SEARCH, req.ssdp);
            sent += 1;
            continue;
        }
        let next = resends.get(sent).map_or(end, |&r| (start + r).min(end));
        let Some(wait) = next.checked_duration_since(now).filter(|d| !d.is_zero()) else { continue };
        if sock.set_read_timeout(Some(wait.min(TICK))).is_err() {
            return Err("the SSDP socket failed".into());
        }
        match sock.recv_from(&mut buf) {
            Ok((n, from)) => {
                let Some(location) = parse_ssdp(&buf[..n], from) else { continue };
                if tried.contains(&location) || tried.len() >= MAX_DEVICES {
                    continue;
                }
                tried.push(location.clone());
                match describe(&location, ctx) {
                    Ok(device) => return Ok(device),
                    Err(w) => why = format!("{}: {w}", location.addr),
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
                ) => {}
            // An ICMP report (Windows) or another transient error.
            Err(_) => std::thread::sleep(Duration::from_millis(5)),
        }
    }
}

/// Fetches a device's description and finds a connection service that
/// names its external IPv4 address.
fn describe(location: &Location, ctx: &Ctx) -> Result<Device, String> {
    let going = || ctx.live();
    // One device may not take the whole budget from the others.
    let deadline = (Instant::now() + DEVICE_WAIT).min(ctx.deadline);
    let (status, body, ours) = http(location.addr, &get(location), deadline, &going)?;
    if status != 200 {
        return Err(format!("its description answered HTTP {status}"));
    }
    let controls = control_urls(&String::from_utf8_lossy(&body), location);
    let mut why = "no WANIPConnection or WANPPPConnection service".to_string();
    for control in controls {
        match soap(&control, "GetExternalIPAddress", &[], deadline, &going) {
            Ok(content) => match element_text(&content, "NewExternalIPAddress")
                .and_then(|t| t.parse::<Ipv4Addr>().ok())
            {
                Some(external) if !external.is_unspecified() => {
                    return Ok(Device { control, external, ours });
                }
                _ => why = "no external IPv4 address".into(),
            },
            Err(e) => why = format!("no external address ({})", tidy(&e.to_string())),
        }
    }
    Err(why)
}

struct Upnp {
    control: Control,
    local: SocketAddrV4,
    /// Seconds; 0 once the router said it takes permanent leases only.
    lease: u32,
    external: Ipv4Addr,
    description: &'static str,
    deadline: Instant,
    renewal: bool,
}

fn protocol(udp: bool) -> &'static str {
    if udp { "UDP" } else { "TCP" }
}

impl Router for Upnp {
    fn method(&self) -> Method {
        Method::Upnp
    }

    /// `AddPortMapping` at exactly `external`. A router answering 725
    /// (only permanent leases) gets lease 0; 718 (taken) and 724 (same
    /// ports required) try the next port.
    fn map(&mut self, udp: bool, external: u16, ctx: &Ctx) -> Result<Grant, MapError> {
        let renewal = self.renewal;
        let going = move || if renewal { !ctx.stopped() } else { ctx.live() };
        loop {
            let args = [
                ("NewRemoteHost", String::new()),
                ("NewExternalPort", external.to_string()),
                ("NewProtocol", protocol(udp).to_string()),
                ("NewInternalPort", self.local.port().to_string()),
                ("NewInternalClient", self.local.ip().to_string()),
                ("NewEnabled", "1".to_string()),
                ("NewPortMappingDescription", self.description.to_string()),
                ("NewLeaseDuration", self.lease.to_string()),
            ];
            match soap(&self.control, "AddPortMapping", &args, self.deadline, &going) {
                Ok(_) => return Ok(Grant { port: external, lifetime: self.lease }),
                Err(SoapError::Refused { code: Some(725), .. }) if self.lease != 0 => self.lease = 0,
                Err(e @ SoapError::Refused { code: Some(718 | 724), .. }) => {
                    return Err(MapError::Conflict(format!("port {external}: {e}")));
                }
                Err(e @ SoapError::Refused { .. }) => return Err(MapError::Fatal(tidy(&e.to_string()))),
                Err(SoapError::Transport(why)) => return Err(MapError::Silent(tidy(&why))),
            }
        }
    }

    fn unmap(&mut self, udp: bool, external: u16) {
        let args = [
            ("NewRemoteHost", String::new()),
            ("NewExternalPort", external.to_string()),
            ("NewProtocol", protocol(udp).to_string()),
        ];
        let _ = soap(&self.control, "DeletePortMapping", &args, Instant::now() + DELETE_WAIT, &|| true);
    }

    fn external_ip(&self) -> Option<Ipv4Addr> {
        Some(self.external)
    }

    fn renewing(&mut self) {
        self.renewal = true;
        self.deadline = Instant::now() + UPNP_DEADLINE;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(ip: [u8; 4], port: u16) -> SocketAddr {
        SocketAddr::from((ip, port))
    }

    #[test]
    fn ssdp_answers_must_name_their_sender() {
        let answer = b"HTTP/1.1 200 OK\r\nST: x\r\nLocation: http://192.168.1.1:5000/rootDesc.xml\r\n\r\n";
        let ok = Location { addr: "192.168.1.1:5000".parse().unwrap(), path: "/rootDesc.xml".into() };
        assert_eq!(parse_ssdp(answer, at([192, 168, 1, 1], 1900)), Some(ok));
        assert_eq!(parse_ssdp(answer, at([192, 168, 1, 7], 1900)), None, "another sender");
        let public = b"HTTP/1.1 200 OK\r\nLOCATION: http://8.8.8.8/d.xml\r\n\r\n";
        assert_eq!(parse_ssdp(public, at([8, 8, 8, 8], 1900)), None, "a global sender");
        assert_eq!(parse_ssdp(b"\xff\xfe\x00", at([192, 168, 1, 1], 1900)), None);
        let name = b"HTTP/1.1 200 OK\r\nLOCATION: http://router.lan/d.xml\r\n\r\n";
        assert_eq!(parse_ssdp(name, at([192, 168, 1, 1], 1900)), None, "no names");
        let junk_line = b"HTTP/1.1 200 OK\r\n\xff junk\r\nLOCATION: http://10.0.0.1/d\r\n\r\n";
        assert!(parse_ssdp(junk_line, at([10, 0, 0, 1], 1900)).is_some(), "a bad line is skipped");
    }

    #[test]
    fn http_bodies_are_framed_and_capped() {
        let done = |b: &str| HttpParse::Done { status: 200, body: b.as_bytes().to_vec() };
        assert_eq!(parse_http(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nabcdef", false), done("abc"));
        assert_eq!(
            parse_http(b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\nabc", false),
            HttpParse::Incomplete
        );
        assert_eq!(
            parse_http(b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\nabc", true),
            HttpParse::Bad("a cut body")
        );
        assert_eq!(parse_http(b"HTTP/1.0 200 OK\r\n\r\nabc", false), HttpParse::Incomplete);
        assert_eq!(parse_http(b"HTTP/1.0 200 OK\r\n\r\nabc", true), done("abc"));
        let chunked = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: Chunked\r\n\r\n3;x=y\r\nabc\r\nA\r\n0123456789\r\n0\r\nT: 1\r\n\r\n";
        assert_eq!(parse_http(chunked, false), done("abc0123456789"));
        assert_eq!(parse_http(&chunked[..60], false), HttpParse::Incomplete);
        let bad = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabcX\r\n0\r\n\r\n";
        assert_eq!(parse_http(bad, false), HttpParse::Bad("a chunk without its CRLF"));
        let hex = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n";
        assert_eq!(parse_http(hex, false), HttpParse::Bad("a bad chunk size"));
        let big = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", BODY_CAP + 1);
        assert_eq!(parse_http(big.as_bytes(), false), HttpParse::Bad("a body over 64 KiB"));
        let big_chunk = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nFFFFFFFF\r\n";
        assert_eq!(parse_http(big_chunk, false), HttpParse::Bad("a body over 64 KiB"));
        assert_eq!(parse_http(&[b'a'; HEAD_CAP + 5], false), HttpParse::Bad("a head over 8 KiB"));
        assert_eq!(parse_http(b"SSH-2.0\r\n\r\n", false), HttpParse::Bad("not an HTTP answer"));
    }

    fn loc() -> Location {
        Location { addr: "192.168.1.1:5000".parse().unwrap(), path: "/rootDesc.xml".into() }
    }

    #[test]
    fn descriptions_name_their_connection_services() {
        let ip = "<root><device><serviceList><service><serviceType>urn:schemas-upnp-org:service:Layer3Forwarding:1\
            </serviceType><controlURL>/l3f</controlURL></service></serviceList><deviceList><device><serviceList>\
            <service><serviceType>urn:schemas-upnp-org:service:WANIPConnection:1</serviceType>\
            <controlURL>/ctl/IPConn</controlURL></service>\
            <service><serviceType>urn:schemas-upnp-org:service:WANPPPConnection:1</serviceType>\
            <controlURL>ppp?a=1&amp;b=2</controlURL></service></serviceList></device></deviceList></device></root>";
        let urls = control_urls(ip, &loc());
        assert_eq!(urls.len(), 2);
        assert_eq!((urls[0].url.path.as_str(), urls[0].service), ("/ctl/IPConn", SERVICES[0]));
        assert_eq!((urls[1].url.path.as_str(), urls[1].service), ("/ppp?a=1&b=2", SERVICES[2]));
        let based = "<root><URLBase>http://192.168.1.1:49152/igd/</URLBase><service>\
            <serviceType>urn:schemas-upnp-org:service:WANIPConnection:2</serviceType>\
            <controlURL>upnp/control</controlURL></service></root>";
        let urls = control_urls(based, &loc());
        assert_eq!(
            urls[0].url,
            Location { addr: "192.168.1.1:49152".parse().unwrap(), path: "/igd/upnp/control".into() }
        );
        let foreign = "<root><service><serviceType>urn:schemas-upnp-org:service:WANIPConnection:1</serviceType>\
            <controlURL>http://203.0.113.9/ctl</controlURL></service></root>";
        assert_eq!(control_urls(foreign, &loc()), vec![], "a control URL on another host");
        let deep = format!("{}{ip}{}", "<a>".repeat(100_000), "</a>".repeat(100_000));
        assert_eq!(control_urls(&deep, &loc()).len(), 2, "depth costs nothing");
        assert_eq!(control_urls("<service><service><<</", &loc()), vec![]);
    }

    #[test]
    fn soap_answers_and_faults() {
        let ok = "<s:Envelope><s:Body><u:GetExternalIPAddressResponse xmlns:u=\"x\">\
            <NewExternalIPAddress>203.0.113.7</NewExternalIPAddress></u:GetExternalIPAddressResponse></s:Body></s:Envelope>";
        let content = soap_result(200, ok, "GetExternalIPAddress").unwrap();
        assert_eq!(element_text(&content, "NewExternalIPAddress").as_deref(), Some("203.0.113.7"));
        let fault = "<s:Fault><detail><UPnPError><errorCode>725</errorCode>\
            <errorDescription>OnlyPermanentLeasesSupported</errorDescription></UPnPError></detail></s:Fault>";
        assert_eq!(
            soap_result(500, fault, "AddPortMapping"),
            Err(SoapError::Refused { code: Some(725), text: "OnlyPermanentLeasesSupported".into() })
        );
        assert!(soap_result(200, "<nope/>", "AddPortMapping").is_err());
        assert_eq!(soap_result(200, "<u:AddPortMappingResponse/>", "AddPortMapping"), Ok(String::new()));
        assert_eq!(escape("a<b>&\"'"), "a&lt;b&gt;&amp;&quot;&apos;");
        assert_eq!(unescape("&lt;&#65;&#x42;&bogus;&"), "<AB&bogus;&");
    }
}
