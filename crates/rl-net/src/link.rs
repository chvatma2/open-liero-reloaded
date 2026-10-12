//! One WebRTC peer connection (networking spec §2.1, decision N15): the
//! game's two data channels over the sans-IO str0m stack.
//!
//! A [`Link`] owns one `str0m::Rtc`. It never touches a socket or a clock:
//! the caller feeds it received datagrams and the time, and takes its
//! datagrams to send ([`Transmit`]), its events and its next timeout from
//! [`Link::poll`]. str0m's contract — every mutation is followed by draining
//! `poll_output` until it returns a timeout, before the next mutation — is
//! kept inside: every method that changes the `Rtc` drains it into the
//! link's own buffers, which `poll` hands out.
//!
//! Both channels are negotiated out of band (decision N17): [`INPUTS`]
//! (stream id 0, unordered, a 1 ms lifetime: in effect no retransmits) carries [`Channel::Unreliable`],
//! [`CONTROL`] (stream id 1, reliable, ordered) carries
//! [`Channel::Reliable`]. Neither side waits for in-band channel setup.

use std::collections::VecDeque;
use std::fmt;
use std::net::SocketAddr;
use std::time::Instant;

use rl_netcode::Channel;
use str0m::change::{SdpAnswer, SdpOffer, SdpPendingOffer};
use str0m::channel::{ChannelConfig, ChannelId, Reliability};
use str0m::net::{Protocol, Receive};
use str0m::{Candidate, Event, IceConnectionState, Input, Output, Rtc, RtcConfig};

/// The negotiated SCTP stream id of the inputs channel (decision N17).
pub const INPUTS: u16 = 0;
/// The negotiated SCTP stream id of the control channel (decision N17).
pub const CONTROL: u16 = 1;
/// The largest message [`Link::send`] takes: the remote's maximum message
/// size str0m assumes when the remote advertises none (64 KiB).
pub const MAX_MESSAGE: usize = 64 * 1024;

/// str0m 0.24.0's SCTP send buffer, shared by all streams
/// (`MAX_BUFFERED_ACROSS_STREAMS`, `src/sctp/mod.rs:30`). A write that does
/// not fit is refused, so the link queues what does not fit yet.
const SCTP_BUFFER: usize = 128 * 1024;
/// Room in str0m's buffer the control channel leaves free for inputs, so a
/// big reliable transfer never holds inputs back.
const INPUTS_ROOM: usize = 16 * 1024;
/// How often one `poll` feeds the time again while str0m asks for immediate
/// attention (a timeout not after `now`, e.g. opening channels).
const MAX_SPINS: usize = 16;

/// Which end of the offer/answer exchange a link is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Offerer,
    Answerer,
}

/// What a link reports from [`Link::poll`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkEvent {
    /// Both channels are open: [`Link::send`] works from now on.
    Open,
    /// A message from the peer.
    Message { channel: Channel, bytes: Vec<u8> },
    /// The connection is gone (reported once; never after [`Link::close`]).
    Closed { why: String },
}

/// One datagram to send from the local address `from` to `to`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transmit {
    pub from: SocketAddr,
    pub to: SocketAddr,
    pub bytes: Vec<u8>,
}

/// Why a link refused a call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkError {
    /// The offer or answer is malformed or does not fit this link.
    Sdp(String),
    /// The channels are not open yet.
    NotOpen,
    /// The message is larger than [`MAX_MESSAGE`].
    TooLarge,
    /// The link is closed.
    Closed,
}

impl fmt::Display for LinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LinkError::Sdp(why) => write!(f, "bad session description: {why}"),
            LinkError::NotOpen => f.write_str("the link is not open yet"),
            LinkError::TooLarge => write!(f, "message larger than {MAX_MESSAGE} bytes"),
            LinkError::Closed => f.write_str("the link is closed"),
        }
    }
}

impl std::error::Error for LinkError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Connecting,
    Open,
    Closed,
}

/// The index of a channel in the link's per-channel arrays (its stream id).
fn index(channel: Channel) -> usize {
    match channel {
        Channel::Unreliable => INPUTS as usize,
        Channel::Reliable => CONTROL as usize,
    }
}

