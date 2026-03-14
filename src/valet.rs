use anyhow::Result;
use colored::Colorize;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::{self, ValetConfig};
use crate::git_helpers::{get_git_dir, get_origin, get_work_tree, load_config, sgit};
use crate::hooks;

const VALET_FILE: &str = ".gitvalet";

// ── .gitvalet file ──────────────────────────────────────────────────────────

/// Reads the .gitvalet file and returns the list of tracked entries.
/// Returns an empty Vec if the file does not exist.
fn read_gitvalet(work_tree: &Path) -> Vec<String> {
    let path = work_tree.join(VALET_FILE);
    if !path.exists() {
        return Vec::new();
    }
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    content
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.to_string())
        .collect()
}

/// Writes the .gitvalet file with the given entries.
fn write_gitvalet(work_tree: &Path, files: &[String]) -> Result<()> {
    let path = work_tree.join(VALET_FILE);
    let content = files.join("\n") + "\n";
    std::fs::write(&path, content)?;
    Ok(())
}

/// Returns tracked files from .gitvalet + .gitvalet itself (always implicitly tracked).
fn tracked_with_gitvalet(work_tree: &Path) -> Vec<String> {
    let mut tracked = read_gitvalet(work_tree);
    if !tracked.iter().any(|f| f == VALET_FILE) {
        tracked.push(VALET_FILE.to_string());
    }
    tracked
}

// ── Gitignore ────────────────────────────────────────────────────────────────

/// Replaces the git-valet section in .git/info/exclude with the given files.
/// This ensures the exclude list always matches the current .gitvalet content.
fn update_exclude(git_dir: &Path, files: &[String]) -> Result<()> {
    let info_dir = git_dir.join("info");
    std::fs::create_dir_all(&info_dir)?;
    let exclude_path = info_dir.join("exclude");

    let existing = if exclude_path.exists() {
        std::fs::read_to_string(&exclude_path)?
    } else {
        String::new()
    };

    // Remove any existing git-valet section
    let marker = "# git-valet: files versioned in the valet repo";
    let mut base_lines: Vec<&str> = Vec::new();
    let mut in_valet_section = false;
    for line in existing.lines() {
        if line.trim() == marker {
            in_valet_section = true;
            continue;
        }
        if in_valet_section {
            // Valet section entries are non-empty, non-comment lines after the marker.
            // An empty line or another comment signals the end of the section.
            if line.trim().is_empty() || (line.starts_with('#') && line.trim() != marker) {
                in_valet_section = false;
                base_lines.push(line);
            }
            continue;
        }
        base_lines.push(line);
    }

    // Remove trailing empty lines from base
    while base_lines.last().map_or(false, |l| l.trim().is_empty()) {
        base_lines.pop();
    }

    let mut content = base_lines.join("\n");
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }

    if !files.is_empty() {
        content.push_str(&format!("\n{}\n", marker));
        for f in files {
            content.push_str(f);
            content.push('\n');
        }
    }

    std::fs::write(&exclude_path, content)?;
    println!("{} .git/info/exclude updated ({} entries)", "->".cyan(), files.len());
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

