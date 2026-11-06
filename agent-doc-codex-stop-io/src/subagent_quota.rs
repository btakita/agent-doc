//! Side effect of a harness subagent-end (`#workerquotaautodetect`).
//!
//! The ledger write is the effect the parent sees on its next read. Hook JSON
//! never uses `decision: "block"`: that would make the exhausted subagent keep
//! going. A second identical block is quiet, so Grok firing both the remapped
//! Stop hook and `SubagentStop` does not announce twice.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use agent_doc_harness::subagent_end::{
    SubagentEnd, SubagentEndParse, parse_subagent_end, usage_limit_from_end,
};
use agent_doc_harness::usage_limit::HarnessUsageLimit;
use agent_doc_queue::worker_quota::{WorkerQuotaBlock, quota_blocked_next_step};
pub use agent_doc_queue_io::worker_quota_autodetect::SubagentQuotaBlockApplied;
use agent_doc_queue_io::worker_quota_autodetect::{
    SubagentQuotaAutodetect, apply_subagent_quota_autodetect,
};
use anyhow::Result;
use serde_json::{Value, json};

const TRANSCRIPT_TAIL_BYTES: u64 = 64 * 1024;

pub(crate) fn observe_payload(payload: &str) -> Result<Option<SubagentQuotaBlockApplied>> {
    let value = serde_json::from_str::<Value>(payload).unwrap_or(Value::Null);
    observe_value(&value)
}

pub(crate) fn observe_value(value: &Value) -> Result<Option<SubagentQuotaBlockApplied>> {
    let signal = match parse_subagent_end(value) {
        Ok(signal) => signal,
        Err(SubagentEndParse::NotASubagentEnd) => return Ok(None),
        Err(SubagentEndParse::MissingWorkerId) => {
            eprintln!(
                "[agent-doc] subagent-end hook had no worker id (`subagentId` or `agent_id`); the claim was left unchanged."
            );
            return Ok(None);
        }
    };
    let tail = signal
        .transcript_path
        .as_deref()
        .and_then(|path| read_transcript_tail(Path::new(path)));
    let Some(limit) = usage_limit_from_end(
        signal.last_message.as_deref(),
        tail.as_deref(),
        signal.structured_reason.as_deref(),
    ) else {
        return Ok(None);
    };
    let block = worker_block(&signal, &limit);
    let scan_paths = scan_paths(&signal);
    let preferred = preferred_session_doc(&signal, &scan_paths);
    match apply_subagent_quota_autodetect(
        &scan_paths,
        preferred.as_deref(),
        &signal.worker_id,
        block,
    )? {
        SubagentQuotaAutodetect::Applied(applied) if applied.announce => Ok(Some(applied)),
        SubagentQuotaAutodetect::Applied(_) | SubagentQuotaAutodetect::NoMatch => Ok(None),
        SubagentQuotaAutodetect::Ambiguous => {
            eprintln!(
                "[agent-doc] subagent-end quota autodetect matched worker `{}` on more than one claim with the same start; no claim was changed.",
                signal.worker_id
            );
            Ok(None)
        }
    }
}

pub(crate) fn subagent_stop_from_payload(
    payload: &str,
) -> Result<(Value, Option<SubagentQuotaBlockApplied>)> {
    let value = match serde_json::from_str::<Value>(payload) {
        Ok(value) => value,
        Err(err) => {
            eprintln!(
                "[agent-doc] subagent-end hook payload is not JSON ({err}); allowing the stop."
            );
            return Ok((json!({}), None));
        }
    };
    let applied = observe_value(&value)?;
    let output = match &applied {
        Some(applied) => announced_hook_output(applied),
        None => json!({}),
    };
    Ok((output, applied))
}

