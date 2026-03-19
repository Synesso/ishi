use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

use crate::amp::run::exit_code_from_log_contents;
use crate::amp::state::{SessionRun, SessionRunStatus, State};

/// Reconcile all persisted session runs and save the state file if any run
/// metadata changed.
pub fn reconcile_state_file(path: &Path) -> Result<usize> {
    let mut state = State::load(path)?;
    let updated = reconcile_state(&mut state)?;
    if updated > 0 {
        state.save(path)?;
    }
    Ok(updated)
}

/// Reconcile session run statuses in-memory.
///
/// Returns the number of run records that were updated.
pub fn reconcile_state(state: &mut State) -> Result<usize> {
    reconcile_state_with(state, now_ms(), is_process_running, exit_code_for_run)
}

fn reconcile_state_with<P, E>(
    state: &mut State,
    now_ms: u64,
    mut process_is_running: P,
    mut exit_code_for_run: E,
) -> Result<usize>
where
    P: FnMut(u32) -> Result<bool>,
    E: FnMut(&SessionRun) -> Option<i32>,
{
    let mut updated = 0;
    for run in state.session_runs.values_mut() {
        if reconcile_run(run, now_ms, &mut process_is_running, &mut exit_code_for_run)? {
            updated += 1;
        }
    }
    Ok(updated)
}

fn reconcile_run<P, E>(
    run: &mut SessionRun,
    now_ms: u64,
    process_is_running: &mut P,
    exit_code_for_run: &mut E,
) -> Result<bool>
where
    P: FnMut(u32) -> Result<bool>,
    E: FnMut(&SessionRun) -> Option<i32>,
{
    let previous_status = run.status;
    let process_alive = match run.pid {
        Some(pid) => Some(process_is_running(pid)?),
        None => None,
    };
    let exit_code = if process_alive == Some(true) {
        None
    } else {
        exit_code_for_run(run)
    };

    let desired = desired_status(run.status, run.pid, process_alive, exit_code);
    let mut changed = false;

    if run.status != desired {
        run.status = desired;
        changed = true;
    }

    match run.status {
        SessionRunStatus::Pending => {
            if run.finished_at_ms.is_some() {
                run.finished_at_ms = None;
                changed = true;
            }
        }
        SessionRunStatus::Running => {
            if run.started_at_ms.is_none() {
                run.started_at_ms = Some(now_ms);
                changed = true;
            }
            if run.finished_at_ms.is_some() {
                run.finished_at_ms = None;
                changed = true;
            }
        }
        SessionRunStatus::Completed | SessionRunStatus::Failed | SessionRunStatus::Stale => {
            if run.pid.is_some() {
                run.pid = None;
                changed = true;
            }
            let should_set_finished_at = run.finished_at_ms.is_none()
                || (run.status != previous_status
                    && matches!(
                        run.status,
                        SessionRunStatus::Completed | SessionRunStatus::Failed
                    ));
            if should_set_finished_at {
                run.finished_at_ms = Some(now_ms);
                changed = true;
            }
        }
    }

    if changed {
        run.updated_at_ms = now_ms;
    }

    Ok(changed)
}

fn desired_status(
    current: SessionRunStatus,
    pid: Option<u32>,
    process_alive: Option<bool>,
    exit_code: Option<i32>,
) -> SessionRunStatus {
    if process_alive == Some(true) {
        return SessionRunStatus::Running;
    }

    if let Some(code) = exit_code {
        return if code == 0 {
            SessionRunStatus::Completed
        } else {
            SessionRunStatus::Failed
        };
    }

    match current {
        SessionRunStatus::Completed | SessionRunStatus::Failed => current,
        SessionRunStatus::Pending if pid.is_none() => SessionRunStatus::Pending,
        _ => SessionRunStatus::Stale,
    }
}

fn is_process_running(pid: u32) -> Result<bool> {
    let output = Command::new("ps")
        .arg("-p")
        .arg(pid.to_string())
        .arg("-o")
        .arg("pid=")
        .output()
        .with_context(|| format!("failed to check process liveness for pid {}", pid))?;

    if !output.status.success() {
        return Ok(false);
    }

    Ok(!String::from_utf8_lossy(&output.stdout).trim().is_empty())
}

