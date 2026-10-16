use olr_netcode::Channel;
use olr_netcode::netsim::{Network, Profile};

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

/// A link in two outages at once (a peer's own network and its host's,
/// net-soak's `bad-14s3c1v-seed974389837`) is cut until both are over: the
/// second did not replace the first.
#[test]
fn outages_on_one_link_add_up() {
    let mut net = Network::new(1);
    net.connect(0, 1, Profile::LAN);
    net.outage(0, 1, 0, 100_000);
    net.outage(0, 1, 50_000, 60_000);
    net.send(70_000, 0, 1, Channel::Unreliable, vec![1]);
    // A reliable message in the first, chained through the second: after both.
    net.outage(0, 1, 90_000, 150_000);
    net.send(10_000, 0, 1, Channel::Reliable, vec![2]);
    net.send(160_000, 0, 1, Channel::Unreliable, vec![3]);
    let mut got = Vec::new();
    net.deliver_due(149_000, &mut |_, _, _, b| got.push(b[0]));
    assert!(got.is_empty(), "{got:?}");
    net.deliver_due(1_000_000, &mut |_, _, _, b| got.push(b[0]));
    assert_eq!(got, vec![2, 3]);
}

/// A one-way outage cuts what one side sends, not what the other does.
#[test]
fn a_one_way_outage_cuts_one_direction() {
    let mut net = Network::new(1);
    net.connect(0, 1, Profile::LAN);
    net.outage_one_way(0, 1, 0, 100_000);
    net.send(10_000, 0, 1, Channel::Unreliable, vec![1]);
    net.send(10_000, 1, 0, Channel::Unreliable, vec![2]);
    let mut got = Vec::new();
    net.deliver_due(50_000, &mut |_, to, _, b| got.push((to, b[0])));
    assert_eq!(got, vec![(0, 2)]);
}

#[test]
fn profiles_are_found_by_name() {
    for p in Profile::ALL {
        assert_eq!(Profile::by_name(p.name), Some(p));
    }
    assert_eq!(Profile::by_name("nope"), None);
}

/// Sends one datagram per ms with no jitter or duplicates; returns the
/// lost/received flag per datagram.
fn loss_pattern(p: Profile, n: u64) -> Vec<bool> {
    let p = Profile { jitter_ms: 0, duplicate: 0.0, ..p };
    let mut net = Network::new(5);
    net.connect(0, 1, p);
    for i in 0..n {
        net.send(i * 1000, 0, 1, Channel::Unreliable, i.to_le_bytes().to_vec());
    }
    let mut got = vec![false; n as usize];
    net.deliver_due(u64::MAX / 2, &mut |_, _, _, b| {
        got[u64::from_le_bytes(b.try_into().unwrap()) as usize] = true
    });
    got.iter().map(|g| !g).collect()
}

fn mean_loss_and_run(lost: &[bool]) -> (f64, f64) {
    let total = lost.iter().filter(|&&l| l).count();
    let runs = lost.iter().enumerate().filter(|&(i, &l)| l && (i == 0 || !lost[i - 1])).count();
    (total as f64 / lost.len() as f64, total as f64 / runs as f64)
}

#[test]
fn bursts_have_the_profiles_mean_run() {
    for (p, tol) in [(Profile::BAD, 0.2), (Profile::AWFUL, 0.6)] {
        let (loss, run) = mean_loss_and_run(&loss_pattern(p, 400_000));
        assert!((loss - p.loss).abs() < p.loss * 0.15, "{} loss {loss}", p.name);
        assert!((run - p.burst).abs() < tol, "{} run {run}", p.name);
    }
}

#[test]
fn burst_one_is_independent_loss() {
    let lost = loss_pattern(Profile::TYPICAL, 1_000_000);
    let pairs = lost.windows(2).filter(|w| w[0] && w[1]).count() as f64 / lost.len() as f64;
    assert!((pairs - 0.0001).abs() < 0.00005, "consecutive-loss rate {pairs}");
}

#[test]
fn an_outage_scheduled_ahead_drops_datagrams_due_inside_it() {
    let mut net = Network::new(1);
    net.connect(0, 1, Profile::GOOD);
    net.outage(0, 1, 100_000, 200_000);
    // Sent before the outage, due (about 15 ms later) inside it.
    net.send(95_000, 0, 1, Channel::Unreliable, vec![1]);
    net.send(10_000, 0, 1, Channel::Unreliable, vec![2]);
    let mut got = Vec::new();
    net.deliver_due(1_000_000, &mut |_, _, _, b| got.push(b[0]));
    assert_eq!(got, vec![2]);
    assert_eq!(net.received(1).dropped, 1);
}

#[test]
fn reliable_order_holds_across_an_outage() {
    let mut net = Network::new(3);
    net.connect(0, 1, Profile::BAD);
    net.outage(0, 1, 20_000, 400_000);
    for i in 0..100u8 {
        net.send(u64::from(i) * 5_000, 0, 1, Channel::Reliable, vec![i]);
    }
    let mut got = Vec::new();
    net.deliver_due(u64::MAX / 2, &mut |_, _, _, b| got.push(b[0]));
    assert_eq!(got, (0..100).collect::<Vec<u8>>());
}

#[test]
fn set_profile_changes_the_latency() {
    let mut net = Network::new(2);
    net.connect(0, 1, Profile::LAN);
    net.set_profile(0, 1, Profile::AWFUL);
    net.send(0, 0, 1, Channel::Reliable, vec![1]);
    assert!(net.next_delivery().unwrap() >= 50_000);
}

#[test]
fn stats_count_sent_and_received() {
    let mut net = Network::new(2);
    net.connect(0, 1, Profile::LAN);
    net.send(0, 0, 1, Channel::Reliable, vec![0; 10]);
    net.send(0, 1, 0, Channel::Reliable, vec![0; 3]);
    net.deliver_due(1_000_000, &mut |_, _, _, _| {});
    assert_eq!((net.sent(0).datagrams, net.sent(0).bytes), (1, 10));
    assert_eq!((net.received(1).datagrams, net.received(1).bytes), (1, 10));
    assert_eq!((net.received(0).datagrams, net.received(0).bytes), (1, 3));
}
