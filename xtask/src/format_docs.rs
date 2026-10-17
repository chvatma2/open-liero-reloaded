//! `cargo xtask format-docs [--check]`: the schema pages in `docs/formats/`,
//! generated from `olr-format`'s authoring types (plan 13d R4).

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;

use anyhow::{Context as _, bail};
use quote::ToTokens as _;
use syn::{Attribute, Expr, Fields, GenericArgument, Lit, PathArguments, Type};

/// One generated page: a pack kind's files and the types they hold.
struct Page {
    /// Under `docs/formats/`.
    out: &'static str,
    title: &'static str,
    /// The source whose module doc comment introduces the page.
    module: &'static str,
    /// Every source read for the page's types and defaults.
    sources: &'static [&'static str],
    /// The file types, with the file each one is.
    roots: &'static [(&'static str, &'static str)],
    /// Files whose type is no item: `(file, type, doc)`.
    maps: &'static [(&'static str, &'static str, &'static str)],
    /// The kind's other files.
    files: &'static [&'static str],
}

const DEFAULTS: &str = "crates/olr-format/src/defaults.rs";

const PAGES: &[Page] = &[
    Page {
        out: "mod.md",
        title: "Mod packs",
        module: "crates/olr-format/src/mods/mod.rs",
        sources: &[
            "crates/olr-format/src/mods/mod.rs",
            "crates/olr-format/src/mods/files.rs",
            "crates/olr-format/src/mods/types.rs",
            "crates/olr-format/src/mods/spawn.rs",
            "crates/olr-format/src/colour.rs",
            "crates/olr-data/src/restrictions.rs",
        ],
        roots: &[
            ("ModFile", "mod.ron"),
            ("Weapon", "weapons/<id>.ron"),
            ("Projectile", "projectiles/<id>.ron"),
        ],
        maps: &[(
            "restrictions.ron",
            "BTreeMap<String, Preset>",
            "The mod's weapon-restriction presets by name (OLX's `.wps` files), optional; the game's \
             default is `Standard 100lt`.",
        )],
        files: &["gfx/<file>.png", "sfx/<file>"],
    },
    Page {
        out: "level.md",
        title: "Level packs",
        module: "crates/olr-format/src/levels/mod.rs",
        sources: &["crates/olr-format/src/levels/mod.rs", "crates/olr-format/src/levels/files.rs"],
        roots: &[("LevelFile", "level.ron")],
        maps: &[],
        files: &["material.png", "front.png", "back.png", "thumb.png"],
    },
    Page {
        out: "skin.md",
        title: "Skin packs",
        module: "crates/olr-format/src/skins/mod.rs",
        sources: &["crates/olr-format/src/skins/mod.rs", "crates/olr-format/src/skins/files.rs"],
        roots: &[("SkinFile", "skin.ron")],
        maps: &[],
        files: &["skin.png", "tint.png", "left.png"],
    },
    Page {
        out: "theme.md",
        title: "Theme packs",
        module: "crates/olr-format/src/themes/mod.rs",
        sources: &["crates/olr-format/src/themes/mod.rs", "crates/olr-format/src/themes/files.rs"],
        roots: &[("ThemeFile", "theme.ron")],
        maps: &[],
        files: &[
            "holes/<i>.png",
            "holes/<i>.mask.png",
            "green_ball.png",
            "green_ball.mask.png",
            "stones/<i>.png",
            "misc/<i>.png",
            "front_tile.png",
            "back_tile.png",
        ],
    },
    Page {
        out: "core.md",
        title: "The core pack",
        module: "crates/olr-format/src/core_pack/mod.rs",
        sources: &[
            "crates/olr-format/src/core_pack/mod.rs",
            "crates/olr-format/src/core_pack/files.rs",
            "crates/olr-format/src/colour.rs",
        ],
        roots: &[("CoreFile", "core.ron")],
        maps: &[],
        files: &["gfx/<sprite>.png", "sounds/<file>"],
    },
    Page {
        out: "pack.md",
        title: "Pack manifests",
        module: "crates/olr-data/src/manifest.rs",
        sources: &["crates/olr-data/src/manifest.rs"],
        roots: &[("PackManifest", "pack.ron")],
        maps: &[],
        files: &[],
    },
];

/// `cargo xtask format-docs [--check]`.
pub fn run(check: bool) -> anyhow::Result<bool> {
    let root = crate::root();
    let dir = root.join("docs/formats");
    let mut stale = Vec::new();
    let mut errors = Vec::new();
    for page in PAGES {
        let text = match render_page(&root, page) {
            Ok(text) => text,
            Err(e) => {
                errors.push(format!("{e:#}"));
                continue;
            }
        };
        let path = dir.join(page.out);
        if check {
            if std::fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) {
                stale.push(format!("docs/formats/{}", page.out));
            }
        } else {
            std::fs::create_dir_all(&dir)?;
            std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
        }
    }
    if !errors.is_empty() {
        bail!("{}", errors.join("\n"));
    }
    if !stale.is_empty() {
        println!("{} differ from the code: run cargo xtask format-docs", stale.join(", "));
        return Ok(false);
    }
    Ok(true)
}

