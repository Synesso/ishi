use anyhow::Result;
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct RawThread {
    id: String,
    created: u64,
    title: Option<String>,
    #[serde(default)]
    messages: Vec<serde_json::Value>,
    #[serde(rename = "usageLedger")]
    usage_ledger: Option<UsageLedger>,
}

#[derive(Debug, Deserialize)]
struct UsageLedger {
    #[serde(default)]
    events: Vec<LedgerEvent>,
}

#[derive(Debug, Deserialize)]
struct LedgerEvent {
    timestamp: String,
}

/// Summary of an Amp thread for display in the TUI.
#[derive(Debug, Clone, PartialEq)]
pub struct ThreadSummary {
    pub id: String,
    pub title: String,
    pub message_count: usize,
    /// Milliseconds since Unix epoch of last activity (or creation time).
    pub last_activity_ms: u64,
}

impl ThreadSummary {
    pub fn relative_time(&self) -> String {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        format_relative_time(now_ms, self.last_activity_ms)
    }
}

pub fn format_relative_time(now_ms: u64, then_ms: u64) -> String {
    let diff_secs = now_ms.saturating_sub(then_ms) / 1000;

    if diff_secs < 60 {
        format!("{}s ago", diff_secs)
    } else if diff_secs < 3600 {
        format!("{}m ago", diff_secs / 60)
    } else if diff_secs < 86400 {
        format!("{}h ago", diff_secs / 3600)
    } else {
        format!("{}d ago", diff_secs / 86400)
    }
}

/// Returns the default directory where Amp stores thread files.
pub fn amp_threads_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("amp").join("threads"))
}

/// Read a thread file and produce a summary.
pub fn read_thread_summary(path: &Path) -> Result<ThreadSummary> {
    let contents = std::fs::read_to_string(path)?;
    parse_thread_summary(&contents)
}

/// Parse a thread JSON string into a summary.
pub fn parse_thread_summary(json: &str) -> Result<ThreadSummary> {
    let raw: RawThread = serde_json::from_str(json)?;

    let title = raw.title.unwrap_or_else(|| "Untitled thread".to_string());

    let message_count = raw.messages.len();

    // Last activity: latest usage ledger event timestamp, or creation time.
    let last_activity_ms = raw
        .usage_ledger
        .and_then(|ledger| {
            ledger
                .events
                .last()
                .and_then(|e| chrono_parse_to_ms(&e.timestamp))
        })
        .unwrap_or(raw.created);

    Ok(ThreadSummary {
        id: raw.id,
        title,
        message_count,
        last_activity_ms,
    })
}

/// Parse an ISO-8601 timestamp string to milliseconds since epoch.
/// Uses manual parsing to avoid adding a chrono dependency.
fn chrono_parse_to_ms(s: &str) -> Option<u64> {
    // Expected format: "2025-12-28T21:21:59.052Z"
    // We'll use a simple parse approach
    let s = s.trim_end_matches('Z');
    let (date_part, time_part) = s.split_once('T')?;

    let mut date_iter = date_part.split('-');
    let year: i64 = date_iter.next()?.parse().ok()?;
    let month: u32 = date_iter.next()?.parse().ok()?;
    let day: u32 = date_iter.next()?.parse().ok()?;

    let (time_main, millis) = if let Some((t, ms)) = time_part.split_once('.') {
        let ms_val: u64 = ms.parse().ok()?;
        (t, ms_val)
    } else {
        (time_part, 0u64)
    };

    let mut time_iter = time_main.split(':');
    let hour: u32 = time_iter.next()?.parse().ok()?;
    let min: u32 = time_iter.next()?.parse().ok()?;
    let sec: u32 = time_iter.next()?.parse().ok()?;

    // Days from epoch (1970-01-01) using a simplified calculation
    let days = days_from_epoch(year, month, day)?;
    let total_secs =
        (days as u64) * 86400 + (hour as u64) * 3600 + (min as u64) * 60 + (sec as u64);
    Some(total_secs * 1000 + millis)
}

fn days_from_epoch(year: i64, month: u32, day: u32) -> Option<i64> {
    // Adjust for months <= 2 (treat Jan/Feb as months 13/14 of previous year)
    let (y, m) = if month <= 2 {
        (year - 1, month + 12)
    } else {
        (year, month)
    };

    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u32;
    let doy = (153 * (m - 3) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe as i64 - 719468;
    Some(days)
}

/// Load thread summaries for a list of thread IDs from the given threads directory.
pub fn load_thread_summaries(threads_dir: &Path, thread_ids: &[String]) -> Vec<ThreadSummary> {
    thread_ids
        .iter()
        .filter_map(|id| {
            let path = threads_dir.join(format!("{}.json", id));
            read_thread_summary(&path).ok()
        })
        .collect()
}

/// Snapshot all thread IDs from a given directory by listing `*.json` filenames.
///
/// Returns a set of thread IDs (filenames without `.json` extension).
#[allow(dead_code)]
pub fn snapshot_thread_ids(dir: &Path) -> std::collections::HashSet<String> {
    let mut ids = std::collections::HashSet::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json")
                && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
            {
                ids.insert(stem.to_string());
            }
        }
    }
    ids
}

