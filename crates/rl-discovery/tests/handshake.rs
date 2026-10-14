//! Handshake events (N45): the host's `Inbox` and the joiner's `Asker`,
//! their round trip, repeats and resends, and a hostile feed.

use std::time::{Duration, Instant};

use proptest::prelude::*;
use rl_discovery::handshake::{ANSWER_DEADLINE, Asker, Inbound, Inbox, MAX_HANDSHAKE_CONTENT, RESEND};
use rl_discovery::nip44::{conversation_key, encrypt};
use rl_discovery::{Event, KIND_HANDSHAKE};
use rl_lobby::Identity;
use rl_proto::{FINGERPRINT, MAX_SDP, PROTOCOL_VERSION, Refusal, Signal};

/// Distinct draws: SHA-256 of (seed, counter), never a repeated or all-zero value.
fn rand_from(seed: u64) -> impl FnMut() -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut n = 0u64;
    move || {
        n += 1;
        let mut h = Sha256::new();
        h.update(seed.to_le_bytes());
        h.update(n.to_le_bytes());
        h.finalize().into()
    }
}

fn offer() -> Signal {
    Signal::Offer { protocol: PROTOCOL_VERSION, fingerprint: FINGERPRINT, sdp: "v=0\r\n".into() }
}

#[test]
fn an_offer_and_its_answer_round_trip() {
    let (t0, unix) = (Instant::now(), 1_800_000_000);
    let host = Identity::from_secret([5; 32]).unwrap();
    let mut inbox = Inbox::new(host.clone());
    let mut rand = rand_from(1);
    let mut asker = Asker::new(host.public(), offer(), t0, unix, &mut rand);
    assert_ne!(asker.key(), host.public(), "a fresh key, never the player's");
    let sent = asker.poll(t0, unix, &mut rand).expect("sent at once");
    assert!(sent.tags.iter().any(|t| t[0] == "p" && t[1] == hex::encode(host.public())));
    let Inbound::Offer(req) = inbox.offer(&sent, t0, unix) else { panic!("not an offer") };
    assert_eq!(req.signal, offer());
    let mapped = Some("203.0.113.7:23400".parse().unwrap());
    let reply =
        inbox.reply(&req, Signal::Answer { sdp: "v=0 answer\r\n".into() }, mapped, t0, unix, &mut rand);
    let got = asker.take(&reply).expect("our reply");
    assert_eq!(got.signal, Signal::Answer { sdp: "v=0 answer\r\n".into() });
    assert_eq!(got.mapped, mapped);
}

#[test]
fn a_repeated_offer_gets_the_same_reply_and_resends_are_spaced() {
    let (t0, unix) = (Instant::now(), 1_800_000_000);
    let host = Identity::from_secret([5; 32]).unwrap();
    let mut inbox = Inbox::new(host.clone());
    let mut rand = rand_from(2);
    let mut asker = Asker::new(host.public(), offer(), t0, unix, &mut rand);
    let first = asker.poll(t0, unix, &mut rand).unwrap();
    assert!(asker.poll(t0 + RESEND / 2, unix, &mut rand).is_none());
    let second = asker.poll(t0 + RESEND, unix + 2, &mut rand).expect("re-sent");
    assert_ne!(first.id, second.id);
    let Inbound::Offer(req) = inbox.offer(&first, t0, unix) else { panic!() };
    let reply = inbox.reply(&req, Signal::Refused { reason: Refusal::Full }, None, t0, unix, &mut rand);
    assert_eq!(inbox.offer(&second, t0, unix + 2), Inbound::Repeat(reply));
    assert!(asker.expired(t0 + ANSWER_DEADLINE));
}

