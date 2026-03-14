//! # git-valet
//!
//! Transparently version private files (.env, secrets, notes, AI prompts)
//! in a separate repo, synced via git hooks. Zero workflow change.

pub mod config;
pub mod git_helpers;
pub mod hooks;
pub mod valet;
