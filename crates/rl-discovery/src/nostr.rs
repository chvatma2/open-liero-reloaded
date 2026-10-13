//! The NIP-01 subset discovery needs (decision N38): events with their
//! canonical serialization and id, BIP-340 signatures through the install's
//! [`Identity`], the relay frames, and NIP-40 expiration. No Nostr library.

use rl_lobby::Identity;
use rl_proto::{PublicKey, Signature};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Announcements: addressable (parameterized replaceable), tag `d` =
/// `"rust-liero"` (ruling PL12-2).
pub const KIND_ANNOUNCE: u32 = 31470;
/// Ephemeral handshake events (plan 12b).
pub const KIND_HANDSHAKE: u32 = 21470;
/// The largest relay frame we decode, in bytes.
pub const MAX_FRAME: usize = 128 * 1024;
/// The largest content of an event of our own, in bytes.
pub const MAX_CONTENT: usize = 4 * 1024;

/// Why a frame or event did not decode.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum NostrError {
    #[error("frame larger than {MAX_FRAME} bytes")]
    TooLarge,
    #[error("not JSON")]
    Json,
    #[error("malformed {0}")]
    Malformed(&'static str),
    #[error("unknown message type")]
    Unknown,
}

/// A signed Nostr event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub id: [u8; 32],
    pub pubkey: PublicKey,
    pub created_at: u64,
    pub kind: u32,
    pub tags: Vec<Vec<String>>,
    pub content: String,
    pub sig: Signature,
}

fn push_escaped(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// NIP-01 canonical serialization,
/// `[0,"<pubkey hex>",created_at,kind,tags,"content"]`, escaping exactly
/// `\n \" \\ \r \t \b \f` and writing every other character verbatim.
pub fn canonical(
    pubkey: &PublicKey,
    created_at: u64,
    kind: u32,
    tags: &[Vec<String>],
    content: &str,
) -> String {
    let mut out = String::with_capacity(96 + content.len());
    out.push_str("[0,\"");
    out.push_str(&hex::encode(pubkey));
    out.push_str("\",");
    out.push_str(&created_at.to_string());
    out.push(',');
    out.push_str(&kind.to_string());
    out.push_str(",[");
    for (i, tag) in tags.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('[');
        for (j, v) in tag.iter().enumerate() {
            if j > 0 {
                out.push(',');
            }
            push_escaped(&mut out, v);
        }
        out.push(']');
    }
    out.push_str("],");
    push_escaped(&mut out, content);
    out.push(']');
    out
}

fn event_id(pubkey: &PublicKey, created_at: u64, kind: u32, tags: &[Vec<String>], content: &str) -> [u8; 32] {
    Sha256::digest(canonical(pubkey, created_at, kind, tags, content).as_bytes()).into()
}

impl Event {
    /// Builds and signs an event with `identity`; `aux` is the signature's
    /// 32 bytes of randomness.
    pub fn sign(
        identity: &Identity,
        created_at: u64,
        kind: u32,
        tags: Vec<Vec<String>>,
        content: String,
        aux: [u8; 32],
    ) -> Event {
        let pubkey = identity.public();
        let id = event_id(&pubkey, created_at, kind, &tags, &content);
        let sig = identity.sign(&id, aux);
        Event { id, pubkey, created_at, kind, tags, content, sig }
    }

    /// The id is the SHA-256 of the canonical form and the signature is a
    /// valid BIP-340 signature of the id by `pubkey`.
    pub fn verify(&self) -> bool {
        let id = event_id(&self.pubkey, self.created_at, self.kind, &self.tags, &self.content);
        id == self.id && rl_lobby::verify(&self.pubkey, &id, &self.sig)
    }

    /// The first value of the first tag named `name`.
    pub fn tag(&self, name: &str) -> Option<&str> {
        self.tags
            .iter()
            .find(|t| t.first().map(String::as_str) == Some(name))
            .and_then(|t| t.get(1))
            .map(String::as_str)
    }

    /// NIP-40: the `expiration` tag's unix time, if it is a number.
    pub fn expiration(&self) -> Option<u64> {
        self.tag("expiration")?.parse().ok()
    }

    fn to_value(&self) -> Value {
        json!({
            "id": hex::encode(self.id),
            "pubkey": hex::encode(self.pubkey),
            "created_at": self.created_at,
            "kind": self.kind,
            "tags": self.tags,
            "content": self.content,
            "sig": hex::encode(self.sig),
        })
    }

