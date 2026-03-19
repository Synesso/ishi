use std::collections::HashMap;

/// A single display line from session output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputLine {
    pub kind: OutputKind,
    pub text: String,
}

/// The kind of output line, used for styling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputKind {
    /// User-sent instruction message.
    User,
}

/// Buffers parsed output lines per thread for display in the TUI.
///
/// Each thread accumulates its own output history. The buffer can be queried
/// for a specific thread's lines and survives navigation away and back.
#[derive(Debug, Default)]
pub struct SessionOutputBuffer {
    buffers: HashMap<String, Vec<OutputLine>>,
}

impl SessionOutputBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Return buffered output lines for a thread.
    pub fn lines_for(&self, thread_id: &str) -> &[OutputLine] {
        self.buffers.get(thread_id).map_or(&[], |v| v.as_slice())
    }

    /// Return the number of buffered lines for a thread.
    pub fn line_count(&self, thread_id: &str) -> usize {
        self.buffers.get(thread_id).map_or(0, |v| v.len())
    }

    /// Append a user-sent message to the buffer for display.
    pub fn push_user_message(&mut self, thread_id: &str, text: &str) {
        let lines: Vec<OutputLine> = text
            .lines()
            .map(|line| OutputLine {
                kind: OutputKind::User,
                text: format!("▶ {line}"),
            })
            .collect();
        if !lines.is_empty() {
            self.buffers
                .entry(thread_id.to_string())
                .or_default()
                .extend(lines);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_unknown_thread_returns_empty() {
        let buf = SessionOutputBuffer::new();
        assert!(buf.lines_for("T-nope").is_empty());
        assert_eq!(buf.line_count("T-nope"), 0);
    }

    #[test]
    fn push_user_message_adds_prefixed_lines() {
        let mut buf = SessionOutputBuffer::new();
        buf.push_user_message("T-1", "do the thing");
        assert_eq!(buf.line_count("T-1"), 1);
        assert_eq!(buf.lines_for("T-1")[0].kind, OutputKind::User);
        assert_eq!(buf.lines_for("T-1")[0].text, "▶ do the thing");
    }

    #[test]
    fn push_user_message_splits_multiline() {
        let mut buf = SessionOutputBuffer::new();
        buf.push_user_message("T-1", "line one\nline two");
        assert_eq!(buf.line_count("T-1"), 2);
        assert_eq!(buf.lines_for("T-1")[0].text, "▶ line one");
        assert_eq!(buf.lines_for("T-1")[1].text, "▶ line two");
    }

    #[test]
    fn push_user_message_empty_is_noop() {
        let mut buf = SessionOutputBuffer::new();
        buf.push_user_message("T-1", "");
        assert_eq!(buf.line_count("T-1"), 0);
    }
}
