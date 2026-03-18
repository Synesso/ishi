use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct Issue {
    #[allow(dead_code)]
    pub id: String,
    pub identifier: String,
    pub title: String,
    pub url: Option<String>,
    pub state: Option<IssueState>,
    pub priority: Option<f64>,
    pub project: Option<IssueProject>,
    pub description: Option<String>,
    pub assignee: Option<IssueUser>,
    pub labels: Option<IssueLabels>,
    pub comments: Option<IssueComments>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct IssueProject {
    pub name: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct IssueUser {
    pub name: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct IssueLabel {
    pub name: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct IssueLabels {
    pub nodes: Vec<IssueLabel>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct IssueComment {
    pub body: String,
    pub user: Option<IssueUser>,
    #[serde(rename = "createdAt")]
    pub created_at: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct IssueComments {
    pub nodes: Vec<IssueComment>,
}

impl Issue {
    pub fn status_str(&self) -> &str {
        self.state.as_ref().map_or("—", |s| s.name.as_str())
    }

    pub fn project_str(&self) -> &str {
        self.project.as_ref().map_or("—", |p| p.name.as_str())
    }

    pub fn priority_str(&self) -> &str {
        match self.priority.map(|p| p as u8) {
            Some(1) => "Urgent",
            Some(2) => "High",
            Some(3) => "Medium",
            Some(4) => "Low",
            _ => "—",
        }
    }

    pub fn matches_search(&self, query: &str) -> bool {
        self.identifier.to_lowercase().contains(query)
            || self.title.to_lowercase().contains(query)
            || self.project_str().to_lowercase().contains(query)
            || self.status_str().to_lowercase().contains(query)
            || self.priority_str().to_lowercase().contains(query)
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct IssueState {
    pub name: String,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct IssueConnection {
    pub nodes: Vec<Issue>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct IssuesData {
    pub issues: IssueConnection,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct ViewerIssuesResponse {
    pub data: IssuesData,
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
