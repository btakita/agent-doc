//! Pure IPC protocol classification policy for agent-doc.
//!
//! This crate owns reusable decisions about plugin IPC payloads. It does not
//! create sockets, read or write files, spawn listener threads, or mutate
//! documents.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::fmt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallbackRequest {
    pub doc_path: String,
    pub doc_hash: String,
    pub operations: Vec<String>,
    pub context: Option<String>,
    pub created_at: u64,
    pub ttl_secs: u64,
    pub request_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallbackPatch {
    pub component: String,
    pub mode: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallbackResponse {
    pub request_id: String,
    pub status: String,
    pub summary: String,
    pub details: Option<String>,
    pub patches: Option<Vec<CallbackPatch>>,
    pub completed_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingCallback {
    pub doc_path: String,
    pub operations: Vec<String>,
    pub urgency: String,
}

pub fn callback_request(
    doc_path: impl Into<String>,
    doc_hash: impl Into<String>,
    operations: impl IntoIterator<Item = impl Into<String>>,
    context: Option<impl Into<String>>,
    created_at: u64,
    ttl_secs: u64,
    request_id: impl Into<String>,
) -> CallbackRequest {
    CallbackRequest {
        doc_path: doc_path.into(),
        doc_hash: doc_hash.into(),
        operations: operations.into_iter().map(Into::into).collect(),
        context: context.map(Into::into),
        created_at,
        ttl_secs,
        request_id: request_id.into(),
    }
}

pub fn callback_response(
    request_id: impl Into<String>,
    status: impl Into<String>,
    summary: impl Into<String>,
    details: Option<impl Into<String>>,
    patches: Option<Vec<CallbackPatch>>,
    completed_at: u64,
) -> CallbackResponse {
    CallbackResponse {
        request_id: request_id.into(),
        status: status.into(),
        summary: summary.into(),
        details: details.map(Into::into),
        patches,
        completed_at,
    }
}

pub fn callback_request_is_expired(request: &CallbackRequest, now: u64) -> bool {
    now.saturating_sub(request.created_at) > request.ttl_secs
}

pub fn callback_urgency_for_elapsed(elapsed_secs: u64) -> &'static str {
    if elapsed_secs < 10 {
        "high"
    } else if elapsed_secs < 60 {
        "normal"
    } else {
        "low"
    }
}

pub fn pending_callback_from_request(request: CallbackRequest, now: u64) -> PendingCallback {
    PendingCallback {
        doc_path: request.doc_path,
        operations: request.operations,
        urgency: callback_urgency_for_elapsed(now.saturating_sub(request.created_at)).to_string(),
    }
}

pub fn callback_response_matches_request(
    response: &CallbackResponse,
    request: Option<&CallbackRequest>,
) -> bool {
    request
        .map(|request| response.request_id == request.request_id)
        .unwrap_or(true)
}

/// Classification of a plugin-sent IPC receipt line.
///
/// Maintained plugins return a JSON receipt after applying a patch. `Applied`
/// means the patch reached the editor buffer. `AlreadyApplied` means the plugin
/// detected the response body is already present and chose not to re-apply it.
/// `Rejected` covers explicit terminal apply rejection. `Unsupported` covers
/// legacy ACK lines and malformed/unknown responses so old plugins fail closed
/// with an update/reinstall error instead of seeding compatibility proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketReceiptClassification {
    Applied,
    AlreadyApplied,
    Rejected,
    /// Early `accepted`/`observed` receipt: the listener received the patch but has
    /// not applied it yet. Liveness only; the sender must keep waiting for a
    /// terminal receipt.
    Pending,
    Unsupported,
}

/// Classify a plugin-sent IPC receipt line.
pub fn classify_socket_receipt(receipt: &str) -> SocketReceiptClassification {
    let Some(value) = serde_json::from_str::<serde_json::Value>(receipt).ok() else {
        return SocketReceiptClassification::Unsupported;
    };
    let Some(receipt_type) = value.get("type").and_then(|t| t.as_str()) else {
        return SocketReceiptClassification::Unsupported;
    };
    if !receipt_type.eq_ignore_ascii_case("receipt") {
        return SocketReceiptClassification::Unsupported;
    }
    let status = value.get("status").and_then(|s| s.as_str());
    let reason = value
        .get("reason")
        .and_then(|r| r.as_str())
        .map(|r| r.to_ascii_lowercase());
    if reason.as_deref() == Some("already_applied") {
        return SocketReceiptClassification::AlreadyApplied;
    }
    if let Some(s) = status
        && (s.eq_ignore_ascii_case("pending")
            || s.eq_ignore_ascii_case("observed")
            || s.eq_ignore_ascii_case("accepted"))
    {
        return SocketReceiptClassification::Pending;
    }
    match status {
        Some(s) if s.eq_ignore_ascii_case("applied") => SocketReceiptClassification::Applied,
        Some(s) if s.eq_ignore_ascii_case("rejected") => SocketReceiptClassification::Rejected,
        _ => SocketReceiptClassification::Unsupported,
    }
}

/// True when a socket-send error message reports an `already_applied` receipt.
pub fn is_already_applied_receipt_error_message(message: &str) -> bool {
    message.starts_with("IPC receipt already_applied")
}

pub fn is_socket_receipt_timeout_error(message: impl AsRef<str>) -> bool {
    // Duration-agnostic: the sender's receipt timeout budget is configurable, so
    // match the stable prefix rather than a hard-coded "(2s)".
    message.as_ref().contains("IPC receipt timeout")
}

