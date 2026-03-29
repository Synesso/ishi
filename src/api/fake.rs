use anyhow::Result;
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::Mutex;

use super::client::LinearApi;
use super::types::{Issue, Project};

/// A fake Linear API client for tests and offline development.
/// Enqueue responses with `push_response`, and they'll be returned in order by `query`.
/// PR URL responses are enqueued separately with `push_pr_url`.
/// Enqueue errors with `push_error` to simulate API failures.
pub struct FakeLinearApi {
    responses: Mutex<VecDeque<Value>>,
    pr_urls: Mutex<VecDeque<Option<String>>>,
    errors: Mutex<VecDeque<String>>,
    team_states: Mutex<VecDeque<Vec<String>>>,
}

impl FakeLinearApi {
    pub fn new() -> Self {
        Self {
            responses: Mutex::new(VecDeque::new()),
            pr_urls: Mutex::new(VecDeque::new()),
            errors: Mutex::new(VecDeque::new()),
            team_states: Mutex::new(VecDeque::new()),
        }
    }

    pub fn push_response(&self, response: Value) {
        self.responses.lock().unwrap().push_back(response);
    }

    pub fn push_pr_url(&self, url: Option<String>) {
        self.pr_urls.lock().unwrap().push_back(url);
    }

    pub fn push_error(&self, message: impl Into<String>) {
        self.errors.lock().unwrap().push_back(message.into());
    }

    pub fn push_team_states(&self, states: Vec<String>) {
        self.team_states.lock().unwrap().push_back(states);
    }
}

