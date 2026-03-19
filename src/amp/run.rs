/// Log-line prefix appended when a background run exits.
pub const EXIT_CODE_MARKER_PREFIX: &str = "__ISHI_EXIT_CODE__=";

/// Parse the latest exit-code marker from run log contents.
pub fn exit_code_from_log_contents(contents: &str) -> Option<i32> {
    contents.lines().rev().find_map(|line| {
        line.trim()
            .strip_prefix(EXIT_CODE_MARKER_PREFIX)
            .and_then(|value| value.parse::<i32>().ok())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_code_from_log_contents_returns_latest_marker() {
        let contents = format!(
            "line 1\n{prefix}1\nline 2\n{prefix}0\n",
            prefix = EXIT_CODE_MARKER_PREFIX
        );
        assert_eq!(exit_code_from_log_contents(contents.as_str()), Some(0));
    }
}
