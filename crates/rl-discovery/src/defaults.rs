//! The default relay and STUN lists (decisions N39, N40, N42), chosen by
//! measurement: `rl-cli nostr-probe` and `rl-cli stun-probe` on 2026-10-13
//! (plan 12a Task 8; the table is in `docs/porting/STATUS.md`, N38).

/// Default Nostr relays: each accepted kind 31470, served the note at once
/// and after 3 minutes, forwarded kind 21470 live and asked for no payment
/// or authentication; eight operators.
///
/// Append-only: an invite code names a relay by its index here
/// (`RelayHint::Default`), so codes made with a shorter list must keep
/// decoding. Never remove, reorder or replace an entry; a relay that dies
/// stays in place (its index then names a relay that does not answer).
pub const DEFAULT_RELAYS: &[&str] = &[
    "wss://relay.damus.io",
    "wss://nos.lol",
    "wss://relay.primal.net",
    "wss://relay.snort.social",
    "wss://offchain.pub",
    "wss://nostr.oxtr.dev",
    "wss://relay.nostr.net",
    "wss://nostr.bitcoiner.social",
];

/// Default STUN servers, `host:port`, the fastest first (`stun::public_ip`
/// takes the first answer); five operators, each answered over IPv4.
pub const DEFAULT_STUN: &[&str] = &[
    "stun.cloudflare.com:3478",
    "stun.l.google.com:19302",
    "global.stun.twilio.com:3478",
    "stun.nextcloud.com:443",
    "stun.sipgate.net:3478",
];

#[cfg(test)]
mod tests {
    use super::*;

    /// The list as first released. Invite codes carry indices into it, so
    /// a change may only append: extend `DEFAULT_RELAYS`, never this.
    const RELEASED: [&str; 8] = [
        "wss://relay.damus.io",
        "wss://nos.lol",
        "wss://relay.primal.net",
        "wss://relay.snort.social",
        "wss://offchain.pub",
        "wss://nostr.oxtr.dev",
        "wss://relay.nostr.net",
        "wss://nostr.bitcoiner.social",
    ];

    #[test]
    fn the_relay_list_only_grows() {
        assert!(DEFAULT_RELAYS.starts_with(&RELEASED), "DEFAULT_RELAYS is append-only");
        // An index is a `u8`.
        assert!(DEFAULT_RELAYS.len() <= 256);
    }

    #[test]
    fn the_lists_are_well_formed() {
        for (i, r) in DEFAULT_RELAYS.iter().enumerate() {
            assert!(r.starts_with("wss://"), "{r}");
            assert!(!DEFAULT_RELAYS[..i].contains(r), "{r} twice");
        }
        for (i, s) in DEFAULT_STUN.iter().enumerate() {
            let port = s.rsplit_once(':').and_then(|(_, p)| p.parse::<u16>().ok());
            assert!(port.is_some(), "{s}");
            assert!(!DEFAULT_STUN[..i].contains(s), "{s} twice");
        }
    }
}
