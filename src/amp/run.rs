use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::amp::state::{SessionRun, SessionRunStatus};

/// Result of launching a background Amp run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchedRun {
    pub run_id: String,
    pub run: SessionRun,
}

/// Launch `amp threads continue` in the background using non-interactive mode.
///
/// The spawned process writes both stdout and stderr to a deterministic log file
/// and returns metadata suitable for immediate persistence in state.
pub fn launch_thread_continue_background(
    thread_id: &str,
    issue_identifier: &str,
    workspace: &Path,
    prompt: &str,
) -> Result<LaunchedRun> {
    let now_ms = now_ms();
    let run_id = build_run_id(thread_id, now_ms);
    let log_path = run_log_path(&run_id)?;
    let args = build_continue_args(thread_id, prompt);
    let pid = spawn_background_command_to_log("amp", &args, workspace, &log_path)?;

    let run = SessionRun {
        thread_id: thread_id.to_string(),
        issue: issue_identifier.to_string(),
        workspace: workspace.to_string_lossy().to_string(),
        pid: Some(pid),
        status: SessionRunStatus::Running,
        log_path: Some(log_path.to_string_lossy().to_string()),
        created_at_ms: now_ms,
        updated_at_ms: now_ms,
        started_at_ms: Some(now_ms),
        finished_at_ms: None,
    };

    Ok(LaunchedRun { run_id, run })
}

/// Build the deterministic run identifier for a thread launch.
pub fn build_run_id(thread_id: &str, created_at_ms: u64) -> String {
    let safe_thread_id = sanitize_for_filename(thread_id);
    format!("run-{created_at_ms}-{safe_thread_id}")
}

/// Return a deterministic run log path for a run ID.
pub fn run_log_path(run_id: &str) -> Result<PathBuf> {
    let dir = run_logs_dir()?;
    Ok(dir.join(format!("{run_id}.log")))
}

fn build_continue_args(thread_id: &str, prompt: &str) -> Vec<String> {
    vec![
        "threads".to_string(),
        "continue".to_string(),
        thread_id.to_string(),
        "-x".to_string(),
        prompt.to_string(),
        "--stream-json".to_string(),
    ]
}

/// Spawn a background process and stream its stdout/stderr into `log_path`.
///
/// Returns the spawned process ID.
pub fn spawn_background_command_to_log(
    program: &str,
    args: &[String],
    working_dir: &Path,
    log_path: &Path,
) -> Result<u32> {
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .with_context(|| format!("failed to open log file at {}", log_path.display()))?;
    let log_file_err = log_file
        .try_clone()
        .with_context(|| format!("failed to clone log file at {}", log_path.display()))?;

    let child = Command::new(program)
        .args(args)
        .current_dir(working_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(log_file_err))
        .spawn()
        .with_context(|| {
            format!(
                "failed to launch `{}` in {}",
                program,
                working_dir.display()
            )
        })?;

    Ok(child.id())
}

fn run_logs_dir() -> Result<PathBuf> {
    let base = dirs::state_dir()
        .or_else(dirs::cache_dir)
        .or_else(dirs::config_dir)
        .ok_or_else(|| anyhow::anyhow!("could not determine state/cache/config directory"))?;
    let dir = base.join("ishi").join("runs");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn sanitize_for_filename(input: &str) -> String {
    input
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_run_id_is_deterministic() {
        let a = build_run_id("T-abc-123", 1_710_000_000_000);
        let b = build_run_id("T-abc-123", 1_710_000_000_000);
        assert_eq!(a, b);
        assert_eq!(a, "run-1710000000000-T-abc-123");
    }

    #[test]
    fn build_run_id_sanitizes_thread_id_for_filenames() {
        let run_id = build_run_id("T/abc:123", 42);
        assert_eq!(run_id, "run-42-T_abc_123");
    }

    #[test]
    fn run_log_path_uses_run_id_filename() {
        let run_id = "run-123-T-abc";
        let path = run_log_path(run_id).unwrap();
        let filename = path.file_name().and_then(|s| s.to_str());
        assert_eq!(filename, Some("run-123-T-abc.log"));
    }

    #[test]
    fn spawn_background_command_to_log_returns_pid_and_writes_output() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("run.log");
        let args = vec!["-c".to_string(), "echo hello-from-background".to_string()];

        let pid = spawn_background_command_to_log("sh", &args, dir.path(), &log_path).unwrap();
        assert!(pid > 0);

        // Process is launched in background. Poll briefly for output.
        let mut output = String::new();
        for _ in 0..20 {
            output = std::fs::read_to_string(&log_path).unwrap_or_default();
            if output.contains("hello-from-background") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        assert!(output.contains("hello-from-background"));
    }

    #[test]
    fn spawn_background_command_to_log_errors_when_program_missing() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("run.log");

        let result = spawn_background_command_to_log(
            "definitely_not_a_real_program",
            &[],
            dir.path(),
            &log_path,
        );

        assert!(result.is_err());
    }
}
