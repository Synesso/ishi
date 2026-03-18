use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Config {
    pub api_key: Option<String>,
}

pub fn config_path() -> Result<PathBuf> {
    let dir = dirs::config_dir()
        .context("could not determine config directory")?
        .join("ishi");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("config.toml"))
}

pub fn resolve_api_key() -> Result<String> {
    // 1. Environment variable
    if let Ok(key) = std::env::var("LINEAR_API_KEY") {
        if !key.is_empty() {
            return Ok(key);
        }
    }

    // 2. Config file
    let path = config_path()?;
    if path.exists() {
        let contents = std::fs::read_to_string(&path)?;
        let cfg: Config = toml::from_str(&contents)?;
        if let Some(key) = cfg.api_key {
            if !key.is_empty() {
                return Ok(key);
            }
        }
    }

    anyhow::bail!(
        "No Linear API key found.\n\
         Set LINEAR_API_KEY or add api_key to {}",
        config_path()?.display()
    )
}

pub fn store_api_key(key: &str) -> Result<()> {
    let cfg = Config {
        api_key: Some(key.to_string()),
    };
    let contents = toml::to_string_pretty(&cfg)?;
    std::fs::write(config_path()?, contents)?;
    Ok(())
}
