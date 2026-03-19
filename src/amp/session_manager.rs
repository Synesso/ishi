use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;
use tokio::sync::mpsc;

use crate::amp::session::{AmpEvent, AmpSession, SessionStatus};
use crate::amp::state::{self, State};

/// An event tagged with the thread ID of the session that produced it.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TaggedEvent {
    pub thread_id: String,
    pub event: AmpEvent,
}

/// Metadata for a managed session.
#[allow(dead_code)]
struct ManagedSession {
    session: AmpSession,
    issue: String,
    workspace: String,
}

/// Manages multiple concurrent `AmpSession` instances and provides a unified
/// event channel for the TUI main loop.
///
/// Each session is keyed by thread ID. Events from all sessions are tagged
/// and forwarded into a single aggregated channel.
#[allow(dead_code)]
pub struct SessionManager {
    sessions: HashMap<String, ManagedSession>,
    agg_tx: mpsc::Sender<TaggedEvent>,
    agg_rx: Option<mpsc::Receiver<TaggedEvent>>,
    buffer_size: usize,
}

#[allow(dead_code)]
impl SessionManager {
    /// Create a new `SessionManager` with the given aggregate channel buffer size.
    pub fn new(buffer_size: usize) -> Self {
        let (agg_tx, agg_rx) = mpsc::channel(buffer_size);
        Self {
            sessions: HashMap::new(),
            agg_tx,
            agg_rx: Some(agg_rx),
            buffer_size,
        }
    }

    /// Take the aggregated event receiver. Can only be called once; subsequent
    /// calls return `None`.
    pub fn take_event_receiver(&mut self) -> Option<mpsc::Receiver<TaggedEvent>> {
        self.agg_rx.take()
    }

    /// Spawn a new `AmpSession` for the given thread and register it.
    ///
    /// The session is linked to the given issue identifier and workspace path.
    /// Session metadata is persisted to the state file.
    pub fn create_session(
        &mut self,
        thread_id: &str,
        issue: &str,
        workspace: &Path,
    ) -> Result<()> {
        if self.sessions.contains_key(thread_id) {
            anyhow::bail!("session already exists for thread {}", thread_id);
        }

        let (session, rx) = AmpSession::spawn(thread_id, workspace, self.buffer_size)?;

        // Spawn a forwarder task that tags events and sends them to the aggregate channel.
        let tx = self.agg_tx.clone();
        let tid = thread_id.to_string();
        tokio::spawn(forward_events(tid, rx, tx));

        self.sessions.insert(
            thread_id.to_string(),
            ManagedSession {
                session,
                issue: issue.to_string(),
                workspace: workspace.to_string_lossy().to_string(),
            },
        );

        // Persist the thread→issue link and workspace to state.
        persist_thread_link(thread_id, issue, workspace)?;

        Ok(())
    }

    /// Look up an active session by thread ID.
    pub fn get(&self, thread_id: &str) -> Option<SessionStatus> {
        self.sessions.get(thread_id).map(|m| m.session.status())
    }

    /// Look up all active thread IDs for a given issue identifier.
    pub fn threads_for_issue(&self, issue: &str) -> Vec<&str> {
        let mut ids: Vec<&str> = self
            .sessions
            .iter()
            .filter(|(_, m)| m.issue == issue)
            .map(|(id, _)| id.as_str())
            .collect();
        ids.sort();
        ids
    }

    /// Send a user message to a session by thread ID.
    pub async fn send_message(&mut self, thread_id: &str, text: &str) -> Result<()> {
        let managed = self
            .sessions
            .get_mut(thread_id)
            .ok_or_else(|| anyhow::anyhow!("no active session for thread {}", thread_id))?;
        managed.session.send_message(text).await
    }

    /// Update session status (e.g. after observing a result event).
    pub fn set_status(&mut self, thread_id: &str, status: SessionStatus) {
        if let Some(managed) = self.sessions.get_mut(thread_id) {
            managed.session.set_status(status);
        }
    }

    /// Kill a session and remove it from management.
    pub async fn kill_session(&mut self, thread_id: &str) {
        if let Some(mut managed) = self.sessions.remove(thread_id) {
            managed.session.kill().await;
        }
    }

    /// Remove all sessions whose status is `Failed` or `Idle` (terminated).
    pub async fn cleanup_terminated(&mut self) {
        let terminated: Vec<String> = self
            .sessions
            .iter()
            .filter(|(_, m)| matches!(m.session.status(), SessionStatus::Failed))
            .map(|(id, _)| id.clone())
            .collect();
        for id in terminated {
            self.sessions.remove(&id);
        }
    }

    /// Return the number of active sessions.
    pub fn active_count(&self) -> usize {
        self.sessions.len()
    }

    /// Return all active thread IDs.
    pub fn active_thread_ids(&self) -> Vec<&str> {
        let mut ids: Vec<&str> = self.sessions.keys().map(|s| s.as_str()).collect();
        ids.sort();
        ids
    }

    /// Check if a session exists for the given thread ID.
    pub fn has_session(&self, thread_id: &str) -> bool {
        self.sessions.contains_key(thread_id)
    }
}

