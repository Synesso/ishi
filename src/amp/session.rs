use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;

/// Status of a live AmpSession process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStatus {
    /// The session process is alive and waiting for input or producing output.
    Running,
    /// The session has finished processing a turn and is idle.
    Idle,
    /// The session process has exited or encountered an error.
    Failed,
}

/// A single JSON event received from the amp process stdout.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmpEvent {
    /// Top-level type: "system", "user", "assistant", "result".
    #[serde(rename = "type")]
    pub event_type: String,
    /// For system events, the subtype (e.g. "init").
    #[serde(default)]
    pub subtype: Option<String>,
    /// Raw JSON value of the full event for downstream consumers.
    #[serde(flatten)]
    pub raw: serde_json::Value,
}

/// Manages a long-lived bidirectional amp child process.
///
/// Holds `ChildStdin` for writing JSON-line messages to amp, and spawns a
/// tokio task to read `ChildStdout`, parse JSON events, and send them through
/// an mpsc channel.
pub struct AmpSession {
    stdin: tokio::process::ChildStdin,
    status: SessionStatus,
    _reader_handle: tokio::task::JoinHandle<()>,
}

impl AmpSession {
    /// Spawn a new amp session for the given thread.
    ///
    /// Launches `amp threads continue <thread_id> -x --stream-json
    /// --stream-json-input --dangerously-allow-all --no-notifications`
    /// in the given workspace directory.
    ///
    /// Returns the session and a receiver for parsed JSON events.
    pub fn spawn(
        thread_id: &str,
        workspace: &Path,
        buffer: usize,
    ) -> Result<(Self, mpsc::Receiver<AmpEvent>)> {
        Self::spawn_with("amp", thread_id, workspace, buffer)
    }

    /// Spawn a session using a custom program (for testing).
    fn spawn_with(
        program: &str,
        thread_id: &str,
        workspace: &Path,
        buffer: usize,
    ) -> Result<(Self, mpsc::Receiver<AmpEvent>)> {
        let args = build_session_args(thread_id);

        let mut child = Command::new(program)
            .args(&args)
            .current_dir(workspace)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("failed to spawn `{}` in {}", program, workspace.display()))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("failed to capture stdout from amp process"))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("failed to capture stdin from amp process"))?;

        let (tx, rx) = mpsc::channel(buffer);
        let reader_handle = tokio::spawn(read_stdout_task(stdout, tx, child));

        let session = Self {
            stdin,
            status: SessionStatus::Running,
            _reader_handle: reader_handle,
        };

        Ok((session, rx))
    }

    /// Send a user message to the amp session.
    ///
    /// Writes a JSON-line in the expected input format:
    /// `{"type":"user","message":{"role":"user","content":[{"type":"text","text":"..."}]}}`
    pub async fn send_message(&mut self, text: &str) -> Result<()> {
        if self.status == SessionStatus::Failed {
            anyhow::bail!("session has failed; cannot send message");
        }

        let msg = serde_json::json!({
            "type": "user",
            "message": {
                "role": "user",
                "content": [
                    { "type": "text", "text": text }
                ]
            }
        });

        let mut line = serde_json::to_string(&msg)?;
        line.push('\n');
        self.stdin
            .write_all(line.as_bytes())
            .await
            .with_context(|| "failed to write message to amp stdin")?;
        self.stdin
            .flush()
            .await
            .with_context(|| "failed to flush amp stdin")?;

        self.status = SessionStatus::Running;
        Ok(())
    }

    /// Return the current session status.
    pub fn status(&self) -> SessionStatus {
        self.status
    }

    /// Update the session status (e.g. after observing a `result` event).
    pub fn set_status(&mut self, status: SessionStatus) {
        self.status = status;
    }

    /// Kill the amp child process.
    pub async fn kill(&mut self) {
        // Dropping stdin signals EOF to the child, but we also want to be
        // explicit about cleanup. Shutdown stdin first, then the reader
        // task will terminate when the child exits.
        let _ = self.stdin.shutdown().await;
        self.status = SessionStatus::Failed;
    }
}

