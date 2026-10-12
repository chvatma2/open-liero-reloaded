//! The server's files in its data directory: its identity
//! (`server-identity.ron`, owner-only like the game's `identity.ron`) and
//! the bans file.

use std::fs;
use std::io::{self, Write as _};
use std::net::IpAddr;
use std::path::Path;

use anyhow::{Context as _, bail};
use rl_lobby::Identity;
use rl_proto::PublicKey;
use serde::{Deserialize, Serialize};

/// The server's secret key, in the data directory.
pub const IDENTITY_FILE: &str = "server-identity.ron";
/// The identity file's format (the game's `identity.ron` format).
pub const IDENTITY_VERSION: u32 = 1;
/// A bans file larger than this is refused (it holds keys and addresses).
pub const MAX_BANS_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IdentityFile {
    version: u32,
    /// 64 hex characters.
    secret: String,
}

/// The bans kept across restarts: keys (64 hex characters) and addresses.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Bans {
    pub keys: Vec<String>,
    pub ips: Vec<IpAddr>,
}

/// 32 bytes as 64 lowercase hex characters.
pub fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// 64 hex characters (either case) as 32 bytes.
pub fn parse_key(text: &str) -> anyhow::Result<PublicKey> {
    let text = text.trim();
    if text.len() != 64 || !text.is_ascii() {
        bail!("not 64 hex characters");
    }
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&text[2 * i..2 * i + 2], 16).context("not hex")?;
    }
    Ok(out)
}

/// The server's identity from `<dir>/server-identity.ron`, made (from the
/// operating system's randomness) and saved owner-only when there is no
/// file; `true` when it was made. A damaged file is an error: the server
/// does not start with another key behind its operator's back.
pub fn load_identity(dir: &Path) -> anyhow::Result<(Identity, bool)> {
    let path = dir.join(IDENTITY_FILE);
    match fs::read_to_string(&path) {
        Ok(text) => {
            let file: IdentityFile = ron::from_str(&text).with_context(|| format!("{}", path.display()))?;
            if file.version != IDENTITY_VERSION {
                bail!("{}: version {} (this server reads {IDENTITY_VERSION})", path.display(), file.version);
            }
            let secret =
                parse_key(&file.secret).with_context(|| format!("{}: the secret", path.display()))?;
            let id = Identity::from_secret(secret)
                .with_context(|| format!("{}: the secret is no valid key", path.display()))?;
            Ok((id, false))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let id = fresh_identity()?;
            let file = IdentityFile { version: IDENTITY_VERSION, secret: hex(&id.secret()) };
            write_atomic(&path, to_ron(&file)?.as_bytes(), true)
                .with_context(|| format!("saving {}", path.display()))?;
            Ok((id, true))
        }
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

fn fresh_identity() -> anyhow::Result<Identity> {
    // A random scalar is valid but for a chance of about 2^-128 a try.
    for _ in 0..16 {
        if let Some(id) = Identity::from_secret(rl_net::random_bytes::<32>()?) {
            return Ok(id);
        }
    }
    bail!("no valid key from the system's randomness")
}

/// The bans in `path` (none when there is no file); its keys checked.
pub fn load_bans(path: &Path) -> anyhow::Result<(Vec<PublicKey>, Vec<IpAddr>)> {
    let text = match fs::metadata(path) {
        Ok(m) if m.len() > MAX_BANS_BYTES => bail!("{}: larger than {MAX_BANS_BYTES} bytes", path.display()),
        Ok(_) => fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok((Vec::new(), Vec::new())),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let bans: Bans = ron::from_str(&text).with_context(|| format!("{}", path.display()))?;
    let keys = bans
        .keys
        .iter()
        .map(|k| parse_key(k).with_context(|| format!("{}: key {k:?}", path.display())))
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok((keys, bans.ips))
}

/// Writes the bans to `path` atomically.
pub fn save_bans(path: &Path, keys: &[PublicKey], ips: &[IpAddr]) -> anyhow::Result<()> {
    let bans = Bans { keys: keys.iter().map(hex).collect(), ips: ips.to_vec() };
    write_atomic(path, to_ron(&bans)?.as_bytes(), false).with_context(|| format!("saving {}", path.display()))
}

fn to_ron<T: Serialize>(value: &T) -> anyhow::Result<String> {
    Ok(ron::ser::to_string_pretty(value, ron::ser::PrettyConfig::default())? + "\n")
}

/// Writes `bytes` to a temporary file next to `path`, syncs it and renames
/// it over `path` (making the directory first); `private` makes it
/// readable by its owner only (unix mode 0600).
fn write_atomic(path: &Path, bytes: &[u8], private: bool) -> io::Result<()> {
    let dir = path.parent().ok_or_else(|| io::Error::other("no directory"))?;
    fs::create_dir_all(dir)?;
    let mut tmp_name = path.file_name().unwrap_or_default().to_os_string();
    tmp_name.push(".tmp");
    let tmp = dir.join(tmp_name);
    let written = (|| {
        let mut f = create_file(&tmp, private)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        fs::rename(&tmp, path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written
}

/// A new file; `private` makes it readable by its owner only (unix).
fn create_file(path: &Path, private: bool) -> io::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.write(true);
    #[cfg(unix)]
    if private {
        use std::os::unix::fs::OpenOptionsExt as _;
        // A stale temporary file is removed first so the mode applies.
        let _ = fs::remove_file(path);
        return options.create_new(true).mode(0o600).open(path);
    }
    let _ = private;
    options.create(true).truncate(true).open(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_round_trip_through_hex() {
        let key: [u8; 32] = core::array::from_fn(|i| (i * 37 + 5) as u8);
        assert_eq!(parse_key(&hex(&key)).unwrap(), key);
        assert_eq!(parse_key(&hex(&key).to_uppercase()).unwrap(), key);
        for bad in ["", "ab", &"g".repeat(64), &"é".repeat(32), &"a".repeat(65)] {
            assert!(parse_key(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn bans_round_trip_and_a_missing_file_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bans.ron");
        assert_eq!(load_bans(&path).unwrap(), (vec![], vec![]));
        let ips: Vec<IpAddr> = vec!["10.0.0.1".parse().unwrap(), "::1".parse().unwrap()];
        save_bans(&path, &[[7; 32]], &ips).unwrap();
        assert_eq!(load_bans(&path).unwrap(), (vec![[7; 32]], ips));
        fs::write(&path, "(keys: [\"zz\"])").unwrap();
        assert!(load_bans(&path).is_err(), "a bad key is an error, not an empty list");
    }

    #[test]
    fn the_identity_is_made_once_and_kept() {
        let dir = tempfile::tempdir().unwrap();
        let (a, made) = load_identity(dir.path()).unwrap();
        assert!(made);
        let (b, made) = load_identity(dir.path()).unwrap();
        assert!(!made);
        assert_eq!(a.public(), b.public());
        fs::write(dir.path().join(IDENTITY_FILE), "(version: 1, secret: \"00\")").unwrap();
        assert!(load_identity(dir.path()).is_err(), "a damaged identity stops the server");
    }
}