const CHANNELS: [Channel; 2] = [Channel::Unreliable, Channel::Reliable];

/// The negotiated configuration of a channel; both sides use the same.
fn config(channel: Channel) -> ChannelConfig {
    match channel {
        Channel::Unreliable => ChannelConfig {
            label: "inputs".into(),
            ordered: false,
            // In effect no retransmits: a chunk is abandoned at any
            // retransmission (the RTO is far above 1 ms). Not
            // `MaxRetransmits { retransmits: 0 }`: sctp-proto 0.10.5 tests
            // `nsent >= retransmits` as it first sends a chunk
            // (`association/mod.rs:4003`), so with 0 (or 1) every chunk is
            // abandoned in flight, every SACK triggers a FORWARD-TSN and each
            // FORWARD-TSN an immediate SACK: with irregular polling a storm of
            // ~45 datagrams per message that overflows socket buffers and
            // loses inputs (`tests/link.rs`,
            // `irregular_polling_does_not_multiply_the_inputs_traffic`).
            reliability: Reliability::MaxPacketLifetime { lifetime: 1 },
            negotiated: Some(INPUTS),
            protocol: String::new(),
        },
        Channel::Reliable => ChannelConfig {
            label: "control".into(),
            ordered: true,
            reliability: Reliability::Reliable,
            negotiated: Some(CONTROL),
            protocol: String::new(),
        },
    }
}

fn sdp_error(e: impl fmt::Display) -> LinkError {
    LinkError::Sdp(e.to_string())
}

/// One peer connection, sans-IO.
pub struct Link {
    rtc: Rtc,
    side: Side,
    state: State,
    /// The offer awaiting its answer (offerer only).
    pending: Option<SdpPendingOffer>,
    /// str0m's id of each channel, by [`index`].
    ids: [Option<ChannelId>; 2],
    opened: [bool; 2],
    /// Messages waiting for room in str0m's send buffer, and their bytes.
    queues: [VecDeque<Vec<u8>>; 2],
    queued: [usize; 2],
    /// str0m's buffered amount of each channel after the last mutation.
    in_sctp: [usize; 2],
    out: Vec<Transmit>,
    events: Vec<LinkEvent>,
    timeout: Instant,
    now: Instant,
}

impl Link {
    fn new(side: Side, local: &[SocketAddr], now: Instant) -> Link {
        let mut link = Link {
            rtc: RtcConfig::new().build(now),
            side,
            state: State::Connecting,
            pending: None,
            ids: [None; 2],
            opened: [false; 2],
            queues: [VecDeque::new(), VecDeque::new()],
            queued: [0; 2],
            in_sctp: [0; 2],
            out: Vec::new(),
            events: Vec::new(),
            timeout: now,
            now,
        };
        for &addr in local {
            // An address no candidate can have (unspecified, multicast) is skipped.
            if let Ok(candidate) = Candidate::host(addr, "udp") {
                link.rtc.add_local_candidate(candidate);
                link.drain();
            }
        }
        link
    }

    /// A link that offers the connection from the local UDP addresses
    /// `local` (its host candidates), and the SDP offer to send the peer.
    pub fn offerer(local: &[SocketAddr], now: Instant) -> (Link, String) {
        let mut link = Link::new(Side::Offerer, local, now);
        let mut api = link.rtc.sdp_api();
        let inputs = api.add_channel_with_config(config(Channel::Unreliable));
        let control = api.add_channel_with_config(config(Channel::Reliable));
        let (offer, pending) = api.apply().expect("the first channels need a negotiation");
        link.ids = [Some(inputs), Some(control)];
        link.pending = Some(pending);
        link.drain();
        (link, offer.to_sdp_string())
    }