pub fn handle_subagent_stop() -> Result<Option<SubagentQuotaBlockApplied>> {
    let payload = match super::read_stdin_payload() {
        Ok(payload) => payload,
        Err(err) => {
            eprintln!(
                "[agent-doc] subagent-end hook could not read its payload: {err:#}. Allowing the stop."
            );
            println!("{{}}");
            return Ok(None);
        }
    };
    let (output, applied) = match subagent_stop_from_payload(&payload) {
        Ok(result) => result,
        Err(err) => {
            eprintln!(
                "[agent-doc] subagent-end quota autodetect failed: {err:#}. Allowing the stop."
            );
            (json!({}), None)
        }
    };
    println!("{}", serde_json::to_string(&output)?);
    Ok(applied)
}

pub(crate) fn announced_hook_output(applied: &SubagentQuotaBlockApplied) -> Value {
    subagent_stop_output(&quota_blocked_next_step(
        &applied.file.display().to_string(),
        &applied.claim,
    ))
}

fn subagent_stop_output(step: &str) -> Value {
    json!({
        "hookSpecificOutput": {
            "hookEventName": "SubagentStop",
            "additionalContext": step,
        }
    })
}

fn worker_block(signal: &SubagentEnd, limit: &HarnessUsageLimit) -> WorkerQuotaBlock {
    WorkerQuotaBlock {
        harness: limit.harness.clone(),
        limit: limit.limit.clone(),
        evidence: limit.evidence.clone(),
        reset_hint: limit.reset_hint.clone(),
        worktree: signal.worktree.clone(),
        branch: signal.branch.clone(),
        blocked_at_secs: 0,
    }
}

fn scan_paths(signal: &SubagentEnd) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for path in [
        signal.cwd.as_deref(),
        signal.workspace_root.as_deref(),
        signal.transcript_path.as_deref(),
    ] {
        if let Some(path) = path {
            paths.push(PathBuf::from(path));
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        paths.push(cwd);
    }
    paths
}

fn preferred_session_doc(signal: &SubagentEnd, scan_paths: &[PathBuf]) -> Option<PathBuf> {
    let session = signal
        .parent_session_id
        .as_deref()
        .or(signal.session_id.as_deref())?;
    let mut roots = Vec::new();
    for path in scan_paths {
        for root in agent_doc_codex_hook_io::project_roots_for(path) {
            agent_doc_codex_hook_io::push_unique_root(&mut roots, root);
        }
    }
    let (_root, state) = agent_doc_codex_hook_io::load_state_any(&roots, session).ok()??;
    let path = PathBuf::from(state.doc_path);
    path.is_file().then_some(path)
}

fn read_transcript_tail(path: &Path) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let start = len.saturating_sub(TRANSCRIPT_TAIL_BYTES);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).ok()?;
    if start > 0
        && let Some(split) = buf.iter().position(|byte| *byte == b'\n')
    {
        buf.drain(..=split);
    }
    Some(String::from_utf8_lossy(&buf).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dual_spelling_payload_without_a_limit_allows_the_stop() {
        let payload = r#"{
            "hookEventName": "subagent_stop",
            "hook_event_name": "SubagentStop",
            "subagentId": "worker-sdk",
            "subagent_id": "worker-sdk",
            "subagentType": "general",
            "lastAssistantMessage": "Done.",
            "last_assistant_message": "Done."
        }"#;
        let (output, applied) = subagent_stop_from_payload(payload).unwrap();
        assert!(applied.is_none());
        assert_eq!(output, json!({}));
        assert!(output.get("decision").is_none());
    }

    #[test]
    fn a_limit_with_no_matching_claim_still_allows_the_stop() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".agent-doc")).unwrap();
        let payload = serde_json::json!({
            "hook_event_name": "SubagentStop",
            "agent_id": "worker-sdk",
            "last_assistant_message": "You've hit your session limit · resets 9pm",
            "cwd": dir.path(),
        })
        .to_string();
        let (output, applied) = subagent_stop_from_payload(&payload).unwrap();
        assert!(applied.is_none());
        assert_eq!(output, json!({}));
    }
}
