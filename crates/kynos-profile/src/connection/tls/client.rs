//! The client's half of a TLS row's conversation, which no instrument counts:
//! it reads what the server wrote and answers between the server's rounds.

use std::io::{self, Read as _, Write as _};

use tokio::net::TcpStream;
use tokio_rustls::rustls::{ClientConnection, HandshakeKind};

/// How many octets one read off the socket takes at most.
const READ: usize = 16 * 1024;

/// The client's socket and its session.
pub(crate) struct Client {
    /// The socket as the runtime knows it, which says when it is readable and
    /// is read through, so that reading it clears what the runtime knows.
    pub(crate) stream: TcpStream,
    /// The same socket, written through without asking the runtime, which
    /// learns it is writable only in a turn: the first flight is written
    /// before any.
    writer: std::net::TcpStream,
    session: ClientConnection,
    /// What the client has decrypted since the last request it sent.
    pub(crate) received: Vec<u8>,
    /// How the connection ended, once the server closed it: cleanly, with a
    /// `close_notify` before the socket's end, or not.
    ended: Option<io::Result<()>>,
}

impl Client {
    /// A client over `stream`, written through `writer`, a clone of it, that
    /// has sent nothing yet.
    pub(crate) fn new(
        stream: TcpStream,
        writer: std::net::TcpStream,
        session: ClientConnection,
    ) -> Self {
        Self {
            stream,
            writer,
            session,
            received: Vec::new(),
            ended: None,
        }
    }

    /// How the connection ended, once it has.
    pub(crate) fn ended(&mut self) -> Option<io::Result<()>> {
        self.ended.take()
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

    /// The client's turn: decrypts everything the socket holds, then writes
    /// what that let it send. Notes the connection's end once the socket has
    /// reached its own.
    pub(crate) fn exchange(&mut self) {
        let mut octets = [0; READ];
        let mut closed = false;
        loop {
            let read = match self.stream.try_read(&mut octets) {
                Ok(0) => {
                    closed = true;
                    0
                }
                Ok(read) => read,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) => panic!("the client could not read its socket: {error}"),
            };
            let mut unread = &octets[..read];
            loop {
                // An empty read is the socket's end, which rustls is told of
                // once, so that a server that never sent `close_notify` reads
                // as a truncated session rather than a clean one.
                self.session
                    .read_tls(&mut unread)
                    .expect("the client reads what the server wrote");
                self.session
                    .process_new_packets()
                    .expect("the server speaks TLS the client accepts");
                if unread.is_empty() {
                    break;
                }
            }
            if closed {
                break;
            }
        }
        match self.session.reader().read_to_end(&mut self.received) {
            Ok(_) => self.ended = Some(Ok(())),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
            Err(error) => self.ended = Some(Err(error)),
        }
        self.flush();
    }

    /// Writes every record the client has ready into the socket, which a
    /// loopback socket's send buffer takes whole.
    fn flush(&mut self) {
        let mut octets = Vec::new();
        while self.session.wants_write() {
            self.session
                .write_tls(&mut octets)
                .expect("a Vec accepts every record");
        }
        self.writer
            .write_all(&octets)
            .expect("the socket's send buffer takes the client's records");
    }
}
