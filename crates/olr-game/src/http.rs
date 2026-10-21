//! A small HTTP/1.1 client over TLS for `https://` (rustls with Mozilla's
//! roots from `webpki-roots`, as the relay pool's, never the platform's)
//! or plain for `http://` (a local mock in tests). A bug report's one POST
//! (issue #3: `Connection: close`, a `Content-Length` or chunked answer,
//! every wait bounded, the answer at most [`MAX_RESPONSE`] bytes), and the
//! update check's GETs (issue #4, [`get`]: redirects followed, the body
//! streamed to a sink with its progress, at most a given size). Call it
//! off the UI thread: it resolves names and blocks.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The most bytes of an answer read (head and body).
pub const MAX_RESPONSE: usize = 64 * 1024;

/// Where a request goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Url {
    pub tls: bool,
    pub host: String,
    pub port: u16,
    /// From its `/`, with the query.
    pub path: String,
}

/// `https://host[:port]/path` or `http://…`; anything else is refused.
pub fn parse_url(text: &str) -> Result<Url, String> {
    let text = text.trim();
    let (tls, rest) = if let Some(r) = text.strip_prefix("https://") {
        (true, r)
    } else if let Some(r) = text.strip_prefix("http://") {
        (false, r)
    } else {
        return Err(format!("{text:?} is no http:// or https:// address"));
    };
    let (authority, path) = match rest.find(['/', '?']) {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let path = if path.starts_with('?') { format!("/{path}") } else { path.to_string() };
    let default = if tls { 443 } else { 80 };
    let (host, port) = if let Some(v6) = authority.strip_prefix('[') {
        let (h, after) = v6.split_once(']').ok_or_else(|| format!("{text:?}: an unclosed [ in the host"))?;
        let port = match after.strip_prefix(':') {
            Some(p) => p.parse().map_err(|_| format!("{text:?}: a bad port"))?,
            None if after.is_empty() => default,
            None => return Err(format!("{text:?}: a bad host")),
        };
        (h.to_string(), port)
    } else {
        match authority.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), p.parse().map_err(|_| format!("{text:?}: a bad port"))?),
            None => (authority.to_string(), default),
        }
    };
    if host.is_empty() || host.contains(['@', ' ']) || path.contains([' ', '\r', '\n']) {
        return Err(format!("{text:?}: a bad host or path"));
    }
    Ok(Url { tls, host, port, path })
}

/// An answer: its status and body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

/// How long each step may take.
#[derive(Clone, Copy, Debug)]
pub struct Timeouts {
    /// Each address's TCP connect.
    pub connect: Duration,
    /// The whole exchange after the connect (sending, the answer).
    pub exchange: Duration,
}

/// POSTs `body` (`content_type`) to `url` and reads the answer.
pub fn post(
    url: &Url,
    content_type: &str,
    body: &[u8],
    user_agent: &str,
    t: Timeouts,
) -> io::Result<Response> {
    let stream = connect(url, t.connect)?;
    let deadline = Instant::now() + t.exchange;
    stream.set_write_timeout(Some(t.exchange))?;
    let host = if url.port == if url.tls { 443 } else { 80 } {
        bracketed(&url.host)
    } else {
        format!("{}:{}", bracketed(&url.host), url.port)
    };
    let head = format!(
        "POST {} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: {user_agent}\r\nAccept: application/json\r\n\
         Content-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        url.path,
        body.len()
    );
    if url.tls {
        let config = tls_config().ok_or_else(|| io::Error::other("TLS could not be set up"))?;
        let name = rustls::pki_types::ServerName::try_from(url.host.clone())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        let conn = rustls::ClientConnection::new(config, name).map_err(io::Error::other)?;
        exchange(rustls::StreamOwned::new(conn, stream), &head, body, deadline)
    } else {
        exchange(stream, &head, body, deadline)
    }
}

fn bracketed(host: &str) -> String {
    if host.contains(':') { format!("[{host}]") } else { host.to_string() }
}

/// The first of the host's addresses that takes the connection.
fn connect(url: &Url, timeout: Duration) -> io::Result<TcpStream> {
    let mut last = io::Error::new(io::ErrorKind::NotFound, format!("{} has no address", url.host));
    for addr in (url.host.as_str(), url.port).to_socket_addrs()? {
        match TcpStream::connect_timeout(&addr, timeout) {
            Ok(s) => return Ok(s),
            Err(e) => last = e,
        }
    }
    Err(last)
}

