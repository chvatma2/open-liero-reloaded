//! Asset scripts (the asset-based levels design, calls 10, 12, 19 and 20):
//! an asset's default script, a placed copy's own and the level script
//! are Rhai text. Issue #80 checks their syntax ([`check`]); issue #88
//! runs them: [`Scripts`] is the simulation's [`olr_sim::Director`], one
//! engine and the compiled scripts behind an `Arc`, stateless, every bit
//! of script state in the world. #89 adds the name checks beside this
//! syntax-only `check`.
//!
//! Rhai is pinned (`=1.27.0`, [`RHAI_VERSION`]: its operation count is
//! part of the simulation, so a version change is a simulation change)
//! with `no_std`, `sync`, `no_float`, `only_i64`, `no_module`,
//! `no_custom_syntax`, `no_time`, `no_closure` and `internals`, so the
//! editor never parses a script the simulation would read differently.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

mod api;
mod runtime;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use rhai::{Engine, OptimizationLevel};

pub use olr_sim::{MAX_LIVE_COMMANDS, MAX_MEMORY_BYTES, MAX_MEMORY_KEYS, MAX_SCRIPT_ERRORS, MAX_TIMERS};
pub use runtime::{CompileError, Scripts, director};

/// The Rhai release the simulation runs (the workspace pins it; a test
/// checks `Cargo.lock` agrees). `olr-proto`'s fingerprint covers this
/// crate's sources, so a Rhai bump changes it.
pub const RHAI_VERSION: &str = "1.27.0";

/// The most bytes a script holds (`olr_format::assets::MAX_SCRIPT_BYTES`,
/// which this crate does not depend on: `olr-editor`'s tests check they
/// agree).
pub const MAX_SCRIPT_BYTES: usize = 64 << 10;
/// The deepest an expression nests (brackets, operators, calls), and in a
/// function's body: Rhai's own limits for a debug build, so that parsing
/// never runs out of stack in either build.
pub const MAX_EXPR_DEPTH: usize = 32;
pub const MAX_FUNCTION_EXPR_DEPTH: usize = 16;
/// The longest string, bytes: a literal (checked when compiled) and any
/// string a run makes.
pub const MAX_STRING_BYTES: usize = 256;
/// The most operations one run (a handler or a timer's call) makes:
/// past it the run fails.
pub const MAX_RUN_OPERATIONS: u64 = 5_000;
/// The most operations a tick's runs make together, each charged its
/// own count (a failed run [`MAX_RUN_OPERATIONS`]). A run starts only
/// while the tick has `MAX_RUN_OPERATIONS` left.
pub const MAX_TICK_OPERATIONS: u64 = 20_000;
/// The deepest function calls nest in a run.
pub const MAX_CALL_LEVELS: usize = 16;
/// The most elements an array holds, and entries a map holds.
pub const MAX_ARRAY: usize = 64;
pub const MAX_MAP: usize = 64;
/// The most variables a run has at once, and functions a script defines.
pub const MAX_VARIABLES: usize = 64;
pub const MAX_FUNCTIONS: usize = 64;

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
/// is evaluated while compiling, and a run's operation count is the
/// script's as written), `eval` disabled, and the limits ([`MAX_EXPR_DEPTH`],
/// [`MAX_FUNCTION_EXPR_DEPTH`], [`MAX_STRING_BYTES`], the run limits).
/// The runtime's engine is this one with its packages and API, so both
/// parse alike.
pub fn engine() -> Engine {
    let mut e = Engine::new_raw();
    e.set_optimization_level(OptimizationLevel::None);
    e.set_max_expr_depths(MAX_EXPR_DEPTH, MAX_FUNCTION_EXPR_DEPTH);
    e.set_max_string_size(MAX_STRING_BYTES);
    e.set_max_operations(MAX_RUN_OPERATIONS);
    e.set_max_call_levels(MAX_CALL_LEVELS);
    e.set_max_array_size(MAX_ARRAY);
    e.set_max_map_size(MAX_MAP);
    e.set_max_variables(MAX_VARIABLES);
    e.set_max_functions(MAX_FUNCTIONS);
    // No shared mutable state: the string interner is off.
    e.set_max_strings_interned(0);
    e.disable_symbol("eval");
    // Nothing reaches the console.
    e.on_print(|_| {});
    e.on_debug(|_, _, _| {});
    e
}

/// The syntax errors of script `text`: at most one (Rhai stops at the
/// first), or a text longer than [`MAX_SCRIPT_BYTES`]. Compiles to an AST
/// and runs nothing; which functions and names a script uses is not
/// checked here (#89's checks).
pub fn check(text: &str) -> Vec<ScriptIssue> {
    match compile_text(&engine(), text) {
        Ok(_) => Vec::new(),
        Err(issue) => alloc::vec![issue],
    }
}

/// `text` compiled by `engine`, or its first issue.
pub(crate) fn compile_text(engine: &Engine, text: &str) -> Result<rhai::AST, ScriptIssue> {
    if text.len() > MAX_SCRIPT_BYTES {
        return Err(ScriptIssue {
            line: 1,
            column: 1,
            message: alloc::format!("the script is {} bytes, more than {MAX_SCRIPT_BYTES}", text.len()),
        });
    }
    engine.compile(text).map_err(|e| {
        let at = |v: Option<usize>| v.map_or(0, |v| u32::try_from(v).unwrap_or(u32::MAX));
        ScriptIssue { line: at(e.1.line()), column: at(e.1.position()), message: e.0.to_string() }
    })
}
