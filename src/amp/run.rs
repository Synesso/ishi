use anyhow::Result;
use std::path::PathBuf;

/// Log-line prefix appended when a background run exits.
pub const EXIT_CODE_MARKER_PREFIX: &str = "__ISHI_EXIT_CODE__=";

/// Build the deterministic run identifier for a thread launch.
#[allow(dead_code)]
pub fn build_run_id(thread_id: &str, created_at_ms: u64) -> String {
    let safe_thread_id = sanitize_for_filename(thread_id);
    format!("run-{created_at_ms}-{safe_thread_id}")
}

/// Return a deterministic run log path for a run ID.
#[allow(dead_code)]
pub fn run_log_path(run_id: &str) -> Result<PathBuf> {
    let dir = run_logs_dir()?;
    Ok(dir.join(format!("{run_id}.log")))
}

/// Parse the latest exit-code marker from run log contents.
pub fn exit_code_from_log_contents(contents: &str) -> Option<i32> {
    contents.lines().rev().find_map(|line| {
        line.trim()
            .strip_prefix(EXIT_CODE_MARKER_PREFIX)
            .and_then(|value| value.parse::<i32>().ok())
    })
}

#[allow(dead_code)]
fn run_logs_dir() -> Result<PathBuf> {
    let base = dirs::state_dir()
        .or_else(dirs::cache_dir)
        .or_else(dirs::config_dir)
        .ok_or_else(|| anyhow::anyhow!("could not determine state/cache/config directory"))?;
    let dir = base.join("ishi").join("runs");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[allow(dead_code)]
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

#[allow(dead_code)]
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
    fn exit_code_from_log_contents_returns_latest_marker() {
        let contents = format!(
            "line 1\n{prefix}1\nline 2\n{prefix}0\n",
            prefix = EXIT_CODE_MARKER_PREFIX
        );
        assert_eq!(exit_code_from_log_contents(contents.as_str()), Some(0));
    }
}
