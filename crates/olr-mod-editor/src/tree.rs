//! The editor's items and a weapon's (or shared projectile's) projectile
//! tree (issue #17): the shot, its projectile, and under each projectile
//! its event spawn, its trail's spawn and its timer, each node by its
//! field path (`olr_edit::mods::path`). A spawn naming a shared projectile
//! leads into it (its path goes on through the spawn's `projectile`, as
//! `olr_edit::mods::get` follows it); a shared projectile already on the
//! branch (a cycle) is shown once, not followed.

use olr_edit::mods::path::Seg;
use olr_format::mods::{Fire, Gfx, ModSource, Projectile, Spawn, SpawnContext, Trail};

/// What the left list chooses.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Item {
    /// The mod's name, worm, rope, presets and loadouts.
    World,
    /// A weapon, by id.
    Weapon(String),
    /// A shared projectile, by id.
    Shared(String),
}

impl Item {
    /// The path of the item's own node (`mod`, `weapons/<id>`,
    /// `projectiles/<id>`).
    pub fn path(&self) -> String {
        match self {
            Item::World => "mod".into(),
            Item::Weapon(id) => format!("weapons/{id}"),
            Item::Shared(id) => format!("projectiles/{id}"),
        }
    }
}

/// Where a spawn is: its defaults depend on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpawnAt {
    /// A weapon's shot.
    Shot,
    /// A projectile's event spawn.
    Event,
    /// A projectile trail's spawn.
    Trail,
}

/// A node's kind: what its panel edits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodeKind {
    World,
    Weapon,
    Spawn(SpawnAt),
    /// A projectile; `shared` its id when it is a shared one.
    Projectile {
        shared: Option<String>,
    },
    Timer,
}

/// One node of the tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub path: String,
    pub kind: NodeKind,
    pub label: String,
    /// 0 for the item's own node.
    pub depth: usize,
}

/// The item's nodes, depth first. An item that is gone has none.
pub fn nodes(src: &ModSource, item: &Item) -> Vec<Node> {
    let mut out = Vec::new();
    match item {
        Item::World => {
            out.push(Node { path: item.path(), kind: NodeKind::World, label: "World".into(), depth: 0 })
        }
        Item::Weapon(id) => {
            let Some((_, w)) = src.weapons.iter().find(|(w, _)| w == id) else { return out };
            let path = item.path();
            out.push(Node { path: path.clone(), kind: NodeKind::Weapon, label: w.name.clone(), depth: 0 });
            if let Fire::Projectile { shot, .. } = &w.fire {
                spawn(src, &format!("{path}.fire.shot"), SpawnAt::Shot, shot, 1, &mut Vec::new(), &mut out);
            }
        }
        Item::Shared(id) => {
            let Some(p) = src.shared.get(id) else { return out };
            projectile(src, &item.path(), p, Some(id), 0, &mut vec![id.clone()], &mut out);
        }
    }
    out
}

/// The path of the node `path` lies in among `nodes` (the longest node
/// path it starts with, at a step's boundary).
pub fn node_of<'a>(nodes: &'a [Node], path: &str) -> Option<&'a Node> {
    nodes
        .iter()
        .filter(|n| {
            path.strip_prefix(n.path.as_str())
                .is_some_and(|rest| rest.is_empty() || rest.starts_with(['.', '[']))
        })
        .max_by_key(|n| n.path.len())
}

/// A projectile's short description: its image's file, or its pixels.
pub fn describe(p: &Projectile) -> String {
    match &p.gfx {
        Gfx::Image { file, .. } => {
            let name = file.rsplit('/').next().unwrap_or(file);
            if name.is_empty() { "Image (no file)".into() } else { name.to_string() }
        }
        Gfx::Pixels(c) if c.len() == 1 => "Pixel".into(),
        Gfx::Pixels(_) => "Pixels".into(),
    }
}

fn spawn<C: SpawnContext>(
    src: &ModSource,
    path: &str,
    at: SpawnAt,
    s: &Spawn<C>,
    depth: usize,
    stack: &mut Vec<String>,
    out: &mut Vec<Node>,
) {
    let what = match at {
        SpawnAt::Shot => "Shot",
        SpawnAt::Event => "Spawn",
        SpawnAt::Trail => "Trail",
    };
    let label = if s.amount == 1 { what.to_string() } else { format!("{what} × {}", s.amount) };
    out.push(Node { path: path.to_string(), kind: NodeKind::Spawn(at), label, depth });
    let p_path = format!("{path}.projectile");
    match (&s.shared, &s.projectile) {
        (Some(id), _) => {
            let Some(p) = src.shared.get(id) else { return };
            if stack.contains(id) || depth > olr_format::mods::MAX_INLINE_DEPTH * 3 {
                // A cycle (an error the check lists): shown, not followed.
                out.push(Node {
                    path: p_path,
                    kind: NodeKind::Projectile { shared: Some(id.clone()) },
                    label: format!("{} (shared: {id}, a cycle)", describe(p)),
                    depth: depth + 1,
                });
                return;
            }
            stack.push(id.clone());
            projectile(src, &p_path, p, Some(id), depth + 1, stack, out);
            stack.pop();
        }
        (None, Some(p)) => {
            if depth <= olr_format::mods::MAX_INLINE_DEPTH * 3 {
                projectile(src, &p_path, p, None, depth + 1, stack, out);
            }
        }
        (None, None) => {}
    }
}

fn projectile(
    src: &ModSource,
    path: &str,
    p: &Projectile,
    shared: Option<&String>,
    depth: usize,
    stack: &mut Vec<String>,
    out: &mut Vec<Node>,
) {
    let label = match shared {
        Some(id) => format!("{} (shared: {id})", describe(p)),
        None => describe(p),
    };
    out.push(Node {
        path: path.to_string(),
        kind: NodeKind::Projectile { shared: shared.cloned() },
        label,
        depth,
    });
    for (key, s) in &p.spawns {
        let seg = Seg::Name(key.clone());
        spawn(src, &format!("{path}.spawns{seg}"), SpawnAt::Event, s, depth + 1, stack, out);
    }
    if let Trail::Projectile { spawn: s, .. } = &p.trail {
        spawn(src, &format!("{path}.trail.spawn"), SpawnAt::Trail, s, depth + 1, stack, out);
    }
    if let Some(t) = &p.timer {
        let label = format!("Timer {} s", t.after_s);
        out.push(Node { path: format!("{path}.timer"), kind: NodeKind::Timer, label, depth: depth + 1 });
    }
}
