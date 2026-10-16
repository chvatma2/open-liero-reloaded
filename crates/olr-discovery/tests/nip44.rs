//! NIP-44 v2 against the official test vectors
//! (`fixtures/nip44.vectors.json`, unmodified; its source and SHA-256 are in
//! the crate's CLAUDE.md), and fuzzed decryption.

use hmac::{KeyInit, Mac};
use olr_discovery::nip44::{Nip44Error, conversation_key, decrypt, encrypt, message_keys, padded_len};
use proptest::prelude::*;
use serde_json::Value;
use sha2::{Digest, Sha256};

fn vectors() -> Value {
    serde_json::from_str(include_str!("fixtures/nip44.vectors.json")).unwrap()
}

fn h32(s: &str) -> [u8; 32] {
    hex::decode(s).unwrap().try_into().unwrap()
}

#[test]
fn conversation_keys_match_the_vectors() {
    let v = vectors();
    for case in v["v2"]["valid"]["get_conversation_key"].as_array().unwrap() {
        let key =
            conversation_key(&h32(case["sec1"].as_str().unwrap()), &h32(case["pub2"].as_str().unwrap()));
        assert_eq!(key, Some(h32(case["conversation_key"].as_str().unwrap())), "{case}");
    }
    for case in v["v2"]["invalid"]["get_conversation_key"].as_array().unwrap() {
        let sec: Option<[u8; 32]> =
            hex::decode(case["sec1"].as_str().unwrap()).ok().and_then(|b| b.try_into().ok());
        let public: Option<[u8; 32]> =
            hex::decode(case["pub2"].as_str().unwrap()).ok().and_then(|b| b.try_into().ok());
        if let (Some(s), Some(p)) = (sec, public) {
            assert_eq!(conversation_key(&s, &p), None, "{}", case["note"]);
        }
    }
}

#[test]
fn message_keys_match_the_vectors() {
    let v = vectors();
    let mk = &v["v2"]["valid"]["get_message_keys"];
    let ck = h32(mk["conversation_key"].as_str().unwrap());
    for k in mk["keys"].as_array().unwrap() {
        let (chacha_key, chacha_nonce, hmac_key) = message_keys(&ck, &h32(k["nonce"].as_str().unwrap()));
        assert_eq!(hex::encode(chacha_key), k["chacha_key"].as_str().unwrap());
        assert_eq!(hex::encode(chacha_nonce), k["chacha_nonce"].as_str().unwrap());
        assert_eq!(hex::encode(hmac_key), k["hmac_key"].as_str().unwrap());
    }
}

#[test]
fn padding_matches_the_vectors() {
    for pair in vectors()["v2"]["valid"]["calc_padded_len"].as_array().unwrap() {
        assert_eq!(padded_len(pair[0].as_u64().unwrap() as usize), pair[1].as_u64().unwrap() as usize);
    }
}

#[test]
fn encryption_matches_the_vectors_both_ways() {
    for case in vectors()["v2"]["valid"]["encrypt_decrypt"].as_array().unwrap() {
        let ck = h32(case["conversation_key"].as_str().unwrap());
        let payload =
            encrypt(&ck, case["plaintext"].as_str().unwrap(), h32(case["nonce"].as_str().unwrap())).unwrap();
        assert_eq!(payload, case["payload"].as_str().unwrap());
        assert_eq!(decrypt(&ck, &payload).unwrap(), case["plaintext"].as_str().unwrap());
        // The conversation key is the same from either side.
        let (sec1, sec2) = (h32(case["sec1"].as_str().unwrap()), h32(case["sec2"].as_str().unwrap()));
        let public = |s: [u8; 32]| olr_lobby::Identity::from_secret(s).unwrap().public();
        assert_eq!(conversation_key(&sec1, &public(sec2)), Some(ck));
        assert_eq!(conversation_key(&sec2, &public(sec1)), Some(ck));
    }
}

#[test]
fn long_messages_match_the_vectors() {
    for case in vectors()["v2"]["valid"]["encrypt_decrypt_long_msg"].as_array().unwrap() {
        let ck = h32(case["conversation_key"].as_str().unwrap());
        let plaintext = case["pattern"].as_str().unwrap().repeat(case["repeat"].as_u64().unwrap() as usize);
        assert_eq!(hex::encode(Sha256::digest(&plaintext)), case["plaintext_sha256"].as_str().unwrap());
        let payload = encrypt(&ck, &plaintext, h32(case["nonce"].as_str().unwrap())).unwrap();
        assert_eq!(hex::encode(Sha256::digest(&payload)), case["payload_sha256"].as_str().unwrap());
        assert_eq!(decrypt(&ck, &payload).unwrap(), plaintext);
    }
}

