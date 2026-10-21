//! GitHub's release list (`GET /repos/<owner>/<repo>/releases`) and the
//! release a player is offered: the newest in their channel that is newer
//! than their build. Drafts, tags that are no version and `.dirty` builds
//! are never offered, nor anything at or below the build (no downgrade).

use serde::Deserialize;

use crate::version::Version;

/// The most bytes of a release list read (30 releases with long notes fit).
pub const MAX_LIST_BYTES: usize = 2 * 1024 * 1024;

/// A file attached to a release.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    /// `browser_download_url`: GitHub redirects it to its storage.
    pub url: String,
    pub size: u64,
}

/// A published release.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    /// The tag, `v<version>`.
    pub tag: String,
    pub version: Version,
    /// The release's title (its tag when it has none).
    pub title: String,
    /// The release's notes (GitHub's Markdown).
    pub notes: String,
    /// Marked pre-release on GitHub.
    pub prerelease: bool,
    /// Its page (`html_url`).
    pub page: String,
    pub assets: Vec<Asset>,
}

impl Release {
    /// The version as `dist` named the files: the tag without its `v`.
    pub fn version_text(&self) -> &str {
        self.tag.strip_prefix('v').unwrap_or(&self.tag)
    }

    /// The asset named `name`.
    pub fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name == name)
    }
}

/// Which releases a player is offered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    /// Final releases only (not marked pre-release, no `-…` version).
    Stable,
    /// Final releases and pre-releases.
    PreRelease,
}

impl Channel {
    /// The channel a build is on by default: a pre-release build's is
    /// pre-releases, a final build's stable.
    pub fn of_build(build: &Version) -> Channel {
        if build.is_prerelease() { Channel::PreRelease } else { Channel::Stable }
    }

    /// The channel of the setting "Include pre-releases" (`None`: never
    /// changed, so the build's).
    pub fn of_setting(include_prereleases: Option<bool>, build: &Version) -> Channel {
        match include_prereleases {
            Some(true) => Channel::PreRelease,
            Some(false) => Channel::Stable,
            None => Channel::of_build(build),
        }
    }

    pub fn includes(self, r: &Release) -> bool {
        match self {
            Channel::Stable => !r.prerelease && !r.version.is_prerelease(),
            Channel::PreRelease => true,
        }
    }
}

#[derive(Deserialize)]
struct RawRelease {
    #[serde(default)]
    tag_name: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    html_url: Option<String>,
    #[serde(default)]
    assets: Vec<RawAsset>,
}

#[derive(Deserialize)]
struct RawAsset {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    browser_download_url: Option<String>,
    #[serde(default)]
    size: u64,
}

/// The releases in GitHub's JSON list; drafts and tags that are no
/// version are left out. Any JSON that is no list is an error.
pub fn parse_releases(json: &[u8]) -> Result<Vec<Release>, String> {
    if json.len() > MAX_LIST_BYTES {
        return Err(format!("the release list is over {MAX_LIST_BYTES} bytes"));
    }
    let raw: Vec<RawRelease> =
        serde_json::from_slice(json).map_err(|e| format!("the release list is not GitHub's: {e}"))?;
    Ok(raw
        .into_iter()
        .filter(|r| !r.draft)
        .filter_map(|r| {
            let tag = r.tag_name?;
            let version = Version::parse(&tag)?;
            let title = r.name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| tag.clone());
            let assets = r
                .assets
                .into_iter()
                .filter_map(|a| {
                    let url = a.browser_download_url.filter(|u| u.starts_with("https://"))?;
                    Some(Asset { name: a.name?, url, size: a.size })
                })
                .collect();
            Some(Release {
                tag,
                version,
                title,
                notes: r.body.unwrap_or_default(),
                prerelease: r.prerelease,
                page: r.html_url.filter(|u| u.starts_with("https://")).unwrap_or_default(),
                assets,
            })
        })
        .collect())
}

/// The release to offer a build `current` on `channel`: the newest one
/// newer than it, `None` when there is none (never a downgrade).
pub fn newest<'a>(releases: &'a [Release], current: &Version, channel: Channel) -> Option<&'a Release> {
    releases
        .iter()
        .filter(|r| channel.includes(r) && !r.version.is_dirty() && r.version.is_newer_than(current))
        .fold(None, |best: Option<&Release>, r| match best {
            Some(b) if !r.version.is_newer_than(&b.version) => Some(b),
            _ => Some(r),
        })
}

/// The release zip's name for Windows, as `cargo xtask dist` writes it.
pub fn windows_zip_name(version: &str) -> String {
    format!("{}-windows-x64.zip", folder_name(version))
}

/// The Linux Flatpak bundle's name, as `cargo xtask publish` writes it.
pub fn flatpak_bundle_name(version: &str) -> String {
    format!("{}-linux-x64.flatpak", folder_name(version))
}

/// The zip's one top folder.
pub fn folder_name(version: &str) -> String {
    format!("open-liero-reloaded-{version}")
}

/// A signed manifest's name beside the file it covers.
pub fn signature_name(file: &str) -> String {
    format!("{file}.sig")
}