/// List all thread summaries from a given directory.
///
/// Scans `dir/*.json` and parses metadata from each file.
/// Files that fail to parse are silently skipped.
#[allow(dead_code)]
pub fn list_threads_in(dir: &Path) -> Result<Vec<ThreadSummary>> {
    let mut threads = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "json")
            && let Ok(summary) = read_thread_summary(&path)
        {
            threads.push(summary);
        }
    }
    threads.sort_by(|a, b| b.last_activity_ms.cmp(&a.last_activity_ms));
    Ok(threads)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_thread() {
        let json = r#"{
            "v": 0,
            "id": "T-abc-123",
            "created": 1700000000000,
            "messages": []
        }"#;
        let summary = parse_thread_summary(json).unwrap();
        assert_eq!(summary.id, "T-abc-123");
        assert_eq!(summary.title, "Untitled thread");
        assert_eq!(summary.message_count, 0);
        assert_eq!(summary.last_activity_ms, 1700000000000);
    }

    #[test]
    fn parse_thread_with_title_and_messages() {
        let json = r#"{
            "v": 0,
            "id": "T-def-456",
            "created": 1700000000000,
            "title": "Fix the bug",
            "messages": [{"role": "user"}, {"role": "assistant"}, {"role": "user"}]
        }"#;
        let summary = parse_thread_summary(json).unwrap();
        assert_eq!(summary.title, "Fix the bug");
        assert_eq!(summary.message_count, 3);
    }

    #[test]
    fn parse_thread_with_usage_ledger() {
        let json = r#"{
            "v": 0,
            "id": "T-ghi-789",
            "created": 1700000000000,
            "messages": [{"role": "user"}],
            "usageLedger": {
                "events": [
                    {"timestamp": "2025-12-28T21:17:52.900Z"},
                    {"timestamp": "2025-12-28T21:21:59.052Z"}
                ]
            }
        }"#;
        let summary = parse_thread_summary(json).unwrap();
        assert!(summary.last_activity_ms > 1700000000000);
        // 2025-12-28T21:21:59.052Z
        assert_eq!(summary.last_activity_ms, 1766956919052);
    }

    #[test]
    fn format_relative_time_seconds() {
        assert_eq!(format_relative_time(1000_000, 1000_000), "0s ago");
        assert_eq!(format_relative_time(1030_000, 1000_000), "30s ago");
    }

    #[test]
    fn format_relative_time_minutes() {
        assert_eq!(format_relative_time(1000_000 + 120_000, 1000_000), "2m ago");
        assert_eq!(format_relative_time(1000_000 + 420_000, 1000_000), "7m ago");
    }

    #[test]
    fn format_relative_time_hours() {
        let two_hours = 2 * 3600 * 1000;
        assert_eq!(
            format_relative_time(1000_000 + two_hours, 1000_000),
            "2h ago"
        );
    }

    #[test]
    fn format_relative_time_days() {
        let three_days = 3 * 86400 * 1000;
        assert_eq!(
            format_relative_time(1000_000 + three_days, 1000_000),
            "3d ago"
        );
    }

    #[test]
    fn chrono_parse_known_timestamp() {
        let ms = chrono_parse_to_ms("2025-12-28T21:21:59.052Z").unwrap();
        assert_eq!(ms, 1766956919052);
    }

    #[test]
    fn chrono_parse_no_millis() {
        let ms = chrono_parse_to_ms("2025-01-01T00:00:00Z").unwrap();
        // 2025-01-01 = day 20089 from epoch
        assert_eq!(ms, 1735689600000);
    }

    #[test]
    fn load_thread_summaries_from_dir() {
        let dir = tempfile::tempdir().unwrap();
        let thread_json = r#"{
            "v": 0,
            "id": "T-test-1",
            "created": 1700000000000,
            "title": "Test thread",
            "messages": [{"role": "user"}]
        }"#;
        std::fs::write(dir.path().join("T-test-1.json"), thread_json).unwrap();

        let summaries = load_thread_summaries(dir.path(), &["T-test-1".to_string()]);
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].title, "Test thread");
    }

    #[test]
    fn load_thread_summaries_skips_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        let summaries = load_thread_summaries(dir.path(), &["T-nonexistent".to_string()]);
        assert!(summaries.is_empty());
    }

    #[test]
    fn list_threads_in_returns_sorted_by_last_activity_desc() {
        let dir = tempfile::tempdir().unwrap();
        let old_thread = r#"{
            "v": 0,
            "id": "T-old",
            "created": 1600000000000,
            "title": "Old thread",
            "messages": [{"role": "user"}]
        }"#;
        let new_thread = r#"{
            "v": 0,
            "id": "T-new",
            "created": 1700000000000,
            "title": "New thread",
            "messages": [{"role": "user"}, {"role": "assistant"}]
        }"#;
        let mid_thread = r#"{
            "v": 0,
            "id": "T-mid",
            "created": 1650000000000,
            "title": "Mid thread",
            "messages": []
        }"#;
        std::fs::write(dir.path().join("T-old.json"), old_thread).unwrap();
        std::fs::write(dir.path().join("T-new.json"), new_thread).unwrap();
        std::fs::write(dir.path().join("T-mid.json"), mid_thread).unwrap();

        let threads = list_threads_in(dir.path()).unwrap();
        assert_eq!(threads.len(), 3);
        assert_eq!(threads[0].id, "T-new");
        assert_eq!(threads[1].id, "T-mid");
        assert_eq!(threads[2].id, "T-old");
    }

    #[test]
    fn list_threads_in_skips_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let valid = r#"{
            "v": 0,
            "id": "T-valid",
            "created": 1700000000000,
            "messages": []
        }"#;
        std::fs::write(dir.path().join("T-valid.json"), valid).unwrap();
        std::fs::write(dir.path().join("T-bad.json"), "not valid json").unwrap();

        let threads = list_threads_in(dir.path()).unwrap();
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].id, "T-valid");
    }

    #[test]
    fn list_threads_in_ignores_non_json_files() {
        let dir = tempfile::tempdir().unwrap();
        let valid = r#"{
            "v": 0,
            "id": "T-only",
            "created": 1700000000000,
            "messages": []
        }"#;
        std::fs::write(dir.path().join("T-only.json"), valid).unwrap();
        std::fs::write(dir.path().join("notes.txt"), "some notes").unwrap();

        let threads = list_threads_in(dir.path()).unwrap();
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].id, "T-only");
    }

    #[test]
    fn list_threads_in_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let threads = list_threads_in(dir.path()).unwrap();
        assert!(threads.is_empty());
    }

    #[test]
    fn list_threads_in_nonexistent_dir_returns_error() {
        let result = list_threads_in(Path::new("/nonexistent/dir"));
        assert!(result.is_err());
    }

    #[test]
    fn snapshot_thread_ids_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let ids = snapshot_thread_ids(dir.path());
        assert!(ids.is_empty());
    }

    #[test]
    fn snapshot_thread_ids_finds_json_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("T-abc.json"), "{}").unwrap();
        std::fs::write(dir.path().join("T-def.json"), "{}").unwrap();
        std::fs::write(dir.path().join("notes.txt"), "not a thread").unwrap();

        let ids = snapshot_thread_ids(dir.path());
        assert_eq!(ids.len(), 2);
        assert!(ids.contains("T-abc"));
        assert!(ids.contains("T-def"));
    }

    #[test]
    fn snapshot_thread_ids_nonexistent_dir() {
        let ids = snapshot_thread_ids(Path::new("/nonexistent/dir"));
        assert!(ids.is_empty());
    }

    #[test]
    fn snapshot_thread_ids_diff_detects_new_thread() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("T-existing.json"), "{}").unwrap();

        let before = snapshot_thread_ids(dir.path());
        assert_eq!(before.len(), 1);

        std::fs::write(dir.path().join("T-new.json"), "{}").unwrap();

        let after = snapshot_thread_ids(dir.path());
        let new_ids: Vec<&String> = after.difference(&before).collect();
        assert_eq!(new_ids.len(), 1);
        assert_eq!(*new_ids[0], "T-new");
    }

    #[test]
    fn snapshot_thread_ids_diff_empty_when_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("T-existing.json"), "{}").unwrap();

        let before = snapshot_thread_ids(dir.path());
        let after = snapshot_thread_ids(dir.path());
        let new_ids: Vec<&String> = after.difference(&before).collect();
        assert!(new_ids.is_empty());
    }

    #[test]
    fn list_threads_in_with_usage_ledger_sorts_by_last_event() {
        let dir = tempfile::tempdir().unwrap();
        // Thread with old creation but recent usage ledger event
        let active_thread = r#"{
            "v": 0,
            "id": "T-active",
            "created": 1600000000000,
            "title": "Active old thread",
            "messages": [{"role": "user"}],
            "usageLedger": {
                "events": [
                    {"timestamp": "2025-12-28T21:21:59.052Z"}
                ]
            }
        }"#;
        // Thread with recent creation but no ledger
        let recent_thread = r#"{
            "v": 0,
            "id": "T-recent",
            "created": 1700000000000,
            "title": "Recent thread",
            "messages": []
        }"#;
        std::fs::write(dir.path().join("T-active.json"), active_thread).unwrap();
        std::fs::write(dir.path().join("T-recent.json"), recent_thread).unwrap();

        let threads = list_threads_in(dir.path()).unwrap();
        assert_eq!(threads.len(), 2);
        // T-active has last_activity from ledger (2025-12-28) > T-recent creation (1700000000000)
        assert_eq!(threads[0].id, "T-active");
        assert_eq!(threads[1].id, "T-recent");
    }

    #[test]
    fn thread_summary_relative_time_is_nonempty() {
        let summary = ThreadSummary {
            id: "T-test".to_string(),
            title: "Test".to_string(),
            message_count: 1,
            last_activity_ms: 1700000000000,
        };
        let relative = summary.relative_time();
        assert!(!relative.is_empty());
        assert!(relative.ends_with(" ago"));
    }
}
