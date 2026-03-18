use anyhow::Result;
use reqwest::Client;
use serde_json::Value;

use super::types::Issue;

const LINEAR_API_URL: &str = "https://api.linear.app/graphql";

const MY_ISSUES_QUERY: &str = r#"
query {
  viewer {
    assignedIssues(
      first: 50
      filter: { state: { type: { nin: ["completed", "canceled"] } } }
      orderBy: updatedAt
    ) {
      nodes {
        id
        identifier
        title
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

    fn fetch_my_issues(
        &self,
    ) -> impl std::future::Future<Output = Result<Vec<Issue>>> + Send;
}

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
        let resp = self.query(MY_ISSUES_QUERY, None).await?;
        let issues: Vec<Issue> = serde_json::from_value(
            resp["data"]["viewer"]["assignedIssues"]["nodes"].clone(),
        )?;
        Ok(issues)
    }
}