/// `git valet init <remote> [files...]`
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

    // 2. Temporary config (tracked list will be finalized below)
    let mut cfg = ValetConfig {
        work_tree: work_tree.to_str().unwrap().to_string(),
        remote: remote.to_string(),
        bare_path: bare_path.to_str().unwrap().to_string(),
        tracked: vec![VALET_FILE.to_string()],
        branch: "main".to_string(),
    };

    // 3. Hide untracked files from sgit status
    Command::new("git")
        .args(["--git-dir", bare_path.to_str().unwrap(), "config", "status.showUntrackedFiles", "no"])
        .output()?;

    // 4. Remote
    let remote_out = Command::new("git")
        .args(["--git-dir", bare_path.to_str().unwrap(), "remote", "add", "origin", remote])
        .output()?;
    if !remote_out.status.success() {
        Command::new("git")
            .args(["--git-dir", bare_path.to_str().unwrap(), "remote", "set-url", "origin", remote])
            .output()?;
    }

    // 5. Hooks
    hooks::install(&git_dir)?;
    println!("{} Git hooks installed (pre-commit, pre-push, post-merge, post-checkout)", "->".cyan());

    if !files.is_empty() {
        // ── First setup: files provided → create .gitvalet ──────────────
        write_gitvalet(&work_tree, files)?;
        println!("{} .gitvalet created with {} entries", "->".cyan(), files.len());

        let tracked = tracked_with_gitvalet(&work_tree);
        cfg.tracked = tracked.clone();
        config::save(&cfg, &project_id)?;
        update_exclude(&git_dir, &tracked)?;

        let existing: Vec<&str> = tracked
            .iter()
            .filter(|f| work_tree.join(f).exists())
            .map(|f| f.as_str())
            .collect();

        if !existing.is_empty() {
            let mut add_args = vec!["add", "-f"];
            add_args.extend(existing.iter());
            sgit(&add_args, &cfg)?;

            let commit_out = sgit(&["commit", "-m", "feat: init valet repo"], &cfg)?;
            if commit_out.status.success() {
                println!("{} Initial commit done", "->".cyan());

                let push_out = sgit(&["push", "-u", "origin", &format!("HEAD:{}", cfg.branch)], &cfg)?;
                if push_out.status.success() {
                    println!("{} Initial push done", "->".cyan());
                } else {
                    let err = String::from_utf8_lossy(&push_out.stderr);
                    println!("{} Initial push failed (remote unreachable?): {}", "!".yellow(), err.trim());
                    println!("  You can push manually with: {}", "git valet push".cyan());
                }
            }
        }
    } else {
        // ── Fresh clone: no files → fetch remote, read .gitvalet ────────
        config::save(&cfg, &project_id)?;

        let fetch_out = sgit(&["fetch", "origin", &cfg.branch], &cfg)?;
        if fetch_out.status.success() {
            let checkout_out = sgit(
                &["checkout", &format!("origin/{}", cfg.branch), "--", "."],
                &cfg,
            )?;
            if checkout_out.status.success() {
                sgit(&["branch", &cfg.branch, &format!("origin/{}", cfg.branch)], &cfg)?;
                sgit(&["symbolic-ref", "HEAD", &format!("refs/heads/{}", cfg.branch)], &cfg)?;
                println!("{} Pulled existing files from remote", "->".cyan());

                // Read .gitvalet that was just checked out
                let tracked = tracked_with_gitvalet(&work_tree);
                cfg.tracked = tracked.clone();
                config::save(&cfg, &project_id)?;
                update_exclude(&git_dir, &tracked)?;
            } else {
                println!("{} Remote exists but checkout failed", "!".yellow());
            }
        } else {
            println!("{} Remote is empty — create a .gitvalet file and run git valet sync", "i".blue());
        }
    }

    let tracked = &cfg.tracked;
    println!("\n{}", "Done! Valet repo initialized.".green().bold());
    println!("The following files are managed by git-valet:");
    for f in tracked {
        println!("  {} {}", "-".dimmed(), f.cyan());
    }
    println!("\nEdit {} to add/remove tracked files.", VALET_FILE.cyan());
    println!("Your usual git commands work as before.");

    Ok(())
}

