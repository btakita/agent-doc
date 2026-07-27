//! Bounded tsift context-pack loading and durable prompt-injection projection.
//!
//! The tsift process is optional and deadline-bound. Once a cycle manifest has
//! been recorded, subsequent callers reconstruct compact handle references from
//! SQLite without launching tsift again.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use agent_doc_prompt_context::dynamic_context::{
    ComponentHash, ContextChunk, ContextInjectionRecord, DynamicContextProjection,
    InjectionLedgerSnapshot, InjectionMode, PromptTargetInputs,
};
use agent_doc_sqlite::context_injection_ledger::{
    ContextInjectionMode, ContextInjectionWrite, ContextLookupScope, ContextManifestWrite,
    StoredContextInjection, already_injected, context_injections_for_cycle,
    context_manifest_for_cycle, record_context_manifest,
};
use agent_doc_sqlite::state_store::Connection;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use wait_timeout::ChildExt;

const CONTRACT_VERSION: &str = "agent-doc-dynamic-context-manifest-v1";
const DEFAULT_TSIFT_TIMEOUT_MS: u64 = 2_000;
const MAX_TSIFT_REPORT_BYTES: u64 = 2 * 1024 * 1024;
const MAX_CANDIDATE_CHUNKS: usize = 8;
const MAX_EXPANDED_CHUNK_BYTES: usize = 1_200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DynamicContextIdentity {
    pub document_id: String,
    pub session_id: String,
    pub cycle_id: String,
    pub cycle_state: String,
    pub harness: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DynamicContextChunkManifest {
    pub pack_id: String,
    pub chunk_id: String,
    pub content_hash: String,
    pub source_uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range_start: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range_end: Option<usize>,
    pub token_count: usize,
    pub injection_mode: String,
    pub handle_reference: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expanded_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DynamicContextSnapshot {
    pub contract_version: String,
    pub status: String,
    pub document_id: String,
    pub session_id: String,
    pub cycle_id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pack_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub prompt_fingerprint: String,
    pub token_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chunks: Vec<DynamicContextChunkManifest>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<String>,
}

impl DynamicContextSnapshot {
    fn unavailable(identity: &DynamicContextIdentity, diagnostic: impl Into<String>) -> Self {
        Self {
            contract_version: CONTRACT_VERSION.to_string(),
            status: "unavailable".to_string(),
            document_id: identity.document_id.clone(),
            session_id: identity.session_id.clone(),
            cycle_id: identity.cycle_id.clone(),
            pack_ids: Vec::new(),
            prompt_fingerprint: String::new(),
            token_count: 0,
            chunks: Vec::new(),
            diagnostics: vec![diagnostic.into()],
        }
    }

    /// Render only first-use payloads. Every repeated or duplicate chunk is a
    /// compact, resolvable handle reference.
    pub fn as_prompt_section(&self) -> Option<String> {
        if self.chunks.is_empty() {
            return None;
        }
        let mut lines = vec![format!(
            "<dynamic_context contract=\"{}\" fingerprint=\"{}\" token_count=\"{}\">",
            CONTRACT_VERSION,
            escape_attribute(&self.prompt_fingerprint),
            self.token_count
        )];
        for chunk in &self.chunks {
            if let Some(text) = &chunk.expanded_text {
                lines.push(format!(
                    "<context_chunk handle=\"{}\" hash=\"{}\" source=\"{}\">",
                    escape_attribute(&chunk.handle_reference),
                    escape_attribute(&chunk.content_hash),
                    escape_attribute(&chunk.source_uri)
                ));
                lines.push(text.clone());
                lines.push("</context_chunk>".to_string());
            } else {
                lines.push(format!(
                    "<context_ref handle=\"{}\" hash=\"{}\" source=\"{}\" mode=\"{}\" />",
                    escape_attribute(&chunk.handle_reference),
                    escape_attribute(&chunk.content_hash),
                    escape_attribute(&chunk.source_uri),
                    escape_attribute(&chunk.injection_mode)
                ));
            }
        }
        lines.push("</dynamic_context>".to_string());
        Some(lines.join("\n"))
    }
}

#[derive(Debug, Clone)]
struct CandidatePayload {
    chunk: ContextChunk,
    text: String,
}

/// Build and record a dynamic-context snapshot for an active document cycle.
///
/// `Ok(None)` means the caller did not supply prompt targets or no open cycle
/// exists. Those are normal states, not context-provider failures.
pub fn build_dynamic_context_snapshot(
    file: &Path,
    document: &str,
    prompt_targets: &[String],
) -> Result<Option<DynamicContextSnapshot>> {
    if prompt_targets.is_empty() {
        return Ok(None);
    }
    let Some(identity) = identity_for_active_cycle(file, document)? else {
        return Ok(None);
    };
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let root = agent_doc_fs::find_project_root(&canonical)
        .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());
    let mut conn = agent_doc_sqlite::state_store::open_state_db_with_timeout(
        &root,
        Duration::from_millis(250),
    )
    .with_context(|| format!("open dynamic-context state for {}", file.display()))?;

    if context_manifest_for_cycle(&conn, &identity.document_id, &identity.cycle_id)?.is_some() {
        return Ok(Some(snapshot_from_stored_cycle(
            &conn,
            &identity,
            prompt_targets,
        )?));
    }

    if !root.join(".tsift/index.db").exists() {
        return Ok(Some(DynamicContextSnapshot::unavailable(
            &identity,
            "tsift context-pack skipped: project index is unavailable",
        )));
    }
    let report = match run_tsift_context_pack(file) {
        Ok(report) => report,
        Err(err) => {
            return Ok(Some(DynamicContextSnapshot::unavailable(
                &identity,
                format!("tsift context-pack unavailable: {err:#}"),
            )));
        }
    };
    let component_hashes = component_hashes(document);
    project_and_record_report(
        &mut conn,
        &identity,
        component_hashes,
        prompt_targets,
        &report,
    )
    .map(Some)
}

/// Project an already-loaded tsift report and atomically record its decisions.
///
/// This is public so non-process callers and tests can use the same boundary
/// without shelling out.
pub fn project_and_record_report(
    conn: &mut Connection,
    identity: &DynamicContextIdentity,
    component_hashes: Vec<ComponentHash>,
    prompt_targets: &[String],
    report: &Value,
) -> Result<DynamicContextSnapshot> {
    if context_manifest_for_cycle(conn, &identity.document_id, &identity.cycle_id)?.is_some() {
        return snapshot_from_stored_cycle(conn, identity, prompt_targets);
    }

    let candidates = candidate_payloads(report)?;
    let lookup_scope = ContextLookupScope::Session {
        document_id: &identity.document_id,
        session_id: &identity.session_id,
    };
    let mut prior = Vec::new();
    for candidate in &candidates {
        if let Some(existing) = already_injected(
            conn,
            lookup_scope,
            &candidate.chunk.chunk_id,
            &candidate.chunk.content_hash,
        )? {
            prior.push(ContextInjectionRecord {
                document_id: existing.injection.document_id,
                session_id: existing.injection.session_id,
                cycle_id: existing.injection.cycle_id,
                pack_id: existing.injection.pack_id,
                chunk_id: existing.injection.chunk_id,
                content_hash: existing.injection.content_hash,
            });
        }
    }

    let projection = DynamicContextProjection::new(
        component_hashes,
        PromptTargetInputs {
            plan_targets: prompt_targets.to_vec(),
            ..PromptTargetInputs::default()
        },
        candidates
            .iter()
            .map(|candidate| candidate.chunk.clone())
            .collect(),
        InjectionLedgerSnapshot { records: prior },
    );
    let rendered = projection.rendered_manifest();
    let payloads = candidates
        .into_iter()
        .map(|candidate| {
            (
                (
                    candidate.chunk.pack_id.clone(),
                    candidate.chunk.chunk_id.clone(),
                    candidate.chunk.content_hash.clone(),
                ),
                candidate.text,
            )
        })
        .collect::<HashMap<_, _>>();

    let mut pack_ids = rendered
        .decisions
        .iter()
        .map(|decision| decision.pack_id.clone())
        .collect::<Vec<_>>();
    pack_ids.sort();
    pack_ids.dedup();
    let writes = rendered
        .decisions
        .iter()
        .map(|decision| ContextInjectionWrite {
            pack_id: decision.pack_id.clone(),
            chunk_id: decision.chunk_id.clone(),
            content_hash: decision.content_hash.clone(),
            source_uri: decision.source_uri.clone(),
            range_start: decision
                .range_start
                .and_then(|value| i64::try_from(value).ok()),
            range_end: decision
                .range_end
                .and_then(|value| i64::try_from(value).ok()),
            injection_mode: sqlite_mode(decision.injection_mode),
        })
        .collect::<Vec<_>>();
    record_context_manifest(
        conn,
        &ContextManifestWrite {
            document_id: identity.document_id.clone(),
            session_id: identity.session_id.clone(),
            cycle_id: identity.cycle_id.clone(),
            cycle_state: identity.cycle_state.clone(),
            harness: identity.harness.clone(),
            prompt_fingerprint: rendered.prompt_fingerprint.clone(),
            pack_ids: pack_ids.clone(),
            token_count: i64::try_from(rendered.token_count)
                .context("dynamic-context token count overflow")?,
            injections: writes,
        },
    )?;

    let chunks = rendered
        .decisions
        .into_iter()
        .map(|decision| {
            let mode = decision.injection_mode;
            let key = (
                decision.pack_id.clone(),
                decision.chunk_id.clone(),
                decision.content_hash.clone(),
            );
            let expanded_text = (mode == InjectionMode::Expanded)
                .then(|| payloads.get(&key).cloned())
                .flatten();
            chunk_manifest(
                decision.pack_id,
                decision.chunk_id,
                decision.content_hash,
                decision.source_uri,
                decision.range_start,
                decision.range_end,
                decision.token_count,
                mode.as_str(),
                expanded_text,
            )
        })
        .collect();
    Ok(DynamicContextSnapshot {
        contract_version: CONTRACT_VERSION.to_string(),
        status: "recorded".to_string(),
        document_id: identity.document_id.clone(),
        session_id: identity.session_id.clone(),
        cycle_id: identity.cycle_id.clone(),
        pack_ids,
        prompt_fingerprint: rendered.prompt_fingerprint,
        token_count: rendered.token_count,
        chunks,
        diagnostics: Vec::new(),
    })
}

fn identity_for_active_cycle(
    file: &Path,
    document: &str,
) -> Result<Option<DynamicContextIdentity>> {
    let Some(cycle) = agent_doc_cycle_state_io::load(file)? else {
        return Ok(None);
    };
    if !cycle.phase.is_open() {
        return Ok(None);
    }
    let (frontmatter, _) = agent_doc_frontmatter_io::session::parse_for_file(document, file)
        .or_else(|_| agent_doc_frontmatter::frontmatter::parse(document))?;
    let Some(session_id) = frontmatter.session.filter(|value| !value.trim().is_empty()) else {
        return Ok(None);
    };
    Ok(Some(DynamicContextIdentity {
        document_id: agent_doc_hash::document_id_for_path(file),
        session_id,
        cycle_id: cycle.cycle_id,
        cycle_state: cycle.phase.as_str().to_string(),
        harness: frontmatter.agent.unwrap_or_else(|| "unknown".to_string()),
    }))
}

fn component_hashes(document: &str) -> Vec<ComponentHash> {
    let mut hashes = agent_doc_element::element::parse(document)
        .map(|components| {
            components
                .into_iter()
                .map(|component| ComponentHash {
                    name: component.name.clone(),
                    content_hash: agent_doc_hash::content_hash(component.content(document)),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if hashes.is_empty() {
        hashes.push(ComponentHash {
            name: "document".to_string(),
            content_hash: agent_doc_hash::content_hash(document),
        });
    }
    hashes
}

fn run_tsift_context_pack(file: &Path) -> Result<Value> {
    let bin = std::env::var("AGENT_DOC_TSIFT_BIN").unwrap_or_else(|_| "tsift".to_string());
    let timeout_ms = std::env::var("AGENT_DOC_TSIFT_CONTEXT_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(DEFAULT_TSIFT_TIMEOUT_MS)
        .max(100);
    let mut stdout = tempfile::tempfile().context("create tsift stdout spool")?;
    let mut stderr = tempfile::tempfile().context("create tsift stderr spool")?;
    let mut child = Command::new(&bin)
        .arg("context-pack")
        .arg(file)
        .args([
            "--json",
            "--budget",
            "normal",
            "--max-items",
            "4",
            "--max-bytes",
            "512",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::from(
            stdout.try_clone().context("clone tsift stdout spool")?,
        ))
        .stderr(Stdio::from(
            stderr.try_clone().context("clone tsift stderr spool")?,
        ))
        .spawn()
        .with_context(|| format!("launch `{bin} context-pack`"))?;
    let Some(status) = child
        .wait_timeout(Duration::from_millis(timeout_ms))
        .context("wait for tsift context-pack")?
    else {
        let _ = child.kill();
        let _ = child.wait();
        bail!("deadline exceeded after {timeout_ms}ms");
    };
    if !status.success() {
        let detail = read_spool(&mut stderr, 512)?;
        bail!(
            "exited with {status}{}",
            if detail.trim().is_empty() {
                String::new()
            } else {
                format!(": {}", detail.trim())
            }
        );
    }
    let bytes = read_spool_bytes(&mut stdout, MAX_TSIFT_REPORT_BYTES + 1)?;
    if bytes.len() as u64 > MAX_TSIFT_REPORT_BYTES {
        bail!(
            "report exceeded the {} byte safety cap",
            MAX_TSIFT_REPORT_BYTES
        );
    }
    serde_json::from_slice(&bytes).context("parse tsift context-pack JSON")
}

fn read_spool(file: &mut File, limit: u64) -> Result<String> {
    Ok(String::from_utf8_lossy(&read_spool_bytes(file, limit)?).into_owned())
}

fn read_spool_bytes(file: &mut File, limit: u64) -> Result<Vec<u8>> {
    file.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    file.take(limit).read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn candidate_payloads(report: &Value) -> Result<Vec<CandidatePayload>> {
    let target = report
        .get("target")
        .and_then(Value::as_str)
        .unwrap_or("context-pack");
    let root = report.get("root").and_then(Value::as_str).unwrap_or("");
    let pack_id = format!(
        "tspack-{}",
        agent_doc_hash::short_content_hash(&format!("{root}\0{target}"))
    );
    let mut values = Vec::<Value>::new();
    if let Some(next_context) = report.get("next_context") {
        let compact = json!({
            "prompt_targets": next_context.get("prompt_targets"),
            "touched_files": next_context.get("touched_files"),
            "unresolved_failures": next_context.get("unresolved_failures"),
            "next_token_actions": next_context.get("next_token_actions"),
        });
        values.push(compact);
    }
    if let Some(queue) = report.get("agent_doc_queue") {
        values.push(json!({
            "active_queue_prompt": queue.get("active_queue_prompt"),
            "expansion_handles": queue.get("expansion_handles"),
        }));
    }
    for pointer in ["/exploration/worker_context", "/exploration/source_windows"] {
        if let Some(items) = report.pointer(pointer).and_then(Value::as_array) {
            values.extend(items.iter().cloned());
        }
    }
    values.truncate(MAX_CANDIDATE_CHUNKS);
    if values.is_empty() {
        values.push(json!({
            "target": report.get("target"),
            "target_kind": report.get("target_kind"),
            "status_reminders": report.get("status_reminders"),
        }));
    }

    values
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            let full_text = serde_json::to_string(&value).context("serialize context chunk")?;
            let content_hash = agent_doc_hash::content_hash(&full_text);
            let text = truncate_utf8(&full_text, MAX_EXPANDED_CHUNK_BYTES);
            let handle = value
                .get("handle")
                .and_then(Value::as_str)
                .map(sanitize_handle)
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| format!("section-{}", index + 1));
            let chunk_id = format!("{}-{}", handle, &content_hash[..12]);
            let source_uri = value
                .get("file")
                .or_else(|| value.get("target"))
                .and_then(Value::as_str)
                .unwrap_or(target)
                .to_string();
            let range_start = value
                .get("start")
                .and_then(Value::as_u64)
                .and_then(|value| usize::try_from(value).ok());
            let range_end = value
                .get("end")
                .and_then(Value::as_u64)
                .and_then(|value| usize::try_from(value).ok());
            Ok(CandidatePayload {
                chunk: ContextChunk {
                    pack_id: pack_id.clone(),
                    chunk_id,
                    content_hash,
                    source_uri,
                    range_start,
                    range_end,
                    token_count: text.len().div_ceil(4),
                    stale: false,
                },
                text,
            })
        })
        .collect()
}

fn snapshot_from_stored_cycle(
    conn: &Connection,
    identity: &DynamicContextIdentity,
    _prompt_targets: &[String],
) -> Result<DynamicContextSnapshot> {
    let manifest = context_manifest_for_cycle(conn, &identity.document_id, &identity.cycle_id)?
        .context("dynamic-context manifest disappeared during cycle lookup")?;
    let chunks = context_injections_for_cycle(conn, &identity.document_id, &identity.cycle_id)?
        .into_iter()
        .map(stored_chunk_manifest)
        .collect();
    Ok(DynamicContextSnapshot {
        contract_version: CONTRACT_VERSION.to_string(),
        status: "reused_cycle_manifest".to_string(),
        document_id: identity.document_id.clone(),
        session_id: identity.session_id.clone(),
        cycle_id: identity.cycle_id.clone(),
        pack_ids: manifest.pack_ids,
        prompt_fingerprint: manifest.prompt_fingerprint,
        token_count: usize::try_from(manifest.token_count).unwrap_or_default(),
        chunks,
        diagnostics: Vec::new(),
    })
}

fn stored_chunk_manifest(stored: StoredContextInjection) -> DynamicContextChunkManifest {
    let mode = stored.injection_mode.as_str();
    chunk_manifest(
        stored.pack_id,
        stored.chunk_id,
        stored.content_hash,
        stored.source_uri,
        stored
            .range_start
            .and_then(|value| usize::try_from(value).ok()),
        stored
            .range_end
            .and_then(|value| usize::try_from(value).ok()),
        0,
        mode,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn chunk_manifest(
    pack_id: String,
    chunk_id: String,
    content_hash: String,
    source_uri: String,
    range_start: Option<usize>,
    range_end: Option<usize>,
    token_count: usize,
    injection_mode: &str,
    expanded_text: Option<String>,
) -> DynamicContextChunkManifest {
    let handle_reference = format!("tsift://{pack_id}/{chunk_id}");
    DynamicContextChunkManifest {
        pack_id,
        chunk_id,
        content_hash,
        source_uri,
        range_start,
        range_end,
        token_count,
        injection_mode: injection_mode.to_string(),
        handle_reference,
        expanded_text,
    }
}

fn sqlite_mode(mode: InjectionMode) -> ContextInjectionMode {
    match mode {
        InjectionMode::Expanded => ContextInjectionMode::Expanded,
        InjectionMode::Referenced => ContextInjectionMode::Referenced,
        InjectionMode::SkippedDuplicate => ContextInjectionMode::SkippedDuplicate,
        InjectionMode::StaleIgnored => ContextInjectionMode::StaleIgnored,
    }
}

fn sanitize_handle(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn truncate_utf8(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &value[..end])
}

fn escape_attribute(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(cycle_id: &str) -> DynamicContextIdentity {
        DynamicContextIdentity {
            document_id: "doc-1".to_string(),
            session_id: "session-1".to_string(),
            cycle_id: cycle_id.to_string(),
            cycle_state: "preflight_started".to_string(),
            harness: "codex".to_string(),
        }
    }

    fn report(summary: &str) -> Value {
        json!({
            "root": "/repo",
            "target": "session.md",
            "next_context": {
                "prompt_targets": [summary],
                "touched_files": ["src/lib.rs"],
                "unresolved_failures": []
            },
            "exploration": {
                "worker_context": [{
                    "handle": "worker-main",
                    "target": "session.md",
                    "summary": summary,
                    "expand": "tsift source-read src/lib.rs"
                }]
            }
        })
    }

    fn components() -> Vec<ComponentHash> {
        vec![ComponentHash {
            name: "exchange".to_string(),
            content_hash: "component-hash".to_string(),
        }]
    }

    #[test]
    fn first_use_expands_and_second_cycle_references_existing_handles() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = agent_doc_sqlite::state_store::open_state_db(dir.path()).unwrap();
        let first = project_and_record_report(
            &mut conn,
            &identity("cycle-1"),
            components(),
            &["do work".to_string()],
            &report("do work"),
        )
        .unwrap();
        assert!(
            first
                .chunks
                .iter()
                .all(|chunk| chunk.injection_mode == "expanded")
        );
        assert!(
            first
                .chunks
                .iter()
                .all(|chunk| chunk.expanded_text.is_some())
        );

        let second = project_and_record_report(
            &mut conn,
            &identity("cycle-2"),
            components(),
            &["do work".to_string()],
            &report("do work"),
        )
        .unwrap();
        assert!(
            second
                .chunks
                .iter()
                .all(|chunk| chunk.injection_mode == "referenced")
        );
        assert!(
            second
                .chunks
                .iter()
                .all(|chunk| chunk.expanded_text.is_none())
        );
        assert!(second.as_prompt_section().unwrap().contains("<context_ref"));
    }

    #[test]
    fn changed_source_hash_expands_and_records_a_new_chunk_hash() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = agent_doc_sqlite::state_store::open_state_db(dir.path()).unwrap();
        let first = project_and_record_report(
            &mut conn,
            &identity("cycle-1"),
            components(),
            &["do work".to_string()],
            &report("old source"),
        )
        .unwrap();
        let changed = project_and_record_report(
            &mut conn,
            &identity("cycle-2"),
            components(),
            &["do work".to_string()],
            &report("new source"),
        )
        .unwrap();
        assert!(
            changed
                .chunks
                .iter()
                .all(|chunk| chunk.injection_mode == "expanded")
        );
        assert_ne!(first.chunks[0].content_hash, changed.chunks[0].content_hash);
        let stored = context_injections_for_cycle(&conn, "doc-1", "cycle-2").unwrap();
        assert_eq!(stored[0].content_hash, changed.chunks[0].content_hash);
    }

    #[test]
    fn same_cycle_reuses_manifest_without_reexpanding_payloads() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = agent_doc_sqlite::state_store::open_state_db(dir.path()).unwrap();
        project_and_record_report(
            &mut conn,
            &identity("cycle-1"),
            components(),
            &["do work".to_string()],
            &report("source"),
        )
        .unwrap();
        let retry = project_and_record_report(
            &mut conn,
            &identity("cycle-1"),
            components(),
            &["do work".to_string()],
            &report("source"),
        )
        .unwrap();
        assert_eq!(retry.status, "reused_cycle_manifest");
        assert!(
            retry
                .chunks
                .iter()
                .all(|chunk| chunk.expanded_text.is_none())
        );
    }
}
