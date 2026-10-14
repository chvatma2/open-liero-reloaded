//! UPnP-IGD (N51) through igd-next's sync API: the `rl-net upnp` thread.
//! igd-next handles the router quirks (chunked HTTP, control URLs, schemas);
//! its HTTP requests have no deadline of their own, so `PortMapper::poll`
//! gives up on this thread after `UPNP_DEADLINE` and a dropped mapper
//! detaches it.

use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::panic::AssertUnwindSafe;
use std::time::Duration;

use igd_next::{AddPortError, Gateway, PortMappingProtocol, SearchOptions};

use super::{Ctx, Grant, MapError, Method, Router, UPNP_LIFETIME, tidy, try_router};

/// The SSDP search's wait for an answer, and for the device's description.
const SEARCH: Duration = Duration::from_secs(2);

/// The `rl-net upnp` thread. igd-next's search subtracts durations that may
/// cross its deadline (a panic, in igd-next 0.17.1's `search_gateway`), so a
/// panic is caught and reported as a failure.
pub(crate) fn run(ctx: Ctx) {
    match std::panic::catch_unwind(AssertUnwindSafe(|| attempt(&ctx))) {
        Ok(Ok(())) => {}
        Ok(Err(why)) => ctx.fail(format!("UPnP: {why}")),
        Err(_) => ctx.fail("UPnP: the UPnP client failed".into()),
    }
}

fn attempt(ctx: &Ctx) -> Result<(), String> {
    let options = SearchOptions {
        broadcast_address: ctx.req.ssdp,
        timeout: Some(SEARCH),
        single_search_timeout: Some(SEARCH),
        ..Default::default()
    };
    let gateway = igd_next::search_gateway(options)
        .map_err(|e| format!("the search failed ({})", tidy(&e.to_string())))?;
    if !ctx.live() {
        return Ok(());
    }
    let external = match gateway.get_external_ip() {
        Ok(IpAddr::V4(ip)) if !ip.is_unspecified() => ip,
        Ok(_) => return Err("the router names no external IPv4 address".into()),
        Err(e) => return Err(format!("no external address ({})", tidy(&e.to_string()))),
    };
    let local = match ctx.req.local.ip() {
        ip if ip.is_unspecified() => {
            SocketAddrV4::new(route_to(gateway.addr).ok_or("no route to the router")?, ctx.req.local.port())
        }
        _ => ctx.req.local,
    };
    let mut router = Upnp {
        gateway,
        local,
        lease: ctx.req.lifetime_or(UPNP_LIFETIME),
        external,
        description: ctx.req.description,
    };
    try_router(&mut router, ctx)
}

/// The local address the system routes toward `to` from.
fn route_to(to: SocketAddr) -> Option<Ipv4Addr> {
    let sock = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    sock.connect(to).ok()?;
    match sock.local_addr().ok()? {
        SocketAddr::V4(a) if !a.ip().is_unspecified() => Some(*a.ip()),
        _ => None,
    }
}

struct Upnp {
    gateway: Gateway,
    local: SocketAddrV4,
    /// Seconds; 0 once the router said it takes permanent leases only.
    lease: u32,
    external: Ipv4Addr,
    description: &'static str,
}

fn protocol(udp: bool) -> PortMappingProtocol {
    if udp { PortMappingProtocol::UDP } else { PortMappingProtocol::TCP }
}

impl Router for Upnp {
    fn method(&self) -> Method {
        Method::Upnp
    }

    /// `AddPortMapping` at exactly `external`. A router answering 725
    /// (`OnlyPermanentLeasesSupported`) gets lease 0; 718 (taken) and 724
    /// (same ports required) try the next port.
    fn map(&mut self, udp: bool, external: u16, _ctx: &Ctx) -> Result<Grant, MapError> {
        loop {
            let local = SocketAddr::V4(self.local);
            match self.gateway.add_port(protocol(udp), external, local, self.lease, self.description) {
                Ok(()) => return Ok(Grant { port: external, lifetime: self.lease }),
                Err(AddPortError::OnlyPermanentLeasesSupported) if self.lease != 0 => self.lease = 0,
                Err(e @ (AddPortError::PortInUse | AddPortError::SamePortValuesRequired)) => {
                    return Err(MapError::Conflict(format!("port {external}: {}", tidy(&e.to_string()))));
                }
                Err(e) => return Err(MapError::Fatal(tidy(&e.to_string()))),
            }
        }
    }

    fn unmap(&mut self, udp: bool, external: u16) {
        let _ = self.gateway.remove_port(protocol(udp), external);
    }

    fn external_ip(&self) -> Option<Ipv4Addr> {
        Some(self.external)
    }
}