fn tls_config() -> Option<Arc<rustls::ClientConfig>> {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config =
        rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::aws_lc_rs::default_provider()))
            .with_safe_default_protocol_versions()
            .ok()?
            .with_root_certificates(roots)
            .with_no_client_auth();
    Some(Arc::new(config))
}

/// The underlying socket of a stream, for its read timeout.
trait Socket: Read + Write {
    fn socket(&self) -> &TcpStream;
}

impl Socket for TcpStream {
    fn socket(&self) -> &TcpStream {
        self
    }
}

impl Socket for rustls::StreamOwned<rustls::ClientConnection, TcpStream> {
    fn socket(&self) -> &TcpStream {
        &self.sock
    }
}

fn exchange(mut s: impl Socket, head: &str, body: &[u8], deadline: Instant) -> io::Result<Response> {
    s.write_all(head.as_bytes())?;
    s.write_all(body)?;
    s.flush()?;
    let mut got = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "no whole answer in time"));
        }
        s.socket().set_read_timeout(Some(left))?;
        match s.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => got.extend_from_slice(&buf[..n]),
            // A server closing without TLS's close_notify: what came is all.
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "no whole answer in time"));
            }
            Err(e) => return Err(e),
        }
        if got.len() > MAX_RESPONSE {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "the answer is too long"));
        }
        if let Some(Ok(r)) = parse_response(&got, false) {
            return Ok(r);
        }
    }
    parse_response(&got, true).unwrap_or_else(|| Err(io::Error::new(io::ErrorKind::InvalidData, "no answer")))
}

/// The answer in `bytes` once it is whole (`None`: more is needed; at the
/// stream's `end`, a body without a length is what came).
pub fn parse_response(bytes: &[u8], end: bool) -> Option<io::Result<Response>> {
    let bad = |m: &str| Some(Err(io::Error::new(io::ErrorKind::InvalidData, m.to_string())));
    let Some(at) = bytes.windows(4).position(|w| w == b"\r\n\r\n") else {
        return if end { bad("no answer head") } else { None };
    };
    let head = String::from_utf8_lossy(&bytes[..at]);
    let rest = &bytes[at + 4..];
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or("");
    let mut parts = status_line.split(' ');
    let (Some(version), Some(code)) = (parts.next(), parts.next()) else { return bad("a bad status line") };
    let Some(status) = code.parse::<u16>().ok().filter(|_| version.starts_with("HTTP/1.")) else {
        return bad("a bad status line");
    };
    let mut length = None;
    let mut chunked = false;
    for l in lines {
        let Some((name, value)) = l.split_once(':') else { continue };
        let value = value.trim();
        if name.eq_ignore_ascii_case("content-length") {
            match value.parse::<usize>() {
                Ok(n) => length = Some(n),
                Err(_) => return bad("a bad Content-Length"),
            }
        } else if name.eq_ignore_ascii_case("transfer-encoding")
            && value.to_ascii_lowercase().contains("chunked")
        {
            chunked = true;
        }
    }
    if chunked {
        return match dechunk(rest) {
            Some(body) => Some(Ok(Response { status, body })),
            None if end => bad("a cut chunked answer"),
            None => None,
        };
    }
    match length {
        Some(n) if rest.len() >= n => Some(Ok(Response { status, body: rest[..n].to_vec() })),
        Some(_) if end => bad("a cut answer"),
        Some(_) => None,
        None if end => Some(Ok(Response { status, body: rest.to_vec() })),
        None => None,
    }
}

/// A chunked body's bytes, once its last chunk came.
fn dechunk(mut rest: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let eol = rest.windows(2).position(|w| w == b"\r\n")?;
        let size_text = String::from_utf8_lossy(&rest[..eol]);
        let size = usize::from_str_radix(size_text.split(';').next()?.trim(), 16).ok()?;
        rest = &rest[eol + 2..];
        if size == 0 {
            return Some(out);
        }
        if rest.len() < size + 2 {
            return None;
        }
        out.extend_from_slice(&rest[..size]);
        rest = &rest[size + 2..];
    }
}

/// How long a GET may wait: each address's connect, and any one read or
/// write (a slow download goes on while bytes come).
#[derive(Clone, Copy, Debug)]
pub struct GetTimeouts {
    pub connect: Duration,
    pub idle: Duration,
}

/// The most redirects a GET follows (GitHub's downloads take one).
pub const MAX_REDIRECTS: usize = 5;
/// The most bytes of an answer's head.
const MAX_HEAD: usize = 64 * 1024;