pub fn is_socket_status_error(message: impl AsRef<str>) -> bool {
    message.as_ref().contains("IPC receipt rejected")
        || message.as_ref().contains("IPC receipt unsupported")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IpcSnapshotSource {
    LegacySidecarProjection,
    LazilyVisibleWriteEvent,
    ContentOurs,
    FileRead,
}

impl IpcSnapshotSource {
    pub const fn label(self) -> &'static str {
        match self {
            Self::LegacySidecarProjection => "legacy_sidecar_projection",
            Self::LazilyVisibleWriteEvent => "lazily_visible_write_event",
            Self::ContentOurs => "content_ours",
            Self::FileRead => "file_read",
        }
    }

    pub const fn is_visible_write_proven(self) -> bool {
        matches!(self, Self::LazilyVisibleWriteEvent)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IpcDiskRepairReason {
    PrefixDivergence,
    IpcDedupe,
    PrefixDivergenceThenIpcDedupe,
    LivePromptDrift,
}

impl IpcDiskRepairReason {
    pub const fn label(self) -> &'static str {
        match self {
            Self::PrefixDivergence => "prefix_divergence",
            Self::IpcDedupe => "ipc_dedupe",
            Self::PrefixDivergenceThenIpcDedupe => "prefix_divergence_then_ipc_dedupe",
            Self::LivePromptDrift => "live_prompt_drift",
        }
    }

    pub const fn redelivery_kind(self) -> FullContentRepairRedelivery {
        match self {
            Self::PrefixDivergence => FullContentRepairRedelivery::NormalizationFallback,
            Self::IpcDedupe | Self::PrefixDivergenceThenIpcDedupe => {
                FullContentRepairRedelivery::IpcDedupe
            }
            Self::LivePromptDrift => FullContentRepairRedelivery::LivePromptDrift,
        }
    }

    pub const fn merge_with_ipc_dedupe(self) -> Self {
        match self {
            Self::PrefixDivergence => Self::PrefixDivergenceThenIpcDedupe,
            Self::IpcDedupe | Self::PrefixDivergenceThenIpcDedupe | Self::LivePromptDrift => self,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EditorBadStateFingerprint {
    pub content: String,
    pub len: usize,
    pub hash: String,
}

impl EditorBadStateFingerprint {
    pub fn new(content: String) -> Self {
        let len = content.len();
        let hash = agent_doc_hash::content_hash(&content);
        Self { content, len, hash }
    }

    pub fn content(&self) -> &str {
        &self.content
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IpcRepairDecision {
    pub snapshot_content: String,
    pub snap_source: IpcSnapshotSource,
    pub disk_repair_reason: Option<IpcDiskRepairReason>,
    pub editor_bad_state: Option<EditorBadStateFingerprint>,
    pub normalize_prefix_lines: Vec<String>,
    pub redeliver_editor: bool,
}

impl IpcRepairDecision {
    pub fn lazily_visible_write(snapshot_content: String) -> Self {
        Self {
            snapshot_content,
            snap_source: IpcSnapshotSource::LazilyVisibleWriteEvent,
            disk_repair_reason: None,
            editor_bad_state: None,
            normalize_prefix_lines: Vec::new(),
            redeliver_editor: false,
        }
    }

    pub fn content_ours(snapshot_content: String) -> Self {
        Self {
            snapshot_content,
            snap_source: IpcSnapshotSource::ContentOurs,
            disk_repair_reason: None,
            editor_bad_state: None,
            normalize_prefix_lines: Vec::new(),
            redeliver_editor: false,
        }
    }

    fn prefix_repair(
        snapshot_content: String,
        bad_state: String,
        normalize_prefix_lines: &[String],
        snap_source: IpcSnapshotSource,
    ) -> Self {
        Self {
            snapshot_content,
            snap_source,
            disk_repair_reason: Some(IpcDiskRepairReason::PrefixDivergence),
            editor_bad_state: Some(EditorBadStateFingerprint::new(bad_state)),
            normalize_prefix_lines: normalize_prefix_lines.to_vec(),
            redeliver_editor: true,
        }
    }

    pub fn lazily_visible_write_prefix_repair(
        snapshot_content: String,
        bad_state: String,
        normalize_prefix_lines: &[String],
    ) -> Self {
        Self::prefix_repair(
            snapshot_content,
            bad_state,
            normalize_prefix_lines,
            IpcSnapshotSource::LazilyVisibleWriteEvent,
        )
    }

    pub fn file_read_prefix_repair(
        snapshot_content: String,
        bad_state: String,
        normalize_prefix_lines: &[String],
    ) -> Self {
        Self::prefix_repair(
            snapshot_content,
            bad_state,
            normalize_prefix_lines,
            IpcSnapshotSource::FileRead,
        )
    }

    pub fn file_read(snapshot_content: String) -> Self {
        Self {
            snapshot_content,
            snap_source: IpcSnapshotSource::FileRead,
            disk_repair_reason: None,
            editor_bad_state: None,
            normalize_prefix_lines: Vec::new(),
            redeliver_editor: false,
        }
    }

    pub fn apply_ipc_dedupe(
        mut self,
        snapshot_content: String,
        bad_state_before_dedupe: String,
    ) -> Self {
        self.snapshot_content = snapshot_content;
        self.disk_repair_reason = Some(match self.disk_repair_reason {
            Some(reason) => reason.merge_with_ipc_dedupe(),
            None => IpcDiskRepairReason::IpcDedupe,
        });
        if self.editor_bad_state.is_none() {
            self.editor_bad_state = Some(EditorBadStateFingerprint::new(bad_state_before_dedupe));
        }
        self.redeliver_editor = self.editor_bad_state.is_some();
        self
    }

    pub fn visible_write_proven(&self) -> bool {
        self.snap_source.is_visible_write_proven()
    }

    pub fn replace_snapshot_with_content_ours_for_live_prompt_drift(
        &mut self,
        content_ours: &str,
        visible_repair_required: bool,
    ) {
        let bad_state = self.snapshot_content.clone();
        self.snapshot_content = content_ours.to_string();
        self.snap_source = IpcSnapshotSource::ContentOurs;
        self.normalize_prefix_lines.clear();
        if visible_repair_required {
            self.disk_repair_reason = Some(IpcDiskRepairReason::LivePromptDrift);
            self.editor_bad_state = Some(EditorBadStateFingerprint::new(bad_state));
            self.redeliver_editor = true;
        } else {
            self.disk_repair_reason = None;
            self.editor_bad_state = None;
            self.redeliver_editor = false;
        }
    }

    pub fn replace_snapshot_with_content_ours_for_prompt_duplication(
        &mut self,
        content_ours: &str,
        bad_state: String,
    ) {
        self.snapshot_content = content_ours.to_string();
        self.snap_source = IpcSnapshotSource::ContentOurs;
        self.disk_repair_reason = Some(IpcDiskRepairReason::IpcDedupe);
        self.editor_bad_state = Some(EditorBadStateFingerprint::new(bad_state));
        self.normalize_prefix_lines.clear();
        self.redeliver_editor = true;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AlreadyAppliedSnapshotOutcome {
    Persisted,
    NeedsAuthoritativeRetry,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FullContentRepairRedelivery {
    NormalizationFallback,
    IpcDedupe,
    LivePromptDrift,
}

impl FullContentRepairRedelivery {
    pub const fn label(self) -> &'static str {
        match self {
            Self::NormalizationFallback => "sidecar_normalization_fallback",
            Self::IpcDedupe => "ipc_dedupe",
            Self::LivePromptDrift => "live_prompt_drift",
        }
    }

    pub const fn success_message(self) -> &'static str {
        match self {
            Self::NormalizationFallback => {
                "[write] sidecar normalization fallback re-delivered to editor via full-content IPC"
            }
            Self::IpcDedupe => "[write] IPC duplicate-response repair re-delivered to editor",
            Self::LivePromptDrift => "[write] live prompt drift repair re-delivered to editor",
        }
    }

    pub const fn not_consumed_message(self) -> &'static str {
        match self {
            Self::NormalizationFallback => {
                "[write] sidecar normalization fallback editor repair was not consumed; refusing direct document write"
            }
            Self::IpcDedupe => {
                "[write] IPC duplicate-response repair was not consumed; refusing direct document write"
            }
            Self::LivePromptDrift => {
                "[write] live prompt drift visible repair was not consumed; refusing direct document write"
            }
        }
    }

    pub fn failed_message(self, error: impl fmt::Display) -> String {
        match self {
            Self::NormalizationFallback => format!(
                "[write] sidecar normalization fallback editor repair failed: {}; refusing direct document write",
                error
            ),
            Self::IpcDedupe => format!(
                "[write] IPC duplicate-response repair failed: {}; refusing direct document write",
                error
            ),
            Self::LivePromptDrift => format!(
                "[write] live prompt drift visible repair failed: {}; refusing direct document write",
                error
            ),
        }
    }
}

pub fn existing_patch_is_reposition_only(payload: &serde_json::Value) -> bool {
    payload
        .get("reposition_boundary")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
        && payload
            .get("patches")
            .and_then(|value| value.as_array())
            .is_none_or(|patches| patches.is_empty())
        && payload
            .get("unmatched")
            .and_then(|value| value.as_str())
            .is_none_or(|unmatched| unmatched.trim().is_empty())
        && payload
            .get("fullContent")
            .and_then(|value| value.as_str())
            .is_none_or(|content| content.is_empty())
}

/// Return the `unmatched` field that should be carried in a patch payload.
///
/// When unmatched content was synthesized into one or more IPC patches, the
/// payload must not also carry the original unmatched body or the plugin will
/// apply the same text twice.
pub fn effective_unmatched_for_patch_payload(
    unmatched: &str,
    explicit_patch_count: usize,
    ipc_patch_count: usize,
) -> &str {
    if explicit_patch_count == 0 && ipc_patch_count > 0 {
        ""
    } else {
        unmatched.trim()
    }
}

/// Tag a `patch` message with the `early_receipt` opt-in when enabled.
///
/// Non-patch traffic is returned unchanged; non-mutating operations that can
/// tolerate accepted-before-terminal semantics should opt in through their own
/// message builder.
pub fn early_receipt_tagged_message(
    message: &serde_json::Value,
    enabled: bool,
) -> serde_json::Value {
    if !enabled {
        return message.clone();
    }
    let is_patch = message
        .get("type")
        .and_then(|t| t.as_str())
        .map(|t| t == "patch")
        .unwrap_or(false);
    if !is_patch {
        return message.clone();
    }
    let mut tagged = message.clone();
    if let Some(obj) = tagged.as_object_mut() {
        obj.insert("early_receipt".to_string(), serde_json::Value::Bool(true));
    }
    tagged
}

/// True when an incoming listener message opts into early receipt.
pub fn message_requests_early_receipt(message: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(message)
        .ok()
        .and_then(|v| v.get("early_receipt").and_then(|e| e.as_bool()))
        .unwrap_or(false)
}

/// The early `accepted` receipt line emitted by an early-receipt-aware listener on patch
/// receipt before the patch is applied.
pub fn early_receipt_line() -> &'static str {
    r#"{"type":"receipt","status":"accepted"}"#
}

/// Ops-log marker recorded when an early `accepted` receipt is emitted before the
/// blocking apply handler runs.
pub fn early_receipt_ops_marker() -> &'static str {
    "[ipc-socket] early_receipt_accepted emitted before apply"
}

/// Ops-log marker emitted when a listener connection is accepted and handed to
/// a fresh handler thread.
pub fn ipc_accept_thread_ops_marker(inflight: u64) -> String {
    format!("[ipc-socket] ipc_accept_thread_spawned inflight={inflight}")
}

/// Build a patch IPC payload for editor-owned document mutation.
pub fn patch_message(
    file: &str,
    patches: serde_json::Value,
    frontmatter_yaml: Option<&str>,
) -> serde_json::Value {
    serde_json::json!({
        "type": "patch",
        "file": file,
        "patches": patches,
        "frontmatter": frontmatter_yaml,
    })
}

/// Build a queue convergence patch payload.
pub fn queue_convergence_message(
    file: &str,
    queue_auto: bool,
    frontmatter_yaml: Option<&str>,
    queue_body: Option<&str>,
) -> serde_json::Value {
    let patches = queue_body
        .map(|content| {
            serde_json::json!([{
                "component": "queue",
                "content": content,
            }])
        })
        .unwrap_or_else(|| serde_json::json!([]));
    serde_json::json!({
        "type": "patch",
        "file": file,
        "patches": patches,
        "unmatched": "",
        "frontmatter": frontmatter_yaml,
        "queue_auto": queue_auto,
    })
}

/// Build a boundary reposition IPC payload.
pub fn reposition_message(
    file: &str,
    boundary_id: Option<&str>,
    preserve_head: bool,
) -> serde_json::Value {
    let mut message = serde_json::json!({
        "type": "reposition",
        "file": file,
    });
    if let Some(boundary_id) = boundary_id {
        message["boundary_id"] = serde_json::Value::String(boundary_id.to_string());
    }
    if preserve_head {
        message["preserve_head"] = serde_json::Value::Bool(true);
    }
    message
}

/// Build an editor save request payload.
pub fn save_document_message(file: &str, patch_id: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "save_document",
        "file": file,
        "patch_id": patch_id,
    })
}

/// Build a full content refresh payload.
pub fn refresh_content_message(
    file: &str,
    content: &str,
    expected_content_hash: &str,
    expected_content_len: usize,
) -> serde_json::Value {
    serde_json::json!({
        "type": "refresh_content",
        "file": file,
        "content": content,
        "expected_content_hash": expected_content_hash,
        "expected_content_len": expected_content_len,
    })
}

/// Build a narrow patch payload that asks the editor to repair only exchange
/// prompt-prefix normalization against an expected source buffer.
pub fn normalization_repair_patch_message(
    file: &str,
    patch_id: &str,
    normalize_prefix_lines: &[String],
    expected_content_hash: &str,
    expected_content_len: usize,
    include_type: bool,
) -> serde_json::Value {
    let mut message = serde_json::json!({
        "file": file,
        "patches": [],
        "unmatched": "",
        "baseline": "",
        "patch_id": patch_id,
        "reposition_boundary": true,
        "preserve_head": true,
        "normalize_prefix_lines": normalize_prefix_lines,
        "expected_content_hash": expected_content_hash,
        "expected_content_len": expected_content_len,
    });
    if include_type {
        message["type"] = serde_json::Value::String("patch".to_string());
    }
    message
}

/// Build a read-only live-buffer publication request payload.
pub fn publish_live_buffer_message(file: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "publish_live_buffer",
        "file": file,
        "early_receipt": true,
        "issued_at_ms": now_millis(),
    })
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

/// Build a VCS refresh payload.
pub fn vcs_refresh_message() -> serde_json::Value {
    serde_json::json!({
        "type": "vcs_refresh",
    })
}

/// Build a read-only cdylib reload announcement payload.
///
/// Sent to editor plugins to tell them a freshly-installed `libagent_doc` cdylib
/// is available so they force the existing native-reload path immediately instead
/// of lazily reloading on the next FFI call. Read-only: carries no `patch_id`,
/// `patches`, or `content` — the plugin reacts by reloading the shared library it
/// already resolves, never by mutating a document.
pub fn reload_lib_message(lib_version: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "reload_lib",
        "lib_version": lib_version,
    })
}