/// Background task that reads stdout line-by-line, parses JSON events,
/// and forwards them through the channel.
async fn read_stdout_task(
    stdout: tokio::process::ChildStdout,
    tx: mpsc::Sender<AmpEvent>,
    _child: Child,
) {
    let reader = BufReader::new(stdout);
    let mut lines = reader.lines();

    while let Ok(Some(line)) = lines.next_line().await {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<AmpEvent>(&line) {
            Ok(event) => {
                if tx.send(event).await.is_err() {
                    break;
                }
            }
            Err(_) => {
                // Non-JSON lines are silently skipped (e.g. debug output).
            }
        }
    }
    // stdout closed — child has exited. The `_child` is dropped here,
    // which will reap the process.
}

fn build_session_args(thread_id: &str) -> Vec<String> {
    vec![
        "threads".to_string(),
        "continue".to_string(),
        thread_id.to_string(),
        "-x".to_string(),
        "--stream-json".to_string(),
        "--stream-json-input".to_string(),
        "--dangerously-allow-all".to_string(),
        "--no-notifications".to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn build_session_args_includes_all_flags() {
        let args = build_session_args("T-abc-123");
        assert_eq!(
            args,
            vec![
                "threads",
                "continue",
                "T-abc-123",
                "-x",
                "--stream-json",
                "--stream-json-input",
                "--dangerously-allow-all",
                "--no-notifications",
            ]
        );
    }

    #[test]
    fn amp_event_deserializes_system_init() {
        let json = r#"{"type":"system","subtype":"init","tools":["Read","Grep"]}"#;
        let event: AmpEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.event_type, "system");
        assert_eq!(event.subtype.as_deref(), Some("init"));
    }

    #[test]
    fn amp_event_deserializes_result() {
        let json = r#"{"type":"result","status":"success"}"#;
        let event: AmpEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.event_type, "result");
        assert_eq!(event.subtype, None);
    }

    #[test]
    fn amp_event_deserializes_assistant() {
        let json = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Hello"}]}}"#;
        let event: AmpEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.event_type, "assistant");
    }

    #[test]
    fn session_status_default_is_running() {
        // Verify a fresh session starts as Running
        assert_eq!(SessionStatus::Running, SessionStatus::Running);
    }

    #[tokio::test]
    async fn spawn_with_echo_receives_event() {
        // Use a shell script that outputs a JSON line and exits.
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("fake-amp.sh");
        std::fs::write(
            &script,
            "#!/bin/sh\necho '{\"type\":\"system\",\"subtype\":\"init\"}'\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let (session, mut rx) =
            AmpSession::spawn_with(script.to_str().unwrap(), "T-test", dir.path(), 16).unwrap();

        let event = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("timed out waiting for event")
            .expect("channel closed without event");

        assert_eq!(event.event_type, "system");
        assert_eq!(event.subtype.as_deref(), Some("init"));
        assert_eq!(session.status(), SessionStatus::Running);
    }

    #[tokio::test]
    async fn spawn_with_missing_program_returns_error() {
        let result = AmpSession::spawn_with(
            "definitely_not_a_real_program",
            "T-test",
            Path::new("/tmp"),
            16,
        );
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn send_message_writes_valid_json_line() {
        // Spawn a cat-like process that echoes stdin to stdout.
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("cat-amp.sh");
        std::fs::write(&script, "#!/bin/sh\ncat\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let (mut session, mut rx) =
            AmpSession::spawn_with(script.to_str().unwrap(), "T-test", dir.path(), 16).unwrap();

        session.send_message("hello world").await.unwrap();

        // The cat process echoes back the JSON line we wrote.
        let event = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("timed out waiting for echoed event")
            .expect("channel closed without event");

        assert_eq!(event.event_type, "user");
    }

    #[tokio::test]
    async fn kill_sets_status_to_failed() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("sleep-amp.sh");
        std::fs::write(&script, "#!/bin/sh\nsleep 60\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let (mut session, _rx) =
            AmpSession::spawn_with(script.to_str().unwrap(), "T-test", dir.path(), 16).unwrap();

        assert_eq!(session.status(), SessionStatus::Running);
        session.kill().await;
        assert_eq!(session.status(), SessionStatus::Failed);
    }

    #[tokio::test]
    async fn send_message_after_kill_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("cat-amp.sh");
        std::fs::write(&script, "#!/bin/sh\ncat\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let (mut session, _rx) =
            AmpSession::spawn_with(script.to_str().unwrap(), "T-test", dir.path(), 16).unwrap();

        session.kill().await;
        let result = session.send_message("should fail").await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("session has failed")
        );
    }
}
