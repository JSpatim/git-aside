//! Configuration management for git-valet.
//!
//! Stores per-project configuration in `~/.git-valets/<project-id>/config.toml`.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ValetConfig {
    /// Absolute path of the main repo (work-tree)
    pub work_tree: String,
    /// Remote of the valet repo
    pub remote: String,
    /// Absolute path of the valet bare repo
    pub bare_path: String,
    /// Tracked files/directories
    pub tracked: Vec<String>,
    /// Valet repo branch (default: "main")
    #[serde(default = "default_branch")]
    pub branch: String,
}

fn default_branch() -> String {
    "main".to_string()
}

/// Returns the ~/.git-valets/ directory
pub fn valets_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Could not find home directory")?;
    let dir = home.join(".git-valets");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Generates a unique ID based on the main remote URL (BLAKE3, 16 hex chars)
pub fn project_id(origin_url: &str) -> String {
    let hash = blake3::hash(origin_url.as_bytes());
    let bytes = hash.as_bytes();
    hex::encode(&bytes[..8])
}

/// Returns the config file path for the current project
pub fn config_path_for(project_id: &str) -> Result<PathBuf> {
    Ok(valets_dir()?.join(project_id).join("config.toml"))
}

/// Loads the valet config for the current repo
pub fn load(main_remote: &str) -> Result<ValetConfig> {
    let id = project_id(main_remote);
    let path = config_path_for(&id)?;
    let content = std::fs::read_to_string(&path)
        .context("Valet repo not initialized. Run: git valet init <remote> <files>")?;
    let config: ValetConfig = toml::from_str(&content).context("Valet config is corrupted")?;
    Ok(config)
}

/// Saves the valet config
pub fn save(config: &ValetConfig, project_id: &str) -> Result<()> {
    let path = config_path_for(project_id)?;
    std::fs::create_dir_all(path.parent().context("Invalid config path")?)?;
    let content = toml::to_string_pretty(config)?;
    std::fs::write(&path, content)?;
    Ok(())
}

/// Removes the valet config
pub fn remove(project_id: &str) -> Result<()> {
    let dir = valets_dir()?.join(project_id);
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    Ok(())
}

/// Hex-encodes a byte slice (lightweight replacement for the `hex` crate)
mod hex {
    use std::fmt::Write;

    pub fn encode(bytes: &[u8]) -> String {
        bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
    }
}
