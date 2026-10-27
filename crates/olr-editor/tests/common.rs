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
            holes: theme
                .holes
                .iter()
                .map(|h| {
                    // Mask 1 carves, 2 paints: both cover (plan 14e R4).
                    let size = (h.paint.width(), h.paint.height());
                    let cover = h.mask.iter().map(|m| *m != 0).collect();
                    std::sync::Arc::new(olr_edit::level::StampMask { size, cover })
                })
                .collect(),
        }],
        mods: vec![
            ModEntry { id: "olx-classic".into(), name: "Liero v1.0".into() },
            ModEntry { id: "olx-mw-1-0".into(), name: "Modern Warfare1.0".into() },
        ],
        home: None,
        textures: Vec::new(),
        assets: Vec::new(),
        materials: olr_edit::material::list_materials(Some(&built_in_materials()), None).0,
    }
}

/// The repository's built-in surface materials (`content/materials`).
pub fn built_in_materials() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/materials")
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

thread_local! {
    /// The asset library the test host keeps (built-in, the user's), when
    /// a test set one (`set_library`).
    static LIBRARY: std::cell::RefCell<Option<(PathBuf, PathBuf)>> = const { std::cell::RefCell::new(None) };
}

/// The repository's built-in assets (`content/assets`).
pub fn built_in_assets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/assets")
}

/// Has the test host keep the asset library: `builtin` read-only, `yours`
/// written; returns the library as `AssetsChanged` lists it.
pub fn set_library(builtin: &Path, yours: &Path) -> Vec<olr_editor::AssetEntry> {
    LIBRARY.with(|l| *l.borrow_mut() = Some((builtin.to_path_buf(), yours.to_path_buf())));
    library()
}

/// The library as the game's host lists it: the built-in assets, then the
/// user's.
fn library() -> Vec<olr_editor::AssetEntry> {
    let Some((builtin, yours)) = LIBRARY.with(|l| l.borrow().clone()) else { return Vec::new() };
    let entries = |dir: &Path, builtin: bool| {
        olr_edit::asset::list_library(dir, usize::MAX).0.into_iter().map(move |(id, s)| {
            olr_editor::AssetEntry {
                key: olr_editor::AssetKey { builtin, id },
                name: s.name,
                size: s.size,
                tags: s.tags,
                glows: s.glows,
                thumb: s.thumb,
            }
        })
    };
    entries(&builtin, true).chain(entries(&yours, false)).collect()
}

