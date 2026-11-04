//! Issue #89: the name checks of a level's scripts (call 20).

use olr_script::{CopyNames, RunScript, ScriptNames, check_level};

fn names() -> ScriptNames {
    let copy = |id: &str, states: &[&str], exposed: &[&str]| CopyNames {
        id: id.into(),
        states: states.iter().map(|s| s.to_string()).collect(),
        exposed: exposed.iter().map(|s| s.to_string()).collect(),
        sounds: Vec::new(),
    };
    ScriptNames {
        copies: vec![
            copy("console", &["Normal", "Alarm"], &["screen"]),
            copy("press", &["Up", "Down"], &["head_y"]),
        ],
        groups: Some(vec!["lamps".into()]),
    }
}

/// `(script, line, message)` of each issue.
fn issues(scripts: &[RunScript<'_>]) -> Vec<(usize, u32, String)> {
    issues_of(&names(), scripts)
}

fn issues_of(names: &ScriptNames, scripts: &[RunScript<'_>]) -> Vec<(usize, u32, String)> {
    check_level(names, scripts).into_iter().map(|i| (i.script, i.issue.line, i.issue.message)).collect()
}

#[test]
fn check_level_finds_unknown_copies_states_names_and_bad_handlers() {
    let good = "fn start() {\n\
                  this.set_state(\"Alarm\");\n\
                  let p = this.copy(\"press\");\n\
                  this.move_to(p, \"head_y\", this.max(p, \"head_y\"), 40);\n\
                  this.set_state(this.copy(\"press\"), \"Up\");\n\
                  this.set(\"screen\", 3);\n\
                }\n\
                fn arrived(name) { this.stop(name); }\n\
                fn entered(s) { if this.moving(this.me(), \"screen\") { this.set_state(s); } }";
    let level = "fn tick() {\n\
                   let c = this.copy(\"console\");\n\
                   if this.moving(this.copy(\"press\"), \"head_y\") { this.set_state(c, \"Alarm\"); }\n\
                 }";
    assert_eq!(
        issues(&[RunScript { copy: Some("console"), text: good }, RunScript { copy: None, text: level }]),
        []
    );

    let bad = "fn start() {\n\
                 this.set_state(\"Alarmm\");\n\
                 let p = this.copy(\"crane\");\n\
                 this.get(this.copy(\"press\"), \"claw_x\");\n\
                 let q = this.copy(\"press\");\n\
                 this.set_state(q, \"Normal\");\n\
                 this.min(\"head_y\");\n\
               }\n\
               fn tick(x) { }\n\
               fn arrived() { }\n\
               fn spawned(w) { this.set(\"sc\" + \"reen\", 1); }";
    let level = "fn start() {\n  this.set_state(\"Alarm\");\n  this.copy(\"console\");\n}";
    let got = issues(&[
        RunScript { copy: Some("console"), text: bad },
        RunScript { copy: None, text: level },
        RunScript { copy: Some("press"), text: "fn start( {" },
    ]);
    let want = [
        (0, 2, "copy console has no state \"Alarmm\""),
        (0, 3, "no live copy \"crane\""),
        (0, 4, "copy press has no exposed value \"claw_x\""),
        (0, 6, "copy press has no state \"Normal\""),
        (0, 7, "copy console has no exposed value \"head_y\""),
        (0, 9, "handler tick() takes 0 parameters, not 1 parameter: it never runs"),
        (0, 10, "handler arrived() takes 1 parameter, not 0 parameters: it never runs"),
        (1, 2, "the level script has no copy of its own: this.set_state(…) names none"),
    ];
    let want: Vec<(usize, u32, String)> = want.iter().map(|&(s, l, m)| (s, l, m.to_string())).collect();
    assert_eq!(got[..got.len() - 1], want[..], "{got:#?}");
    // A syntax error is the script's only issue.
    let last = got.last().unwrap();
    assert_eq!((last.0, last.1), (2, 1), "{last:?}");
    // A handle that is no literal's (a parameter, a variable bound to two
    // copies) is not checked.
    let unknown = "fn killed(v, k) { let c = this.copy(\"press\"); let c = this.copy(\"console\");\n\
                   this.set_state(c, \"Up\"); this.set_state(v, \"Nothing\"); }";
    assert_eq!(issues(&[RunScript { copy: Some("console"), text: unknown }]), []);
}

/// Issue #92: `play`'s literal names a sound of the copy it acts on.
#[test]
fn check_finds_an_unknown_sound_name() {
    let mut names = names();
    names.copies[1].sounds = vec!["hum.wav".into(), "tick.wav".into()];
    let press = "fn start() {\n\
                   this.play(\"hum.wav\");\n\
                   this.play(\"hum.ogg\");\n\
                   this.play(this.copy(\"console\"), \"tick.wav\");\n\
                   this.play(this.me(), \"tick.wav\");\n\
                 }";
    let level = "fn tick() {\n  this.play(this.copy(\"press\"), \"tock.wav\");\n  this.play(\"hum.wav\");\n}";
    let got: Vec<(usize, u32, String)> = check_level(
        &names,
        &[RunScript { copy: Some("press"), text: press }, RunScript { copy: None, text: level }],
    )
    .into_iter()
    .map(|i| (i.script, i.issue.line, i.issue.message))
    .collect();
    let want = [
        (0, 3, "copy press has no sound \"hum.ogg\""),
        (0, 4, "copy console has no sound \"tick.wav\""),
        (1, 2, "copy press has no sound \"tock.wav\""),
        (1, 3, "the level script has no copy of its own: this.play(…) names none"),
    ];
    let want: Vec<(usize, u32, String)> = want.iter().map(|&(s, l, m)| (s, l, m.to_string())).collect();
    assert_eq!(got, want);
}

/// The factory's plant works: a literal group id names one of the
/// level's glow groups; unchecked when the groups are unknown.
#[test]
fn check_level_finds_unknown_glow_groups() {
    let text = "fn start() {\n\
                  this.group_intensity(\"lamps\", 0);\n\
                  this.group_colour(\"lampz\", \"#ff0000\");\n\
                  this.group_reset(\"emergency\");\n\
                }";
    let scripts = [RunScript { copy: None, text }];
    assert_eq!(
        issues(&scripts),
        [
            (0, 3, "the level has no glow group \"lampz\"".to_string()),
            (0, 4, "the level has no glow group \"emergency\"".to_string()),
        ]
    );
    let unchecked = ScriptNames { groups: None, ..names() };
    assert_eq!(issues_of(&unchecked, &scripts), []);
}
