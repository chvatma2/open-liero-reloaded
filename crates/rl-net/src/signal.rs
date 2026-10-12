//! The signalling connection (spec §5.1, decision N16): one WebSocket over a
//! non-blocking TCP stream, carrying `rl_proto::Signal`s, one per binary
//! frame. Never blocks: every call does what the socket allows now.

use std::io;
use std::net::{SocketAddr, TcpStream};

use tungstenite::handshake::client::ClientHandshake;
use tungstenite::handshake::server::{NoCallback, ServerHandshake};
use tungstenite::handshake::{HandshakeError, MidHandshake};
use tungstenite::protocol::WebSocketConfig;
use tungstenite::{Error, Message, WebSocket};

enum State {
    Server(MidHandshake<ServerHandshake<TcpStream, NoCallback>>),
    Client(MidHandshake<ClientHandshake<TcpStream>>),
    Open(WebSocket<TcpStream>),
    Closed,
}

/// One signalling connection, either end.
pub(crate) struct SignalConn {
    state: State,
    /// Frames to send once the handshake is done.
    outbox: Vec<Vec<u8>>,
}

/// At most this many messages are read in one `poll`, so a peer that
/// keeps sending cannot hold the caller's loop.
const MAX_READS: usize = 4;

/// Frames and messages above `max` bytes are refused by the reader, which
/// reads at most one such frame (and its 14-byte header) at a time.
fn config(max: usize) -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(Some(max))
        .max_frame_size(Some(max))
        .read_buffer_size(max + 14)
}

fn would_block(e: &Error) -> bool {
    matches!(e, Error::Io(e) if e.kind() == io::ErrorKind::WouldBlock)
}

impl SignalConn {
    /// The host's end of an accepted connection: runs the server handshake.
    pub(crate) fn accept(stream: TcpStream, max: usize) -> SignalConn {
        let state = match tungstenite::accept_with_config(stream, Some(config(max))) {
            Ok(ws) => State::Open(ws),
            Err(HandshakeError::Interrupted(mid)) => State::Server(mid),
            Err(HandshakeError::Failure(_)) => State::Closed,
        };
        SignalConn { state, outbox: Vec::new() }
    }

    /// The client's end of a connection to the host at `host`.
    pub(crate) fn connect(stream: TcpStream, host: SocketAddr, max: usize) -> SignalConn {
        let url = format!("ws://{host}/");
        let state = match tungstenite::client::client_with_config(url, stream, Some(config(max))) {
            Ok((ws, _)) => State::Open(ws),
            Err(HandshakeError::Interrupted(mid)) => State::Client(mid),
            Err(HandshakeError::Failure(_)) => State::Closed,
        };
        SignalConn { state, outbox: Vec::new() }
    }

    pub(crate) fn is_closed(&self) -> bool {
        matches!(self.state, State::Closed)
    }

    /// Sends one frame, or queues it until the handshake is done; it leaves
    /// as the socket allows.
    pub(crate) fn send(&mut self, frame: Vec<u8>) {
        if !self.is_closed() {
            self.outbox.push(frame);
            self.write();
        }
    }

    /// Advances the handshake, sends what is queued and returns the next
    /// received frame, if any, reading at most [`MAX_READS`] messages. A
    /// failure, a text frame, a ping or pong (our peers send none; tungstenite
    /// would answer every ping) or the peer's close closes the connection
    /// (`is_closed`); a frame above the limit too.
    pub(crate) fn poll(&mut self) -> Option<Vec<u8>> {
        self.advance_handshake();
        self.write();
        let State::Open(ws) = &mut self.state else { return None };
        for _ in 0..MAX_READS {
            match ws.read() {
                Ok(Message::Binary(bytes)) => return Some(bytes.into()),
                Ok(Message::Frame(_)) => {}
                Ok(Message::Ping(_) | Message::Pong(_) | Message::Text(_) | Message::Close(_)) => {
                    self.state = State::Closed;
                    return None;
                }
                Err(e) if would_block(&e) => return None,
                Err(_) => {
                    self.state = State::Closed;
                    return None;
                }
            }
        }
        None
    }

    /// Starts the WebSocket close after what is queued; the caller drops the
    /// connection once the peer answers (`is_closed`) or a deadline passes.
    pub(crate) fn close(&mut self) {
        self.write();
        if let State::Open(ws) = &mut self.state {
            let _ = ws.close(None);
            let _ = ws.flush();
        }
    }

    /// Writes the queued frames once the handshake is done, and flushes.
    fn write(&mut self) {
        let State::Open(ws) = &mut self.state else { return };
        let mut failed = false;
        for frame in self.outbox.drain(..) {
            match ws.write(Message::Binary(frame.into())) {
                Ok(()) => {}
                Err(e) if would_block(&e) => {}
                Err(_) => failed = true,
            }
        }
        match ws.flush() {
            Ok(()) => {}
            Err(e) if would_block(&e) => {}
            Err(_) => failed = true,
        }
        if failed {
            self.state = State::Closed;
        }
    }

    fn advance_handshake(&mut self) {
        self.state = match std::mem::replace(&mut self.state, State::Closed) {
            State::Server(mid) => match mid.handshake() {
                Ok(ws) => State::Open(ws),
                Err(HandshakeError::Interrupted(mid)) => State::Server(mid),
                Err(HandshakeError::Failure(_)) => State::Closed,
            },
            State::Client(mid) => match mid.handshake() {
                Ok((ws, _)) => State::Open(ws),
                Err(HandshakeError::Interrupted(mid)) => State::Client(mid),
                Err(HandshakeError::Failure(_)) => State::Closed,
            },
            other => other,
        };
    }
}