/// A change of the user's assets carried out, answered as the game's host
/// answers it.
fn change_assets(
    queue: &mut std::collections::VecDeque<olr_editor::EditorAction>,
    change: impl FnOnce(&Path) -> Result<Option<String>, olr_edit::asset::AssetError>,
) {
    use olr_editor::EditorAction;
    let Some((_, yours)) = LIBRARY.with(|l| l.borrow().clone()) else { return };
    let done = change(&yours);
    queue.push_back(EditorAction::AssetsChanged(library()));
    match done {
        Ok(Some(id)) => {
            // Issue #83: with the files written, as the game's host reads them.
            let source = olr_edit::asset::read_dir(&yours.join(&id)).ok().map(Box::new);
            queue.push_back(EditorAction::AssetSaved {
                key: olr_editor::AssetKey { builtin: false, id },
                source,
            })
        }
        Ok(None) => {}
        Err(e) => queue.push_back(EditorAction::Failed(e.to_string())),
    }
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
                EditorCommand::ReadAsset { key, at } => {
                    let lib = LIBRARY.with(|l| l.borrow().clone()).expect("a library");
                    let dir = if key.builtin { lib.0 } else { lib.1 };
                    let asset = olr_edit::asset::read_dir(&dir.join(&key.id))
                        .map(Box::new)
                        .map_err(|e| e.to_string());
                    queue.push_back(EditorAction::AssetRead { key: key.clone(), at: *at, asset });
                }
                // Issue #83: the library's sources, as the game's host reads them.
                EditorCommand::CompareLibrary { origins } => {
                    let lib = LIBRARY.with(|l| l.borrow().clone());
                    let (b, y) = (lib.as_ref().map(|l| l.0.as_path()), lib.as_ref().map(|l| l.1.as_path()));
                    queue.push_back(EditorAction::LibraryCompared(olr_edit::library::read_origins(
                        origins, b, y,
                    )));
                }
                EditorCommand::ReadAssetToEdit(key) => {
                    let lib = LIBRARY.with(|l| l.borrow().clone()).expect("a library");
                    let dir = if key.builtin { lib.0 } else { lib.1 };
                    let asset = olr_edit::asset::load_dir(&dir.join(&key.id))
                        .map(Box::new)
                        .map_err(|e| e.to_string());
                    queue.push_back(EditorAction::AssetToEdit { key: key.clone(), asset });
                }
                EditorCommand::SaveAsset { asset, replace } => change_assets(&mut queue, |dir| {
                    olr_edit::asset::add_to_library(dir, asset, replace.as_deref()).map(Some)
                }),
                EditorCommand::ImportAsset(path) => {
                    change_assets(&mut queue, |dir| olr_edit::asset::import_into_library(dir, path).map(Some))
                }
                EditorCommand::RemoveAsset(id) => change_assets(&mut queue, |dir| {
                    olr_edit::asset::remove_from_library(dir, id).map(|()| None)
                }),
                // Issue #80: as the game's host, bounded.
                EditorCommand::ReadSound(path) => queue.push_back(match std::fs::read(path) {
                    Ok(bytes) if bytes.len() as u64 <= olr_format::assets::MAX_ASSET_SOUND_BYTES => {
                        let name = path.file_name().unwrap().to_string_lossy().into_owned();
                        EditorAction::SoundRead { name, bytes }
                    }
                    Ok(_) => EditorAction::ImportFailed("too large".into()),
                    Err(e) => EditorAction::ImportFailed(e.to_string()),
                }),

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

/// A directory as the game's host lists it for the browser (folders,
/// packs and assets marked).
pub fn list_dir(dir: &Path) -> Vec<olr_editor::browser::DirEntry> {
    let mut out: Vec<olr_editor::browser::DirEntry> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| {
                    let is_dir = e.path().is_dir();
                    olr_editor::browser::DirEntry {
                        name: e.file_name().to_string_lossy().into_owned(),
                        is_dir,
                        is_pack: is_dir && e.path().join("pack.ron").is_file(),
                        is_asset: is_dir && e.path().join("asset.ron").is_file(),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    olr_editor::browser::sort_entries(&mut out);
    out
}

/// Edit in level (issue #81): the level's ground selected (its row in
/// Copies) and opened in the asset tab, where its layers are painted.
pub fn edit_ground(state: &mut olr_editor::EditorState) {
    use olr_editor::{EditorAction, Selection};
    run(state, EditorAction::Select(Selection::Copy(olr_editor::ground::GROUND_COPY.into())));
    run(state, EditorAction::EditGround);
    assert!(state.ground_open(), "the ground opened in the asset tab");
}

/// The fixture, copied and opened, its ground in the asset tab (issue
/// #81: the layer tests' document).
pub fn ground(root: &Path) -> (tempfile::TempDir, olr_editor::EditorState) {
    ground_at(root, &fixture())
}

/// `editor_at`, its ground in the asset tab.
pub fn ground_at(root: &Path, pack: &Path) -> (tempfile::TempDir, olr_editor::EditorState) {
    let (tmp, mut state) = editor_at(root, pack);
    edit_ground(&mut state);
    (tmp, state)
}

/// A level tab's edit (issue #81: its layers are the asset tab's): the
/// level renamed `name`, one step.
pub fn rename_level(state: &olr_editor::EditorState, name: &str) -> olr_editor::EditorAction {
    let meta = state.document().expect("a level").meta();
    olr_editor::EditorAction::Meta(olr_editor::LevelMeta { name: name.into(), ..meta })
}
