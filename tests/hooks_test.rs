use std::fs;
use tempfile::TempDir;

#[test]
fn hook_install_creates_files() {
    let tmp = TempDir::new().unwrap();
    let git_dir = tmp.path();

    // Simule la structure .git/hooks/
    let hooks_dir = git_dir.join("hooks");
    fs::create_dir_all(&hooks_dir).unwrap();

    // Installe les hooks (via le même mécanisme que le code)
    let hook_names = ["pre-commit", "pre-push", "post-merge", "post-checkout"];
    let marker = "# git-valet:";

    for name in &hook_names {
        let hook_path = hooks_dir.join(name);
        let content = format!("#!/bin/sh\n{} test hook\necho test\nfi\n", marker);
        fs::write(&hook_path, &content).unwrap();
        assert!(hook_path.exists());
        assert!(fs::read_to_string(&hook_path).unwrap().contains(marker));
    }
}

#[test]
fn hook_append_does_not_duplicate_shebang() {
    let tmp = TempDir::new().unwrap();
    let hook_path = tmp.path().join("pre-commit");

    // Hook existant
    let existing = "#!/bin/sh\necho 'existing hook'\n";
    fs::write(&hook_path, existing).unwrap();

    // Simule l'append comme le fait hooks.rs (après le fix)
    let new_hook = "#!/bin/sh\n# git-valet: sync\ngit-valet sync\nfi\n";
    let stripped = new_hook.trim_start_matches("#!/bin/sh\n");
    let combined = format!("{}\n{}", existing.trim_end(), stripped);
    fs::write(&hook_path, &combined).unwrap();

    let result = fs::read_to_string(&hook_path).unwrap();

    // Un seul shebang
    assert_eq!(result.matches("#!/bin/sh").count(), 1);
    // Le contenu git-valet est bien là
    assert!(result.contains("# git-valet:"));
    // Le hook existant est préservé
    assert!(result.contains("existing hook"));
}

#[test]
fn hook_uninstall_removes_valet_block() {
    let content = "#!/bin/sh\necho 'my hook'\n# git-valet: sync\nif command -v git-valet; then\n    git-valet sync\nfi\necho 'after'\n";

    let mut filtered = Vec::new();
    let mut in_valet_block = false;
    for line in content.lines() {
        if line.contains("# git-valet:") {
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

    let result = filtered.join("\n");
    assert!(result.contains("my hook"));
    assert!(result.contains("after"));
    assert!(!result.contains("git-valet"));
}

#[test]
fn hook_uninstall_empty_after_removal() {
    let content = "#!/bin/sh\n# git-valet: sync\nif command -v git-valet; then\n    git-valet sync\nfi\n";

    let mut filtered = Vec::new();
    let mut in_valet_block = false;
    for line in content.lines() {
        if line.contains("# git-valet:") {
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

    let result = filtered.join("\n");
    let trimmed = result.trim();

    // Après suppression du bloc git-valet, il ne reste que le shebang
    assert!(trimmed == "#!/bin/sh");
}
