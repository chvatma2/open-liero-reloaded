//! `cargo xtask api`: compact public-API summaries so agents can learn a crate
//! without reading its source. One file per library crate in `target/api/`.

use std::path::{Path, PathBuf};

use anyhow::Context;
use quote::ToTokens;
use syn::{Attribute, ImplItem, Item, TraitItem, Visibility};

use crate::root;

fn is_pub(v: &Visibility) -> bool {
    matches!(v, Visibility::Public(_))
}

fn is_test_cfg(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| a.path().is_ident("cfg") && a.meta.to_token_stream().to_string().contains("test"))
}

/// Keeps only the first line of doc comments and `derive` attributes.
fn trim_attrs(attrs: &mut Vec<Attribute>) {
    let mut seen_doc = false;
    attrs.retain(|a| {
        if a.path().is_ident("doc") {
            let keep = !seen_doc;
            seen_doc = true;
            keep
        } else {
            a.path().is_ident("derive")
        }
    });
}

fn empty_block() -> syn::Block {
    syn::parse_quote!({})
}

/// Reduces an item to its public surface; returns `None` if nothing is public.
fn summarize(item: &Item) -> Option<Item> {
    let mut item = item.clone();
    let keep = match &mut item {
        Item::Fn(f) => {
            *f.block = empty_block();
            trim_attrs(&mut f.attrs);
            is_pub(&f.vis)
        }
        Item::Struct(s) => {
            if let syn::Fields::Named(n) = &mut s.fields {
                n.named = n
                    .named
                    .iter()
                    .filter(|f| is_pub(&f.vis))
                    .cloned()
                    .map(|mut f| {
                        trim_attrs(&mut f.attrs);
                        f
                    })
                    .collect();
            }
            trim_attrs(&mut s.attrs);
            is_pub(&s.vis)
        }
        Item::Enum(e) => {
            for v in &mut e.variants {
                trim_attrs(&mut v.attrs);
                v.attrs.retain(|a| a.path().is_ident("doc"));
            }
            trim_attrs(&mut e.attrs);
            is_pub(&e.vis)
        }
        Item::Trait(t) => {
            for ti in &mut t.items {
                if let TraitItem::Fn(f) = ti {
                    f.default = None;
                    f.semi_token = Some(Default::default());
                    trim_attrs(&mut f.attrs);
                }
            }
            trim_attrs(&mut t.attrs);
            is_pub(&t.vis)
        }
        Item::Impl(i) => {
            let is_trait_impl = i.trait_.is_some();
            i.items.retain_mut(|ii| match ii {
                ImplItem::Fn(f) if is_trait_impl || is_pub(&f.vis) => {
                    f.block = empty_block();
                    trim_attrs(&mut f.attrs);
                    !is_trait_impl
                }
                ImplItem::Const(c) => is_pub(&c.vis),
                _ => false,
            });
            i.attrs.clear();
            is_trait_impl || !i.items.is_empty()
        }
        Item::Const(c) => {
            trim_attrs(&mut c.attrs);
            is_pub(&c.vis)
        }
        Item::Type(t) => {
            trim_attrs(&mut t.attrs);
            is_pub(&t.vis)
        }
        Item::Use(u) => is_pub(&u.vis),
        _ => false,
    };
    keep.then_some(item)
}

/// Collects `(module path, items)` for `file` and the modules it declares.
fn walk(path: &str, file: &Path, out: &mut Vec<(String, Vec<Item>)>) -> anyhow::Result<()> {
    let text = std::fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?;
    let ast = syn::parse_file(&text).with_context(|| format!("parsing {}", file.display()))?;
    let dir: PathBuf = if file.ends_with("lib.rs") || file.ends_with("mod.rs") {
        file.parent().expect("dir").to_path_buf()
    } else {
        file.with_extension("")
    };
    let mut items = Vec::new();
    for item in &ast.items {
        if let Item::Mod(m) = item {
            if is_test_cfg(&m.attrs) {
                continue;
            }
            let sub = format!("{path}::{}", m.ident);
            if let Some((_, inner)) = &m.content {
                let mut nested = Vec::new();
                nested.extend(inner.iter().filter_map(summarize));
                out.push((sub, nested));
            } else {
                let a = dir.join(format!("{}.rs", m.ident));
                let b = dir.join(m.ident.to_string()).join("mod.rs");
                walk(&sub, if a.is_file() { &a } else { &b }, out)?;
            }
        } else if let Some(s) = summarize(item) {
            items.push(s);
        }
    }
    out.insert(0, (path.to_string(), items));
    Ok(())
}

pub fn run() -> anyhow::Result<()> {
    let root = root();
    let out_dir = root.join("target/api");
    std::fs::create_dir_all(&out_dir)?;
    let mut crates: Vec<PathBuf> =
        std::fs::read_dir(root.join("crates"))?.map(|e| e.map(|e| e.path())).collect::<Result<_, _>>()?;
    crates.sort();
    for dir in crates {
        let lib = dir.join("src/lib.rs");
        if !lib.is_file() {
            continue;
        }
        let name = dir.file_name().expect("name").to_string_lossy().replace('-', "_");
        let mut mods = Vec::new();
        walk(&name, &lib, &mut mods)?;
        mods.sort_by(|a, b| a.0.cmp(&b.0));
        let mut text = format!("// Public API of {name} (generated by `cargo xtask api`; do not edit)\n");
        for (path, items) in mods {
            if items.is_empty() {
                continue;
            }
            let file = syn::File { shebang: None, attrs: vec![], items };
            text.push_str(&format!("\n// ---- {path}\n{}", prettyplease::unparse(&file)));
        }
        let target = out_dir.join(format!("{name}.txt"));
        std::fs::write(&target, text)?;
        println!("wrote {}", target.display());
    }
    Ok(())
}
