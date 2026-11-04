//! Studio Live's handshake and frame encryption (docs/STUDIO-LIVE-SPEC.md § 5, § 6, § 9 and LIVE-CRYPTO.md of the 0.5
//! site; desktop-shell `live/crypto.rs` and `assets/js/live-crypto.js` of 0.5, byte for byte). Knows nothing of sockets
//! or MessagePack: the session server carries the four handshake messages in `pake` envelopes and passes every later
//! frame through [`Session::seal`] / [`Session::open`].
//!
//! Handshake (joiner = SPAKE2 side A, host = side B; RustCrypto `spake2`, Ed25519 group, as noble does in the JS):
//! 1. joiner → host: A message (33 bytes, `'A'` ‖ point)
//! 2. host → joiner: B message (33 bytes, `'B'` ‖ point)
//! 3. joiner → host: the joiner's confirmation (32 bytes, HMAC)
//! 4. host → joiner: the host's confirmation (32 bytes), sent only after 3 checked out
//!
//! The joiner proves the code first, so a wrong code is always the host's to count (three in a row replace the code).
//!
//! Frames: `header (11) ‖ ChaCha20-Poly1305(key, nonce, body, aad = header)`. Header = version 1, kind (0 data, 1 rekey),
//! epoch, counter (u64 big-endian). Nonce = direction (`'J'` joiner → host, `'H'` host → joiner) ‖ epoch ‖ 0 ‖ 0 ‖
//! counter. Each direction has its own key and counter; a counter that is not exactly the next one, a wrong epoch or a
//! failed tag is an error and the connection closes. `Session::rekey` (host only) sends a rekey frame under the old key
//! and moves both directions to `HKDF(old key, "next")`; the joiner follows when it opens that frame.
//!
//! Randomness comes from the caller (`*_with_rng`), so this module also builds for the browser; [`os_rng`] is the
//! desktop's (the OS's random source).

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use rand_core::{CryptoRng, RngCore};
use sha2::Sha256;
use spake2::{Ed25519Group, Identity, Password, Spake2};

/// Version byte of every frame header, and the label in every derived key.
pub const VERSION: u8 = 1;
const LABEL: &[u8] = b"cadcraft-live/1";
pub const PAKE_LEN: usize = 33;
pub const CONFIRM_LEN: usize = 32;
pub const HEADER_LEN: usize = 11;
pub const TAG_LEN: usize = 16;
const KIND_DATA: u8 = 0;
const KIND_REKEY: u8 = 1;
const DIR_JOINER: u8 = b'J';
const DIR_HOST: u8 = b'H';
const MAX_COUNTER: u64 = 1 << 63;

/// Crockford base32: no I, L, O or U.
pub const CODE_ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// The OS's random source as a `rand_core` RNG (desktop; getrandom). A failure of the OS source is a panic-free
/// error at the call that needs randomness, never a weak key.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone, Copy, Default)]
pub struct OsRng;

#[cfg(not(target_arch = "wasm32"))]
impl RngCore for OsRng {
    fn next_u32(&mut self) -> u32 {
        let mut b = [0u8; 4];
        self.fill_bytes(&mut b);
        u32::from_le_bytes(b)
    }
    fn next_u64(&mut self) -> u64 {
        let mut b = [0u8; 8];
        self.fill_bytes(&mut b);
        u64::from_le_bytes(b)
    }
    fn fill_bytes(&mut self, dest: &mut [u8]) {
        // Infallible on every desktop OS we ship (getrandom's syscalls); should it ever fail, the bytes stay zero and
        // `random` below reports it, so callers that can check do (session tokens, codes).
        let _ = getrandom::fill(dest);
    }
    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
        getrandom::fill(dest).map_err(|_| rand_core::Error::from(core::num::NonZeroU32::MIN))
    }
}
#[cfg(not(target_arch = "wasm32"))]
impl CryptoRng for OsRng {}

