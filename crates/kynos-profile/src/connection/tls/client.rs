//! The client's half of a TLS row's conversation, which no instrument counts:
//! it reads what the server wrote and answers between the server's polls.

use std::io::{self, Read as _, Write as _};
use std::task::{Context, Poll, Waker};

use bytes::BytesMut;
use tokio::io::DuplexStream;
use tokio_rustls::rustls::{ClientConnection, HandshakeKind};

use super::super::{read, write};

/// The client's end of the duplex and its session.
pub(crate) struct Client {
    stream: DuplexStream,
    session: ClientConnection,
    /// What the client has decrypted since the last request it sent.
    pub(crate) received: Vec<u8>,
}

impl Client {
    /// A client over `stream` that has sent nothing yet.
    pub(crate) fn new(stream: DuplexStream, session: ClientConnection) -> Self {
        Self {
            stream,
            session,
            received: Vec::new(),
        }
    }

    /// How the session's handshake went, once it has.
    pub(crate) fn handshake(&self) -> Option<HandshakeKind> {
        self.session.handshake_kind()
    }

    /// Queues `octets` as application data and writes what the client can
    /// send into the duplex: before the handshake ends, only its first
    /// flight, and the octets with its `Finished`.
    pub(crate) fn send(&mut self, octets: &[u8]) {
        self.session
            .writer()
            .write_all(octets)
            .expect("the client buffers its request");
        self.flush();
    }

    /// The client's turn: decrypts what the server wrote, then writes what
    /// that let it send. `true` when it wrote anything.
    pub(crate) fn exchange(&mut self) -> bool {
        let mut octets = BytesMut::new();
        let mut context = Context::from_waker(Waker::noop());
        while let Poll::Ready(true) = read(&mut self.stream, &mut octets, &mut context) {}
        let mut unread = &octets[..];
        while !unread.is_empty() {
            self.session
                .read_tls(&mut unread)
                .expect("the client reads what the server wrote");
            self.session
                .process_new_packets()
                .expect("the server speaks TLS the client accepts");
        }
        match self.session.reader().read_to_end(&mut self.received) {
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
            Err(error) => panic!("the client could not read the response: {error}"),
        }
        self.flush()
    }

    /// Writes every record the client has ready into the duplex; `true` when
    /// there was one.
    fn flush(&mut self) -> bool {
        let mut octets = Vec::new();
        while self.session.wants_write() {
            self.session
                .write_tls(&mut octets)
                .expect("a Vec accepts every record");
        }
        write(&mut self.stream, &octets);
        !octets.is_empty()
    }
}
