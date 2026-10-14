//! Repository-locked GitHub issue filing for agent-doc maintainers.
//!
//! `agent-doc admin issue` intentionally owns the `btakita/agent-doc` target.
//! A future top-level `agent-doc issue` remains available for caller-selected
//! repositories instead of inheriting this maintenance-only default.

use anyhow::{Context, bail};
use clap::ValueEnum;
use serde_json::json;
use std::fmt;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const REPOSITORY: &str = "btakita/agent-doc";

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum IssueHandling {
    /// Only a human should investigate and implement the issue.
    HumanOnly,
    /// Agents may investigate and propose or implement changes under human review.
    AgentAssisted,
}

impl IssueHandling {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HumanOnly => "human-only",
            Self::AgentAssisted => "agent-assisted",
        }
    }

    fn policy_text(self) -> &'static str {
        match self {
            Self::HumanOnly => {
                "Human-only. Do not dispatch an autonomous agent to investigate or implement this issue."
            }
            Self::AgentAssisted => {
                "Agent-assisted. Agents may investigate or implement this issue under human review."
            }
        }
    }
}

impl fmt::Display for IssueHandling {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

pub struct IssueOptions {
    pub title: String,
    pub body: Option<String>,
    pub body_file: Option<PathBuf>,
    pub handling: IssueHandling,
    pub labels: Vec<String>,
    pub dry_run: bool,
    pub json: bool,
}

pub fn run(options: IssueOptions) -> anyhow::Result<()> {
    let body = read_body(options.body.as_deref(), options.body_file.as_deref())?;
    let payload = body_with_policy(&body, options.handling);

    if options.dry_run {
        if options.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "status": "dry_run",
                    "repository": REPOSITORY,
                    "handling": options.handling.as_str(),
                    "title": options.title,
                    "body": payload,
                    "labels": options.labels,
                }))?
            );
        } else {
            println!(
                "[admin] issue dry-run repository={} handling={} title={:?}",
                REPOSITORY, options.handling, options.title
            );
            println!("{payload}");
        }
        return Ok(());
    }

    let mut command = gh_command(&options.title, &options.labels);
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to execute GitHub CLI (`gh`); install and authenticate `gh` first")?;
    child
        .stdin
        .take()
        .context("failed to open stdin for GitHub CLI")?
        .write_all(payload.as_bytes())
        .context("failed to send issue body to GitHub CLI")?;
    let output = child
        .wait_with_output()
        .context("failed while waiting for GitHub CLI")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        bail!(
            "GitHub CLI could not create an issue in {REPOSITORY}{}",
            if stderr.is_empty() {
                String::new()
            } else {
                format!(": {stderr}")
            }
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let url = stdout
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .map(str::trim);
    if options.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "status": "created",
                "repository": REPOSITORY,
                "handling": options.handling.as_str(),
                "url": url,
            }))?
        );
    } else if let Some(url) = url {
        println!("{url}");
    }
    Ok(())
}

fn read_body(body: Option<&str>, body_file: Option<&Path>) -> anyhow::Result<String> {
    match (body, body_file) {
        (Some(body), None) => Ok(body.to_string()),
        (None, Some(path)) if path == Path::new("-") => {
            let mut body = String::new();
            std::io::stdin()
                .read_to_string(&mut body)
                .context("failed to read issue body from stdin")?;
            Ok(body)
        }
        (None, Some(path)) => std::fs::read_to_string(path)
            .with_context(|| format!("failed to read issue body from {}", path.display())),
        (None, None) => Ok(String::new()),
        (Some(_), Some(_)) => bail!("--body and --body-file cannot be used together"),
    }
}

fn body_with_policy(body: &str, handling: IssueHandling) -> String {
    let body = body.trim_end();
    let separator = if body.is_empty() { "" } else { "\n\n" };
    format!(
        "{body}{separator}---\n\nAgent handling policy: **{}**\n\n<!-- agent-doc-admin-issue: handling={} -->\n",
        handling.policy_text(),
        handling.as_str()
    )
}

fn gh_command(title: &str, labels: &[String]) -> Command {
    let mut command = Command::new("gh");
    command.args([
        "issue",
        "create",
        "--repo",
        REPOSITORY,
        "--title",
        title,
        "--body-file",
        "-",
    ]);
    for label in labels {
        command.args(["--label", label]);
    }
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_marker_is_machine_readable_and_preserves_body() {
        let body = body_with_policy("Observed failure.\n", IssueHandling::HumanOnly);
        assert!(body.starts_with("Observed failure.\n\n---"));
        assert!(body.contains("Do not dispatch an autonomous agent"));
        assert!(body.contains("<!-- agent-doc-admin-issue: handling=human-only -->"));
    }

    #[test]
    fn gh_command_is_locked_to_agent_doc_repository() {
        let command = gh_command("Title", &["bug".to_string()]);
        let args = command
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            args,
            [
                "issue",
                "create",
                "--repo",
                REPOSITORY,
                "--title",
                "Title",
                "--body-file",
                "-",
                "--label",
                "bug",
            ]
        );
    }
}
