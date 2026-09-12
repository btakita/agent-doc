//! Bounded tsift context-pack loading and durable prompt-injection projection.
//!
//! The tsift process is optional and deadline-bound. Once a cycle manifest has
//! been recorded, subsequent callers reconstruct compact handle references from
//! SQLite without launching tsift again.

use std::collections::{BTreeMap, HashMap};
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
    ClearedContextRows, ContextClearScope, ContextInjectionMode, ContextInjectionWrite,
    ContextLookupScope, ContextManifestWrite, StoredContextInjection, already_injected,
    clear_context_scope, context_injections_for_cycle, context_manifest_for_cycle,
    latest_context_manifest_for_session, record_context_manifest,
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
    #[serde(default)]
    pub expansion_command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expanded_text: Option<String>,
}

impl DynamicContextChunkManifest {
    /// Whether this chunk names a command that actually re-resolves it.
    ///
    /// `#ctxrefsynthsource`: a chunk agent-doc synthesized from the cycle's own
    /// report has no file behind it and no durable text in `state.db` (the
    /// injection ledger records identity, never payload), so nothing can read it
    /// back. Saying so is the honest answer; the alternative it replaces was a
    /// `tsift source-read` of the session document, which returns the document.
    pub fn is_expandable(&self) -> bool {
        !self.expansion_command.is_empty()
    }
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
        self.as_orchestration_child_section(0, 1)
    }

    /// Render durable context identity without replaying any expanded payload.
    ///
    /// Compaction and document transfer use this projection so a later agent
    /// can resolve the exact tsift handles while SQLite remains authoritative
    /// for prior injection decisions.
    pub fn as_reference_section(&self) -> Option<String> {
        if self.chunks.is_empty() {
            return None;
        }
        let mut mode_counts = BTreeMap::<&str, usize>::new();
        for chunk in &self.chunks {
            *mode_counts.entry(&chunk.injection_mode).or_default() += 1;
        }
        let modes = mode_counts
            .into_iter()
            .map(|(mode, count)| format!("{mode}:{count}"))
            .collect::<Vec<_>>()
            .join(",");
        let mut lines = vec![format!(
            "<dynamic_context_ref contract=\"{}\" session=\"{}\" cycle=\"{}\" fingerprint=\"{}\" token_count=\"{}\" modes=\"{}\">",
            CONTRACT_VERSION,
            escape_attribute(&self.session_id),
            escape_attribute(&self.cycle_id),
            escape_attribute(&self.prompt_fingerprint),
            self.token_count,
            escape_attribute(&modes),
        )];
        for chunk in &self.chunks {
            lines.push(format!(
                "<context_ref handle=\"{}\" hash=\"{}\" source=\"{}\" mode=\"{}\" {} />",
                escape_attribute(&chunk.handle_reference),
                escape_attribute(&chunk.content_hash),
                escape_attribute(&chunk.source_uri),
                escape_attribute(&chunk.injection_mode),
                expansion_attribute(chunk),
            ));
        }
        lines.push("</dynamic_context_ref>".to_string());
        Some(lines.join("\n"))
    }

    /// Render one orchestration child view of this manifest.
    ///
    /// Every child receives every durable handle. First-use excerpts are
    /// deterministically partitioned across children, so a parallel fan-out
    /// shares context identity without copying the same payload into every
    /// worktree prompt.
    pub fn as_orchestration_child_section(
        &self,
        child_index: usize,
        child_count: usize,
    ) -> Option<String> {
        if self.chunks.is_empty() {
            return None;
        }
        let child_count = child_count.max(1);
        let child_index = child_index.min(child_count - 1);
        let mut lines = vec![format!(
            "<dynamic_context contract=\"{}\" fingerprint=\"{}\" token_count=\"{}\" child=\"{}/{}\">",
            CONTRACT_VERSION,
            escape_attribute(&self.prompt_fingerprint),
            self.token_count,
            child_index + 1,
            child_count
        )];
        for (chunk_index, chunk) in self.chunks.iter().enumerate() {
            let owns_expansion = chunk_index % child_count == child_index;
            if let Some(text) = chunk.expanded_text.as_ref().filter(|_| owns_expansion) {
                lines.push(format!(
                    "<context_chunk handle=\"{}\" hash=\"{}\" source=\"{}\" {}>",
                    escape_attribute(&chunk.handle_reference),
                    escape_attribute(&chunk.content_hash),
                    escape_attribute(&chunk.source_uri),
                    expansion_attribute(chunk),
                ));
                lines.push(text.clone());
                lines.push("</context_chunk>".to_string());
            } else {
                lines.push(format!(
                    "<context_ref handle=\"{}\" hash=\"{}\" source=\"{}\" mode=\"{}\" {} />",
                    escape_attribute(&chunk.handle_reference),
                    escape_attribute(&chunk.content_hash),
                    escape_attribute(&chunk.source_uri),
                    escape_attribute(&chunk.injection_mode),
                    expansion_attribute(chunk),
                ));
            }
        }
        lines.push("</dynamic_context>".to_string());
        Some(lines.join("\n"))
    }
}

