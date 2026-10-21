//! The release list as GitHub serves it, and the release each build on
//! each channel is offered.

use olr_update::release::{Channel, newest, parse_releases};
use olr_update::{Version, channel, flatpak};

/// GitHub's list (trimmed to the fields read, plus some it has), newest
/// first as GitHub orders it, with a draft and a tag that is no version.
const LIST: &str = r#"[
  {"tag_name": "v0.2.1-dev.2.0abc123", "name": "Dev 0.2.1", "body": "dev notes", "draft": false,
   "prerelease": true, "html_url": "https://github.com/o/r/releases/tag/v0.2.1-dev.2.0abc123",
   "assets": [{"name": "open-liero-reloaded-0.2.1-dev.2.0abc123-windows-x64.zip",
               "browser_download_url": "https://github.com/o/r/releases/download/x/a.zip", "size": 23000000,
               "uploader": {"login": "m"}}]},
  {"tag_name": "v9.9.9", "name": "Draft", "draft": true, "prerelease": false},
  {"tag_name": "v0.2.0", "name": null, "body": null, "draft": false, "prerelease": false,
   "html_url": "https://github.com/o/r/releases/tag/v0.2.0", "assets": []},
  {"tag_name": "v0.2.0-dev.330.0145601", "name": "", "draft": false, "prerelease": true, "assets": [
    {"name": "x.zip", "browser_download_url": "http://insecure.example/x.zip", "size": 1}]},
  {"tag_name": "v0.2.0-dev.318.014560b", "draft": false, "prerelease": true},
  {"tag_name": "local-play-demo", "draft": false, "prerelease": false},
  {"tag_name": "v0.1.0", "name": "Open Liero Reloaded 0.1.0", "draft": false, "prerelease": false}
]"#;

fn v(s: &str) -> Version {
    Version::parse(s).unwrap()
}

#[test]
fn the_list_is_read_without_drafts_or_odd_tags() {
    let r = parse_releases(LIST.as_bytes()).unwrap();
    let tags: Vec<&str> = r.iter().map(|r| r.tag.as_str()).collect();
    assert_eq!(
        tags,
        ["v0.2.1-dev.2.0abc123", "v0.2.0", "v0.2.0-dev.330.0145601", "v0.2.0-dev.318.014560b", "v0.1.0"]
    );
    assert_eq!(r[0].title, "Dev 0.2.1");
    assert_eq!(r[0].notes, "dev notes");
    assert_eq!(r[0].version_text(), "0.2.1-dev.2.0abc123");
    assert_eq!(
        r[0].asset("open-liero-reloaded-0.2.1-dev.2.0abc123-windows-x64.zip").unwrap().size,
        23_000_000
    );
    // A release without a title is called by its tag; no notes are empty.
    assert_eq!((r[1].title.as_str(), r[1].notes.as_str()), ("v0.2.0", ""));
    // Only https downloads are kept.
    assert!(r[2].assets.is_empty());
    for bad in ["", "{}", "[1]", "not json", "[{\"tag_name\": 5}]"] {
        assert!(parse_releases(bad.as_bytes()).is_err(), "{bad}");
    }
    assert!(parse_releases(&vec![b' '; olr_update::release::MAX_LIST_BYTES + 1]).is_err());
}

#[test]
fn each_channel_offers_its_newest_never_a_downgrade() {
    let r = parse_releases(LIST.as_bytes()).unwrap();
    let offer = |build: &str, ch: Channel| newest(&r, &v(build), ch).map(|r| r.tag.as_str());
    // A dev build on the pre-release channel: the newest of all.
    assert_eq!(offer("0.2.0-dev.318.014560b", Channel::PreRelease), Some("v0.2.1-dev.2.0abc123"));
    // On stable: the newest final.
    assert_eq!(offer("0.2.0-dev.318.014560b", Channel::Stable), Some("v0.2.0"));
    assert_eq!(offer("0.1.0", Channel::Stable), Some("v0.2.0"));
    // Up to date, and never back: a dev build above the last final leaving
    // the pre-release channel stays until a newer final.
    assert_eq!(offer("0.2.1-dev.2.0abc123", Channel::PreRelease), None);
    assert_eq!(offer("0.2.1-dev.1.0000000", Channel::Stable), None);
    assert_eq!(offer("0.2.1-dev.1.0000000", Channel::PreRelease), Some("v0.2.1-dev.2.0abc123"));
    assert_eq!(offer("0.3.0", Channel::PreRelease), None);
    // The same count is no update, whatever the commit.
    let same = newest(&r, &v("0.2.1-dev.2.fffffff"), Channel::PreRelease);
    assert_eq!(same, None);
}

#[test]
fn the_channel_follows_the_build_until_set_and_the_branch_under_flatpak() {
    let dev = v("0.2.0-dev.318.014560b");
    let fin = v("0.1.0");
    assert_eq!(channel(None, &dev, None), Channel::PreRelease);
    assert_eq!(channel(None, &fin, None), Channel::Stable);
    assert_eq!(channel(Some(false), &dev, None), Channel::Stable);
    assert_eq!(channel(Some(true), &fin, None), Channel::PreRelease);
    let info = |branch: &str| flatpak::FlatpakInfo {
        app_id: flatpak::APP_ID.into(),
        branch: branch.into(),
        user: None,
    };
    assert_eq!(channel(Some(true), &fin, Some(&info("stable"))), Channel::Stable);
    assert_eq!(channel(Some(false), &fin, Some(&info("beta"))), Channel::PreRelease);
    // An older bundle's master is on no branch's channel: the setting's.
    assert_eq!(channel(None, &dev, Some(&info("master"))), Channel::PreRelease);
}
