use anyhow::Result;
use reqwest::Client;
use serde_json::Value;

use super::types::{Issue, Project};

const LINEAR_API_URL: &str = "https://api.linear.app/graphql";

const ISSUE_PR_QUERY: &str = r#"
query($issueId: String!) {
  issue(id: $issueId) {
    attachments {
      nodes {
        url
        sourceType
      }
    }
  }
}
"#;

const MY_ISSUES_QUERY: &str = r#"
query($after: String) {
  viewer {
    assignedIssues(
      first: 50
      after: $after
      filter: { state: { type: { nin: ["completed", "canceled"] } } }
      orderBy: updatedAt
    ) {
      pageInfo { hasNextPage endCursor }
      nodes {
        id
        identifier
        title
        url
        state { name }
        priority
        project { name }
        description
        assignee { name }
        labels { nodes { name } }
        comments { nodes { body user { name } createdAt } }
      }
    }
  }
}
"#;

const MY_PROJECTS_QUERY: &str = r#"
query {
  viewer {
    teamMemberships {
      nodes {
        team {
          projects(
            first: 50
            filter: { state: { nin: ["completed", "canceled"] } }
            orderBy: updatedAt
          ) {
            nodes {
              id
              name
              state
              progress
              lead { name }
              url
            }
          }
        }
      }
    }
  }
}
"#;

const TEAM_STATES_QUERY: &str = r#"
query($issueId: String!) {
  issue(id: $issueId) {
    team {
      states {
        nodes {
          id
          name
          type
          position
        }
      }
    }
  }
}
"#;

const UPDATE_ISSUE_STATE_MUTATION: &str = r#"
mutation($issueId: String!, $stateId: String!) {
  issueUpdate(id: $issueId, input: { stateId: $stateId }) {
    issue {
      state { name }
    }
  }
}
"#;

const PROJECT_ISSUES_QUERY: &str = r#"
query($projectId: String!, $after: String) {
  project(id: $projectId) {
    issues(
      first: 50
      after: $after
      filter: { state: { type: { nin: ["completed", "canceled"] } } }
      orderBy: updatedAt
    ) {
      pageInfo { hasNextPage endCursor }
      nodes {
        id
        identifier
        title
        url
        state { name }
        priority
        project { name }
        description
        assignee { name }
        labels { nodes { name } }
        comments { nodes { body user { name } createdAt } }
      }
    }
  }
}
"#;

pub trait LinearApi: Send + Sync {
    fn query(
        &self,
        query: &str,
        variables: Option<Value>,
    ) -> impl std::future::Future<Output = Result<Value>> + Send;

    fn fetch_my_issues(&self) -> impl std::future::Future<Output = Result<Vec<Issue>>> + Send;

    fn fetch_pull_request_url(
        &self,
        issue_id: &str,
    ) -> impl std::future::Future<Output = Result<Option<String>>> + Send;

    fn fetch_projects(&self) -> impl std::future::Future<Output = Result<Vec<Project>>> + Send;

    fn fetch_project_issues(
        &self,
        project_id: &str,
    ) -> impl std::future::Future<Output = Result<Vec<Issue>>> + Send;

    fn fetch_team_states(
        &self,
        issue_id: &str,
    ) -> impl std::future::Future<Output = Result<Vec<String>>> + Send;

    fn update_issue_state(
        &self,
        issue_id: &str,
        state_name: &str,
    ) -> impl std::future::Future<Output = Result<String>> + Send;
}

#[derive(Clone)]
pub struct LinearClient {
    client: Client,
    api_key: String,
}

impl LinearClient {
    pub fn new(api_key: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
        }
    }
}

impl LinearApi for LinearClient {
    async fn query(&self, query: &str, variables: Option<Value>) -> Result<Value> {
        let mut body = serde_json::json!({ "query": query });
        if let Some(vars) = variables {
            body["variables"] = vars;
        }

        let resp = self
            .client
            .post(LINEAR_API_URL)
            .header("Authorization", &self.api_key)
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json::<Value>()
            .await?;

        Ok(resp)
    }