/// GETs `url` (`accept`: its `Accept` header) and writes the body into
/// `sink`, at most `max` bytes, calling `progress(received, total)` as it
/// comes (`total` when the server says it). Redirects are followed, at
/// most [`MAX_REDIRECTS`], never from `https://` to `http://`. A status
/// other than 2xx is an error saying it.
pub fn get(
    url: &str,
    accept: &str,
    user_agent: &str,
    max: u64,
    t: GetTimeouts,
    sink: &mut dyn Write,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> io::Result<()> {
    let mut url = parse_url(url).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    for _ in 0..=MAX_REDIRECTS {
        let stream = connect(&url, t.connect)?;
        stream.set_write_timeout(Some(t.idle))?;
        stream.set_read_timeout(Some(t.idle))?;
        let head = format!(
            "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: {user_agent}\r\nAccept: {accept}\r\n\
             Connection: close\r\n\r\n",
            url.path,
            host_header(&url)
        );
        let next = if url.tls {
            let config = tls_config().ok_or_else(|| io::Error::other("TLS could not be set up"))?;
            let name = rustls::pki_types::ServerName::try_from(url.host.clone())
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
            let conn = rustls::ClientConnection::new(config, name).map_err(io::Error::other)?;
            get_once(rustls::StreamOwned::new(conn, stream), &head, max, sink, progress)?
        } else {
            get_once(stream, &head, max, sink, progress)?
        };
        let Some(location) = next else { return Ok(()) };
        let to = if location.starts_with('/') {
            Url { path: location, ..url.clone() }
        } else {
            parse_url(&location).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?
        };
        if url.tls && !to.tls {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "a redirect from https to http"));
        }
        url = to;
    }
    Err(io::Error::new(io::ErrorKind::InvalidData, "too many redirects"))
}

fn host_header(url: &Url) -> String {
    if url.port == if url.tls { 443 } else { 80 } {
        bracketed(&url.host)
    } else {
        format!("{}:{}", bracketed(&url.host), url.port)
    }
}

/// One GET's exchange: `Some(location)` for a redirect, else the body
/// written.
fn get_once(
    mut s: impl Read + Write,
    head: &str,
    max: u64,
    sink: &mut dyn Write,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> io::Result<Option<String>> {
    s.write_all(head.as_bytes())?;
    s.flush()?;
    let mut r = BufReader::new(s);
    let mut lines = Vec::new();
    let mut size = 0;
    loop {
        let mut line = Vec::new();
        let n = read_line(&mut r, &mut line, MAX_HEAD - size)?;
        size += n;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "no answer"));
        }
        let text = String::from_utf8_lossy(&line).trim_end_matches(['\r', '\n']).to_string();
        if text.is_empty() {
            break;
        }
        lines.push(text);
    }
    let bad = |m: &str| io::Error::new(io::ErrorKind::InvalidData, m.to_string());
    let status_line = lines.first().ok_or_else(|| bad("no status line"))?;
    let mut parts = status_line.split(' ');
    let status = match (parts.next(), parts.next()) {
        (Some(v), Some(code)) if v.starts_with("HTTP/1.") => {
            code.parse::<u16>().map_err(|_| bad("a bad status"))?
        }
        _ => return Err(bad("a bad status line")),
    };
    let header = |name: &str| {
        lines[1..].iter().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim().eq_ignore_ascii_case(name).then(|| v.trim().to_string())
        })
    };
    if matches!(status, 301 | 302 | 303 | 307 | 308) {
        return header("location").map(Some).ok_or_else(|| bad("a redirect without a Location"));
    }
    if !(200..300).contains(&status) {
        return Err(io::Error::other(format!("the server answered {status}")));
    }
    let chunked = header("transfer-encoding").is_some_and(|v| v.to_ascii_lowercase().contains("chunked"));
    let length = match header("content-length") {
        Some(v) if !chunked => Some(v.parse::<u64>().map_err(|_| bad("a bad Content-Length"))?),
        _ => None,
    };
    if length.is_some_and(|n| n > max) {
        return Err(bad("the answer is too long"));
    }
    let mut body = Body { got: 0, max, length, buf: vec![0u8; 64 * 1024], sink, progress };
    if chunked {
        loop {
            let mut line = Vec::new();
            if read_line(&mut r, &mut line, 1024)? == 0 {
                return Err(bad("a cut chunked answer"));
            }
            let text = String::from_utf8_lossy(&line);
            let size_text = text.trim().split(';').next().unwrap_or("").trim().to_string();
            let mut left = u64::from_str_radix(&size_text, 16).map_err(|_| bad("a bad chunk size"))?;
            if left == 0 {
                return Ok(None);
            }
            while left > 0 {
                let n = body.take(&mut r, left)?;
                if n == 0 {
                    return Err(bad("a cut chunked answer"));
                }
                left -= n as u64;
            }
            let mut crlf = Vec::new();
            read_line(&mut r, &mut crlf, 4)?;
        }
    }
    match length {
        Some(n) => {
            while body.got < n {
                if body.take(&mut r, n - body.got)? == 0 {
                    return Err(bad("a cut answer"));
                }
            }
        }
        None => loop {
            match body.take(&mut r, u64::MAX) {
                Ok(0) => break,
                Ok(_) => {}
                // A server closing without TLS's close_notify: what came is all.
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(e),
            }
        },
    }
    Ok(None)
}