/// The `check` step: `format-docs --check`.
pub fn step() -> Result<(), String> {
    match run(true) {
        Ok(true) => Ok(()),
        Ok(false) => Err("docs/formats/ differs from the code: run cargo xtask format-docs".into()),
        Err(e) => Err(format!("{e:#}")),
    }
}

fn render_page(root: &Path, page: &Page) -> anyhow::Result<String> {
    let mut sources = Sources::default();
    for label in std::iter::once(&DEFAULTS).chain(page.sources) {
        let text = std::fs::read_to_string(root.join(label)).with_context(|| format!("reading {label}"))?;
        sources.add(label, &text)?;
    }
    let module = std::fs::read_to_string(root.join(page.module))?;
    let module = syn::parse_file(&module).with_context(|| format!("parsing {}", page.module))?;
    let mut out = format!("# {} (generated by `cargo xtask format-docs`; do not edit)\n\n", page.title);
    let files: Vec<String> = page
        .roots
        .iter()
        .map(|(ty, file)| format!("`{file}` ([{ty}](#{}))", ty.to_lowercase()))
        .chain(page.maps.iter().map(|(file, _, _)| format!("[`{file}`](#{})", file.replace('.', "-"))))
        .chain(page.files.iter().map(|f| format!("`{f}`")))
        .collect();
    out += &format!("Files: {}.\n\n", files.join(", "));
    for p in paragraphs(&doc_lines(&module.attrs)) {
        out += &p;
        out += "\n\n";
    }
    out += &sources.sections_with(page.roots, page.maps, &BTreeMap::new())?;
    Ok(out)
}

/// The lines of the doc comments in `attrs`, one leading space removed.
fn doc_lines(attrs: &[Attribute]) -> Vec<String> {
    attrs
        .iter()
        .filter(|a| a.path().is_ident("doc"))
        .filter_map(|a| match &a.meta {
            syn::Meta::NameValue(nv) => match &nv.value {
                Expr::Lit(syn::ExprLit { lit: Lit::Str(s), .. }) => Some(s.value()),
                _ => None,
            },
            _ => None,
        })
        .map(|l| l.strip_prefix(' ').unwrap_or(&l).trim_end().to_string())
        .collect()
}

/// Paragraphs (separated by blank lines), each one line, intra-doc links
/// reduced to their text.
fn paragraphs(lines: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for l in lines.iter().map(|l| l.trim()).chain(std::iter::once("")) {
        if l.is_empty() {
            if !current.is_empty() {
                out.push(strip_doc_links(&current.join(" ")));
                current.clear();
            }
        } else {
            current.push(l);
        }
    }
    out
}

/// A doc comment as one table cell.
fn cell(lines: &[String]) -> String {
    paragraphs(lines).join(" ").replace('|', "\\|")
}

/// `[`x`]` (an intra-doc link) becomes `` `x` ``; Markdown links stay.
fn strip_doc_links(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("[`") {
        let Some(j) = rest[i..].find("`]") else { break };
        let end = i + j + 2;
        let next = rest[end..].chars().next();
        out += &rest[..i];
        if matches!(next, Some('(' | '[')) {
            out += &rest[i..end];
        } else {
            out += &rest[i + 1..end - 1];
        }
        rest = &rest[end..];
    }
    out + rest
}

/// A field's serde default.
#[derive(Clone, Debug, PartialEq)]
enum Default {
    None,
    Bare,
    Fn(String),
}

#[derive(Clone)]
struct Field {
    name: String,
    ty: Type,
    doc: Vec<String>,
    default: Default,
    line: usize,
}

#[derive(Clone)]
enum VariantFields {
    Unit,
    Tuple(Vec<Type>),
    Named(Vec<Field>),
}

#[derive(Clone)]
struct Variant {
    name: String,
    doc: Vec<String>,
    fields: VariantFields,
    line: usize,
}

#[derive(Clone)]
enum Body {
    Struct(Vec<Field>),
    Enum(Vec<Variant>),
    /// Hand-written serde: documented by its doc comment alone.
    Custom,
}

#[derive(Clone)]
struct Item {
    name: String,
    file: String,
    doc: Vec<String>,
    body: Body,
    /// `Default`: an enum's `#[default]` variant, or a struct literal.
    default: Option<String>,
    /// `#[serde(from = "W")]`: the text form is `W`'s fields.
    from: Option<String>,
}

/// The types and default values of a set of sources.
#[derive(Default)]
struct Sources {
    items: BTreeMap<String, Item>,
    /// Default functions by name: their value as written.
    values: BTreeMap<String, String>,
    /// `impl Default for T` whose value is not a literal.
    opaque_defaults: BTreeSet<String>,
    /// `impl Default for T`'s value of each field: `(T, field)`.
    field_defaults: BTreeMap<(String, String), String>,
    /// `type A = T;`: the target and the doc comment.
    aliases: BTreeMap<String, (Type, Vec<String>)>,
}

