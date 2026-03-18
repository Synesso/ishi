use anyhow::Result;
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::Mutex;

use super::client::LinearApi;

/// A fake Linear API client for tests and offline development.
/// Enqueue responses with `push_response`, and they'll be returned in order by `query`.
pub struct FakeLinearApi {
    responses: Mutex<VecDeque<Value>>,
}

impl FakeLinearApi {
    pub fn new() -> Self {
        Self {
            responses: Mutex::new(VecDeque::new()),
        }
    }

    pub fn push_response(&self, response: Value) {
        self.responses.lock().unwrap().push_back(response);
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
}
