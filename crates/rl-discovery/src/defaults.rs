//! The default relay and STUN lists (decisions N39, N40, N42).
//!
//! PLACEHOLDERS: plan 12a Task 8 replaces both with the measured choice.

/// Default Nostr relays. Append-only: an invite code names a relay by its
/// index here (`RelayHint::Default`), so codes made with a shorter list must
/// keep decoding.
pub const DEFAULT_RELAYS: &[&str] = &[
    "wss://relay.damus.io",
    "wss://nos.lol",
    "wss://relay.primal.net",
    "wss://relay.nostr.band",
    "wss://nostr.mom",
    "wss://relay.snort.social",
    "wss://offchain.pub",
    "wss://nostr.oxtr.dev",
];

/// Default STUN servers, `host:port`.
pub const DEFAULT_STUN: &[&str] = &[
    "stun.l.google.com:19302",
    "stun.cloudflare.com:3478",
    "global.stun.twilio.com:3478",
    "stun.nextcloud.com:443",
    "stun.stunprotocol.org:3478",
];
