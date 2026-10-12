use rl_netcode::Channel;
use rl_netcode::netsim::{Network, Profile};

fn run(profile: Profile, n: u64, ch: Channel) -> Vec<(u64, u64)> {
    let mut net = Network::new(42);
    net.connect(0, 1, profile);
    let mut got = Vec::new();
    for i in 0..n {
        let now = i * 1000;
        net.send(now, 0, 1, ch, i.to_le_bytes().to_vec());
        net.deliver_due(now, &mut |_, _, _, b| got.push((u64::from_le_bytes(b.try_into().unwrap()), now)));
    }
    let end = n * 1000 + 10_000_000;
    net.deliver_due(end, &mut |_, _, _, b| got.push((u64::from_le_bytes(b.try_into().unwrap()), end)));
    got
}

#[test]
fn loss_matches_the_profile() {
    let got = run(Profile::BAD, 100_000, Channel::Unreliable);
    let distinct: std::collections::BTreeSet<u64> = got.iter().map(|g| g.0).collect();
    let lost = 1.0 - distinct.len() as f64 / 100_000.0;
    assert!((lost - 0.05).abs() < 0.01, "lost {lost}");
    assert!(got.len() > distinct.len(), "duplicates appear");
}

#[test]
fn latency_is_half_the_ping() {
    let mut net = Network::new(7);
    net.connect(0, 1, Profile::TYPICAL);
    let mut total = 0u64;
    let mut count = 0u64;
    for i in 0..20_000u64 {
        let sent = i * 1000;
        net.send(sent, 0, 1, Channel::Unreliable, sent.to_le_bytes().to_vec());
        // Step finely so the delivery time is the true arrival time.
        for t in sent..sent + 1000 {
            if net.next_delivery().is_some_and(|d| d <= t) {
                net.deliver_due(t, &mut |_, _, _, b| {
                    total += t - u64::from_le_bytes(b.try_into().unwrap());
                    count += 1;
                });
            }
        }
    }
    net.deliver_due(u64::MAX / 2, &mut |_, _, _, _| {});
    let mean_ms = total as f64 / count as f64 / 1000.0;
    assert!((mean_ms - 40.0).abs() < 2.0, "mean one-way {mean_ms} ms");
}

#[test]
fn reliable_messages_arrive_in_order_and_all() {
    let got = run(Profile::AWFUL, 5_000, Channel::Reliable);
    let order: Vec<u64> = got.iter().map(|g| g.0).collect();
    assert_eq!(order, (0..5_000).collect::<Vec<_>>());
}

#[test]
fn the_same_seed_gives_the_same_deliveries() {
    assert_eq!(run(Profile::BAD, 3_000, Channel::Unreliable), run(Profile::BAD, 3_000, Channel::Unreliable));
}

#[test]
fn an_outage_drops_datagrams_and_delays_messages() {
    let mut net = Network::new(1);
    net.connect(0, 1, Profile::LAN);
    net.outage(0, 1, 0, 50_000);
    net.send(10_000, 0, 1, Channel::Unreliable, vec![1]);
    net.send(10_000, 0, 1, Channel::Reliable, vec![2]);
    let mut got = Vec::new();
    net.deliver_due(49_000, &mut |_, _, _, b| got.push(b[0]));
    assert!(got.is_empty());
    net.deliver_due(60_000, &mut |_, _, _, b| got.push(b[0]));
    assert_eq!(got, vec![2]);
}

#[test]
fn profiles_are_found_by_name() {
    for p in Profile::ALL {
        assert_eq!(Profile::by_name(p.name), Some(p));
    }
    assert_eq!(Profile::by_name("nope"), None);
}
