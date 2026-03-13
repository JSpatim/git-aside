use anyhow::Result;
use colored::Colorize;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::{self, ValetConfig};
use crate::git_helpers::{get_git_dir, get_origin, get_work_tree, load_config, sgit};
use crate::hooks;

// ── Gitignore ────────────────────────────────────────────────────────────────

/// Adds entries to .git/info/exclude (not tracked — no trace on public repo)
fn update_exclude(git_dir: &Path, files: &[String]) -> Result<()> {
    let info_dir = git_dir.join("info");
    std::fs::create_dir_all(&info_dir)?;
    let exclude_path = info_dir.join("exclude");

    let existing = if exclude_path.exists() {
        std::fs::read_to_string(&exclude_path)?
    } else {
        String::new()
    };

    let mut to_add: Vec<String> = Vec::new();
    for file in files {
        if !existing.lines().any(|line| line.trim() == file.as_str()) {
            to_add.push(file.clone());
        }
    }

    if to_add.is_empty() {
        return Ok(());
    }

    let mut content = existing;
    if !content.ends_with('\n') && !content.is_empty() {
        content.push('\n');
    }
    content.push_str("\n# git-valet: files versioned in the valet repo\n");
    for f in &to_add {
        content.push_str(f);
        content.push('\n');
    }

    std::fs::write(&exclude_path, content)?;
    println!("{} .git/info/exclude updated ({} entries added)", "->".cyan(), to_add.len());
    Ok(())
}

/// Removes git-valet entries from .git/info/exclude
fn remove_from_exclude(git_dir: &Path, files: &[String]) -> Result<()> {
    let exclude_path = git_dir.join("info").join("exclude");
    if !exclude_path.exists() {
        return Ok(());
    }

    let content = std::fs::read_to_string(&exclude_path)?;
    let filtered: Vec<&str> = content
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !files.iter().any(|f| f == trimmed)
                && trimmed != "# git-valet: files versioned in the valet repo"
        })
        .collect();

    std::fs::write(&exclude_path, filtered.join("\n") + "\n")?;
    Ok(())
}

// ── Public commands ──────────────────────────────────────────────────────────