    /// A link that answers the peer's SDP `offer` from the local UDP
    /// addresses `local`, and the SDP answer to send back.
    pub fn answerer(local: &[SocketAddr], offer: &str, now: Instant) -> Result<(Link, String), LinkError> {
        let offer = SdpOffer::from_sdp_string(offer).map_err(sdp_error)?;
        let mut link = Link::new(Side::Answerer, local, now);
        let answer = link.rtc.sdp_api().accept_offer(offer).map_err(sdp_error)?;
        link.drain();
        // Negotiated channels are not in the SDP: the answerer adds the same ones.
        for channel in CHANNELS {
            link.ids[index(channel)] = Some(link.rtc.direct_api().create_data_channel(config(channel)));
            link.drain();
        }
        Ok((link, answer.to_sdp_string()))
    }

    /// Takes the peer's SDP answer to this offerer's offer.
    pub fn accept_answer(&mut self, answer: &str) -> Result<(), LinkError> {
        let answer = SdpAnswer::from_sdp_string(answer).map_err(sdp_error)?;
        let pending =
            self.pending.take().ok_or_else(|| LinkError::Sdp("no offer awaits an answer".into()))?;
        let accepted = self.rtc.sdp_api().accept_answer(pending, answer).map_err(sdp_error);
        self.drain();
        accepted
    }

    /// Which end of the offer/answer exchange this link is.
    pub fn side(&self) -> Side {
        self.side
    }

    /// Whether the datagram `bytes` from `from` to the local `to` belongs to
    /// this link (to share one socket between links).
    pub fn accepts(&self, from: SocketAddr, to: SocketAddr, bytes: &[u8]) -> bool {
        Receive::new(Protocol::Udp, from, to, bytes)
            .is_ok_and(|r| self.rtc.accepts(&Input::Receive(self.now, r)))
    }

    /// Takes a datagram received at `now`. A datagram that is not WebRTC
    /// (or is malformed) is ignored.
    pub fn receive(&mut self, from: SocketAddr, to: SocketAddr, bytes: &[u8], now: Instant) {
        if self.state == State::Closed {
            return;
        }
        let Ok(datagram) = Receive::new(Protocol::Udp, from, to, bytes) else {
            return;
        };
        self.now = self.now.max(now);
        let handled = self.rtc.handle_input(Input::Receive(self.now, datagram));
        self.drain();
        if let Err(e) = handled
            && !self.rtc.is_alive()
        {
            self.fail(e.to_string());
        }
        self.refresh_buffered();
    }

    /// Queues `bytes` as one message on `channel`. Messages leave in order
    /// as str0m's send buffer has room; [`Link::buffered`] tells how much waits.
    pub fn send(&mut self, channel: Channel, bytes: &[u8]) -> Result<(), LinkError> {
        match self.state {
            State::Closed => return Err(LinkError::Closed),
            State::Connecting => return Err(LinkError::NotOpen),
            State::Open => {}
        }
        if bytes.len() > MAX_MESSAGE {
            return Err(LinkError::TooLarge);
        }
        let i = index(channel);
        self.queues[i].push_back(bytes.to_vec());
        self.queued[i] += bytes.len();
        self.flush();
        Ok(())
    }

    /// The bytes sent on `channel` and not yet acknowledged (reliable) or
    /// sent (unreliable): the link's queue plus str0m's buffer.
    pub fn buffered(&self, channel: Channel) -> usize {
        let i = index(channel);
        self.queued[i] + self.in_sctp[i]
    }

    /// Advances the link to `now`: appends the datagrams to send to `out`
    /// and the events to `events`, and returns when to poll again at the
    /// latest (a datagram's arrival or a `send` is reason to poll sooner).
    pub fn poll(&mut self, now: Instant, out: &mut Vec<Transmit>, events: &mut Vec<LinkEvent>) -> Instant {
        self.now = self.now.max(now);
        for _ in 0..MAX_SPINS {
            if let Err(e) = self.rtc.handle_input(Input::Timeout(self.now)) {
                self.drain();
                self.fail(e.to_string());
                break;
            }
            self.drain();
            if self.timeout > self.now {
                break;
            }
        }
        self.flush();
        out.append(&mut self.out);
        events.append(&mut self.events);
        self.timeout
    }

    /// Closes the connection: the peer is told (the datagrams leave with the
    /// next `poll`); no events follow.
    pub fn close(&mut self) {
        if self.state == State::Closed {
            return;
        }
        self.state = State::Closed;
        let _ = self.rtc.close();
        self.drain();
    }

