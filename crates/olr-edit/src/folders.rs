//! Layer folders (issue #59): every computation over a composition's
//! folder tree, once, for `element`'s pack operations and the editor's
//! document. The rules themselves are the format's (`check_folders`,
//! re-exported here, as `check_elements` applies them on every load), and
//! so is the bounded walk up the parents (`folder_chain`).
//!
//! A folder is a block of the stack: its elements, its subfolders'
//! included, are one contiguous run of `Composition::elements`. The
//! operations work on the tree those runs make ([`Node`]) and write it
//! back flat, which leaves out every folder holding no element and lists
//! the folders in their canonical order (by where each run starts, a
//! parent before its children): equal trees save byte for byte. Each
//! returns an error and leaves the composition as it was when the result
//! would break a rule. Locks refuse content changes, not order (plan 14d
//! R4): grouping, ungrouping, moving and reordering ignore them.

use std::collections::BTreeSet;

use olr_format::levels::{Composition, Element};
pub use olr_format::levels::{Folder, MAX_FOLDER_DEPTH, MAX_FOLDERS, check_folders, folder_chain};

use crate::element::DISTANT_ORDER;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum FolderError {
    #[error("no element or folder {0:?}")]
    NoItem(String),
    #[error("{0:?} is not a folder")]
    NotAFolder(String),
    #[error("{0}")]
    Bad(String),
}

/// Whether element `e` of `c` is shown: it and every folder holding it
/// visible.
pub fn shown(c: &Composition, e: &Element) -> bool {
    e.visible && folder_chain(c, e.folder.as_deref()).iter().all(|f| f.visible)
}

/// Whether element `e` of `c` is locked: it or any folder holding it.
pub fn locked(c: &Composition, e: &Element) -> bool {
    e.locked || folder_chain(c, e.folder.as_deref()).iter().any(|f| f.locked)
}

/// Whether folder `id` of `c` is locked: it or any folder holding it.
pub fn folder_locked(c: &Composition, id: &str) -> bool {
    folder_chain(c, Some(id)).iter().any(|f| f.locked)
}

/// Element `e` of `c` with its effective flags: `visible` when
/// [`shown`], `locked` when [`locked`]. What the bake reads.
pub fn fold(c: &Composition, e: &Element) -> Element {
    Element { visible: shown(c, e), locked: locked(c, e), ..e.clone() }
}

/// Every element of `c` with its effective flags ([`fold`]), bottom to
/// top: a composition without folders as it is.
pub fn folded(c: &Composition) -> Vec<Element> {
    if c.folders.is_empty() {
        return c.elements.clone();
    }
    c.elements.iter().map(|e| fold(c, e)).collect()
}

/// The folder `id` is in, an element's or a folder's (its innermost);
/// `None` at the top level or for an unknown id.
pub fn parent_of<'a>(c: &'a Composition, id: &str) -> Option<&'a str> {
    match c.elements.iter().find(|e| e.id == id) {
        Some(e) => e.folder.as_deref(),
        None => c.folders.iter().find(|f| f.id == id)?.parent.as_deref(),
    }
}

/// Whether element `e` of `c` lies in folder `folder`, at any depth.
pub fn holds(c: &Composition, folder: &str, e: &Element) -> bool {
    folder_chain(c, e.folder.as_deref()).iter().any(|f| f.id == folder)
}

/// The indices of `elements` folder `id` holds, at any depth, bottom to top.
pub fn members(c: &Composition, id: &str) -> Vec<usize> {
    c.elements.iter().enumerate().filter(|(_, e)| holds(c, id, e)).map(|(i, _)| i).collect()
}

/// How deep folder `id` lies: 1 at the top level (0 for an unknown id).
pub fn depth(c: &Composition, id: &str) -> usize {
    folder_chain(c, Some(id)).len()
}