/// Build a VCS refresh probe payload.
pub fn vcs_refresh_probe_message(probe: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "vcs_refresh",
        "probe": probe,
    })
}

/// Build node-keyed IPC patches that transform tracked markdown items from
/// `before` to `after`.
pub fn build_ipc_node_patches_json(
    before: Option<&str>,
    after: Option<&str>,
) -> Vec<serde_json::Value> {
    let (Some(before), Some(after)) = (before, after) else {
        return Vec::new();
    };
    if before == after {
        return Vec::new();
    }

    let mut component_names = BTreeSet::new();
    for component in agent_doc_markdown_ast::overlay::components(before)
        .into_iter()
        .chain(agent_doc_markdown_ast::overlay::components(after))
    {
        if !component.items.is_empty() {
            component_names.insert(component.name);
        }
    }

    let mut node_patches = Vec::new();
    for component in component_names {
        let before_nodes =
            agent_doc_markdown_ast::mutations::item_nodes(before, &component).unwrap_or_default();
        let after_nodes =
            agent_doc_markdown_ast::mutations::item_nodes(after, &component).unwrap_or_default();
        if before_nodes.is_empty() && after_nodes.is_empty() {
            continue;
        }

        let before_by_key: HashMap<&str, _> = before_nodes
            .iter()
            .map(|node| (node.node_key.as_str(), node))
            .collect();
        let after_by_key: HashMap<&str, _> = after_nodes
            .iter()
            .map(|node| (node.node_key.as_str(), node))
            .collect();

        for node in &before_nodes {
            if !after_by_key.contains_key(node.node_key.as_str()) {
                let before_source = ipc_node_source(before, node);
                node_patches.push(serde_json::json!({
                    "component": component.as_str(),
                    "node_key": node.node_key.as_str(),
                    "op": "remove",
                    "content": before_source,
                    "expected_content": before_source,
                    "expected_content_hash": agent_doc_hash::content_hash(&before_source),
                }));
            }
        }

        for (index, node) in after_nodes.iter().enumerate() {
            if before_by_key.contains_key(node.node_key.as_str()) {
                continue;
            }
            let mut patch = serde_json::json!({
                "component": component.as_str(),
                "node_key": node.node_key.as_str(),
                "op": "insert",
                "content": ipc_node_source(after, node),
            });
            if let Some(anchor) = previous_existing_node_key(&after_nodes[..index], &before_by_key)
            {
                patch["after"] = serde_json::Value::String(anchor);
            } else if let Some(anchor) =
                next_existing_node_key(&after_nodes[index + 1..], &before_by_key)
            {
                patch["before"] = serde_json::Value::String(anchor);
            }
            node_patches.push(patch);
        }

        for node in &before_nodes {
            let Some(after_node) = after_by_key.get(node.node_key.as_str()) else {
                continue;
            };
            let before_source = ipc_node_source(before, node);
            let after_source = ipc_node_source(after, after_node);
            if before_source == after_source {
                continue;
            }
            let op = if !node.item.struck && after_node.item.struck {
                "strike"
            } else if node.item.struck && !after_node.item.struck {
                "unstrike"
            } else {
                "replace"
            };
            node_patches.push(serde_json::json!({
                "component": component.as_str(),
                "node_key": node.node_key.as_str(),
                "op": op,
                "content": after_source,
                "expected_content": before_source,
                "expected_content_hash": agent_doc_hash::content_hash(&before_source),
            }));
        }

        let before_shared = before_nodes
            .iter()
            .filter(|node| after_by_key.contains_key(node.node_key.as_str()))
            .map(|node| node.node_key.as_str())
            .collect::<Vec<_>>();
        let after_shared = after_nodes
            .iter()
            .filter(|node| before_by_key.contains_key(node.node_key.as_str()))
            .map(|node| node.node_key.as_str())
            .collect::<Vec<_>>();
        if before_shared != after_shared {
            for (index, node_key) in after_shared.iter().enumerate() {
                if before_shared.get(index).copied() == Some(*node_key) {
                    continue;
                }
                let mut patch = serde_json::json!({
                    "component": component.as_str(),
                    "node_key": *node_key,
                    "op": "move",
                });
                if let Some(node) = before_by_key.get(*node_key) {
                    let before_source = ipc_node_source(before, node);
                    patch["expected_content"] = serde_json::Value::String(before_source.clone());
                    patch["expected_content_hash"] =
                        serde_json::Value::String(agent_doc_hash::content_hash(&before_source));
                }
                if let Some(anchor) = after_shared[..index].last() {
                    patch["after"] = serde_json::Value::String((*anchor).to_string());
                } else if let Some(anchor) = after_shared.get(index + 1) {
                    patch["before"] = serde_json::Value::String((*anchor).to_string());
                }
                node_patches.push(patch);
            }
        }
    }

    node_patches
}

