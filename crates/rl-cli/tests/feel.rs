//! The feel numbers (networking spec §9.2) on a short run: their
//! definitions make sense. The gate on the full numbers is `cargo xtask
//! net-feel` (in `full`).

use crate::common;

use rl_cli::feel::{ProfileFeel, measure_one};
use rl_cli::netsim::NetSpec;

#[test]
fn feel_numbers_make_sense_on_a_short_run() {
    let Some(c) = common::content() else { return };
    let mut spec = rl_cli::feel::feel_spec("typical", 1);
    spec.ticks = 600;
    let f: ProfileFeel = measure_one(&c, &spec).unwrap();
    assert!(f.rollback_p50 >= 1 && f.rollback_p99 <= 28, "{f:?}");
    assert!(f.rollback_p50 <= f.rollback_p99);
    assert_eq!(f.stalls, 0);
    assert!(f.down_bytes_per_s > 0 && f.down_bytes_per_s < 24_576);
    let lan = measure_one(&c, &NetSpec { profile: "lan".into(), ..spec }).unwrap();
    assert!(lan.rollback_p99 <= f.rollback_p99);
}
