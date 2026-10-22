//! `cargo xtask format-docs [--check]`: the schema pages in `docs/formats/`,
//! generated from `olr-format`'s authoring types (plan 13d R4).

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;

use anyhow::{Context as _, bail};
use quote::ToTokens as _;
use syn::{Attribute, Expr, Fields, GenericArgument, Lit, PathArguments, Type};

/// One generated page: a pack kind's files and the types they hold.
struct Page {
    /// The kind's name in the guide's `<!-- file: <kind>/<path> -->` markers.
    kind: &'static str,
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
/// The hand-written guide, whose checked blocks are the pages' examples.
const GUIDE: &str = "docs/formats/README.md";
/// The guide's name in errors.
const GUIDE_NAME: &str = "README.md";

const PAGES: &[Page] = &[
    Page {
        kind: "mod",
        out: "mod.md",
        title: "Mod packs",
        module: "crates/olr-format/src/mods/mod.rs",
        sources: &[
            "crates/olr-format/src/mods/mod.rs",
            "crates/olr-format/src/mods/files.rs",
            "crates/olr-format/src/mods/types.rs",
            "crates/olr-format/src/mods/spawn.rs",
            "crates/olr-format/src/mods/editor.rs",
            "crates/olr-format/src/look.rs",
            "crates/olr-format/src/colour.rs",
            "crates/olr-data/src/restrictions.rs",
        ],
        roots: &[
            ("ModFile", "mod.ron"),
            ("Weapon", "weapons/<id>.ron"),
            ("Projectile", "projectiles/<id>.ron"),
            ("ModEditorFile", "editor/mod.ron"),
        ],
        maps: &[(
            "restrictions.ron",
            "BTreeMap<String, Preset>",
            "The mod's weapon-restriction presets by name (OLX's `.wps` files), optional; the game's \
             default is `Standard 100lt`.",
        )],
        files: &["gfx/<file>.png", "gfx/<file>.glow.png", "sfx/<file>"],
    },
    Page {
        kind: "level",
        out: "level.md",
        title: "Level packs",
        module: "crates/olr-format/src/levels/mod.rs",
        sources: &[
            "crates/olr-format/src/levels/mod.rs",
            "crates/olr-format/src/levels/files.rs",
            "crates/olr-format/src/levels/composition.rs",
            "crates/olr-format/src/look.rs",
            "crates/olr-format/src/materials.rs",
            "crates/olr-format/src/colour.rs",
        ],
        roots: &[("LevelFile", "level.ron"), ("Composition", "editor/composition.ron")],
        maps: &[],
        files: &[
            "material.png",
            "front.png",
            "back.png",
            "thumb.png",
            "look/glow.png",
            "look/glow-back.png",
            "look/groups.png",
            "look/groups-back.png",
            "look/clear.png",
            "look/surface.png",
            "look/surface-back.png",
            "distant/<i>.png",
        ],
    },
    Page {
        kind: "skin",
        out: "skin.md",
        title: "Skin packs",
        module: "crates/olr-format/src/skins/mod.rs",
        sources: &["crates/olr-format/src/skins/mod.rs", "crates/olr-format/src/skins/files.rs"],
        roots: &[("SkinFile", "skin.ron")],
        maps: &[],
        files: &["skin.png", "tint.png", "left.png"],
    },
    Page {
        kind: "theme",
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
        kind: "core",
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
        kind: "asset",
        out: "asset.md",
        title: "Level editor assets",
        module: "crates/olr-format/src/assets.rs",
        sources: &[
            "crates/olr-format/src/assets.rs",
            "crates/olr-format/src/levels/composition.rs",
            "crates/olr-format/src/look.rs",
            "crates/olr-format/src/materials.rs",
            "crates/olr-format/src/colour.rs",
        ],
        roots: &[("AssetFile", "asset.ron")],
        maps: &[],
        files: &[
            "images/<source>",
            "layers/<id>.png",
            "layers/<id>.material.png",
            "layers/<id>.back.png",
            "masks/<id>.png",
            "glow/<id>.png",
            "surface/<id>.png",
            "surface/<id>.back.png",
            "thumb.png",
        ],
    },
    Page {
        kind: "material",
        out: "material.md",
        title: "Surface materials",
        module: "crates/olr-format/src/materials.rs",
        sources: &["crates/olr-format/src/materials.rs", "crates/olr-format/src/colour.rs"],
        roots: &[("MaterialSource", "<id>.ron")],
        maps: &[],
        files: &[],
    },
    Page {
        kind: "pack",
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
    let stale = pages(check)?;
    if !stale.is_empty() {
        println!("{}", stale_message(&stale));
    }
    Ok(stale.is_empty())
}

/// What `--check` says of the pages `stale`.
fn stale_message(stale: &[String]) -> String {
    let verb = if stale.len() == 1 { "differs" } else { "differ" };
    format!("{} {verb} from the code: run cargo xtask format-docs", stale.join(", "))
}

/// Writes the pages, or with `check` compares them; returns the pages that
/// differ (with `check`).
fn pages(check: bool) -> anyhow::Result<Vec<String>> {
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
    if let Some(table) = field_table(&root, check)? {
        stale.push(table);
    }
    Ok(stale)
}

/// The mod editor's field table (issue #17): every serialized field of the
/// mod format's types by `Type.field` (`Type::Variant.field` in an enum,
/// `Type::Variant.0` for a tuple variant's), each variant by
/// `Type::Variant`, with its type and its doc comment as the hover text.
/// The hand columns (`unit`, `range`, `place`, `asset`) are kept from the
/// file; a new field comes under More with none.
pub(crate) const FIELD_TABLE: &str = "crates/olr-mod-editor/fields.ron";
/// The types the table starts from (in the mod page's sources).
const TABLE_ROOTS: &[&str] = &["ModFile", "Weapon", "Projectile", "WeaponState"];
/// Fields the table lists without following their types: the look
/// system's, left to the art editor.
const TABLE_UNFOLLOWED: &[&str] = &["look"];
/// The table's first lines.
const TABLE_HEADER: &str = "\
// The mod editor's field table (issue #17), written by `cargo xtask
// format-docs` from olr-format's types: `key`, `ty` and `help` (the doc
// comment) are generated, `check` fails when they drift from the format.
// Edit `unit` (shown in the label), `range` (the slider's soft range; any
// value may be typed), `place` (Common, More: under More, or Panel: drawn
// by the editor's own panels) and `asset` (`gfx` or `sfx`: a dropdown of
// the mod's files); writing the table again keeps them.
";

/// Where the editor shows a field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum Place {
    Common,
    More,
    Panel,
}

/// One row of the table.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct FieldRow {
    key: String,
    ty: String,
    unit: String,
    range: Option<(f64, f64)>,
    place: Place,
    asset: Option<String>,
    help: String,
}