/// `n` bytes from the OS's random source; None when it fails.
#[cfg(not(target_arch = "wasm32"))]
pub fn random(n: usize) -> Option<Vec<u8>> {
    let mut b = vec![0u8; n];
    getrandom::fill(&mut b).ok()?;
    Some(b)
}

/// A short code `TT-SSSSS` as typed: case, spaces and the dash don't matter; O reads as 0, I and L as 1. Returns the
/// public tag and the secret (the 5 normalized characters, the SPAKE2 password). The context is `lan:<tag>`.
pub fn parse_code(code: &str) -> Option<(String, Vec<u8>)> {
    let mut out = Vec::with_capacity(7);
    for ch in code.chars().take(64) {
        let c = match ch.to_ascii_uppercase() {
            ' ' | '-' => continue,
            'O' => '0',
            'I' | 'L' => '1',
            c => c,
        };
        let b = u8::try_from(u32::from(c)).ok()?;
        if !CODE_ALPHABET.contains(&b) {
            return None;
        }
        out.push(b);
    }
    if out.len() != 7 {
        return None;
    }
    let tag = String::from_utf8(out.get(..2)?.to_vec()).ok()?;
    Some((tag, out.get(2..)?.to_vec()))
}

/// A short code from 7 random bytes, e.g. `K7-Q4M2X`.
pub fn code_of(b: &[u8; 7]) -> String {
    let c: Vec<char> = b.iter().map(|x| char::from(CODE_ALPHABET.get(usize::from(x & 31)).copied().unwrap_or(b'0'))).collect();
    let rest: String = c.iter().skip(2).collect();
    format!("{}{}-{rest}", c.first().copied().unwrap_or('0'), c.get(1).copied().unwrap_or('0'))
}

/// A new short code from the OS's random source.
#[cfg(not(target_arch = "wasm32"))]
pub fn new_code() -> Option<String> {
    let r = random(7)?;
    let mut b = [0u8; 7];
    b.copy_from_slice(r.get(..7)?);
    Some(code_of(&b))
}

/// An invite link's fragment (base64url, no padding) → the secret bytes (16–32 of them). The context is `relay:<room>`.
pub fn parse_link_secret(fragment: &str) -> Option<Vec<u8>> {
    let f = fragment.strip_prefix('#').unwrap_or(fragment);
    if f.len() > 64 {
        return None;
    }
    let mut bits: u32 = 0;
    let mut n: u32 = 0;
    let mut out = vec![];
    for ch in f.bytes() {
        let v = u32::from(match ch {
            b'A'..=b'Z' => ch.wrapping_sub(b'A'),
            b'a'..=b'z' => ch.wrapping_sub(b'a').wrapping_add(26),
            b'0'..=b'9' => ch.wrapping_sub(b'0').wrapping_add(52),
            b'-' => 62,
            b'_' => 63,
            _ => return None,
        });
        bits = (bits << 6) | v;
        n = n.wrapping_add(6);
        if n >= 8 {
            n = n.wrapping_sub(8);
            out.push(u8::try_from((bits >> n) & 0xff).ok()?);
            bits &= (1u32 << n).wrapping_sub(1);
        }
    }
    // Leftover bits must be zero (one spelling per secret); 16–32 bytes.
    if bits != 0 || f.len() % 4 == 1 || !(16..=32).contains(&out.len()) {
        return None;
    }
    Some(out)
}

/// Why a handshake or a frame failed (live-crypto.js `LiveCryptoError` codes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CryptoError {
    /// A handshake message of the wrong length or side, or a point that isn't on the curve.
    BadMessage,
    /// The confirmation didn't match: the two sides used different codes (or someone tampered).
    WrongCode,
    /// A frame that is too short, has another version or kind, or fails its tag.
    BadFrame,
    /// A frame whose counter or epoch isn't the next one (replayed, reordered or dropped).
    Order,
    /// Only the host may rotate keys; the joiner refuses a rekey it didn't expect and never sends one.
    Role,
    /// 255 rotations or 2^63 frames in one direction: end the connection and join again.
    Exhausted,
    /// The handshake was used out of turn.
    State,
}

