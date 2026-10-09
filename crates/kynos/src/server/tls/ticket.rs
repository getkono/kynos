//! Session-ticket keys the operator supplies, so that replicas resume one
//! another's sessions.
//!
//! A ticket is `name ‖ salt ‖ issued ‖ ciphertext ‖ tag`: the sixteen-byte
//! name of the key that sealed it, thirty-two random bytes, the second it was
//! issued at as a big-endian `u64`, and the session sealed under AES-256-GCM
//! with those three fields as associated data. Every ticket has a key of its
//! own, HKDF-SHA384 of the operator's secret over the salt, so no nonce is ever
//! drawn twice under one key however many replicas issue at once — which a
//! random nonce under one shared key cannot promise past 2³² tickets.

use std::{
    fmt,
    sync::{Arc, PoisonError, RwLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use tokio_rustls::rustls::{
    CipherSuite, SupportedCipherSuite,
    crypto::{
        CryptoProvider, SecureRandom,
        cipher::{AeadKey, Iv},
        tls13::HkdfExpander,
    },
    quic,
    server::ProducesTickets,
};

use crate::server::tls::{crypto_provider, error::TlsError};

const NAME_LEN: usize = 16;
const SALT_LEN: usize = 32;
const ISSUED_LEN: usize = size_of::<u64>();
const HEADER_LEN: usize = NAME_LEN + SALT_LEN + ISSUED_LEN;
const KEY_LEN: usize = 32;
const IV_LEN: usize = 12;

/// The labels that separate a secret's two uses. Versioned, so a later
/// construction derives names no ticket of this one carries.
const NAME_LABEL: &[u8] = b"kynos session ticket v1 name";
const KEY_LABEL: &[u8] = b"kynos session ticket v1 key";

/// RFC 8446 §4.6.1: a server must not advertise a ticket lifetime past seven
/// days.
const MAX_LIFETIME: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// One generation of session-ticket key, derived from a secret every replica
/// is given.
///
/// Two keys from the same secret are the same key, in this process or another.
/// The secret itself is not kept, only what HKDF extracted from it.
///
/// Tickets are sealed with AES-256-GCM under a per-ticket key derived by
/// HKDF-SHA384, both performed by the process's crypto provider: the one the
/// binary installed as rustls's default if it installed one, and `aws-lc-rs`
/// otherwise. The construction is fixed, so replicas on different providers
/// still resume one another's sessions.
#[derive(Clone)]
pub struct TicketKey {
    name: [u8; NAME_LEN],
    expander: Arc<dyn HkdfExpander>,
    aead: &'static dyn quic::Algorithm,
}

impl TicketKey {
    /// Derives a key from 32 secret bytes.
    ///
    /// The bytes must come from a cryptographically secure random source and
    /// reach every replica over a channel as trusted as the one that carries
    /// its private key: anyone holding them can do what
    /// [`SessionResumption::SharedTickets`](crate::server::tls::SessionResumption::SharedTickets)
    /// says a ticket key allows.
    ///
    /// A binary that installs its own crypto provider installs it before
    /// calling this, since the provider is read here.
    ///
    /// # Errors
    ///
    /// [`TlsError::TicketCipher`] when the provider offers no AES-256-GCM.
    pub fn from_secret(secret: &[u8; 32]) -> std::result::Result<Self, TlsError> {
        Self::derive(&crypto_provider(), secret)
    }

    /// The AEAD is reached through the provider's QUIC packet protection, the
    /// one rustls interface that seals under caller-supplied associated data.
    pub(in crate::server) fn derive(
        provider: &CryptoProvider,
        secret: &[u8; 32],
    ) -> std::result::Result<Self, TlsError> {
        let suite = provider
            .cipher_suites
            .iter()
            .filter_map(SupportedCipherSuite::tls13)
            .find(|suite| suite.common.suite == CipherSuite::TLS13_AES_256_GCM_SHA384)
            .ok_or(TlsError::TicketCipher)?;
        let aead = suite
            .quic
            .filter(|aead| aead.aead_key_len() == KEY_LEN)
            .ok_or(TlsError::TicketCipher)?;
        let expander: Arc<dyn HkdfExpander> =
            Arc::from(suite.hkdf_provider.extract_from_secret(None, secret));
        let mut name = [0; NAME_LEN];
        expander
            .expand_slice(&[NAME_LABEL], &mut name)
            .map_err(|_| TlsError::TicketCipher)?;
        Ok(Self {
            name,
            expander,
            aead,
        })
    }

    fn packet_key(&self, salt: &[u8]) -> Option<Box<dyn quic::PacketKey>> {
        let mut key = [0; KEY_LEN];
        let mut iv = [0; IV_LEN];
        self.expander
            .expand_slice(&[KEY_LABEL, salt, b"key"], &mut key)
            .ok()?;
        self.expander
            .expand_slice(&[KEY_LABEL, salt, b"iv"], &mut iv)
            .ok()?;
        Some(self.aead.packet_key(AeadKey::from(key), Iv::new(iv)))
    }

    fn seal(&self, random: &dyn SecureRandom, issued: u64, session: &[u8]) -> Option<Vec<u8>> {
        let mut salt = [0; SALT_LEN];
        random.fill(&mut salt).ok()?;
        let key = self.packet_key(&salt)?;
        let mut ticket = Vec::with_capacity(HEADER_LEN + session.len() + key.tag_len());
        ticket.extend_from_slice(&self.name);
        ticket.extend_from_slice(&salt);
        ticket.extend_from_slice(&issued.to_be_bytes());
        ticket.extend_from_slice(session);
        let (header, body) = ticket.split_at_mut(HEADER_LEN);
        let tag = key.encrypt_in_place(0, header, body).ok()?;
        ticket.extend_from_slice(tag.as_ref());
        Some(ticket)
    }

    /// The second the ticket was issued at, and the session it carried.
    fn open(&self, ticket: &[u8]) -> Option<(u64, Vec<u8>)> {
        let (header, body) = ticket.split_at_checked(HEADER_LEN)?;
        let (salt, issued) = header[NAME_LEN..].split_at(SALT_LEN);
        let mut session = body.to_vec();
        let length = self
            .packet_key(salt)?
            .decrypt_in_place(0, header, &mut session)
            .ok()?
            .len();
        session.truncate(length);
        Some((u64::from_be_bytes(issued.try_into().ok()?), session))
    }
}

/// The name only: it is public, since every ticket carries it in the clear.
impl fmt::Debug for TicketKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "TicketKey(")?;
        for byte in self.name {
            write!(formatter, "{byte:02x}")?;
        }
        write!(formatter, ")")
    }
}

