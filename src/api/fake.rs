use anyhow::Result;
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::Mutex;

use super::client::LinearApi;
use super::types::Issue;

/// A fake Linear API client for tests and offline development.
/// Enqueue responses with `push_response`, and they'll be returned in order by `query`.
/// PR URL responses are enqueued separately with `push_pr_url`.
pub struct FakeLinearApi {
    responses: Mutex<VecDeque<Value>>,
    pr_urls: Mutex<VecDeque<Option<String>>>,
}

impl FakeLinearApi {
    pub fn new() -> Self {
        Self {
            responses: Mutex::new(VecDeque::new()),
            pr_urls: Mutex::new(VecDeque::new()),
        }
    }

    pub fn push_response(&self, response: Value) {
        self.responses.lock().unwrap().push_back(response);
    }

    #[allow(dead_code)]
    pub fn push_pr_url(&self, url: Option<String>) {
        self.pr_urls.lock().unwrap().push_back(url);
    }
}

impl LinearApi for FakeLinearApi {
    async fn query(&self, _query: &str, _variables: Option<Value>) -> Result<Value> {
        let response = self
            .responses
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| serde_json::json!({"data": null}));
        Ok(response)
    }

    async fn fetch_my_issues(&self) -> Result<Vec<Issue>> {
        let resp = self.query("", None).await?;
        let nodes = &resp["data"]["issues"]["nodes"];
        if nodes.is_null() {
            return Ok(vec![]);
        }
        let issues: Vec<Issue> = serde_json::from_value(nodes.clone())?;
        Ok(issues)
    }

    async fn fetch_pull_request_url(&self, _issue_id: &str) -> Result<Option<String>> {
        Ok(self.pr_urls.lock().unwrap().pop_front().flatten())
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
    async fn fetch_pull_request_url_returns_none_explicitly() {
        let fake = FakeLinearApi::new();
        fake.push_pr_url(None);

        let url = fake.fetch_pull_request_url("issue-1").await.unwrap();
        assert!(url.is_none());
    }
}
