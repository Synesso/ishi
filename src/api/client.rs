use anyhow::Result;
use reqwest::Client;
use serde_json::Value;

const LINEAR_API_URL: &str = "https://api.linear.app/graphql";

pub trait LinearApi: Send + Sync {
    fn query(
        &self,
        query: &str,
        variables: Option<Value>,
    ) -> impl std::future::Future<Output = Result<Value>> + Send;
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
}
