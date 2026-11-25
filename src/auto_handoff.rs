//! `queue handoff --all` and the hooks that run it (`#autohandoff`).
//!
//! Records each in-flight claim through the existing cross-harness handoff
//! store, then appends one exchange packet of `queue resume` commands. A
//! second run with the same packet body does not append again.

use std::io::Read;
use std::path::{Path, PathBuf};

use agent_doc_queue::cross_harness_handoff::handoff_chat_target;
use agent_doc_queue_io::cross_harness_handoff::{AutoHandoffClaim, AutoHandoffReport, handoff_all};
use anyhow::{Context, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExchangeWrite {
    Written,
    AlreadyPresent,
    SkippedEmpty,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Outcome {
    pub(crate) report: AutoHandoffReport,
    pub(crate) exchange: ExchangeWrite,
}

pub(crate) fn execute(file: &Path, to: Option<&str>) -> Result<Outcome> {
    let content = agent_doc_document_realtime_io::try_resolve_current_document_content(
        file,
        "queue_handoff_all",
    )?;
    let report = handoff_all(file, &content, to)?;
    if report.packet.is_empty() {
        return Ok(Outcome {
            report,
            exchange: ExchangeWrite::SkippedEmpty,
        });
    }
    if content.contains(&report.packet) {
        return Ok(Outcome {
            report,
            exchange: ExchangeWrite::AlreadyPresent,
        });
    }
    match crate::exchange::append_response(file, &report.header, &report.packet) {
        Ok(()) => Ok(Outcome {
            report,
            exchange: ExchangeWrite::Written,
        }),
        Err(err) => Ok(Outcome {
            report,
            exchange: ExchangeWrite::Failed(format!("{err:#}")),
        }),
    }
}

pub(crate) fn notice(outcome: &Outcome) -> String {
    if outcome.report.claims.is_empty() {
        return "Cross-harness handoff found no running claims (#autohandoff).".to_string();
    }
    let dest = outcome.report.to.as_deref().unwrap_or("the next harness");
    let count = outcome.report.claims.len();
    match &outcome.exchange {
        ExchangeWrite::Written => format!(
            "Cross-harness handoff recorded {count} running claim(s) for {dest}. Resume commands are in agent:exchange (#autohandoff)."
        ),
        ExchangeWrite::AlreadyPresent => format!(
            "Cross-harness handoff packet is already in agent:exchange for {dest} (#autohandoff)."
        ),
        ExchangeWrite::SkippedEmpty => {
            "Cross-harness handoff found no running claims (#autohandoff).".to_string()
        }
        ExchangeWrite::Failed(err) => format!(
            "Cross-harness handoff recorded {count} running claim(s) for {dest}, and the exchange write failed: {err}\n\n{}",
            outcome.report.packet
        ),
    }
}

pub(crate) fn run_cli(file: &Path, to: Option<&str>) -> Result<()> {
    let outcome = execute(file, to)?;
    print_outcome(file, &outcome);
    if let ExchangeWrite::Failed(err) = &outcome.exchange {
        anyhow::bail!("handoff records were stored; exchange packet was not written: {err}");
    }
    Ok(())
}

fn print_outcome(file: &Path, outcome: &Outcome) {
    println!("running claims: {}", outcome.report.claims.len());
    if outcome.report.skipped_without_worker > 0 {
        eprintln!(
            "[queue] handoff skipped {} in-flight claim(s) with no worker",
            outcome.report.skipped_without_worker
        );
    }
    for claim in &outcome.report.claims {
        print_claim(claim);
    }
    match &outcome.exchange {
        ExchangeWrite::Written => {
            eprintln!(
                "[queue] handoff packet written to agent:exchange ({})",
                file.display()
            );
            println!("---\n{}\n{}", outcome.report.header, outcome.report.packet);
        }
        ExchangeWrite::AlreadyPresent => {
            eprintln!("[queue] handoff packet already in agent:exchange");
        }
        ExchangeWrite::SkippedEmpty => {}
        ExchangeWrite::Failed(err) => {
            eprintln!("[queue] handoff packet was not written to agent:exchange: {err}");
            println!("---\n{}\n{}", outcome.report.header, outcome.report.packet);
        }
    }
}

fn print_claim(claim: &AutoHandoffClaim) {
    let recorded = &claim.record;
    println!(
        "session: {}\ndocument: {}\nitem: {}\nowner: {}\ngeneration: {}\nworker: {}\nworktree: {}\nbranch: {}\nhead: {}\nresume: {}",
        recorded.key.session_id,
        recorded.key.document_path,
        recorded.key.item,
        recorded.key.owner,
        recorded.key.generation,
        recorded.key.worker_id,
        recorded.worktree.as_deref().unwrap_or(""),
        recorded.branch.as_deref().unwrap_or(""),
        recorded.head.as_deref().unwrap_or(""),
        claim.resume_command,
    );
    if !claim.missing.is_empty() {
        println!("missing: {}", claim.missing.join(", "));
    }
}

pub(crate) fn run_for_pane(cwd: &Path, to: Option<&str>) -> Result<String> {
    let pane = agent_doc_tmux_io::current_pane_id_from_env();
    let file =
        agent_doc_session_registry_io::pane_document::resolve_pane_document(cwd, pane.as_deref())
            .map_err(|err| anyhow::anyhow!("{err}"))
            .context("cross-harness handoff needs the document this tmux pane claims")?;
    let outcome = execute(&file, to)?;
    Ok(notice(&outcome))
}

/// Claude/Grok `PreCompact`. Compaction continues even when the handoff cannot.
pub fn handle_pre_compact() -> Result<()> {
    let mut payload = String::new();
    let _ = std::io::stdin().read_to_string(&mut payload);
    let cwd = precompact_cwd(&payload);
    let to = precompact_target(&payload);
    match run_for_pane(&cwd, to.as_deref()) {
        Ok(text) => eprintln!("{text}"),
        Err(err) => eprintln!("[agent-doc] pre-compact handoff skipped: {err:#}"),
    }
    Ok(())
}

pub(crate) fn precompact_target(payload: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(payload).ok()?;
    let instructions = value
        .get("custom_instructions")
        .and_then(|item| item.as_str())
        .unwrap_or("");
    handoff_chat_target(instructions)
}

fn precompact_cwd(payload: &str) -> PathBuf {
    serde_json::from_str::<serde_json::Value>(payload)
        .ok()
        .and_then(|value| {
            value
                .get("cwd")
                .and_then(|item| item.as_str())
                .filter(|cwd| !cwd.is_empty())
                .map(PathBuf::from)
        })
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precompact_reads_a_handoff_phrase_and_ignores_auto_compact() {
        assert_eq!(
            precompact_target(
                r#"{"hook_event_name":"PreCompact","trigger":"manual","custom_instructions":"handoff to Grok Build"}"#
            )
            .as_deref(),
            Some("grok")
        );
        assert!(
            precompact_target(r#"{"hook_event_name":"PreCompact","trigger":"auto","cwd":"/tmp"}"#)
                .is_none()
        );
        assert!(precompact_target("not json").is_none());
    }

    #[test]
    fn second_identical_packet_is_not_appended() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("bugs.md");
        let baseline = concat!(
            "---\nagent_doc_session: 78c70863-fed7-40a8-b703-8261eeb8da9c\n",
            "agent_doc_format: template\n---\n\n",
            "## Queue\n\n<!-- agent:queue subagents go -->\n- do [#autohandoff]\n<!-- /agent:queue -->\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: original — test\n\n",
            "Original answer.\n",
            "<!-- agent:boundary:old -->\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, baseline).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        let (_, claimed) = agent_doc_queue_io::queue_claim::claim(
            &doc,
            "do [#autohandoff]",
            "subagent:patchrun",
            3600,
        )
        .unwrap();
        let started = agent_doc_queue_io::queue_claim::start_attempt(
            &doc,
            &claimed.item_text,
            "subagent:patchrun",
            "worker-1",
            3600,
            claimed.generation,
        )
        .unwrap();
        let content = std::fs::read_to_string(&doc).unwrap();
        agent_doc_queue_io::cross_harness_handoff::record(
            &doc,
            &content,
            agent_doc_queue_io::cross_harness_handoff::HandoffInput {
                item: started.item_text,
                owner: started.owner,
                generation: started.generation,
                worker_id: "worker-1".to_string(),
                owner_harness: None,
                worker_harness: Some("claude".to_string()),
                worker_conversation_id: None,
                worktree: Some("/home/brian/worktrees/patchrun/agent-doc".to_string()),
                branch: Some("patchrun".to_string()),
                head: Some("abc123".to_string()),
            },
        )
        .unwrap();

        let first = execute(&doc, Some("grok")).unwrap();
        assert_eq!(first.exchange, ExchangeWrite::Written);
        assert!(first.report.packet.contains("agent-doc queue resume"));
        let written = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(written.matches("**Handoff** (#autohandoff)").count(), 1);

        let second = execute(&doc, Some("grok")).unwrap();
        assert_eq!(second.exchange, ExchangeWrite::AlreadyPresent);
        let again = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(again.matches("**Handoff** (#autohandoff)").count(), 1);
    }
}
