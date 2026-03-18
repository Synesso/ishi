use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A thread's link to a Linear issue and the workspace it was started in.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ThreadLink {
    pub issue: String,
    pub workspace: String,
}

/// Persistent state for ishi, stored at `~/.config/ishi/state.toml`.
///
/// Two data structures:
/// * **Thread links** — maps thread ID → issue identifier + workspace path.
///   One issue can have many threads, but each thread belongs to exactly one issue.
/// * **Workspace history** — ordered list of previously used directory paths,
///   most recent first.
#[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq)]
pub struct State {
    /// thread ID → link (issue identifier + workspace path)
    #[serde(default)]
    pub thread_links: HashMap<String, ThreadLink>,

    /// Ordered list of workspace directory paths, most recent first.
    #[serde(default)]
    pub workspace_history: Vec<String>,
}

#[allow(dead_code)]
impl State {
    /// Load state from a TOML file, returning default if the file does not exist.
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let contents = std::fs::read_to_string(path)?;
        let state: State = toml::from_str(&contents)?;
        Ok(state)
    }

    /// Save state to a TOML file, creating parent directories as needed.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let contents = toml::to_string_pretty(self)?;
        std::fs::write(path, contents)?;
        Ok(())
    }

    /// Record a thread link: thread ID → issue identifier + workspace path.
    /// If the thread already exists, it is updated.
    pub fn add_thread_link(&mut self, thread_id: &str, issue: &str, workspace: &str) {
        self.thread_links.insert(
            thread_id.to_string(),
            ThreadLink {
                issue: issue.to_string(),
                workspace: workspace.to_string(),
            },
        );
    }

    /// Return all thread IDs linked to the given issue identifier.
    pub fn threads_for_issue(&self, issue_identifier: &str) -> Vec<&str> {
        let mut ids: Vec<&str> = self
            .thread_links
            .iter()
            .filter(|(_, link)| link.issue == issue_identifier)
            .map(|(id, _)| id.as_str())
            .collect();
        ids.sort();
        ids
    }

    /// Look up the workspace directory for a given thread ID.
    pub fn workspace_for(&self, thread_id: &str) -> Option<&str> {
        self.thread_links
            .get(thread_id)
            .map(|link| link.workspace.as_str())
    }

    /// Add a workspace directory to the front of the history.
    /// If it already appears, it is moved to the front.
    pub fn add_workspace(&mut self, workspace: &str) {
        self.workspace_history
            .retain(|w| w != workspace);
        self.workspace_history.insert(0, workspace.to_string());
    }

    /// Return the ordered workspace history (most recent first).
    pub fn workspaces(&self) -> &[String] {
        &self.workspace_history
    }
}

