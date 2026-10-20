use std::path::Path;

use olr_runner::install::{service_unit, session_settings, timer_unit};

#[test]
fn units_exact() {
    assert_eq!(
        service_unit("/home/m/.cargo/bin:/usr/bin"),
        "[Unit]\nDescription=OLR issue runner: one poll-and-work pass\n[Service]\nType=oneshot\n\
         ExecStart=%h/.local/bin/olr-runner tick\nTimeoutStartSec=infinity\n\
         Environment=PATH=/home/m/.cargo/bin:/usr/bin\nNice=19\nIOSchedulingClass=idle\n"
    );
    assert_eq!(
        timer_unit(),
        "[Unit]\nDescription=OLR issue runner every 15 minutes\n[Timer]\nOnBootSec=2min\n\
         OnUnitInactiveSec=15min\n[Install]\nWantedBy=timers.target\n"
    );
}

#[test]
fn session_settings_keys() {
    let v = session_settings(Path::new("/h/.local/share/olr-runner/work"), Path::new("/h"));
    assert_eq!(v["model"], "opus");
    assert_eq!(v["effortLevel"], "high");
    assert_eq!(v["autoMemoryEnabled"], false);
    assert_eq!(v["promptCacheTtl"], "1h");
    assert_eq!(v["autoCompactWindow"], 400000);
    assert_eq!(v["enabledPlugins"], serde_json::json!({}));
    assert_eq!(v["hooks"]["PreToolUse"][0]["hooks"][0]["command"], "rtk hook claude");
    assert_eq!(v["permissions"]["defaultMode"], "bypassPermissions");
    let deny: Vec<&str> =
        v["permissions"]["deny"].as_array().unwrap().iter().map(|d| d.as_str().unwrap()).collect();
    assert!(deny.contains(&"Bash(git push*)") && deny.contains(&"Bash(gh *)"));
    assert!(deny.contains(&"Read(~/.config/gh/**)"));
    for rule in [
        "WebFetch",
        "WebSearch",
        "Read(~/.config/olr-runner/**)",
        "Edit(~/.config/**)",
        "Edit(~/.local/bin/**)",
        "Edit(~/.local/state/**)",
        "Edit(~/.claude/**)",
        "Edit(~/.ssh/**)",
        "Edit(~/.bashrc)",
        "Edit(~/.profile)",
        "Edit(~/.gitconfig)",
    ] {
        assert!(deny.contains(&rule), "{rule} not denied");
    }
    let sb = &v["sandbox"];
    assert_eq!((sb["enabled"].clone(), sb["failIfUnavailable"].clone()), (true.into(), true.into()));
    assert_eq!(sb["allowUnsandboxedCommands"], false);
    assert_eq!(sb["network"]["allowedDomains"], serde_json::json!(["crates.io", "*.crates.io"]));
    assert_eq!(sb["network"]["strictAllowlist"], true);
    let write: Vec<&str> =
        sb["filesystem"]["allowWrite"].as_array().unwrap().iter().map(|d| d.as_str().unwrap()).collect();
    assert_eq!(write, ["/h/.local/share/olr-runner/work", "/h/.cargo"]);
    assert_eq!(
        sb["filesystem"]["denyRead"],
        serde_json::json!(["/h/.config/gh", "/h/.git-credentials", "/h/.ssh", "/h/.config/olr-runner"])
    );
}