impl std::fmt::Display for CryptoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            CryptoError::BadMessage => "bad_message",
            CryptoError::WrongCode => "wrong_code",
            CryptoError::BadFrame => "bad_frame",
            CryptoError::Order => "order",
            CryptoError::Role => "role",
            CryptoError::Exhausted => "exhausted",
            CryptoError::State => "state",
        })
    }
}
impl std::error::Error for CryptoError {}

/// Which end of the handshake.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Joiner,
    Host,
}

/// The two SPAKE2 identities: they bind the session's context ("lan:<tag>", "relay:<room>", "rejoin:<session>"), so
/// a handshake recorded in one session is useless in another.
fn identities(context: &str) -> (Vec<u8>, Vec<u8>) {
    let mut a = LABEL.to_vec();
    a.extend_from_slice(b" joiner ");
    a.extend_from_slice(context.as_bytes());
    let mut b = LABEL.to_vec();
    b.extend_from_slice(b" host ");
    b.extend_from_slice(context.as_bytes());
    (a, b)
}

/// HKDF-SHA256(salt = label, ikm, info) → 32 bytes.
fn derive(ikm: &[u8], info: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    // 32 bytes is far below HKDF-SHA256's limit (255 × 32): expand cannot fail here.
    let _ = Hkdf::<Sha256>::new(Some(LABEL), ikm).expand(info.as_bytes(), &mut out);
    out
}

fn confirm_mac(key: &[u8; 32], who: &[u8], a: &[u8], b: &[u8]) -> [u8; 32] {
    // HMAC takes any key length; a 32-byte key never fails
    let Ok(mut m) = <Hmac<Sha256> as Mac>::new_from_slice(key) else { return [0u8; 32] };
    m.update(who);
    m.update(a);
    m.update(b);
    m.finalize().into_bytes().into()
}

/// Constant-time comparison of two byte strings of equal length.
pub fn same(x: &[u8], y: &[u8]) -> bool {
    x.len() == y.len() && x.iter().zip(y).fold(0u8, |acc, (p, q)| acc | (p ^ q)) == 0
}

/// Everything the four handshake messages agree on.
struct Agreed {
    a: Vec<u8>,
    b: Vec<u8>,
    k: Vec<u8>,
}

impl Agreed {
    fn confirm(&self, side: Side) -> [u8; 32] {
        let (info, who) = match side {
            Side::Joiner => ("confirm joiner", b"joiner".as_slice()),
            Side::Host => ("confirm host", b"host".as_slice()),
        };
        confirm_mac(&derive(&self.k, info), who, &self.a, &self.b)
    }
    fn session(&self, side: Side) -> Session {
        let j2h = derive(&self.k, "frames j2h");
        let h2j = derive(&self.k, "frames h2j");
        let (send, recv) = match side {
            Side::Joiner => (j2h, h2j),
            Side::Host => (h2j, j2h),
        };
        Session {
            side,
            send: Direction::new(send, if side == Side::Joiner { DIR_JOINER } else { DIR_HOST }),
            recv: Direction::new(recv, if side == Side::Joiner { DIR_HOST } else { DIR_JOINER }),
            recv_max: 0,
            resume: derive(&self.k, "resume"),
        }
    }
}

/// The joiner's side: `start` → send its message; `receive(host's)` → send the confirmation it returns;
/// `finish(host's confirmation)` → the session.
pub struct JoinerHandshake {
    spake: Option<Spake2<Ed25519Group>>,
    a: Vec<u8>,
    agreed: Option<Agreed>,
}

