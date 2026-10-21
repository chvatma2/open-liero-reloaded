//! `cargo xtask publish` (issue #4): publishes the build `dist` just made,
//! in one step, so that a player who sees the update notice can install:
//!
//! 0. refuses a `.dirty` build, a dev build whose base version is not
//!    above the latest final release tag (bump the workspace version after
//!    each final release), a signing key or GPG keyring that does not match
//!    the committed public halves, and missing inputs;
//! 1. signs the Windows zip's manifest (`<zip>.sig`, `olr_update::manifest`);
//! 2. commits the Flatpak build (`target/flatpak/repo`'s `master`) into the
//!    local GPG-signed repository on its branch(es) (a pre-release on
//!    `beta`, a final release on `stable` and `beta`), warning when its
//!    permissions differ from the branch's previous build, prunes it
//!    (`--keep` commits a branch) and updates its summary, builds the
//!    release's bundle from it (`--repo-url`, `--gpg-keys`: installing it
//!    subscribes to the repository), pushes the repository with
//!    `.nojekyll`, the `.flatpakrepo`, a `.flatpakref` a branch and an
//!    index page to the Pages repository as one fresh commit, force-pushed,
//!    and waits until the live repository's summary is the new one;
//! 3. creates the GitHub release `v<version>` (a pre-release for `-dev.`)
//!    with the zip, its signature and the bundle, the notes from `--notes`.
//!
//! Everything it touches is a parameter ([`Config`]): the tests publish
//! into temporary directories, a local bare git repository as Pages and a
//! directory as the GitHub release.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use olr_update::Version;
use olr_update::flatpak::{self, APP_ID, BETA, REPO_URL, STABLE};
use olr_update::manifest::{Manifest, SigningKey, public_key_from_hex};
use olr_update::release;

use crate::root;

/// The committed public half of the update signing key.
pub const SIGNING_PUBLIC: &str = "tools/release/update-signing.pub";
/// The committed public Flatpak repository key (armored).
pub const GPG_PUBLIC: &str = "tools/flatpak/olr-flatpak.gpg";
/// The Pages repository.
pub const PAGES_REMOTE: &str = "git@github.com:openlieroreloaded/flatpak.git";
/// The branch Pages serves.
pub const PAGES_BRANCH: &str = "main";
/// Commits kept a branch in the Flatpak repository.
pub const KEEP: u32 = 3;
/// How long the live repository may take to show the new summary.
pub const WAIT: Duration = Duration::from_secs(30 * 60);
/// How often it is looked at meanwhile (Pages' CDN caches ~10 minutes).
pub const WAIT_EVERY: Duration = Duration::from_secs(20);

/// Where the maintainer keeps the release keys and the repository.
pub fn default_dir(rel: &str) -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join(rel)
}

/// Where the GitHub release goes.
pub enum ReleaseTarget {
    /// `gh release create` in this repository (`owner/name`).
    Gh(String),
    /// A directory (tests, a dry run): the assets, the notes and
    /// `release.txt`.
    Dir(PathBuf),
}

/// Fetches the live repository's file at a URL.
pub type Live = Box<dyn Fn(&str) -> Result<Vec<u8>, String>>;

/// Everything `publish` reads and writes.
pub struct Config {
    /// The version (`dist`'s); its zip must be in `zips`.
    pub version: String,
    /// The release's notes (Markdown).
    pub notes: PathBuf,
    /// `dist`'s output: the zip; the signature and the bundle go there.
    pub zips: PathBuf,
    /// `dist`'s Flatpak repository (unsigned, branch `master`).
    pub src_repo: PathBuf,
    /// The ed25519 secret key (PKCS#8 PEM) and the committed public half.
    pub signing_key: PathBuf,
    pub signing_public: PathBuf,
    /// The GPG home with the repository's secret key (`None`: unsigned,
    /// the tests where gpg cannot run), and the committed public key.
    pub gnupg_home: Option<PathBuf>,
    pub gpg_public: PathBuf,
    /// The persistent, signed Flatpak repository.
    pub flatpak_repo: PathBuf,
    /// Where the Pages tree is made.
    pub work: PathBuf,
    pub pages_remote: String,
    pub pages_url: String,
    pub keep: u32,
    /// The release tags (`v<x.y.z>` ones count).
    pub tags: Vec<String>,
    /// Fetches the live repository (`curl`; the tests' own).
    pub live: Live,
    pub wait: Duration,
    pub wait_every: Duration,
    pub release: ReleaseTarget,
}

/// What a publish did.
#[derive(Debug, Default)]
pub struct Published {
    pub tag: String,
    pub branches: Vec<String>,
    pub bundle: PathBuf,
    pub signature: PathBuf,
    /// Things the maintainer should know (permissions changed, unsigned).
    pub warnings: Vec<String>,
}