#[test]
fn a_hostile_handshake_feed_is_bounded() {
    let (t0, unix) = (Instant::now(), 1_800_000_000);
    let host = Identity::from_secret([5; 32]).unwrap();
    let other = Identity::from_secret([6; 32]).unwrap();
    let mut inbox = Inbox::new(host.clone());
    let mut rand = rand_from(3);
    // Addressed to someone else, too old, in the future: ignored before any decryption.
    let mut wrong = Asker::new(other.public(), offer(), t0, unix, &mut rand);
    assert!(matches!(inbox.offer(&wrong.poll(t0, unix, &mut rand).unwrap(), t0, unix), Inbound::Ignored(_)));
    let mut old = Asker::new(host.public(), offer(), t0, unix - 3600, &mut rand);
    assert!(matches!(
        inbox.offer(&old.poll(t0, unix - 3600, &mut rand).unwrap(), t0, unix),
        Inbound::Ignored(_)
    ));
    let mut future = Asker::new(host.public(), offer(), t0, unix + 3600, &mut rand);
    assert!(matches!(
        inbox.offer(&future.poll(t0, unix + 3600, &mut rand).unwrap(), t0, unix),
        Inbound::Ignored(_)
    ));
    // 2 000 fresh sessions (each its own key) within 2 ms: at most the burst (20) gets through.
    let mut taken = 0;
    for i in 0..2_000u64 {
        let mut a = Asker::new(host.public(), offer(), t0, unix, &mut rand_from(1_000 + i));
        let e = a.poll(t0, unix, &mut rand).unwrap();
        if matches!(inbox.offer(&e, t0 + Duration::from_micros(i), unix), Inbound::Offer(_)) {
            taken += 1;
        }
    }
    assert!(taken <= 20, "{taken}");
    // A steady 10 a second for 200 s of virtual time: every offer is taken, and memory is
    // bounded by SESSION_MEMORY (120 s × 10/s), far below the 4096 cap.
    let start = t0 + Duration::from_secs(10);
    for i in 0..2_000u64 {
        let (at, at_unix) = (start + Duration::from_millis(100 * i), unix + 10 + i / 10);
        let mut a = Asker::new(host.public(), offer(), at, at_unix, &mut rand_from(10_000 + i));
        let e = a.poll(at, at_unix, &mut rand).unwrap();
        assert!(matches!(inbox.offer(&e, at, at_unix), Inbound::Offer(_)), "offer {i}");
    }
    assert!((1_190..=1_222).contains(&inbox.sessions()), "{}", inbox.sessions());
    // A tampered content or signature is ignored, never a panic.
    let late = start + Duration::from_secs(300);
    let mut honest = Asker::new(host.public(), offer(), late, unix + 310, &mut rand);
    let mut e = honest.poll(late, unix + 310, &mut rand).unwrap();
    let flipped = if &e.content[10..11] == "A" { "B" } else { "A" };
    e.content.replace_range(10..11, flipped);
    assert!(matches!(inbox.offer(&e, late, unix + 310), Inbound::Ignored(_)));
}

/// An event from `from` to the host carrying `plaintext`, encrypted for the
/// host (any text: the JSON checks behind the decryption).
fn sealed_offer(from: &Identity, host: &Identity, plaintext: &str, unix: u64) -> Event {
    let ck = conversation_key(&from.secret(), &host.public()).unwrap();
    let content = encrypt(&ck, plaintext, [8; 32]).unwrap();
    let tags = vec![
        vec!["p".to_owned(), hex::encode(host.public())],
        vec!["expiration".to_owned(), (unix + 60).to_string()],
    ];
    Event::sign(from, unix, KIND_HANDSHAKE, tags, content, [9; 32])
}

