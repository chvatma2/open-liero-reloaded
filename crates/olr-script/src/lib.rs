//! Asset scripts (the asset-based levels design, call 19): an asset's
//! default script is Rhai text. Issue #80 checks its syntax ([`check`]);
//! #88 adds the runtime here, and #89 the name checks beside this
//! syntax-only `check`.
//!
//! Rhai is pinned (`=1.27.0`) with the features #88's runtime needs
//! (`no_std`, `sync`, `no_float`, `only_i64`, `no_module`,
//! `no_custom_syntax`, `no_time`, `no_closure`, `internals`), so the
//! editor never parses a script the simulation would read differently.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use rhai::{Engine, OptimizationLevel};

/// The most bytes a script holds (`olr_format::assets::MAX_SCRIPT_BYTES`,
/// which this crate does not depend on: `olr-editor`'s tests check they
/// agree).
pub const MAX_SCRIPT_BYTES: usize = 64 << 10;
/// The deepest an expression nests (brackets, operators, calls), and in a
/// function's body: Rhai's own limits for a debug build, so that parsing
/// never runs out of stack in either build.
pub const MAX_EXPR_DEPTH: usize = 32;
pub const MAX_FUNCTION_EXPR_DEPTH: usize = 16;
/// The longest string literal, bytes.
pub const MAX_STRING_BYTES: usize = 4096;

/// A problem in a script: its line and column (from 1; 0 when Rhai gives
/// none) and what is wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptIssue {
    pub line: u32,
    pub column: u32,
    pub message: String,
}

impl core::fmt::Display for ScriptIssue {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.column, self.message)
    }
}

/// The engine scripts are read with: no packages, no optimisation (nothing
/// is evaluated while compiling), and limits that keep hostile text cheap
/// ([`MAX_EXPR_DEPTH`], [`MAX_FUNCTION_EXPR_DEPTH`], [`MAX_STRING_BYTES`]).
pub fn engine() -> Engine {
    let mut e = Engine::new_raw();
    e.set_optimization_level(OptimizationLevel::None);
    e.set_max_expr_depths(MAX_EXPR_DEPTH, MAX_FUNCTION_EXPR_DEPTH);
    e.set_max_string_size(MAX_STRING_BYTES);
    e
}

/// The syntax errors of script `text`: at most one (Rhai stops at the
/// first), or a text longer than [`MAX_SCRIPT_BYTES`]. Compiles to an AST
/// and runs nothing; which functions and names a script uses is not
/// checked here (#88's API, #89's checks).
pub fn check(text: &str) -> Vec<ScriptIssue> {
    if text.len() > MAX_SCRIPT_BYTES {
        return alloc::vec![ScriptIssue {
            line: 1,
            column: 1,
            message: alloc::format!("the script is {} bytes, more than {MAX_SCRIPT_BYTES}", text.len()),
        }];
    }
    match engine().compile(text) {
        Ok(_) => Vec::new(),
        Err(e) => {
            let at = |v: Option<usize>| v.map_or(0, |v| u32::try_from(v).unwrap_or(u32::MAX));
            alloc::vec![ScriptIssue {
                line: at(e.1.line()),
                column: at(e.1.position()),
                message: e.0.to_string(),
            }]
        }
    }
}
