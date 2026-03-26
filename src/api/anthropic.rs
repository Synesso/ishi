use anyhow::Result;
use serde_json::Value;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct ExtractedIssue {
    pub title: String,
    pub description: String,
    pub team_name: Option<String>,
    pub project_name: Option<String>,
    pub priority: Option<String>,
}

pub fn extract_issue_from_text(
    user_text: &str,
    team_names: &[String],
    project_names: &[String],
) -> Result<ExtractedIssue> {
    let prompt = format!(
        "Extract a Linear issue from the following free-form text. \
         Return ONLY a JSON object (no markdown fences, no explanation) with these fields: \
         title (concise issue title), \
         description (markdown description), \
         team (team name from the list if mentioned, or null), \
         project (project name from the list if mentioned, or null), \
         priority (one of: urgent, high, medium, low, or null if not mentioned).\n\n\
         Available teams: {}\n\
         Available projects: {}\n\n\
         User input: {}",
        serde_json::to_string(team_names)?,
        serde_json::to_string(project_names)?,
        user_text,
    );

    let output = Command::new("amp")
        .args([
            "--execute",
            &prompt,
            "--stream-json",
            "--no-ide",
            "--archive",
            "--no-notifications",
            "-m",
            "rush",
        ])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("amp exited with {}: {}", output.status, stderr);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Parse the last NDJSON line with type "result" to get the response text.
    let result_text = stdout
        .lines()
        .rev()
        .find_map(|line| {
            let v: Value = serde_json::from_str(line).ok()?;
            if v["type"] == "result" {
                v["result"].as_str().map(String::from)
            } else {
                None
            }
        })
        .ok_or_else(|| anyhow::anyhow!("no result found in amp output"))?;

    // Strip markdown code fences if present.
    let json_text = result_text
        .trim()
        .strip_prefix("```json")
        .or_else(|| result_text.trim().strip_prefix("```"))
        .and_then(|s| s.strip_suffix("```"))
        .map(|s| s.trim())
        .unwrap_or(result_text.trim());

    let parsed: Value = serde_json::from_str(json_text)?;

    Ok(ExtractedIssue {
        title: parsed["title"].as_str().unwrap_or_default().to_string(),
        description: parsed["description"].as_str().unwrap_or_default().to_string(),
        team_name: parsed["team"].as_str().map(String::from),
        project_name: parsed["project"].as_str().map(String::from),
        priority: parsed["priority"].as_str().map(String::from),
    })
}
