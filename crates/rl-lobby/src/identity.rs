//! A player's identity: a secp256k1 key pair signing with BIP-340 Schnorr
//! (the Nostr key type; spec §5.2), its short fingerprint and the join
//! challenge's messages.

use alloc::string::String;
use alloc::vec::Vec;

use k256::schnorr::{Signature as SchnorrSignature, SigningKey, VerifyingKey};
use rl_proto::{PublicKey, Signature};
use sha2::{Digest as _, Sha256};

/// The domain prefix of a join challenge's signed message.
pub const CHALLENGE_PREFIX: &[u8] = b"rust-liero join v1";

/// A secret key. Randomness for signing comes from the caller (no
/// clock and no `getrandom` here: the crate is `no_std`).
#[derive(Clone)]
pub struct Identity {
    key: SigningKey,
    /// The secret as given: `key.to_bytes()` is BIP-340's normalised scalar
    /// (negated for an odd-y public key), which is not what was stored.
    secret: [u8; 32],
}

impl core::fmt::Debug for Identity {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // Never prints the secret.
        write!(f, "Identity({})", fingerprint(&self.public()))
    }
}

impl Identity {
    /// The identity of a secret scalar; `None` when it is not a valid one
    /// (zero, or not below the curve order).
    pub fn from_secret(secret: [u8; 32]) -> Option<Identity> {
        SigningKey::from_bytes((&secret).into()).ok().map(|key| Identity { key, secret })
    }

    /// The secret scalar, for the identity file.
    pub fn secret(&self) -> [u8; 32] {
        self.secret
    }

    /// The x-only public key (32 bytes).
    pub fn public(&self) -> PublicKey {
        self.key.verifying_key().to_bytes().into()
    }

    /// A BIP-340 signature of `message`, with the caller's `aux` randomness.
    pub fn sign(&self, message: &[u8], aux: [u8; 32]) -> Signature {
        match self.key.sign_raw(message, &aux) {
            Ok(sig) => sig.to_bytes(),
            // Signing fails only for a degenerate nonce (probability 2^-256);
            // an all-zero signature never verifies, and no panic on any input.
            Err(_) => [0; 64],
        }
    }
}

/// Whether `sig` is a valid signature of `message` by `key`; false for any
/// malformed key or signature.
pub fn verify(key: &PublicKey, message: &[u8], sig: &Signature) -> bool {
    let Ok(key) = VerifyingKey::from_bytes(key.into()) else { return false };
    let Ok(sig) = SchnorrSignature::try_from(sig.as_slice()) else { return false };
    key.verify_raw(message, &sig).is_ok()
}

/// A key's 8-character fingerprint: lowercase unpadded base32 (RFC 4648
/// alphabet) of the first 5 bytes of `SHA-256(key)`.
pub fn fingerprint(key: &PublicKey) -> String {
    const ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";
    let hash = Sha256::digest(key);
    let bits = hash[..5].iter().fold(0u64, |acc, b| (acc << 8) | u64::from(*b));
    (0..8).map(|i| char::from(ALPHABET[((bits >> (35 - 5 * i)) & 31) as usize])).collect()
}

/// What a joiner signs: `b"rust-liero join v1" ++ nonce ++ host key`.
pub fn challenge_message(nonce: &[u8; 32], host: &PublicKey) -> Vec<u8> {
    let mut m = Vec::with_capacity(CHALLENGE_PREFIX.len() + 64);
    m.extend_from_slice(CHALLENGE_PREFIX);
    m.extend_from_slice(nonce);
    m.extend_from_slice(host);
    m
}

