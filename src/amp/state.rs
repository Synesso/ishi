use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Maps Linear issue identifiers (e.g. "JEM-91") to a list of Amp thread IDs.
#[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq)]
pub struct State {
    /// issue identifier → list of thread IDs
    pub threads: HashMap<String, Vec<String>>,
}

#[allow(dead_code)]
impl State {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let contents = std::fs::read_to_string(path)?;
        let state: State = serde_json::from_str(&contents)?;
        Ok(state)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let contents = serde_json::to_string_pretty(self)?;
        std::fs::write(path, contents)?;
        Ok(())
    }

    pub fn thread_ids_for(&self, issue_identifier: &str) -> &[String] {
        self.threads
            .get(issue_identifier)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    pub fn link_thread(&mut self, issue_identifier: &str, thread_id: &str) {
        let ids = self
            .threads
            .entry(issue_identifier.to_string())
            .or_default();
        if !ids.contains(&thread_id.to_string()) {
            ids.push(thread_id.to_string());
        }
    }
}

pub fn state_path() -> Result<PathBuf> {
    let dir = dirs::data_dir()
        .ok_or_else(|| anyhow::anyhow!("could not determine data directory"))?
        .join("ishi");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("state.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_missing_file_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.json");
        let state = State::load(&path).unwrap();
        assert!(state.threads.is_empty());
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");

        let mut state = State::default();
        state.link_thread("JEM-1", "T-abc-123");
        state.link_thread("JEM-1", "T-def-456");
        state.link_thread("JEM-2", "T-ghi-789");

        state.save(&path).unwrap();

        let loaded = State::load(&path).unwrap();
        assert_eq!(loaded, state);
    }

    #[test]
    fn thread_ids_for_unknown_issue_returns_empty() {
        let state = State::default();
        assert!(state.thread_ids_for("JEM-999").is_empty());
    }

    #[test]
    fn thread_ids_for_known_issue() {
        let mut state = State::default();
        state.link_thread("JEM-1", "T-abc");
        state.link_thread("JEM-1", "T-def");
        assert_eq!(state.thread_ids_for("JEM-1"), &["T-abc", "T-def"]);
    }

    #[test]
    fn link_thread_prevents_duplicates() {
        let mut state = State::default();
        state.link_thread("JEM-1", "T-abc");
        state.link_thread("JEM-1", "T-abc");
        assert_eq!(state.thread_ids_for("JEM-1").len(), 1);
    }

    #[test]
    fn save_creates_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("dir").join("state.json");

        let state = State::default();
        state.save(&path).unwrap();
        assert!(path.exists());
    }
}