/// A container's `#[serde(from = "W")]`.
fn serde_from(attrs: &[Attribute]) -> anyhow::Result<Option<String>> {
    let mut from = None;
    for a in attrs.iter().filter(|a| a.path().is_ident("serde")) {
        a.parse_nested_meta(|meta| {
            let value = if meta.input.peek(syn::Token![=]) {
                Some(meta.value()?.parse::<syn::LitStr>()?.value())
            } else {
                None
            };
            if meta.path.is_ident("from") {
                from = value;
            }
            Ok(())
        })?;
    }
    Ok(from)
}

/// Whether `attrs` derive serde.
fn derives_serde(attrs: &[Attribute]) -> bool {
    attrs
        .iter()
        .any(|a| a.path().is_ident("derive") && a.meta.to_token_stream().to_string().contains("Deserialize"))
}

/// The line (1-based) of the first line at or after `from` that `matches`.
fn find_line(text: &str, from: usize, matches: impl Fn(&str) -> bool) -> usize {
    text.lines().enumerate().skip(from.saturating_sub(1)).find(|(_, l)| matches(l)).map_or(0, |(i, _)| i + 1)
}

/// Whether `line` declares `name` (a field `name:`, a variant `Name`).
fn declares(line: &str, name: &str, field: bool) -> bool {
    let l = line.trim_start();
    let l = l.strip_prefix("pub(crate) ").or_else(|| l.strip_prefix("pub ")).unwrap_or(l);
    l.strip_prefix(name).is_some_and(|rest| {
        if field {
            rest.trim_start().starts_with(':')
        } else {
            !rest.starts_with(|c: char| c.is_alphanumeric() || c == '_')
        }
    })
}

/// A literal expression as written (`-75.0`, `"x"`), if it is one.
fn literal(e: &Expr) -> Option<String> {
    match e {
        Expr::Lit(l) => Some(l.lit.to_token_stream().to_string()),
        Expr::Unary(u) if matches!(u.op, syn::UnOp::Neg(_)) => literal(&u.expr).map(|v| format!("-{v}")),
        _ => None,
    }
}

impl Sources {
    fn add(&mut self, label: &str, text: &str) -> anyhow::Result<()> {
        let file = syn::parse_file(text).with_context(|| format!("parsing {label}"))?;
        self.add_items(label, text, &file.items)
    }