#[test]
fn invalid_plaintext_lengths_are_refused() {
    let ck = [1; 32];
    for len in vectors()["v2"]["invalid"]["encrypt_msg_lengths"].as_array().unwrap() {
        let len = len.as_u64().unwrap() as usize;
        // The vectors also name lengths of megabytes: those are just as invalid.
        assert_eq!(encrypt(&ck, &"a".repeat(len.min(100_000)), [2; 32]), Err(Nip44Error::Length), "{len}");
    }
    assert!(encrypt(&ck, &"a".repeat(65_535), [2; 32]).is_ok());
}

#[test]
fn invalid_payloads_are_refused() {
    for case in vectors()["v2"]["invalid"]["decrypt"].as_array().unwrap() {
        let ck = h32(case["conversation_key"].as_str().unwrap());
        assert!(decrypt(&ck, case["payload"].as_str().unwrap()).is_err(), "{}", case["note"]);
    }
    let ck = [1; 32];
    assert_eq!(decrypt(&ck, "#unsupported"), Err(Nip44Error::Version));
    assert_eq!(decrypt(&ck, ""), Err(Nip44Error::Version));
    assert_eq!(
        decrypt(&ck, &"A".repeat(87_476)),
        Err(Nip44Error::Payload),
        "too long for the base64 decoder"
    );
}

/// `0x02 || nonce || chacha20(padded) || mac`, with a correct MAC over any
/// `padded` bytes: decryption then reaches the padding checks.
fn sealed(ck: &[u8; 32], nonce: [u8; 32], padded: &[u8]) -> String {
    use base64::Engine as _;
    use chacha20::cipher::{KeyIvInit, StreamCipher};
    let (key, iv, hmac_key) = message_keys(ck, &nonce);
    let mut ct = padded.to_vec();
    chacha20::ChaCha20::new(&key.into(), &iv.into()).apply_keystream(&mut ct);
    let mut mac = <hmac::Hmac<Sha256> as KeyInit>::new_from_slice(&hmac_key).unwrap();
    mac.update(&nonce);
    mac.update(&ct);
    let mut out = vec![2u8];
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    out.extend_from_slice(&mac.finalize().into_bytes());
    base64::engine::general_purpose::STANDARD.encode(out)
}

#[test]
fn bad_padding_behind_a_good_mac_is_refused() {
    let ck = [3; 32];
    let pad = |len: u16, body: &[u8], total: usize| {
        let mut p = len.to_be_bytes().to_vec();
        p.extend_from_slice(body);
        p.resize(total, 0);
        p
    };
    // The honest form decrypts.
    assert_eq!(decrypt(&ck, &sealed(&ck, [4; 32], &pad(2, b"hi", 34))).unwrap(), "hi");
    for (what, padded) in [
        ("a zero length", pad(0, b"", 34)),
        ("a length beyond the data", pad(40, b"hi", 34)),
        ("a padded length that does not fit the length", pad(2, b"hi", 66)),
    ] {
        assert_eq!(decrypt(&ck, &sealed(&ck, [4; 32], &padded)), Err(Nip44Error::Padding), "{what}");
    }
    assert!(decrypt(&ck, &sealed(&ck, [4; 32], &pad(2, &[0xff, 0xfe], 34))).is_err(), "not UTF-8");
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn arbitrary_payloads_never_panic(s in ".{0,400}", b64 in "[A-Za-z0-9+/=#]{0,400}") {
        let ck = [9; 32];
        let _ = decrypt(&ck, &s);
        let _ = decrypt(&ck, &b64);
    }

    #[test]
    fn mutated_payloads_never_panic(
        text in ".{1,300}",
        edits in prop::collection::vec((any::<usize>(), any::<u8>()), 0..6),
        cut in any::<usize>(),
        padded in prop::collection::vec(any::<u8>(), 0..600),
    ) {
        let ck = [9; 32];
        let payload = encrypt(&ck, &text, [7; 32]).unwrap();
        prop_assert_eq!(decrypt(&ck, &payload), Ok(text.clone()));
        // Edits of the base64 text, then a cut.
        let mut bytes = payload.into_bytes();
        for (at, b) in &edits {
            let i = at % bytes.len();
            bytes[i] = b"ABCxyz019+/=#\0\xc3"[usize::from(*b) % 15];
        }
        bytes.truncate(cut % (bytes.len() + 1));
        let _ = decrypt(&ck, &String::from_utf8_lossy(&bytes));
        // Arbitrary padded plaintexts behind a correct MAC.
        let _ = decrypt(&ck, &sealed(&ck, [5; 32], &padded));
    }
}