fn run(what: &str, c: &mut Command) -> Result<String, String> {
    let out = c.output().map_err(|e| format!("cannot run {what}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{what} failed ({}): {}{}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim(),
            String::from_utf8_lossy(&out.stdout).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The newest final release among `tags` (`v<x.y.z>`).
pub fn latest_final(tags: &[String]) -> Option<Version> {
    tags.iter().filter(|t| crate::version::is_release_tag(t)).filter_map(|t| Version::parse(t)).fold(
        None,
        |best: Option<Version>, v| match best {
            Some(b) if !v.is_newer_than(&b) => Some(b),
            _ => Some(v),
        },
    )
}

/// Step 0's version rules: no `.dirty` build; a dev build's base version
/// above the latest final release (else it would sort below it and never
/// be offered); a final release not below it.
pub fn check_version(version: &str, tags: &[String]) -> Result<Version, String> {
    let v = Version::parse(version).ok_or_else(|| format!("{version} is no version"))?;
    if v.is_dirty() {
        return Err(format!(
            "{version} was built from uncommitted changes (.dirty): commit, then run dist again"
        ));
    }
    if let Some(last) = latest_final(tags) {
        if v.is_prerelease() && !v.base().is_newer_than(&last) {
            return Err(format!(
                "{version} would sort below the released {last}: bump the workspace version in Cargo.toml \
                 above {last} (after each final release), commit, then run dist again"
            ));
        }
        if !v.is_prerelease() && last.is_newer_than(&v) {
            return Err(format!("{version} is older than the released {last}"));
        }
    }
    Ok(v)
}

/// An armored OpenPGP public key's primary key fingerprint (v4: SHA-1
/// of 0x99, the key packet's length and body), uppercase hex.
pub fn gpg_fingerprint(armored: &str) -> Result<String, String> {
    use base64::Engine as _;
    use sha1::Digest as _;
    let b64 = flatpak::gpg_key_base64(armored).ok_or("no armored public key")?;
    let bytes =
        base64::engine::general_purpose::STANDARD.decode(b64).map_err(|e| format!("bad armor: {e}"))?;
    let tag = *bytes.first().ok_or("an empty key")?;
    if tag & 0x80 == 0 {
        return Err("no OpenPGP packet".into());
    }
    let (packet, len, at) = if tag & 0x40 != 0 {
        // New format: one, two or five length bytes.
        let b = |i: usize| bytes.get(i).copied().map(usize::from).ok_or("a cut packet");
        let first = b(1)?;
        match first {
            0..=191 => (tag & 0x3f, first, 2),
            192..=223 => (tag & 0x3f, ((first - 192) << 8) + b(2)? + 192, 3),
            255 => (tag & 0x3f, (b(2)? << 24) | (b(3)? << 16) | (b(4)? << 8) | b(5)?, 6),
            _ => return Err("a partial-length key packet".into()),
        }
    } else {
        let b = |i: usize| bytes.get(i).copied().map(usize::from).ok_or("a cut packet");
        match tag & 3 {
            0 => ((tag >> 2) & 0xf, b(1)?, 2),
            1 => ((tag >> 2) & 0xf, (b(1)? << 8) | b(2)?, 3),
            2 => ((tag >> 2) & 0xf, (b(1)? << 24) | (b(2)? << 16) | (b(3)? << 8) | b(4)?, 5),
            _ => return Err("an indeterminate-length key packet".into()),
        }
    };
    if packet != 6 {
        return Err("the first packet is no public key".into());
    }
    let body = bytes.get(at..at + len).ok_or("a cut key packet")?;
    if body.first() != Some(&4) || len > 0xffff {
        return Err("not a version 4 key".into());
    }
    let mut h = sha1::Sha1::new();
    h.update([0x99, (len >> 8) as u8, len as u8]);
    h.update(body);
    Ok(h.finalize().iter().map(|b| format!("{b:02X}")).collect())
}

/// The fingerprints of the secret keys in GPG home `home`.
fn gpg_secret_fingerprints(home: &Path) -> Result<Vec<String>, String> {
    let out = run(
        "gpg --list-secret-keys",
        Command::new("gpg").arg("--homedir").arg(home).args([
            "--batch",
            "--with-colons",
            "--list-secret-keys",
        ]),
    )?;
    Ok(out
        .lines()
        .filter_map(|l| l.strip_prefix("fpr:"))
        .filter_map(|l| l.split(':').find(|f| f.len() == 40).map(str::to_uppercase))
        .collect())
}

/// The permission sections of a Flatpak's `metadata` (everything but
/// `[Application]` and `[Build]`), as lines in order.
pub fn permissions(metadata: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut keep = false;
    for line in metadata.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if line.starts_with('[') {
            keep = line != "[Application]" && line != "[Build]";
        }
        if keep {
            out.push(line.to_string());
        }
    }
    out
}

/// The Flatpak's architecture (`flatpak --default-arch`).
fn arch() -> Result<String, String> {
    Ok(run("flatpak --default-arch", Command::new("flatpak").arg("--default-arch"))?.trim().to_string())
}

fn ostree_cat(repo: &Path, r#ref: &str, file: &str) -> Result<String, String> {
    run(
        "ostree cat",
        Command::new("ostree").arg(format!("--repo={}", repo.display())).args(["cat", r#ref, file]),
    )
}

fn ref_exists(repo: &Path, r#ref: &str) -> bool {
    Command::new("ostree")
        .arg(format!("--repo={}", repo.display()))
        .args(["rev-parse", r#ref])
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Copies the repository `from` into `to` (its working files left out).
fn copy_repo(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    for e in std::fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))? {
        let e = e.map_err(|e| e.to_string())?;
        let name = e.file_name();
        if [".lock", "tmp", "state"].iter().any(|n| name == *n) {
            continue;
        }
        let (src, dst) = (e.path(), to.join(&name));
        if e.file_type().map_err(|e| e.to_string())?.is_dir() {
            copy_repo(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst).map_err(|e| format!("{}: {e}", src.display()))?;
        }
    }
    Ok(())
}

/// The Pages repository's index page.
fn index_html() -> String {
    format!(
        "<!doctype html>\n<meta charset=\"utf-8\">\n<title>Open Liero Reloaded: Flatpak repository</title>\n\
         <h1>Open Liero Reloaded</h1>\n<p>This is the game's Flatpak repository. Install the final releases \
         with</p>\n<pre>flatpak install --user {REPO_URL}{stable}</pre>\n<p>or the pre-releases too with</p>\n\
         <pre>flatpak install --user {REPO_URL}{beta}</pre>\n<p>The game and your software centre update it from \
         here. Releases: <a href=\"https://github.com/{repo}/releases\">github.com/{repo}/releases</a>.</p>\n",
        stable = flatpak::ref_file_name(STABLE),
        beta = flatpak::ref_file_name(BETA),
        repo = crate::version::RELEASE_REPO,
    )
}

/// Publishes per `cfg`.
pub fn publish(cfg: &Config) -> Result<Published, String> {
    // 0. Everything checked before anything is written.
    let version = check_version(&cfg.version, &cfg.tags)?;
    let v = cfg.version.as_str();
    let prerelease = version.is_prerelease();
    let zip_name = release::windows_zip_name(v);
    let zip = cfg.zips.join(&zip_name);
    let zip_bytes =
        std::fs::read(&zip).map_err(|e| format!("{} ({e}): run cargo xtask dist first", zip.display()))?;
    if !cfg.notes.is_file() {
        return Err(format!("the notes {} are missing", cfg.notes.display()));
    }
    let pem = std::fs::read_to_string(&cfg.signing_key)
        .map_err(|e| format!("the signing key {}: {e}", cfg.signing_key.display()))?;
    let key = SigningKey::from_pem(&pem)
        .map_err(|e| format!("the signing key {}: {e}", cfg.signing_key.display()))?;
    let public = std::fs::read_to_string(&cfg.signing_public)
        .map_err(|e| format!("{}: {e}", cfg.signing_public.display()))
        .and_then(|t| public_key_from_hex(&t))?;
    if key.public() != public {
        return Err(format!(
            "the signing key {} does not match the committed public key {}: refusing to sign",
            cfg.signing_key.display(),
            cfg.signing_public.display()
        ));
    }
    let armored =
        std::fs::read_to_string(&cfg.gpg_public).map_err(|e| format!("{}: {e}", cfg.gpg_public.display()))?;
    let fingerprint = gpg_fingerprint(&armored)?;
    let mut warnings = Vec::new();
    let sign: Vec<String> = match &cfg.gnupg_home {
        Some(home) => {
            let secret = gpg_secret_fingerprints(home)?;
            if !secret.contains(&fingerprint) {
                return Err(format!(
                    "the GPG home {} has no secret key for the committed {} ({fingerprint}): refusing to publish",
                    home.display(),
                    cfg.gpg_public.display()
                ));
            }
            vec![format!("--gpg-sign={fingerprint}"), format!("--gpg-homedir={}", home.display())]
        }
        None => {
            warnings.push("the Flatpak repository is not signed (no GPG home)".to_string());
            Vec::new()
        }
    };
    let arch = arch()?;
    let src_ref = format!("app/{APP_ID}/{arch}/master");
    let meta = format!("/files/share/metainfo/{APP_ID}.metainfo.xml");
    let built = ostree_cat(&cfg.src_repo, &src_ref, &meta).map_err(|e| {
        format!("dist's Flatpak ({}): {e}: run cargo xtask dist first", cfg.src_repo.display())
    })?;
    if !built.contains(&format!("<release version=\"{v}\"")) {
        return Err(format!(
            "dist's Flatpak in {} is not {v}: run cargo xtask dist again",
            cfg.src_repo.display()
        ));
    }
    let new_metadata = ostree_cat(&cfg.src_repo, &src_ref, "/metadata")?;
    let tag = format!("v{v}");
    // The release must be creatable before the repository goes live.
    let target = match &cfg.release {
        ReleaseTarget::Gh(gh_repo) => Some(gh_preflight(gh_repo, &tag, &version)?),
        ReleaseTarget::Dir(_) => None,
    };

    // 1. The Windows zip's signature.
    let signature = cfg.zips.join(release::signature_name(&zip_name));
    let sig_text = key.sign(&Manifest::of(v, &zip_name, &zip_bytes));
    std::fs::write(&signature, &sig_text).map_err(|e| format!("{}: {e}", signature.display()))?;
    println!("signed {}", signature.display());

    // 2. The Flatpak repository.
    let repo = &cfg.flatpak_repo;
    if !repo.join("config").is_file() {
        std::fs::create_dir_all(repo).map_err(|e| format!("{}: {e}", repo.display()))?;
        run(
            "ostree init",
            Command::new("ostree").arg(format!("--repo={}", repo.display())).args(["init", "--mode=archive"]),
        )?;
        println!("made the Flatpak repository {}", repo.display());
    }
    let branches = flatpak::branches_for(prerelease);
    for branch in branches {
        let dst_ref = format!("app/{APP_ID}/{arch}/{branch}");
        if ref_exists(repo, &dst_ref) {
            let old = ostree_cat(repo, &dst_ref, "/metadata")?;
            if permissions(&old) != permissions(&new_metadata) {
                warnings.push(format!(
                    "the permissions (finish-args) differ from {branch}'s previous build: the Flatpak portal \
                     will not install this update, players must update from their software centre or run \
                     flatpak update"
                ));
            }
        }
        run(
            "flatpak build-commit-from",
            Command::new("flatpak")
                .arg("build-commit-from")
                .args(&sign)
                .arg(format!("--src-repo={}", cfg.src_repo.display()))
                .arg(format!("--src-ref={src_ref}"))
                .arg("--no-update-summary")
                .arg(format!("--subject=Open Liero Reloaded {v}"))
                .arg(repo)
                .arg(&dst_ref),
        )?;
        println!("committed {v} to {branch}");
    }
    run(
        "flatpak build-update-repo",
        Command::new("flatpak")
            .arg("build-update-repo")
            .args(&sign)
            .arg("--prune")
            .arg(format!("--prune-depth={}", cfg.keep.max(1)))
            .arg(format!("--title={}", flatpak::TITLE))
            .arg(repo),
    )?;
    let bundle_branch = if prerelease { BETA } else { STABLE };
    let bundle = cfg.zips.join(release::flatpak_bundle_name(v));
    run(
        "flatpak build-bundle",
        Command::new("flatpak")
            .arg("build-bundle")
            .arg(format!("--repo-url={REPO_URL}"))
            .arg(format!("--gpg-keys={}", cfg.gpg_public.display()))
            .arg(format!("--runtime-repo={}", flatpak::RUNTIME_REPO))
            .arg(repo)
            .arg(&bundle)
            .arg(APP_ID)
            .arg(bundle_branch),
    )?;
    println!("bundled {} ({bundle_branch})", bundle.display());

    // The Pages tree: one fresh commit, force-pushed.
    let pages = cfg.work.join("pages");
    match std::fs::remove_dir_all(&pages) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            return Err(format!("{}: {e}", pages.display()));
        }
        _ => {}
    }
    copy_repo(repo, &pages)?;
    let b64 = flatpak::gpg_key_base64(&armored).ok_or("the committed GPG key has no armor body")?;
    let extra = [
        (".nojekyll".to_string(), String::new()),
        ("index.html".to_string(), index_html()),
        (flatpak::REPO_FILE.to_string(), flatpak::flatpakrepo(&b64)),
        (flatpak::ref_file_name(STABLE), flatpak::flatpakref(STABLE, &b64)),
        (flatpak::ref_file_name(BETA), flatpak::flatpakref(BETA, &b64)),
        ("olr-flatpak.gpg".to_string(), armored.clone()),
    ];
    for (name, text) in extra {
        std::fs::write(pages.join(&name), text).map_err(|e| format!("{name}: {e}"))?;
    }
    let git = |args: &[&str]| {
        run(
            &format!("git {}", args.first().unwrap_or(&"")),
            Command::new("git").arg("-C").arg(&pages).args(args),
        )
    };
    git(&["init", "-q", "-b", PAGES_BRANCH])?;
    git(&["add", "-A"])?;
    let name = git_config("user.name").unwrap_or_else(|| "Open Liero Reloaded".into());
    let email = git_config("user.email").unwrap_or_else(|| "olr-publish@users.noreply.github.com".into());
    git(&[
        "-c",
        &format!("user.name={name}"),
        "-c",
        &format!("user.email={email}"),
        "commit",
        "-q",
        "-m",
        &format!("Open Liero Reloaded {v}"),
    ])?;
    git(&["push", "-q", "--force", &cfg.pages_remote, &format!("HEAD:{PAGES_BRANCH}")])?;
    println!("pushed the repository to {}", cfg.pages_remote);

    // Live: the summary served is the new one.
    let local = std::fs::read(repo.join("summary")).map_err(|e| format!("the repository's summary: {e}"))?;
    let url = format!("{}/summary", cfg.pages_url.trim_end_matches('/'));
    let start = Instant::now();
    loop {
        match (cfg.live)(&url) {
            Ok(served) if served == local => break,
            Ok(_) => {}
            Err(e) => println!("     {url}: {e}"),
        }
        if start.elapsed() >= cfg.wait {
            return Err(format!(
                "{url} did not show the new summary within {} min: the GitHub release is not made (run \
                 publish again once Pages is live)",
                cfg.wait.as_secs() / 60
            ));
        }
        println!("     waiting for {url} ({} s)", start.elapsed().as_secs());
        std::thread::sleep(cfg.wait_every);
    }
    println!("the repository is live at {}", cfg.pages_url);

    // 3. The GitHub release.
    let title = format!("Open Liero Reloaded {v}");
    let assets = [zip.clone(), signature.clone(), bundle.clone()];
    match &cfg.release {
        ReleaseTarget::Dir(dir) => {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            for a in assets.iter().chain([&cfg.notes]) {
                let name = a.file_name().ok_or("an asset without a name")?;
                std::fs::copy(a, dir.join(name)).map_err(|e| format!("{}: {e}", a.display()))?;
            }
            let text = format!("tag {tag}\ntitle {title}\nprerelease {prerelease}\n");
            std::fs::write(dir.join("release.txt"), text).map_err(|e| e.to_string())?;
        }
        ReleaseTarget::Gh(gh_repo) => {
            let mut c = Command::new("gh");
            c.current_dir(root()).args(["release", "create", &tag]).args(&assets);
            c.args(["--repo", gh_repo, "--title", &title, "--notes-file"]).arg(&cfg.notes);
            if let Some(Some(commit)) = &target {
                c.args(["--prerelease", "--latest=false", "--target", commit]);
            } else {
                c.args(["--verify-tag", "--latest"]);
            }
            run("gh release create", &mut c)?;
        }
    }
    println!("released {tag}");
    Ok(Published {
        tag,
        branches: branches.iter().map(|b| b.to_string()).collect(),
        bundle,
        signature,
        warnings,
    })
}

/// Step 0 for `gh release create`: gh logged in, no release `tag` yet, and
/// what it will point at on GitHub: a final release's pushed tag, or a
/// dev build's commit (named in its version; returned in full).
fn gh_preflight(gh_repo: &str, tag: &str, version: &Version) -> Result<Option<String>, String> {
    run("gh auth status", Command::new("gh").args(["auth", "status"]))
        .map_err(|e| format!("{e}: log in with gh auth login"))?;
    let view = Command::new("gh")
        .args(["release", "view", tag, "--repo", gh_repo])
        .output()
        .map_err(|e| format!("cannot run gh: {e}"))?;
    if view.status.success() {
        return Err(format!("the release {tag} exists already in {gh_repo}"));
    }
    let why = String::from_utf8_lossy(&view.stderr);
    if !why.contains("not found") {
        return Err(format!("gh release view {tag}: {}", why.trim()));
    }
    if !version.is_prerelease() {
        let api = format!("repos/{gh_repo}/git/ref/tags/{tag}");
        run("gh api", Command::new("gh").args(["api", &api]))
            .map_err(|_| format!("the tag {tag} is not on GitHub: git push origin {tag}"))?;
        return Ok(None);
    }
    let short = version.pre.get(2).cloned().unwrap_or_default();
    let commit = run(
        "git rev-parse",
        Command::new("git").current_dir(root()).args([
            "rev-parse",
            "--verify",
            &format!("{short}^{{commit}}"),
        ]),
    )?
    .trim()
    .to_string();
    run("gh api", Command::new("gh").args(["api", &format!("repos/{gh_repo}/commits/{commit}")]))
        .map_err(|_| format!("the build's commit {commit} is not on GitHub: push it first"))?;
    Ok(Some(commit))
}

fn git_config(key: &str) -> Option<String> {
    let out = Command::new("git").current_dir(root()).args(["config", key]).output().ok()?;
    let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !v.is_empty()).then_some(v)
}

/// Fetches `url` with `curl` (the live repository).
fn curl(url: &str) -> Result<Vec<u8>, String> {
    let out = Command::new("curl")
        .args(["-fsSL", "--max-time", "60", url])
        .output()
        .map_err(|e| format!("cannot run curl: {e}"))?;
    if !out.status.success() {
        return Err(format!("curl: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(out.stdout)
}

/// The command's options (`main.rs`).
pub struct Opts {
    pub notes: PathBuf,
    pub version: Option<String>,
    pub signing_key: Option<PathBuf>,
    pub gnupg_home: Option<PathBuf>,
    pub flatpak_repo: Option<PathBuf>,
    pub pages_remote: Option<String>,
    pub pages_url: Option<String>,
    pub release_dir: Option<PathBuf>,
    pub keep: u32,
}

/// The newest Windows zip `dist` left in `zips`: its version.
fn newest_zip_version(zips: &Path) -> Option<String> {
    let prefix = release::folder_name("");
    std::fs::read_dir(zips)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let v = name.strip_prefix(&prefix)?.strip_suffix("-windows-x64.zip")?.to_string();
            Some((e.metadata().and_then(|m| m.modified()).ok()?, v))
        })
        .max()
        .map(|(_, v)| v)
}

/// `cargo xtask publish`.
pub fn command(o: Opts) -> anyhow::Result<bool> {
    let missing: Vec<&str> = ["flatpak", "ostree", "git", "gpg", "curl"]
        .into_iter()
        .chain(o.release_dir.is_none().then_some("gh"))
        .filter(|t| Command::new(t).arg("--version").output().is_err())
        .collect();
    if !missing.is_empty() {
        println!("FAIL publish: {} not found", missing.join(", "));
        return Ok(false);
    }
    let zips = root().join("target").join(crate::dist::ZIP_DIR);
    let Some(version) = o.version.or_else(|| newest_zip_version(&zips)) else {
        println!("FAIL publish: no Windows zip in {}: run cargo xtask dist first", zips.display());
        return Ok(false);
    };
    let tags = run("git tag", Command::new("git").current_dir(root()).args(["tag", "--list", "v*"]))
        .map_err(anyhow::Error::msg)?
        .lines()
        .map(str::to_string)
        .collect();
    let cfg = Config {
        version,
        notes: o.notes,
        zips,
        src_repo: root().join("target/flatpak/repo"),
        signing_key: o.signing_key.unwrap_or_else(|| default_dir(".config/olr-release/update-signing.pem")),
        signing_public: root().join(SIGNING_PUBLIC),
        gnupg_home: Some(o.gnupg_home.unwrap_or_else(|| default_dir(".config/olr-release/gnupg"))),
        gpg_public: root().join(GPG_PUBLIC),
        flatpak_repo: o.flatpak_repo.unwrap_or_else(|| default_dir(".local/share/olr-release/flatpak-repo")),
        work: root().join("target/publish"),
        pages_remote: o.pages_remote.unwrap_or_else(|| PAGES_REMOTE.into()),
        pages_url: o.pages_url.unwrap_or_else(|| REPO_URL.into()),
        keep: o.keep,
        tags,
        live: Box::new(curl),
        wait: WAIT,
        wait_every: WAIT_EVERY,
        release: match o.release_dir {
            Some(d) => ReleaseTarget::Dir(d),
            None => ReleaseTarget::Gh(crate::version::RELEASE_REPO.into()),
        },
    };
    println!("publishing {}", cfg.version);
    match publish(&cfg) {
        Ok(p) => {
            for w in &p.warnings {
                println!("warning: {w}");
            }
            println!(
                "published {} ({}): {}, {}",
                p.tag,
                p.branches.join(" and "),
                p.signature.display(),
                p.bundle.display()
            );
            Ok(true)
        }
        Err(e) => {
            println!("FAIL publish: {e}");
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_committed_gpg_key_is_the_maintainers() {
        let armored = std::fs::read_to_string(root().join(GPG_PUBLIC)).unwrap();
        assert_eq!(gpg_fingerprint(&armored).unwrap(), "299C4EBC1F402E2FBBA74FBB0D79D2E80E5AA5AB");
        assert!(gpg_fingerprint("no key").is_err());
    }

    #[test]
    fn dev_builds_must_sort_above_the_last_release() {
        let tags = |t: &[&str]| t.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let t = tags(&["v0.1.0", "v0.1.0-local-play", "v0.2.0"]);
        assert_eq!(latest_final(&t).unwrap().to_string(), "0.2.0");
        assert!(check_version("0.2.1-dev.3.0abc123", &t).is_ok());
        let e = check_version("0.2.0-dev.330.0145601", &t).unwrap_err();
        assert!(e.contains("bump the workspace version"), "{e}");
        assert!(check_version("0.2.1-dev.3.0abc123.dirty", &t).unwrap_err().contains(".dirty"));
        assert!(check_version("0.2.0", &t).is_ok(), "a final release's own tag exists");
        assert!(check_version("0.1.5", &t).is_err());
        assert!(check_version("0.2.0-dev.1.a", &tags(&["v0.1.0"])).is_ok());
        assert!(check_version("x", &t).is_err());
    }

    /// A publish into temporary directories: `dist`'s Flatpak an OSTree
    /// commit made here, Pages a bare git repository, the release a
    /// directory. Unsigned (gpg cannot run its agent everywhere).
    struct Bench {
        dir: tempfile::TempDir,
        arch: String,
        key: SigningKey,
    }

    const SEED: [u8; 32] = [7; 32];

    impl Bench {
        fn new() -> Option<Bench> {
            if ["flatpak", "ostree", "git"].iter().any(|t| Command::new(t).arg("--version").output().is_err())
            {
                println!("skip: flatpak, ostree or git missing");
                return None;
            }
            let dir = tempfile::tempdir().unwrap();
            let p = dir.path();
            for d in ["zips", "work"] {
                std::fs::create_dir_all(p.join(d)).unwrap();
            }
            run(
                "ostree init",
                Command::new("ostree")
                    .arg(format!("--repo={}", p.join("src").display()))
                    .args(["init", "--mode=archive"]),
            )
            .unwrap();
            run(
                "git init",
                Command::new("git").args(["init", "-q", "--bare", "-b", "main"]).arg(p.join("pages.git")),
            )
            .unwrap();
            std::fs::write(p.join("notes.md"), "## New\n- updates\n").unwrap();
            // PKCS#8 v1 of an ed25519 seed, as `openssl genpkey` writes it.
            use base64::Engine as _;
            let mut der = vec![0x30, 0x2e, 2, 1, 0, 0x30, 5, 6, 3, 0x2b, 0x65, 0x70, 4, 0x22, 4, 0x20];
            der.extend(SEED);
            let b64 = base64::engine::general_purpose::STANDARD.encode(der);
            std::fs::write(
                p.join("key.pem"),
                format!("-----BEGIN PRIVATE KEY-----\n{b64}\n-----END PRIVATE KEY-----\n"),
            )
            .unwrap();
            let key = SigningKey::from_seed(&SEED).unwrap();
            std::fs::write(p.join("key.pub"), olr_update::manifest::hex(&key.public()) + "\n").unwrap();
            Some(Bench { dir, arch: arch().unwrap(), key })
        }

        fn path(&self, rel: &str) -> PathBuf {
            self.dir.path().join(rel)
        }

        /// `dist`'s output for `version`: the zip and the Flatpak commit
        /// with `context` as its permissions.
        fn dist(&self, version: &str, context: &str) {
            std::fs::write(
                self.path("zips").join(release::windows_zip_name(version)),
                format!("zip of {version}"),
            )
            .unwrap();
            let tree = self.path("tree");
            let _ = std::fs::remove_dir_all(&tree);
            std::fs::create_dir_all(tree.join("files/share/metainfo")).unwrap();
            let metadata = format!(
                "[Application]\nname={APP_ID}\nruntime=org.freedesktop.Platform/{a}/26.08\ncommand=olr\n\n[Context]\n{context}\n",
                a = self.arch
            );
            std::fs::write(tree.join("metadata"), &metadata).unwrap();
            std::fs::write(
                tree.join(format!("files/share/metainfo/{APP_ID}.metainfo.xml")),
                format!("<component><releases><release version=\"{version}\"/></releases></component>"),
            )
            .unwrap();
            run(
                "ostree commit",
                Command::new("ostree")
                    .arg(format!("--repo={}", self.path("src").display()))
                    .arg("commit")
                    .arg(format!("--branch=app/{APP_ID}/{}/master", self.arch))
                    .arg(format!("--tree=dir={}", tree.display()))
                    .arg(format!("--add-metadata-string=xa.metadata={metadata}"))
                    .arg(format!("--subject={version}")),
            )
            .unwrap();
        }

        fn config(&self, version: &str, tags: &[&str]) -> Config {
            let bare = self.path("pages.git");
            Config {
                version: version.into(),
                notes: self.path("notes.md"),
                zips: self.path("zips"),
                src_repo: self.path("src"),
                signing_key: self.path("key.pem"),
                signing_public: self.path("key.pub"),
                gnupg_home: None,
                gpg_public: root().join(GPG_PUBLIC),
                flatpak_repo: self.path("flatpak-repo"),
                work: self.path("work"),
                pages_remote: bare.display().to_string(),
                pages_url: "https://pages.example/flatpak".into(),
                keep: 2,
                tags: tags.iter().map(|t| t.to_string()).collect(),
                // Pages "serves" what was pushed.
                live: Box::new(move |url| {
                    assert_eq!(url, "https://pages.example/flatpak/summary");
                    let out = Command::new("git")
                        .arg(format!("--git-dir={}", bare.display()))
                        .args(["show", "main:summary"])
                        .output()
                        .map_err(|e| e.to_string())?;
                    out.status.success().then_some(out.stdout).ok_or_else(|| "404".to_string())
                }),
                wait: Duration::ZERO,
                wait_every: Duration::ZERO,
                release: ReleaseTarget::Dir(self.path("release")),
            }
        }

        fn pages(&self, args: &[&str]) -> String {
            run(
                "git",
                Command::new("git").arg(format!("--git-dir={}", self.path("pages.git").display())).args(args),
            )
            .unwrap()
        }

        fn refs(&self) -> String {
            run(
                "ostree refs",
                Command::new("ostree")
                    .arg(format!("--repo={}", self.path("flatpak-repo").display()))
                    .arg("refs"),
            )
            .unwrap()
        }
    }

    #[test]
    fn a_pre_release_then_a_final_release_are_published() {
        let Some(b) = Bench::new() else { return };
        let net = "shared=network;ipc;";
        let dev = "0.2.1-dev.3.0abc123";
        b.dist(dev, net);
        let p = publish(&b.config(dev, &["v0.2.0"])).unwrap();
        assert_eq!(
            (p.tag.as_str(), p.branches.as_slice()),
            ("v0.2.1-dev.3.0abc123", &["beta".to_string()][..])
        );
        assert_eq!(p.warnings, ["the Flatpak repository is not signed (no GPG home)"]);
        // The signature vouches for the zip, to the game's checks.
        let zip_name = release::windows_zip_name(dev);
        let zip = std::fs::read(b.path("zips").join(&zip_name)).unwrap();
        let m = olr_update::manifest::verify(&std::fs::read(&p.signature).unwrap(), &b.key.public()).unwrap();
        let older = Version::parse("0.2.0").unwrap();
        m.check(dev, &older, &zip_name, zip.len() as u64, &olr_update::manifest::sha256(&zip)).unwrap();
        // The repository has the beta branch only; Pages has it and the files.
        let refs = b.refs();
        assert!(
            refs.contains(&format!("app/{APP_ID}/{}/beta", b.arch)) && !refs.contains("stable"),
            "{refs}"
        );
        let files = b.pages(&["ls-tree", "-r", "--name-only", "main"]);
        for f in [
            ".nojekyll",
            "index.html",
            "summary",
            "config",
            "stable.flatpakref",
            "beta.flatpakref",
            flatpak::REPO_FILE,
        ] {
            assert!(files.lines().any(|l| l == f), "{f} not in {files}");
        }
        assert!(!files.lines().any(|l| l.starts_with("tmp/") || l.starts_with("state/") || l == ".lock"));
        assert!(b.pages(&["show", "main:beta.flatpakref"]).contains("GPGKey="));
        // The release: its assets, its notes, a pre-release.
        let rel = b.path("release");
        for f in [zip_name.clone(), release::signature_name(&zip_name), release::flatpak_bundle_name(dev)] {
            assert!(rel.join(&f).is_file(), "{f}");
        }
        let txt = std::fs::read_to_string(rel.join("release.txt")).unwrap();
        assert!(txt.contains("tag v0.2.1-dev.3.0abc123") && txt.contains("prerelease true"), "{txt}");
        assert!(p.bundle.is_file());

        // A final release with more permissions: both branches, a warning
        // for beta's, Pages one commit again.
        let fin = "0.3.0";
        b.dist(fin, "shared=network;ipc;\nfilesystems=home;");
        let p = publish(&b.config(fin, &["v0.2.0", "v0.3.0"])).unwrap();
        assert_eq!(p.branches, ["stable", "beta"]);
        assert!(
            p.warnings.iter().any(|w| w.contains("beta") && w.contains("permissions")),
            "{:?}",
            p.warnings
        );
        assert!(!p.warnings.iter().any(|w| w.contains("stable's")), "stable had no build: {:?}", p.warnings);
        let refs = b.refs();
        assert!(refs.contains("stable") && refs.contains("beta"), "{refs}");
        assert_eq!(b.pages(&["rev-list", "--count", "main"]).trim(), "1");
        let txt = std::fs::read_to_string(b.path("release/release.txt")).unwrap();
        assert!(txt.contains("tag v0.3.0") && txt.contains("prerelease false"), "{txt}");
    }

    #[test]
    fn publish_refuses_before_writing_anything() {
        let Some(b) = Bench::new() else { return };
        let dev = "0.2.1-dev.3.0abc123";
        b.dist(dev, "shared=network;");
        let sig = b.path("zips").join(release::signature_name(&release::windows_zip_name(dev)));
        // Another signing key than the committed one.
        let mut c = b.config(dev, &["v0.2.0"]);
        c.signing_public = root().join(SIGNING_PUBLIC);
        let e = publish(&c).unwrap_err();
        assert!(e.contains("does not match the committed public key"), "{e}");
        // A GPG home without the repository's secret key.
        let mut c = b.config(dev, &["v0.2.0"]);
        std::fs::create_dir_all(b.path("gnupg")).unwrap();
        c.gnupg_home = Some(b.path("gnupg"));
        assert!(publish(&c).is_err());
        // dist's Flatpak is another version.
        let e = publish(&b.config("0.2.1-dev.4.0abc124", &["v0.2.0"])).unwrap_err();
        assert!(e.contains("run cargo xtask dist"), "{e}");
        // A dev build not above the last final release.
        let e = publish(&b.config(dev, &["v0.2.1"])).unwrap_err();
        assert!(e.contains("bump the workspace version"), "{e}");
        assert!(!sig.exists() && !b.path("flatpak-repo").exists());
        // Pages never shows the new summary: no release is made.
        let mut c = b.config(dev, &["v0.2.0"]);
        c.live = Box::new(|_| Ok(b"old".to_vec()));
        let e = publish(&c).unwrap_err();
        assert!(e.contains("did not show the new summary"), "{e}");
        assert!(!b.path("release").exists());
    }

    #[test]
    fn permissions_leave_the_application_out() {
        let a = "[Application]\nname=x\nruntime=a\n\n[Context]\nshared=network;ipc;\n";
        let b = "[Application]\nname=x\nruntime=b\n\n[Context]\nshared=network;ipc;\n";
        let c = "[Application]\nname=x\n\n[Context]\nshared=network;ipc;\nfilesystems=home;\n";
        assert_eq!(permissions(a), permissions(b));
        assert_ne!(permissions(a), permissions(c));
    }
}
