use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct Issue {
    pub id: String,
    pub identifier: String,
    pub title: String,
    pub state: Option<IssueState>,
    pub priority: Option<f64>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct IssueState {
    pub name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_issue_with_state() {
        let json = r#"{
            "id": "abc-123",
            "identifier": "JEM-1",
            "title": "Test issue",
            "state": { "name": "In Progress" },
            "priority": 2.0
        }"#;
        let issue: Issue = serde_json::from_str(json).unwrap();
        assert_eq!(issue.identifier, "JEM-1");
        assert_eq!(issue.title, "Test issue");
        assert_eq!(issue.state.unwrap().name, "In Progress");
        assert_eq!(issue.priority.unwrap(), 2.0);
    }

    #[test]
    fn deserialize_issue_without_optional_fields() {
        let json = r#"{
            "id": "abc-456",
            "identifier": "JEM-2",
            "title": "Minimal issue"
        }"#;
        let issue: Issue = serde_json::from_str(json).unwrap();
        assert_eq!(issue.identifier, "JEM-2");
        assert!(issue.state.is_none());
        assert!(issue.priority.is_none());
    }

    #[test]
    fn deserialize_issues_from_fixture() {
        let fixture = include_str!("../../tests/fixtures/my_issues.json");
        let value: serde_json::Value = serde_json::from_str(fixture).unwrap();
        let nodes = &value["data"]["issues"]["nodes"];
        let issues: Vec<Issue> = serde_json::from_value(nodes.clone()).unwrap();
        assert_eq!(issues.len(), 2);
        assert_eq!(issues[0].identifier, "JEM-1");
        assert_eq!(issues[1].identifier, "JEM-2");
    }
}
