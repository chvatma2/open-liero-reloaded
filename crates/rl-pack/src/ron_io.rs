use rl_data::manifest::{PACK_FORMAT_VERSION, PackManifest};
use rl_vfs::{ContentSink, ContentSource, VPath};
use serde::{Serialize, de::DeserializeOwned};

use crate::{MANIFEST, PackError};

/// Serializes `value` as pretty RON with a trailing newline.
pub(crate) fn to_ron<T: Serialize>(value: &T) -> String {
    let cfg = ron::ser::PrettyConfig::default().indentor("    ".to_string());
    let mut s = ron::ser::to_string_pretty(value, cfg).expect("content types always serialize");
    s.push('\n');
    s
}

pub(crate) fn read_ron<T: DeserializeOwned>(src: &dyn ContentSource, file: &str) -> Result<T, PackError> {
    let bytes = src.read(&VPath::new(file)?)?;
    let text =
        std::str::from_utf8(&bytes).map_err(|e| PackError::invalid(file, format!("not UTF-8: {e}")))?;
    ron::from_str(text).map_err(|e| PackError::Ron {
        file: file.to_string(),
        line: e.span.start.line,
        col: e.span.start.col,
        message: e.code.to_string(),
    })
}

pub(crate) fn write_ron<T: Serialize>(
    sink: &mut dyn ContentSink,
    file: &str,
    value: &T,
) -> Result<(), PackError> {
    sink.write(&VPath::new(file)?, to_ron(value).as_bytes())?;
    Ok(())
}

/// Reads `pack.ron` and checks its format version.
pub fn read_manifest(src: &dyn ContentSource) -> Result<PackManifest, PackError> {
    let m: PackManifest = read_ron(src, MANIFEST)?;
    if m.format_version != PACK_FORMAT_VERSION {
        return Err(PackError::invalid(
            MANIFEST,
            format!(
                "unsupported format_version {} (this build reads {PACK_FORMAT_VERSION}; re-import the pack)",
                m.format_version
            ),
        ));
    }
    Ok(m)
}

pub fn write_manifest(sink: &mut dyn ContentSink, m: &PackManifest) -> Result<(), PackError> {
    write_ron(sink, MANIFEST, m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rl_data::manifest::PackKind;
    use rl_vfs::MemFs;

    #[test]
    fn an_older_pack_format_is_refused_with_a_re_import_hint() {
        let mut m = crate::mod_pack::tests::manifest(PackKind::Theme);
        let mut fs = MemFs::new();
        write_manifest(&mut fs, &m).unwrap();
        assert_eq!(read_manifest(&fs).unwrap(), m);
        // Version 1 theme packs may lack the green ball OLX has.
        m.format_version = 1;
        write_manifest(&mut fs, &m).unwrap();
        let err = read_manifest(&fs).unwrap_err().to_string();
        assert!(err.contains("format_version 1") && err.contains("re-import"), "{err}");
    }
}
