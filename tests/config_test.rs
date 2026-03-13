#[test]
fn project_id_is_deterministic() {
    use sha2::{Digest, Sha256};

    let url = "git@github.com:user/repo.git";
    let mut hasher = Sha256::new();
    hasher.update(url.as_bytes());
    let result = hasher.finalize();
    let id = hex::encode(&result[..8]);

    // Même input = même output
    let mut hasher2 = Sha256::new();
    hasher2.update(url.as_bytes());
    let result2 = hasher2.finalize();
    let id2 = hex::encode(&result2[..8]);

    assert_eq!(id, id2);
    assert_eq!(id.len(), 16); // 8 bytes = 16 hex chars
}

#[test]
fn project_id_differs_for_different_remotes() {
    use sha2::{Digest, Sha256};

    let hash = |url: &str| -> String {
        let mut hasher = Sha256::new();
        hasher.update(url.as_bytes());
        let result = hasher.finalize();
        hex::encode(&result[..8])
    };

    let id1 = hash("git@github.com:user/repo-a.git");
    let id2 = hash("git@github.com:user/repo-b.git");

    assert_ne!(id1, id2);
}

#[test]
fn config_roundtrip_toml() {
    #[derive(Debug, serde::Serialize, serde::Deserialize, PartialEq)]
    struct ValetConfig {
        work_tree: String,
        remote: String,
        bare_path: String,
        tracked: Vec<String>,
        #[serde(default = "default_branch")]
        branch: String,
    }

    fn default_branch() -> String {
        "main".to_string()
    }

    let cfg = ValetConfig {
        work_tree: "/home/user/project".to_string(),
        remote: "git@github.com:user/project-private.git".to_string(),
        bare_path: "/home/user/.git-valets/abc123/repo.git".to_string(),
        tracked: vec!["CLAUDE.md".to_string(), ".env".to_string()],
        branch: "main".to_string(),
    };

    let serialized = toml::to_string_pretty(&cfg).unwrap();
    let deserialized: ValetConfig = toml::from_str(&serialized).unwrap();

    assert_eq!(cfg, deserialized);
}

#[test]
fn config_toml_default_branch() {
    #[derive(Debug, serde::Serialize, serde::Deserialize, PartialEq)]
    struct ValetConfig {
        work_tree: String,
        remote: String,
        bare_path: String,
        tracked: Vec<String>,
        #[serde(default = "default_branch")]
        branch: String,
    }

    fn default_branch() -> String {
        "main".to_string()
    }

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