/// Scheme for a chunk agent-doc synthesized from the cycle's own report rather
/// than read out of a file (`#ctxrefsynthsource`).
const SYNTHESIZED_SOURCE_SCHEME: &str = "agent-doc://";

/// Render the expansion half of a context reference.
///
/// A chunk that can be re-read names the command; one that cannot says so
/// outright rather than carrying a command that returns something else.
fn expansion_attribute(chunk: &DynamicContextChunkManifest) -> String {
    if chunk.is_expandable() {
        return format!(
            "expand=\"{}\"",
            escape_attribute(&chunk.expansion_command)
        );
    }
    "expandable=\"false\"".to_string()
}

const CONTEXT_REFERENCE_OPEN: &str = "<dynamic_context_ref";
const CONTEXT_REFERENCE_CLOSE: &str = "</dynamic_context_ref>";

/// True when a recorded injection points at the session document itself.
///
/// `#fixcompactexchange`: the tsift pack for a session document always contains
/// that document, usually twice (once by absolute path, once relative to the
/// project root). Writing "re-read the file you are already reading" handles into
/// that same file is noise, not continuity, so they are dropped from every
/// document-facing reference projection. SQLite still holds the full manifest.
/// Whether a recorded injection is worth carrying into the session document.
///
/// Two shapes are not. A handle naming the document itself says "re-read the file
/// you are already reading" (`#fixcompactexchange`). A handle with no expansion
/// command cannot be resolved by anyone (`#ctxrefsynthsource`) — it used to be
/// indistinguishable from the first, because a synthesized chunk was labelled with
/// the session document; now that it names itself, the reason it is dropped is the
/// accurate one rather than a path coincidence.
fn chunk_is_actionable_continuity(
    chunk: &DynamicContextChunkManifest,
    document: &Path,
    root: &Path,
) -> bool {
    chunk.is_expandable() && !chunk_source_is_document(&chunk.source_uri, document, root)
}

fn chunk_source_is_document(source_uri: &str, document: &Path, root: &Path) -> bool {
    let candidate = Path::new(source_uri);
    let candidate = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        root.join(candidate)
    };
    let candidate = candidate.canonicalize().unwrap_or(candidate);
    candidate == document
}

