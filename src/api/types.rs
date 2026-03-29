use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct IssueParent {
    pub identifier: String,
    pub title: String,
    pub description: Option<String>,
    pub url: Option<String>,
    pub state: Option<IssueState>,
    pub labels: Option<IssueLabels>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Issue {
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
    pub parent: Option<Box<IssueParent>>,
    pub team: Option<IssueTeam>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct IssueProject {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct IssueTeam {
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

    pub fn agent_prompt(&self) -> String {
        let mut prompt = format!(
            "Work on Linear issue {}:\n\n<issue identifier=\"{}\">",
            self.identifier, self.identifier
        );
        prompt.push_str(&format!("\n<title>{}</title>", self.title));
        if let Some(desc) = &self.description {
            prompt.push_str(&format!("\n<description>\n{}\n</description>", desc));
        }
        if let Some(team) = &self.team {
            prompt.push_str(&format!("\n<team name=\"{}\"/>", team.name));
        }
        if let Some(project) = &self.project {
            if let Some(desc) = &project.description {
                prompt.push_str(&format!(
                    "\n<project name=\"{}\">{}</project>",
                    project.name, desc
                ));
            } else {
                prompt.push_str(&format!("\n<project name=\"{}\"/>", project.name));
            }
        }
        prompt.push_str("\n</issue>");
        prompt
    }

    pub fn matches_search(&self, query: &str) -> bool {
        let query = query.to_lowercase();
        self.identifier.to_lowercase().contains(&query)
            || self.title.to_lowercase().contains(&query)
            || self.project_str().to_lowercase().contains(&query)
            || self.status_str().to_lowercase().contains(&query)
            || self.priority_str().to_lowercase().contains(&query)
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct IssueState {
    pub name: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub state: Option<String>,
    pub progress: Option<f64>,
    pub lead: Option<IssueUser>,
    pub url: Option<String>,
}

impl Project {
    pub fn status_str(&self) -> &str {
        self.state.as_deref().unwrap_or("—")
    }

    pub fn lead_str(&self) -> &str {
        self.lead.as_ref().map_or("—", |l| l.name.as_str())
    }

    pub fn progress_percent(&self) -> String {
        match self.progress {
            Some(p) => format!("{:.0}%", p * 100.0),
            None => "—".into(),
        }
    }

    pub fn matches_search(&self, query: &str) -> bool {
        let query = query.to_lowercase();
        self.name.to_lowercase().contains(&query)
            || self.status_str().to_lowercase().contains(&query)
            || self.lead_str().to_lowercase().contains(&query)
    }
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
    fn deserialize_project() {
        let json = r#"{
            "id": "proj-1",
            "name": "My Project",
            "state": "started",
            "progress": 0.75,
            "lead": { "name": "Alice" },
            "url": "https://linear.app/test/project/my-project"
        }"#;
        let project: Project = serde_json::from_str(json).unwrap();
        assert_eq!(project.name, "My Project");
        assert_eq!(project.status_str(), "started");
        assert_eq!(project.lead_str(), "Alice");
        assert_eq!(project.progress_percent(), "75%");
    }

    #[test]
    fn deserialize_project_without_optional_fields() {
        let json = r#"{
            "id": "proj-2",
            "name": "Minimal Project"
        }"#;
        let project: Project = serde_json::from_str(json).unwrap();
        assert_eq!(project.name, "Minimal Project");
        assert_eq!(project.status_str(), "—");
        assert_eq!(project.lead_str(), "—");
        assert_eq!(project.progress_percent(), "—");
    }

    #[test]
    fn project_progress_percent_rounds() {
        let project = Project {
            id: "p".into(),
            name: "P".into(),
            state: None,
            progress: Some(0.333),
            lead: None,
            url: None,
        };
        assert_eq!(project.progress_percent(), "33%");
    }

    #[test]
    fn project_matches_search_case_insensitively_across_name_status_and_lead() {
        let project = Project {
            id: "p".into(),
            name: "Alpha Project".into(),
            state: Some("started".into()),
            progress: Some(0.33),
            lead: Some(IssueUser {
                name: "Alice".into(),
            }),
            url: None,
        };

        assert!(project.matches_search("alpha"));
        assert!(project.matches_search("STARTED"));
        assert!(project.matches_search("aliCE"));
        assert!(!project.matches_search("beta"));
    }

    #[test]
    fn deserialize_projects_from_fixture() {
        let fixture = include_str!("../../tests/fixtures/projects.json");
        let value: serde_json::Value = serde_json::from_str(fixture).unwrap();
        let nodes = &value["data"]["projects"]["nodes"];
        let projects: Vec<Project> = serde_json::from_value(nodes.clone()).unwrap();
        assert_eq!(projects.len(), 2);
        assert_eq!(projects[0].name, "Alpha Project");
        assert_eq!(projects[1].name, "Beta Project");
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

    #[test]
    fn agent_prompt_minimal() {
        let issue = Issue {
            id: "1".into(),
            identifier: "JEM-42".into(),
            title: "Fix the widget".into(),
            url: None,
            state: None,
            priority: None,
            project: None,
            description: None,
            assignee: None,
            labels: None,
            comments: None,
            parent: None,
            team: None,
        };
        let prompt = issue.agent_prompt();
        assert!(prompt.starts_with("Work on Linear issue JEM-42:"));
        assert!(prompt.contains("<issue identifier=\"JEM-42\">"));
        assert!(prompt.contains("<title>Fix the widget</title>"));
        assert!(prompt.contains("</issue>"));
        assert!(!prompt.contains("<description>"));
        assert!(!prompt.contains("<team"));
        assert!(!prompt.contains("<project"));
    }

    #[test]
    fn agent_prompt_full() {
        let issue = Issue {
            id: "1".into(),
            identifier: "JEM-1".into(),
            title: "Add feature".into(),
            url: None,
            state: None,
            priority: None,
            project: Some(IssueProject {
                name: "ishi".into(),
                description: Some("A TUI for Linear".into()),
            }),
            description: Some("Implement the thing".into()),
            assignee: None,
            labels: None,
            comments: None,
            parent: None,
            team: Some(IssueTeam { name: "Jem".into() }),
        };
        let prompt = issue.agent_prompt();
        assert!(prompt.contains("<description>\nImplement the thing\n</description>"));
        assert!(prompt.contains("<team name=\"Jem\"/>"));
        assert!(prompt.contains("<project name=\"ishi\">A TUI for Linear</project>"));
    }
}
