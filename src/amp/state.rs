use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

/// A thread's link to a Linear issue and the workspace it was started in.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ThreadLink {
    pub issue: String,
    pub workspace: String,
}

/// Lifecycle state for a background Amp session run.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionRunStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Stale,
}

impl Default for SessionRunStatus {
    fn default() -> Self {
        Self::Pending
    }
}

/// Metadata for one backgroundable Amp session run.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct SessionRun {
    /// Thread this run targets.
    pub thread_id: String,
    /// Linear issue identifier this run is associated with.
    pub issue: String,
    /// Workspace where this run executes.
    pub workspace: String,
    /// Process ID while the run is active.
    #[serde(default)]
    pub pid: Option<u32>,
    /// Current lifecycle status.
    #[serde(default)]
    pub status: SessionRunStatus,
    /// Path to run log output, if one is recorded.
    #[serde(default)]
    pub log_path: Option<String>,
    /// Run record creation timestamp (milliseconds since Unix epoch).
    pub created_at_ms: u64,
    /// Last metadata update timestamp (milliseconds since Unix epoch).
    pub updated_at_ms: u64,
    /// Start timestamp when the process actually begins (milliseconds since epoch).
    #[serde(default)]
    pub started_at_ms: Option<u64>,
    /// Terminal timestamp for completed/failed runs (milliseconds since epoch).
    #[serde(default)]
    pub finished_at_ms: Option<u64>,
}

/// Persistent state for ishi, stored at `~/.config/ishi/state.toml`.
///
/// Three data structures:
/// * **Thread links** — maps thread ID → issue identifier + workspace path.
///   One issue can have many threads, but each thread belongs to exactly one issue.
/// * **Workspace history** — ordered list of previously used directory paths,
///   most recent first.
/// * **Session runs** — maps run ID → lifecycle metadata for backgroundable Amp
///   session execution.
#[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq)]
pub struct State {
    /// thread ID → link (issue identifier + workspace path)
    #[serde(default)]
    pub thread_links: HashMap<String, ThreadLink>,

    /// Ordered list of workspace directory paths, most recent first.
    #[serde(default)]
    pub workspace_history: Vec<String>,

