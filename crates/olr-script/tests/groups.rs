//! The factory's plant works: scripts set the level's glow groups, and
//! read a copy's centre.

use std::sync::Arc;

use olr_sim::{LiveGroup, LiveLevel};

use crate::common::{copy, play, read, world};

const ABC: &[&str] = &["A", "B", "C"];

fn level(scripts: &[&str], level_script: Option<u16>, copies: Vec<olr_sim::LiveCopy>) -> Arc<LiveLevel> {
    let level = LiveLevel::new(copies, scripts.iter().map(|s| (*s).to_string()).collect(), level_script, 0);
    Arc::new(level.with_groups(vec!["lamps".into(), "emergency".into()]))
}

#[test]
fn a_level_script_sets_and_resets_glow_groups() {
    let text = "fn start() {\n\
                  this.group_intensity(\"lamps\", 250);\n\
                  this.group_colour(\"emergency\", \"#ff2010\");\n\
                  this.after(5, \"back\");\n\
                }\n\
                fn back() { this.group_reset(\"lamps\"); }";
    let mut w = world(level(&[text], Some(0), vec![copy("a", 20, ABC, vec![])]));
    assert!(play(&mut w, 1).iter().all(Vec::is_empty));
    let groups = w.live().unwrap().groups().to_vec();
    assert_eq!(
        groups,
        [LiveGroup { intensity: 250, colour: -1 }, LiveGroup { intensity: -1, colour: 0xff2010 }]
    );
    play(&mut w, 10);
    assert_eq!(w.live().unwrap().groups()[0], LiveGroup::OWN);
}

#[test]
fn an_unknown_group_or_a_bad_value_is_a_script_error() {
    for call in [
        "this.group_intensity(\"lampz\", 0)",
        "this.group_intensity(\"lamps\", 1001)",
        "this.group_colour(\"lamps\", \"red\")",
    ] {
        let text = format!("fn start() {{ {call}; }}");
        let mut w = world(level(&[&text], Some(0), vec![copy("a", 20, ABC, vec![])]));
        assert_eq!(play(&mut w, 1)[0].len(), 1, "{call}");
        assert!(w.live().unwrap().groups().iter().all(|g| *g == LiveGroup::OWN), "{call}");
    }
}

#[test]
fn centre_gives_a_copys_middle() {
    // `a` is 4 x 4 at (20, 40): its centre (22, 42); `b` at (60, 40),
    // moved 10 px right by its `x`, reads it from `c`.
    let a = "fn start() { this.set(\"v\", this.centre_x() * 1000 + this.centre_y()); }";
    let c = "fn start() { this.set(this.copy(\"b\"), \"x\", 10); }\n\
             fn tick() { let b = this.copy(\"b\"); this.set(\"v\", this.centre_x(b) * 1000 + this.centre_y(b)); }";
    let copies = vec![copy("a", 20, ABC, vec![0]), copy("b", 60, ABC, vec![]), copy("c", 100, ABC, vec![1])];
    let mut w = world(level(&[a, c], None, copies));
    play(&mut w, 2);
    assert_eq!(read(&w, "a").1, 22_042);
    assert_eq!(read(&w, "c").1, 72_042);
}
