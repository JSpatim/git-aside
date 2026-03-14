use git_valet::config::{self, ValetConfig};

#[test]
fn project_id_is_deterministic() {
    let url = "git@github.com:user/repo.git";
    let id1 = config::project_id(url);
    let id2 = config::project_id(url);

    assert_eq!(id1, id2);
    assert_eq!(id1.len(), 16); // 8 bytes = 16 hex chars
}

#[test]
fn project_id_differs_for_different_remotes() {
    let id1 = config::project_id("git@github.com:user/repo-a.git");
    let id2 = config::project_id("git@github.com:user/repo-b.git");

    assert_ne!(id1, id2);
}

#[test]
fn config_roundtrip_toml() {
    let cfg = ValetConfig {
        work_tree: "/home/user/project".to_string(),
        remote: "git@github.com:user/project-private.git".to_string(),
        bare_path: "/home/user/.git-valets/abc123/repo.git".to_string(),
        tracked: vec!["CLAUDE.md".to_string(), ".env".to_string()],
        branch: "main".to_string(),
    };

    let serialized = toml::to_string_pretty(&cfg).unwrap();
    let deserialized: ValetConfig = toml::from_str(&serialized).unwrap();

    assert_eq!(cfg.work_tree, deserialized.work_tree);
    assert_eq!(cfg.remote, deserialized.remote);
    assert_eq!(cfg.bare_path, deserialized.bare_path);
    assert_eq!(cfg.tracked, deserialized.tracked);
    assert_eq!(cfg.branch, deserialized.branch);
}

#[test]
fn config_toml_default_branch() {
    // TOML sans champ branch → doit default à "main"
    let toml_str = r#"
work_tree = "/home/user/project"
remote = "git@github.com:user/project-private.git"
bare_path = "/home/user/.git-valets/abc123/repo.git"
tracked = ["CLAUDE.md"]
"#;

    let cfg: ValetConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(cfg.branch, "main");
}
