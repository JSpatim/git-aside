use std::fs;
use tempfile::TempDir;

use git_valet::hooks;

#[test]
fn hook_install_creates_files() {
    let tmp = TempDir::new().unwrap();
    let git_dir = tmp.path();

    hooks::install(git_dir).unwrap();

    let hooks_dir = git_dir.join("hooks");
    for name in &["pre-commit", "pre-push", "post-merge", "post-checkout"] {
        let hook_path = hooks_dir.join(name);
        assert!(hook_path.exists(), "Hook {name} should exist");
        let content = fs::read_to_string(&hook_path).unwrap();
        assert!(content.contains("# git-valet:"), "Hook {name} should contain marker");
    }
}

#[test]
fn hook_append_does_not_duplicate_shebang() {
    let tmp = TempDir::new().unwrap();
    let git_dir = tmp.path();
    let hooks_dir = git_dir.join("hooks");
    fs::create_dir_all(&hooks_dir).unwrap();

    // Write an existing hook
    let existing = "#!/bin/sh\necho 'existing hook'\n";
    fs::write(hooks_dir.join("pre-commit"), existing).unwrap();

    hooks::install(git_dir).unwrap();

    let result = fs::read_to_string(hooks_dir.join("pre-commit")).unwrap();
    assert_eq!(result.matches("#!/bin/sh").count(), 1, "Should have only one shebang");
    assert!(result.contains("# git-valet:"), "Should contain valet marker");
    assert!(result.contains("existing hook"), "Should preserve existing hook");
}

#[test]
fn hook_uninstall_removes_valet_block() {
    let tmp = TempDir::new().unwrap();
    let git_dir = tmp.path();
    let hooks_dir = git_dir.join("hooks");
    fs::create_dir_all(&hooks_dir).unwrap();

    // Write hook with existing content + valet block
    let content = "#!/bin/sh\necho 'my hook'\n";
    fs::write(hooks_dir.join("pre-commit"), content).unwrap();

    // Install then uninstall
    hooks::install(git_dir).unwrap();
    hooks::uninstall(git_dir).unwrap();

    let result = fs::read_to_string(hooks_dir.join("pre-commit")).unwrap();
    assert!(result.contains("my hook"), "Should preserve existing hook");
    assert!(!result.contains("git-valet"), "Should remove valet block");
}

#[test]
fn hook_uninstall_deletes_empty_hooks() {
    let tmp = TempDir::new().unwrap();
    let git_dir = tmp.path();

    // Install then uninstall (no pre-existing content)
    hooks::install(git_dir).unwrap();
    hooks::uninstall(git_dir).unwrap();

    let hooks_dir = git_dir.join("hooks");
    for name in &["pre-commit", "pre-push", "post-merge", "post-checkout"] {
        assert!(
            !hooks_dir.join(name).exists(),
            "Hook {name} should be deleted when empty after removal"
        );
    }
}
