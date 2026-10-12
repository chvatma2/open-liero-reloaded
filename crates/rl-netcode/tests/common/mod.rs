//! A host and one client over a link the test controls, in 1 ms steps:
//! what the harness's network model cannot do (hold or drop one direction,
//! see each datagram's answer).
#![allow(dead_code)]

use rl_netcode::harness::Factory;
use rl_netcode::toy::ToyFactory;
use rl_netcode::{Channel, Config, Driver, Event, PeerId, Role, Seat, Session, ToySim, input_delay};
use rl_proto::{Control, decode_control};

pub const HOST: PeerId = 0;
pub const CLIENT: PeerId = 1;

/// What the link does with a message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fate {
    Pass,
    Drop,
    /// Kept until the direction stops holding, then delivered in order.
    Hold,
}

/// Decides a message's fate from the time (ms), its channel and bytes.
pub type Link = Box<dyn FnMut(u64, Channel, &[u8]) -> Fate>;

/// The host (every slot but the client's is its own) and a client playing
/// `slot`, `one_way_ms` apart.
pub struct Pair {
    pub host: Session<ToySim>,
    pub client: Session<ToySim>,
    hd: Box<dyn Driver<ToySim>>,
    cd: Box<dyn Driver<ToySim>>,
    pub one_way_ms: u64,
    /// The time of the last step, in ms.
    pub now_ms: u64,
    /// (arrival ms, to, channel, bytes), in sending order.
    wire: Vec<(u64, PeerId, Channel, Vec<u8>)>,
    held: Vec<(PeerId, Channel, Vec<u8>)>,
    /// Client to host.
    pub up: Link,
    /// Host to client.
    pub down: Link,
    /// Per datagram the host received: the `Late` answers it made.
    pub lates_per_datagram: Vec<usize>,
    pub host_events: Vec<(u64, Event)>,
    pub client_events: Vec<(u64, Event)>,
    /// Messages the client sent, with the time.
    pub client_sent: Vec<(u64, Channel, Vec<u8>)>,
}

impl Pair {
    pub fn new(slots: u8, slot: u8, one_way_ms: u64, seed: u64) -> Pair {
        let role = Role::Host { peers: vec![(CLIENT, Seat::Player(slot))] };
        let host = Session::new(Config::new(slots, 0), role, ToySim::new(slots as usize), 0);
        let delay = input_delay(2 * one_way_ms as u32);
        let client = Session::new(
            Config::new(slots, delay),
            Role::Client { host: HOST, slot },
            ToySim::new(slots as usize),
            0,
        );
        let mut f = ToyFactory { slots, seed };
        Pair {
            host,
            client,
            hd: f.driver(HOST, None),
            cd: f.driver(CLIENT, Some(Seat::Player(slot))),
            one_way_ms,
            now_ms: 0,
            wire: Vec::new(),
            held: Vec::new(),
            up: Box::new(|_, _, _| Fate::Pass),
            down: Box::new(|_, _, _| Fate::Pass),
            lates_per_datagram: Vec::new(),
            host_events: Vec::new(),
            client_events: Vec::new(),
            client_sent: Vec::new(),
        }
    }

    /// Delivers what is due, then advances and transmits the host and the client.
    pub fn step(&mut self) {
        self.now_ms += 1;
        let now = self.now_ms;
        let us = now * 1000;
        let (due, later): (Vec<_>, Vec<_>) =
            std::mem::take(&mut self.wire).into_iter().partition(|m| m.0 <= now);
        self.wire = later;
        for (_, to, ch, bytes) in due {
            if to == HOST {
                self.host.receive(CLIENT, ch, &bytes, us);
                // What the host answered this datagram with.
                let mut lates = 0;
                let mut out = Vec::new();
                self.host.transmit(&mut |to, ch, b| out.push((to, ch, b)));
                for (to, ch, b) in out {
                    if ch == Channel::Reliable && matches!(decode_control(&b), Ok(Control::Late { .. })) {
                        lates += 1;
                    }
                    self.send(to, ch, b);
                }
                if ch == Channel::Unreliable {
                    self.lates_per_datagram.push(lates);
                }
            } else {
                self.client.receive(HOST, ch, &bytes, us);
            }
        }
        // Held messages go out once their direction passes again.
        let held = std::mem::take(&mut self.held);
        for (to, ch, b) in held {
            self.send(to, ch, b);
        }
        self.host.advance(us, self.hd.as_mut());
        let mut out = Vec::new();
        self.host.transmit(&mut |to, ch, b| out.push((to, ch, b)));
        for (to, ch, b) in out {
            self.send(to, ch, b);
        }
        self.client.advance(us, self.cd.as_mut());
        self.transmit_client();
        self.host_events.extend(self.host.drain_events().into_iter().map(|e| (now, e)));
        self.client_events.extend(self.client.drain_events().into_iter().map(|e| (now, e)));
    }

    /// Sends what the client queued (after [`Session::leave`], say).
    pub fn transmit_client(&mut self) {
        let mut out = Vec::new();
        self.client.transmit(&mut |to, ch, b| out.push((to, ch, b)));
        for (to, ch, b) in out {
            self.client_sent.push((self.now_ms, ch, b.clone()));
            self.send(to, ch, b);
        }
    }

    fn send(&mut self, to: PeerId, ch: Channel, bytes: Vec<u8>) {
        let fate = if to == HOST {
            (self.up)(self.now_ms, ch, &bytes)
        } else {
            (self.down)(self.now_ms, ch, &bytes)
        };
        // The reliable channel keeps its order: nothing passes a held message.
        let blocked =
            ch == Channel::Reliable && self.held.iter().any(|h| h.0 == to && h.1 == Channel::Reliable);
        match fate {
            Fate::Drop => {}
            Fate::Hold => self.held.push((to, ch, bytes)),
            Fate::Pass if blocked => self.held.push((to, ch, bytes)),
            Fate::Pass => self.wire.push((self.now_ms + self.one_way_ms, to, ch, bytes)),
        }
    }

    /// Plants a desync in the client (`Session::change_confirmed`).
    pub fn corrupt_client(&mut self) {
        self.client.change_confirmed(self.cd.as_mut(), |s| s.displace(0, 1));
    }

    pub fn run_ms(&mut self, ms: u64) {
        for _ in 0..ms {
            self.step();
        }
    }
}