impl JoinerHandshake {
    /// Starts with the OS's randomness (desktop).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn start(secret: &[u8], context: &str) -> (Self, Vec<u8>) {
        Self::start_with_rng(secret, context, OsRng)
    }
    /// The RNG gives the SPAKE2 scalar (64 bytes, reduced): tests and vectors pass a fixed one.
    pub fn start_with_rng(secret: &[u8], context: &str, rng: impl CryptoRng + RngCore) -> (Self, Vec<u8>) {
        let (ia, ib) = identities(context);
        let (s, msg) = Spake2::<Ed25519Group>::start_a_with_rng(&Password::new(secret), &Identity::new(&ia), &Identity::new(&ib), rng);
        (JoinerHandshake { spake: Some(s), a: msg.clone(), agreed: None }, msg)
    }
    /// Takes the host's B message; returns the joiner's confirmation to send.
    pub fn receive(&mut self, host_msg: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let s = self.spake.take().ok_or(CryptoError::State)?;
        if host_msg.len() != PAKE_LEN || host_msg.first() != Some(&b'B') {
            return Err(CryptoError::BadMessage);
        }
        let k = s.finish(host_msg).map_err(|_| CryptoError::BadMessage)?;
        let agreed = Agreed { a: self.a.clone(), b: host_msg.to_vec(), k };
        let c = agreed.confirm(Side::Joiner).to_vec();
        self.agreed = Some(agreed);
        Ok(c)
    }
    /// Checks the host's confirmation. `WrongCode` here means the code was wrong (or the host is not who it says).
    pub fn finish(self, host_confirm: &[u8]) -> Result<Session, CryptoError> {
        let agreed = self.agreed.ok_or(CryptoError::State)?;
        if !same(host_confirm, &agreed.confirm(Side::Host)) {
            return Err(CryptoError::WrongCode);
        }
        Ok(agreed.session(Side::Joiner))
    }
}

/// The host's side, one per connection: `receive(joiner's)` → send the B message it returns;
/// `finish(joiner's confirmation)` → the host's confirmation to send, and the session.
pub struct HostHandshake {
    secret: Vec<u8>,
    context: String,
    agreed: Option<Agreed>,
}

impl HostHandshake {
    pub fn new(secret: &[u8], context: &str) -> Self {
        HostHandshake { secret: secret.to_vec(), context: context.into(), agreed: None }
    }
    /// With the OS's randomness (desktop).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn receive(&mut self, joiner_msg: &[u8]) -> Result<Vec<u8>, CryptoError> {
        self.receive_with_rng(joiner_msg, OsRng)
    }
    pub fn receive_with_rng(&mut self, joiner_msg: &[u8], rng: impl CryptoRng + RngCore) -> Result<Vec<u8>, CryptoError> {
        if self.agreed.is_some() {
            return Err(CryptoError::State);
        }
        if joiner_msg.len() != PAKE_LEN || joiner_msg.first() != Some(&b'A') {
            return Err(CryptoError::BadMessage);
        }
        let (ia, ib) = identities(&self.context);
        let (s, b) = Spake2::<Ed25519Group>::start_b_with_rng(&Password::new(&self.secret), &Identity::new(&ia), &Identity::new(&ib), rng);
        let k = s.finish(joiner_msg).map_err(|_| CryptoError::BadMessage)?;
        self.agreed = Some(Agreed { a: joiner_msg.to_vec(), b: b.clone(), k });
        Ok(b)
    }
    /// Checks the joiner's confirmation: `WrongCode` = a wrong try to count. On success, send the returned
    /// confirmation and keep the session.
    pub fn finish(self, joiner_confirm: &[u8]) -> Result<(Vec<u8>, Session), CryptoError> {
        let agreed = self.agreed.ok_or(CryptoError::State)?;
        if !same(joiner_confirm, &agreed.confirm(Side::Joiner)) {
            return Err(CryptoError::WrongCode);
        }
        Ok((agreed.confirm(Side::Host).to_vec(), agreed.session(Side::Host)))
    }
}

struct Direction {
    key: [u8; 32],
    dir: u8,
    epoch: u8,
    counter: u64,
}

