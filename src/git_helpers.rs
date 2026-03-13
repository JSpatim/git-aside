use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::config::ValetConfig;

/// Runs a git command in the main repo
pub fn git(args: &[&str], work_tree: &Path) -> Result<Output> {
    let out = Command::new("git")
        .args(args)
        .current_dir(work_tree)
        .output()
        .context("Failed to execute git")?;
    Ok(out)
}

/// Runs a git command against the valet bare repo + work-tree
pub fn sgit(args: &[&str], config: &ValetConfig) -> Result<Output> {
    let out = Command::new("git")
        .arg("--git-dir")
        .arg(&config.bare_path)
        .arg("--work-tree")
        .arg(&config.work_tree)
        .args(args)
        .output()
        .context("Failed to execute valet git")?;
    Ok(out)
}

/// Returns the stdout of a git command as a String
pub fn git_output(args: &[&str], work_tree: &Path) -> Result<String> {
    let out = git(args, work_tree)?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Returns the origin remote URL of the current repo
pub fn get_origin(work_tree: &Path) -> Result<String> {
    git_output(&["remote", "get-url", "origin"], work_tree)
        .context("Could not get remote origin. Does this repo have an 'origin' remote?")
}

/// Returns the absolute path of the current repo's .git directory
pub fn get_git_dir(work_tree: &Path) -> Result<PathBuf> {
    let s = git_output(&["rev-parse", "--absolute-git-dir"], work_tree)?;
    Ok(PathBuf::from(s))
}

/// Returns the root of the current git repo
pub fn get_work_tree() -> Result<PathBuf> {
    let out = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("Not inside a git repository")?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        anyhow::bail!("Not inside a git repository");
    }
    Ok(PathBuf::from(s))
}

/// Loads the valet config from the current repo
pub fn load_config() -> Result<ValetConfig> {
    let work_tree = get_work_tree()?;
    let origin = get_origin(&work_tree)?;
    crate::config::load(&origin)
}