fn exit_code_for_run(run: &SessionRun) -> Option<i32> {
    let path = run.log_path.as_deref()?;
    let contents = std::fs::read_to_string(path).ok()?;
    exit_code_from_log_contents(&contents)
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

    fn sample_run(status: SessionRunStatus) -> SessionRun {
        SessionRun {
            thread_id: "T-abc".to_string(),
            issue: "JEM-101".to_string(),
            workspace: "/tmp/workspace".to_string(),
            pid: None,
            status,
            log_path: None,
            created_at_ms: 10,
            updated_at_ms: 10,
            started_at_ms: None,
            finished_at_ms: None,
        }
    }

    #[test]
    fn pending_without_pid_stays_pending() {
        let mut state = State::default();
        state.add_session_run("run-1", sample_run(SessionRunStatus::Pending));

        let updated = reconcile_state_with(&mut state, 100, |_| Ok(false), |_| None).unwrap();

        assert_eq!(updated, 0);
        assert_eq!(
            state.session_runs.get("run-1").map(|r| r.status),
            Some(SessionRunStatus::Pending)
        );
    }

    #[test]
    fn pending_with_live_pid_becomes_running() {
        let mut state = State::default();
        let mut run = sample_run(SessionRunStatus::Pending);
        run.pid = Some(123);
        state.add_session_run("run-1", run);

        let updated =
            reconcile_state_with(&mut state, 100, |pid| Ok(pid == 123), |_| None).unwrap();

        assert_eq!(updated, 1);
        let run = state.session_runs.get("run-1").unwrap();
        assert_eq!(run.status, SessionRunStatus::Running);
        assert_eq!(run.started_at_ms, Some(100));
        assert_eq!(run.finished_at_ms, None);
        assert_eq!(run.updated_at_ms, 100);
    }

    #[test]
    fn running_with_live_pid_does_not_consult_exit_marker() {
        let mut state = State::default();
        let mut run = sample_run(SessionRunStatus::Running);
        run.pid = Some(123);
        run.started_at_ms = Some(10);
        state.add_session_run("run-1", run);

        let updated = reconcile_state_with(
            &mut state,
            100,
            |pid| Ok(pid == 123),
            |_| panic!("exit marker should not be read while pid is alive"),
        )
        .unwrap();

        assert_eq!(updated, 0);
        assert_eq!(
            state.session_runs.get("run-1").map(|r| r.status),
            Some(SessionRunStatus::Running)
        );
    }

    #[test]
    fn running_with_dead_pid_and_success_exit_becomes_completed() {
        let mut state = State::default();
        let mut run = sample_run(SessionRunStatus::Running);
        run.pid = Some(456);
        run.started_at_ms = Some(50);
        state.add_session_run("run-1", run);

        let updated = reconcile_state_with(&mut state, 200, |_| Ok(false), |_| Some(0)).unwrap();

        assert_eq!(updated, 1);
        let run = state.session_runs.get("run-1").unwrap();
        assert_eq!(run.status, SessionRunStatus::Completed);
        assert_eq!(run.pid, None);
        assert_eq!(run.started_at_ms, Some(50));
        assert_eq!(run.finished_at_ms, Some(200));
    }

    #[test]
    fn running_with_dead_pid_and_non_zero_exit_becomes_failed() {
        let mut state = State::default();
        let mut run = sample_run(SessionRunStatus::Running);
        run.pid = Some(789);
        state.add_session_run("run-1", run);

        let updated = reconcile_state_with(&mut state, 300, |_| Ok(false), |_| Some(17)).unwrap();

        assert_eq!(updated, 1);
        let run = state.session_runs.get("run-1").unwrap();
        assert_eq!(run.status, SessionRunStatus::Failed);
        assert_eq!(run.pid, None);
        assert_eq!(run.finished_at_ms, Some(300));
    }

    #[test]
    fn running_with_dead_pid_and_no_exit_marker_becomes_stale() {
        let mut state = State::default();
        let mut run = sample_run(SessionRunStatus::Running);
        run.pid = Some(999);
        state.add_session_run("run-1", run);

        let updated = reconcile_state_with(&mut state, 400, |_| Ok(false), |_| None).unwrap();

        assert_eq!(updated, 1);
        let run = state.session_runs.get("run-1").unwrap();
        assert_eq!(run.status, SessionRunStatus::Stale);
        assert_eq!(run.pid, None);
        assert_eq!(run.finished_at_ms, Some(400));
    }

    #[test]
    fn stale_with_later_exit_marker_becomes_terminal() {
        let mut state = State::default();
        state.add_session_run("run-1", sample_run(SessionRunStatus::Stale));

        let updated = reconcile_state_with(&mut state, 500, |_| Ok(false), |_| Some(0)).unwrap();

        assert_eq!(updated, 1);
        let run = state.session_runs.get("run-1").unwrap();
        assert_eq!(run.status, SessionRunStatus::Completed);
        assert_eq!(run.finished_at_ms, Some(500));
    }

    #[test]
    fn completed_without_new_signal_stays_completed() {
        let mut state = State::default();
        let mut run = sample_run(SessionRunStatus::Completed);
        run.finished_at_ms = Some(150);
        run.updated_at_ms = 150;
        state.add_session_run("run-1", run);

        let updated = reconcile_state_with(&mut state, 500, |_| Ok(false), |_| None).unwrap();

        assert_eq!(updated, 0);
        let run = state.session_runs.get("run-1").unwrap();
        assert_eq!(run.status, SessionRunStatus::Completed);
        assert_eq!(run.finished_at_ms, Some(150));
        assert_eq!(run.updated_at_ms, 150);
    }

    #[test]
    fn reconcile_state_returns_error_when_process_check_fails() {
        let mut state = State::default();
        let mut run = sample_run(SessionRunStatus::Running);
        run.pid = Some(123);
        state.add_session_run("run-1", run);

        let result = reconcile_state_with(
            &mut state,
            500,
            |_| Err(anyhow::anyhow!("ps failed")),
            |_| None,
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("ps failed"));
    }

    #[test]
    fn reconcile_state_file_persists_updates() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("state.toml");
        let log_path = dir.path().join("run.log");
        std::fs::write(
            &log_path,
            format!("{}0\n", crate::amp::run::EXIT_CODE_MARKER_PREFIX),
        )
        .unwrap();

        let mut state = State::default();
        let mut run = sample_run(SessionRunStatus::Running);
        run.log_path = Some(log_path.to_string_lossy().to_string());
        state.add_session_run("run-1", run);
        state.save(&state_path).unwrap();

        let updated = reconcile_state_file(&state_path).unwrap();
        assert_eq!(updated, 1);

        let reloaded = State::load(&state_path).unwrap();
        assert_eq!(
            reloaded.session_runs.get("run-1").map(|r| r.status),
            Some(SessionRunStatus::Completed)
        );
    }
}
