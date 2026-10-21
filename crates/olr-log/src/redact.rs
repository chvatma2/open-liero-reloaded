//! What the file never shows (issue #2, privacy): the home directory, and
//! at `info` and above network addresses and identity keys. Logs end up
//! in public bug reports; the terminal, the player's own, shows all.

use std::borrow::Cow;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::path::Path;

/// What stands for a hidden address.
pub const IP: &str = "<ip>";
/// What stands for a hidden key (64 hex digits, `npub1…`, `nsec1…`).
pub const KEY: &str = "<key>";
/// What stands for a hidden invite code (`olr1…`, an address inside).
pub const INVITE: &str = "<invite>";

/// The home directory's text, if it is worth replacing (not empty, not
/// the root).
pub fn home_text(home: Option<&Path>) -> Option<String> {
    let text = home?.to_str()?.trim_end_matches(['/', '\\']).to_string();
    (text.len() > 1).then_some(text)
}

/// `text` with `home` as `~` wherever it stands as a whole directory (not
/// `/home/ann` in `/home/anna`).
pub fn home<'a>(text: &'a str, home: Option<&str>) -> Cow<'a, str> {
    let Some(home) = home else { return Cow::Borrowed(text) };
    if !text.contains(home) {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(home) {
        let after = &rest[at + home.len()..];
        let name = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '-'));
        let mut next = after.chars();
        let whole = match next.next() {
            // `/home/ann.bak` goes on; `/home/ann.` ends a sentence.
            Some('.') => !name(next.next()),
            c => !name(c),
        };
        out.push_str(&rest[..at]);
        out.push_str(if whole { "~" } else { home });
        rest = after;
    }
    out.push_str(rest);
    Cow::Owned(out)
}

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// The length of the run of `ok` bytes at the start of `b`.
fn run(b: &[u8], ok: impl Fn(u8) -> bool) -> usize {
    b.iter().position(|&c| !ok(c)).unwrap_or(b.len())
}

/// What hides at the start of `b` (a token's first byte): its length and
/// what stands for it.
fn hidden(b: &[u8]) -> Option<(usize, &'static str)> {
    let ends_word = |n: usize| b.get(n).is_none_or(|&c| !is_word(c));
    // IPv4, perhaps with a port after it (kept); not a version
    // (`6.6.87.2-microsoft`).
    let n = run(b, |c| c.is_ascii_digit() || c == b'.');
    let v4 = b[..n].iter().rposition(|&c| c != b'.').map_or(0, |i| i + 1);
    if v4 > 0
        && ends_word(v4)
        && b.get(v4) != Some(&b'-')
        && std::str::from_utf8(&b[..v4]).is_ok_and(|s| s.parse::<Ipv4Addr>().is_ok())
    {
        return Some((v4, IP));
    }
    // IPv6: at least two colons and a digit (not `a::b`), trailing colons
    // and dots tried off.
    let n = run(b, |c| c.is_ascii_hexdigit() || c == b':' || c == b'.');
    let mut end = n;
    while end > 0 {
        let s = std::str::from_utf8(&b[..end]).unwrap_or("");
        let colons = s.bytes().filter(|&c| c == b':').count();
        let digit = s.bytes().any(|c| c.is_ascii_digit());
        if colons >= 2 && digit && ends_word(end) && s.parse::<Ipv6Addr>().is_ok() {
            return Some((end, IP));
        }
        if !matches!(b[end - 1], b':' | b'.') {
            break;
        }
        end -= 1;
    }
    // A key: 64 hex digits.
    let n = run(b, |c| c.is_ascii_hexdigit());
    if n == 64 && ends_word(n) {
        return Some((n, KEY));
    }
    // Bech32, in either case: Nostr keys and invite codes.
    let n = run(b, |c| c.is_ascii_alphanumeric());
    if n >= 20 && ends_word(n) {
        let word = b[..n].to_ascii_lowercase();
        for (prefix, what) in [("npub1", KEY), ("nsec1", KEY), ("nprofile1", KEY), ("olr1", INVITE)] {
            if word.starts_with(prefix.as_bytes()) {
                return Some((n, what));
            }
        }
    }
    None
}

/// `text` with network addresses, identity keys and invite codes hidden.
/// A token starts where the byte before is no word byte nor a dot (`:` is
/// one: `turn:1.2.3.4`), so module paths and versions stay.
pub fn network(text: &str) -> Cow<'_, str> {
    let b = text.as_bytes();
    let mut out: Option<String> = None;
    let (mut i, mut copied) = (0, 0);
    while i < b.len() {
        let starts = i == 0 || !(is_word(b[i - 1]) || b[i - 1] == b'.');
        if starts
            && (b[i].is_ascii_alphanumeric() || b[i] == b':')
            && let Some((n, with)) = hidden(&b[i..])
        {
            let o = out.get_or_insert_with(|| String::with_capacity(text.len()));
            o.push_str(&text[copied..i]);
            o.push_str(with);
            i += n;
            copied = i;
            continue;
        }
        i += 1;
    }
    match out {
        Some(mut o) => {
            o.push_str(&text[copied..]);
            Cow::Owned(o)
        }
        None => Cow::Borrowed(text),
    }
}
