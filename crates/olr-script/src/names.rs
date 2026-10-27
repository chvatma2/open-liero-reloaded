//! The name checks of a level's scripts (issue #89; the asset-based
//! levels design, call 20: "Ids and exposed names a script uses are
//! checked when the level is saved and when its library assets change").
//!
//! [`check_level`] reads each script the level runs (a copy's default and
//! own scripts, the level script) against a plain table of the names it
//! may use ([`ScriptNames`]): its syntax (#80's [`crate::check`]), its
//! handlers' parameter counts, and every string literal it passes to the
//! API where a copy id, a state or an exposed name goes. Only literals
//! are checked: a name built at runtime is a script error when it runs.
//! A copy handle is known when it is `this.copy("…")` itself, `this.me()`,
//! or a variable every `let` of which in the script binds one copy's
//! `this.copy("…")`.

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;

use rhai::{ASTNode, Expr, FnCallExpr, Stmt};

use crate::runtime::HANDLERS;
use crate::{ScriptIssue, compile_text, engine};

/// The names a level's scripts may use: its live copies (what
/// `this.copy(id)` reaches), each with its states, its exposed names and
/// its sounds (#92's `play` checks those). No `olr-format` types: the
/// caller fills it from the level's placement.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScriptNames {
    pub copies: Vec<CopyNames>,
}

/// One live copy's names.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CopyNames {
    pub id: String,
    pub states: Vec<String>,
    pub exposed: Vec<String>,
    pub sounds: Vec<String>,
}

impl ScriptNames {
    fn copy(&self, id: &str) -> Option<&CopyNames> {
        self.copies.iter().find(|c| c.id == id)
    }
}

/// One script the level runs: its text and the live copy it runs on (an
/// id of [`ScriptNames::copies`]; none: the level script).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunScript<'a> {
    pub copy: Option<&'a str>,
    pub text: &'a str,
}

/// An issue of script `script` (an index into [`check_level`]'s scripts).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LevelIssue {
    pub script: usize,
    pub issue: ScriptIssue,
}

impl fmt::Display for LevelIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "script {}: {}", self.script, self.issue)
    }
}

/// What a string literal argument names.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    State,
    Exposed,
}

/// The API methods taking a name, by name: what it names and its count of
/// parameters in its own-copy form (the handle form takes the handle
/// first, the name second).
const NAMED: [(&str, Kind, usize); 8] = [
    ("set_state", Kind::State, 1),
    ("get", Kind::Exposed, 1),
    ("set", Kind::Exposed, 2),
    ("move_to", Kind::Exposed, 3),
    ("stop", Kind::Exposed, 1),
    ("moving", Kind::Exposed, 1),
    ("min", Kind::Exposed, 1),
    ("max", Kind::Exposed, 1),
];

/// A copy handle as the check reads it.
#[derive(Clone, PartialEq, Eq)]
enum Handle {
    /// This copy (`this.me()`).
    Me,
    /// `this.copy("…")`'s.
    Copy(String),
    /// Anything else: not checked.
    Unknown,
}

fn issue(line: Option<usize>, column: Option<usize>, message: String) -> ScriptIssue {
    let at = |v: Option<usize>| v.map_or(0, |v| u32::try_from(v).unwrap_or(u32::MAX));
    ScriptIssue { line: at(line), column: at(column), message }
}

/// `e` as `this.<name>(args)`: the method call.
fn this_call(e: &Expr) -> Option<&FnCallExpr> {
    let Expr::Dot(b, _, _) = e else { return None };
    match (&b.lhs, &b.rhs) {
        (Expr::ThisPtr(_), Expr::MethodCall(f, _)) => Some(f),
        _ => None,
    }
}

fn literal(e: &Expr) -> Option<(&str, rhai::Position)> {
    match e {
        Expr::StringConstant(s, pos) => Some((s.as_str(), *pos)),
        _ => None,
    }
}

/// `e` as `this.copy("…")`: the literal.
fn copy_literal(e: &Expr) -> Option<&str> {
    let f = this_call(e)?;
    if f.name != "copy" || f.args.len() != 1 {
        return None;
    }
    literal(&f.args[0]).map(|(s, _)| s)
}

fn handle(e: &Expr, vars: &BTreeMap<String, Option<String>>) -> Handle {
    if let Some(id) = copy_literal(e) {
        return Handle::Copy(id.to_string());
    }
    if let Some(f) = this_call(e)
        && f.name == "me"
        && f.args.is_empty()
    {
        return Handle::Me;
    }
    if let Expr::Variable(v, _, _) = e
        && let Some(Some(id)) = vars.get(v.1.as_str())
    {
        return Handle::Copy(id.clone());
    }
    Handle::Unknown
}

