//! Git hook installation and uninstallation for transparent valet syncing.

use anyhow::Result;
use std::path::Path;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

const PRE_COMMIT_HOOK: &str = r#"#!/bin/sh
# git-valet: sync valet repo before commit
if command -v git-valet >/dev/null 2>&1; then
    git-valet sync --message "chore: auto-sync before commit" 2>/dev/null || true
fi
"#;

const PRE_PUSH_HOOK: &str = r#"#!/bin/sh
# git-valet: sync valet repo before push
if command -v git-valet >/dev/null 2>&1; then
    git-valet sync --message "chore: auto-sync before push" 2>/dev/null || true
fi
"#;

const POST_MERGE_HOOK: &str = r"#!/bin/sh
# git-valet: pull valet repo after merge
if command -v git-valet >/dev/null 2>&1; then
    git-valet pull 2>/dev/null || true
fi
";

const POST_CHECKOUT_HOOK: &str = r"#!/bin/sh
# git-valet: pull valet repo after checkout
if command -v git-valet >/dev/null 2>&1; then
    git-valet pull 2>/dev/null || true
fi
";

pub struct HookInstall {
    pub name: &'static str,
    pub content: &'static str,
}

const HOOKS: &[HookInstall] = &[
    HookInstall { name: "pre-commit", content: PRE_COMMIT_HOOK },
    HookInstall { name: "pre-push", content: PRE_PUSH_HOOK },
    HookInstall { name: "post-merge", content: POST_MERGE_HOOK },
    HookInstall { name: "post-checkout", content: POST_CHECKOUT_HOOK },
];

const SHADOW_MARKER: &str = "# git-valet:";

/// Installs git-valet hooks in the main repo
pub fn install(git_dir: &Path) -> Result<()> {
    let hooks_dir = git_dir.join("hooks");
    std::fs::create_dir_all(&hooks_dir)?;

    for hook in HOOKS {
        let hook_path = hooks_dir.join(hook.name);

        if hook_path.exists() {
            let existing = std::fs::read_to_string(&hook_path)?;
            if existing.contains(SHADOW_MARKER) {
                continue; // Already installed
            }
            // Append without duplicating the shebang (handle both LF and CRLF)
            let stripped =
                hook.content.trim_start_matches("#!/bin/sh\r\n").trim_start_matches("#!/bin/sh\n");
            let combined = format!("{}\n{stripped}", existing.trim_end());
            std::fs::write(&hook_path, combined)?;
        } else {
            std::fs::write(&hook_path, hook.content)?;
        }

        // Make executable (Unix only)
        #[cfg(unix)]
        {
            let mut perms = std::fs::metadata(&hook_path)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&hook_path, perms)?;
        }
    }

    Ok(())
}

/// Uninstalls git-valet hooks
pub fn uninstall(git_dir: &Path) -> Result<()> {
    let hooks_dir = git_dir.join("hooks");

    for hook in HOOKS {
        let hook_path = hooks_dir.join(hook.name);
        if !hook_path.exists() {
            continue;
        }

        let content = std::fs::read_to_string(&hook_path)?;

        if !content.contains(SHADOW_MARKER) {
            continue;
        }

        // Filter out the git-valet block (marker + following lines until "fi")
        let mut filtered = Vec::new();
        let mut in_valet_block = false;
        for line in content.lines() {
            if line.contains(SHADOW_MARKER) {
                in_valet_block = true;
                continue;
            }
            if in_valet_block {
                if line.trim() == "fi" {
                    in_valet_block = false;
                }
                continue;
            }
            filtered.push(line);
        }
        let filtered = filtered.join("\n");

        let trimmed = filtered.trim();

        if trimmed.is_empty() || trimmed == "#!/bin/sh" {
            // Hook is empty after removal — delete the file
            std::fs::remove_file(&hook_path)?;
        } else {
            std::fs::write(&hook_path, format!("{trimmed}\n"))?;
        }
    }

    Ok(())
}
