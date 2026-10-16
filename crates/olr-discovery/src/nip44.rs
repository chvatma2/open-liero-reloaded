//! NIP-44 v2 encryption (decision N44), exactly as the NIP's pseudocode,
//! checked against its official test vectors (`tests/fixtures/`): a
//! secp256k1 ECDH conversation key (HKDF-SHA256 extract, salt `nip44-v2`),
//! per-message keys (HKDF expand of the nonce), padded plaintext, ChaCha20
//! and HMAC-SHA256 over `nonce || ciphertext`, all in base64.
//!
//! Only the 2-byte length prefix is supported: plaintexts of 1..=65535
//! bytes. The NIP's later extended prefix (6 bytes, for 64 KiB and more) is
//! left out, which the NIP allows ("implementations SHOULD enforce their own
//! maximum"); handshakes are at most 16 KiB.
//!
//! Decryption never panics on any input and compares MACs in constant time.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use chacha20::ChaCha20;
use chacha20::cipher::{KeyIvInit, StreamCipher};
use hmac::{Hmac, KeyInit, Mac};
use olr_proto::PublicKey;
use sha2::Sha256;

/// The version byte of NIP-44 v2 payloads.
const VERSION: u8 = 2;
/// Plaintext lengths with the 2-byte prefix.
const MIN_PLAINTEXT: usize = 1;
const MAX_PLAINTEXT: usize = 65_535;
/// Base64 payload lengths: 99 and 65 603 decoded bytes.
const MIN_PAYLOAD: usize = 132;
const MAX_PAYLOAD: usize = 87_472;
/// Decoded payload lengths: version, nonce and MAC (65) plus a padded
/// plaintext with its prefix (34 ..= 65 538).
const MIN_DATA: usize = 99;
const MAX_DATA: usize = 65_603;

/// Why a payload does not decrypt, or a plaintext not encrypt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Nip44Error {
    #[error("unknown version")]
    Version,
    #[error("bad payload")]
    Payload,
    #[error("bad MAC")]
    Mac,
    #[error("bad padding")]
    Padding,
    #[error("bad length")]
    Length,
}

/// The conversation key of `secret` (a scalar as given, as
/// `Identity::secret` returns it) and the x-only `public` key (lifted to
/// its even-y point): HKDF-extract(salt `nip44-v2`, the shared point's x).
/// `None` for a secret that is zero or not below the curve order, or an x
/// that is on no point of the curve.
pub fn conversation_key(secret: &[u8; 32], public: &PublicKey) -> Option<[u8; 32]> {
    let scalar = Option::<k256::NonZeroScalar>::from(k256::NonZeroScalar::from_repr((*secret).into()))?;
    let mut sec1 = [0u8; 33];
    sec1[0] = 0x02;
    sec1[1..].copy_from_slice(public);
    let point = k256::PublicKey::from_sec1_bytes(&sec1).ok()?;
    let shared = k256::ecdh::diffie_hellman(scalar, point.as_affine());
    let (prk, _) = hkdf::Hkdf::<Sha256>::extract(Some(b"nip44-v2"), shared.raw_secret_bytes());
    Some(prk.into())
}

/// The message keys of `nonce`: HKDF-expand(conversation key, info =
/// nonce, 76 bytes), split into the ChaCha20 key, the ChaCha20 nonce and
/// the HMAC key.
pub fn message_keys(conversation_key: &[u8; 32], nonce: &[u8; 32]) -> ([u8; 32], [u8; 12], [u8; 32]) {
    let mut okm = [0u8; 76];
    // A 32-byte PRK and 76 bytes of output are always valid for HKDF-SHA256.
    if let Ok(hk) = hkdf::Hkdf::<Sha256>::from_prk(conversation_key) {
        let _ = hk.expand(nonce, &mut okm);
    }
    let mut key = [0u8; 32];
    let mut iv = [0u8; 12];
    let mut hmac_key = [0u8; 32];
    key.copy_from_slice(&okm[..32]);
    iv.copy_from_slice(&okm[32..44]);
    hmac_key.copy_from_slice(&okm[44..]);
    (key, iv, hmac_key)
}