    fn add_items(&mut self, label: &str, text: &str, items: &[syn::Item]) -> anyhow::Result<()> {
        for item in items {
            match item {
                syn::Item::Struct(s) => {
                    let name = s.ident.to_string();
                    let start = find_line(text, 1, |l| {
                        l.contains(&format!("struct {name}"))
                            && declares(l.split("struct ").nth(1).unwrap_or(""), &name, false)
                    });
                    let body = match (&s.fields, derives_serde(&s.attrs)) {
                        (Fields::Named(f), true) => Body::Struct(self.fields(text, start, &f.named)?),
                        _ => Body::Custom,
                    };
                    let doc = doc_lines(&s.attrs);
                    let from = serde_from(&s.attrs)?;
                    self.items.insert(
                        name.clone(),
                        Item { name, file: label.into(), doc, body, default: None, from },
                    );
                }
                syn::Item::Enum(e) if derives_serde(&e.attrs) => {
                    let name = e.ident.to_string();
                    let start = find_line(text, 1, |l| {
                        l.contains(&format!("enum {name}"))
                            && declares(l.split("enum ").nth(1).unwrap_or(""), &name, false)
                    });
                    let mut variants = Vec::new();
                    let mut default = None;
                    for v in &e.variants {
                        let vname = v.ident.to_string();
                        let line = find_line(text, start + 1, |l| declares(l, &vname, false));
                        if v.attrs.iter().any(|a| a.path().is_ident("default")) {
                            default = Some(vname.clone());
                        }
                        let fields = match &v.fields {
                            Fields::Unit => VariantFields::Unit,
                            Fields::Unnamed(f) => {
                                VariantFields::Tuple(f.unnamed.iter().map(|f| f.ty.clone()).collect())
                            }
                            Fields::Named(f) => VariantFields::Named(self.fields(text, line, &f.named)?),
                        };
                        variants.push(Variant { name: vname, doc: doc_lines(&v.attrs), fields, line });
                    }
                    let doc = doc_lines(&e.attrs);
                    self.items.insert(
                        name.clone(),
                        Item {
                            name,
                            file: label.into(),
                            doc,
                            body: Body::Enum(variants),
                            default,
                            from: None,
                        },
                    );
                }
                syn::Item::Impl(i) => self.add_default_impl(i),
                syn::Item::Type(t) => {
                    self.aliases.insert(t.ident.to_string(), ((*t.ty).clone(), doc_lines(&t.attrs)));
                }
                syn::Item::Fn(f) if f.sig.inputs.is_empty() => {
                    if let [syn::Stmt::Expr(e, None)] = f.block.stmts.as_slice()
                        && let Some(v) = literal(e)
                    {
                        self.values.insert(f.sig.ident.to_string(), v);
                    }
                }
                syn::Item::Macro(m) => self.add_default_macro(&m.mac),
                syn::Item::Mod(m)
                    if !m.attrs.iter().any(|a| a.meta.to_token_stream().to_string().contains("test")) =>
                {
                    if let Some((_, items)) = &m.content {
                        self.add_items(label, text, items)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// The serialized fields of a struct (or struct variant) declared at
    /// line `start`.
    fn fields(
        &self,
        text: &str,
        start: usize,
        fields: &syn::punctuated::Punctuated<syn::Field, syn::Token![,]>,
    ) -> anyhow::Result<Vec<Field>> {
        let mut out = Vec::new();
        for f in fields {
            let Some(ident) = &f.ident else { continue };
            let name = ident.to_string();
            let mut default = Default::None;
            let mut skip = false;
            for a in f.attrs.iter().filter(|a| a.path().is_ident("serde")) {
                a.parse_nested_meta(|meta| {
                    if meta.path.is_ident("default") {
                        default = if meta.input.peek(syn::Token![=]) {
                            Default::Fn(meta.value()?.parse::<syn::LitStr>()?.value())
                        } else {
                            Default::Bare
                        };
                    } else {
                        if meta.path.is_ident("skip") {
                            skip = true;
                        }
                        if meta.input.peek(syn::Token![=]) {
                            meta.value()?.parse::<Expr>()?;
                        }
                    }
                    Ok(())
                })?;
            }
            if skip {
                continue;
            }
            let line = find_line(text, start + 1, |l| declares(l, &name, true));
            out.push(Field { name, ty: f.ty.clone(), doc: doc_lines(&f.attrs), default, line });
        }
        Ok(out)
    }

    /// `impl Default for T { fn default() -> Self { Self { a: 1, .. } } }`.
    fn add_default_impl(&mut self, i: &syn::ItemImpl) {
        let Some((_, path, _)) = &i.trait_ else { return };
        if !path.is_ident("Default") {
            return;
        }
        let Type::Path(ty) = &*i.self_ty else { return };
        let Some(name) = ty.path.segments.last().map(|s| s.ident.to_string()) else { return };
        let generics: BTreeSet<String> = i.generics.type_params().map(|p| p.ident.to_string()).collect();
        for it in &i.items {
            if let syn::ImplItem::Fn(f) = it
                && let [syn::Stmt::Expr(Expr::Struct(st), None)] = f.block.stmts.as_slice()
            {
                for fv in &st.fields {
                    let value = match (&fv.expr, literal(&fv.expr)) {
                        (_, Some(v)) => format!("`{v}`"),
                        // `C::RELATIVE_TO_PARENT`: the generic context's.
                        (Expr::Path(p), None)
                            if p.path.segments.len() > 1
                                && generics.contains(&p.path.segments[0].ident.to_string()) =>
                        {
                            "by context".into()
                        }
                        _ => continue,
                    };
                    self.field_defaults
                        .insert((name.clone(), fv.member.to_token_stream().to_string()), value);
                }
            }
        }
        let value = i.items.iter().find_map(|it| match it {
            syn::ImplItem::Fn(f) => match f.block.stmts.as_slice() {
                [syn::Stmt::Expr(Expr::Struct(s), None)] if s.rest.is_none() => s
                    .fields
                    .iter()
                    .map(|fv| Some(format!("{}: {}", fv.member.to_token_stream(), literal(&fv.expr)?)))
                    .collect::<Option<Vec<_>>>()
                    .map(|v| format!("({})", v.join(", "))),
                _ => None,
            },
            _ => None,
        });
        match value {
            Some(v) => {
                self.values.insert(format!("{name}::default"), v);
            }
            None => {
                self.opaque_defaults.insert(name);
            }
        }
    }

    /// `f32_defaults! { name / is_name = value; ... }` and the like.
    fn add_default_macro(&mut self, mac: &syn::Macro) {
        if !mac.path.segments.last().is_some_and(|s| s.ident.to_string().ends_with("_defaults")) {
            return;
        }
        let tokens: Vec<proc_macro2::TokenTree> = mac.tokens.clone().into_iter().collect();
        for entry in tokens.split(|t| matches!(t, proc_macro2::TokenTree::Punct(p) if p.as_char() == ';')) {
            let Some(proc_macro2::TokenTree::Ident(get)) = entry.first() else { continue };
            let Some(eq) = entry
                .iter()
                .position(|t| matches!(t, proc_macro2::TokenTree::Punct(p) if p.as_char() == '='))
            else {
                continue;
            };
            let value: String = entry[eq + 1..].iter().map(|t| t.to_string()).collect();
            self.values.insert(get.to_string(), value);
        }
    }

    /// The sections for `roots` and every type they reach, in order of
    /// first reference; `examples` by type name.
    #[cfg(test)]
    fn sections(
        &self,
        roots: &[(&str, &str)],
        examples: &BTreeMap<String, String>,
    ) -> anyhow::Result<String> {
        self.sections_with(roots, &[], examples)
    }

    /// As [`Sources::sections`], after a section for each file whose type is
    /// no item (`(file, type, doc)`, such as a map).
    fn sections_with(
        &self,
        roots: &[(&str, &str)],
        maps: &[(&str, &str, &str)],
        examples: &BTreeMap<String, String>,
    ) -> anyhow::Result<String> {
        let files: BTreeMap<&str, &str> = roots.iter().copied().collect();
        let mut queue: VecDeque<String> = roots.iter().map(|(t, _)| t.to_string()).collect();
        let mut seen: BTreeSet<String> = queue.iter().cloned().collect();
        let mut out = Vec::new();
        let mut errors = Vec::new();
        let mut refs = Vec::new();
        for (file, ty, doc) in maps {
            let ty: Type = syn::parse_str(ty).with_context(|| format!("{file}: parsing {ty}"))?;
            out.push(format!(
                "<a id=\"{}\"></a>\n\n## `{file}`\n\n{doc}\n\nType: {}.\n",
                file.replace('.', "-"),
                self.label(&ty, &mut refs)
            ));
        }
        let enqueue = |refs: Vec<String>, queue: &mut VecDeque<String>, seen: &mut BTreeSet<String>| {
            for r in refs {
                if self.has_section(&r) && seen.insert(r.clone()) {
                    queue.push_back(r);
                }
            }
        };
        enqueue(std::mem::take(&mut refs), &mut queue, &mut seen);
        while let Some(name) = queue.pop_front() {
            let mut refs = Vec::new();
            if let Some((target, doc)) = self.aliases.get(&name) {
                let mut s = format!("<a id=\"{}\"></a>\n\n## {name}\n\n", name.to_lowercase());
                for p in paragraphs(doc) {
                    s += &p;
                    s += "\n\n";
                }
                s += &format!("Type: {}.\n", self.label(target, &mut refs));
                out.push(s);
            } else {
                let item =
                    self.items.get(&name).with_context(|| format!("no type `{name}` in the sources"))?;
                out.push(self.section(
                    item,
                    files.get(name.as_str()).copied(),
                    examples,
                    &mut refs,
                    &mut errors,
                ));
            }
            enqueue(refs, &mut queue, &mut seen);
        }
        if !errors.is_empty() {
            bail!("{}", errors.join("\n"));
        }
        Ok(out.join("\n"))
    }

    /// Whether `name` has a section: an item, or an alias of no item.
    fn has_section(&self, name: &str) -> bool {
        self.items.contains_key(name) || self.aliases.get(name).is_some_and(|(t, _)| !self.aliases_item(t))
    }

    /// Whether an alias's target is an item (its section serves the alias).
    fn aliases_item(&self, target: &Type) -> bool {
        outer(target).is_some_and(|(n, _)| self.items.contains_key(&n))
    }

    /// `item`'s fields as its text holds them: with `from = "W"`, `W`'s
    /// fields, each documented, typed and defaulted by `item`'s field of the
    /// same name when there is one.
    fn text_fields(&self, item: &Item, fields: &[Field]) -> Vec<Field> {
        let Some(Item { body: Body::Struct(wire), .. }) = item.from.as_ref().and_then(|w| self.items.get(w))
        else {
            return fields.to_vec();
        };
        wire.iter()
            .map(|w| match fields.iter().find(|f| f.name == w.name) {
                Some(f) => Field {
                    doc: if w.doc.is_empty() { f.doc.clone() } else { w.doc.clone() },
                    default: match self.field_defaults.get(&(item.name.clone(), f.name.clone())) {
                        Some(v) => Default::Fn(format!("={v}")),
                        None => w.default.clone(),
                    },
                    ..f.clone()
                },
                None => w.clone(),
            })
            .collect()
    }

    fn section(
        &self,
        item: &Item,
        file: Option<&str>,
        examples: &BTreeMap<String, String>,
        refs: &mut Vec<String>,
        errors: &mut Vec<String>,
    ) -> String {
        let anchor = item.name.to_lowercase();
        let heading = match (&item.body, file) {
            (_, Some(f)) => format!("{} (`{f}`)", item.name),
            (Body::Enum(_), None) => format!("{} (enum)", item.name),
            (Body::Custom, None) => anchor.clone(),
            (Body::Struct(_), None) => item.name.clone(),
        };
        let mut s = format!("<a id=\"{anchor}\"></a>\n\n## {heading}\n\n");
        for p in paragraphs(&item.doc) {
            s += &p;
            s += "\n\n";
        }
        match &item.body {
            Body::Struct(fields) => s += &self.table(item, &self.text_fields(item, fields), refs, errors),
            Body::Enum(variants) => {
                s += "| Variant | Fields | Meaning |\n|---|---|---|\n";
                let mut subtables = String::new();
                for v in variants {
                    if v.doc.is_empty() {
                        errors.push(format!(
                            "{}:{}: variant `{}` of `{}` has no doc comment",
                            item.file, v.line, v.name, item.name
                        ));
                    }
                    let fields = match &v.fields {
                        VariantFields::Unit => "—".to_string(),
                        VariantFields::Tuple(types) => {
                            types.iter().map(|t| self.label(t, refs)).collect::<Vec<_>>().join(", ")
                        }
                        VariantFields::Named(fields) => {
                            subtables += &format!("\n### `{}::{}`\n\n", item.name, v.name);
                            subtables += &self.table(item, fields, refs, errors);
                            fields.iter().map(|f| format!("`{}`", f.name)).collect::<Vec<_>>().join(", ")
                        }
                    };
                    s += &format!("| `{}` | {fields} | {} |\n", v.name, cell(&v.doc));
                }
                s += &subtables;
            }
            Body::Custom => {}
        }
        if let Some(example) = examples.get(&item.name) {
            s += &format!("\nExample:\n\n```ron\n{example}```\n");
        }
        s.trim_end().to_string() + "\n"
    }

    fn table(
        &self,
        item: &Item,
        fields: &[Field],
        refs: &mut Vec<String>,
        errors: &mut Vec<String>,
    ) -> String {
        let mut s = "| Field | Type | Default | Meaning |\n|---|---|---|---|\n".to_string();
        for f in fields {
            if f.doc.is_empty() {
                errors.push(format!(
                    "{}:{}: field `{}` of `{}` has no doc comment",
                    item.file, f.line, f.name, item.name
                ));
            }
            s += &format!(
                "| `{}` | {} | {} | {} |\n",
                f.name,
                self.label(&f.ty, refs),
                self.default_of(f),
                cell(&f.doc)
            );
        }
        s
    }

    fn default_of(&self, f: &Field) -> String {
        match &f.default {
            Default::None if outer(&f.ty).is_some_and(|(n, _)| n == "Option") => "none".into(),
            Default::None => "required".into(),
            // A value already rendered (`text_fields`).
            Default::Fn(v) if v.starts_with('=') => v[1..].to_string(),
            Default::Fn(path) => {
                let name = path.rsplit("::").next().unwrap_or(path);
                match self.values.get(name) {
                    Some(v) => format!("`{v}`"),
                    None => format!("see `{name}()`"),
                }
            }
            Default::Bare => match self.zero(&f.ty) {
                Some(z) => format!("`{z}`"),
                None if outer(&f.ty).is_some_and(|(n, _)| n == "Option") => "none".into(),
                None if self.every_field_defaults(&f.ty) => "each field's default".into(),
                None => format!("see `{}::default`", outer(&f.ty).map_or_else(|| "?".into(), |(n, _)| n)),
            },
        }
    }

    /// Whether `ty` is a struct whose every field has a serde default.
    fn every_field_defaults(&self, ty: &Type) -> bool {
        matches!(
            outer(ty).and_then(|(n, _)| self.items.get(&n)),
            Some(Item { body: Body::Struct(fields), .. }) if fields.iter().all(|f| f.default != Default::None)
        )
    }

    /// The default value of `ty` as written in RON, when known.
    fn zero(&self, ty: &Type) -> Option<String> {
        if let Type::Tuple(t) = ty {
            let parts = t.elems.iter().map(|e| self.zero(e)).collect::<Option<Vec<_>>>()?;
            return Some(format!("({})", parts.join(", ")));
        }
        let (name, _) = outer(ty)?;
        Some(match name.as_str() {
            "String" => "\"\"".into(),
            "f32" | "f64" => "0.0".into(),
            "bool" => "false".into(),
            "Vec" | "BTreeSet" => "[]".into(),
            "BTreeMap" => "{}".into(),
            "Option" => return None,
            n if is_integer(n) => "0".into(),
            n => match self.items.get(n).and_then(|i| i.default.clone()) {
                Some(v) => v,
                None => self.values.get(&format!("{n}::default"))?.clone(),
            },
        })
    }

    /// `ty` as the reader sees it; named types link to their section.
    fn label(&self, ty: &Type, refs: &mut Vec<String>) -> String {
        if let Type::Tuple(t) = ty {
            let parts: Vec<String> = t.elems.iter().map(|e| self.label(e, refs)).collect();
            return format!("({})", parts.join(", "));
        }
        let Some((name, args)) = outer(ty) else { return format!("`{}`", ty.to_token_stream()) };
        let arg =
            |i: usize, refs: &mut Vec<String>| args.get(i).map_or_else(String::new, |t| self.label(t, refs));
        match name.as_str() {
            "String" | "str" => "string".into(),
            "f32" | "f64" => "number".into(),
            "bool" => "boolean".into(),
            n if is_integer(n) => "integer".into(),
            "Vec" | "BTreeSet" => format!("list of {}", arg(0, refs)),
            "BTreeMap" => {
                let key = if args.first().is_some_and(|k| outer(k).is_some_and(|(n, _)| n == "String")) {
                    "name".to_string()
                } else {
                    arg(0, refs)
                };
                format!("map of {key} → {}", arg(1, refs))
            }
            "Option" => format!("{} (optional)", arg(0, refs)),
            "Box" => arg(0, refs),
            "PhantomData" => "—".into(),
            n if self.aliases.get(n).is_some_and(|(t, _)| self.aliases_item(t)) => {
                let target = outer(&self.aliases[n].0).map(|(t, _)| t).unwrap_or_default();
                refs.push(target.clone());
                format!("[{n}](#{})", target.to_lowercase())
            }
            n if self.aliases.contains_key(n) => {
                refs.push(n.to_string());
                format!("[{n}](#{})", n.to_lowercase())
            }
            n => {
                refs.push(n.to_string());
                match self.items.get(n) {
                    Some(Item { body: Body::Custom, .. }) => format!("[{0}](#{0})", n.to_lowercase()),
                    Some(_) => format!("[{n}](#{})", n.to_lowercase()),
                    None => format!("`{n}`"),
                }
            }
        }
    }
}

fn is_integer(n: &str) -> bool {
    matches!(n, "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64" | "usize" | "isize")
}

/// A path type's last segment and its type arguments.
fn outer(ty: &Type) -> Option<(String, Vec<Type>)> {
    let Type::Path(p) = ty else { return None };
    let seg = p.path.segments.last()?;
    let args = match &seg.arguments {
        PathArguments::AngleBracketed(a) => a
            .args
            .iter()
            .filter_map(|a| match a {
                GenericArgument::Type(t) => Some(t.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };
    Some((seg.ident.to_string(), args))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sources(files: &[(&str, &str)]) -> Sources {
        let mut s = Sources::default();
        for (label, text) in files {
            s.add(label, text).unwrap();
        }
        s
    }

    #[test]
    fn renders_a_struct_table() {
        let s = sources(&[(
            "src/thing.rs",
            r#"
/// A thing.
#[derive(Serialize, Deserialize)]
struct Thing {
    /// Its name.
    name: String,
    /// How many, at most 3.
    /// Per second.
    #[serde(default, skip_serializing_if = "is_zero")]
    count: u32,
    /// Where.
    #[serde(default)]
    at: Option<(i32, i32)>,
    /// The parts.
    #[serde(default)]
    parts: Vec<Part>,
    #[serde(skip)]
    cache: u8,
}

/// A part.
#[derive(Serialize, Deserialize)]
struct Part {
    /// Its labels, by name; a | b.
    labels: BTreeMap<String, f32>,
    /// On or off.
    #[serde(default)]
    on: bool,
    /// Who made it.
    author: Option<String>,
}
"#,
        )]);
        let md = s.sections(&[("Thing", "thing.ron")], &BTreeMap::new()).unwrap();
        assert_eq!(
            md,
            r#"<a id="thing"></a>

## Thing (`thing.ron`)

A thing.

| Field | Type | Default | Meaning |
|---|---|---|---|
| `name` | string | required | Its name. |
| `count` | integer | `0` | How many, at most 3. Per second. |
| `at` | (integer, integer) (optional) | none | Where. |
| `parts` | list of [Part](#part) | `[]` | The parts. |

<a id="part"></a>

## Part

A part.

| Field | Type | Default | Meaning |
|---|---|---|---|
| `labels` | map of name → number | required | Its labels, by name; a \| b. |
| `on` | boolean | `false` | On or off. |
| `author` | string (optional) | none | Who made it. |
"#
        );
    }

    #[test]
    fn reads_macro_defaults() {
        let s = sources(&[
            (
                "src/defaults.rs",
                r#"
f32_defaults! {
    half / is_half = 0.5;
    worm_jump / is_worm_jump = -75.0;
}
i32_defaults! {
    one_i32 / is_one_i32 = 1;
}
"#,
            ),
            (
                "src/w.rs",
                r#"
fn default_scale() -> u32 {
    1
}

#[derive(Serialize, Deserialize)]
struct W {
    /// A factor.
    #[serde(default = "d::half", skip_serializing_if = "d::is_half")]
    factor: f32,
    /// Up.
    #[serde(default = "d::worm_jump")]
    jump: f32,
    /// Width.
    #[serde(default = "d::one_i32")]
    width: i32,
    /// Scale.
    #[serde(default = "default_scale")]
    scale: u32,
    /// Inner.
    #[serde(default)]
    inner: Inner,
    /// Mode.
    #[serde(default)]
    mode: Mode,
    /// Other.
    #[serde(default)]
    other: Other,
}

#[derive(Serialize, Deserialize)]
struct Inner {
    /// A.
    a: i32,
    /// B.
    b: f32,
}

impl Default for Inner {
    fn default() -> Self {
        Self { a: 2, b: -0.5 }
    }
}

#[derive(Default, Serialize, Deserialize)]
enum Mode {
    /// On.
    On,
    /// Off.
    #[default]
    Off,
}

#[derive(Serialize, Deserialize)]
struct Other {
    /// X.
    x: i32,
}

impl Default for Other {
    fn default() -> Self {
        Self { x: compute() }
    }
}
"#,
            ),
        ]);
        let md = s.sections(&[("W", "w.ron")], &BTreeMap::new()).unwrap();
        let rows: Vec<&str> = md.lines().filter(|l| l.starts_with("| `")).take(7).collect();
        assert_eq!(
            rows,
            [
                "| `factor` | number | `0.5` | A factor. |",
                "| `jump` | number | `-75.0` | Up. |",
                "| `width` | integer | `1` | Width. |",
                "| `scale` | integer | `1` | Scale. |",
                "| `inner` | [Inner](#inner) | `(a: 2, b: -0.5)` | Inner. |",
                "| `mode` | [Mode](#mode) | `Off` | Mode. |",
                "| `other` | [Other](#other) | see `Other::default` | Other. |",
            ]
        );
    }

    #[test]
    fn an_undocumented_field_is_an_error() {
        let s = sources(&[(
            "src/x.rs",
            r#"
#[derive(Serialize, Deserialize)]
struct X {
    /// Fine.
    a: i32,
    b: i32,
}

#[derive(Serialize, Deserialize)]
enum Y {
    /// Fine.
    One,
    Two,
}
"#,
        )]);
        let e = s.sections(&[("X", "x.ron")], &BTreeMap::new()).unwrap_err().to_string();
        assert!(e.contains("src/x.rs:6: field `b` of `X` has no doc comment"), "{e}");
        let e = s.sections(&[("Y", "y.ron")], &BTreeMap::new()).unwrap_err().to_string();
        assert!(e.contains("src/x.rs:13: variant `Two` of `Y` has no doc comment"), "{e}");
    }

    #[test]
    fn renders_enum_variants() {
        let s = sources(&[(
            "src/e.rs",
            r##"
/// What happens.
#[derive(Serialize, Deserialize)]
enum Effect {
    /// Nothing.
    Halt,
    /// Spawns the named spawn.
    Spawn(String),
    /// A blast.
    Explode {
        /// Health points.
        #[serde(default)]
        damage: i32,
        /// Pack path.
        sound: Option<String>,
    },
}

/// A colour, `"#rrggbb"`.
pub struct Colour(pub Rgba);

#[derive(Serialize, Deserialize)]
struct Uses {
    /// The effect.
    effect: Effect,
    /// Its colour.
    colour: Colour,
}
"##,
        )]);
        let md = s.sections(&[("Uses", "uses.ron")], &BTreeMap::new()).unwrap();
        let effect = &md[md.find("<a id=\"effect\">").unwrap()..];
        assert_eq!(
            effect,
            r##"<a id="effect"></a>

## Effect (enum)

What happens.

| Variant | Fields | Meaning |
|---|---|---|
| `Halt` | — | Nothing. |
| `Spawn` | string | Spawns the named spawn. |
| `Explode` | `damage`, `sound` | A blast. |

### `Effect::Explode`

| Field | Type | Default | Meaning |
|---|---|---|---|
| `damage` | integer | `0` | Health points. |
| `sound` | string (optional) | none | Pack path. |

<a id="colour"></a>

## colour

A colour, `"#rrggbb"`.
"##
        );
        assert!(md.contains("| `colour` | [colour](#colour) | required | Its colour. |"), "{md}");
    }

    #[test]
    fn follows_serde_from() {
        let s = sources(&[(
            "src/s.rs",
            r#"
pub type ShotSpawn = Spawn<ShotContext>;

/// How children are made.
///
/// Context defaults: shot true, event false.
#[derive(Serialize, Deserialize)]
#[serde(from = "SpawnWire", into = "SpawnWire")]
pub struct Spawn<C: SpawnContext> {
    /// Children per spawn.
    pub amount: i32,
    /// Adds the parent's heading.
    pub relative_to_parent: bool,
    /// Never written.
    pub context: PhantomData<C>,
}

impl<C: SpawnContext> Default for Spawn<C> {
    fn default() -> Self {
        Self { amount: 1, relative_to_parent: C::RELATIVE_TO_PARENT, context: PhantomData }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename = "Spawn")]
struct SpawnWire {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    amount: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    relative_to_parent: Option<bool>,
    /// A shared projectile's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    shared: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Weapon {
    /// What a shot spawns.
    shot: ShotSpawn,
}
"#,
        )]);
        let md = s.sections(&[("Weapon", "w.ron")], &BTreeMap::new()).unwrap();
        assert!(md.contains("| `shot` | [ShotSpawn](#spawn) | required | What a shot spawns. |"), "{md}");
        let spawn = &md[md.find("<a id=\"spawn\">").unwrap()..];
        assert_eq!(
            spawn,
            r#"<a id="spawn"></a>

## Spawn

How children are made.

Context defaults: shot true, event false.

| Field | Type | Default | Meaning |
|---|---|---|---|
| `amount` | integer | `1` | Children per spawn. |
| `relative_to_parent` | boolean | by context | Adds the parent's heading. |
| `shared` | string (optional) | none | A shared projectile's id. |
"#
        );
        assert!(!md.contains("spawnwire"), "the wire type has no section of its own");
    }

    #[test]
    fn a_map_file_gets_a_section() {
        let s = sources(&[(
            "src/r.rs",
            r#"
pub type Preset = BTreeMap<String, State>;

/// A weapon's state.
#[derive(Serialize, Deserialize)]
pub enum State {
    /// On.
    On,
}
"#,
        )]);
        let md = s
            .sections_with(
                &[],
                &[("restrictions.ron", "BTreeMap<String, Preset>", "Named presets.")],
                &BTreeMap::new(),
            )
            .unwrap();
        assert!(
            md.starts_with(
                "<a id=\"restrictions-ron\"></a>\n\n## `restrictions.ron`\n\nNamed presets.\n\nType: map of name → [Preset](#preset).\n"
            ),
            "{md}"
        );
        assert!(
            md.contains("<a id=\"preset\"></a>\n\n## Preset\n\nType: map of name → [State](#state).\n"),
            "{md}"
        );
        assert!(md.contains("## State (enum)"), "{md}");
    }
}