    /// Whether both channels are open and the link is not closed.
    pub fn is_open(&self) -> bool {
        self.state == State::Open
    }

    /// Drains str0m's output into the link's buffers (str0m's contract:
    /// after every mutation, until it returns a timeout).
    fn drain(&mut self) {
        loop {
            match self.rtc.poll_output() {
                Ok(Output::Timeout(t)) => {
                    self.timeout = t;
                    return;
                }
                Ok(Output::Transmit(t)) => {
                    self.out.push(Transmit { from: t.source, to: t.destination, bytes: t.contents.into() })
                }
                Ok(Output::Event(e)) => self.on_event(e),
                Err(e) => {
                    self.fail(e.to_string());
                    self.timeout = self.now;
                    return;
                }
            }
        }
    }

    fn channel_of(&self, id: ChannelId) -> Option<Channel> {
        CHANNELS.into_iter().find(|&c| self.ids[index(c)] == Some(id))
    }

    fn on_event(&mut self, event: Event) {
        if self.state == State::Closed {
            return;
        }
        match event {
            Event::ChannelOpen(id, _) => {
                if let Some(channel) = self.channel_of(id) {
                    self.opened[index(channel)] = true;
                }
                if self.opened == [true; 2] && self.state == State::Connecting {
                    self.state = State::Open;
                    self.events.push(LinkEvent::Open);
                }
            }
            Event::ChannelData(data) => {
                if let Some(channel) = self.channel_of(data.id) {
                    self.events.push(LinkEvent::Message { channel, bytes: data.data });
                }
            }
            Event::ChannelClose(id) if self.channel_of(id).is_some() => self.fail("a channel closed".into()),
            Event::IceConnectionStateChange(IceConnectionState::Disconnected) => {
                self.fail("the connection was lost".into())
            }
            Event::Closed => self.fail("the peer closed the connection".into()),
            _ => {}
        }
    }

    /// Ends the link, reporting `why` once.
    fn fail(&mut self, why: String) {
        if self.state == State::Closed {
            return;
        }
        self.state = State::Closed;
        self.events.push(LinkEvent::Closed { why });
        self.rtc.disconnect();
    }

    /// Moves queued messages into str0m while its buffer has room: inputs
    /// first, control messages leaving [`INPUTS_ROOM`] free.
    fn flush(&mut self) {
        for channel in CHANNELS {
            let i = index(channel);
            let room = match channel {
                Channel::Unreliable => SCTP_BUFFER,
                Channel::Reliable => SCTP_BUFFER - INPUTS_ROOM,
            };
            while self.state == State::Open {
                let Some(len) = self.queues[i].front().map(Vec::len) else { break };
                self.refresh_buffered();
                if self.in_sctp[0] + self.in_sctp[1] + len > room {
                    break;
                }
                let Some(id) = self.ids[i] else { break };
                let Some(mut writer) = self.rtc.channel(id) else { break };
                match writer.write(true, &self.queues[i][0]) {
                    Ok(true) => {
                        self.queues[i].pop_front();
                        self.queued[i] -= len;
                        self.drain();
                    }
                    Ok(false) => break,
                    Err(e) => {
                        self.drain();
                        self.fail(e.to_string());
                    }
                }
            }
        }
        self.refresh_buffered();
    }

    fn refresh_buffered(&mut self) {
        for channel in CHANNELS {
            let i = index(channel);
            self.in_sctp[i] =
                self.ids[i].and_then(|id| self.rtc.channel(id)).map_or(0, |mut c| c.buffered_amount());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_channels_are_negotiated_with_the_agreed_ids_and_reliability() {
        let inputs = config(Channel::Unreliable);
        assert_eq!((inputs.negotiated, inputs.ordered), (Some(INPUTS), false));
        assert_eq!(inputs.reliability, Reliability::MaxPacketLifetime { lifetime: 1 });
        let control = config(Channel::Reliable);
        assert_eq!((control.negotiated, control.ordered), (Some(CONTROL), true));
        assert_eq!(control.reliability, Reliability::Reliable);
    }
}