/// Writes the field table, or with `check` compares it; returns its name
/// when it differs (with `check`).
fn field_table(root: &Path, check: bool) -> anyhow::Result<Option<String>> {
    let page = PAGES.iter().find(|p| p.kind == "mod").context("no mod page")?;
    let mut sources = Sources::default();
    for label in std::iter::once(&DEFAULTS).chain(page.sources) {
        let text = std::fs::read_to_string(root.join(label)).with_context(|| format!("reading {label}"))?;
        sources.add(label, &text)?;
    }
    let path = root.join(FIELD_TABLE);
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    let text = render_table(&sources, TABLE_ROOTS, &old)?;
    if check {
        return Ok((old != text).then(|| FIELD_TABLE.to_string()));
    }
    if old != text {
        std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(None)
}

/// The table for the types of `sources` reached from `roots`, keeping the
/// hand columns of `old`'s rows.
fn render_table(sources: &Sources, roots: &[&str], old: &str) -> anyhow::Result<String> {
    let kept: BTreeMap<String, FieldRow> = if old.trim().is_empty() {
        BTreeMap::new()
    } else {
        ron::from_str::<Vec<FieldRow>>(old)
            .with_context(|| format!("reading {FIELD_TABLE}"))?
            .into_iter()
            .map(|r| (r.key.clone(), r))
            .collect()
    };
    let rows: Vec<FieldRow> = sources
        .field_rows(roots)?
        .into_iter()
        .map(|(key, ty, help)| match kept.get(&key) {
            Some(k) => FieldRow { key, ty, help, ..k.clone() },
            None => {
                FieldRow { key, ty, unit: String::new(), range: None, place: Place::More, asset: None, help }
            }
        })
        .collect();
    let pretty = ron::ser::PrettyConfig::new().indentor("    ".to_string()).new_line("\n".to_string());
    Ok(format!("{TABLE_HEADER}{}\n", ron::ser::to_string_pretty(&rows, pretty)?))
}

/// The `check` step: `format-docs --check`, its failure naming each stale page.
pub fn step() -> Result<(), String> {
    match pages(true) {
        Ok(stale) if stale.is_empty() => Ok(()),
        Ok(stale) => Err(stale_message(&stale)),
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
    let guide = std::fs::read_to_string(root.join(GUIDE)).with_context(|| format!("reading {GUIDE}"))?;
    let examples = guide_examples(&guide, page.kind, page.roots, page.maps)?;
    out += &sources.sections_with(page.roots, page.maps, &examples)?;
    Ok(out)
}

/// The guide's blocks of `kind` (`<!-- file: <kind>/<path> -->` then a
/// fenced `ron` block), by the type of the file they hold (a root's type,
/// or a map file's name). A type nested in a file shows no example of
/// its own: the file's block covers it.
fn guide_examples(
    guide: &str,
    kind: &str,
    roots: &[(&str, &str)],
    maps: &[(&str, &str, &str)],
) -> anyhow::Result<BTreeMap<String, String>> {
    // `weapons/<id>.ron` matches `weapons/gun.ron`.
    let matches = |pattern: &str, path: &str| match pattern.split_once("<id>") {
        Some((head, tail)) => {
            path.len() > head.len() + tail.len() && path.starts_with(head) && path.ends_with(tail)
        }
        None => pattern == path,
    };
    let lines: Vec<&str> = guide.lines().collect();
    let mut out = BTreeMap::new();
    for (i, l) in lines.iter().enumerate() {
        let Some(file) = l.trim().strip_prefix("<!-- file: ").and_then(|r| r.strip_suffix(" -->")) else {
            if l.contains("<!--") && l.contains("file:") {
                bail!("{GUIDE_NAME}:{}: a malformed marker (`<!-- file: <kind>/<path> -->`): {l}", i + 1);
            }
            continue;
        };
        let Some(path) = file.strip_prefix(kind).and_then(|r| r.strip_prefix('/')) else { continue };
        if lines.get(i + 1) != Some(&"```ron") {
            continue;
        }
        let Some(len) = lines[i + 2..].iter().position(|l| *l == "```") else { continue };
        let text: String = lines[i + 2..i + 2 + len].iter().map(|l| format!("{l}\n")).collect();
        let key = roots
            .iter()
            .find(|(_, f)| matches(f, path))
            .map(|(t, _)| t.to_string())
            .or_else(|| maps.iter().find(|(f, _, _)| *f == path).map(|(f, _, _)| f.to_string()));
        if let Some(key) = key {
            out.entry(key).or_insert(text);
        }
    }
    for file in roots.iter().map(|(t, f)| (*t, *f)).chain(maps.iter().map(|(f, _, _)| (*f, *f))) {
        if !out.contains_key(file.0) {
            bail!("{GUIDE_NAME} has no example of `{}` for the {kind} page", file.1);
        }
    }
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
    /// What the sources hold that the generator cannot document: reported
    /// with the page's errors.
    problems: Vec<String>,
}

/// The serde attributes the generator understands, on a container and on
/// a field; any other (on a variant: any) is an error, as it would change
/// the text form unseen.
const CONTAINER_SERDE: &[&str] = &["deny_unknown_fields", "rename", "from", "into", "bound"];
const FIELD_SERDE: &[&str] = &["default", "skip", "skip_serializing_if"];

/// The serde attribute names in `attrs` that `allowed` does not list.
fn unsupported_serde(attrs: &[Attribute], allowed: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    for a in attrs.iter().filter(|a| a.path().is_ident("serde")) {
        let _ = a.parse_nested_meta(|meta| {
            let name = meta.path.to_token_stream().to_string();
            if !allowed.contains(&name.as_str()) {
                out.push(name);
            }
            if meta.input.peek(syn::Token![=]) {
                meta.value()?.parse::<Expr>()?;
            }
            Ok(())
        });
    }
    out
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
                    if derives_serde(&s.attrs) {
                        self.check_serde(label, &name, &s.attrs, CONTAINER_SERDE);
                        if let Fields::Named(f) = &s.fields {
                            for field in &f.named {
                                self.check_serde(label, &name, &field.attrs, FIELD_SERDE);
                            }
                        }
                    }
                    self.check_unique(label, &name);
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
                    self.check_serde(label, &name, &e.attrs, CONTAINER_SERDE);
                    for v in &e.variants {
                        self.check_serde(label, &name, &v.attrs, &[]);
                        for field in &v.fields {
                            self.check_serde(label, &name, &field.attrs, FIELD_SERDE);
                        }
                    }
                    self.check_unique(label, &name);
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

    fn check_serde(&mut self, label: &str, item: &str, attrs: &[Attribute], allowed: &[&str]) {
        for a in unsupported_serde(attrs, allowed) {
            self.problems
                .push(format!("{label}: `{item}`: serde attribute `{a}` is not supported by format-docs"));
        }
    }

    fn check_unique(&mut self, label: &str, name: &str) {
        if self.items.contains_key(name) || self.aliases.contains_key(name) {
            self.problems.push(format!("{label}: type `{name}` is declared twice"));
        }
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
        let mut errors = self.problems.clone();
        let mut refs = Vec::new();
        for (file, ty, doc) in maps {
            let ty: Type = syn::parse_str(ty).with_context(|| format!("{file}: parsing {ty}"))?;
            let before = refs.len();
            let label = self.label(&ty, &mut refs);
            self.check_refs(&mut refs, before, &format!("`{file}`"), &mut errors);
            let mut s = format!(
                "<a id=\"{}\"></a>\n\n## `{file}`\n\n{doc}\n\nType: {}.\n",
                file.replace('.', "-"),
                label
            );
            if let Some(example) = examples.get(*file) {
                s += &format!("\nExample:\n\n```ron\n{example}```\n");
            }
            out.push(s);
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

    /// The field table's rows (`render_table`): `(key, type, help)` for
    /// every type reached from `roots`, in order of first reference.
    fn field_rows(&self, roots: &[&str]) -> anyhow::Result<Vec<(String, String, String)>> {
        let ty_text = |t: &Type| t.to_token_stream().to_string().replace(' ', "");
        let help = |doc: &[String]| paragraphs(doc).join(" ");
        let mut queue: VecDeque<String> = roots.iter().map(|t| t.to_string()).collect();
        let mut seen: BTreeSet<String> = queue.iter().cloned().collect();
        let mut out = Vec::new();
        while let Some(name) = queue.pop_front() {
            let item = self.items.get(&name).with_context(|| format!("no type `{name}` in the sources"))?;
            let mut refs = Vec::new();
            let field = |key: String, f: &Field, out: &mut Vec<_>, refs: &mut Vec<String>| {
                out.push((key, ty_text(&f.ty), help(&f.doc)));
                if !TABLE_UNFOLLOWED.contains(&f.name.as_str()) {
                    self.label(&f.ty, refs);
                }
            };
            match &item.body {
                Body::Struct(fields) => {
                    for f in self.text_fields(item, fields) {
                        field(format!("{name}.{}", f.name), &f, &mut out, &mut refs);
                    }
                }
                Body::Enum(variants) => {
                    for v in variants {
                        out.push((format!("{name}::{}", v.name), "variant".into(), help(&v.doc)));
                        match &v.fields {
                            VariantFields::Unit => {}
                            VariantFields::Tuple(types) => {
                                for (i, t) in types.iter().enumerate() {
                                    out.push((format!("{name}::{}.{i}", v.name), ty_text(t), help(&v.doc)));
                                    self.label(t, &mut refs);
                                }
                            }
                            VariantFields::Named(fields) => {
                                for f in fields {
                                    field(format!("{name}::{}.{}", v.name, f.name), f, &mut out, &mut refs);
                                }
                            }
                        }
                    }
                }
                Body::Custom => {}
            }
            for r in refs {
                if self.items.contains_key(&r) && seen.insert(r.clone()) {
                    queue.push_back(r);
                }
            }
        }
        Ok(out)
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
                            let before = refs.len();
                            let label =
                                types.iter().map(|t| self.label(t, refs)).collect::<Vec<_>>().join(", ");
                            let at =
                                format!("{}:{}: variant `{}` of `{}`", item.file, v.line, v.name, item.name);
                            self.check_refs(refs, before, &at, errors);
                            label
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
            let at = format!("{}:{}: field `{}` of `{}`", item.file, f.line, f.name, item.name);
            let before = refs.len();
            let label = self.label(&f.ty, refs);
            self.check_refs(refs, before, &at, errors);
            let default = self.default_of(f).unwrap_or_else(|e| {
                errors.push(format!("{at}: {e}"));
                String::new()
            });
            s += &format!("| `{}` | {label} | {default} | {} |\n", f.name, cell(&f.doc));
        }
        s
    }

    /// Reports (and drops) the unresolved type markers `label` left in
    /// `refs` from `from` on.
    fn check_refs(&self, refs: &mut Vec<String>, from: usize, at: &str, errors: &mut Vec<String>) {
        let mut kept = refs.split_off(from);
        kept.retain(|r| match r.strip_prefix('?') {
            Some(n) => {
                errors.push(format!("{at}: type `{n}` is not in the page's sources"));
                false
            }
            None => true,
        });
        refs.extend(kept);
    }

    fn default_of(&self, f: &Field) -> Result<String, String> {
        Ok(match &f.default {
            Default::None if outer(&f.ty).is_some_and(|(n, _)| n == "Option") => "none".into(),
            Default::None => "required".into(),
            // A value already rendered (`text_fields`).
            Default::Fn(v) if v.starts_with('=') => v[1..].to_string(),
            Default::Fn(path) => {
                let name = path.rsplit("::").next().unwrap_or(path);
                match self.values.get(name) {
                    Some(v) => format!("`{v}`"),
                    None => return Err(format!("no value for default `{path}`")),
                }
            }
            Default::Bare => match self.zero(&f.ty) {
                Some(z) => format!("`{z}`"),
                None if outer(&f.ty).is_some_and(|(n, _)| n == "Option") => "none".into(),
                None if self.every_field_defaults(&f.ty) => "each field's default".into(),
                None => {
                    let ty = outer(&f.ty).map_or_else(|| f.ty.to_token_stream().to_string(), |(n, _)| n);
                    return Err(format!("no default value known for `{ty}`"));
                }
            },
        })
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
                    None => {
                        // Not documented here: `check_refs` reports it.
                        refs.pop();
                        refs.push(format!("?{n}"));
                        format!("`{n}`")
                    }
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

    /// Issue #17: the field table lists every field (variants, tuple
    /// fields, `look` listed but not followed), keeps the hand columns
    /// and puts a new field under More.
    #[test]
    fn the_field_table_keeps_the_hand_columns() {
        let s = sources(&[(
            "src/m.rs",
            r#"
/// Root.
#[derive(Serialize, Deserialize)]
struct Root {
    /// Gravity, px/s².
    gravity: f32,
    /// How.
    how: How,
    /// The look.
    look: Option<Look>,
}

/// How.
#[derive(Serialize, Deserialize)]
enum How {
    /// Not at all.
    Off,
    /// By a number.
    Number(i32),
    /// With fields.
    Fast {
        /// Its speed.
        speed: u32,
    },
}

/// Never followed.
#[derive(Serialize, Deserialize)]
struct Look {
    /// Glow.
    glow: u8,
}
"#,
        )]);
        let old = r#"[(key: "Root.gravity", ty: "i32", unit: "px/s²", range: Some((0.0, 9.0)), place: Common,
            asset: None, help: "old")]"#;
        let text = render_table(&s, &["Root"], old).unwrap();
        assert!(text.starts_with(TABLE_HEADER));
        let rows: Vec<FieldRow> = ron::from_str(&text).unwrap();
        let keys: Vec<&str> = rows.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "Root.gravity",
                "Root.how",
                "Root.look",
                "How::Off",
                "How::Number",
                "How::Number.0",
                "How::Fast",
                "How::Fast.speed"
            ]
        );
        assert_eq!(
            rows[0],
            FieldRow {
                key: "Root.gravity".into(),
                ty: "f32".into(),
                unit: "px/s²".into(),
                range: Some((0.0, 9.0)),
                place: Place::Common,
                asset: None,
                help: "Gravity, px/s².".into(),
            }
        );
        assert_eq!(
            (rows[7].place, rows[7].ty.as_str(), rows[2].ty.as_str()),
            (Place::More, "u32", "Option<Look>")
        );
        assert_eq!(render_table(&s, &["Root"], &text).unwrap(), text, "writing again changes nothing");
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

"#,
            ),
        ]);
        let md = s.sections(&[("W", "w.ron")], &BTreeMap::new()).unwrap();
        let rows: Vec<&str> = md.lines().filter(|l| l.starts_with("| `")).take(6).collect();
        assert_eq!(
            rows,
            [
                "| `factor` | number | `0.5` | A factor. |",
                "| `jump` | number | `-75.0` | Up. |",
                "| `width` | integer | `1` | Width. |",
                "| `scale` | integer | `1` | Scale. |",
                "| `inner` | [Inner](#inner) | `(a: 2, b: -0.5)` | Inner. |",
                "| `mode` | [Mode](#mode) | `Off` | Mode. |",
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

    #[test]
    fn examples_come_from_the_guide() {
        let guide = "Text.\n\n<!-- file: mod/weapons/gun.ron -->\n```ron\n(\n    name: \"Gun\",\n)\n```\n\n\
            <!-- file: mod/restrictions.ron -->\n```ron\n{}\n```\n\n\
            <!-- file: level/level.ron -->\n```ron\n(\n    name: \"L\",\n)\n```\n";
        let examples = guide_examples(
            guide,
            "mod",
            &[("Weapon", "weapons/<id>.ron")],
            &[("restrictions.ron", "BTreeMap<String, Preset>", "")],
        )
        .unwrap();
        assert_eq!(
            examples,
            BTreeMap::from([
                ("Weapon".to_string(), "(\n    name: \"Gun\",\n)\n".to_string()),
                ("restrictions.ron".to_string(), "{}\n".to_string()),
            ])
        );
        let s = sources(&[(
            "src/w.rs",
            "#[derive(Serialize, Deserialize)]\nstruct Weapon {\n    /// Its name.\n    name: String,\n}\n",
        )]);
        let md = s.sections(&[("Weapon", "weapons/<id>.ron")], &examples).unwrap();
        assert!(md.ends_with("| `name` | string | required | Its name. |\n\nExample:\n\n```ron\n(\n    name: \"Gun\",\n)\n```\n"), "{md}");
    }

    /// Review C-2: nothing the generator cannot resolve passes silently.
    #[test]
    fn what_cannot_be_documented_is_an_error() {
        let cases = [
            (
                "#[derive(Serialize, Deserialize)]\nstruct A {\n    /// X.\n    x: Elsewhere,\n}\n",
                "src/a.rs:4: field `x` of `A`: type `Elsewhere` is not in the page's sources",
            ),
            (
                "#[derive(Serialize, Deserialize)]\nstruct A {\n    /// X.\n    #[serde(default = \"d::nowhere\")]\n    x: i32,\n}\n",
                "src/a.rs:5: field `x` of `A`: no value for default `d::nowhere`",
            ),
            (
                "#[derive(Serialize, Deserialize)]\nstruct A {\n    /// X.\n    #[serde(default)]\n    x: B,\n}\n\
                 #[derive(Serialize, Deserialize)]\nstruct B {\n    /// Y.\n    y: i32,\n}\n\
                 impl Default for B {\n    fn default() -> Self {\n        Self { y: compute() }\n    }\n}\n",
                "src/a.rs:5: field `x` of `A`: no default value known for `B`",
            ),
            (
                "#[derive(Serialize, Deserialize)]\nstruct A {\n    /// X.\n    #[serde(rename = \"y\")]\n    x: i32,\n}\n",
                "src/a.rs: `A`: serde attribute `rename` is not supported by format-docs",
            ),
            (
                "#[derive(Serialize, Deserialize)]\n#[serde(rename_all = \"snake_case\")]\nenum A {\n    /// X.\n    X,\n}\n",
                "src/a.rs: `A`: serde attribute `rename_all` is not supported by format-docs",
            ),
            (
                "#[derive(Serialize, Deserialize)]\nstruct A {\n    /// X.\n    x: i32,\n}\nmod m {\n    #[derive(Serialize, Deserialize)]\n    struct A {\n        /// Y.\n        y: i32,\n    }\n}\n",
                "src/a.rs: type `A` is declared twice",
            ),
        ];
        for (src, want) in cases {
            let s = sources(&[("src/a.rs", src)]);
            let e = s.sections(&[("A", "a.ron")], &BTreeMap::new()).unwrap_err().to_string();
            assert!(e.contains(want), "{want}\n got: {e}");
        }
    }

    /// Review C-1: the check's failure names each stale page.
    #[test]
    fn the_stale_message_names_the_pages() {
        assert_eq!(
            stale_message(&["docs/formats/mod.md".into()]),
            "docs/formats/mod.md differs from the code: run cargo xtask format-docs"
        );
        assert_eq!(
            stale_message(&["docs/formats/a.md".into(), "docs/formats/b.md".into()]),
            "docs/formats/a.md, docs/formats/b.md differ from the code: run cargo xtask format-docs"
        );
    }

    /// Review C-3: a malformed marker, and a file type without an example,
    /// are errors.
    #[test]
    fn the_guide_must_cover_every_file() {
        let roots = [("Weapon", "weapons/<id>.ron")];
        let ok = "<!-- file: mod/weapons/gun.ron -->\n```ron\n()\n```\n";
        assert!(guide_examples(ok, "mod", &roots, &[]).is_ok());
        let e = guide_examples("Nothing here.\n", "mod", &roots, &[]).unwrap_err().to_string();
        assert!(e.contains("no example of `weapons/<id>.ron`"), "{e}");
        let bad = format!("{ok}<!--file: mod/mod.ron-->\n```ron\n()\n```\n");
        let e = guide_examples(&bad, "mod", &roots, &[]).unwrap_err().to_string();
        assert!(e.contains("README.md:5: a malformed marker"), "{e}");
    }
}
