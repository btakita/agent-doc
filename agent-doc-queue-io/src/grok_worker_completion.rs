//! Read exact native Grok worker history through the coordinator's retained
//! subagent store. The claim remains the lifecycle authority; a native result
//! is an observation for the existing response-evidence projection.

use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use agent_doc_queue::queue_claim::{
    QueueClaim, SUBAGENT_END_HANDBACK_REASON_PREFIX, hand_back_defers_result,
    hand_back_matches_recorded_end, subagent_end_freeze_reason,
};
use agent_doc_queue::subagent_brief::SUBAGENT_PREAMBLE_MARKER;
use serde::Deserialize;
use serde::de::DeserializeOwned;

use super::HistoryResponse;

const MAX_ARTIFACT_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Deserialize)]
struct NativeWorker {
    subagent_id: String,
    parent_session_id: String,
    child_session_id: String,
    prompt: String,
    status: String,
    started_at: String,
    completed_at: Option<String>,
}

#[derive(Deserialize)]
struct NativeOutput {
    schema_version: u32,
    output: String,
}

pub(super) fn recover(
    sessions: &Path,
    claim: &QueueClaim,
    parent_id: &str,
    worker_id: &str,
) -> HistoryResponse {
    use HistoryResponse::{Ambiguous, Missing, Recovered, ScopeMismatch, Unsupported};

    if claim.generation == 0
        || !super::codex_thread_id_is_safe(parent_id)
        || !super::codex_thread_id_is_safe(worker_id)
    {
        return Unsupported;
    }
    let Ok(workspaces) = fs::read_dir(sessions) else {
        return Missing;
    };
    // Grok partitions sessions by workspace. Locate the exact recorded parent
    // and worker one level below the partition, never a child in another parent.
    let mut candidates = workspaces.filter_map(|workspace| {
        let workspace = workspace.ok()?;
        if !workspace.file_type().ok()?.is_dir() {
            return None;
        }
        let parent = workspace.path().join(parent_id);
        let subagents = parent.join("subagents");
        let worker = subagents.join(worker_id);
        (is_directory(&parent) && is_directory(&subagents) && is_directory(&worker))
            .then_some(worker)
    });
    let Some(path) = candidates.next() else {
        return Missing;
    };
    let terminal = |path: &Path| {
        let (meta, bytes) = read_json::<NativeWorker>(&path.join("meta.json"))?;
        (meta.parent_session_id == parent_id
            && meta.subagent_id == worker_id
            && meta.child_session_id == worker_id
            && meta.status == "completed"
            && terminal_for_attempt(&meta, claim.claimed_at_secs))
        .then_some((meta, bytes))
    };
    let retained = terminal(&path);
    if let Some(other) = candidates.next() {
        let has_terminal = retained.is_some()
            || terminal(&other).is_some()
            || candidates.any(|path| terminal(&path).is_some());
        return if has_terminal { Ambiguous } else { Missing };
    }
    let Some((meta, metadata_bytes)) = retained else {
        return Missing;
    };
    let Some(item) = dispatched_item(&meta.prompt) else {
        // A prose summary or a reference to a detached prompt file is not a
        // durable attestation of the prompt Grok actually received.
        return Unsupported;
    };
    if item != claim.item_text.trim_end() {
        return ScopeMismatch;
    }
    let Some((output, _)) = read_json::<NativeOutput>(&path.join("output.json")) else {
        return Missing;
    };
    if output.schema_version != 1
        || output.output.trim().is_empty()
        || hand_back_defers_result(&output.output)
        || !subagent_end_freeze_reason(Some(&output.output))
            .starts_with(SUBAGENT_END_HANDBACK_REASON_PREFIX)
        || claim
            .ended_hand_back_owed()
            .is_some_and(|excerpt| !hand_back_matches_recorded_end(excerpt, &output.output))
        || read_artifact(&path.join("meta.json")).as_deref() != Some(metadata_bytes.as_slice())
    {
        return Missing;
    }
    Recovered(output.output)
}

fn is_directory(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir())
}

fn read_artifact(path: &Path) -> Option<Vec<u8>> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_ARTIFACT_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    File::open(path)
        .ok()?
        .take(MAX_ARTIFACT_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() as u64 <= MAX_ARTIFACT_BYTES).then_some(bytes)
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Option<(T, Vec<u8>)> {
    let bytes = read_artifact(path)?;
    let result = serde_json::from_slice(&bytes).ok()?;
    Some((result, bytes))
}

fn terminal_for_attempt(meta: &NativeWorker, claimed_at_secs: u64) -> bool {
    let Some(started_at) = grok_timestamp(&meta.started_at) else {
        return false;
    };
    meta.completed_at
        .as_deref()
        .and_then(grok_timestamp)
        .is_some_and(|completed_at| claimed_at_secs <= started_at && started_at <= completed_at)
}