#[test]
fn malformed_offers_are_ignored() {
    let (t0, unix) = (Instant::now(), 1_800_000_000);
    let host = Identity::from_secret([5; 32]).unwrap();
    let mut inbox = Inbox::new(host.clone());
    let session = "00112233445566778899aabbccddeeff";
    let offer_json = serde_json::to_string(&offer()).unwrap();
    let big =
        Signal::Offer { protocol: PROTOCOL_VERSION, fingerprint: FINGERPRINT, sdp: "a".repeat(MAX_SDP + 1) };
    let cases = [
        format!(r#"{{"v":2,"session":"{session}","signal":{offer_json},"mapped":null}}"#),
        format!(r#"{{"v":1,"session":"0011","signal":{offer_json},"mapped":null}}"#),
        format!(r#"{{"v":1,"session":"{session}","signal":{{"Answer":{{"sdp":"x"}}}},"mapped":null}}"#),
        format!(
            r#"{{"v":1,"session":"{session}","signal":{},"mapped":null}}"#,
            serde_json::to_string(&big).unwrap()
        ),
        format!(r#"{{"v":1,"session":"{session}","signal":{offer_json},"mapped":"[2001:db8::1]:23400"}}"#),
        format!(r#"{{"v":1,"session":"{session}","signal":{offer_json},"mapped":"not an address"}}"#),
        "not json".to_owned(),
    ];
    for (i, text) in cases.iter().enumerate() {
        let from = Identity::from_secret([40 + i as u8; 32]).unwrap();
        let e = sealed_offer(&from, &host, text, unix);
        assert!(matches!(inbox.offer(&e, t0, unix), Inbound::Ignored(_)), "{text}");
    }
    // The honest form of the same is taken.
    let from = Identity::from_secret([60; 32]).unwrap();
    let honest = format!(r#"{{"v":1,"session":"{session}","signal":{offer_json},"mapped":"203.0.113.9:1"}}"#);
    let Inbound::Offer(req) = inbox.offer(&sealed_offer(&from, &host, &honest, unix), t0, unix) else {
        panic!()
    };
    assert_eq!(req.mapped, Some("203.0.113.9:1".parse().unwrap()));
    assert_eq!(req.from, from.public());
    assert_eq!(inbox.sessions(), 1);
    // Too large a content is ignored before anything else.
    let mut e = sealed_offer(&from, &host, &honest, unix);
    e.content = "A".repeat(MAX_HANDSHAKE_CONTENT + 1);
    assert!(matches!(inbox.offer(&e, t0, unix), Inbound::Ignored(_)));
}

#[test]
fn replays_and_pending_sessions_are_not_answered_twice() {
    let (t0, unix) = (Instant::now(), 1_800_000_000);
    let host = Identity::from_secret([5; 32]).unwrap();
    let mut inbox = Inbox::new(host.clone());
    let mut rand = rand_from(4);
    let mut asker = Asker::new(host.public(), offer(), t0, unix, &mut rand);
    let first = asker.poll(t0, unix, &mut rand).unwrap();
    let Inbound::Offer(req) = inbox.offer(&first, t0, unix) else { panic!() };
    // The same event again (another relay, or a replay) is a duplicate.
    assert!(matches!(inbox.offer(&first, t0, unix), Inbound::Ignored(_)));
    // A resend before the reply: still being answered.
    let second = asker.poll(t0 + RESEND, unix + 2, &mut rand).unwrap();
    assert!(matches!(inbox.offer(&second, t0, unix + 2), Inbound::Ignored(_)));
    let reply = inbox.reply(&req, Signal::Answer { sdp: "v=0\r\n".into() }, None, t0, unix, &mut rand);
    let third = asker.poll(t0 + RESEND * 2, unix + 4, &mut rand).unwrap();
    assert_eq!(inbox.offer(&third, t0, unix + 4), Inbound::Repeat(reply.clone()));
    assert!(matches!(inbox.offer(&third, t0, unix + 4), Inbound::Ignored(_)), "each event once");
    // The joiner takes its reply once; a reply to another session is not ours.
    let mut other = Asker::new(host.public(), offer(), t0, unix, &mut rand);
    assert_eq!(other.take(&reply), None);
    assert!(asker.take(&reply).is_some());
    assert_eq!(asker.take(&reply), None, "once");
    assert!(asker.poll(t0 + RESEND * 3, unix + 6, &mut rand).is_none(), "answered: no more resends");
    assert!(!asker.expired(t0 + ANSWER_DEADLINE), "answered, not expired");
    // After SESSION_MEMORY the session is forgotten.
    let later = t0 + Duration::from_secs(121);
    let mut fresh = Asker::new(host.public(), offer(), later, unix + 121, &mut rand);
    let e = fresh.poll(later, unix + 121, &mut rand).unwrap();
    assert!(matches!(inbox.offer(&e, later, unix + 121), Inbound::Offer(_)));
    assert_eq!(inbox.sessions(), 1);
}

#[test]
fn a_reply_must_come_from_the_host() {
    let (t0, unix) = (Instant::now(), 1_800_000_000);
    let host = Identity::from_secret([5; 32]).unwrap();
    let impostor = Identity::from_secret([7; 32]).unwrap();
    let mut rand = rand_from(5);
    let mut asker = Asker::new(host.public(), offer(), t0, unix, &mut rand);
    let sent = asker.poll(t0, unix, &mut rand).unwrap();
    // An impostor cannot re-address the offer to itself: the signature covers the tags.
    let mut retagged = sent.clone();
    retagged.tags[0][1] = hex::encode(impostor.public());
    let mut fake = Inbox::new(impostor.clone());
    assert!(matches!(fake.offer(&retagged, t0, unix), Inbound::Ignored(_)));
    let mut inbox = Inbox::new(host.clone());
    let Inbound::Offer(req) = inbox.offer(&sent, t0, unix) else { panic!() };
    let mut forged = inbox.reply(&req, Signal::Answer { sdp: "x".into() }, None, t0, unix, &mut rand);
    forged.pubkey = impostor.public();
    assert_eq!(asker.take(&forged), None);
}

#[test]
fn filters_select_the_handshakes_addressed_to_us() {
    let (t0, unix) = (Instant::now(), 1_800_000_000);
    let host = Identity::from_secret([5; 32]).unwrap();
    let inbox = Inbox::new(host.clone());
    let mut rand = rand_from(6);
    let mut asker = Asker::new(host.public(), offer(), t0, unix, &mut rand);
    let sent = asker.poll(t0, unix, &mut rand).unwrap();
    assert!(inbox.filter(unix).matches(&sent));
    assert_eq!(inbox.filter(unix).since, Some(unix - 60));
    let mut inbox = inbox;
    let Inbound::Offer(req) = inbox.offer(&sent, t0, unix) else { panic!() };
    let reply = inbox.reply(&req, Signal::Answer { sdp: "x".into() }, None, t0, unix, &mut rand);
    assert!(asker.filter(unix).matches(&reply));
    assert!(!asker.filter(unix).matches(&sent));
    assert_eq!(reply.expiration(), Some(unix + 60));
    assert_eq!(sent.expiration(), Some(unix + 60));
}

#[test]
fn invalid_secrets_are_drawn_again() {
    let host = Identity::from_secret([5; 32]).unwrap();
    // All zeros, then the curve order: no valid secret; then a valid one.
    let n = hex::decode("fffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141").unwrap();
    let draws = [[0u8; 32], n.try_into().unwrap(), [0x11; 32], [0x22; 32], [0x33; 32], [0x44; 32]];
    let mut i = 0;
    let mut rand = move || {
        i += 1;
        draws[(i - 1).min(draws.len() - 1)]
    };
    let asker = Asker::new(host.public(), offer(), Instant::now(), 1_800_000_000, &mut rand);
    assert_eq!(asker.key(), Identity::from_secret([0x11; 32]).unwrap().public());
    // A source that never yields a valid secret still ends, and the join
    // sends nothing (no key derived from it, which every join would share).
    let t0 = Instant::now();
    let mut asker = Asker::new(host.public(), offer(), t0, 1_800_000_000, &mut || [0; 32]);
    assert!(asker.poll(t0, 1_800_000_000, &mut rand_from(8)).is_none());
    assert!(asker.expired(t0 + ANSWER_DEADLINE));
}

/// An SDP of exactly `MAX_SDP` bytes in lines of 61 (CRLF included).
fn maximal_sdp() -> String {
    let mut sdp = String::from("v=0\r\n");
    let line = format!("a=x-filler:{}\r\n", "0123456789abcdef".repeat(3));
    while sdp.len() < MAX_SDP {
        sdp.push_str(&line);
    }
    sdp.truncate(MAX_SDP);
    sdp
}

#[test]
fn a_maximal_offer_fits_and_an_unsendable_one_is_not_sent() {
    let (t0, unix) = (Instant::now(), 1_800_000_000);
    let host = Identity::from_secret([5; 32]).unwrap();
    let mut inbox = Inbox::new(host.clone());
    let mut rand = rand_from(9);
    let big = Signal::Offer { protocol: PROTOCOL_VERSION, fingerprint: FINGERPRINT, sdp: maximal_sdp() };
    assert_eq!(maximal_sdp().len(), MAX_SDP);
    let mut asker = Asker::new(host.public(), big.clone(), t0, unix, &mut rand);
    let e = asker.poll(t0, unix, &mut rand).expect("a maximal offer is sent");
    assert!(e.content.len() <= MAX_HANDSHAKE_CONTENT, "{}", e.content.len());
    let Inbound::Offer(req) = inbox.offer(&e, t0, unix) else { panic!("a maximal offer is taken") };
    assert_eq!(req.signal, big);
    // A maximal answer fits too.
    let answer = Signal::Answer { sdp: maximal_sdp() };
    let reply = inbox.reply(&req, answer.clone(), None, t0, unix, &mut rand);
    assert!(reply.content.len() <= MAX_HANDSHAKE_CONTENT);
    assert_eq!(asker.take(&reply).map(|r| r.signal), Some(answer));
    // An SDP whose escapes overflow the content limit: the offer is never
    // sent, and the join expires.
    let escaped =
        Signal::Offer { protocol: PROTOCOL_VERSION, fingerprint: FINGERPRINT, sdp: "\n".repeat(MAX_SDP) };
    let mut asker = Asker::new(host.public(), escaped, t0, unix, &mut rand);
    assert!(asker.poll(t0, unix, &mut rand).is_none());
    assert!(asker.expired(t0 + ANSWER_DEADLINE));
    // An answer that cannot be sent is replaced by a refusal the joiner reads.
    let mut asker = Asker::new(host.public(), offer(), t0, unix, &mut rand);
    let Inbound::Offer(req) = inbox.offer(&asker.poll(t0, unix, &mut rand).unwrap(), t0, unix) else {
        panic!()
    };
    let reply = inbox.reply(&req, Signal::Answer { sdp: "\n".repeat(MAX_SDP) }, None, t0, unix, &mut rand);
    assert!(reply.content.len() <= MAX_HANDSHAKE_CONTENT);
    assert_eq!(asker.take(&reply).map(|r| r.signal), Some(Signal::Refused { reason: Refusal::Malformed }));
}

#[test]
fn only_hostile_or_over_limit_events_are_counted() {
    let (t0, unix) = (Instant::now(), 1_800_000_000);
    let host = Identity::from_secret([5; 32]).unwrap();
    let mut inbox = Inbox::new(host.clone());
    let mut rand = rand_from(10);
    let mut asker = Asker::new(host.public(), offer(), t0, unix, &mut rand);
    let first = asker.poll(t0, unix, &mut rand).unwrap();
    assert!(matches!(inbox.offer(&first, t0, unix), Inbound::Offer(_)));
    // Every relay's copy, and a resend before the reply: honest redundancy.
    for _ in 0..7 {
        assert_eq!(inbox.offer(&first, t0, unix), Inbound::Ignored("duplicate"));
    }
    let resend = asker.poll(t0 + RESEND, unix + 2, &mut rand).unwrap();
    assert_eq!(inbox.offer(&resend, t0, unix + 2), Inbound::Ignored("being answered"));
    assert_eq!(inbox.ignored(), 0);
    // A tampered event and a stale one count.
    let mut forged = asker.poll(t0 + RESEND * 2, unix + 4, &mut rand).unwrap();
    forged.content.push('A');
    assert!(matches!(inbox.offer(&forged, t0, unix + 2), Inbound::Ignored(_)));
    assert!(matches!(inbox.offer(&first, t0, unix + 3600), Inbound::Ignored(_)));
    assert_eq!(inbox.ignored(), 2);
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn arbitrary_handshake_events_never_panic(
        content in ".{0,300}",
        plaintext in ".{1,300}",
        tags in prop::collection::vec(prop::collection::vec("[a-z0-9]{0,70}", 0..3), 0..4),
        skew in -100i64..100,
        seed in any::<u64>(),
    ) {
        let (t0, unix) = (Instant::now(), 1_800_000_000u64);
        let host = Identity::from_secret([5; 32]).unwrap();
        let mut inbox = Inbox::new(host.clone());
        let mut rand = rand_from(seed);
        let from = Identity::from_secret(rand()).unwrap();
        let at = unix.saturating_add_signed(skew);
        let p = vec!["p".to_owned(), hex::encode(host.public())];
        // Arbitrary content, arbitrary tags, and arbitrary text encrypted for the host.
        let raw = Event::sign(&from, at, KIND_HANDSHAKE, vec![p.clone()], content.clone(), rand());
        let tagged = Event::sign(&from, at, KIND_HANDSHAKE, tags.clone(), content, rand());
        let sealed = sealed_offer(&from, &host, &plaintext, at);
        for e in [&raw, &tagged, &sealed] {
            let _ = inbox.offer(e, t0, unix);
        }
        // The joiner's side: the same, as replies from the host.
        let mut asker = Asker::new(host.public(), offer(), t0, unix, &mut rand);
        let to_asker = vec!["p".to_owned(), hex::encode(asker.key())];
        let ck = conversation_key(&host.secret(), &asker.key()).unwrap();
        let reply = Event::sign(&host, at, KIND_HANDSHAKE, vec![to_asker.clone()], encrypt(&ck, &plaintext, rand()).unwrap(), rand());
        let raw_reply = Event::sign(&host, at, KIND_HANDSHAKE, tags, plaintext, rand());
        prop_assert_eq!(asker.take(&reply), None);
        prop_assert_eq!(asker.take(&raw_reply), None);
    }
}

#[test]
fn resends_are_bounded_and_forgeries_use_none() {
    let (t0, unix) = (Instant::now(), 1_800_000_000);
    let host = Identity::from_secret([5; 32]).unwrap();
    let mut inbox = Inbox::new(host.clone());
    // Askers from the same seed share the key and the session: each poll
    // is another event of the one session.
    let mut rand = rand_from(70);
    let mut event =
        || Asker::new(host.public(), offer(), t0, unix, &mut rand_from(7)).poll(t0, unix, &mut rand).unwrap();
    let first = event();
    let Inbound::Offer(req) = inbox.offer(&first, t0, unix) else { panic!() };
    let reply =
        inbox.reply(&req, Signal::Answer { sdp: "v=0\r\n".into() }, None, t0, unix, &mut rand_from(71));
    // Forgeries naming the joiner's key (another content under its
    // signature) are refused and use up nothing.
    for i in 0..20 {
        let mut forged = event();
        forged.content = format!("forged {i}");
        assert_eq!(inbox.offer(&forged, t0, unix), Inbound::Ignored("bad signature"));
    }
    for i in 1..=5 {
        assert_eq!(inbox.offer(&event(), t0, unix), Inbound::Repeat(reply.clone()), "resend {i}");
    }
    assert_eq!(inbox.offer(&event(), t0, unix), Inbound::Ignored("too many repeats"));
}