#[derive(Debug)]
struct Generations {
    issuing: TicketKey,
    accepted: Vec<TicketKey>,
}

/// The session-ticket keys a server issues and accepts, replaceable while it
/// runs.
///
/// One key issues; that key and every accepted one open tickets. A clone is a
/// handle on the same keys, so the application keeps one and hands the other to
/// [`SessionResumption::SharedTickets`](crate::server::tls::SessionResumption::SharedTickets),
/// then calls [`rotate`](Self::rotate) from wherever it learns of a new key — a
/// timer, a secret store's watch, a signal.
///
/// Replicas do not rotate at the same instant, so a fleet rotates in two
/// steps: every replica first accepts the next key while still issuing the
/// current one, and only then does each start issuing it. Skipping the first
/// step costs full handshakes, not correctness.
///
/// # What a key exposes
///
/// These keys outlive the process, wherever the operator stores them, and
/// Kynos rotates nothing.
///
/// * **Forward secrecy.** A TLS 1.2 ticket carries the session's master
///   secret, so a key that leaks decrypts every recorded TLS 1.2 session whose
///   ticket it sealed, however long ago. TLS 1.3 resumption always runs a
///   fresh key exchange, so those sessions stay secret. What bounds the
///   exposure is erasing the key everywhere it was stored, not the ticket
///   lifetime.
/// * **Identity.** A resumed session takes its client certificate chain from
///   the ticket, so a key holder can resume as any mutual-TLS identity on
///   every replica, over either version, until the key is dropped. A
///   resumption is also not a re-verification: each one issues fresh tickets,
///   so a client that keeps returning within the lifetime keeps its identity
///   past its certificate's expiry, across restarts as well.
/// * **Rotation.** Issue under a key for no longer than the ticket lifetime,
///   keep it accepted for one lifetime after it stops issuing, then erase it.
///   A key is then held for two lifetimes, as the built-in tickets hold theirs
///   for two six-hour rotations — and six hours is the lifetime to choose
///   unless distributing keys that often is not practical.
///
/// # Examples
///
/// ```
/// use kynos::server::tls::ticket::{TicketKey, TicketKeys};
///
/// # fn main() -> Result<(), kynos::server::tls::error::TlsError> {
/// # let (first, second) = ([1; 32], [2; 32]);
/// let current = TicketKey::from_secret(&first)?;
/// let next = TicketKey::from_secret(&second)?;
///
/// let keys = TicketKeys::new(current.clone(), []);
/// // Once the next key is distributed: accept it everywhere …
/// keys.rotate(current.clone(), [next.clone()]);
/// // … then issue it, still accepting the last for one ticket lifetime …
/// keys.rotate(next.clone(), [current]);
/// // … and then forget the last one.
/// keys.rotate(next, []);
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug)]
pub struct TicketKeys {
    generations: Arc<RwLock<Arc<Generations>>>,
}

