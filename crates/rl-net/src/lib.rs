//! Real transport for online play (networking spec §2.1, §5.1): WebRTC data
//! channels over the sans-IO str0m stack, signalling over a WebSocket, LAN
//! announcements. Layer 5, std.

pub mod candidates;
pub mod core;
pub mod endpoint;
mod impair;
pub mod lan;
pub mod limits;
pub mod link;
#[cfg(feature = "test-hooks")]
pub mod natsim;
pub mod probe;
mod signal;

/// `N` bytes from the operating system's random source (identity keys,
/// nonces): the native peers' randomness, which the `no_std` crates take
/// as an argument.
pub fn random_bytes<const N: usize>() -> std::io::Result<[u8; N]> {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).map_err(std::io::Error::other)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    #[test]
    fn random_bytes_differ() {
        let (a, b) = (super::random_bytes::<32>().unwrap(), super::random_bytes::<32>().unwrap());
        assert_ne!(a, b);
    }
}