/// The padded length of a `len`-byte plaintext: 32 up to 32 bytes, then
/// chunks of 32 bytes up to 256, then of an eighth of the next power of
/// two.
pub fn padded_len(len: usize) -> usize {
    if len <= 32 {
        return 32;
    }
    // 128-bit arithmetic: the NIP's own note, the result can exceed the
    // input's width.
    let n = (len - 1) as u128;
    let next = 1u128 << (128 - n.leading_zeros());
    let chunk = if next <= 256 { 32 } else { next / 8 };
    usize::try_from(chunk * (n / chunk + 1)).unwrap_or(usize::MAX)
}

/// HMAC-SHA256 with `hmac_key`, fed `nonce || ciphertext`.
fn mac(hmac_key: &[u8; 32], nonce: &[u8; 32], ciphertext: &[u8]) -> Hmac<Sha256> {
    // HMAC pads a key shorter than the block (64 bytes) with zeros: the
    // padded key is the same key, and needs no fallible length check.
    let mut block = [0u8; 64];
    block[..32].copy_from_slice(hmac_key);
    let mut m = <Hmac<Sha256> as KeyInit>::new(&block.into());
    m.update(nonce);
    m.update(ciphertext);
    m
}

/// Encrypts `plaintext` (1..=65535 bytes) with the caller's random
/// `nonce`; the base64 payload.
pub fn encrypt(conversation_key: &[u8; 32], plaintext: &str, nonce: [u8; 32]) -> Result<String, Nip44Error> {
    let len = plaintext.len();
    if !(MIN_PLAINTEXT..=MAX_PLAINTEXT).contains(&len) {
        return Err(Nip44Error::Length);
    }
    let (key, iv, hmac_key) = message_keys(conversation_key, &nonce);
    let mut out = Vec::with_capacity(1 + 32 + 2 + padded_len(len) + 32);
    out.push(VERSION);
    out.extend_from_slice(&nonce);
    // `len` fits: it was checked against MAX_PLAINTEXT.
    out.extend_from_slice(&(len as u16).to_be_bytes());
    out.extend_from_slice(plaintext.as_bytes());
    out.resize(1 + 32 + 2 + padded_len(len), 0);
    ChaCha20::new(&key.into(), &iv.into()).apply_keystream(&mut out[33..]);
    let tag = mac(&hmac_key, &nonce, &out[33..]).finalize().into_bytes();
    out.extend_from_slice(&tag);
    Ok(BASE64.encode(out))
}

/// Decrypts a base64 payload: the version (a leading `#` is an unknown
/// one), the lengths, the version byte, the MAC (in constant time), then
/// the padding.
pub fn decrypt(conversation_key: &[u8; 32], payload: &str) -> Result<String, Nip44Error> {
    if payload.is_empty() || payload.starts_with('#') {
        return Err(Nip44Error::Version);
    }
    if !(MIN_PAYLOAD..=MAX_PAYLOAD).contains(&payload.len()) {
        return Err(Nip44Error::Payload);
    }
    let data = BASE64.decode(payload).map_err(|_| Nip44Error::Payload)?;
    if !(MIN_DATA..=MAX_DATA).contains(&data.len()) {
        return Err(Nip44Error::Payload);
    }
    if data[0] != VERSION {
        return Err(Nip44Error::Version);
    }
    let mut nonce = [0u8; 32];
    nonce.copy_from_slice(&data[1..33]);
    let (ciphertext, tag) = data[33..].split_at(data.len() - 33 - 32);
    let (key, iv, hmac_key) = message_keys(conversation_key, &nonce);
    mac(&hmac_key, &nonce, ciphertext).verify_slice(tag).map_err(|_| Nip44Error::Mac)?;
    let mut padded = ciphertext.to_vec();
    ChaCha20::new(&key.into(), &iv.into()).apply_keystream(&mut padded);
    let len = usize::from(u16::from_be_bytes([padded[0], padded[1]]));
    if len < MIN_PLAINTEXT || padded.len() != 2 + padded_len(len) {
        return Err(Nip44Error::Padding);
    }
    // `padded_len(len) >= len`, so the slice is in bounds.
    String::from_utf8(padded[2..2 + len].to_vec()).map_err(|_| Nip44Error::Payload)
}
