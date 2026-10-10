use rl_vfs::{ContentSource, VPath, walk};
use sha2::{Digest, Sha256};

use crate::{MANIFEST, PackError};

/// SHA-256 over every file of the pack except `pack.ron`, as lowercase hex.
/// Files are taken in sorted path order; each contributes its path, a 0 byte,
/// its length as little-endian u64, and its bytes. Replays use this hash to
/// identify the exact content they were recorded with.
pub fn content_hash(src: &dyn ContentSource) -> Result<String, PackError> {
    let mut h = Sha256::new();
    for path in walk(src, &VPath::root())? {
        if path.as_str() == MANIFEST {
            continue;
        }
        let data = src.read(&path)?;
        h.update(path.as_str().as_bytes());
        h.update([0u8]);
        h.update((data.len() as u64).to_le_bytes());
        h.update(&data);
    }
    Ok(hex::encode(h.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rl_vfs::MemFs;

    #[test]
    fn ignores_manifest_and_depends_on_paths_and_bytes() {
        let a = MemFs::new().with("pack.ron", "x").with("a.txt", "1");
        let b = MemFs::new().with("pack.ron", "y").with("a.txt", "1");
        let c = MemFs::new().with("pack.ron", "x").with("b.txt", "1");
        let d = MemFs::new().with("pack.ron", "x").with("a.txt", "2");
        assert_eq!(content_hash(&a).unwrap(), content_hash(&b).unwrap());
        assert_ne!(content_hash(&a).unwrap(), content_hash(&c).unwrap());
        assert_ne!(content_hash(&a).unwrap(), content_hash(&d).unwrap());
        assert_eq!(content_hash(&a).unwrap().len(), 64);
    }
}