fn ipc_node_source(
    source: &str,
    node: &agent_doc_markdown_ast::mutations::MutationItemNode,
) -> String {
    source
        .get(node.item.start_byte..node.item.end_byte)
        .unwrap_or(&node.item.raw)
        .to_string()
}

fn previous_existing_node_key(
    nodes: &[agent_doc_markdown_ast::mutations::MutationItemNode],
    existing: &HashMap<&str, &agent_doc_markdown_ast::mutations::MutationItemNode>,
) -> Option<String> {
    nodes
        .iter()
        .rev()
        .find(|node| existing.contains_key(node.node_key.as_str()))
        .map(|node| node.node_key.clone())
}

fn next_existing_node_key(
    nodes: &[agent_doc_markdown_ast::mutations::MutationItemNode],
    existing: &HashMap<&str, &agent_doc_markdown_ast::mutations::MutationItemNode>,
) -> Option<String> {
    nodes
        .iter()
        .find(|node| existing.contains_key(node.node_key.as_str()))
        .map(|node| node.node_key.clone())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FullContentIpcMode {
    /// Late fallback repair for an agent response. Must not dirty an already
    /// committed cycle.
    ResponseFallback,
    /// Operator-owned replacement such as Compact Exchange. This is a new
    /// document mutation even when the previous response cycle is committed.
    OperatorMutation,
}

impl FullContentIpcMode {
    pub const fn source_label(self) -> &'static str {
        match self {
            Self::ResponseFallback => "response_fallback",
            Self::OperatorMutation => "compact_exchange",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AlreadyAppliedSnapshotOutcome, EditorBadStateFingerprint, FullContentIpcMode,
        FullContentRepairRedelivery, IpcDiskRepairReason, IpcRepairDecision, IpcSnapshotSource,
        SocketReceiptClassification, build_ipc_node_patches_json, callback_request,
        callback_request_is_expired, callback_response, callback_response_matches_request,
        callback_urgency_for_elapsed, classify_socket_receipt, early_receipt_line,
        early_receipt_ops_marker, early_receipt_tagged_message,
        effective_unmatched_for_patch_payload, existing_patch_is_reposition_only,
        ipc_accept_thread_ops_marker, is_already_applied_receipt_error_message,
        is_socket_receipt_timeout_error, is_socket_status_error, message_requests_early_receipt,
        normalization_repair_patch_message, patch_message, pending_callback_from_request,
        publish_live_buffer_message, queue_convergence_message, refresh_content_message,
        reload_lib_message, reposition_message, save_document_message, vcs_refresh_message,
        vcs_refresh_probe_message,
    };

    #[test]
    fn classify_socket_receipt_treats_pending_status_as_pending() {
        assert_eq!(
            classify_socket_receipt(r#"{"type":"receipt","status":"pending"}"#),
            SocketReceiptClassification::Pending
        );
        assert_eq!(
            classify_socket_receipt(r#"{"type":"receipt","status":"accepted"}"#),
            SocketReceiptClassification::Pending
        );
        assert_eq!(
            classify_socket_receipt(early_receipt_line()),
            SocketReceiptClassification::Pending
        );
    }

    #[test]
    fn classify_socket_receipt_treats_applied_status_as_applied() {
        let receipt = r#"{"type":"receipt","status":"applied","id":"patch-123"}"#;
        assert_eq!(
            classify_socket_receipt(receipt),
            SocketReceiptClassification::Applied
        );
    }

    #[test]
    fn classify_socket_receipt_treats_legacy_ack_as_unsupported() {
        let legacy_ack = r#"{"type":"ack","status":"applied","id":"patch-123"}"#;
        assert_eq!(
            classify_socket_receipt(legacy_ack),
            SocketReceiptClassification::Unsupported
        );
    }

    #[test]
    fn classify_socket_receipt_treats_already_applied_reason_as_already_applied() {
        let receipt = r#"{"type":"receipt","status":"applied","reason":"already_applied"}"#;
        assert_eq!(
            classify_socket_receipt(receipt),
            SocketReceiptClassification::AlreadyApplied
        );
    }

    #[test]
    fn classify_socket_receipt_treats_already_applied_reason_uppercase_as_already_applied() {
        let receipt = r#"{"type":"receipt","status":"APPLIED","reason":"Already_Applied"}"#;
        assert_eq!(
            classify_socket_receipt(receipt),
            SocketReceiptClassification::AlreadyApplied
        );
    }

    #[test]
    fn classify_socket_receipt_treats_rejected_status_as_rejected() {
        let receipt = r#"{"type":"receipt","status":"rejected","reason":"apply_failed"}"#;
        assert_eq!(
            classify_socket_receipt(receipt),
            SocketReceiptClassification::Rejected
        );
    }

    #[test]
    fn classify_socket_receipt_treats_unknown_receipt_status_as_unsupported() {
        let receipt = r#"{"type":"receipt","status":"error"}"#;
        assert_eq!(
            classify_socket_receipt(receipt),
            SocketReceiptClassification::Unsupported
        );
    }

    #[test]
    fn classify_socket_receipt_treats_malformed_json_as_unsupported() {
        let receipt = "not json at all";
        assert_eq!(
            classify_socket_receipt(receipt),
            SocketReceiptClassification::Unsupported
        );
    }

    #[test]
    fn already_applied_receipt_error_message_matches_socket_send_error_shape() {
        assert!(is_already_applied_receipt_error_message(concat!(
            "IPC receipt already_applied: ",
            r#"{"type":"receipt","status":"applied","reason":"already_applied"}"#
        )));
    }

    #[test]
    fn already_applied_receipt_error_message_rejects_other_socket_errors() {
        assert!(!is_already_applied_receipt_error_message(
            "IPC receipt rejected: something else"
        ));
        assert!(!is_already_applied_receipt_error_message(
            "IPC receipt timeout (2s)"
        ));
        assert!(!is_already_applied_receipt_error_message(
            r#"{"type":"receipt","status":"applied","reason":"already_applied"}"#
        ));
    }

    #[test]
    fn is_socket_receipt_timeout_error_is_duration_agnostic() {
        assert!(is_socket_receipt_timeout_error("IPC receipt timeout (2s)"));
        assert!(is_socket_receipt_timeout_error("IPC receipt timeout (6s)"));
        assert!(!is_socket_receipt_timeout_error(
            "IPC receipt rejected: something else"
        ));
    }

    #[test]
    fn is_socket_status_error_matches_terminal_apply_rejection() {
        assert!(is_socket_status_error(
            r#"IPC receipt rejected: {"type":"receipt","status":"rejected"}"#
        ));
        assert!(!is_socket_status_error("IPC receipt timeout (6s)"));
        assert!(!is_socket_status_error(
            r#"IPC receipt already_applied: {"type":"receipt","status":"applied","reason":"already_applied"}"#
        ));
    }

    #[test]
    fn message_requests_early_receipt_reads_flag() {
        assert!(message_requests_early_receipt(
            r#"{"type":"patch","early_receipt":true}"#
        ));
        assert!(!message_requests_early_receipt(r#"{"type":"patch"}"#));
        assert!(!message_requests_early_receipt(
            r#"{"type":"patch","early_receipt":false}"#
        ));
        assert!(!message_requests_early_receipt("not json"));
    }

    #[test]
    fn early_receipt_tagging_marks_only_enabled_patch_messages() {
        let patch = serde_json::json!({"type": "patch", "file": "x.md"});
        let tagged = early_receipt_tagged_message(&patch, true);
        assert_eq!(tagged["early_receipt"], serde_json::Value::Bool(true));
        assert_eq!(tagged["type"], "patch");
        assert_eq!(tagged["file"], "x.md");
        assert!(message_requests_early_receipt(&tagged.to_string()));

        assert_eq!(early_receipt_tagged_message(&patch, false), patch);

        let other = serde_json::json!({"type": "vcs_refresh"});
        assert_eq!(early_receipt_tagged_message(&other, true), other);
    }

    #[test]
    fn early_receipt_ops_marker_carries_predicate_token() {
        let marker = early_receipt_ops_marker();
        assert!(
            marker.contains("early_receipt_accepted"),
            "ops marker must carry the predicate token: {marker}"
        );
        assert!(!early_receipt_line().contains("early_receipt_accepted"));
    }

    #[test]
    fn build_ipc_node_patches_json_emits_stale_guarded_item_ops() {
        let before = "\
<!-- agent:queue -->
- do [#alpha]
- do [#beta]
<!-- /agent:queue -->
";
        let after = "\
<!-- agent:queue -->
- do [#alpha] updated
- do [#gamma]
<!-- /agent:queue -->
";

        let patches = build_ipc_node_patches_json(Some(before), Some(after));

        assert!(patches.iter().any(|patch| {
            let expected = patch["expected_content"].as_str().unwrap_or_default();
            patch["component"] == "queue"
                && patch["op"] == "remove"
                && patch["content"]
                    .as_str()
                    .is_some_and(|text| text.trim_end() == "- do [#beta]")
                && expected.trim_end() == "- do [#beta]"
                && patch["expected_content_hash"] == agent_doc_hash::content_hash(expected)
        }));
        assert!(patches.iter().any(|patch| {
            let expected = patch["expected_content"].as_str().unwrap_or_default();
            patch["component"] == "queue"
                && patch["op"] == "replace"
                && patch["content"]
                    .as_str()
                    .is_some_and(|text| text.trim_end() == "- do [#alpha] updated")
                && expected.trim_end() == "- do [#alpha]"
                && patch["expected_content_hash"] == agent_doc_hash::content_hash(expected)
        }));
        assert!(patches.iter().any(|patch| {
            patch["component"] == "queue"
                && patch["op"] == "insert"
                && patch["content"]
                    .as_str()
                    .is_some_and(|text| text.trim_end() == "- do [#gamma]")
                && patch["after"]
                    .as_str()
                    .is_some_and(|anchor| anchor.contains("alpha"))
        }));
    }

    #[test]
    fn ipc_accept_thread_ops_marker_carries_predicate_and_count() {
        let marker = ipc_accept_thread_ops_marker(7);
        assert!(marker.contains("ipc_accept_thread_spawned"));
        assert!(marker.contains("inflight=7"));
    }

    #[test]
    fn patch_message_carries_patch_payload_and_frontmatter() {
        let message = patch_message(
            "/tmp/plan.md",
            serde_json::json!([{"component": "exchange", "content": "done"}]),
            Some("queue: []"),
        );

        assert_eq!(message["type"], "patch");
        assert_eq!(message["file"], "/tmp/plan.md");
        assert_eq!(message["frontmatter"], "queue: []");
        assert_eq!(message["patches"][0]["component"], "exchange");
    }

    #[test]
    fn queue_convergence_message_uses_patch_shape() {
        let message = queue_convergence_message("/tmp/plan.md", false, None, Some("- item"));

        assert_eq!(message["type"], "patch");
        assert_eq!(message["file"], "/tmp/plan.md");
        assert_eq!(message["unmatched"], "");
        assert_eq!(message["queue_auto"], false);
        assert_eq!(message["frontmatter"], serde_json::Value::Null);
        assert_eq!(message["patches"][0]["component"], "queue");
        assert_eq!(message["patches"][0]["content"], "- item");
    }

    #[test]
    fn reposition_message_adds_only_requested_options() {
        let bare = reposition_message("/tmp/plan.md", None, false);
        assert_eq!(bare["type"], "reposition");
        assert_eq!(bare["file"], "/tmp/plan.md");
        assert!(bare.get("boundary_id").is_none());
        assert!(bare.get("preserve_head").is_none());

        let full = reposition_message("/tmp/plan.md", Some("boundary-1"), true);
        assert_eq!(full["boundary_id"], "boundary-1");
        assert_eq!(full["preserve_head"], true);
    }

    #[test]
    fn existing_patch_is_reposition_only_classifies_empty_reposition_payload() {
        let payload = serde_json::json!({
            "reposition_boundary": true,
            "patches": [],
            "unmatched": "   ",
            "fullContent": "",
        });

        assert!(existing_patch_is_reposition_only(&payload));
    }

    #[test]
    fn existing_patch_is_reposition_only_rejects_response_body_payloads() {
        assert!(!existing_patch_is_reposition_only(&serde_json::json!({
            "reposition_boundary": true,
            "patches": [{"component": "response", "content": "body"}],
            "unmatched": "",
            "fullContent": "",
        })));
        assert!(!existing_patch_is_reposition_only(&serde_json::json!({
            "reposition_boundary": true,
            "patches": [],
            "unmatched": "body",
            "fullContent": "",
        })));
        assert!(!existing_patch_is_reposition_only(&serde_json::json!({
            "reposition_boundary": true,
            "patches": [],
            "unmatched": "",
            "fullContent": "body",
        })));
        assert!(!existing_patch_is_reposition_only(&serde_json::json!({
            "patches": [],
            "unmatched": "",
            "fullContent": "",
        })));
    }

    #[test]
    fn effective_unmatched_for_patch_payload_clears_synthesized_unmatched() {
        assert_eq!(effective_unmatched_for_patch_payload(" body ", 0, 1), "");
    }

    #[test]
    fn effective_unmatched_for_patch_payload_keeps_explicit_patch_unmatched() {
        assert_eq!(
            effective_unmatched_for_patch_payload(" body ", 1, 1),
            "body"
        );
    }

    #[test]
    fn effective_unmatched_for_patch_payload_keeps_unsynthesized_unmatched() {
        assert_eq!(
            effective_unmatched_for_patch_payload(" body ", 0, 0),
            "body"
        );
    }

    #[test]
    fn save_document_message_is_typed_and_readonly() {
        let message = save_document_message("/tmp/plan.md", "save-123");

        assert_eq!(message["type"], "save_document");
        assert_eq!(message["file"], "/tmp/plan.md");
        assert_eq!(message["patch_id"], "save-123");
        assert!(message.get("content").is_none());
        assert!(message.get("patches").is_none());
    }

    #[test]
    fn refresh_content_message_carries_expected_source_proof() {
        let message = refresh_content_message("/tmp/plan.md", "content", "hash-1", 7);

        assert_eq!(message["type"], "refresh_content");
        assert_eq!(message["file"], "/tmp/plan.md");
        assert_eq!(message["content"], "content");
        assert_eq!(message["expected_content_hash"], "hash-1");
        assert_eq!(message["expected_content_len"], 7);
    }

    #[test]
    fn normalization_repair_patch_message_is_narrow_and_source_proven() {
        let message = normalization_repair_patch_message(
            "/tmp/plan.md",
            "repair-1",
            &["do #norm".to_string()],
            "hash-1",
            42,
            true,
        );

        assert_eq!(message["type"], "patch");
        assert_eq!(message["file"], "/tmp/plan.md");
        assert_eq!(message["patch_id"], "repair-1");
        assert_eq!(message["patches"], serde_json::json!([]));
        assert_eq!(message["unmatched"], "");
        assert_eq!(message["baseline"], "");
        assert_eq!(message["reposition_boundary"], true);
        assert_eq!(message["preserve_head"], true);
        assert_eq!(message["normalize_prefix_lines"][0], "do #norm");
        assert_eq!(message["expected_content_hash"], "hash-1");
        assert_eq!(message["expected_content_len"], 42);
        assert!(message.get("fullContent").is_none());

        let file_ipc_message = normalization_repair_patch_message(
            "/tmp/plan.md",
            "repair-1",
            &["do #norm".to_string()],
            "hash-1",
            42,
            false,
        );
        assert!(file_ipc_message.get("type").is_none());
    }

    #[test]
    fn publish_live_buffer_message_is_readonly_and_requests_early_receipt() {
        let message = publish_live_buffer_message("/tmp/plan.md");

        assert_eq!(message["type"], "publish_live_buffer");
        assert_eq!(message["file"], "/tmp/plan.md");
        assert_eq!(message["early_receipt"], true);
        assert!(message["issued_at_ms"].as_u64().unwrap() > 0);
        assert!(message_requests_early_receipt(&message.to_string()));
        assert!(message.get("content").is_none());
        assert!(message.get("patches").is_none());
    }

    #[test]
    fn reload_lib_message_is_typed_and_readonly() {
        let message = reload_lib_message("0.34.68");

        assert_eq!(message["type"], "reload_lib");
        assert_eq!(message["lib_version"], "0.34.68");
        assert!(message.get("patch_id").is_none());
        assert!(message.get("patches").is_none());
        assert!(message.get("content").is_none());
        assert!(message.get("file").is_none());
    }

    #[test]
    fn vcs_refresh_messages_have_stable_type_and_probe_field() {
        let message = vcs_refresh_message();
        assert_eq!(message["type"], "vcs_refresh");
        assert!(message.get("probe").is_none());

        let probe = vcs_refresh_probe_message("ipc_degraded_self_heal");
        assert_eq!(probe["type"], "vcs_refresh");
        assert_eq!(probe["probe"], "ipc_degraded_self_heal");
    }

    #[test]
    fn callback_request_expiry_uses_request_ttl() {
        let request = callback_request(
            "/tmp/plan.md",
            "doc-hash",
            ["compact"],
            None::<String>,
            100,
            60,
            "request-1",
        );

        assert!(!callback_request_is_expired(&request, 160));
        assert!(callback_request_is_expired(&request, 161));
    }

    #[test]
    fn callback_pending_urgency_is_elapsed_time_bucket() {
        assert_eq!(callback_urgency_for_elapsed(0), "high");
        assert_eq!(callback_urgency_for_elapsed(9), "high");
        assert_eq!(callback_urgency_for_elapsed(10), "normal");
        assert_eq!(callback_urgency_for_elapsed(59), "normal");
        assert_eq!(callback_urgency_for_elapsed(60), "low");
    }

    #[test]
    fn callback_pending_projection_keeps_doc_and_operations() {
        let request = callback_request(
            "/tmp/plan.md",
            "doc-hash",
            ["compact", "summary"],
            Some("operator context"),
            100,
            300,
            "request-1",
        );

        let pending = pending_callback_from_request(request, 115);

        assert_eq!(pending.doc_path, "/tmp/plan.md");
        assert_eq!(pending.operations, vec!["compact", "summary"]);
        assert_eq!(pending.urgency, "normal");
    }

    #[test]
    fn callback_response_match_rejects_stale_request_id() {
        let request = callback_request(
            "/tmp/plan.md",
            "doc-hash",
            ["compact"],
            None::<String>,
            100,
            300,
            "request-1",
        );
        let matching = callback_response("request-1", "success", "done", None::<String>, None, 150);
        let stale = callback_response("request-2", "success", "done", None::<String>, None, 150);

        assert!(callback_response_matches_request(&matching, Some(&request)));
        assert!(!callback_response_matches_request(&stale, Some(&request)));
        assert!(callback_response_matches_request(&stale, None));
    }

    #[test]
    fn full_content_ipc_mode_source_labels_are_stable() {
        assert_eq!(
            FullContentIpcMode::ResponseFallback.source_label(),
            "response_fallback"
        );
        assert_eq!(
            FullContentIpcMode::OperatorMutation.source_label(),
            "compact_exchange"
        );
    }

    #[test]
    fn ipc_repair_vocabulary_labels_and_messages_are_stable() {
        assert_eq!(
            IpcSnapshotSource::LegacySidecarProjection.label(),
            "legacy_sidecar_projection"
        );
        assert!(IpcSnapshotSource::LazilyVisibleWriteEvent.is_visible_write_proven());
        assert!(!IpcSnapshotSource::LegacySidecarProjection.is_visible_write_proven());
        assert!(!IpcSnapshotSource::FileRead.is_visible_write_proven());

        assert_eq!(
            IpcDiskRepairReason::PrefixDivergence.label(),
            "prefix_divergence"
        );
        assert_eq!(
            IpcDiskRepairReason::PrefixDivergence.merge_with_ipc_dedupe(),
            IpcDiskRepairReason::PrefixDivergenceThenIpcDedupe
        );
        assert_eq!(
            IpcDiskRepairReason::LivePromptDrift.redelivery_kind(),
            FullContentRepairRedelivery::LivePromptDrift
        );

        assert_eq!(
            AlreadyAppliedSnapshotOutcome::Persisted,
            AlreadyAppliedSnapshotOutcome::Persisted
        );
        assert_ne!(
            AlreadyAppliedSnapshotOutcome::Persisted,
            AlreadyAppliedSnapshotOutcome::NeedsAuthoritativeRetry
        );

        assert_eq!(FullContentRepairRedelivery::IpcDedupe.label(), "ipc_dedupe");
        assert!(
            FullContentRepairRedelivery::IpcDedupe
                .success_message()
                .contains("re-delivered")
        );
        assert!(
            FullContentRepairRedelivery::LivePromptDrift
                .not_consumed_message()
                .contains("refusing direct document write")
        );
        assert!(
            FullContentRepairRedelivery::NormalizationFallback
                .failed_message("boom")
                .contains("boom")
        );
    }

    #[test]
    fn editor_bad_state_fingerprint_records_content_len_and_hash() {
        let fingerprint = EditorBadStateFingerprint::new("bad editor state\n".to_string());

        assert_eq!(fingerprint.content(), "bad editor state\n");
        assert_eq!(fingerprint.len, "bad editor state\n".len());
        assert_eq!(
            fingerprint.hash,
            agent_doc_hash::content_hash("bad editor state\n")
        );
    }

    #[test]
    fn ipc_repair_decision_dedupe_merges_prefix_repair_state() {
        let decision = IpcRepairDecision::lazily_visible_write_prefix_repair(
            "prefix repaired\n".to_string(),
            "bad prefix\n".to_string(),
            &["normalize me".to_string()],
        )
        .apply_ipc_dedupe(
            "deduped\n".to_string(),
            "ignored later bad state\n".to_string(),
        );

        assert_eq!(decision.snapshot_content, "deduped\n");
        assert_eq!(
            decision.disk_repair_reason,
            Some(IpcDiskRepairReason::PrefixDivergenceThenIpcDedupe)
        );
        assert_eq!(
            decision.snap_source,
            IpcSnapshotSource::LazilyVisibleWriteEvent
        );
        assert!(decision.redeliver_editor);
        assert_eq!(
            decision
                .editor_bad_state
                .as_ref()
                .map(|state| state.content()),
            Some("bad prefix\n")
        );
        assert_eq!(
            decision.normalize_prefix_lines,
            ["normalize me".to_string()]
        );
    }
}
