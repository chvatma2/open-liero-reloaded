//! The subscription's usage limit, as Claude Code reports it
//! ("You've hit your session limit · resets 3:45pm").

/// When the limit lifts, in local time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reset {
    At {
        hour: u32,
        minute: u32,
    },
    /// A weekday form or no time: wait the fallback and try again.
    Unknown,
}

/// `Some` when `text` reports a hit usage limit.
pub fn parse(text: &str) -> Option<Reset> {
    let lower = text.to_ascii_lowercase();
    let hit = lower.find("hit your")?;
    lower[hit..].find("limit")?;
    let Some(at) = lower.find("resets ") else { return Some(Reset::Unknown) };
    let token = lower[at + 7..].split_whitespace().next().unwrap_or_default();
    Some(clock(token).map_or(Reset::Unknown, |(hour, minute)| Reset::At { hour, minute }))
}

/// `3:45pm`, `3pm`, `12am`, `15:45` as 24-hour `(hour, minute)`.
fn clock(token: &str) -> Option<(u32, u32)> {
    let (digits, meridiem) = match token.strip_suffix("am").or_else(|| token.strip_suffix("pm")) {
        Some(d) => (d, Some(token.ends_with("pm"))),
        None => (token, None),
    };
    let (h, m) = match digits.split_once(':') {
        Some((h, m)) => (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?),
        None => (digits.parse::<u32>().ok()?, 0),
    };
    let hour = match meridiem {
        Some(pm) if (1..=12).contains(&h) => h % 12 + if pm { 12 } else { 0 },
        Some(_) => return None,
        None if h < 24 => h,
        None => return None,
    };
    (m < 60).then_some((hour, m))
}

/// Seconds from `now` (h, m, s) to the next `at` (h, m), plus a minute of slack.
pub fn seconds_until(now: (u32, u32, u32), at: (u32, u32)) -> u64 {
    let now = u64::from(now.0 * 3600 + now.1 * 60 + now.2);
    let at = u64::from(at.0 * 3600 + at.1 * 60);
    let wait = if at > now { at - now } else { at + 86_400 - now };
    wait + 60
}