/// Remove every rendered `<dynamic_context_ref>` block from component text.
///
/// `#fixcompactexchange`: a compact that carries an earlier preamble forward must
/// drop the reference block that preamble already holds, or each compact appends
/// another copy of it and the operator's exchange accretes identical manifest XML
/// without bound. Callers strip first, then append the current manifest, so the
/// document holds exactly zero or one block.
pub fn strip_context_reference_blocks(content: &str) -> String {
    if !content.contains(CONTEXT_REFERENCE_OPEN) {
        return content.to_string();
    }
    let mut out = String::with_capacity(content.len());
    let mut inside = false;
    for line in content.lines() {
        let trimmed = line.trim_start();
        if inside {
            if trimmed.starts_with(CONTEXT_REFERENCE_CLOSE) {
                inside = false;
            }
            continue;
        }
        if trimmed.starts_with(CONTEXT_REFERENCE_OPEN) {
            // A one-line rendering closes on the same line.
            if !trimmed.contains(CONTEXT_REFERENCE_CLOSE) {
                inside = true;
            }
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    if inside {
        // Unterminated block: everything after the opener was manifest metadata.
        return out;
    }
    out
}

/// Load the latest durable context manifest for this document session and
/// render it as handle-only continuity metadata.
pub fn durable_context_reference_for_document(
    file: &Path,
    document: &str,
) -> Result<Option<String>> {
    let Some(session_id) = session_id_for_document(file, document)? else {
        return Ok(None);
    };
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let root = project_root_for_document(&canonical);
    if !agent_doc_sqlite::state_store::state_db_path(&root).is_file() {
        return Ok(None);
    }
    let conn = agent_doc_sqlite::state_store::open_state_db_with_timeout(
        &root,
        Duration::from_millis(250),
    )
    .with_context(|| format!("open dynamic-context state for {}", file.display()))?;
    let document_id = agent_doc_hash::document_id_for_path(file);
    let Some(manifest) = latest_context_manifest_for_session(&conn, &document_id, &session_id)?
    else {
        return Ok(None);
    };
    let chunks: Vec<DynamicContextChunkManifest> =
        context_injections_for_cycle(&conn, &document_id, &manifest.cycle_id)?
            .into_iter()
            .map(stored_chunk_manifest)
            .filter(|chunk| chunk_is_actionable_continuity(chunk, &canonical, &root))
            .collect();
    if chunks.is_empty() {
        return Ok(None);
    }
    Ok(DynamicContextSnapshot {
        contract_version: CONTRACT_VERSION.to_string(),
        status: "durable_session_reference".to_string(),
        document_id,
        session_id,
        cycle_id: manifest.cycle_id,
        pack_ids: manifest.pack_ids,
        prompt_fingerprint: manifest.prompt_fingerprint,
        token_count: usize::try_from(manifest.token_count).unwrap_or_default(),
        chunks,
        diagnostics: Vec::new(),
    }
    .as_reference_section())
}

/// Reset only the dynamic-context injection memory for an explicit successful
/// session clear. Cycle lifecycle rows remain owned by the turn state machine.
pub fn clear_durable_context_session(file: &Path, session_id: &str) -> Result<ClearedContextRows> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let root = project_root_for_document(&canonical);
    if !agent_doc_sqlite::state_store::state_db_path(&root).is_file() {
        return Ok(ClearedContextRows::default());
    }
    let mut conn = agent_doc_sqlite::state_store::open_state_db_with_timeout(
        &root,
        Duration::from_millis(250),
    )
    .with_context(|| format!("open dynamic-context state for {}", file.display()))?;
    clear_context_scope(
        &mut conn,
        ContextClearScope::Session {
            document_id: &agent_doc_hash::document_id_for_path(file),
            session_id,
        },
    )
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
    let root = project_root_for_document(&canonical);
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
    let Some(session_id) = session_id_for_document(file, document)? else {
        return Ok(None);
    };
    let (frontmatter, _) = agent_doc_frontmatter_io::session::parse_for_file(document, file)
        .or_else(|_| agent_doc_frontmatter::frontmatter::parse(document))?;
    Ok(Some(DynamicContextIdentity {
        document_id: agent_doc_hash::document_id_for_path(file),
        session_id,
        cycle_id: cycle.cycle_id,
        cycle_state: cycle.phase.as_str().to_string(),
        harness: frontmatter.agent.unwrap_or_else(|| "unknown".to_string()),
    }))
}

fn session_id_for_document(file: &Path, document: &str) -> Result<Option<String>> {
    let (frontmatter, _) = agent_doc_frontmatter_io::session::parse_for_file(document, file)
        .or_else(|_| agent_doc_frontmatter::frontmatter::parse(document))?;
    Ok(frontmatter.session.filter(|value| !value.trim().is_empty()))
}

fn project_root_for_document(file: &Path) -> std::path::PathBuf {
    agent_doc_fs::find_project_root(file)
        .unwrap_or_else(|| file.parent().unwrap_or(Path::new(".")).to_path_buf())
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
    // `#ctxrefsynthsource`: a synthesized chunk is built FROM the report, not read
    // out of a file, so it carries its own source identity. Falling through to the
    // pack target labelled it with the session document and emitted
    // `expand="tsift --envelope source-read <the document>"` — a command that
    // returns the document instead of the chunk. Exploration items keep `None`
    // because they carry a real `file`/`target` of their own.
    let mut values = Vec::<(Option<&'static str>, Value)>::new();
    if let Some(next_context) = report.get("next_context") {
        let compact = json!({
            "prompt_targets": next_context.get("prompt_targets"),
            "touched_files": next_context.get("touched_files"),
            "unresolved_failures": next_context.get("unresolved_failures"),
            "next_token_actions": next_context.get("next_token_actions"),
        });
        values.push((Some("agent-doc://cycle/next-context"), compact));
    }
    if let Some(queue) = report.get("agent_doc_queue") {
        values.push((
            Some("agent-doc://cycle/agent-doc-queue"),
            json!({
                "active_queue_prompt": queue.get("active_queue_prompt"),
                "expansion_handles": queue.get("expansion_handles"),
            }),
        ));
    }
    for pointer in ["/exploration/worker_context", "/exploration/source_windows"] {
        if let Some(items) = report.pointer(pointer).and_then(Value::as_array) {
            values.extend(items.iter().cloned().map(|item| (None, item)));
        }
    }
    values.truncate(MAX_CANDIDATE_CHUNKS);
    if values.is_empty() {
        values.push((
            Some("agent-doc://cycle/report-summary"),
            json!({
                "target": report.get("target"),
                "target_kind": report.get("target_kind"),
                "status_reminders": report.get("status_reminders"),
            }),
        ));
    }

    values
        .into_iter()
        .enumerate()
        .map(|(index, (synthesized_source, value))| {
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
            let source_uri = synthesized_source.map(str::to_string).unwrap_or_else(|| {
                value
                    .get("file")
                    .or_else(|| value.get("target"))
                    .and_then(Value::as_str)
                    .unwrap_or(target)
                    .to_string()
            });
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
    let expansion_command = source_read_command(&source_uri, range_start, range_end);
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
        expansion_command,
        expanded_text,
    }
}

fn source_read_command(
    source_uri: &str,
    range_start: Option<usize>,
    range_end: Option<usize>,
) -> String {
    // `#ctxrefsynthsource`: there is no file to read and the injection ledger
    // records identity without payload, so nothing can resolve this chunk after
    // the cycle that built it. An empty command renders as `expandable="false"`.
    if source_uri.starts_with(SYNTHESIZED_SOURCE_SCHEME) {
        return String::new();
    }
    let source = shell_quote(source_uri);
    match (range_start, range_end) {
        (Some(start), Some(end)) if end >= start => format!(
            "tsift --envelope source-read {source} --start {start} --lines {} --budget normal",
            end.saturating_sub(start).saturating_add(1)
        ),
        _ => format!("tsift --envelope source-read {source} --budget normal"),
    }
}

fn shell_quote(value: &str) -> String {
    if !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-' | b':')
        })
    {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
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
                    // A worker window points at a source file, not at the session
                    // document — a document-sourced handle is dropped from every
                    // document-facing projection (`#fixcompactexchange`).
                    "target": "src/worker.rs",
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

    #[test]
    fn parallel_children_share_handles_without_duplicate_expanded_text() {
        let snapshot = DynamicContextSnapshot {
            contract_version: CONTRACT_VERSION.to_string(),
            status: "recorded".to_string(),
            document_id: "doc".to_string(),
            session_id: "session".to_string(),
            cycle_id: "cycle".to_string(),
            pack_ids: vec!["pack".to_string()],
            prompt_fingerprint: "fingerprint".to_string(),
            token_count: 4,
            chunks: vec![
                chunk_manifest(
                    "pack".to_string(),
                    "chunk-a".to_string(),
                    "hash-a".to_string(),
                    "src/a.rs".to_string(),
                    Some(10),
                    Some(12),
                    2,
                    "expanded",
                    Some("unique excerpt a".to_string()),
                ),
                chunk_manifest(
                    "pack".to_string(),
                    "chunk-b".to_string(),
                    "hash-b".to_string(),
                    "src/b.rs".to_string(),
                    None,
                    None,
                    2,
                    "expanded",
                    Some("unique excerpt b".to_string()),
                ),
            ],
            diagnostics: Vec::new(),
        };
        let first = snapshot.as_orchestration_child_section(0, 2).unwrap();
        let second = snapshot.as_orchestration_child_section(1, 2).unwrap();
        for handle in ["tsift://pack/chunk-a", "tsift://pack/chunk-b"] {
            assert!(first.contains(handle));
            assert!(second.contains(handle));
        }
        let combined = format!("{first}\n{second}");
        assert_eq!(combined.matches("unique excerpt a").count(), 1);
        assert_eq!(combined.matches("unique excerpt b").count(), 1);
        assert!(combined.contains("--start 10 --lines 3"));
    }

    #[test]
    fn queue_continuation_child_context_references_prior_cycle_without_reexpansion() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = agent_doc_sqlite::state_store::open_state_db(dir.path()).unwrap();
        project_and_record_report(
            &mut conn,
            &identity("cycle-1"),
            components(),
            &["first queue head".to_string()],
            &report("shared source"),
        )
        .unwrap();
        let continuation = project_and_record_report(
            &mut conn,
            &identity("cycle-2"),
            components(),
            &["next queue head".to_string()],
            &report("shared source"),
        )
        .unwrap();
        let prompt = continuation.as_orchestration_child_section(0, 1).unwrap();
        assert!(prompt.contains("<context_ref"));
        assert!(!prompt.contains("\"summary\":\"shared source\""));
        assert!(prompt.contains("tsift://"));
        assert!(prompt.contains("expand=\"tsift --envelope source-read"));
    }

    #[test]
    fn durable_reference_survives_reopen_and_never_replays_expanded_payload() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("session.md");
        let document = concat!(
            "---\n",
            "agent_doc_session: session-1\n",
            "agent: codex\n",
            "---\n\n",
            "# Session\n"
        );
        std::fs::write(&file, document).unwrap();
        let mut first_identity = identity("cycle-1");
        first_identity.document_id = agent_doc_hash::document_id_for_path(&file);
        let mut conn = agent_doc_sqlite::state_store::open_state_db(dir.path()).unwrap();
        project_and_record_report(
            &mut conn,
            &first_identity,
            components(),
            &["do work".to_string()],
            &report("expanded secret payload"),
        )
        .unwrap();
        drop(conn);

        let reference = durable_context_reference_for_document(&file, document)
            .unwrap()
            .unwrap();
        assert!(reference.contains("<dynamic_context_ref"));
        assert!(reference.contains("<context_ref"));
        assert!(reference.contains("tsift://"));
        assert!(reference.contains("modes=\"expanded:"));
        assert!(!reference.contains("expanded secret payload"));
        assert!(!reference.contains("<context_chunk"));
        // The synthesized `next_context` chunk inherits the report `target`, i.e. the
        // session document itself, so it is filtered out (`#fixcompactexchange`).
        assert!(reference.contains("source=\"src/worker.rs\""), "{reference}");
        assert!(!reference.contains("source=\"session.md\""), "{reference}");
    }

    /// `#fixcompactexchange`: a tsift pack for a session document contains that
    /// document, typically twice (absolute path and project-root-relative). Writing
    /// "re-read the file you are already reading" handles into that same file is noise.
    #[test]
    fn durable_reference_drops_handles_that_point_at_the_document_itself() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("session.md");
        let document = concat!(
            "---\n",
            "agent_doc_session: session-1\n",
            "agent: codex\n",
            "---\n\n",
            "# Session\n"
        );
        std::fs::write(&file, document).unwrap();
        let canonical = file.canonicalize().unwrap();
        let mut conn = agent_doc_sqlite::state_store::open_state_db(dir.path()).unwrap();
        record_context_manifest(
            &mut conn,
            &ContextManifestWrite {
                document_id: agent_doc_hash::document_id_for_path(&file),
                session_id: "session-1".to_string(),
                cycle_id: "cycle-1".to_string(),
                cycle_state: "preflight_started".to_string(),
                harness: "codex".to_string(),
                prompt_fingerprint: "fingerprint".to_string(),
                pack_ids: vec!["pack".to_string()],
                token_count: 7,
                injections: vec![
                    // Absolute path to the document under compaction.
                    ContextInjectionWrite {
                        pack_id: "pack".to_string(),
                        chunk_id: "chunk-self-abs".to_string(),
                        content_hash: "hash-self-abs".to_string(),
                        source_uri: canonical.display().to_string(),
                        range_start: None,
                        range_end: None,
                        injection_mode: ContextInjectionMode::Expanded,
                    },
                    // Project-root-relative path to the same document.
                    ContextInjectionWrite {
                        pack_id: "pack".to_string(),
                        chunk_id: "chunk-self-rel".to_string(),
                        content_hash: "hash-self-rel".to_string(),
                        source_uri: "session.md".to_string(),
                        range_start: None,
                        range_end: None,
                        injection_mode: ContextInjectionMode::Expanded,
                    },
                    ContextInjectionWrite {
                        pack_id: "pack".to_string(),
                        chunk_id: "chunk-real".to_string(),
                        content_hash: "hash-real".to_string(),
                        source_uri: "src/real.rs".to_string(),
                        range_start: Some(1),
                        range_end: Some(40),
                        injection_mode: ContextInjectionMode::Expanded,
                    },
                ],
            },
        )
        .unwrap();
        drop(conn);

        let reference = durable_context_reference_for_document(&file, document)
            .unwrap()
            .unwrap();
        assert!(reference.contains("chunk-real"), "real handle dropped: {reference}");
        assert!(
            !reference.contains("chunk-self-abs"),
            "absolute self-handle survived: {reference}"
        );
        assert!(
            !reference.contains("chunk-self-rel"),
            "relative self-handle survived: {reference}"
        );
        assert!(reference.contains("modes=\"expanded:1\""), "{reference}");
    }

    /// `#ctxrefsynthsource`: a chunk agent-doc builds from the cycle's own report
    /// has no file behind it. Falling through to the pack target labelled it with
    /// the session document and emitted
    /// `expand="tsift --envelope source-read <the document>"` — a command that
    /// returns the document instead of the chunk.
    #[test]
    fn a_synthesized_chunk_names_itself_and_declares_no_expansion() {
        let report = json!({
            "target": "/home/dev/repo/tasks/session.md",
            "root": "/home/dev/repo",
            "next_context": {
                "prompt_targets": ["do #x"],
                "touched_files": ["src/a.rs"],
                "unresolved_failures": [],
                "next_token_actions": [],
            },
            "agent_doc_queue": {
                "active_queue_prompt": "do #x",
                "expansion_handles": [],
            },
        });

        let payloads = candidate_payloads(&report).unwrap();
        let sources: Vec<&str> = payloads
            .iter()
            .map(|payload| payload.chunk.source_uri.as_str())
            .collect();
        assert_eq!(
            sources,
            vec![
                "agent-doc://cycle/next-context",
                "agent-doc://cycle/agent-doc-queue"
            ],
            "a synthesized chunk must not borrow the session document's identity"
        );
        for payload in &payloads {
            assert!(
                source_read_command(&payload.chunk.source_uri, None, None).is_empty(),
                "a synthesized chunk must not claim a source-read command: {}",
                payload.chunk.source_uri
            );
        }
    }

    /// A real file window keeps its own source and its working `source-read`; the
    /// fix must not make every chunk unexpandable.
    #[test]
    fn an_exploration_window_keeps_its_file_source_and_expansion() {
        let report = json!({
            "target": "/home/dev/repo/tasks/session.md",
            "root": "/home/dev/repo",
            "exploration": {
                "source_windows": [{"file": "src/real.rs", "start": 10, "end": 20}],
            },
        });

        let payloads = candidate_payloads(&report).unwrap();
        assert_eq!(payloads.len(), 1);
        assert_eq!(payloads[0].chunk.source_uri, "src/real.rs");
        assert_eq!(
            source_read_command(&payloads[0].chunk.source_uri, Some(10), Some(20)),
            "tsift --envelope source-read src/real.rs --start 10 --lines 11 --budget normal"
        );
    }

    /// The rendered reference must say which of the two it is, so a reader never
    /// runs a command that resolves to something other than the chunk.
    #[test]
    fn a_reference_renders_expandable_false_instead_of_a_command_that_lies() {
        let expandable = chunk_manifest(
            "pack".to_string(),
            "chunk-real".to_string(),
            "hash-real".to_string(),
            "src/real.rs".to_string(),
            None,
            None,
            4,
            "expanded",
            None,
        );
        let synthesized = chunk_manifest(
            "pack".to_string(),
            "chunk-next".to_string(),
            "hash-next".to_string(),
            "agent-doc://cycle/next-context".to_string(),
            None,
            None,
            4,
            "expanded",
            None,
        );
        assert!(expandable.is_expandable());
        assert!(!synthesized.is_expandable());
        assert_eq!(
            expansion_attribute(&expandable),
            "expand=\"tsift --envelope source-read src/real.rs --budget normal\""
        );
        assert_eq!(expansion_attribute(&synthesized), "expandable=\"false\"");
    }

    /// `#fixcompactexchange`: a manifest that holds nothing but self-references has no
    /// continuity value, so no block is written at all.
    #[test]
    fn durable_reference_is_absent_when_every_handle_is_the_document_itself() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("session.md");
        let document = "---\nagent_doc_session: session-1\nagent: codex\n---\n\n# Session\n";
        std::fs::write(&file, document).unwrap();
        let mut conn = agent_doc_sqlite::state_store::open_state_db(dir.path()).unwrap();
        record_context_manifest(
            &mut conn,
            &ContextManifestWrite {
                document_id: agent_doc_hash::document_id_for_path(&file),
                session_id: "session-1".to_string(),
                cycle_id: "cycle-1".to_string(),
                cycle_state: "preflight_started".to_string(),
                harness: "codex".to_string(),
                prompt_fingerprint: "fingerprint".to_string(),
                pack_ids: vec!["pack".to_string()],
                token_count: 7,
                injections: vec![ContextInjectionWrite {
                    pack_id: "pack".to_string(),
                    chunk_id: "chunk-self".to_string(),
                    content_hash: "hash-self".to_string(),
                    source_uri: "session.md".to_string(),
                    range_start: None,
                    range_end: None,
                    injection_mode: ContextInjectionMode::Expanded,
                }],
            },
        )
        .unwrap();
        drop(conn);

        assert!(
            durable_context_reference_for_document(&file, document)
                .unwrap()
                .is_none()
        );
    }

    /// `#fixcompactexchange`: strip is what keeps a carried-forward preamble from
    /// accreting one identical manifest block per compact.
    #[test]
    fn stripping_context_reference_blocks_keeps_surrounding_prose() {
        let content = concat!(
            "### Session Summary\n\n",
            "Compacted content:\n",
            "- Archived 3 response topic(s): a; b; c\n\n",
            "<dynamic_context_ref contract=\"v1\" session=\"s\" cycle=\"c1\" ",
            "fingerprint=\"f\" token_count=\"7\" modes=\"expanded:1\">\n",
            "<context_ref handle=\"tsift://pack/one\" hash=\"h\" source=\"a.rs\" ",
            "mode=\"expanded\" expand=\"tsift --envelope source-read a.rs\" />\n",
            "</dynamic_context_ref>\n\n",
            "<dynamic_context_ref contract=\"v1\" session=\"s\" cycle=\"c2\" ",
            "fingerprint=\"f\" token_count=\"7\" modes=\"expanded:1\">\n",
            "<context_ref handle=\"tsift://pack/two\" hash=\"h\" source=\"b.rs\" ",
            "mode=\"expanded\" expand=\"tsift --envelope source-read b.rs\" />\n",
            "</dynamic_context_ref>\n",
            "trailing prose\n",
        );

        let stripped = strip_context_reference_blocks(content);
        assert!(!stripped.contains("dynamic_context_ref"), "{stripped}");
        assert!(!stripped.contains("context_ref"), "{stripped}");
        assert!(!stripped.contains("tsift://"), "{stripped}");
        assert!(stripped.contains("### Session Summary"));
        assert!(stripped.contains("- Archived 3 response topic(s): a; b; c"));
        assert!(stripped.contains("trailing prose"));
    }

    /// An unterminated block must not leave its `<context_ref />` lines behind.
    #[test]
    fn stripping_an_unterminated_context_reference_block_drops_its_tail() {
        let content = concat!(
            "prose before\n",
            "<dynamic_context_ref contract=\"v1\" session=\"s\" cycle=\"c\" ",
            "fingerprint=\"f\" token_count=\"7\" modes=\"expanded:1\">\n",
            "<context_ref handle=\"tsift://pack/one\" hash=\"h\" source=\"a.rs\" ",
            "mode=\"expanded\" expand=\"x\" />\n",
        );

        let stripped = strip_context_reference_blocks(content);
        assert_eq!(stripped, "prose before\n");
    }

    /// Content with no manifest block is returned byte-identical.
    #[test]
    fn stripping_content_without_a_context_reference_block_is_a_no_op() {
        let content = "### Session Summary\n\nCompacted content:\n- Archived 1 topic\n";
        assert_eq!(strip_context_reference_blocks(content), content);
    }

    #[test]
    fn explicit_session_clear_starts_a_fresh_injection_scope() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("session.md");
        let document = concat!(
            "---\n",
            "agent_doc_session: session-1\n",
            "agent: codex\n",
            "---\n\n",
            "# Session\n"
        );
        std::fs::write(&file, document).unwrap();
        let mut first_identity = identity("cycle-1");
        first_identity.document_id = agent_doc_hash::document_id_for_path(&file);
        let mut conn = agent_doc_sqlite::state_store::open_state_db(dir.path()).unwrap();
        project_and_record_report(
            &mut conn,
            &first_identity,
            components(),
            &["do work".to_string()],
            &report("shared source"),
        )
        .unwrap();
        drop(conn);

        let cleared = clear_durable_context_session(&file, "session-1").unwrap();
        assert_eq!(cleared.manifests, 1);
        assert!(cleared.injections > 0);
        assert!(
            durable_context_reference_for_document(&file, document)
                .unwrap()
                .is_none()
        );

        let mut second_identity = identity("cycle-2");
        second_identity.document_id = agent_doc_hash::document_id_for_path(&file);
        let mut reopened = agent_doc_sqlite::state_store::open_state_db(dir.path()).unwrap();
        let fresh = project_and_record_report(
            &mut reopened,
            &second_identity,
            components(),
            &["do work".to_string()],
            &report("shared source"),
        )
        .unwrap();
        assert!(
            fresh
                .chunks
                .iter()
                .all(|chunk| chunk.injection_mode == "expanded")
        );
    }
}