/// Leaves out the folders that hold no element and lists the rest in the
/// canonical order: by where each one's elements start, a parent before
/// its children. Safe on an unchecked composition (bounded walks); what
/// every pack operation runs after its edit, so an operation emptying a
/// folder removes it in the same step.
pub fn tidy(c: &mut Composition) {
    if c.folders.is_empty() {
        return;
    }
    let mut first: Vec<(String, usize)> = Vec::new();
    for (k, e) in c.elements.iter().enumerate() {
        for f in folder_chain(c, e.folder.as_deref()) {
            if !first.iter().any(|(id, _)| *id == f.id) {
                first.push((f.id.clone(), k));
            }
        }
    }
    let start = |id: &str| first.iter().find(|(f, _)| f == id).map(|(_, k)| *k);
    let keys: Vec<Option<(usize, usize)>> =
        c.folders.iter().map(|f| start(&f.id).map(|k| (k, folder_chain(c, Some(&f.id)).len()))).collect();
    let mut kept: Vec<(Folder, (usize, usize))> =
        std::mem::take(&mut c.folders).into_iter().zip(keys).filter_map(|(f, k)| Some((f, k?))).collect();
    kept.sort_by_key(|(_, k)| *k);
    c.folders = kept.into_iter().map(|(f, _)| f).collect();
}

/// "Folder n", the smallest n from 1 no folder of `c` is named.
pub fn default_name(c: &Composition) -> String {
    (1u32..)
        .map(|n| format!("Folder {n}"))
        .find(|n| !c.folders.iter().any(|f| f.name == *n))
        .unwrap_or_default()
}

/// A new folder id from `name` (as an element's, ruling R8 of plan 14a):
/// not an element's or folder's id of `c`.
pub fn new_folder_id(c: &Composition, name: &str) -> String {
    let taken = |id: &str| c.elements.iter().any(|e| e.id == id) || c.folders.iter().any(|f| f.id == id);
    crate::element::unique(&crate::element::slug(name, "folder"), taken)
}

/// One node of the folder tree: an element, or a folder with its
/// children bottom to top. `Slot` marks where `group` puts its folder.
#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Element(Element),
    Folder(Folder, Vec<Node>),
    Slot,
}

impl Node {
    fn id(&self) -> Option<&str> {
        match self {
            Node::Element(e) => Some(&e.id),
            Node::Folder(f, _) => Some(&f.id),
            Node::Slot => None,
        }
    }

    /// Whether it holds an element (a folder at any depth).
    fn has_element(&self) -> bool {
        match self {
            Node::Element(_) => true,
            Node::Folder(_, kids) => kids.iter().any(Node::has_element),
            Node::Slot => false,
        }
    }

    /// Whether it is distant: an element's own, a folder's first element's
    /// (a folder's are all or none).
    fn distant(&self) -> bool {
        match self {
            Node::Element(e) => e.is_distant(),
            Node::Folder(_, kids) => kids.iter().find(|k| k.has_element()).is_some_and(Node::distant),
            Node::Slot => false,
        }
    }

    fn elements(self, out: &mut Vec<Element>) {
        match self {
            Node::Element(e) => out.push(e),
            Node::Folder(_, kids) => kids.into_iter().for_each(|k| k.elements(out)),
            Node::Slot => {}
        }
    }
}