/// `git valet status`
pub fn status() -> Result<()> {
    let cfg = load_config()?;
    let work_tree = PathBuf::from(&cfg.work_tree);

    // Show tracked files from .gitvalet (source of truth)
    let tracked = tracked_with_gitvalet(&work_tree);

    println!("{}", "Valet repo status".bold());
    println!("  Remote  : {}", cfg.remote.cyan());
    println!("  Tracked ({}):", VALET_FILE.cyan());
    for f in &tracked {
        let exists = work_tree.join(f).exists();
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

/// `git valet sync` — re-read .gitvalet, update excludes/config, add + commit + push
pub fn sync(message: &str) -> Result<()> {
    let mut cfg = load_config()?;
    let work_tree = PathBuf::from(&cfg.work_tree);
    let git_dir = get_git_dir(&work_tree)?;

    // Re-read .gitvalet to pick up any changes
    let tracked = tracked_with_gitvalet(&work_tree);
    if tracked != cfg.tracked {
        let origin = get_origin(&work_tree)?;
        let project_id = config::project_id(&origin);
        cfg.tracked = tracked.clone();
        config::save(&cfg, &project_id)?;
        update_exclude(&git_dir, &tracked)?;
    }

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
    let mut cfg = load_config()?;

    let out = sgit(&["pull", "origin", &cfg.branch], &cfg)?;

    if out.status.success() {
        let stdout = String::from_utf8_lossy(&out.stdout);
        if stdout.contains("Already up to date") || stdout.contains("up to date") {
            println!("{} Valet already up to date", "+".green());
        } else {
            println!("{} Valet updated", "+".green());
            println!("{}", stdout.trim().dimmed());

            // Re-read .gitvalet in case it was updated by the pull
            let work_tree = PathBuf::from(&cfg.work_tree);
            let tracked = tracked_with_gitvalet(&work_tree);
            if tracked != cfg.tracked {
                let origin = get_origin(&work_tree)?;
                let project_id = config::project_id(&origin);
                let git_dir = get_git_dir(&work_tree)?;
                cfg.tracked = tracked.clone();
                config::save(&cfg, &project_id)?;
                update_exclude(&git_dir, &tracked)?;
            }
        }
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        println!("{} Valet pull failed: {}", "!".yellow(), err.trim());
    }

    Ok(())
}

/// `git valet add <files>` — adds entries to .gitvalet and stages them
pub fn add_files(files: &[String]) -> Result<()> {
    let work_tree = get_work_tree()?;
    let origin = get_origin(&work_tree)?;
    let project_id = config::project_id(&origin);
    let git_dir = get_git_dir(&work_tree)?;

    // Read current .gitvalet and merge new entries
    let mut entries = read_gitvalet(&work_tree);
    for f in files {
        if !entries.contains(f) {
            entries.push(f.clone());
        }
    }
    write_gitvalet(&work_tree, &entries)?;

    // Update config + excludes
    let tracked = tracked_with_gitvalet(&work_tree);
    let mut cfg = load_config()?;
    cfg.tracked = tracked.clone();
    config::save(&cfg, &project_id)?;
    update_exclude(&git_dir, &tracked)?;

    // Stage the new files + .gitvalet itself
    let existing: Vec<&str> = tracked
        .iter()
        .filter(|f| work_tree.join(f).exists())
        .map(|f| f.as_str())
        .collect();
    let mut add_args = vec!["add", "-f"];
    add_args.extend(existing.iter());
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

    // Clean up all tracked files including .gitvalet itself
    let mut all_tracked = cfg.tracked.clone();
    if !all_tracked.contains(&VALET_FILE.to_string()) {
        all_tracked.push(VALET_FILE.to_string());
    }
    remove_from_exclude(&git_dir, &all_tracked)?;
    println!("{} .git/info/exclude cleaned up", "->".cyan());

    config::remove(&project_id)?;
    println!("{} Local config removed", "->".cyan());

    println!("\n{}", "Done! Valet repo removed.".green());
    println!("{}", "Note: the remote repo and local files (.gitvalet, etc.) are unchanged.".dimmed());

    Ok(())
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ── .gitvalet read/write ─────────────────────────────────────────────

    #[test]
    fn read_gitvalet_returns_empty_when_no_file() {
        let tmp = TempDir::new().unwrap();
        let result = read_gitvalet(tmp.path());
        assert!(result.is_empty());
    }

    #[test]
    fn read_gitvalet_parses_entries() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join(".gitvalet"),
            ".env\nsecrets/\nnotes/ai.md\n",
        )
        .unwrap();

        let result = read_gitvalet(tmp.path());
        assert_eq!(result, vec![".env", "secrets/", "notes/ai.md"]);
    }

    #[test]
    fn read_gitvalet_ignores_comments_and_blank_lines() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join(".gitvalet"),
            "# This is a comment\n\n.env\n  \n# Another comment\nsecrets/\n",
        )
        .unwrap();

        let result = read_gitvalet(tmp.path());
        assert_eq!(result, vec![".env", "secrets/"]);
    }

    #[test]
    fn read_gitvalet_trims_whitespace() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join(".gitvalet"), "  .env  \n  secrets/  \n").unwrap();

        let result = read_gitvalet(tmp.path());
        assert_eq!(result, vec![".env", "secrets/"]);
    }

    #[test]
    fn write_gitvalet_creates_file() {
        let tmp = TempDir::new().unwrap();
        let files = vec![".env".to_string(), "secrets/".to_string()];

        write_gitvalet(tmp.path(), &files).unwrap();

        let content = std::fs::read_to_string(tmp.path().join(".gitvalet")).unwrap();
        assert_eq!(content, ".env\nsecrets/\n");
    }

    #[test]
    fn write_then_read_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let files = vec![".env".to_string(), "config/local.toml".to_string(), "notes/".to_string()];

        write_gitvalet(tmp.path(), &files).unwrap();
        let result = read_gitvalet(tmp.path());

        assert_eq!(result, files);
    }

    // ── tracked_with_gitvalet ────────────────────────────────────────────

    #[test]
    fn tracked_with_gitvalet_adds_gitvalet_implicitly() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join(".gitvalet"), ".env\n").unwrap();

        let result = tracked_with_gitvalet(tmp.path());
        assert_eq!(result, vec![".env", ".gitvalet"]);
    }

    #[test]
    fn tracked_with_gitvalet_no_duplicate_if_explicit() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join(".gitvalet"), ".env\n.gitvalet\n").unwrap();

        let result = tracked_with_gitvalet(tmp.path());
        assert_eq!(result, vec![".env", ".gitvalet"]);
    }

    #[test]
    fn tracked_with_gitvalet_returns_just_itself_when_no_file() {
        let tmp = TempDir::new().unwrap();

        let result = tracked_with_gitvalet(tmp.path());
        assert_eq!(result, vec![".gitvalet"]);
    }

    // ── update_exclude ───────────────────────────────────────────────────

    #[test]
    fn update_exclude_creates_section() {
        let tmp = TempDir::new().unwrap();
        let git_dir = tmp.path();
        std::fs::create_dir_all(git_dir.join("info")).unwrap();

        let files = vec![".env".to_string(), ".gitvalet".to_string()];
        update_exclude(git_dir, &files).unwrap();

        let content = std::fs::read_to_string(git_dir.join("info/exclude")).unwrap();
        assert!(content.contains("# git-valet: files versioned in the valet repo"));
        assert!(content.contains(".env"));
        assert!(content.contains(".gitvalet"));
    }

    #[test]
    fn update_exclude_preserves_existing_content() {
        let tmp = TempDir::new().unwrap();
        let git_dir = tmp.path();
        let info_dir = git_dir.join("info");
        std::fs::create_dir_all(&info_dir).unwrap();
        std::fs::write(info_dir.join("exclude"), "# my custom excludes\n*.log\n").unwrap();

        let files = vec![".env".to_string()];
        update_exclude(git_dir, &files).unwrap();

        let content = std::fs::read_to_string(info_dir.join("exclude")).unwrap();
        assert!(content.contains("*.log"));
        assert!(content.contains(".env"));
    }

    #[test]
    fn update_exclude_replaces_valet_section_on_update() {
        let tmp = TempDir::new().unwrap();
        let git_dir = tmp.path();
        std::fs::create_dir_all(git_dir.join("info")).unwrap();

        // First update with .env
        let files1 = vec![".env".to_string(), ".gitvalet".to_string()];
        update_exclude(git_dir, &files1).unwrap();

        // Second update with .env + secrets/
        let files2 = vec![".env".to_string(), "secrets/".to_string(), ".gitvalet".to_string()];
        update_exclude(git_dir, &files2).unwrap();

        let content = std::fs::read_to_string(git_dir.join("info/exclude")).unwrap();

        // Only one marker section
        assert_eq!(
            content.matches("# git-valet: files versioned in the valet repo").count(),
            1
        );
        // New file is present
        assert!(content.contains("secrets/"));
        // Old file still present
        assert!(content.contains(".env"));
    }

    #[test]
    fn update_exclude_removes_section_when_empty() {
        let tmp = TempDir::new().unwrap();
        let git_dir = tmp.path();
        let info_dir = git_dir.join("info");
        std::fs::create_dir_all(&info_dir).unwrap();
        std::fs::write(
            info_dir.join("exclude"),
            "*.log\n\n# git-valet: files versioned in the valet repo\n.env\n",
        )
        .unwrap();

        // Update with empty list
        update_exclude(git_dir, &[]).unwrap();

        let content = std::fs::read_to_string(info_dir.join("exclude")).unwrap();
        assert!(!content.contains("git-valet"));
        assert!(content.contains("*.log"));
    }

    // ── remove_from_exclude ──────────────────────────────────────────────

    #[test]
    fn remove_from_exclude_cleans_entries_and_marker() {
        let tmp = TempDir::new().unwrap();
        let git_dir = tmp.path();
        let info_dir = git_dir.join("info");
        std::fs::create_dir_all(&info_dir).unwrap();
        std::fs::write(
            info_dir.join("exclude"),
            "*.log\n# git-valet: files versioned in the valet repo\n.env\n.gitvalet\n",
        )
        .unwrap();

        let files = vec![".env".to_string(), ".gitvalet".to_string()];
        remove_from_exclude(git_dir, &files).unwrap();

        let content = std::fs::read_to_string(info_dir.join("exclude")).unwrap();
        assert!(!content.contains(".env"));
        assert!(!content.contains(".gitvalet"));
        assert!(!content.contains("git-valet"));
        assert!(content.contains("*.log"));
    }

    #[test]
    fn remove_from_exclude_noop_when_no_file() {
        let tmp = TempDir::new().unwrap();
        let files = vec![".env".to_string()];
        // Should not panic
        remove_from_exclude(tmp.path(), &files).unwrap();
    }
}