/// `git valet init <remote> <files...>`
pub fn init(remote: &str, files: &[String]) -> Result<()> {
    let work_tree = get_work_tree()?;
    let origin = get_origin(&work_tree)?;
    let git_dir = get_git_dir(&work_tree)?;

    let project_id = config::project_id(&origin);
    let bare_path = config::valets_dir()?.join(&project_id).join("repo.git");

    println!("{}", "Initializing valet repo...".bold());
    println!("  Project : {}", origin.dimmed());
    println!("  Valet   : {}", remote.cyan());
    println!("  Bare repo : {}", bare_path.display().to_string().dimmed());

    // 1. Init bare repo
    std::fs::create_dir_all(&bare_path)?;
    let init_out = Command::new("git")
        .args(["init", "--bare", bare_path.to_str().unwrap()])
        .output()?;
    if !init_out.status.success() {
        anyhow::bail!("Failed to initialize bare repo");
    }

    // 2. Config
    let cfg = ValetConfig {
        work_tree: work_tree.to_str().unwrap().to_string(),
        remote: remote.to_string(),
        bare_path: bare_path.to_str().unwrap().to_string(),
        tracked: files.to_vec(),
        branch: "main".to_string(),
    };
    config::save(&cfg, &project_id)?;

    // 3. Hide untracked files from sgit status
    Command::new("git")
        .args(["--git-dir", bare_path.to_str().unwrap(), "config", "status.showUntrackedFiles", "no"])
        .output()?;

    // 4. Remote
    let remote_out = Command::new("git")
        .args(["--git-dir", bare_path.to_str().unwrap(), "remote", "add", "origin", remote])
        .output()?;
    if !remote_out.status.success() {
        // Already exists — update instead
        Command::new("git")
            .args(["--git-dir", bare_path.to_str().unwrap(), "remote", "set-url", "origin", remote])
            .output()?;
    }

    // 5. Update .git/info/exclude (no trace on public repo)
    update_exclude(&git_dir, files)?;

    // 6. Hooks
    hooks::install(&git_dir)?;
    println!("{} Git hooks installed (pre-commit, pre-push, post-merge, post-checkout)", "->".cyan());

    // 7. Initial commit if tracked files already exist
    let existing_files: Vec<&String> = files
        .iter()
        .filter(|f| work_tree.join(f).exists())
        .collect();

    if !existing_files.is_empty() {
        let add_args: Vec<&str> = std::iter::once("add")
            .chain(std::iter::once("-f"))
            .chain(existing_files.iter().map(|f| f.as_str()))
            .collect();
        sgit(&add_args, &cfg)?;

        let commit_out = sgit(&["commit", "-m", "feat: init valet repo"], &cfg)?;
        if commit_out.status.success() {
            println!("{} Initial commit done", "->".cyan());

            // 8. Initial push
            let push_out = sgit(&["push", "-u", "origin", &format!("HEAD:{}", cfg.branch)], &cfg)?;
            if push_out.status.success() {
                println!("{} Initial push done", "->".cyan());
            } else {
                let err = String::from_utf8_lossy(&push_out.stderr);
                println!("{} Initial push failed (remote unreachable?): {}", "!".yellow(), err.trim());
                println!("  You can push manually with: {}", "git valet push".cyan());
            }
        }
    } else {
        // No local files — try to pull from remote (fresh clone scenario)
        let fetch_out = sgit(&["fetch", "origin", &cfg.branch], &cfg)?;
        if fetch_out.status.success() {
            let checkout_out = sgit(
                &["checkout", &format!("origin/{}", cfg.branch), "--", "."],
                &cfg,
            )?;
            if checkout_out.status.success() {
                // Set up local branch tracking
                sgit(&["branch", &cfg.branch, &format!("origin/{}", cfg.branch)], &cfg)?;
                sgit(&["symbolic-ref", "HEAD", &format!("refs/heads/{}", cfg.branch)], &cfg)?;
                println!("{} Pulled existing files from remote", "->".cyan());
            } else {
                println!("{} Remote exists but checkout failed", "!".yellow());
            }
        } else {
            println!("{} No tracked files found locally and remote is empty", "i".blue());
        }
    }

    println!("\n{}", "Done! Valet repo initialized.".green().bold());
    println!("The following files are now managed by git-valet:");
    for f in files {
        println!("  {} {}", "-".dimmed(), f.cyan());
    }
    println!("\nYour usual git commands work as before.");

    Ok(())
}

/// `git valet status`
pub fn status() -> Result<()> {
    let cfg = load_config()?;

    println!("{}", "Valet repo status".bold());
    println!("  Remote  : {}", cfg.remote.cyan());
    println!("  Tracked :");
    for f in &cfg.tracked {
        let exists = PathBuf::from(&cfg.work_tree).join(f).exists();
        let marker = if exists { "+".green() } else { "x".red() };
        println!("    {} {}", marker, f);
    }
    println!();

    let head_check = sgit(&["rev-parse", "HEAD"], &cfg)?;
    if !head_check.status.success() {
        println!("{}", "Valet repo has no commits yet — run `git valet sync` to create the initial commit.".yellow());
        return Ok(());
    }

    let out = sgit(&["status", "--short"], &cfg)?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    if stdout.trim().is_empty() {
        println!("{}", "Nothing to commit — valet repo is clean.".green());
    } else {
        println!("{}", stdout);
    }

    Ok(())
}