/// Every variable the script binds with `let`: the copy literal every
/// binding of it gives, or none.
fn copy_variables(ast: &rhai::AST) -> BTreeMap<String, Option<String>> {
    let mut vars: BTreeMap<String, Option<String>> = BTreeMap::new();
    ast.walk(&mut |path: &[ASTNode]| {
        if let Some(ASTNode::Stmt(Stmt::Var(v, _, _))) = path.last() {
            let (ident, expr, _) = &**v;
            let id = copy_literal(expr).map(String::from);
            vars.entry(ident.name.to_string())
                .and_modify(|old| {
                    if *old != id {
                        *old = None;
                    }
                })
                .or_insert(id);
        }
        true
    });
    vars
}

/// The first line (and column) where function `name` is defined in
/// `text`.
fn fn_line(text: &str, name: &str) -> (Option<usize>, Option<usize>) {
    for (k, line) in text.lines().enumerate() {
        let mut rest = line;
        let mut col = 0;
        while let Some(at) = rest.find("fn ") {
            let after = rest[at + 3..].trim_start();
            let starts = at == 0 || !rest.as_bytes()[at - 1].is_ascii_alphanumeric();
            if starts
                && let Some(tail) = after.strip_prefix(name)
                && tail.trim_start().starts_with('(')
            {
                return (Some(k + 1), Some(col + at + 1));
            }
            col += at + 3;
            rest = &rest[at + 3..];
        }
    }
    (None, None)
}

/// The issues of `scripts` (each a script the level runs) against
/// `names`, in script order and by line within a script: a syntax error (only
/// that: a script that does not compile is not read further), a handler
/// defined with the wrong number of parameters (and none with the right
/// one), and every string literal passed to the API that names no copy
/// (`this.copy("crane")`), no state of the copy it acts on
/// (`set_state`), or no exposed name of it (`get`, `set`, `move_to`,
/// `stop`, `moving`, `min`, `max`), the copy being this one or a known
/// handle's. In the level script the own-copy forms are issues too.
pub fn check_level(names: &ScriptNames, scripts: &[RunScript<'_>]) -> Vec<LevelIssue> {
    let engine = engine();
    let mut out = Vec::new();
    for (k, s) in scripts.iter().enumerate() {
        let mut push = |issue: ScriptIssue| out.push(LevelIssue { script: k, issue });
        let ast = match compile_text(&engine, s.text) {
            Ok(ast) => ast,
            Err(e) => {
                push(e);
                continue;
            }
        };
        let mut found = Vec::new();
        // Handlers of the wrong arity.
        let mut fns: Vec<(String, usize)> =
            ast.iter_functions().map(|f| (String::from(f.name), f.params.len())).collect();
        fns.sort();
        for (name, arity) in &fns {
            let Some(&(_, _, want)) = HANDLERS.iter().find(|(_, n, _)| n == name) else { continue };
            if *arity != want && !fns.iter().any(|(n, a)| n == name && *a == want) {
                let (line, column) = fn_line(s.text, name);
                let params =
                    |n: usize| if n == 1 { String::from("1 parameter") } else { format!("{n} parameters") };
                found.push(issue(
                    line,
                    column,
                    format!("handler {name}() takes {}, not {}: it never runs", params(want), params(*arity)),
                ));
            }
        }
        let own = s.copy.and_then(|id| names.copy(id));
        let vars = copy_variables(&ast);
        ast.walk(&mut |path: &[ASTNode]| {
            let Some(ASTNode::Expr(e)) = path.last() else { return true };
            let Some(f) = this_call(e) else { return true };
            let pos = e.position();
            if f.name == "copy" && f.args.len() == 1 {
                if let Some((id, at)) = literal(&f.args[0])
                    && names.copy(id).is_none()
                {
                    found.push(issue(at.line(), at.position(), format!("no live copy {id:?}")));
                }
                return true;
            }
            let Some(&(_, kind, n)) = NAMED.iter().find(|(m, _, _)| *m == f.name.as_str()) else {
                return true;
            };
            let (target, name) = if f.args.len() == n {
                (Handle::Me, &f.args[0])
            } else if f.args.len() == n + 1 {
                (handle(&f.args[0], &vars), &f.args[1])
            } else {
                return true;
            };
            let Some((name, at)) = literal(name) else { return true };
            let copy = match &target {
                Handle::Me if s.copy.is_none() => {
                    found.push(issue(
                        pos.line(),
                        pos.position(),
                        format!("the level script has no copy of its own: this.{}(…) names none", f.name),
                    ));
                    return true;
                }
                Handle::Me => own,
                Handle::Copy(id) => names.copy(id),
                Handle::Unknown => None,
            };
            let Some(copy) = copy else { return true };
            let (list, what) = match kind {
                Kind::State => (&copy.states, "state"),
                Kind::Exposed => (&copy.exposed, "exposed value"),
            };
            if !list.iter().any(|x| x == name) {
                found.push(issue(
                    at.line(),
                    at.position(),
                    format!("copy {} has no {what} {name:?}", copy.id),
                ));
            }
            true
        });
        found.sort_by_key(|i| (i.line, i.column));
        for i in found {
            push(i);
        }
    }
    out
}
