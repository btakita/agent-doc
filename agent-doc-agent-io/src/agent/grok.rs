//! Grok Build's one-shot JSON protocol (verified with Grok 1.0.24).
//!
//! This actorless backend shares the bounded child-process executor. It does not
//! own document state or retries; only a successful final cut reaches closeout.
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::io::Write;
use std::process::{Command, Stdio};

use super::{
    Agent, AgentResponse, configure_agent_child_process_group, run_agent_timeout,
    wait_with_output_timeout,
};

pub struct Grok {
    command: String,
    base_args: Vec<String>,
    env: Vec<(String, Option<String>)>,
}

impl Grok {
    pub fn new(command: Option<String>, base_args: Option<Vec<String>>) -> Self {
        Self {
            command: command.unwrap_or_else(|| "grok".into()),
            base_args: base_args.unwrap_or_default(),
            env: vec![],
        }
    }

    pub fn with_env(mut self, env: Vec<(String, Option<String>)>) -> Self {
        self.env = env;
        self
    }

    fn args(
        &self,
        prompt_file: &std::path::Path,
        session_id: Option<&str>,
        fork: bool,
        model: Option<&str>,
    ) -> Result<Vec<String>> {
        let mut args = self.base_args.clone();
        if let Some(id) = session_id {
            args.extend(["--resume".into(), id.into()]);
        }
        // `run` sets fork=true on every first invocation. Without a bound ID,
        // start fresh instead of forking an unrelated latest conversation.
        if fork && session_id.is_some() {
            args.push("--fork-session".into());
        }
        if let Some(model) = model {
            args.extend(["--model".into(), model.into()]);
        }
        args.extend([
            "--prompt-file".into(),
            prompt_file.to_string_lossy().into_owned(),
            "--output-format".into(),
            "json".into(),
        ]);
        Ok(args)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FinalResponse {
    text: String,
    stop_reason: String,
    session_id: String,
}

fn protocol_error(stdout: &[u8]) -> Option<String> {
    #[derive(Deserialize)]
    struct ErrorResponse {
        #[serde(rename = "type")]
        kind: String,
        message: String,
    }
    let error: ErrorResponse = serde_json::from_slice(stdout).ok()?;
    (error.kind == "error" && !error.message.trim().is_empty()).then_some(error.message)
}

fn parse_response(stdout: &[u8]) -> Result<AgentResponse> {
    if let Some(message) = protocol_error(stdout) {
        bail!("Grok Build: {message}");
    }
    // Deliberately ignore thought/usage fields; never publish reasoning traces.
    let response: FinalResponse =
        serde_json::from_slice(stdout).context("invalid Grok Build final JSON response")?;
    if response.stop_reason != "end_turn" {
        bail!(
            "Grok Build did not complete the turn: {}",
            response.stop_reason
        );
    }
    if response.text.trim().is_empty() || response.session_id.trim().is_empty() {
        bail!("Grok Build returned empty response text or session identity");
    }
    Ok(AgentResponse {
        text: response.text,
        session_id: Some(response.session_id),
    })
}

impl Agent for Grok {
    fn send(
        &self,
        prompt: &str,
        session_id: Option<&str>,
        fork: bool,
        model: Option<&str>,
    ) -> Result<AgentResponse> {
        // A private temporary file avoids argv limits and exposing document text
        // in process listings. It remains alive until the child has exited.
        let mut prompt_file = tempfile::NamedTempFile::new().context("create Grok prompt file")?;
        prompt_file
            .write_all(prompt.as_bytes())
            .context("write Grok prompt file")?;
        prompt_file.flush()?;
        let args = self.args(prompt_file.path(), session_id, fork, model)?;
        let mut command = Command::new(&self.command);
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in &self.env {
            if let Some(value) = value {
                command.env(key, value);
            } else {
                command.env_remove(key);
            }
        }
        configure_agent_child_process_group(&mut command);
        let child = command.spawn().with_context(|| {
            format!(
                "failed to launch Grok Build '{}'; install Grok Build and run `grok login`",
                self.command
            )
        })?;
        let output =
            wait_with_output_timeout(child, run_agent_timeout()).context("wait for Grok Build")?;
        if !output.status.success() {
            let message = protocol_error(&output.stdout)
                .unwrap_or_else(|| String::from_utf8_lossy(&output.stderr).trim().to_string());
            bail!("Grok Build exited {}: {}", output.status, message);
        }
        parse_response(&output.stdout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn final_protocol_excludes_thought_and_refuses_incomplete_output() {
        let response = parse_response(br#"{"text":"answer", "stopReason":"end_turn", "sessionId":"id", "thought":"private reasoning"}"#).unwrap();
        assert_eq!(response.text, "answer");
        assert_eq!(response.session_id.as_deref(), Some("id"));
        let quota = parse_response(
            br#"{"type":"error","message":"usage limit reached","thought":"private reasoning"}"#,
        )
        .err()
        .unwrap()
        .to_string();
        assert_eq!(quota, "Grok Build: usage limit reached");
        for input in [
            r#"{"result":"wrong protocol","session_id":"id"}"#,
            r#"{"text":"partial","stopReason":"max_tokens","sessionId":"id"}"#,
            r#"{"text":"partial","stopReason":"cancelled","sessionId":"id"}"#,
            r#"{"text":"","stopReason":"end_turn","sessionId":"id"}"#,
            r#"{"text":"answer","stopReason":"end_turn","sessionId":""}"#,
            "not json",
        ] {
            assert!(parse_response(input.as_bytes()).is_err(), "{input}");
        }
    }

    #[test]
    fn resume_and_fork_use_only_the_document_identity() {
        let backend = Grok::new(None, None);
        let path = std::path::Path::new("prompt.txt");
        let fresh = backend.args(path, None, true, None).unwrap();
        assert!(!fresh.contains(&"--continue".into()));
        assert!(!fresh.contains(&"--fork-session".into()));
        let args = backend
            .args(path, Some("doc-id"), true, Some("custom-model"))
            .unwrap();
        assert_eq!(
            args,
            [
                "--resume",
                "doc-id",
                "--fork-session",
                "--model",
                "custom-model",
                "--prompt-file",
                "prompt.txt",
                "--output-format",
                "json"
            ]
        );
        assert!(
            !backend
                .args(path, None, false, None)
                .unwrap()
                .contains(&"--continue".into())
        );
    }

    #[cfg(unix)]
    #[test]
    fn child_receives_large_prompt_and_explicit_environment() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("fake-grok.sh");
        std::fs::write(
            &script,
            r#"#!/bin/sh
test "$GROK_TEST_VALUE" = present || exit 21
test -z "$GROK_TEST_REMOVED" || exit 22
test -z "$GROK_SESSION_ID" || exit 27
test "$1" = --prompt-file || exit 23
test "$(wc -c < "$2")" -gt 200000 || exit 24
test "$3" = --output-format || exit 25
test "$4" = json || exit 26
printf '%s' '{"text":"ok","stopReason":"end_turn","sessionId":"test-session"}'
"#,
        )
        .unwrap();
        let backend = Grok::new(
            Some("sh".into()),
            Some(vec![script.to_string_lossy().into_owned()]),
        )
        .with_env(vec![
            ("GROK_TEST_VALUE".into(), Some("present".into())),
            ("GROK_TEST_REMOVED".into(), None),
            ("GROK_SESSION_ID".into(), Some("parent-session".into())),
        ]);
        assert_eq!(
            backend
                .send(&"x".repeat(250000), None, false, None)
                .unwrap()
                .text,
            "ok"
        );
        let failing = Grok::new(
            Some("sh".into()),
            Some(vec!["-c".into(), "echo access-denied >&2; exit 4".into()]),
        );
        assert!(
            failing
                .send("prompt", None, false, None)
                .err()
                .unwrap()
                .to_string()
                .contains("access-denied")
        );
    }
}
