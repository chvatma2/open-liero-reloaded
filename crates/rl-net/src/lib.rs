//! Real transport for online play (networking spec §2.1, §5.1): WebRTC data
//! channels over the sans-IO str0m stack, signalling over a WebSocket, LAN
//! announcements. Layer 5, std.

pub mod link;