/// `git valet sync` — add + commit + push
pub fn sync(message: &str) -> Result<()> {
    let cfg = load_config()?;

    let work_tree = PathBuf::from(&cfg.work_tree);
    let existing: Vec<&str> = cfg.tracked
        .iter()
        .filter(|f| work_tree.join(f).exists())
        .map(|f| f.as_str())
        .collect();

    if existing.is_empty() {
        println!("{}", "No tracked files found.".yellow());
        return Ok(());
    }

    let mut add_args = vec!["add", "-f"];
    add_args.extend(existing.iter());
    sgit(&add_args, &cfg)?;

    // On an empty repo (no commits), status --porcelain shows nothing.
    // Detect this case by checking if HEAD exists.
    let head_check = sgit(&["rev-parse", "HEAD"], &cfg)?;
    let is_empty_repo = !head_check.status.success();

    let status_out = sgit(&["status", "--porcelain"], &cfg)?;
    let has_changes = is_empty_repo || !String::from_utf8_lossy(&status_out.stdout).trim().is_empty();

    if has_changes {
        let commit_out = sgit(&["commit", "-m", message], &cfg)?;
        if !commit_out.status.success() {
            let err = String::from_utf8_lossy(&commit_out.stderr);
            println!("{} Valet commit: {}", "!".yellow(), err.trim());
        } else {
            println!("{} Valet committed", "->".cyan());
        }
    }

    push()?;
    Ok(())
}

/// `git valet push`
pub fn push() -> Result<()> {
    let cfg = load_config()?;

    let out = sgit(&["push", "origin", &format!("HEAD:{}", cfg.branch)], &cfg)?;

    if out.status.success() {
        println!("{} Valet pushed to {}", "+".green(), cfg.remote.cyan());
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        if err.contains("Everything up-to-date") || err.contains("up to date") {
            println!("{} Valet already up to date", "+".green());
        } else {
            println!("{} Valet push failed: {}", "!".yellow(), err.trim());
        }
    }

    Ok(())
}

/// `git valet pull`
pub fn pull() -> Result<()> {
    let cfg = load_config()?;

    let out = sgit(&["pull", "origin", &cfg.branch], &cfg)?;

    if out.status.success() {
        let stdout = String::from_utf8_lossy(&out.stdout);
        if stdout.contains("Already up to date") || stdout.contains("up to date") {
            println!("{} Valet already up to date", "+".green());
        } else {
            println!("{} Valet updated", "+".green());
            println!("{}", stdout.trim().dimmed());
        }
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        println!("{} Valet pull failed: {}", "!".yellow(), err.trim());
    }

    Ok(())
}

/// `git valet add <files>`
pub fn add_files(files: &[String]) -> Result<()> {
    let work_tree = get_work_tree()?;
    let origin = get_origin(&work_tree)?;
    let project_id = config::project_id(&origin);

    let mut cfg = load_config()?;

    for f in files {
        if !cfg.tracked.contains(f) {
            cfg.tracked.push(f.clone());
        }
    }
    config::save(&cfg, &project_id)?;

    let git_dir = get_git_dir(&work_tree)?;
    update_exclude(&git_dir, files)?;

    let file_refs: Vec<&str> = files.iter().map(|f| f.as_str()).collect();
    let mut add_args = vec!["add", "-f"];
    add_args.extend(file_refs.iter());
    sgit(&add_args, &cfg)?;

    println!("{} {} file(s) added to valet", "+".green(), files.len());
    Ok(())
}

/// `git valet deinit`
pub fn deinit() -> Result<()> {
    let work_tree = get_work_tree()?;
    let origin = get_origin(&work_tree)?;
    let git_dir = get_git_dir(&work_tree)?;
    let project_id = config::project_id(&origin);

    let cfg = load_config()?;

    println!("{}", "Removing valet repo...".yellow().bold());

    hooks::uninstall(&git_dir)?;
    println!("{} Hooks removed", "->".cyan());

    remove_from_exclude(&git_dir, &cfg.tracked)?;
    println!("{} .git/info/exclude cleaned up", "->".cyan());

    config::remove(&project_id)?;
    println!("{} Local config removed", "->".cyan());

    println!("\n{}", "Done! Valet repo removed.".green());
    println!("{}", "Note: the remote repo is unchanged.".dimmed());

    Ok(())
}