/// The tree of `c`, bottom to top; `c` must keep the folder rules.
pub fn tree(c: &Composition) -> Result<Vec<Node>, FolderError> {
    check_folders(c).map_err(|e| FolderError::Bad(e.to_string()))?;
    let mut root: Vec<Node> = Vec::new();
    // The folders open around the current element, outermost first.
    let mut open: Vec<(Folder, Vec<Node>)> = Vec::new();
    fn close(open: &mut Vec<(Folder, Vec<Node>)>, root: &mut Vec<Node>) {
        if let Some((f, kids)) = open.pop() {
            match open.last_mut() {
                Some((_, parent)) => parent.push(Node::Folder(f, kids)),
                None => root.push(Node::Folder(f, kids)),
            }
        }
    }
    for e in &c.elements {
        let mut chain = folder_chain(c, e.folder.as_deref());
        chain.reverse();
        let common = open.iter().zip(&chain).take_while(|(o, f)| o.0.id == f.id).count();
        while open.len() > common {
            close(&mut open, &mut root);
        }
        open.extend(chain[common..].iter().map(|f| ((*f).clone(), Vec::new())));
        match open.last_mut() {
            Some((_, kids)) => kids.push(Node::Element(e.clone())),
            None => root.push(Node::Element(e.clone())),
        }
    }
    while !open.is_empty() {
        close(&mut open, &mut root);
    }
    Ok(root)
}

/// `nodes` written flat: elements bottom to top naming their innermost
/// folder, folders in the canonical order naming their parent, a folder
/// holding no element left out.
pub fn flatten(nodes: Vec<Node>) -> Composition {
    fn go(nodes: Vec<Node>, parent: Option<&str>, c: &mut Composition) {
        for n in nodes {
            match n {
                Node::Element(mut e) => {
                    e.folder = parent.map(Into::into);
                    c.elements.push(e);
                }
                Node::Folder(mut f, kids) => {
                    if !kids.iter().any(Node::has_element) {
                        continue;
                    }
                    f.parent = parent.map(Into::into);
                    let id = f.id.clone();
                    c.folders.push(f);
                    go(kids, Some(&id), c);
                }
                Node::Slot => {}
            }
        }
    }
    let mut c = Composition::default();
    go(nodes, None, &mut c);
    c
}

/// The path of node `id` in `nodes`: its index in each list from the top.
fn path_of(nodes: &[Node], id: &str) -> Option<Vec<usize>> {
    for (i, n) in nodes.iter().enumerate() {
        if n.id() == Some(id) {
            return Some(vec![i]);
        }
        if let Node::Folder(_, kids) = n
            && let Some(mut p) = path_of(kids, id)
        {
            p.insert(0, i);
            return Some(p);
        }
    }
    None
}

/// A path that names no node: never from `path_of` on the same tree.
fn lost() -> FolderError {
    FolderError::Bad("no such place in the folder tree".into())
}

/// The children of the folder at `path` (the root for an empty one).
fn list_mut<'a>(nodes: &'a mut Vec<Node>, path: &[usize]) -> Result<&'a mut Vec<Node>, FolderError> {
    match path.split_first() {
        None => Ok(nodes),
        Some((i, rest)) => match nodes.get_mut(*i) {
            Some(Node::Folder(_, kids)) => list_mut(kids, rest),
            _ => Err(lost()),
        },
    }
}

fn node_at<'a>(nodes: &'a [Node], path: &[usize]) -> Option<&'a Node> {
    let (last, parents) = path.split_last()?;
    let mut list = nodes;
    for i in parents {
        match list.get(*i)? {
            Node::Folder(_, kids) => list = kids,
            _ => return None,
        }
    }
    list.get(*last)
}

fn take(nodes: &mut Vec<Node>, path: &[usize]) -> Result<Node, FolderError> {
    let (last, parents) = path.split_last().ok_or_else(lost)?;
    let list = list_mut(nodes, parents)?;
    if *last >= list.len() {
        return Err(lost());
    }
    Ok(list.remove(*last))
}

/// Writes `nodes` into `c` when the result keeps the folder rules and
/// the distant layers' order; else `c` stays as it was.
fn commit(c: &mut Composition, nodes: Vec<Node>) -> Result<(), FolderError> {
    let out = flatten(nodes);
    check_folders(&out).map_err(|e| FolderError::Bad(e.to_string()))?;
    // Distant layers stay below every other element (issue #11).
    let near = out.elements.iter().position(|e| !e.is_distant()).unwrap_or(out.elements.len());
    if out.elements[near..].iter().any(Element::is_distant) {
        return Err(FolderError::Bad(DISTANT_ORDER.into()));
    }
    *c = out;
    Ok(())
}

