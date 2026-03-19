use std::collections::HashMap;

use crate::amp::session::AmpEvent;

/// A single display line from session output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputLine {
    pub kind: OutputKind,
    pub text: String,
}

/// The kind of output line, used for styling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputKind {
    /// Assistant text response.
    Assistant,
    /// Tool use activity (tool name / status).
    Tool,
    /// Result: success or error.
    ResultSuccess,
    ResultError,
    /// System / init events.
    System,
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

    /// Append parsed output lines for an event from a specific thread.
    pub fn push_event(&mut self, thread_id: &str, event: &AmpEvent) {
        let lines = parse_event(event);
        if !lines.is_empty() {
            self.buffers
                .entry(thread_id.to_string())
                .or_default()
                .extend(lines);
        }
    }

    /// Return buffered output lines for a thread.
    pub fn lines_for(&self, thread_id: &str) -> &[OutputLine] {
        self.buffers.get(thread_id).map_or(&[], |v| v.as_slice())
    }

    /// Return the number of buffered lines for a thread.
    pub fn line_count(&self, thread_id: &str) -> usize {
        self.buffers.get(thread_id).map_or(0, |v| v.len())
    }

    /// Clear buffered output for a thread.
    pub fn clear_thread(&mut self, thread_id: &str) {
        self.buffers.remove(thread_id);
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

/// Parse an `AmpEvent` into zero or more display lines.
fn parse_event(event: &AmpEvent) -> Vec<OutputLine> {
    let mut lines = Vec::new();

    match event.event_type.as_str() {
        "assistant" => {
            // Extract text content from assistant messages.
            if let Some(message) = event.raw.get("message")
                && let Some(content) = message.get("content")
                && let Some(arr) = content.as_array()
            {
                for item in arr {
                    if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                        if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                            for line in text.lines() {
                                lines.push(OutputLine {
                                    kind: OutputKind::Assistant,
                                    text: line.to_string(),
                                });
                            }
                        }
                    } else if item.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                        let tool_name = item
                            .get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or("unknown");
                        lines.push(OutputLine {
                            kind: OutputKind::Tool,
                            text: format!("⚡ {tool_name}"),
                        });
                    }
                }
            }
        }
        "result" => {
            let status = event
                .raw
                .get("status")
                .and_then(|s| s.as_str())
                .unwrap_or("unknown");
            let kind = if status == "success" {
                OutputKind::ResultSuccess
            } else {
                OutputKind::ResultError
            };
            lines.push(OutputLine {
                kind,
                text: format!("── result: {status} ──"),
            });
        }
        "system" => {
            if let Some(subtype) = &event.subtype {
                lines.push(OutputLine {
                    kind: OutputKind::System,
                    text: format!("system: {subtype}"),
                });
            }
        }
        _ => {}
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_event(json: &str) -> AmpEvent {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn parse_assistant_text_event() {
        let event = make_event(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Hello\nWorld"}]}}"#,
        );
        let lines = parse_event(&event);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].kind, OutputKind::Assistant);
        assert_eq!(lines[0].text, "Hello");
        assert_eq!(lines[1].text, "World");
    }

    #[test]
    fn parse_assistant_tool_use_event() {
        let event = make_event(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","name":"Read","id":"t1","input":{}}]}}"#,
        );
        let lines = parse_event(&event);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].kind, OutputKind::Tool);
        assert_eq!(lines[0].text, "⚡ Read");
    }

    #[test]
    fn parse_result_success() {
        let event = make_event(r#"{"type":"result","status":"success"}"#);
        let lines = parse_event(&event);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].kind, OutputKind::ResultSuccess);
        assert!(lines[0].text.contains("success"));
    }

    #[test]
    fn parse_result_error() {
        let event = make_event(r#"{"type":"result","status":"error"}"#);
        let lines = parse_event(&event);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].kind, OutputKind::ResultError);
        assert!(lines[0].text.contains("error"));
    }

    #[test]
    fn parse_system_init() {
        let event = make_event(r#"{"type":"system","subtype":"init"}"#);
        let lines = parse_event(&event);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].kind, OutputKind::System);
        assert_eq!(lines[0].text, "system: init");
    }

    #[test]
    fn parse_unknown_type_produces_no_lines() {
        let event = make_event(r#"{"type":"unknown_type"}"#);
        let lines = parse_event(&event);
        assert!(lines.is_empty());
    }

    #[test]
    fn buffer_push_and_retrieve() {
        let mut buf = SessionOutputBuffer::new();
        let event = make_event(r#"{"type":"result","status":"success"}"#);
        buf.push_event("T-1", &event);
        assert_eq!(buf.line_count("T-1"), 1);
        assert_eq!(buf.lines_for("T-1")[0].kind, OutputKind::ResultSuccess);
    }

    #[test]
    fn buffer_separate_threads() {
        let mut buf = SessionOutputBuffer::new();
        let e1 = make_event(r#"{"type":"result","status":"success"}"#);
        let e2 = make_event(r#"{"type":"result","status":"error"}"#);
        buf.push_event("T-1", &e1);
        buf.push_event("T-2", &e2);
        assert_eq!(buf.line_count("T-1"), 1);
        assert_eq!(buf.line_count("T-2"), 1);
        assert_eq!(buf.lines_for("T-1")[0].kind, OutputKind::ResultSuccess);
        assert_eq!(buf.lines_for("T-2")[0].kind, OutputKind::ResultError);
    }

    #[test]
    fn buffer_accumulates_lines() {
        let mut buf = SessionOutputBuffer::new();
        let e1 = make_event(r#"{"type":"system","subtype":"init"}"#);
        let e2 = make_event(r#"{"type":"result","status":"success"}"#);
        buf.push_event("T-1", &e1);
        buf.push_event("T-1", &e2);
        assert_eq!(buf.line_count("T-1"), 2);
    }

    #[test]
    fn buffer_unknown_thread_returns_empty() {
        let buf = SessionOutputBuffer::new();
        assert!(buf.lines_for("T-nope").is_empty());
        assert_eq!(buf.line_count("T-nope"), 0);
    }

    #[test]
    fn buffer_clear_thread() {
        let mut buf = SessionOutputBuffer::new();
        let event = make_event(r#"{"type":"result","status":"success"}"#);
        buf.push_event("T-1", &event);
        assert_eq!(buf.line_count("T-1"), 1);
        buf.clear_thread("T-1");
        assert_eq!(buf.line_count("T-1"), 0);
    }

    #[test]
    fn parse_assistant_mixed_content() {
        let event = make_event(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Line 1"},{"type":"tool_use","name":"Grep","id":"t2","input":{}},{"type":"text","text":"Line 2"}]}}"#,
        );
        let lines = parse_event(&event);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].kind, OutputKind::Assistant);
        assert_eq!(lines[0].text, "Line 1");
        assert_eq!(lines[1].kind, OutputKind::Tool);
        assert_eq!(lines[1].text, "⚡ Grep");
        assert_eq!(lines[2].kind, OutputKind::Assistant);
        assert_eq!(lines[2].text, "Line 2");
    }

    #[test]
    fn parse_system_without_subtype_produces_no_lines() {
        let event = make_event(r#"{"type":"system"}"#);
        let lines = parse_event(&event);
        assert!(lines.is_empty());
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
