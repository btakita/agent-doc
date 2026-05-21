//! # Module: tsift_graph
//!
//! ## Spec
//! - Optional integration with a materialized tsift graph database.
//! - For queued `do #id` / `do [#id]` items, collect `tsift graph-db evidence`
//!   and `tsift conflict-matrix` JSON, then expose compact graph handles for
//!   planning and orchestration prompts.
//! - The integration is active only when an ancestor `.tsift/graph.db` exists.
//!   Once active, stale graph freshness or unresolved targets fail closed.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const TSIFT_BIN_ENV: &str = "AGENT_DOC_TSIFT_BIN";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TsiftGraphEvidencePlan {
    pub(crate) targets: Vec<String>,
    pub(crate) graph_db_status: TsiftGraphDbStatus,
    pub(crate) prompt_target_handles: Vec<TsiftPromptTargetHandle>,
    pub(crate) conflict_matrix: TsiftConflictMatrixSummary,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) next_commands: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TsiftGraphDbStatus {
    pub(crate) root: Option<String>,
    pub(crate) graph_db: Option<String>,
    pub(crate) status: String,
    pub(crate) content_hash: Option<String>,
    pub(crate) source_watermark: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) diagnostics: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TsiftPromptTargetHandle {
    pub(crate) prompt_target: String,
    pub(crate) target: String,
    pub(crate) evidence_packet_id: String,
    pub(crate) target_node_id: String,
    pub(crate) target_kind: String,
    pub(crate) target_label: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) worker_context_handles: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) source_handles: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) semantic_handles: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) next_commands: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TsiftConflictMatrixSummary {
    pub(crate) can_parallel: bool,
    pub(crate) fail_closed: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) evidence_packet_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) decisions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) worker_ownership_blocks: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) worker_prompt_packets: Vec<TsiftWorkerPromptPacket>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) next_commands: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TsiftWorkerPromptPacket {
    pub(crate) target: String,
    pub(crate) rank: usize,
    pub(crate) risk: String,
    pub(crate) title: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) owned_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) owned_symbols: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) read_only_context: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) forbidden_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) expected_tests: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) expansion_commands: Vec<String>,
}

#[derive(Debug, Serialize)]
struct TaskGraphPromptContext<'a> {
    target: &'a str,
    evidence_packet_id: &'a str,
    target_node_id: &'a str,
    target_kind: &'a str,
    target_label: &'a str,
    worker_context_handles: &'a [String],
    source_handles: &'a [String],
    semantic_handles: &'a [String],
    conflict_matrix: TaskGraphConflictContext<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    worker_prompt_packet: Option<&'a TsiftWorkerPromptPacket>,
}

#[derive(Debug, Serialize)]
struct TaskGraphConflictContext<'a> {
    can_parallel: bool,
    fail_closed: bool,
    evidence_packet_ids: &'a [String],
    decisions: &'a [String],
    worker_ownership_blocks: &'a [String],
}