impl Direction {
    fn new(key: [u8; 32], dir: u8) -> Self {
        Direction { key, dir, epoch: 0, counter: 0 }
    }
    fn nonce(&self) -> [u8; 12] {
        let c = self.counter.to_be_bytes();
        [self.dir, self.epoch, 0, 0, c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]
    }
    fn header(&self, kind: u8) -> [u8; HEADER_LEN] {
        let c = self.counter.to_be_bytes();
        [VERSION, kind, self.epoch, c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]
    }
    fn next(&mut self) -> Result<(), CryptoError> {
        let epoch = self.epoch.checked_add(1).ok_or(CryptoError::Exhausted)?;
        self.key = derive(&self.key, "next");
        self.epoch = epoch;
        self.counter = 0;
        Ok(())
    }
}

impl Drop for Direction {
    fn drop(&mut self) {
        self.key = [0u8; 32];
    }
}

/// What `open` found in a frame.
#[derive(Debug, PartialEq, Eq)]
pub enum Opened {
    Data(Vec<u8>),
    /// The host moved to the next keys; the joiner already followed. Nothing to hand on.
    Rekeyed,
}

/// One connection's keys after the handshake.
pub struct Session {
    side: Side,
    send: Direction,
    recv: Direction,
    /// Host only: the newest epoch it announced. The joiner's frames may come under any epoch from `recv.epoch` up to
    /// this one (it may have sent some under a middle key before the next rekey frame reached it).
    recv_max: u8,
    resume: [u8; 32],
}

impl Session {
    pub fn side(&self) -> Side {
        self.side
    }
    /// The send direction's epoch (how many rotations so far).
    pub fn epoch(&self) -> u8 {
        self.send.epoch
    }

    fn seal_kind(&mut self, kind: u8, body: &[u8]) -> Result<Vec<u8>, CryptoError> {
        if self.send.counter >= MAX_COUNTER {
            return Err(CryptoError::Exhausted);
        }
        let header = self.send.header(kind);
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&self.send.key));
        let ct = cipher
            .encrypt(Nonce::from_slice(&self.send.nonce()), Payload { msg: body, aad: &header })
            .map_err(|_| CryptoError::BadFrame)?;
        self.send.counter = self.send.counter.saturating_add(1);
        let mut out = Vec::with_capacity(HEADER_LEN.saturating_add(ct.len()));
        out.extend_from_slice(&header);
        out.extend_from_slice(&ct);
        Ok(out)
    }

    /// Encrypts one frame body (a MessagePack envelope).
    pub fn seal(&mut self, body: &[u8]) -> Result<Vec<u8>, CryptoError> {
        self.seal_kind(KIND_DATA, body)
    }

    /// Host only: tells the joiner to move to the next keys, and moves. Send the returned frame before any other.
    pub fn rekey(&mut self) -> Result<Vec<u8>, CryptoError> {
        if self.side != Side::Host {
            return Err(CryptoError::Role);
        }
        let max = self.recv_max.checked_add(1).ok_or(CryptoError::Exhausted)?;
        self.recv_max = max;
        let frame = self.seal_kind(KIND_REKEY, &[])?;
        self.send.next()?;
        Ok(frame)
    }

    /// Decrypts one frame. Any error means: close the connection.
    pub fn open(&mut self, frame: &[u8]) -> Result<Opened, CryptoError> {
        let (Some(head), Some(sealed)) = (frame.get(..HEADER_LEN), frame.get(HEADER_LEN..)) else { return Err(CryptoError::BadFrame) };
        let (Some(&version), Some(&kind), Some(&epoch), Some(cb)) = (head.first(), head.get(1), head.get(2), head.get(3..HEADER_LEN))
        else {
            return Err(CryptoError::BadFrame);
        };
        if sealed.len() < TAG_LEN || version != VERSION || kind > KIND_REKEY {
            return Err(CryptoError::BadFrame);
        }
        let mut c8 = [0u8; 8];
        c8.copy_from_slice(cb);
        let counter = u64::from_be_bytes(c8);
        if kind == KIND_REKEY && self.side == Side::Host {
            return Err(CryptoError::Role);
        }
        // Host: the joiner's first frame under a newer announced key ends the older ones (its counter starts at 0).
        let ahead = epoch > self.recv.epoch && epoch <= self.recv_max;
        let mut d = Direction::new(self.recv.key, self.recv.dir);
        d.epoch = self.recv.epoch;
        d.counter = self.recv.counter;
        if ahead {
            while d.epoch < epoch {
                d.next()?;
            }
        }
        if epoch != d.epoch || counter != d.counter {
            return Err(CryptoError::Order);
        }
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&d.key));
        let body = cipher.decrypt(Nonce::from_slice(&d.nonce()), Payload { msg: sealed, aad: head }).map_err(|_| CryptoError::BadFrame)?;
        d.counter = d.counter.saturating_add(1);
        self.recv = d;
        if kind == KIND_REKEY {
            if !body.is_empty() {
                return Err(CryptoError::BadFrame);
            }
            self.recv.next()?;
            self.send.next()?;
            self.recv_max = self.recv.epoch;
            return Ok(Opened::Rekeyed);
        }
        Ok(Opened::Data(body))
    }

    /// A 32-byte secret for one purpose (`label`), the same on both ends and never sent: the rejoin ticket
    /// (`"ticket"`), the signed-in name's audience (`"assertion"`). Not changed by rekeys.
    pub fn exporter(&self, label: &str) -> [u8; 32] {
        derive(&self.resume, label)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.resume = [0u8; 32];
    }
}