fn find(nodes: &[Node], id: &str) -> Result<Vec<usize>, FolderError> {
    path_of(nodes, id).ok_or_else(|| FolderError::NoItem(id.into()))
}

/// Groups `ids` (elements and folders) into a new folder named `name`
/// ("Folder n" when empty) inside the innermost folder they all share;
/// an id lying in another of `ids`' folders goes with it. Members not
/// one block are first moved into one at the topmost member's place
/// (within that shared folder: above the subfolder holding it, if any),
/// in their order. Returns the new folder's id.
pub fn group(c: &mut Composition, ids: &[String], name: &str) -> Result<String, FolderError> {
    if ids.is_empty() {
        return Err(FolderError::Bad("nothing to group".into()));
    }
    if c.folders.len() >= MAX_FOLDERS {
        return Err(FolderError::Bad(format!("a level holds at most {MAX_FOLDERS} folders")));
    }
    let mut nodes = tree(c)?;
    let mut paths: Vec<Vec<usize>> = ids.iter().map(|id| find(&nodes, id)).collect::<Result<_, _>>()?;
    paths.sort();
    paths.dedup();
    // An id inside another one's folder goes with that folder.
    let all = paths.clone();
    paths.retain(|p| !all.iter().any(|q| q.len() < p.len() && p.starts_with(q)));
    // The innermost folder they share: the longest common prefix of their
    // parents' paths.
    let mut shared: Vec<usize> = paths[0][..paths[0].len() - 1].to_vec();
    for p in &paths[1..] {
        let n = shared.iter().zip(p).take_while(|(a, b)| a == b).count().min(p.len() - 1);
        shared.truncate(n);
    }
    // Sorted paths of disjoint nodes are in stack order: the last is the
    // topmost member.
    let top = paths.last().ok_or_else(lost)?.clone();
    let anchor = top[shared.len()];
    let slot = if top.len() == shared.len() + 1 { anchor } else { anchor + 1 };
    list_mut(&mut nodes, &shared)?.insert(slot, Node::Slot);
    // Paths at or above the slot in the shared folder move up one.
    for p in &mut paths {
        if p[shared.len()] >= slot {
            p[shared.len()] += 1;
        }
    }
    let mut taken: Vec<Node> = Vec::new();
    for p in paths.iter().rev() {
        taken.push(take(&mut nodes, p)?);
    }
    taken.reverse();
    let name = if name.is_empty() { default_name(c) } else { name.to_string() };
    let id = new_folder_id(c, &name);
    let list = list_mut(&mut nodes, &shared)?;
    let at = list.iter().position(|n| *n == Node::Slot).ok_or_else(lost)?;
    list[at] = Node::Folder(Folder::new(&id, &name, None), taken);
    commit(c, nodes)?;
    Ok(id)
}

/// Ungroups folder `id`: its members take its place in its parent, in
/// their order.
pub fn ungroup(c: &mut Composition, id: &str) -> Result<(), FolderError> {
    let mut nodes = tree(c)?;
    let p = find(&nodes, id)?;
    let Node::Folder(_, kids) = take(&mut nodes, &p)? else {
        return Err(FolderError::NotAFolder(id.into()));
    };
    let (last, parents) = p.split_last().ok_or_else(lost)?;
    let list = list_mut(&mut nodes, parents)?;
    for (k, n) in kids.into_iter().enumerate() {
        list.insert(last + k, n);
    }
    commit(c, nodes)
}

