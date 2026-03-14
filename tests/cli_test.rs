use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn cli_version_flag() {
    Command::cargo_bin("git-valet")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("git-valet"));
}

#[test]
fn cli_help_flag() {
    Command::cargo_bin("git-valet")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("private files"));
}

#[test]
fn cli_status_without_repo_fails() {
    Command::cargo_bin("git-valet")
        .unwrap()
        .arg("status")
        .current_dir(std::env::temp_dir())
        .assert()
        .failure()
        .stderr(predicate::str::contains("error:"));
}

#[test]
fn cli_completions_bash() {
    Command::cargo_bin("git-valet")
        .unwrap()
        .args(["completions", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("git-valet"));
}

#[test]
fn cli_completions_zsh() {
    Command::cargo_bin("git-valet").unwrap().args(["completions", "zsh"]).assert().success();
}

#[test]
fn cli_respects_no_color() {
    Command::cargo_bin("git-valet")
        .unwrap()
        .arg("status")
        .current_dir(std::env::temp_dir())
        .env("NO_COLOR", "1")
        .assert()
        .failure()
        // Should not contain ANSI escape codes
        .stderr(predicate::str::contains("\x1b[").not());
}