    fn from_value(v: &Value) -> Result<Event, NostrError> {
        let o = v.as_object().ok_or(NostrError::Malformed("event"))?;
        let created_at =
            o.get("created_at").and_then(Value::as_u64).ok_or(NostrError::Malformed("created_at"))?;
        let kind = o
            .get("kind")
            .and_then(Value::as_u64)
            .and_then(|k| u32::try_from(k).ok())
            .ok_or(NostrError::Malformed("kind"))?;
        let content =
            o.get("content").and_then(Value::as_str).ok_or(NostrError::Malformed("content"))?.to_owned();
        let mut tags = Vec::new();
        for t in o.get("tags").and_then(Value::as_array).ok_or(NostrError::Malformed("tags"))? {
            let t = t.as_array().ok_or(NostrError::Malformed("tag"))?;
            let mut tag = Vec::with_capacity(t.len());
            for s in t {
                tag.push(s.as_str().ok_or(NostrError::Malformed("tag"))?.to_owned());
            }
            tags.push(tag);
        }
        Ok(Event {
            id: hex_field(o, "id")?,
            pubkey: hex_field(o, "pubkey")?,
            created_at,
            kind,
            tags,
            content,
            sig: hex_field(o, "sig")?,
        })
    }
}

fn hex_val<const N: usize>(v: &Value, what: &'static str) -> Result<[u8; N], NostrError> {
    let s = v.as_str().ok_or(NostrError::Malformed(what))?;
    let mut out = [0u8; N];
    hex::decode_to_slice(s, &mut out).map_err(|_| NostrError::Malformed(what))?;
    Ok(out)
}

fn hex_field<const N: usize>(o: &Map<String, Value>, what: &'static str) -> Result<[u8; N], NostrError> {
    hex_val(o.get(what).ok_or(NostrError::Malformed(what))?, what)
}

/// A REQ filter (the subset we use).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Filter {
    pub kinds: Vec<u32>,
    pub authors: Vec<PublicKey>,
    /// `#<letter>` tag filters.
    pub tags: Vec<(char, Vec<String>)>,
    pub since: Option<u64>,
    pub limit: Option<u32>,
}

impl Filter {
    fn to_value(&self) -> Value {
        let mut o = Map::new();
        if !self.kinds.is_empty() {
            o.insert("kinds".into(), json!(self.kinds));
        }
        if !self.authors.is_empty() {
            let a: Vec<String> = self.authors.iter().map(hex::encode).collect();
            o.insert("authors".into(), json!(a));
        }
        for (c, vals) in &self.tags {
            o.insert(format!("#{c}"), json!(vals));
        }
        if let Some(s) = self.since {
            o.insert("since".into(), json!(s));
        }
        if let Some(l) = self.limit {
            o.insert("limit".into(), json!(l));
        }
        Value::Object(o)
    }

    fn from_value(v: &Value) -> Result<Filter, NostrError> {
        let o = v.as_object().ok_or(NostrError::Malformed("filter"))?;
        let mut f = Filter::default();
        for (k, v) in o {
            match k.as_str() {
                "kinds" => {
                    for k in v.as_array().ok_or(NostrError::Malformed("kinds"))? {
                        let k = k
                            .as_u64()
                            .and_then(|k| u32::try_from(k).ok())
                            .ok_or(NostrError::Malformed("kinds"))?;
                        f.kinds.push(k);
                    }
                }
                "authors" => {
                    for a in v.as_array().ok_or(NostrError::Malformed("authors"))? {
                        f.authors.push(hex_val(a, "authors")?);
                    }
                }
                "since" => f.since = Some(v.as_u64().ok_or(NostrError::Malformed("since"))?),
                "limit" => {
                    let l = v.as_u64().ok_or(NostrError::Malformed("limit"))?;
                    f.limit = Some(u32::try_from(l).unwrap_or(u32::MAX));
                }
                key => {
                    let mut chars = key.strip_prefix('#').into_iter().flat_map(str::chars);
                    if let (Some(c), None) = (chars.next(), chars.next()) {
                        let mut vals = Vec::new();
                        for s in v.as_array().ok_or(NostrError::Malformed("tag filter"))? {
                            vals.push(s.as_str().ok_or(NostrError::Malformed("tag filter"))?.to_owned());
                        }
                        f.tags.push((c, vals));
                    }
                    // Other keys (ids, until, search, ...) are ignored.
                }
            }
        }
        Ok(f)
    }
}