pub(crate) fn collect_for_do_items(
    file: &Path,
    prompt_targets: &[String],
) -> Result<Option<TsiftGraphEvidencePlan>> {
    let do_items = collect_do_items(prompt_targets);
    if do_items.is_empty() || find_materialized_graph_db(file).is_none() {
        return Ok(None);
    }

    let file_arg = file.display().to_string();
    let status_json =
        run_tsift_json(&["graph-db", "--path", file_arg.as_str(), "--json", "status"])
            .context("running tsift graph-db status")?;
    let graph_db_status = parse_graph_db_status(&status_json);
    if fail_closed_freshness(&status_json) {
        anyhow::bail!(
            "tsift graph.db is not current for {}: {}",
            file.display(),
            freshness_diagnostics(&status_json).join("; ")
        );
    }

    let mut handles = Vec::new();
    for item in &do_items {
        let evidence = run_tsift_json(&[
            "graph-db",
            "--path",
            file_arg.as_str(),
            "--json",
            "evidence",
            &item.target,
            "--depth",
            "3",
            "--limit",
            "8",
        ])
        .with_context(|| format!("collecting tsift graph-db evidence for #{}", item.target))?;
        handles.push(parse_evidence_handle(
            &item.prompt_target,
            &item.target,
            &evidence,
        )?);
    }

    let mut conflict_args = vec![
        "conflict-matrix".to_string(),
        "--path".to_string(),
        file.display().to_string(),
        "--json".to_string(),
    ];
    conflict_args.extend(do_items.iter().map(|item| item.target.clone()));
    let conflict_refs = conflict_args.iter().map(String::as_str).collect::<Vec<_>>();
    let conflict_json = run_tsift_json(&conflict_refs).context("running tsift conflict-matrix")?;
    let conflict_matrix = parse_conflict_matrix(&conflict_json);

    let mut next_commands = BTreeSet::new();
    for command in string_array(status_json.pointer("/next_commands")) {
        next_commands.insert(command);
    }
    for handle in &handles {
        for command in &handle.next_commands {
            next_commands.insert(command.clone());
        }
    }
    for command in &conflict_matrix.next_commands {
        next_commands.insert(command.clone());
    }

    Ok(Some(TsiftGraphEvidencePlan {
        targets: do_items.into_iter().map(|item| item.target).collect(),
        graph_db_status,
        prompt_target_handles: handles,
        conflict_matrix,
        next_commands: next_commands.into_iter().collect(),
    }))
}

pub(crate) fn extract_do_target(text: &str) -> Option<String> {
    let mut normalized = text.trim().trim_start_matches('❯').trim();
    if normalized.starts_with('[')
        && let Some(closing) = normalized.find(']')
    {
        normalized = normalized[closing + 1..].trim();
    }
    let lower = normalized.to_ascii_lowercase();
    let rest = lower.strip_prefix("do ")?;
    let hash_idx = rest.find('#')?;
    let id_start = hash_idx + 1;
    let id: String = rest[id_start..]
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric())
        .collect();
    (!id.is_empty()).then_some(id)
}

impl TsiftGraphEvidencePlan {
    pub(crate) fn prompt_context_for_task(&self, task_label: &str) -> Result<Option<String>> {
        let Some(target) = extract_do_target(task_label) else {
            return Ok(None);
        };
        let Some(handle) = self
            .prompt_target_handles
            .iter()
            .find(|handle| handle.target.eq_ignore_ascii_case(&target))
        else {
            return Ok(None);
        };
        let worker_prompt_packet = self
            .conflict_matrix
            .worker_prompt_packets
            .iter()
            .find(|packet| packet.target.eq_ignore_ascii_case(&target));
        let context = TaskGraphPromptContext {
            target: &handle.target,
            evidence_packet_id: &handle.evidence_packet_id,
            target_node_id: &handle.target_node_id,
            target_kind: &handle.target_kind,
            target_label: &handle.target_label,
            worker_context_handles: &handle.worker_context_handles,
            source_handles: &handle.source_handles,
            semantic_handles: &handle.semantic_handles,
            conflict_matrix: TaskGraphConflictContext {
                can_parallel: self.conflict_matrix.can_parallel,
                fail_closed: self.conflict_matrix.fail_closed,
                evidence_packet_ids: &self.conflict_matrix.evidence_packet_ids,
                decisions: &self.conflict_matrix.decisions,
                worker_ownership_blocks: &self.conflict_matrix.worker_ownership_blocks,
            },
            worker_prompt_packet,
        };
        Ok(Some(format!(
            "<tsift_graph_evidence>\n{}\n</tsift_graph_evidence>",
            serde_json::to_string_pretty(&context)
                .context("serializing tsift graph prompt context")?
        )))
    }
}

#[derive(Debug, Clone)]
struct DoItem {
    prompt_target: String,
    target: String,
}