/// The proof of a lobby password: `SHA-256(nonce ++ password)`.
pub fn password_proof(nonce: &[u8; 32], password: &str) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(nonce);
    h.update(password.as_bytes());
    h.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alice() -> Identity {
        let mut s = [0u8; 32];
        s[31] = 1;
        Identity::from_secret(s).unwrap()
    }

    #[test]
    fn sign_then_verify() {
        let id = alice();
        let sig = id.sign(b"hello", [7; 32]);
        assert!(verify(&id.public(), b"hello", &sig));
        assert_eq!(id.secret()[31], 1);
    }

    #[test]
    fn a_changed_message_key_or_signature_fails() {
        let id = alice();
        let sig = id.sign(b"hello", [7; 32]);
        assert!(!verify(&id.public(), b"hellp", &sig));
        let other = Identity::from_secret([9; 32]).unwrap();
        assert!(!verify(&other.public(), b"hello", &sig));
        for i in [0, 31, 32, 63] {
            let mut bad = sig;
            bad[i] ^= 1;
            assert!(!verify(&id.public(), b"hello", &bad), "byte {i}");
        }
        assert!(!verify(&[0; 32], b"hello", &sig), "not a point");
        assert!(!verify(&[0xff; 32], b"hello", &sig), "not on the curve");
        assert!(!verify(&id.public(), b"hello", &[0xff; 64]));
    }

    #[test]
    fn aux_randomness_changes_the_signature_not_its_validity() {
        let id = alice();
        let (a, b) = (id.sign(b"m", [1; 32]), id.sign(b"m", [2; 32]));
        assert_ne!(a, b);
        assert!(verify(&id.public(), b"m", &a) && verify(&id.public(), b"m", &b));
        assert_eq!(a, id.sign(b"m", [1; 32]));
    }

    #[test]
    fn secret_round_trips_for_an_odd_y_key() {
        // Secret 3's public key has odd y: BIP-340 negates the scalar.
        let mut s = [0u8; 32];
        s[31] = 3;
        let id = Identity::from_secret(s).unwrap();
        assert_eq!(id.secret(), s);
        assert_eq!(Identity::from_secret(id.secret()).unwrap().public(), id.public());
    }

    #[test]
    fn invalid_scalars_are_refused() {
        assert!(Identity::from_secret([0; 32]).is_none());
        assert!(Identity::from_secret([0xff; 32]).is_none(), "above the order");
    }

    #[test]
    fn bip340_test_vector_zero() {
        // BIP-340 test vector 0 (secret 3, aux 0, message 0).
        let mut s = [0u8; 32];
        s[31] = 3;
        let id = Identity::from_secret(s).unwrap();
        let public = "F9308A019258C31049344F85F89D5229B531C845836F99B08601F113BCE036F9";
        let sig = "E907831F80848D1069A5371B402410364BDF1C5F8307B0084C55F1CE2DCA821525F66A4A85EA8B71E482A74F382D2CE5EBEEE8FDB2172F477DF4900D310536C0";
        assert_eq!(hex(&id.public()), public);
        assert_eq!(hex(&id.sign(&[0; 32], [0; 32])), sig);
        assert!(verify(&id.public(), &[0; 32], &id.sign(&[0; 32], [0; 32])));
    }

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| alloc::format!("{x:02X}")).collect()
    }

    /// The format is pinned: changing it changes every player's displayed
    /// fingerprint.
    #[test]
    fn fingerprint_format_is_pinned() {
        let fp = fingerprint(&alice().public());
        assert_eq!(fp.len(), 8);
        assert_eq!(fp, "cmxttkmm", "secret 1 (public key: the generator's x)");
        assert_eq!(fingerprint(&[0; 32]), "mzuhvlpy");
        assert!(fp.bytes().all(|c| c.is_ascii_lowercase() || (b'2'..=b'7').contains(&c)));
    }

    #[test]
    fn challenge_message_layout() {
        let (nonce, host) = ([1u8; 32], [2u8; 32]);
        let m = challenge_message(&nonce, &host);
        assert_eq!(m.len(), 18 + 32 + 32);
        assert_eq!(&m[..18], b"rust-liero join v1");
        assert_eq!(&m[18..50], &nonce);
        assert_eq!(&m[50..], &host);
    }

    #[test]
    fn password_proof_depends_on_nonce_and_password() {
        let p = password_proof(&[1; 32], "secret");
        assert_ne!(p, password_proof(&[2; 32], "secret"));
        assert_ne!(p, password_proof(&[1; 32], "Secret"));
        assert_eq!(p, password_proof(&[1; 32], "secret"));
        let mut h = Sha256::new();
        h.update([1u8; 32]);
        h.update(b"secret");
        assert_eq!(p, <[u8; 32]>::from(h.finalize()));
    }
}