/// A message from a client to a relay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToRelay {
    Event(Event),
    Req { sub: String, filters: Vec<Filter> },
    Close { sub: String },
}

/// A message from a relay to a client.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FromRelay {
    Event { sub: String, event: Event },
    Eose { sub: String },
    Ok { id: [u8; 32], accepted: bool, message: String },
    Notice(String),
    Closed { sub: String, message: String },
}

pub fn encode_to_relay(m: &ToRelay) -> String {
    match m {
        ToRelay::Event(e) => json!(["EVENT", e.to_value()]),
        ToRelay::Req { sub, filters } => {
            let mut a = vec![json!("REQ"), json!(sub)];
            a.extend(filters.iter().map(Filter::to_value));
            Value::Array(a)
        }
        ToRelay::Close { sub } => json!(["CLOSE", sub]),
    }
    .to_string()
}

/// For the test relay.
pub fn encode_from_relay(m: &FromRelay) -> String {
    match m {
        FromRelay::Event { sub, event } => json!(["EVENT", sub, event.to_value()]),
        FromRelay::Eose { sub } => json!(["EOSE", sub]),
        FromRelay::Ok { id, accepted, message } => {
            json!(["OK", hex::encode(id), accepted, message])
        }
        FromRelay::Notice(s) => json!(["NOTICE", s]),
        FromRelay::Closed { sub, message } => json!(["CLOSED", sub, message]),
    }
    .to_string()
}

/// Parses a frame: at most [`MAX_FRAME`] bytes (serde_json's own recursion
/// limit bounds the nesting), a JSON array.
fn parse_frame(text: &str) -> Result<Vec<Value>, NostrError> {
    if text.len() > MAX_FRAME {
        return Err(NostrError::TooLarge);
    }
    match serde_json::from_str(text).map_err(|_| NostrError::Json)? {
        Value::Array(a) => Ok(a),
        _ => Err(NostrError::Malformed("frame")),
    }
}

fn string_at(a: &[Value], i: usize, what: &'static str) -> Result<String, NostrError> {
    Ok(a.get(i).and_then(Value::as_str).ok_or(NostrError::Malformed(what))?.to_owned())
}

pub fn decode_from_relay(text: &str) -> Result<FromRelay, NostrError> {
    let a = parse_frame(text)?;
    match a.first().and_then(Value::as_str) {
        Some("EVENT") => Ok(FromRelay::Event {
            sub: string_at(&a, 1, "subscription")?,
            event: Event::from_value(a.get(2).ok_or(NostrError::Malformed("event"))?)?,
        }),
        Some("EOSE") => Ok(FromRelay::Eose { sub: string_at(&a, 1, "subscription")? }),
        Some("OK") => Ok(FromRelay::Ok {
            id: hex_val(a.get(1).ok_or(NostrError::Malformed("id"))?, "id")?,
            accepted: a.get(2).and_then(Value::as_bool).ok_or(NostrError::Malformed("OK"))?,
            // The message is optional in the wild.
            message: string_at(&a, 3, "message").unwrap_or_default(),
        }),
        Some("NOTICE") => Ok(FromRelay::Notice(string_at(&a, 1, "notice")?)),
        Some("CLOSED") => Ok(FromRelay::Closed {
            sub: string_at(&a, 1, "subscription")?,
            message: string_at(&a, 2, "message").unwrap_or_default(),
        }),
        Some(_) => Err(NostrError::Unknown),
        None => Err(NostrError::Malformed("frame")),
    }
}

pub fn decode_to_relay(text: &str) -> Result<ToRelay, NostrError> {
    let a = parse_frame(text)?;
    match a.first().and_then(Value::as_str) {
        Some("EVENT") => {
            Ok(ToRelay::Event(Event::from_value(a.get(1).ok_or(NostrError::Malformed("event"))?)?))
        }
        Some("REQ") => {
            let sub = string_at(&a, 1, "subscription")?;
            let filters = a[2..].iter().map(Filter::from_value).collect::<Result<_, _>>()?;
            Ok(ToRelay::Req { sub, filters })
        }
        Some("CLOSE") => Ok(ToRelay::Close { sub: string_at(&a, 1, "subscription")? }),
        Some(_) => Err(NostrError::Unknown),
        None => Err(NostrError::Malformed("frame")),
    }
}
