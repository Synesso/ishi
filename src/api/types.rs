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