impl LinearApi for FakeLinearApi {
    async fn query(&self, _query: &str, _variables: Option<Value>) -> Result<Value> {
        if let Some(err_msg) = self.errors.lock().unwrap().pop_front() {
            return Err(anyhow::anyhow!("{}", err_msg));
        }
        let response = self
            .responses
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| serde_json::json!({"data": null}));
        Ok(response)
    }

    async fn fetch_my_issues(&self) -> Result<Vec<Issue>> {
        let mut all_issues: Vec<Issue> = Vec::new();
        loop {
            let resp = self.query("", None).await?;
            let connection = &resp["data"]["issues"];
            let nodes = &connection["nodes"];
            if nodes.is_null() {
                break;
            }
            let issues: Vec<Issue> = serde_json::from_value(nodes.clone())?;
            all_issues.extend(issues);
            let has_next = connection["pageInfo"]["hasNextPage"]
                .as_bool()
                .unwrap_or(false);
            if !has_next {
                break;
            }
        }
        Ok(all_issues)
    }

    async fn fetch_pull_request_url(&self, _issue_id: &str) -> Result<Option<String>> {
        Ok(self.pr_urls.lock().unwrap().pop_front().flatten())
    }

    async fn fetch_projects(&self) -> Result<Vec<Project>> {
        let resp = self.query("", None).await?;
        let nodes = &resp["data"]["projects"]["nodes"];
        if nodes.is_null() {
            return Ok(vec![]);
        }
        let projects: Vec<Project> = serde_json::from_value(nodes.clone())?;
        Ok(projects)
    }

    async fn fetch_project_issues(&self, _project_id: &str) -> Result<Vec<Issue>> {
        let mut all_issues: Vec<Issue> = Vec::new();
        loop {
            let resp = self.query("", None).await?;
            let connection = &resp["data"]["project"]["issues"];
            let nodes = &connection["nodes"];
            if nodes.is_null() {
                break;
            }
            let issues: Vec<Issue> = serde_json::from_value(nodes.clone())?;
            all_issues.extend(issues);
            let has_next = connection["pageInfo"]["hasNextPage"]
                .as_bool()
                .unwrap_or(false);
            if !has_next {
                break;
            }
        }
        Ok(all_issues)
    }

    async fn fetch_team_states(&self, _issue_id: &str) -> Result<Vec<String>> {
        if let Some(err_msg) = self.errors.lock().unwrap().pop_front() {
            return Err(anyhow::anyhow!("{}", err_msg));
        }
        Ok(self
            .team_states
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_default())
    }

    async fn update_issue_state(&self, _issue_id: &str, state_name: &str) -> Result<String> {
        if let Some(err_msg) = self.errors.lock().unwrap().pop_front() {
            return Err(anyhow::anyhow!("{}", err_msg));
        }
        Ok(state_name.to_string())
    }

    async fn create_comment(
        &self,
        _issue_id: &str,
        body: &str,
    ) -> Result<super::types::IssueComment> {
        if let Some(err_msg) = self.errors.lock().unwrap().pop_front() {
            return Err(anyhow::anyhow!("{}", err_msg));
        }
        Ok(super::types::IssueComment {
            body: body.to_string(),
            user: Some(super::types::IssueUser {
                name: "Test User".to_string(),
            }),
            created_at: "2025-01-01T00:00:00.000Z".to_string(),
        })
    }

    async fn create_issue(
        &self,
        _team_id: &str,
        title: &str,
        _project_id: Option<&str>,
        _priority: Option<i32>,
        description: Option<&str>,
        _assignee_id: Option<&str>,
    ) -> Result<Issue> {
        if let Some(err_msg) = self.errors.lock().unwrap().pop_front() {
            return Err(anyhow::anyhow!("{}", err_msg));
        }
        Ok(Issue {
            id: "new-id".to_string(),
            identifier: "JEM-99".to_string(),
            title: title.to_string(),
            url: None,
            state: Some(super::types::IssueState {
                name: "Backlog".to_string(),
            }),
            priority: None,
            project: None,
            description: description.map(|d| d.to_string()),
            assignee: None,
            labels: None,
            comments: None,
            parent: None,
            team: None,
        })
    }

    async fn fetch_viewer_teams(&self) -> Result<(String, Vec<(String, String)>)> {
        if let Some(err_msg) = self.errors.lock().unwrap().pop_front() {
            return Err(anyhow::anyhow!("{}", err_msg));
        }
        Ok((
            "viewer-1".to_string(),
            vec![("team-1".to_string(), "Jem".to_string())],
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn returns_enqueued_responses_in_order() {
        let fake = FakeLinearApi::new();
        fake.push_response(serde_json::json!({"data": {"first": true}}));
        fake.push_response(serde_json::json!({"data": {"second": true}}));

        let r1 = fake.query("q1", None).await.unwrap();
        let r2 = fake.query("q2", None).await.unwrap();

        assert_eq!(r1["data"]["first"], true);
        assert_eq!(r2["data"]["second"], true);
    }

    #[tokio::test]
    async fn returns_null_data_when_exhausted() {
        let fake = FakeLinearApi::new();
        let r = fake.query("q", None).await.unwrap();
        assert!(r["data"].is_null());
    }

    #[tokio::test]
    async fn fetch_my_issues_from_fixture() {
        let fake = FakeLinearApi::new();
        let fixture: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/my_issues.json")).unwrap();
        fake.push_response(fixture);

        let issues = fake.fetch_my_issues().await.unwrap();
        assert_eq!(issues.len(), 2);
        assert_eq!(issues[0].identifier, "JEM-1");
        assert_eq!(issues[1].identifier, "JEM-2");
    }

    #[tokio::test]
    async fn fetch_my_issues_empty_when_no_data() {
        let fake = FakeLinearApi::new();
        let issues = fake.fetch_my_issues().await.unwrap();
        assert!(issues.is_empty());
    }

    #[tokio::test]
    async fn fetch_pull_request_url_returns_enqueued_url() {
        let fake = FakeLinearApi::new();
        fake.push_pr_url(Some("https://github.com/org/repo/pull/42".into()));

        let url = fake.fetch_pull_request_url("issue-1").await.unwrap();
        assert_eq!(url, Some("https://github.com/org/repo/pull/42".into()));
    }

    #[tokio::test]
    async fn fetch_pull_request_url_returns_none_when_empty() {
        let fake = FakeLinearApi::new();
        let url = fake.fetch_pull_request_url("issue-1").await.unwrap();
        assert!(url.is_none());
    }

    #[tokio::test]
    async fn fetch_projects_from_fixture() {
        let fake = FakeLinearApi::new();
        let fixture: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/projects.json")).unwrap();
        fake.push_response(fixture);

        let projects = fake.fetch_projects().await.unwrap();
        assert_eq!(projects.len(), 2);
        assert_eq!(projects[0].name, "Alpha Project");
        assert_eq!(projects[1].name, "Beta Project");
    }

    #[tokio::test]
    async fn fetch_projects_empty_when_no_data() {
        let fake = FakeLinearApi::new();
        let projects = fake.fetch_projects().await.unwrap();
        assert!(projects.is_empty());
    }

    #[tokio::test]
    async fn fetch_project_issues_from_fixture() {
        let fake = FakeLinearApi::new();
        let fixture: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/project_issues.json")).unwrap();
        fake.push_response(fixture);

        let issues = fake.fetch_project_issues("proj-1").await.unwrap();
        assert_eq!(issues.len(), 2);
        assert_eq!(issues[0].identifier, "JEM-10");
    }

    #[tokio::test]
    async fn fetch_project_issues_empty_when_no_data() {
        let fake = FakeLinearApi::new();
        let issues = fake.fetch_project_issues("proj-1").await.unwrap();
        assert!(issues.is_empty());
    }

    #[tokio::test]
    async fn fetch_my_issues_paginates_across_pages() {
        let fake = FakeLinearApi::new();
        fake.push_response(serde_json::json!({
            "data": { "issues": {
                "pageInfo": { "hasNextPage": true, "endCursor": "cursor-1" },
                "nodes": [{ "id": "i1", "identifier": "JEM-1", "title": "First" }]
            }}
        }));
        fake.push_response(serde_json::json!({
            "data": { "issues": {
                "pageInfo": { "hasNextPage": false, "endCursor": null },
                "nodes": [{ "id": "i2", "identifier": "JEM-2", "title": "Second" }]
            }}
        }));

        let issues = fake.fetch_my_issues().await.unwrap();
        assert_eq!(issues.len(), 2);
        assert_eq!(issues[0].identifier, "JEM-1");
        assert_eq!(issues[1].identifier, "JEM-2");
    }

    #[tokio::test]
    async fn fetch_project_issues_paginates_across_pages() {
        let fake = FakeLinearApi::new();
        fake.push_response(serde_json::json!({
            "data": { "project": { "issues": {
                "pageInfo": { "hasNextPage": true, "endCursor": "cursor-1" },
                "nodes": [{ "id": "i10", "identifier": "JEM-10", "title": "First" }]
            }}}
        }));
        fake.push_response(serde_json::json!({
            "data": { "project": { "issues": {
                "pageInfo": { "hasNextPage": false, "endCursor": null },
                "nodes": [{ "id": "i11", "identifier": "JEM-11", "title": "Second" }]
            }}}
        }));

        let issues = fake.fetch_project_issues("proj-1").await.unwrap();
        assert_eq!(issues.len(), 2);
        assert_eq!(issues[0].identifier, "JEM-10");
        assert_eq!(issues[1].identifier, "JEM-11");
    }

    #[tokio::test]
    async fn fetch_pull_request_url_returns_none_explicitly() {
        let fake = FakeLinearApi::new();
        fake.push_pr_url(None);

        let url = fake.fetch_pull_request_url("issue-1").await.unwrap();
        assert!(url.is_none());
    }

    #[tokio::test]
    async fn fetch_team_states_returns_enqueued_states() {
        let fake = FakeLinearApi::new();
        fake.push_team_states(vec![
            "Backlog".into(),
            "Todo".into(),
            "In Progress".into(),
            "To be deployed".into(),
            "Done".into(),
        ]);

        let states = fake.fetch_team_states("issue-1").await.unwrap();
        assert_eq!(states.len(), 5);
        assert!(states.contains(&"To be deployed".to_string()));
    }

    #[tokio::test]
    async fn fetch_team_states_returns_empty_when_none_enqueued() {
        let fake = FakeLinearApi::new();
        let states = fake.fetch_team_states("issue-1").await.unwrap();
        assert!(states.is_empty());
    }
}
