use crate::{
    model::{ChatMessage, ModelClient, ToolCall},
    tools,
};
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::path::Path;
use tokio::sync::{mpsc, oneshot};

const MAX_TURNS: usize = 8;

#[derive(Debug)]
pub enum AgentEvent {
    Thinking { turn: usize },
    TextDelta(String),
    ToolStarted { name: String },
    ToolFinished {
        name: String,
        success: bool,
        summary: String,
    },
    Done,
}

#[derive(Debug)]
pub struct ConfirmRequest {
    pub question: String,
    pub response: oneshot::Sender<bool>,
}

#[derive(Debug, Clone)]
pub struct Agent {
    model: ModelClient,
}

impl Agent {
    pub fn new(model: ModelClient) -> Self {
        Self { model }
    }

    pub async fn run(
        &self,
        root: &Path,
        input: String,
        events: mpsc::UnboundedSender<AgentEvent>,
        confirms: mpsc::UnboundedSender<ConfirmRequest>,
    ) -> Result<()> {
        let mut messages = vec![
            ChatMessage::system(system_prompt()),
            ChatMessage::user(input),
        ];

        for turn in 1..=MAX_TURNS {
            let _ = events.send(AgentEvent::Thinking { turn });
            let response = self.model.chat(&messages, tools::tool_definitions()).await?;

            if !response.content.is_empty() {
                let _ = events.send(AgentEvent::TextDelta(response.content.clone()));
            }

            messages.push(ChatMessage::assistant(
                response.content,
                response.tool_calls.clone(),
            ));

            if response.tool_calls.is_empty() {
                let _ = events.send(AgentEvent::Done);
                return Ok(());
            }

            for call in response.tool_calls {
                let result = self.run_tool(root, &call, &events, &confirms).await;
                let (success, summary) = match result {
                    Ok(summary) => (true, summary),
                    Err(error) => (false, error.to_string()),
                };
                let _ = events.send(AgentEvent::ToolFinished {
                    name: call.name().to_string(),
                    success,
                    summary: summary.clone(),
                });
                messages.push(ChatMessage::tool(
                    call.id.clone(),
                    call.name().to_string(),
                    json!({
                        "success": success,
                        "content": summary,
                    })
                    .to_string(),
                ));
            }
        }

        let _ = events.send(AgentEvent::Done);
        anyhow::bail!("agent reached the maximum number of tool turns")
    }

    async fn run_tool(
        &self,
        root: &Path,
        call: &ToolCall,
        events: &mpsc::UnboundedSender<AgentEvent>,
        confirms: &mpsc::UnboundedSender<ConfirmRequest>,
    ) -> Result<String> {
        let _ = events.send(AgentEvent::ToolStarted {
            name: call.name().to_string(),
        });

        let arguments = call.parsed_arguments();
        match call.name() {
            "read_file" => {
                let path = string_arg(&arguments, "path")?;
                tools::read_file(root, path)
            }
            "write_file" => {
                let path = string_arg(&arguments, "path")?;
                let content = string_arg(&arguments, "content")?;
                let diff = tools::diff_for_write(root, path, content)?;
                if !confirm(confirms, format!("Apply this edit?\n{}", diff)).await? {
                    return Ok("Edit rejected by user.".into());
                }
                tools::write_file(root, path, content)?;
                Ok(diff)
            }
            "search_files" => {
                let query = string_arg(&arguments, "query")?;
                tools::search_files(root, query)
            }
            "shell" => {
                let command = string_arg(&arguments, "command")?;
                if tools::is_dangerous(command)
                    && !confirm(confirms, format!("Run potentially dangerous command?\n{}", command)).await?
                {
                    return Ok("Command rejected by user.".into());
                }
                tools::shell(root, command).await
            }
            other => anyhow::bail!("unknown tool {}", other),
        }
    }
}

fn string_arg<'a>(arguments: &'a Value, key: &str) -> Result<&'a str> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .with_context(|| format!("missing string argument {}", key))
}

async fn confirm(confirms: &mpsc::UnboundedSender<ConfirmRequest>, question: String) -> Result<bool> {
    let (response, receiver) = oneshot::channel();
    confirms
        .send(ConfirmRequest { question, response })
        .context("unable to request confirmation")?;
    Ok(receiver.await.unwrap_or(false))
}

fn system_prompt() -> String {
    "You are wemi-coder, a careful coding agent. Inspect files before editing, keep changes focused, and explain results clearly.".into()
}