    async fn fetch_my_issues(&self) -> Result<Vec<Issue>> {
        let mut all_issues: Vec<Issue> = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let vars = serde_json::json!({ "after": cursor });
            let resp = self.query(MY_ISSUES_QUERY, Some(vars)).await?;
            let connection = &resp["data"]["viewer"]["assignedIssues"];
            let issues: Vec<Issue> = serde_json::from_value(connection["nodes"].clone())?;
            all_issues.extend(issues);
            let has_next = connection["pageInfo"]["hasNextPage"].as_bool().unwrap_or(false);
            if !has_next {
                break;
            }
            cursor = connection["pageInfo"]["endCursor"].as_str().map(String::from);
        }
        Ok(all_issues)
    }

    async fn fetch_pull_request_url(&self, issue_id: &str) -> Result<Option<String>> {
        let vars = serde_json::json!({ "issueId": issue_id });
        let resp = self.query(ISSUE_PR_QUERY, Some(vars)).await?;
        let nodes = &resp["data"]["issue"]["attachments"]["nodes"];
        if let Some(arr) = nodes.as_array() {
            for node in arr {
                if let Some(url) = node["url"].as_str()
                    && url.contains("github.com")
                    && url.contains("/pull/")
                {
                    return Ok(Some(url.to_string()));
                }
            }
        }
        Ok(None)
    }

    async fn fetch_projects(&self) -> Result<Vec<Project>> {
        let resp = self.query(MY_PROJECTS_QUERY, None).await?;
        let mut projects: Vec<Project> = Vec::new();
        let mut seen_ids = std::collections::HashSet::new();
        if let Some(memberships) = resp["data"]["viewer"]["teamMemberships"]["nodes"].as_array() {
            for membership in memberships {
                if let Some(nodes) = membership["team"]["projects"]["nodes"].as_array() {
                    for node in nodes {
                        if let Ok(project) = serde_json::from_value::<Project>(node.clone())
                            && seen_ids.insert(project.id.clone())
                        {
                            projects.push(project);
                        }
                    }
                }
            }
        }
        Ok(projects)
    }

    async fn fetch_project_issues(&self, project_id: &str) -> Result<Vec<Issue>> {
        let mut all_issues: Vec<Issue> = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let vars = serde_json::json!({ "projectId": project_id, "after": cursor });
            let resp = self.query(PROJECT_ISSUES_QUERY, Some(vars)).await?;
            let connection = &resp["data"]["project"]["issues"];
            let issues: Vec<Issue> = serde_json::from_value(connection["nodes"].clone())?;
            all_issues.extend(issues);
            let has_next = connection["pageInfo"]["hasNextPage"].as_bool().unwrap_or(false);
            if !has_next {
                break;
            }
            cursor = connection["pageInfo"]["endCursor"].as_str().map(String::from);
        }
        Ok(all_issues)
    }

    async fn fetch_team_states(&self, issue_id: &str) -> Result<Vec<String>> {
        let vars = serde_json::json!({ "issueId": issue_id });
        let resp = self.query(TEAM_STATES_QUERY, Some(vars)).await?;
        let states = resp["data"]["issue"]["team"]["states"]["nodes"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("failed to fetch workflow states"))?;

        // Sort by position as configured in the Linear project.
        let mut state_entries: Vec<(String, f64)> = states
            .iter()
            .filter_map(|s| {
                let name = s["name"].as_str()?.to_string();
                let position = s["position"].as_f64().unwrap_or(f64::MAX);
                Some((name, position))
            })
            .collect();
        state_entries.sort_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        Ok(state_entries.into_iter().map(|(name, _)| name).collect())
    }

    async fn update_issue_state(&self, issue_id: &str, state_name: &str) -> Result<String> {
        let vars = serde_json::json!({ "issueId": issue_id });
        let resp = self.query(TEAM_STATES_QUERY, Some(vars)).await?;
        let states = resp["data"]["issue"]["team"]["states"]["nodes"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("failed to fetch workflow states"))?;

        let state_id = states
            .iter()
            .find(|s| s["name"].as_str() == Some(state_name))
            .and_then(|s| s["id"].as_str())
            .ok_or_else(|| anyhow::anyhow!("state '{}' not found", state_name))?;

        let vars = serde_json::json!({ "issueId": issue_id, "stateId": state_id });
        let resp = self.query(UPDATE_ISSUE_STATE_MUTATION, Some(vars)).await?;
        let new_state = resp["data"]["issueUpdate"]["issue"]["state"]["name"]
            .as_str()
            .unwrap_or(state_name)
            .to_string();
        Ok(new_state)
    }
}