impl TicketKeys {
    /// Keys that issue under `issuing` and also accept every key in
    /// `accepted`.
    pub fn new(issuing: TicketKey, accepted: impl IntoIterator<Item = TicketKey>) -> Self {
        Self {
            generations: Arc::new(RwLock::new(Arc::new(Generations {
                issuing,
                accepted: accepted.into_iter().collect(),
            }))),
        }
    }

    /// Replaces the keys on every server holding this handle, from its next
    /// handshake.
    ///
    /// A key in neither argument is dropped: tickets sealed under it stop
    /// resuming, and once no clone of it is left this process can no longer
    /// decrypt anything it sealed.
    pub fn rotate(&self, issuing: TicketKey, accepted: impl IntoIterator<Item = TicketKey>) {
        let next = Arc::new(Generations {
            issuing,
            accepted: accepted.into_iter().collect(),
        });
        *self
            .generations
            .write()
            .unwrap_or_else(PoisonError::into_inner) = next;
    }

    /// Nothing panics while the lock is held, so a poisoned one still guards a
    /// whole value.
    fn current(&self) -> Arc<Generations> {
        Arc::clone(
            &self
                .generations
                .read()
                .unwrap_or_else(PoisonError::into_inner),
        )
    }
}

/// rustls's ticketer over operator-supplied keys.
#[derive(Debug)]
pub(in crate::server) struct SharedTicketer {
    keys: TicketKeys,
    lifetime: u32,
    random: &'static dyn SecureRandom,
}

impl SharedTicketer {
    pub(in crate::server) fn new(
        keys: TicketKeys,
        lifetime: Duration,
        provider: &CryptoProvider,
    ) -> std::result::Result<Self, TlsError> {
        let seconds = u32::try_from(lifetime.as_secs())
            .ok()
            .filter(|seconds| *seconds > 0 && lifetime <= MAX_LIFETIME)
            .ok_or(TlsError::TicketLifetime(lifetime))?;
        Ok(Self {
            keys,
            lifetime: seconds,
            random: provider.secure_random,
        })
    }

    pub(in crate::server) fn seal_at(&self, now: u64, session: &[u8]) -> Option<Vec<u8>> {
        self.keys.current().issuing.seal(self.random, now, session)
    }

    /// A ticket issued after `now` is one a replica with a faster clock
    /// sealed, and counts as new rather than as invalid.
    pub(in crate::server) fn open_at(&self, now: u64, ticket: &[u8]) -> Option<Vec<u8>> {
        let generations = self.keys.current();
        let name = ticket.get(..NAME_LEN)?;
        let key = std::iter::once(&generations.issuing)
            .chain(&generations.accepted)
            .find(|key| key.name == name)?;
        let (issued, session) = key.open(ticket)?;
        (now.saturating_sub(issued) <= u64::from(self.lifetime)).then_some(session)
    }
}

impl ProducesTickets for SharedTicketer {
    fn enabled(&self) -> bool {
        true
    }

    fn lifetime(&self) -> u32 {
        self.lifetime
    }

    fn encrypt(&self, plain: &[u8]) -> Option<Vec<u8>> {
        self.seal_at(now(), plain)
    }

    fn decrypt(&self, cipher: &[u8]) -> Option<Vec<u8>> {
        self.open_at(now(), cipher)
    }
}

/// Seconds since the Unix epoch, which is what replicas agree on.
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}