/// A GET's body going to its sink.
struct Body<'a> {
    got: u64,
    max: u64,
    length: Option<u64>,
    buf: Vec<u8>,
    sink: &'a mut dyn Write,
    progress: &'a mut dyn FnMut(u64, Option<u64>),
}

impl Body<'_> {
    /// Reads at most `want` bytes into the sink: how many (0 at the end).
    fn take(&mut self, r: &mut impl Read, want: u64) -> io::Result<usize> {
        let n = want.min(self.buf.len() as u64) as usize;
        let n = loop {
            match r.read(&mut self.buf[..n]) {
                Ok(n) => break n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {
                    return Err(io::Error::new(io::ErrorKind::TimedOut, "the download stalled"));
                }
                Err(e) => return Err(e),
            }
        };
        if self.got + n as u64 > self.max {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "the answer is too long"));
        }
        self.sink.write_all(&self.buf[..n])?;
        self.got += n as u64;
        (self.progress)(self.got, self.length);
        Ok(n)
    }
}

/// Reads one line (its `\n` included) into `out`, at most `max` bytes.
fn read_line(r: &mut impl BufRead, out: &mut Vec<u8>, max: usize) -> io::Result<usize> {
    let n = r.take(max as u64 + 1).read_until(b'\n', out)?;
    if n > max {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "an answer's line is too long"));
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        let u = parse_url("https://olr-bug-relay.example.workers.dev/report").unwrap();
        assert_eq!(
            u,
            Url {
                tls: true,
                host: "olr-bug-relay.example.workers.dev".into(),
                port: 443,
                path: "/report".into()
            }
        );
        let u = parse_url("http://127.0.0.1:8787").unwrap();
        assert_eq!((u.tls, u.host.as_str(), u.port, u.path.as_str()), (false, "127.0.0.1", 8787, "/"));
        let u = parse_url("http://[::1]:9/x?y=1").unwrap();
        assert_eq!((u.host.as_str(), u.port, u.path.as_str()), ("::1", 9, "/x?y=1"));
        for bad in ["ftp://x", "https://", "https://a:b/", "https://u@h/", "x"] {
            assert!(parse_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn answers() {
        let r =
            parse_response(b"HTTP/1.1 201 Created\r\nContent-Length: 2\r\n\r\nok", false).unwrap().unwrap();
        assert_eq!((r.status, r.body.as_slice()), (201, &b"ok"[..]));
        assert!(parse_response(b"HTTP/1.1 201 Created\r\nContent-Length: 5\r\n\r\nok", false).is_none());
        assert!(
            parse_response(b"HTTP/1.1 201 Created\r\nContent-Length: 5\r\n\r\nok", true).unwrap().is_err()
        );
        let chunked =
            b"HTTP/1.1 429 Too Many\r\ntransfer-encoding: chunked\r\n\r\n4\r\nabcd\r\n2;x\r\nef\r\n0\r\n\r\n";
        let r = parse_response(chunked, false).unwrap().unwrap();
        assert_eq!((r.status, r.body.as_slice()), (429, &b"abcdef"[..]));
        assert!(parse_response(&chunked[..60], false).is_none());
        let r = parse_response(b"HTTP/1.0 500 Oops\r\n\r\nbody", true).unwrap().unwrap();
        assert_eq!((r.status, r.body.as_slice()), (500, &b"body"[..]));
        assert!(parse_response(b"SSH-2.0\r\n\r\n", true).unwrap().is_err());
    }
}