fn collect_do_items(prompt_targets: &[String]) -> Vec<DoItem> {
    let mut seen = BTreeSet::new();
    let mut items = Vec::new();
    for prompt_target in prompt_targets {
        let Some(target) = extract_do_target(prompt_target) else {
            continue;
        };
        if seen.insert(target.to_ascii_lowercase()) {
            items.push(DoItem {
                prompt_target: prompt_target.clone(),
                target,
            });
        }
    }
    items
}

fn run_tsift_json(args: &[&str]) -> Result<Value> {
    let bin = std::env::var(TSIFT_BIN_ENV).unwrap_or_else(|_| "tsift".to_string());
    let output = Command::new(&bin)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .with_context(|| format!("failed to launch {bin}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        anyhow::bail!(
            "`{}` exited with status {}: {}{}{}",
            std::iter::once(bin.as_str())
                .chain(args.iter().copied())
                .collect::<Vec<_>>()
                .join(" "),
            output.status,
            stderr.trim(),
            if stderr.trim().is_empty() || stdout.trim().is_empty() {
                ""
            } else {
                "\n"
            },
            stdout.trim()
        );
    }
    serde_json::from_slice(&output.stdout).with_context(|| {
        format!(
            "failed to parse tsift JSON from `{}`",
            args.to_vec().join(" ")
        )
    })
}

fn find_materialized_graph_db(file: &Path) -> Option<PathBuf> {
    let canonical = file.canonicalize().ok()?;
    let mut cursor = if canonical.is_dir() {
        canonical.as_path()
    } else {
        canonical.parent()?
    };
    loop {
        let graph_db = cursor.join(".tsift/graph.db");
        if graph_db.exists() {
            return Some(graph_db);
        }
        cursor = cursor.parent()?;
    }
}

fn parse_graph_db_status(value: &Value) -> TsiftGraphDbStatus {
    TsiftGraphDbStatus {
        root: string_at(value, "/root"),
        graph_db: string_at(value, "/graph_db"),
        status: string_at(value, "/freshness/status")
            .or_else(|| string_at(value, "/status"))
            .unwrap_or_else(|| "unknown".to_string()),
        content_hash: string_at(value, "/freshness/content_hash"),
        source_watermark: string_at(value, "/freshness/source_watermark"),
        diagnostics: freshness_diagnostics(value),
    }
}

