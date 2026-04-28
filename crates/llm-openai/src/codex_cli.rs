//! Codex CLI adapter.
//!
//! Delegates model calls to `codex exec`, so authentication, token refresh,
//! model availability, and ChatGPT/Codex backend transport stay owned by the
//! installed Codex CLI.

use std::env;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use async_trait::async_trait;
use lattice_core::error::LLMError;
use lattice_core::llm::Decision;
use lattice_core::{Event, EventPayload, ToolDescription};
use serde::Deserialize;
use tracing::{info, instrument};

/// LLM client backed by the local `codex` CLI.
pub struct CodexCliClient {
    model: String,
    codex_bin: String,
}

impl CodexCliClient {
    /// Create a Codex CLI client with a model name accepted by `codex exec`.
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            codex_bin: default_codex_bin(),
        }
    }

    /// Override the Codex executable path, mainly for tests or custom installs.
    #[must_use]
    pub fn with_codex_bin(mut self, codex_bin: impl Into<String>) -> Self {
        self.codex_bin = codex_bin.into();
        self
    }

    fn build_prompt(
        &self,
        history: &[Event],
        available_tools: &[ToolDescription],
        system_prompt: &str,
    ) -> String {
        let mut prompt = String::new();

        if !system_prompt.is_empty() {
            prompt.push_str("System instructions:\n");
            prompt.push_str(system_prompt);
            prompt.push_str("\n\n");
        }

        if !available_tools.is_empty() {
            prompt.push_str("Lattice tools available to the outer runtime:\n");
            for tool in available_tools {
                prompt.push_str("- ");
                prompt.push_str(&tool.name);
                prompt.push_str(": ");
                prompt.push_str(&tool.description);
                prompt.push('\n');
            }
            prompt.push_str(
                "\nReturn a final answer. Do not call tools yourself unless the user explicitly asks you to inspect the local system.\n\n",
            );
        }

        prompt.push_str("Conversation history:\n");
        for event in history {
            match &event.payload {
                EventPayload::SessionCreated => {}
                EventPayload::UserMessage { content } => {
                    prompt.push_str("User: ");
                    prompt.push_str(content);
                    prompt.push('\n');
                }
                EventPayload::Thinking { reasoning } => {
                    prompt.push_str("Assistant thinking: ");
                    prompt.push_str(reasoning);
                    prompt.push('\n');
                }
                EventPayload::ToolCallRequested { tool, params } => {
                    prompt.push_str("Assistant requested tool ");
                    prompt.push_str(tool);
                    prompt.push_str(" with params ");
                    prompt.push_str(&params.to_string());
                    prompt.push('\n');
                }
                EventPayload::ToolCallResult {
                    stdout,
                    stderr,
                    exit_code,
                } => {
                    prompt.push_str("Tool result exit=");
                    prompt.push_str(&exit_code.to_string());
                    prompt.push_str(" stdout=");
                    prompt.push_str(stdout);
                    if !stderr.is_empty() {
                        prompt.push_str(" stderr=");
                        prompt.push_str(stderr);
                    }
                    prompt.push('\n');
                }
                EventPayload::ToolCallError { error } => {
                    prompt.push_str("Tool error: ");
                    prompt.push_str(error);
                    prompt.push('\n');
                }
                EventPayload::FinalAnswer { answer } => {
                    prompt.push_str("Assistant final answer: ");
                    prompt.push_str(answer);
                    prompt.push('\n');
                }
                EventPayload::StateChange { from, to } => {
                    prompt.push_str("State changed: ");
                    prompt.push_str(from);
                    prompt.push_str(" -> ");
                    prompt.push_str(to);
                    prompt.push('\n');
                }
            }
        }

        prompt.push_str("\nProduce the next assistant final answer.");
        prompt
    }

    fn parse_json_output(stdout: &str) -> Result<String, LLMError> {
        let mut last_message = None;
        for line in stdout.lines() {
            let line = line.trim();
            if line.is_empty() || !line.starts_with('{') {
                continue;
            }

            let event: CodexJsonEvent =
                serde_json::from_str(line).map_err(|e| LLMError::InvalidResponse(e.to_string()))?;
            if let CodexJsonEvent::ItemCompleted {
                item: CodexItem::AgentMessage { text },
            } = event
            {
                last_message = Some(text);
            }
        }

        last_message.ok_or_else(|| {
            LLMError::InvalidResponse("codex exec produced no agent_message item".into())
        })
    }
}

fn default_codex_bin() -> String {
    if let Ok(appdata) = env::var("APPDATA") {
        let npm_codex = PathBuf::from(appdata).join("npm").join("codex.cmd");
        if npm_codex.exists() {
            return npm_codex.to_string_lossy().into_owned();
        }
    }

    "codex".into()
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum CodexJsonEvent {
    #[serde(rename = "item.completed")]
    ItemCompleted { item: CodexItem },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum CodexItem {
    AgentMessage {
        text: String,
    },
    #[serde(other)]
    Other,
}

#[async_trait]
impl lattice_core::LLMClient for CodexCliClient {
    #[instrument(skip(self, history, available_tools))]
    async fn decide(
        &self,
        history: &[Event],
        available_tools: &[ToolDescription],
        system_prompt: &str,
    ) -> Result<Decision, LLMError> {
        let prompt = self.build_prompt(history, available_tools, system_prompt);

        info!("running codex exec: model={}", self.model);
        let mut child = Command::new(&self.codex_bin)
            .args([
                "exec",
                "--skip-git-repo-check",
                "--json",
                "--model",
                &self.model,
                "-",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| LLMError::RequestFailed(format!("failed to run codex exec: {e}")))?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(prompt.as_bytes()).map_err(|e| {
                LLMError::RequestFailed(format!("failed to write codex stdin: {e}"))
            })?;
        }

        let output = child
            .wait_with_output()
            .map_err(|e| LLMError::RequestFailed(format!("failed to wait for codex exec: {e}")))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !output.status.success() {
            let message = if stderr.trim().is_empty() {
                stdout.trim().to_string()
            } else {
                stderr.trim().to_string()
            };
            return Err(LLMError::RequestFailed(message));
        }

        let answer = Self::parse_json_output(&stdout)?;
        Ok(Decision::FinalAnswer { answer })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_last_agent_message() {
        let stdout = r#"{"type":"thread.started","thread_id":"t"}
{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"first"}}
{"type":"item.completed","item":{"id":"item_1","type":"agent_message","text":"second"}}"#;

        assert_eq!(CodexCliClient::parse_json_output(stdout).unwrap(), "second");
    }

    #[test]
    fn rejects_missing_agent_message() {
        let stdout = r#"{"type":"thread.started","thread_id":"t"}"#;
        assert!(CodexCliClient::parse_json_output(stdout).is_err());
    }
}