fn grok_timestamp(timestamp: &str) -> Option<u64> {
    let utc = timestamp.strip_suffix('Z')?;
    let seconds = if let Some((seconds, fractional)) = utc.split_once('.') {
        if fractional.is_empty() || !fractional.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        seconds
    } else {
        utc
    };
    agent_doc_log_time::parse_log_timestamp(seconds)
}

fn dispatched_item(prompt: &str) -> Option<String> {
    let preamble = prompt.trim_start().strip_prefix(SUBAGENT_PREAMBLE_MARKER)?;
    let (_, quoted) = preamble.split_once("\nQueue item (verbatim):\n")?;
    let item = quoted
        .lines()
        .map_while(|line| {
            if line == ">" {
                Some("")
            } else {
                line.strip_prefix("> ")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    (!item.trim().is_empty()).then_some(item)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_queue::queue_claim::QueueClaimLedger;
    use serde_json::{Value, json};

    const PARENT: &str = "parent-session";
    const WORKER: &str = "worker-session";
    const ITEM: &str = "Review feature 12: #review";
    const RESPONSE: &str = "Reviewed the exact head.\n\nVerification is complete.";

    fn fixture(workspace: &Path, output: &str) -> (std::path::PathBuf, Value) {
        let dir = workspace.join(PARENT).join("subagents").join(WORKER);
        fs::create_dir_all(&dir).unwrap();
        let meta = json!({
            "subagent_id": WORKER,
            "parent_session_id": PARENT,
            "child_session_id": WORKER,
            "status": "completed",
            "started_at": "1970-01-01T00:00:12.412Z",
            "completed_at": "1970-01-01T00:00:20.509Z",
            "prompt": format!("{SUBAGENT_PREAMBLE_MARKER}\n\nQueue item (verbatim):\n> {ITEM}\n\nScope."),
        });
        fs::write(dir.join("meta.json"), meta.to_string()).unwrap();
        fs::write(
            dir.join("output.json"),
            json!({"schema_version": 1, "output": output}).to_string(),
        )
        .unwrap();
        (dir, meta)
    }

    fn claim() -> QueueClaim {
        let mut ledger = QueueClaimLedger::default();
        ledger.claim(ITEM, "subagent:review", 10, 600);
        ledger.claims.remove(0)
    }

    #[test]
    fn accepts_exact_completed_worker_from_the_claiming_grok_session() {
        let sessions = tempfile::tempdir().unwrap();
        let (path, mut meta) = fixture(&sessions.path().join("workspace"), RESPONSE);
        let claim = claim();
        let observe = || recover(sessions.path(), &claim, PARENT, WORKER);
        assert_eq!(observe(), HistoryResponse::Recovered(RESPONSE.to_string()));

        // The same native worker can be mistakenly registered against a leaf
        // with the same name under another parent.
        meta["prompt"] = json!(format!(
            "{SUBAGENT_PREAMBLE_MARKER}\n\nQueue item (verbatim):\n> Review feature 99: #review\n"
        ));
        fs::write(path.join("meta.json"), meta.to_string()).unwrap();
        assert_eq!(observe(), HistoryResponse::ScopeMismatch);

        meta["prompt"] = json!("Read the prompt from /tmp/brief.md.");
        fs::write(path.join("meta.json"), meta.to_string()).unwrap();
        assert_eq!(observe(), HistoryResponse::Unsupported);
    }

    #[test]
    fn refuses_nonterminal_stale_foreign_or_ambiguous_artifacts() {
        let sessions = tempfile::tempdir().unwrap();
        let (path, terminal) = fixture(&sessions.path().join("workspace-one"), RESPONSE);
        let claim = claim();
        let observe = || recover(sessions.path(), &claim, PARENT, WORKER);
        for (field, value) in [
            ("status", json!("running")),
            ("completed_at", Value::Null),
            ("started_at", json!("1970-01-01T00:00:09.423Z")),
            ("parent_session_id", json!("other-parent")),
            ("child_session_id", json!("other-worker")),
        ] {
            let mut wrong = terminal.clone();
            wrong[field] = value;
            fs::write(path.join("meta.json"), wrong.to_string()).unwrap();
            assert_eq!(observe(), HistoryResponse::Missing, "{field}");
        }
        fs::write(path.join("meta.json"), terminal.to_string()).unwrap();
        fixture(&sessions.path().join("workspace-two"), RESPONSE);
        assert_eq!(observe(), HistoryResponse::Ambiguous);
    }

    #[test]
    fn a_waiting_wrapper_is_not_a_worker_result() {
        let sessions = tempfile::tempdir().unwrap();
        fixture(
            &sessions.path().join("workspace"),
            "Task child-task is still running. Use /codex:status before continuing it.",
        );
        assert_eq!(
            recover(sessions.path(), &claim(), PARENT, WORKER),
            HistoryResponse::Missing
        );
    }
}