fn fail_closed_freshness(value: &Value) -> bool {
    value
        .pointer("/freshness/fail_closed")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn freshness_diagnostics(value: &Value) -> Vec<String> {
    string_array(value.pointer("/freshness/diagnostics"))
}

fn parse_evidence_handle(
    prompt_target: &str,
    target: &str,
    value: &Value,
) -> Result<TsiftPromptTargetHandle> {
    if fail_closed_freshness(value) {
        anyhow::bail!(
            "tsift graph-db evidence for #{} failed closed: {}",
            target,
            freshness_diagnostics(value).join("; ")
        );
    }
    let target_node_id = string_at(value, "/target_node/id")
        .with_context(|| format!("tsift graph-db evidence target not found: {target}"))?;
    let target_kind = string_at(value, "/target_node/kind").unwrap_or_default();
    let target_label = string_at(value, "/target_node/label").unwrap_or_default();
    Ok(TsiftPromptTargetHandle {
        prompt_target: prompt_target.to_string(),
        target: target.to_string(),
        evidence_packet_id: format!("{target}:{target_node_id}"),
        target_node_id,
        target_kind,
        target_label,
        worker_context_handles: node_ids(value.pointer("/worker_context")),
        source_handles: node_ids(value.pointer("/source_handles")),
        semantic_handles: node_ids(value.pointer("/semantic_related")),
        next_commands: string_array(value.pointer("/next_commands")),
    })
}

fn parse_conflict_matrix(value: &Value) -> TsiftConflictMatrixSummary {
    TsiftConflictMatrixSummary {
        can_parallel: value
            .pointer("/can_parallel")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        fail_closed: value
            .pointer("/fail_closed")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        evidence_packet_ids: string_array(value.pointer("/orchestration/evidence_packet_ids")),
        decisions: string_array(value.pointer("/orchestration/conflict_matrix_decisions")),
        worker_ownership_blocks: string_array(
            value.pointer("/orchestration/worker_ownership_blocks"),
        ),
        worker_prompt_packets: parse_worker_prompt_packets(value.pointer("/worker_prompt_packets")),
        next_commands: string_array(value.pointer("/next_commands")),
    }
}

fn parse_worker_prompt_packets(value: Option<&Value>) -> Vec<TsiftWorkerPromptPacket> {
    value
        .and_then(Value::as_array)
        .map(|packets| {
            packets
                .iter()
                .map(|packet| TsiftWorkerPromptPacket {
                    target: string_at(packet, "/target").unwrap_or_default(),
                    rank: packet.pointer("/rank").and_then(Value::as_u64).unwrap_or(0) as usize,
                    risk: string_at(packet, "/risk").unwrap_or_default(),
                    title: string_at(packet, "/title").unwrap_or_default(),
                    owned_files: string_array(packet.pointer("/owned_files")),
                    owned_symbols: string_array(packet.pointer("/owned_symbols")),
                    read_only_context: string_array(packet.pointer("/read_only_context")),
                    forbidden_files: string_array(packet.pointer("/forbidden_files")),
                    expected_tests: string_array(packet.pointer("/expected_tests")),
                    expansion_commands: string_array(packet.pointer("/expansion_commands")),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn node_ids(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|nodes| {
            nodes
                .iter()
                .filter_map(|node| string_at(node, "/id"))
                .collect()
        })
        .unwrap_or_default()
}

fn string_at(value: &Value, pointer: &str) -> Option<String> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    struct EnvGuard {
        key: &'static str,
        prev: Option<String>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let lock = crate::harness_prompt::TEST_ENV_LOCK.lock().unwrap();
            let prev = std::env::var(key).ok();
            unsafe { std::env::set_var(key, value) };
            Self {
                key,
                prev,
                _lock: lock,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(value) = &self.prev {
                unsafe { std::env::set_var(self.key, value) };
            } else {
                unsafe { std::env::remove_var(self.key) };
            }
        }
    }

    fn setup_doc() -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".tsift")).unwrap();
        std::fs::write(dir.path().join(".tsift/graph.db"), "fake").unwrap();
        let doc = dir.path().join("tasks.md");
        std::fs::write(&doc, "# Tasks\n").unwrap();
        (dir, doc)
    }

    #[cfg(unix)]
    fn fake_tsift(dir: &Path, log: &Path, stale: bool) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let script = dir.join("fake-tsift.sh");
        let status = if stale {
            r#"{"root":"/tmp/repo","graph_db":"/tmp/repo/.tsift/graph.db","freshness":{"status":"stale","fail_closed":true,"content_hash":"old","source_watermark":"old","diagnostics":["graph.db is stale"]},"next_commands":["tsift graph-db --path /tmp/repo refresh --json"]}"#
        } else {
            r#"{"root":"/tmp/repo","graph_db":"/tmp/repo/.tsift/graph.db","freshness":{"status":"current","fail_closed":false,"content_hash":"abc","source_watermark":"abc","diagnostics":[]},"next_commands":["tsift graph-db --path /tmp/repo status --json"]}"#
        };
        let script_body = format!(
            r##"#!/bin/sh
printf '%s\n' "$*" >> "{}"
case "$*" in
  *"graph-db"*"--json status"*)
    cat <<'JSON'
{}
JSON
    ;;
  *"graph-db"*"--json evidence agbr"*)
    cat <<'JSON'
{{"root":"/tmp/repo","backend":"sqlite","target":"agbr","freshness":{{"status":"current","fail_closed":false,"diagnostics":[]}},"target_node":{{"id":"gbak-agbr","kind":"backlog","label":"#agbr"}},"worker_context":[{{"id":"wctx-agbr"}}],"source_handles":[{{"id":"src-agbr"}}],"semantic_related":[{{"id":"sem-agbr"}}],"next_commands":["tsift graph-db --path /tmp/repo evidence agbr --json"]}}
JSON
    ;;
  *"conflict-matrix"*)
    cat <<'JSON'
{{"targets":["agbr"],"can_parallel":true,"fail_closed":false,"orchestration":{{"evidence_packet_ids":["agbr:gbak-agbr"],"conflict_matrix_decisions":["candidate #1 agbr risk=low"],"worker_ownership_blocks":["Worker 1 owns agbr (#agbr)"]}},"worker_prompt_packets":[{{"target":"agbr","rank":1,"risk":"low","title":"Worker 1 owns agbr (#agbr)","owned_files":["tasks.md"],"owned_symbols":["Exchange"],"read_only_context":["src-agbr"],"forbidden_files":[],"expected_tests":["cargo test"],"expansion_commands":["tsift graph-db evidence agbr --json"]}}],"next_commands":["tsift conflict-matrix --path /tmp/repo agbr --json"]}}
JSON
    ;;
  *)
    echo "unexpected fake tsift args: $*" >&2
    exit 2
    ;;