/// base64url without padding (the assertion audience, LIVE-CRYPTO.md § Signed-in names).
pub fn b64url(b: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let ch = |v: u32| char::from(A.get(usize::try_from(v & 63).unwrap_or(0)).copied().unwrap_or(b'A'));
    let mut s = String::with_capacity(b.len().saturating_mul(2));
    for chunk in b.chunks(3) {
        let (b0, b1, b2) = (chunk.first().copied().unwrap_or(0), chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0));
        let v = (u32::from(b0) << 16) | (u32::from(b1) << 8) | u32::from(b2);
        s.push(ch(v >> 18));
        s.push(ch(v >> 12));
        if chunk.len() > 1 {
            s.push(ch(v >> 6));
        }
        if chunk.len() > 2 {
            s.push(ch(v));
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(js: &[u8], hs: &[u8], ctx: &str) -> Result<(Session, Session), CryptoError> {
        let (mut j, a) = JoinerHandshake::start(js, ctx);
        let mut h = HostHandshake::new(hs, ctx);
        let b = h.receive(&a)?;
        let cj = j.receive(&b)?;
        let (ch, host) = h.finish(&cj)?;
        let joiner = j.finish(&ch)?;
        Ok((joiner, host))
    }

    #[test]
    fn codes_and_links() {
        assert_eq!(parse_code("k7-q4m2x"), Some(("K7".into(), b"Q4M2X".to_vec())));
        assert_eq!(parse_code(" K7 QOMIL "), Some(("K7".into(), b"Q0M11".to_vec())));
        assert_eq!(parse_code("K7-Q4M2U"), None);
        assert_eq!(parse_code("K7-Q4M2"), None);
        assert_eq!(parse_code("K7-Q4M2Xé"), None);
        for _ in 0..50 {
            let c = new_code().expect("random");
            assert_eq!(c.len(), 8);
            assert!(parse_code(&c).is_some());
        }
        assert_eq!(parse_link_secret("#paWlpaWlpaWlpaWlpaWlpQ"), Some(vec![0xa5; 16]));
        assert_eq!(parse_link_secret("paWlpaWlpaWlpaWlpaWlpR"), None);
        assert_eq!(parse_link_secret("paWl"), None);
        assert_eq!(parse_link_secret("paWlpaWlpaWlpaWlpaWlpQ=="), None);
        assert_eq!(b64url(&[0xfb, 0xff]), "-_8");
    }

    #[test]
    fn handshake_and_frames() {
        let (mut j, mut h) = pair(b"Q4M2X", b"Q4M2X", "lan:K7").expect("pair");
        let f = j.seal(b"hello").expect("seal");
        assert_eq!(h.open(&f), Ok(Opened::Data(b"hello".to_vec())));
        let g = h.seal(b"welcome").expect("seal");
        assert_eq!(j.open(&g), Ok(Opened::Data(b"welcome".to_vec())));
        assert_eq!(j.exporter("ticket"), h.exporter("ticket"));
    }

    #[test]
    fn wrong_code_is_the_hosts_to_count() {
        assert_eq!(pair(b"Q4M2X", b"Q4M2Y", "lan:K7").err(), Some(CryptoError::WrongCode));
        let (mut j, a) = JoinerHandshake::start(b"Q4M2X", "lan:K7");
        let mut h = HostHandshake::new(b"Q4M2X", "lan:K8");
        let b = h.receive(&a).expect("b");
        let cj = j.receive(&b).expect("c");
        assert_eq!(h.finish(&cj).err(), Some(CryptoError::WrongCode));
        // bad messages: wrong side, wrong length
        let mut h = HostHandshake::new(b"x", "c");
        assert_eq!(h.receive(&[b'B'; 33]).err(), Some(CryptoError::BadMessage));
        assert_eq!(h.receive(&[b'A'; 3]).err(), Some(CryptoError::BadMessage));
    }

    #[test]
    fn replay_reorder_tamper() {
        let (mut j, mut h) = pair(b"s", b"s", "relay:ROOM").expect("pair");
        let f1 = j.seal(b"1").expect("seal");
        let f2 = j.seal(b"2").expect("seal");
        assert_eq!(h.open(&f2).err(), Some(CryptoError::Order));
        assert!(h.open(&f1).is_ok());
        assert_eq!(h.open(&f1).err(), Some(CryptoError::Order));
        let mut f3 = j.seal(b"3").expect("seal");
        assert!(h.open(&f2).is_ok());
        if let Some(last) = f3.last_mut() {
            *last ^= 1;
        }
        assert_eq!(h.open(&f3).err(), Some(CryptoError::BadFrame));
        assert_eq!(h.open(&[1, 0]).err(), Some(CryptoError::BadFrame));
        let mut j2 = pair(b"s", b"s", "x").expect("pair");
        let echo = j2.1.seal(b"x").expect("seal");
        assert!(j2.1.open(&echo).is_err());
    }

    #[test]
    fn rekey_both_directions_with_frames_in_flight() {
        let (mut j, mut h) = pair(b"s", b"s", "lan:AB").expect("pair");
        let before = j.seal(b"in flight").expect("seal");
        let r = h.rekey().expect("rekey");
        assert_eq!(h.open(&before), Ok(Opened::Data(b"in flight".to_vec())));
        assert_eq!(j.open(&r), Ok(Opened::Rekeyed));
        let after = j.seal(b"new key").expect("seal");
        assert_eq!(after.get(2), Some(&1));
        assert_eq!(h.open(&after), Ok(Opened::Data(b"new key".to_vec())));
        let back = h.seal(b"ok").expect("seal");
        assert_eq!(j.open(&back), Ok(Opened::Data(b"ok".to_vec())));
        assert_eq!(h.open(&before).err(), Some(CryptoError::Order));
        assert_eq!(j.rekey().err(), Some(CryptoError::Role));
    }

    #[test]
    fn two_rekeys_before_the_joiner_answers() {
        let (mut j, mut h) = pair(b"s", b"s", "lan:AB").expect("pair");
        let r1 = h.rekey().expect("rekey");
        let r2 = h.rekey().expect("rekey");
        assert_eq!(j.open(&r1), Ok(Opened::Rekeyed));
        assert_eq!(j.open(&r2), Ok(Opened::Rekeyed));
        let f = j.seal(b"e2").expect("seal");
        assert_eq!(f.get(2), Some(&2));
        assert_eq!(h.open(&f), Ok(Opened::Data(b"e2".to_vec())));
    }
}
