//! Telling public addresses from the rest (decision N57): game lists,
//! router mappings and STUN answers judge addresses alike.

use core::net::IpAddr;

/// Whether `a` is a public address (no `is_global` on stable Rust): not
/// unspecified, loopback, private, shared, link-local, documentation,
/// benchmarking, multicast, reserved or broadcast. A v6 address that wraps a
/// v4 one (mapped or compatible) is judged as the v4 address.
pub fn is_global(a: IpAddr) -> bool {
    match a {
        IpAddr::V4(a) => {
            let [a0, a1, a2, _] = a.octets();
            !(a0 == 0
                || a0 == 10
                || (a0 == 100 && (64..128).contains(&a1))
                || a0 == 127
                || (a0 == 169 && a1 == 254)
                || (a0 == 172 && (16..32).contains(&a1))
                || (a0 == 192 && a1 == 0 && (a2 == 0 || a2 == 2))
                || (a0 == 192 && a1 == 168)
                || (a0 == 198 && (a1 == 18 || a1 == 19))
                || (a0 == 198 && a1 == 51 && a2 == 100)
                || (a0 == 203 && a1 == 0 && a2 == 113)
                || a0 >= 224)
        }
        IpAddr::V6(a) => {
            if let Some(v4) = a.to_ipv4() {
                return is_global(IpAddr::V4(v4));
            }
            let s = a.segments();
            !(a.is_unspecified()
                || a.is_loopback()
                || (s[0] & 0xfe00) == 0xfc00
                || (s[0] & 0xffc0) == 0xfe80
                || (s[0] == 0x2001 && s[1] == 0x0db8)
                || (s[0] & 0xff00) == 0xff00)
        }
    }
}