esac
"##,
            log.display(),
            status
        );
        std::fs::write(&script, script_body).unwrap();
        let mut perms = std::fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).unwrap();
        script
    }

    #[test]
    fn extracts_do_targets_from_common_task_shapes() {
        assert_eq!(
            extract_do_target("do #agbr. spec"),
            Some("agbr".to_string())
        );
        assert_eq!(
            extract_do_target("do [#agbr]. spec"),
            Some("agbr".to_string())
        );
        assert_eq!(
            extract_do_target("[prep] do #agbr"),
            Some("agbr".to_string())
        );
        assert_eq!(extract_do_target("run tests"), None);
    }

    #[cfg(unix)]
    #[test]
    fn collect_for_do_items_attaches_graph_handles() {
        let (dir, doc) = setup_doc();
        let log = dir.path().join("calls.log");
        let fake = fake_tsift(dir.path(), &log, false);
        let _env = EnvGuard::set(TSIFT_BIN_ENV, fake.to_str().unwrap());

        let plan = collect_for_do_items(&doc, &["do [#agbr]. spec-test".to_string()])
            .unwrap()
            .unwrap();

        assert_eq!(plan.targets, vec!["agbr"]);
        assert_eq!(plan.graph_db_status.status, "current");
        assert_eq!(
            plan.prompt_target_handles[0].evidence_packet_id,
            "agbr:gbak-agbr"
        );
        assert_eq!(
            plan.conflict_matrix.evidence_packet_ids,
            vec!["agbr:gbak-agbr"]
        );
        let context = plan
            .prompt_context_for_task("do #agbr")
            .unwrap()
            .expect("expected prompt context");
        assert!(context.contains("<tsift_graph_evidence>"));
        assert!(context.contains("\"source_handles\": ["));
        assert!(context.contains("\"Worker 1 owns agbr (#agbr)\""));

        let calls = std::fs::read_to_string(log).unwrap();
        assert!(calls.contains("graph-db"));
        assert!(calls.contains("evidence agbr"));
        assert!(calls.contains("conflict-matrix"));
    }

    #[cfg(unix)]
    #[test]
    fn collect_for_do_items_fails_closed_on_stale_graph_db() {
        let (dir, doc) = setup_doc();
        let log = dir.path().join("calls.log");
        let fake = fake_tsift(dir.path(), &log, true);
        let _env = EnvGuard::set(TSIFT_BIN_ENV, fake.to_str().unwrap());

        let err = collect_for_do_items(&doc, &["do #agbr".to_string()])
            .unwrap_err()
            .to_string();

        assert!(err.contains("not current"));
        let calls = std::fs::read_to_string(log).unwrap();
        assert!(calls.contains("status"));
        assert!(!calls.contains("evidence agbr"));
        assert!(!calls.contains("conflict-matrix"));
    }
}