/// Moves element or folder `id` (its whole run) into folder `into` (none:
/// the top level) at `position` among its children, bottom to top
/// (clamped to the top). Without a position it lands directly above the
/// folder it leaves when `into` holds that folder (out to a parent), else
/// on top of `into`. A folder never goes into itself.
pub fn move_to(
    c: &mut Composition,
    id: &str,
    into: Option<&str>,
    position: Option<usize>,
) -> Result<(), FolderError> {
    let mut nodes = tree(c)?;
    let p = find(&nodes, id)?;
    let target = match into {
        Some(f) => {
            let q = find(&nodes, f)?;
            if !matches!(node_at(&nodes, &q), Some(Node::Folder(..))) {
                return Err(FolderError::NotAFolder(f.into()));
            }
            if q.starts_with(&p) {
                return Err(FolderError::Bad(format!("folder {id:?} cannot go into itself")));
            }
            q
        }
        None => Vec::new(),
    };
    let parent = &p[..p.len() - 1];
    if position.is_none() && target == parent {
        return Ok(());
    }
    // Out to a folder holding it: above the child of `target` it leaves.
    let leaving = (position.is_none() && parent.starts_with(&target))
        .then(|| node_at(&nodes, &p[..target.len() + 1]).and_then(Node::id).map(String::from))
        .flatten();
    let node = take(&mut nodes, &p)?;
    let target = match into {
        Some(f) => find(&nodes, f)?,
        None => Vec::new(),
    };
    let list = list_mut(&mut nodes, &target)?;
    let at = match (position, leaving) {
        (Some(k), _) => k.min(list.len()),
        (None, Some(l)) => list.iter().position(|n| n.id() == Some(&l)).map_or(list.len(), |i| i + 1),
        (None, None) => list.len(),
    };
    list.insert(at, node);
    commit(c, nodes)
}

/// Moves element or folder `id` one step up among its siblings: past the
/// next sibling, a folder's whole run; at the top of a folder it leaves
/// the folder, just above it (the stack unchanged); the top one stays. A
/// distant layer stays below every other (`DISTANT_ORDER`).
pub fn raise(c: &mut Composition, id: &str) -> Result<(), FolderError> {
    step(c, id, true)
}

/// [`raise`]'s mirror: one step down, out of a folder at its bottom.
pub fn lower(c: &mut Composition, id: &str) -> Result<(), FolderError> {
    step(c, id, false)
}

fn step(c: &mut Composition, id: &str, up: bool) -> Result<(), FolderError> {
    let mut nodes = tree(c)?;
    let p = find(&nodes, id)?;
    let (last, parents) = p.split_last().ok_or_else(lost)?;
    let (i, parents) = (*last, parents.to_vec());
    let list = list_mut(&mut nodes, &parents)?;
    let next = if up { (i + 1 < list.len()).then_some(i + 1) } else { i.checked_sub(1) };
    match next {
        Some(j) => {
            if list[i].distant() != list[j].distant() {
                return Err(FolderError::Bad(DISTANT_ORDER.into()));
            }
            list.swap(i, j);
        }
        None => {
            let Some((f, outer)) = parents.split_last() else { return Ok(()) };
            let node = list.remove(i);
            list_mut(&mut nodes, outer)?.insert(if up { f + 1 } else { *f }, node);
        }
    }
    commit(c, nodes)
}

/// Removes folder `id` with everything it holds; returns the elements
/// removed, bottom to top (the caller drops their files).
pub fn remove_folder(c: &mut Composition, id: &str) -> Result<Vec<Element>, FolderError> {
    let mut nodes = tree(c)?;
    let p = find(&nodes, id)?;
    if !matches!(node_at(&nodes, &p), Some(Node::Folder(..))) {
        return Err(FolderError::NotAFolder(id.into()));
    }
    let mut gone = Vec::new();
    take(&mut nodes, &p)?.elements(&mut gone);
    commit(c, nodes)?;
    Ok(gone)
}

/// The ids of `c`'s folders, for callers telling a folder id from an
/// element's.
pub fn folder_ids(c: &Composition) -> BTreeSet<&str> {
    c.folders.iter().map(|f| f.id.as_str()).collect()
}