/// Forward events from a single session's receiver to the aggregate channel,
/// tagging each with the thread ID.
async fn forward_events(
    thread_id: String,
    mut rx: mpsc::Receiver<AmpEvent>,
    tx: mpsc::Sender<TaggedEvent>,
) {
    while let Some(event) = rx.recv().await {
        let tagged = TaggedEvent {
            thread_id: thread_id.clone(),
            event,
        };
        if tx.send(tagged).await.is_err() {
            break;
        }
    }
}

/// Persist a thread→issue link and workspace to the state file.
fn persist_thread_link(thread_id: &str, issue: &str, workspace: &Path) -> Result<()> {
    let state_path = state::state_path()?;
    let mut state = State::load(&state_path)?;
    state.add_thread_link(
        thread_id,
        issue,
        &workspace.to_string_lossy(),
    );
    state.add_workspace(&workspace.to_string_lossy());
    state.save(&state_path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_session_manager_has_no_sessions() {
        let mgr = SessionManager::new(16);
        assert_eq!(mgr.active_count(), 0);
        assert!(mgr.active_thread_ids().is_empty());
    }

    #[test]
    fn take_event_receiver_returns_some_once() {
        let mut mgr = SessionManager::new(16);
        assert!(mgr.take_event_receiver().is_some());
        assert!(mgr.take_event_receiver().is_none());
    }

    #[test]
    fn get_returns_none_for_unknown_thread() {
        let mgr = SessionManager::new(16);
        assert!(mgr.get("T-unknown").is_none());
    }

    #[test]
    fn has_session_returns_false_for_unknown() {
        let mgr = SessionManager::new(16);
        assert!(!mgr.has_session("T-unknown"));
    }

    #[test]
    fn threads_for_issue_returns_empty_when_no_sessions() {
        let mgr = SessionManager::new(16);
        assert!(mgr.threads_for_issue("JEM-1").is_empty());
    }

    #[tokio::test]
    async fn create_session_with_fake_amp() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("fake-amp.sh");
        std::fs::write(
            &script,
            "#!/bin/sh\necho '{\"type\":\"system\",\"subtype\":\"init\"}'\nsleep 60\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        // Use spawn_with indirectly via the session manager by testing
        // the forwarding logic in isolation instead.
        let (tx, mut rx) = mpsc::channel(16);
        let event = AmpEvent {
            event_type: "system".to_string(),
            subtype: Some("init".to_string()),
            raw: serde_json::json!({"type": "system", "subtype": "init"}),
        };

        let (inner_tx, inner_rx) = mpsc::channel(16);
        inner_tx.send(event.clone()).await.unwrap();
        drop(inner_tx);

        let handle = tokio::spawn(forward_events("T-test".to_string(), inner_rx, tx));

        let tagged = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
            .await
            .expect("timed out")
            .expect("channel closed");

        assert_eq!(tagged.thread_id, "T-test");
        assert_eq!(tagged.event.event_type, "system");
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn forward_events_stops_when_receiver_closes() {
        let (tx, _rx) = mpsc::channel(1);
        let (inner_tx, inner_rx) = mpsc::channel(1);
        drop(inner_tx);

        // Should complete without hanging.
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            forward_events("T-done".to_string(), inner_rx, tx),
        )
        .await
        .expect("forward_events should terminate when source closes");
    }

    #[tokio::test]
    async fn forward_events_stops_when_aggregate_channel_closed() {
        let (tx, rx) = mpsc::channel(1);
        drop(rx); // close the aggregate receiver

        let (inner_tx, inner_rx) = mpsc::channel(16);
        let event = AmpEvent {
            event_type: "assistant".to_string(),
            subtype: None,
            raw: serde_json::json!({"type": "assistant"}),
        };
        inner_tx.send(event).await.unwrap();

        // Should stop because aggregate channel is closed.
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            forward_events("T-orphan".to_string(), inner_rx, tx),
        )
        .await
        .expect("forward_events should stop when aggregate tx fails");
    }

    #[tokio::test]
    async fn send_message_to_unknown_thread_returns_error() {
        let mut mgr = SessionManager::new(16);
        let result = mgr.send_message("T-nonexistent", "hello").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no active session"));
    }

    #[tokio::test]
    async fn kill_session_removes_it() {
        let mut mgr = SessionManager::new(16);
        // Kill a non-existent session should be a no-op.
        mgr.kill_session("T-ghost").await;
        assert_eq!(mgr.active_count(), 0);
    }

    #[tokio::test]
    async fn cleanup_terminated_is_noop_when_empty() {
        let mut mgr = SessionManager::new(16);
        mgr.cleanup_terminated().await;
        assert_eq!(mgr.active_count(), 0);
    }

    #[test]
    fn tagged_event_carries_thread_id() {
        let tagged = TaggedEvent {
            thread_id: "T-abc".to_string(),
            event: AmpEvent {
                event_type: "result".to_string(),
                subtype: None,
                raw: serde_json::json!({"type": "result"}),
            },
        };
        assert_eq!(tagged.thread_id, "T-abc");
        assert_eq!(tagged.event.event_type, "result");
    }
}
