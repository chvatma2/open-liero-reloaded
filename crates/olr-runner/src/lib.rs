//! The issue runner: approved GitHub issues of the open milestone become
//! commits on `main`, implemented by headless Claude Code sessions
//! (spec: `docs/superpowers/specs/2026-10-20-issue-runner-design.md`).

pub mod gate;
pub mod github;
pub mod install;
pub mod limit;
pub mod prompt;
pub mod queue;
pub mod repo;
pub mod report;
pub mod result;
pub mod run;
pub mod session;
pub mod settings;
pub mod state;
pub mod stream;
pub mod trust;