    /// run ID → session run metadata.
    #[serde(default)]
    pub session_runs: BTreeMap<String, SessionRun>,
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
        self.workspace_history.retain(|w| w != workspace);
        self.workspace_history.insert(0, workspace.to_string());
    }

    /// Return the ordered workspace history (most recent first).
    pub fn workspaces(&self) -> &[String] {
        &self.workspace_history
    }

    /// Record session run metadata: run ID → lifecycle info.
    /// If the run already exists, it is updated.
    pub fn add_session_run(&mut self, run_id: &str, run: SessionRun) {
        self.session_runs.insert(run_id.to_string(), run);
    }

    /// Look up a session run by run ID.
    pub fn session_run(&self, run_id: &str) -> Option<&SessionRun> {
        self.session_runs.get(run_id)
    }

    /// Return all session runs keyed by run ID in deterministic key order.
    pub fn session_runs(&self) -> &BTreeMap<String, SessionRun> {
        &self.session_runs
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
        assert!(state.session_runs.is_empty());
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
        state.add_session_run(
            "run-1",
            SessionRun {
                thread_id: "T-abc-123".to_string(),
                issue: "JEM-1".to_string(),
                workspace: "/home/user/project-a".to_string(),
                pid: Some(4242),
                status: SessionRunStatus::Running,
                log_path: Some("/tmp/ishi-run-1.log".to_string()),
                created_at_ms: 1_710_000_000_000,
                updated_at_ms: 1_710_000_000_500,
                started_at_ms: Some(1_710_000_000_250),
                finished_at_ms: None,
            },
        );

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
    fn session_runs_empty_by_default() {
        let state = State::default();
        assert!(state.session_runs().is_empty());
    }

    #[test]
    fn add_session_run_overwrites_existing() {
        let mut state = State::default();
        state.add_session_run(
            "run-1",
            SessionRun {
                thread_id: "T-abc".to_string(),
                issue: "JEM-1".to_string(),
                workspace: "/ws".to_string(),
                pid: Some(111),
                status: SessionRunStatus::Pending,
                log_path: None,
                created_at_ms: 10,
                updated_at_ms: 10,
                started_at_ms: None,
                finished_at_ms: None,
            },
        );
        state.add_session_run(
            "run-1",
            SessionRun {
                thread_id: "T-abc".to_string(),
                issue: "JEM-1".to_string(),
                workspace: "/ws".to_string(),
                pid: Some(222),
                status: SessionRunStatus::Running,
                log_path: Some("/tmp/run-1.log".to_string()),
                created_at_ms: 10,
                updated_at_ms: 20,
                started_at_ms: Some(15),
                finished_at_ms: None,
            },
        );

        assert_eq!(state.session_runs.len(), 1);
        assert_eq!(state.session_run("run-1").and_then(|r| r.pid), Some(222));
        assert_eq!(
            state.session_run("run-1").map(|r| r.status),
            Some(SessionRunStatus::Running)
        );
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
        assert_eq!(
            loaded.workspaces(),
            &["/project-c", "/project-b", "/project-a"]
        );
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
        assert!(state.session_runs.is_empty());
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
        assert!(state.session_runs.is_empty());
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
        assert!(state.session_runs.is_empty());
    }

    #[test]
    fn load_toml_without_session_runs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.toml");
        let toml = r#"
workspace_history = ["/a", "/b"]

[thread_links.T-abc]
issue = "JEM-1"
workspace = "/ws"
"#;
        std::fs::write(&path, toml).unwrap();

        let state = State::load(&path).unwrap();
        assert_eq!(state.threads_for_issue("JEM-1"), vec!["T-abc"]);
        assert_eq!(state.workspaces(), &["/a", "/b"]);
        assert!(state.session_runs.is_empty());
    }

    #[test]
    fn session_run_round_trips_through_save_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.toml");

        let mut state = State::default();
        state.add_session_run(
            "run-xyz",
            SessionRun {
                thread_id: "T-abc".to_string(),
                issue: "JEM-9".to_string(),
                workspace: "/workspace/jem-9".to_string(),
                pid: Some(9876),
                status: SessionRunStatus::Completed,
                log_path: Some("/tmp/jem-9-run.log".to_string()),
                created_at_ms: 1_710_000_100_000,
                updated_at_ms: 1_710_000_120_000,
                started_at_ms: Some(1_710_000_101_000),
                finished_at_ms: Some(1_710_000_119_000),
            },
        );
        state.save(&path).unwrap();

        let loaded = State::load(&path).unwrap();
        assert_eq!(loaded, state);
        assert_eq!(
            loaded.session_run("run-xyz").map(|r| r.status),
            Some(SessionRunStatus::Completed)
        );
    }

    #[test]
    fn session_runs_serialize_deterministically() {
        let mut state = State::default();
        state.add_session_run(
            "run-b",
            SessionRun {
                thread_id: "T-b".to_string(),
                issue: "JEM-2".to_string(),
                workspace: "/ws/b".to_string(),
                pid: None,
                status: SessionRunStatus::Pending,
                log_path: None,
                created_at_ms: 2,
                updated_at_ms: 2,
                started_at_ms: None,
                finished_at_ms: None,
            },
        );
        state.add_session_run(
            "run-a",
            SessionRun {
                thread_id: "T-a".to_string(),
                issue: "JEM-1".to_string(),
                workspace: "/ws/a".to_string(),
                pid: Some(123),
                status: SessionRunStatus::Running,
                log_path: Some("/tmp/run-a.log".to_string()),
                created_at_ms: 1,
                updated_at_ms: 3,
                started_at_ms: Some(2),
                finished_at_ms: None,
            },
        );

        let toml_first = toml::to_string_pretty(&state).unwrap();
        let toml_second = toml::to_string_pretty(&state).unwrap();

        assert_eq!(toml_first, toml_second);
        let run_a_idx = toml_first.find("[session_runs.run-a]").unwrap();
        let run_b_idx = toml_first.find("[session_runs.run-b]").unwrap();
        assert!(run_a_idx < run_b_idx);
        assert!(toml_first.contains("status = \"running\""));
    }
}
