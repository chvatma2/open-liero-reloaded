//! The Windows zip's signature (issue #4): a small manifest naming the
//! release's version, the zip's name, size and SHA-256, signed with the
//! maintainer's ed25519 key (its public half `tools/release/update-signing.pub`,
//! baked in as [`release_key`]). Signing the version, not the zip alone,
//! means an older, validly signed zip cannot pass for a newer release: the
//! updater checks the signed version is the one offered and newer than its
//! own ([`Manifest::check`]).
//!
//! The file (`<zip>.sig`), lines ending in `\n`:
//!
//! ```text
//! olr-update-manifest 1
//! version 0.2.0-dev.330.014560b
//! file open-liero-reloaded-0.2.0-dev.330.014560b-windows-x64.zip
//! size 23456789
//! sha256 <64 hex digits>
//! signature <128 hex digits: ed25519 over every line above, newlines included>
//! ```

use aws_lc_rs::signature::{ED25519, Ed25519KeyPair, KeyPair as _, UnparsedPublicKey};

use crate::version::Version;

/// The manifest's first line.
pub const MAGIC: &str = "olr-update-manifest 1";
/// The most bytes a signature file may have.
pub const MAX_SIGNATURE_BYTES: usize = 4096;

/// The committed public key, hex (`tools/release/update-signing.pub`).
const RELEASE_KEY_HEX: &str = include_str!("../../../tools/release/update-signing.pub");

/// The release signing key's public half, as the game trusts it.
pub fn release_key() -> [u8; 32] {
    public_key_from_hex(RELEASE_KEY_HEX).expect("tools/release/update-signing.pub holds 32 bytes of hex")
}

/// A `.pub` file's key: 64 hex digits, one line.
pub fn public_key_from_hex(text: &str) -> Result<[u8; 32], String> {
    let bytes = hex_bytes(text.trim()).ok_or("the public key is not hex")?;
    bytes.try_into().map_err(|b: Vec<u8>| format!("the public key has {} bytes, not 32", b.len()))
}

/// What a signature vouches for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    /// The release's version (its tag without the `v`).
    pub version: String,
    /// The signed file's name.
    pub file: String,
    pub size: u64,
    pub sha256: [u8; 32],
}

impl Manifest {
    /// The signed lines.
    pub fn body(&self) -> String {
        format!(
            "{MAGIC}\nversion {}\nfile {}\nsize {}\nsha256 {}\n",
            self.version,
            self.file,
            self.size,
            hex(&self.sha256)
        )
    }

    /// The manifest of `bytes` named `file` for `version`.
    pub fn of(version: &str, file: &str, bytes: &[u8]) -> Manifest {
        Manifest {
            version: version.into(),
            file: file.into(),
            size: bytes.len() as u64,
            sha256: sha256(bytes),
        }
    }

    /// Whether this (verified) manifest vouches for the release `offered`
    /// (its version text, `dist`'s) to a build `current`: the same
    /// version, newer than the build, the expected file `file` of `size`
    /// bytes and digest `sha256`. Every refusal says why.
    pub fn check(
        &self,
        offered: &str,
        current: &Version,
        file: &str,
        size: u64,
        sha256: &[u8; 32],
    ) -> Result<(), String> {
        if self.version != offered {
            return Err(format!("the signature is for version {}, not {offered}", self.version));
        }
        let Some(signed) = Version::parse(&self.version) else {
            return Err(format!("the signed version {} is no version", self.version));
        };
        if !signed.is_newer_than(current) {
            return Err(format!(
                "the signed version {} is not newer than this build ({current})",
                self.version
            ));
        }
        if self.file != file {
            return Err(format!("the signature is for {}, not {file}", self.file));
        }
        if self.size != size {
            return Err(format!("the download has {size} bytes, the signature says {}", self.size));
        }
        if &self.sha256 != sha256 {
            return Err("the download's SHA-256 differs from the signed one".into());
        }
        Ok(())
    }
}

/// The SHA-256 of `bytes`.
pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finish()
}

/// A running SHA-256 (a download hashed as it comes).
pub struct Sha256(aws_lc_rs::digest::Context);