/// Return the default state file path: `~/.config/ishi/state.toml`.
pub fn state_path() -> Result<PathBuf> {
    let dir = dirs::config_dir()
        .ok_or_else(|| anyhow::anyhow!("could not determine config directory"))?
        .join("ishi");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("state.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_missing_file_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.toml");
        let state = State::load(&path).unwrap();
        assert!(state.thread_links.is_empty());
        assert!(state.workspace_history.is_empty());
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.toml");

        let mut state = State::default();
        state.add_thread_link("T-abc-123", "JEM-1", "/home/user/project-a");
        state.add_thread_link("T-def-456", "JEM-1", "/home/user/project-a");
        state.add_thread_link("T-ghi-789", "JEM-2", "/home/user/project-b");
        state.add_workspace("/home/user/project-a");
        state.add_workspace("/home/user/project-b");

        state.save(&path).unwrap();

        let loaded = State::load(&path).unwrap();
        assert_eq!(loaded, state);
    }

    #[test]
    fn threads_for_issue_unknown_returns_empty() {
        let state = State::default();
        assert!(state.threads_for_issue("JEM-999").is_empty());
    }

    #[test]
    fn threads_for_issue_returns_matching_threads() {
        let mut state = State::default();
        state.add_thread_link("T-abc", "JEM-1", "/ws");
        state.add_thread_link("T-def", "JEM-1", "/ws");
        state.add_thread_link("T-ghi", "JEM-2", "/ws");

        let threads = state.threads_for_issue("JEM-1");
        assert_eq!(threads.len(), 2);
        assert!(threads.contains(&"T-abc"));
        assert!(threads.contains(&"T-def"));
    }

    #[test]
    fn add_thread_link_overwrites_existing() {
        let mut state = State::default();
        state.add_thread_link("T-abc", "JEM-1", "/old/path");
        state.add_thread_link("T-abc", "JEM-1", "/new/path");

        assert_eq!(state.thread_links.len(), 1);
        assert_eq!(state.workspace_for("T-abc"), Some("/new/path"));
    }

    #[test]
    fn workspace_for_unknown_thread_returns_none() {
        let state = State::default();
        assert!(state.workspace_for("T-unknown").is_none());
    }

    #[test]
    fn workspace_for_known_thread() {
        let mut state = State::default();
        state.add_thread_link("T-abc", "JEM-1", "/home/user/project");
        assert_eq!(state.workspace_for("T-abc"), Some("/home/user/project"));
    }

    #[test]
    fn add_workspace_inserts_at_front() {
        let mut state = State::default();
        state.add_workspace("/first");
        state.add_workspace("/second");
        assert_eq!(state.workspaces(), &["/second", "/first"]);
    }

    #[test]
    fn add_workspace_deduplicates_and_moves_to_front() {
        let mut state = State::default();
        state.add_workspace("/a");
        state.add_workspace("/b");
        state.add_workspace("/a");
        assert_eq!(state.workspaces(), &["/a", "/b"]);
    }

    #[test]
    fn workspaces_empty_by_default() {
        let state = State::default();
        assert!(state.workspaces().is_empty());
    }

    #[test]
    fn save_creates_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("dir").join("state.toml");

        let state = State::default();
        state.save(&path).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn workspace_history_round_trips_through_save_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.toml");

        let mut state = State::default();
        state.add_workspace("/project-a");
        state.add_workspace("/project-b");
        state.add_workspace("/project-c");
        state.save(&path).unwrap();

        let loaded = State::load(&path).unwrap();
        assert_eq!(loaded.workspaces(), &["/project-c", "/project-b", "/project-a"]);
    }

    #[test]
    fn thread_link_round_trips_through_save_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.toml");

        let mut state = State::default();
        state.add_thread_link("T-abc", "JEM-1", "/workspace/dir");
        state.save(&path).unwrap();

        let loaded = State::load(&path).unwrap();
        assert_eq!(loaded.workspace_for("T-abc"), Some("/workspace/dir"));
        let threads = loaded.threads_for_issue("JEM-1");
        assert_eq!(threads, vec!["T-abc"]);
    }

    #[test]
    fn load_empty_toml_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.toml");
        std::fs::write(&path, "").unwrap();

        let state = State::load(&path).unwrap();
        assert!(state.thread_links.is_empty());
        assert!(state.workspace_history.is_empty());
    }

    #[test]
    fn load_toml_without_workspace_history() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.toml");
        let toml = r#"
[thread_links.T-abc]
issue = "JEM-1"
workspace = "/ws"
"#;
        std::fs::write(&path, toml).unwrap();

        let state = State::load(&path).unwrap();
        assert_eq!(state.threads_for_issue("JEM-1"), vec!["T-abc"]);
        assert!(state.workspace_history.is_empty());
    }

    #[test]
    fn load_toml_without_thread_links() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.toml");
        let toml = r#"workspace_history = ["/a", "/b"]"#;
        std::fs::write(&path, toml).unwrap();

        let state = State::load(&path).unwrap();
        assert!(state.thread_links.is_empty());
        assert_eq!(state.workspaces(), &["/a", "/b"]);
    }
}
