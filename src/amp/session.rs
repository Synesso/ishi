use serde::{Deserialize, Serialize};

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