impl Sha256 {
    pub fn new() -> Sha256 {
        Sha256(aws_lc_rs::digest::Context::new(&aws_lc_rs::digest::SHA256))
    }

    pub fn update(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }

    pub fn finish(self) -> [u8; 32] {
        let d = self.0.finish();
        let mut out = [0u8; 32];
        out.copy_from_slice(d.as_ref());
        out
    }
}

impl Default for Sha256 {
    fn default() -> Self {
        Sha256::new()
    }
}

/// The signing key (the maintainer's, `cargo xtask publish`; tests' own).
pub struct SigningKey(Ed25519KeyPair);

impl SigningKey {
    /// A PKCS#8 PEM file's key (`openssl genpkey -algorithm ed25519`).
    pub fn from_pem(text: &str) -> Result<SigningKey, String> {
        let begin = "-----BEGIN PRIVATE KEY-----";
        let end = "-----END PRIVATE KEY-----";
        let (_, rest) = text.split_once(begin).ok_or("no PKCS#8 private key (BEGIN PRIVATE KEY)")?;
        let (body, _) = rest.split_once(end).ok_or("the private key has no END line")?;
        let b64: String = body.chars().filter(|c| !c.is_whitespace()).collect();
        let der = base64_decode(&b64).ok_or("the private key is not base64")?;
        SigningKey::from_pkcs8(&der)
    }

    /// A PKCS#8 (v1 or v2) DER key.
    pub fn from_pkcs8(der: &[u8]) -> Result<SigningKey, String> {
        Ed25519KeyPair::from_pkcs8(der).map(SigningKey).map_err(|e| format!("not an ed25519 key: {e}"))
    }

    /// A key from its 32-byte seed (tests).
    pub fn from_seed(seed: &[u8; 32]) -> Result<SigningKey, String> {
        Ed25519KeyPair::from_seed_unchecked(seed)
            .map(SigningKey)
            .map_err(|e| format!("not an ed25519 seed: {e}"))
    }

    pub fn public(&self) -> [u8; 32] {
        let mut out = [0u8; 32];
        out.copy_from_slice(self.0.public_key().as_ref());
        out
    }

    /// The signature file of `m`.
    pub fn sign(&self, m: &Manifest) -> String {
        let body = m.body();
        let sig = self.0.sign(body.as_bytes());
        format!("{body}signature {}\n", hex(sig.as_ref()))
    }
}

/// The manifest in the signature file `text`, if `key` signed it; every
/// other file (unsigned, another key's, changed, malformed) is refused.
pub fn verify(text: &[u8], key: &[u8; 32]) -> Result<Manifest, String> {
    if text.len() > MAX_SIGNATURE_BYTES {
        return Err("the signature file is too long".into());
    }
    let text = std::str::from_utf8(text).map_err(|_| "the signature file is not text")?;
    let at = text.find("signature ").filter(|&i| i > 0 && text.as_bytes()[i - 1] == b'\n');
    let at = at.ok_or("the file holds no signature")?;
    let (body, sig_line) = text.split_at(at);
    let sig_hex = sig_line.strip_prefix("signature ").unwrap_or("").trim_end_matches('\n');
    let sig = hex_bytes(sig_hex).filter(|s| s.len() == 64).ok_or("the signature is not 64 bytes of hex")?;
    UnparsedPublicKey::new(&ED25519, key)
        .verify(body.as_bytes(), &sig)
        .map_err(|_| "the signature does not match the release key")?;
    // Signed by the maintainer: now read what it says, strictly.
    let mut lines = body.strip_suffix('\n').unwrap_or(body).split('\n');
    let mut field = |name: &str| {
        lines
            .next()
            .and_then(|l| l.strip_prefix(name))
            .and_then(|l| l.strip_prefix(' '))
            .map(str::to_string)
            .ok_or(format!("the signed manifest has no {name} line"))
    };
    if body.split('\n').next() != Some(MAGIC) {
        return Err("the signed manifest is of another format".into());
    }
    let _ = field("olr-update-manifest")?;
    let version = field("version")?;
    let file = field("file")?;
    let size = field("size")?.parse().map_err(|_| "the signed size is no number")?;
    let digest = hex_bytes(&field("sha256")?).filter(|d| d.len() == 32).ok_or("the signed SHA-256 is bad")?;
    if lines.next().is_some() {
        return Err("the signed manifest has extra lines".into());
    }
    let mut sha256 = [0u8; 32];
    sha256.copy_from_slice(&digest);
    Ok(Manifest { version, file, size, sha256 })
}

