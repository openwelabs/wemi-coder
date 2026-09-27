use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct ModelSettings {
    pub api_key: String,
    #[serde(default = "default_base_url")]
    pub base_url: String,
    #[serde(default = "default_model")]
    pub model: String,
}

#[derive(Debug, Clone)]
pub struct ModelClient {
    client: Client,
    settings: ModelSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl ChatMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self::text("user", content)
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self::text("system", content)
    }

    pub fn assistant(content: String, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: "assistant".into(),
            content: Some(content),
            tool_calls: (!tool_calls.is_empty()).then_some(tool_calls),
            tool_call_id: None,
            name: None,
        }
    }

    pub fn tool(tool_call_id: String, name: String, content: String) -> Self {
        Self {
            role: "tool".into(),
            content: Some(content),
            tool_calls: None,
            tool_call_id: Some(tool_call_id),
            name: Some(name),
        }
    }

    fn text(role: &str, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub function: ToolFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolFunction {
    pub name: String,
    pub arguments: String,
}

impl ToolCall {
    pub fn parsed_arguments(&self) -> Value {
        serde_json::from_str(&self.function.arguments).unwrap_or(Value::Null)
    }

    pub fn name(&self) -> &str {
        &self.function.name
    }
}

#[derive(Debug, Clone)]
pub struct ModelResponse {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
}

impl ModelClient {
    pub fn from_settings(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).with_context(|| {
            format!(
                "unable to read API settings at {}; create it with api_key/model/base_url",
                path.display()
            )
        })?;
        let settings: ModelSettings = serde_json::from_str(&text).context("invalid API settings JSON")?;
        if settings.api_key.trim().is_empty() {
            anyhow::bail!("api_key is required in {}", path.display());
        }
        Ok(Self {
            client: Client::new(),
            settings,
        })
    }

    pub async fn chat(&self, messages: &[ChatMessage], tools: Vec<Value>) -> Result<ModelResponse> {
        let url = format!(
            "{}/chat/completions",
            self.settings.base_url.trim_end_matches('/')
        );
        let body = json!({
            "model": self.settings.model,
            "messages": messages,
            "tools": tools,
            "tool_choice": "auto",
        });

        let response: Value = self
            .client
            .post(url)
            .bearer_auth(&self.settings.api_key)
            .json(&body)
            .send()
            .await
            .context("model request failed")?
            .error_for_status()
            .context("model returned an error status")?
            .json()
            .await
            .context("unable to parse model response")?;

        let message = response
            .pointer("/choices/0/message")
            .cloned()
            .context("model response did not include choices[0].message")?;
        let content = message
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let tool_calls = message
            .get("tool_calls")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(parse_tool_call)
            .collect();

        Ok(ModelResponse {
            content,
            tool_calls,
        })
    }
}

fn parse_tool_call(value: &Value) -> Option<ToolCall> {
    let function = value.get("function")?;

    Some(ToolCall {
        id: value.get("id")?.as_str()?.to_string(),
        kind: value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("function")
            .to_string(),
        function: ToolFunction {
            name: function.get("name")?.as_str()?.to_string(),
            arguments: function
                .get("arguments")
                .and_then(Value::as_str)
                .unwrap_or("{}")
                .to_string(),
        },
    })
}

fn default_base_url() -> String {
    "https://api.openai.com/v1".into()
}

fn default_model() -> String {
    "gpt-4o-mini".into()
}
