//! Helpers shared by the test modules.

use std::path::{Path, PathBuf};

use olr_editor::{Catalog, ModEntry, ThemeEntry};

/// The bundled content root, when `OLR_CONTENT_DIR` names a directory.
pub fn content() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("OLR_CONTENT_DIR")?);
    dir.is_dir().then_some(dir)
}

/// `fixtures/edit/composed-level` (plan 14a Task 9).
pub fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/edit/composed-level")
}

/// The installed packs the editor knows: `olx-theme-dirt` and the two mods.
pub fn catalog(root: &Path) -> Catalog {
    let theme =
        olr_format::themes::read_theme_files(&olr_vfs::DirFs::new(root.join("olx-theme-dirt"))).unwrap();
    Catalog {
        themes: vec![ThemeEntry {
            id: "olx-theme-dirt".into(),
            name: "Dirt".into(),
            front_tile: theme.front_tile,
            back_tile: theme.back_tile,
        }],
        mods: vec![
            ModEntry { id: "olx-classic".into(), name: "Liero v1.0".into() },
            ModEntry { id: "olx-mw-1-0".into(), name: "Modern Warfare1.0".into() },
        ],
        home: None,
    }
}

/// Copies the pack at `root.join(path)` (`path` may be absolute) into a
/// fresh temporary directory; returns it and the copy's path.
pub fn copy_pack(root: &Path, path: &Path) -> (tempfile::TempDir, PathBuf) {
    let from = root.join(path);
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join(from.file_name().unwrap());
    copy_dir(&from, &dir);
    (tmp, dir)
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let target = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &target);
        } else {
            std::fs::copy(e.path(), target).unwrap();
        }
    }
}

/// Every file under `dir`, by `/`-separated relative path, with its bytes.
pub fn tree(dir: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    fn go(base: &Path, dir: &Path, out: &mut std::collections::BTreeMap<String, Vec<u8>>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                go(base, &p, out);
            } else {
                let rel = p.strip_prefix(base).unwrap().to_str().unwrap().replace('\\', "/");
                out.insert(rel, std::fs::read(&p).unwrap());
            }
        }
    }
    let mut out = std::collections::BTreeMap::new();
    go(dir, dir, &mut out);
    out
}

/// The fixture, copied and opened.
pub fn open_fixture(root: &Path) -> (tempfile::TempDir, olr_editor::Document) {
    let (tmp, dir) = copy_pack(root, &fixture());
    let pack = olr_edit::pack::load(&dir).unwrap();
    let doc = olr_editor::Document::open(pack, dir, &catalog(root)).unwrap();
    (tmp, doc)
}

/// `pack::files` of the document's pack, by path string.
pub fn files(doc: &olr_editor::Document) -> std::collections::BTreeMap<String, Vec<u8>> {
    olr_edit::pack::files(&doc.to_pack())
        .unwrap()
        .into_iter()
        .map(|(p, b)| (p.as_str().to_string(), b))
        .collect()
}

/// Carries out `action` and every command it leads to as the host would,
/// synchronously, feeding the answers back; returns every command emitted.
pub fn run(
    state: &mut olr_editor::EditorState,
    action: olr_editor::EditorAction,
) -> Vec<olr_editor::EditorCommand> {
    use olr_editor::{EditorAction, EditorCommand};
    let mut out = Vec::new();
    let mut queue = std::collections::VecDeque::from([action]);
    while let Some(action) = queue.pop_front() {
        for command in state.update(action) {
            match &command {
                EditorCommand::Open(dir) => queue.push_back(open(state.catalog(), dir)),
                EditorCommand::Create(n) => {
                    let made = olr_edit::new::new_pack(
                        &n.dir,
                        olr_data::manifest::PackKind::Level,
                        &n.id,
                        &n.name,
                        olr_edit::new::Template::Level { size: n.size, theme: &n.theme },
                    );
                    queue.push_back(match made {
                        Ok(_) => open(state.catalog(), &n.dir),
                        Err(e) => EditorAction::OpenFailed(e.to_string()),
                    });
                }
                EditorCommand::Save { dir, pack } => queue.push_back(EditorAction::Saved(
                    olr_edit::pack::save(dir, pack).map_err(|e| e.to_string()),
                )),
                _ => {}
            }
            out.push(command);
        }
    }
    out
}

fn open(catalog: &olr_editor::Catalog, dir: &Path) -> olr_editor::EditorAction {
    use olr_editor::EditorAction;
    match olr_edit::pack::load(dir) {
        Ok(pack) => match olr_editor::Document::open(pack, dir.to_path_buf(), catalog) {
            Ok(doc) => EditorAction::Opened(Box::new(doc)),
            Err(e) => EditorAction::OpenFailed(e.to_string()),
        },
        Err(e) => EditorAction::OpenFailed(e.to_string()),
    }
}

/// An editor state with a copy of the fixture open.
pub fn editor(root: &Path) -> (tempfile::TempDir, olr_editor::EditorState) {
    editor_at(root, &fixture())
}

/// An editor state with a copy of the pack at `pack` (relative to `root`,
/// or absolute) open.
pub fn editor_at(root: &Path, pack: &Path) -> (tempfile::TempDir, olr_editor::EditorState) {
    let (tmp, dir) = copy_pack(root, pack);
    let mut state = olr_editor::EditorState::new(olr_editor::EditorSettings::default(), catalog(root));
    run(&mut state, olr_editor::EditorAction::Open(dir));
    assert!(state.document().is_some());
    (tmp, state)
}