/// Lowercase hex.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hex_bytes(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return None;
    }
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok()).collect()
}

fn base64_decode(text: &str) -> Option<Vec<u8>> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.decode(text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEED: [u8; 32] = [7; 32];

    fn manifest() -> Manifest {
        Manifest::of(
            "0.2.0-dev.330.0145601",
            "open-liero-reloaded-0.2.0-dev.330.0145601-windows-x64.zip",
            b"zip",
        )
    }

    #[test]
    fn the_committed_key_is_the_maintainers() {
        assert_eq!(hex(&release_key()), "a69eec1542556716eb173bca479ec04f82e533b698333844fba1a86e98f69845");
        assert!(public_key_from_hex("abcd").is_err());
        assert!(public_key_from_hex("zz").is_err());
    }

    #[test]
    fn a_signed_manifest_verifies_and_nothing_else_does() {
        let key = SigningKey::from_seed(&SEED).unwrap();
        let text = key.sign(&manifest());
        assert!(text.starts_with(MAGIC));
        assert_eq!(verify(text.as_bytes(), &key.public()), Ok(manifest()));
        // Another key's.
        let other = SigningKey::from_seed(&[8; 32]).unwrap();
        assert!(verify(text.as_bytes(), &other.public()).is_err());
        assert!(verify(text.as_bytes(), &release_key()).is_err());
        // Any change to what is signed.
        let changed = text.replace("size 3", "size 4");
        assert!(verify(changed.as_bytes(), &key.public()).unwrap_err().contains("does not match"));
        let newer = text.replace("dev.330", "dev.331");
        assert!(verify(newer.as_bytes(), &key.public()).is_err());
        // Unsigned, truncated, or not text.
        assert!(verify(manifest().body().as_bytes(), &key.public()).is_err());
        assert!(verify(&text.as_bytes()[..text.len() - 10], &key.public()).is_err());
        assert!(verify(b"\xff\xfe", &key.public()).is_err());
        assert!(verify(b"", &key.public()).is_err());
    }

    #[test]
    fn a_manifest_vouches_only_for_its_release() {
        let m = manifest();
        let current = Version::parse("0.2.0-dev.318.0000000").unwrap();
        let digest = sha256(b"zip");
        assert_eq!(m.check(&m.version, &current, &m.file, 3, &digest), Ok(()));
        assert!(m.check("0.2.0-dev.331.0", &current, &m.file, 3, &digest).unwrap_err().contains("not 0.2.0"));
        let newer_build = Version::parse("0.2.0").unwrap();
        assert!(m.check(&m.version, &newer_build, &m.file, 3, &digest).unwrap_err().contains("not newer"));
        assert!(m.check(&m.version, &current, "other.zip", 3, &digest).is_err());
        assert!(m.check(&m.version, &current, &m.file, 4, &digest).is_err());
        assert!(m.check(&m.version, &current, &m.file, 3, &sha256(b"zap")).is_err());
    }

    #[test]
    fn keys_load_from_pkcs8_pem() {
        // `openssl genpkey -algorithm ed25519` for seed [7; 32]: PKCS#8 v1.
        let mut der = vec![
            0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04, 0x20,
        ];
        der.extend_from_slice(&SEED);
        use base64::Engine as _;
        let pem = format!(
            "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----\n",
            base64::engine::general_purpose::STANDARD.encode(&der)
        );
        let key = SigningKey::from_pem(&pem).unwrap();
        assert_eq!(key.public(), SigningKey::from_seed(&SEED).unwrap().public());
        assert!(SigningKey::from_pem("-----BEGIN PRIVATE KEY-----\n!!\n-----END PRIVATE KEY-----").is_err());
        assert!(SigningKey::from_pem("nothing").is_err());
    }
}
