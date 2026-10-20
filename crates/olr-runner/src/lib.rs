//! The issue runner: approved GitHub issues of the open milestone become
//! commits on `main`, implemented by headless Claude Code sessions
//! (spec: `docs/superpowers/specs/2026-10-20-issue-runner-design.md`).

pub mod github;
pub mod settings;
