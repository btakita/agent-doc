//! # Module: ffi
//!
//! ## Spec
//! - Exports a C ABI (`extern "C"`) surface consumed by the JetBrains plugin via JNA and the VS
//!   Code extension via Node native addons, eliminating duplicated parsing/merge logic in Kotlin
//!   and TypeScript.
//! - `agent_doc_parse_components(doc)`: parses all `<!-- agent:name -->` components and returns a
//!   JSON-encoded array with fields `name`, `attrs`, `open_start`, `open_end`, `close_start`,
//!   `close_end`, `content`.
//! - `agent_doc_visual_tokens_json(doc)`: returns a JSON-encoded array of visual token ranges used
//!   by editor plugins to highlight agent-doc-specific markdown structures consistently. Exported
//!   offsets are UTF-16 document positions so editor APIs can consume them directly.
//! - `agent_doc_apply_patch(doc, component_name, content, mode)`: applies a patch to a named
//!   component using `replace`, `append`, or `prepend` mode.
//! - `agent_doc_apply_patch_with_caret(…, caret_offset)`: caret-aware append — inserts content at
//!   the line boundary before the caret when `mode == "append"` and `caret_offset >= 0`.  Falls
//!   back to `apply_patch` otherwise.
//! - `agent_doc_apply_patch_with_boundary(…, boundary_id)`: boundary-marker-aware append — inserts
//!   content at the named boundary marker position, then falls back to `apply_patch` if the marker
//!   is not found.
//! - `agent_doc_crdt_merge(base_state, base_state_len, ours, theirs)`: 3-way CRDT merge; `base_state`
//!   may be null for the first merge.  Returns merged text and updated opaque CRDT state bytes.
//! - `agent_doc_merge_frontmatter(doc, yaml_fields)`: merges YAML key/value pairs into the
//!   document's frontmatter additively (never removes keys).
//! - `agent_doc_reposition_boundary_to_end(doc)`: removes all existing boundary markers and inserts
//!   a single fresh one at the end of the `exchange` component.
//! - `agent_doc_lazily_current_observed_v1(...)`: publishes the editor's complete current
//!   document into the Lazily authority model. No typing, live-buffer, or status sidecar is
//!   created.
//! - `agent_doc_record_editor_surface_event(...)`: records one cross-editor surface event through
//!   the shared Rust ops-log formatter so JetBrains and VS Code emit the same schema.
//! - `agent_doc_admin_*_json(...)`: controller-backed admin/editor wrappers for inspect, queue
//!   pause/resume/drain, handoff, reap, and projection repair. They return the same JSON receipt
//!   envelopes as the CLI `--json` forms.
//! - `agent_doc_free_string(ptr)` / `agent_doc_free_state(ptr, len)`: free memory returned by any
//!   `agent_doc_*` function.  Must be called for every non-null pointer.
//!
//! ## Agentic Contracts
//! - All string parameters must be valid, non-null, NUL-terminated UTF-8; violation is UB.
//! - Every non-null `text` or `error` pointer in a result struct must be freed exactly once with
//!   `agent_doc_free_string`; CRDT `state` pointers must be freed with `agent_doc_free_state`.
//! - On parse/apply errors, `text` (or `json`) is null and `error` holds a message; callers must
//!   check nullability before use.
//!
//! ## Evals
//! - parse_components_roundtrip: single `agent:status` component → JSON count=1, content="hello\n"
//! - apply_patch_replace: replace mode on `agent:output` → new content present, old content absent
//! - merge_frontmatter_adds_field: add `model: opus` to existing frontmatter → both keys present, body unchanged
//! - reposition_boundary_removes_stale: two boundary markers in exchange → exactly one marker at end
//! - crdt_merge_no_base: identical `ours`/`theirs` with null base → merged text equals input

use agent_doc_state_backbone::{EventLedger, StateEvent, StateFact};
use agent_doc_sync::{
    DEFAULT_SYNC_LOCK_STALE_BOUND_MS, SyncLockDecision, sync_lock_acquire_decision,
};
use agent_doc_turn::op_log::OpsLogEvent;
use anyhow::Context as _;
use serde::Serialize;
use std::ffi::{CStr, CString, c_char, c_int};
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};

/// Cross-editor sync lock — prevents concurrent layout syncs.
static SYNC_LOCKED: AtomicBool = AtomicBool::new(false);

/// Sync debounce generation counter — only the latest scheduled sync fires.
static SYNC_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Epoch-millis when the current [`SYNC_LOCKED`] holder acquired the guard (`0` = no
/// holder). Lets a later acquire detect a guard held past the stale bound — a wedged or
/// dead holder that never called `agent_doc_sync_unlock` (`#recyclerestart` Q2) — and
/// supersede it instead of deferring forever with "another sync is already running".
static SYNC_LOCK_ACQUIRED_AT_MS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

struct IpcListenerGeneration {
    root: PathBuf,
    shutdown: std::sync::Arc<AtomicBool>,
    thread: std::thread::JoinHandle<()>,
}

static IPC_LISTENER_GENERATIONS: std::sync::LazyLock<
    parking_lot::Mutex<std::collections::HashMap<String, IpcListenerGeneration>>,
> = std::sync::LazyLock::new(|| parking_lot::Mutex::new(std::collections::HashMap::new()));

/// Reject new listeners while the currently-loaded cdylib is crossing its
/// quiesce boundary. A freshly loaded generation starts with this flag clear.
static NATIVE_GENERATION_QUIESCING: AtomicBool = AtomicBool::new(false);

fn sync_lock_now_epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn sync_try_lock_with_bound(stale_bound_ms: u64) -> i32 {
    let now = sync_lock_now_epoch_ms();
    // Fast path: take a free guard.
    if SYNC_LOCKED
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
    {
        SYNC_LOCK_ACQUIRED_AT_MS.store(now, Ordering::SeqCst);
        return 1;
    }
    // Held: self-heal a guard wedged past the stale bound so it can never block every
    // later sync (`#recyclerestart` Q2). The CLI keeps its own `.agent-doc/sync.lock`
    // file-lock with stale-orphan reaping, so superseding here cannot double-run a sync.
    let acquired_at = SYNC_LOCK_ACQUIRED_AT_MS.load(Ordering::SeqCst);
    match sync_lock_acquire_decision(true, acquired_at, now, stale_bound_ms) {
        SyncLockDecision::SupersedeStaleHolder { held_ms } => {
            SYNC_LOCK_ACQUIRED_AT_MS.store(now, Ordering::SeqCst);
            eprintln!(
                "[sync] sync_guard_released reason=stale_holder_superseded held_ms={held_ms} bound_ms={stale_bound_ms}"
            );
            1
        }
        SyncLockDecision::Defer => 0,
        // Raced free between the CAS and the load — take it if still free.
        SyncLockDecision::Acquire => {
            if SYNC_LOCKED
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                SYNC_LOCK_ACQUIRED_AT_MS.store(now, Ordering::SeqCst);
                1
            } else {
                0
            }
        }
    }
}

/// Result of [`agent_doc_resolve_project_path`].
#[repr(C)]
pub struct FfiProjectPath {
    /// Absolute path to the project root (nearest ancestor containing `.agent-doc/`),
    /// or null if no ancestor has `.agent-doc/`. Free with [`agent_doc_free_string`].
    pub project_root: *mut c_char,
    /// Path to the input file, relative to `project_root`. Null when `project_root`
    /// is null. Free with [`agent_doc_free_string`].
    pub relative_path: *mut c_char,
}

/// JSON result returned by controller-backed editor/admin FFI wrappers.
#[repr(C)]
pub struct FfiJsonResult {
    /// JSON object on success, or null on error. Free with [`agent_doc_free_string`].
    pub json: *mut c_char,
    /// Error message when `json` is null. Free with [`agent_doc_free_string`].
    pub error: *mut c_char,
}

fn ffi_json_ok<T: Serialize>(value: &T) -> FfiJsonResult {
    match serde_json::to_string(value) {
        Ok(json) => FfiJsonResult {
            json: CString::new(json).unwrap_or_default().into_raw(),
            error: ptr::null_mut(),
        },
        Err(err) => ffi_json_err(&format!("failed to serialize JSON result: {err}")),
    }
}

fn ffi_json_err(message: &str) -> FfiJsonResult {
    FfiJsonResult {
        json: ptr::null_mut(),
        error: CString::new(message).unwrap_or_default().into_raw(),
    }
}

fn ffi_json_from_result<T: Serialize>(result: anyhow::Result<T>) -> FfiJsonResult {
    match result {
        Ok(value) => ffi_json_ok(&value),
        Err(err) => ffi_json_err(&format!("{err:#}")),
    }
}

unsafe fn optional_ffi_string(ptr: *const c_char, name: &str) -> anyhow::Result<Option<String>> {
    if ptr.is_null() {
        return Ok(None);
    }
    let value = unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .with_context(|| format!("{name} is not valid UTF-8"))?
        .trim();
    if value.is_empty() {
        Ok(None)
    } else {
        Ok(Some(value.to_string()))
    }
}

unsafe fn required_ffi_string(ptr: *const c_char, name: &str) -> anyhow::Result<String> {
    unsafe { optional_ffi_string(ptr, name) }?.with_context(|| format!("{name} is required"))
}

fn optional_generation(value: i64, name: &str) -> anyhow::Result<Option<u64>> {
    if value < 0 {
        return Ok(None);
    }
    u64::try_from(value)
        .map(Some)
        .with_context(|| format!("{name} is out of range"))
}

fn required_generation(value: i64, name: &str) -> anyhow::Result<u64> {
    optional_generation(value, name)?.with_context(|| format!("{name} is required"))
}

fn optional_path(value: Option<String>) -> Option<PathBuf> {
    value.map(PathBuf::from)
}

fn editor_surface_relative_path(project_root: &Path, file_path: &str) -> String {
    if file_path == "." {
        return ".".to_string();
    }
    let root = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());
    let file = Path::new(file_path)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(file_path));
    let relative = file.strip_prefix(&root).unwrap_or(&file);
    let rendered = relative.to_string_lossy();
    if rendered.is_empty() {
        ".".to_string()
    } else {
        rendered.into_owned()
    }
}

struct EditorSurfaceEvent<'a> {
    source: &'a str,
    file_path: &'a str,
    surface: &'a str,
    action: &'a str,
    agent_command: &'a str,
    patch_id: Option<&'a str>,
    status: &'a str,
}

fn editor_surface_event_message(project_root: &Path, event: EditorSurfaceEvent<'_>) -> String {
    let relative_path = editor_surface_relative_path(project_root, event.file_path);
    let doc = Path::new(&relative_path)
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty() && *value != ".")
        .unwrap_or("project");
    format!(
        "editor_surface_event source={} surface={} action={} agent_command={} status={} \
         file={relative_path} patch_id={} doc={doc} #cyh0",
        event.source,
        event.surface,
        event.action,
        event.agent_command,
        event.status,
        event.patch_id.unwrap_or("-")
    )
}

/// Record a typed editor-surface outcome in the project ops log.
///
/// This is the shared formatter/writer for JetBrains and VS Code. `file_path`
/// may be `"."` for a project-scoped event and `patch_id` may be null or empty.
/// Returns `1` only when the event was appended, and `0` for invalid input or an
/// ops-log write failure.
///
/// # Safety
/// Every non-null pointer must reference a NUL-terminated UTF-8 string for the
/// duration of this call. `patch_id` is the only nullable argument.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_record_editor_surface_event(
    project_root: *const c_char,
    source: *const c_char,
    file_path: *const c_char,
    surface: *const c_char,
    action: *const c_char,
    agent_command: *const c_char,
    patch_id: *const c_char,
    status: *const c_char,
) -> c_int {
    let result = (|| -> anyhow::Result<()> {
        let project_root =
            PathBuf::from(unsafe { required_ffi_string(project_root, "project_root") }?);
        let source = unsafe { required_ffi_string(source, "source") }?;
        let file_path = unsafe { required_ffi_string(file_path, "file_path") }?;
        let surface = unsafe { required_ffi_string(surface, "surface") }?;
        let action = unsafe { required_ffi_string(action, "action") }?;
        let agent_command = unsafe { required_ffi_string(agent_command, "agent_command") }?;
        let patch_id = unsafe { optional_ffi_string(patch_id, "patch_id") }?;
        let status = unsafe { required_ffi_string(status, "status") }?;
        anyhow::ensure!(
            project_root.join(".agent-doc").is_dir(),
            "project_root has no .agent-doc directory"
        );
        let message = editor_surface_event_message(
            &project_root,
            EditorSurfaceEvent {
                source: &source,
                file_path: &file_path,
                surface: &surface,
                action: &action,
                agent_command: &agent_command,
                patch_id: patch_id.as_deref(),
                status: &status,
            },
        );
        agent_doc_ops_log_io::append_ops_log_at_project(
            &project_root,
            &message,
            agent_doc_ops_log_io::OpsLogTracking::default(),
        )
        .context("failed to append editor surface event")?;
        Ok(())
    })();

    match result {
        Ok(()) => 1,
        Err(error) => {
            eprintln!("[ffi] record editor surface event failed: {error:#}");
            0
        }
    }
}

fn resolve_admin_root(
    project_root: Option<&str>,
    document: Option<&Path>,
) -> anyhow::Result<PathBuf> {
    if let Some(root) = project_root {
        return Ok(PathBuf::from(root));
    }
    if let Some(document) = document
        && let Some(root) = agent_doc_fs::find_project_root(document)
    {
        return Ok(root);
    }
    let cwd = std::env::current_dir().context("failed to read current directory")?;
    agent_doc_fs::find_project_root(&cwd)
        .with_context(|| format!("no .agent-doc project root found from {}", cwd.display()))
}

/// Current change notification with replica-churn provenance.
///
/// Carries `no_unsaved_operator_edits`: pass `1` when the editor has proven the
/// buffer holds no unsaved *local operator* edits ahead of disk (any divergence
/// is replica-driven — a `remoteCrdtApply`), so the binary re-merges on replica
/// churn instead of failing the visible-write guard closed. Pass `0` when there
/// may be unsaved operator text, which keeps operator text authoritative.
///
/// # Safety
///
/// `file_path`, `content`, `editor_id`, `editor_kind`, `editor_version`, and
/// `capabilities_csv` must be valid, NUL-terminated UTF-8 strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_lazily_current_observed_v1(
    file_path: *const c_char,
    content: *const c_char,
    editor_id: *const c_char,
    _editor_kind: *const c_char,
    _editor_version: *const c_char,
    _capabilities_csv: *const c_char,
    no_unsaved_operator_edits: i32,
) {
    mark_embedded_editor_host();
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(path) => path,
        Err(_) => return,
    };
    let text = match unsafe { CStr::from_ptr(content) }.to_str() {
        Ok(text) => text,
        Err(_) => return,
    };
    let editor_id = match unsafe { CStr::from_ptr(editor_id) }.to_str() {
        Ok(editor_id) => editor_id,
        Err(_) => return,
    };
    let file = Path::new(path);
    if agent_doc_frontmatter_io::session::is_agent_doc_document_for_file(text, file)
        && let Some(project_root) = agent_doc_project_root_io::project_root_containing(file)
    {
        let disk_persisted =
            std::fs::read(file).is_ok_and(|disk| disk.as_slice() == text.as_bytes());
        if let Err(err) =
            agent_doc_controller_io::project_controller::observe_editor_document_projection(
                &project_root,
                file,
                editor_id,
                &agent_doc_hash::content_hash(text),
                disk_persisted,
                "editor_document_state_projection",
            )
        {
            eprintln!("[ffi] could not publish editor document projection for {path}: {err:#}");
        }
        if no_unsaved_operator_edits == 0
            && let Err(err) = observe_editor_current_from_ffi(
                &project_root,
                file,
                text,
                "operator_editor_content_advanced",
            )
        {
            eprintln!(
                "[deferred-write] could not reconcile operator editor authority for {path}: {err}"
            );
        }
    }
}

/// Retire legacy buffer state and mirror a document-close hint. Durable
/// multi-editor liveness is owned by the reliable-sync Lazily OR-set; this ABI
/// never counts filesystem sidecars to decide whether an editor remains open.
///
/// # Safety
///
/// `file_path` and `editor_id` must be valid, NUL-terminated UTF-8 strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_document_closed_for_editor(
    file_path: *const c_char,
    editor_id: *const c_char,
) {
    mark_embedded_editor_host();
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(path) => path,
        Err(_) => return,
    };
    let _editor = match unsafe { CStr::from_ptr(editor_id) }.to_str() {
        Ok(editor) => editor,
        Err(_) => return,
    };
    agent_doc_document_realtime::editor_open_docs::editor_open_docs().mark_closed(path);
    if agent_doc_reliable_sync_io::plane_editor_live_for_path(path) == Some(false) {
        let file = std::path::Path::new(path);
        if let Some(project_root) = agent_doc_project_root_io::project_root_containing(file)
            && let Err(err) = document_closed_from_ffi(&project_root, file)
        {
            eprintln!("[ffi] last editor close projection failed for {path}: {err:#}");
        }
    }
}

/// `#cancel-orphans-preflight-cycle`: explicit run-cancel reclaim seam.
///
/// The JB plugin's "cancel run" action calls this so an orphaned, empty
/// `preflight_started` cycle (no response capture) is abandoned immediately and
/// the next `Run Agent Doc` starts fresh instead of waiting for the staleness
/// window. The abandon decision is fail-safe in the binary: a cycle that
/// advanced past preflight or already owns a response capture is left intact.
///
/// Returns `1` if a cycle was abandoned, `0` if nothing was reclaimed
/// (no open cycle, or the cycle is protected), and `-1` on error.
///
/// # Safety
///
/// `file_path` must be a valid, NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_cancel_preflight_cycle(file_path: *const c_char) -> i32 {
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };
    match agent_doc_repair_io::cancel_preflight_cycle(
        &agent_doc_closeout_runtime_io::REPAIR_IO_EFFECTS,
        std::path::Path::new(path),
    ) {
        Ok(agent_doc_turn::repair::CancelOutcome::Abandoned) => 1,
        Ok(_) => 0,
        Err(e) => {
            eprintln!("[ffi] agent_doc_cancel_preflight_cycle failed for {path}: {e}");
            -1
        }
    }
}

/// Get the Project Controller→plugin turn-state projection for a document, as JSON.
///
/// Returns a NUL-terminated JSON string of `TurnProjection`:
/// `{"state":"idle|awaiting_response|persisting","turn_in_flight":bool,"transition_authority":"project_controller","realtime_steering":{"state":"...","preview":"..."}}`.
/// The Project Controller owns the authoritative turn phase; the plugin observes
/// this projection to render turn-in-flight UI and to decide whether a forwarded
/// operator prompt starts a fresh turn (`turn_in_flight == false`) or would
/// collide with an in-flight response (the `live_prompt_drift` double-append
/// guard). Defaults to the idle projection when no cycle state exists or on any
/// error.
///
/// Caller must free with `agent_doc_free_string`.
///
/// # Safety
///
/// `file_path` must be a valid, NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_turn_projection(file_path: *const c_char) -> *mut c_char {
    fn idle_json() -> String {
        let proj = agent_doc_turn::cp_projection::TurnProjection::from_phase(
            agent_doc_turn::CyclePhase::Committed,
        );
        serde_json::to_string(&proj).unwrap_or_else(|_| r#"{"state":"idle"}"#.to_string())
    }
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(s) => s,
        Err(_) => return CString::new(idle_json()).unwrap().into_raw(),
    };
    // Legacy compatibility-cache read. Current editor adapters query the
    // controller's in-memory projection directly; this function never opens a
    // controller request, subscribes, or reads SQLite.
    let proj = agent_doc_project_root_io::project_root_containing(Path::new(path))
        .and_then(|project_root| {
            agent_doc_editor_surface_io::document_authority(&project_root, path)
                .and_then(|authority| authority.turn)
        })
        .unwrap_or_else(|| {
            agent_doc_turn::cp_projection::TurnProjection::from_phase(
                agent_doc_turn::CyclePhase::Committed,
            )
        });
    let json = serde_json::to_string(&proj).unwrap_or_else(|_| idle_json());
    CString::new(json)
        .unwrap_or_else(|_| CString::new(idle_json()).unwrap())
        .into_raw()
}

/// Return the `agent:exchange` nodes of `file_path` as a JSON array
/// (`[{"id","kind","label"}, …]`), or `[]` on any error or when the document has no
/// exchange component. This is the Phase 4 read surface of the exchange document-tree
/// (`tasks/agent-doc/plan-exchange-tree-seqcrdt-and-ipc-unify.md`): editors use it to
/// show the exchange's distinct per-response / per-prompt structure.
///
/// Read-only by design — mutating operations go through the binary-owned
/// `agent-doc exchange {remove,add-response,add-prompt,move}` CLI so the snapshot +
/// CRDT re-baseline runs correctly (editors stay thin; the binary owns document
/// mutation).
///
/// Caller must free with `agent_doc_free_string`.
///
/// # Safety
///
/// `file_path` must be a valid, NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_exchange_nodes(file_path: *const c_char) -> *mut c_char {
    fn empty() -> *mut c_char {
        CString::new("[]").unwrap().into_raw()
    }
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(s) => s,
        Err(_) => return empty(),
    };
    let Ok(doc) = std::fs::read_to_string(path) else {
        return empty();
    };
    let Ok(components) = agent_doc_element::element::parse(&doc) else {
        return empty();
    };
    let Some(comp) = components.into_iter().find(|c| c.name == "exchange") else {
        return empty();
    };
    let nodes = agent_doc_markdown_ast::exchange_tree::list_exchange_nodes(comp.content(&doc));
    let json = serde_json::to_string(
        &nodes
            .iter()
            .map(|n| serde_json::json!({ "id": n.node_id, "kind": n.kind, "label": n.label }))
            .collect::<Vec<_>>(),
    )
    .unwrap_or_else(|_| "[]".to_string());
    CString::new(json)
        .unwrap_or_else(|_| CString::new("[]").unwrap())
        .into_raw()
}

/// Check whether a `.md` file is an opted-in agent-doc session document.
///
/// Returns `1` when the file carries agent-doc frontmatter, matches a
/// `[documents] include` glob in `.agent-doc/config.toml`, or runs under the
/// `auto_session_for_all_md` escape hatch; `0` otherwise (including unreadable
/// paths). Editor plugins call this to gate **Run Agent Doc** / SubmitAction so
/// a plain `.md` is not offered as a session. Mirrors the binary opt-in gate.
///
/// # Safety
///
/// `file_path` must be a valid, NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_is_session_document(file_path: *const c_char) -> i32 {
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return 0,
    };
    agent_doc_frontmatter_io::session::is_agent_doc_document_for_file(
        &content,
        std::path::Path::new(path),
    ) as i32
}

/// Try to acquire the sync lock. Returns `true` if acquired, `false` if already held.
///
/// Editors call this before triggering `agent-doc sync`. If it returns `false`,
/// skip the sync (another sync is in progress). Call `agent_doc_sync_unlock()`
/// when the sync completes.
///
/// This is a cross-editor shared lock — prevents concurrent syncs from IntelliJ
/// and VS Code plugins simultaneously.
///
/// `#recyclerestart` Q2: the guard self-heals — a holder that wedges past
/// [`DEFAULT_SYNC_LOCK_STALE_BOUND_MS`] (a sync thread blocked on in-pane recovery that
/// never released) is superseded so a later `Sync Tmux Layout` can never be deferred
/// forever with "another sync is already running". A legitimately in-flight sync (held
/// under the bound) still defers as before. Use [`agent_doc_sync_try_lock_bounded`] to
/// override the bound.
#[unsafe(no_mangle)]
pub extern "C" fn agent_doc_sync_try_lock() -> i32 {
    sync_try_lock_with_bound(DEFAULT_SYNC_LOCK_STALE_BOUND_MS)
}

/// Like [`agent_doc_sync_try_lock`] but with an explicit stale bound in milliseconds
/// (`<= 0` falls back to [`DEFAULT_SYNC_LOCK_STALE_BOUND_MS`]). Lets a caller that knows
/// its sync is fast use a tighter self-heal window.
#[unsafe(no_mangle)]
pub extern "C" fn agent_doc_sync_try_lock_bounded(stale_bound_ms: i64) -> i32 {
    let bound = if stale_bound_ms <= 0 {
        DEFAULT_SYNC_LOCK_STALE_BOUND_MS
    } else {
        stale_bound_ms as u64
    };
    sync_try_lock_with_bound(bound)
}

/// Release the sync lock acquired by `agent_doc_sync_try_lock()`.
#[unsafe(no_mangle)]
pub extern "C" fn agent_doc_sync_unlock() {
    // Clear the holder timestamp before releasing so a racing acquirer that observes the
    // free guard never reads a stale acquisition time.
    SYNC_LOCK_ACQUIRED_AT_MS.store(0, Ordering::SeqCst);
    SYNC_LOCKED.store(false, Ordering::SeqCst);
}

/// Bump the sync debounce generation. Returns the new generation number.
///
/// Editors call this when a layout change is detected. After a delay (e.g., 500ms),
/// they call `agent_doc_sync_check_generation(gen)` — if it returns `true`, the
/// generation is still current and the sync should proceed. If `false`, a newer
/// event superseded this one.
#[unsafe(no_mangle)]
pub extern "C" fn agent_doc_sync_bump_generation() -> u64 {
    SYNC_GENERATION.fetch_add(1, Ordering::SeqCst) + 1
}

/// Check if a generation is still current. Returns `true` if `generation` matches the
/// latest generation (no newer events have been scheduled).
#[unsafe(no_mangle)]
pub extern "C" fn agent_doc_sync_check_generation(generation: u64) -> i32 {
    (SYNC_GENERATION.load(Ordering::SeqCst) == generation) as i32
}

/// Start the IPC socket listener on a background thread.
///
/// The plugin calls this on project open to start listening for socket IPC
/// messages from the CLI. The callback receives each JSON message as a
/// read-only NUL-terminated string (do NOT free it) and returns `true` if
/// the message was handled successfully, `false` on error. The listener
/// generates the socket receipt response internally based on the return value.
///
/// Returns `true` if the listener was started, `false` on error.
///
/// # Safety
///
/// - `project_root` must be a valid, NUL-terminated UTF-8 string.
/// - `callback` must be a valid function pointer that remains valid for the
///   lifetime of the listener thread. The message pointer is borrowed — the
///   callback must NOT free it or hold a reference after returning.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_start_ipc_listener(
    project_root: *const c_char,
    callback: extern "C" fn(message: *const c_char) -> i32,
) -> i32 {
    let root_str = match unsafe { CStr::from_ptr(project_root) }.to_str() {
        Ok(s) => s.to_string(),
        Err(_) => return 0,
    };
    spawn_ipc_listener(root_str, "v1", move |msg| {
        // Lend the message to the callback (no ownership transfer).
        let c_msg = match CString::new(msg) {
            Ok(c) => c,
            Err(_) => {
                return Some(r#"{"type":"receipt","status":"rejected"}"#.to_string());
            }
        };
        let success = callback(c_msg.as_ptr()) != 0;
        if success {
            Some(r#"{"type":"receipt","status":"applied"}"#.to_string())
        } else {
            Some(r#"{"type":"receipt","status":"rejected"}"#.to_string())
        }
    })
}

/// V2 of [`agent_doc_start_ipc_listener`] with extended receipt-result encoding.
///
/// The callback returns one of three values:
/// - `0` → receipt `{"type":"receipt","status":"rejected"}` (apply failed)
/// - `1` → receipt `{"type":"receipt","status":"applied"}` (apply succeeded)
/// - `2` → receipt `{"type":"receipt","status":"applied","reason":"already_applied"}`
///   (plugin detected the patch is already in the live buffer and chose NOT
///   to re-apply; binary skips redundant CP delivery so a duplicate response
///   heading cannot land).
///
/// Plugins should prefer v2 when available.
///
/// Early receipt (`#ipc-early-receipt`): the underlying
/// `agent_doc_ipc_io::start_listener` transport emits an `accepted` receipt the
/// instant it receives a patch that opted in (`"early_receipt": true`), before
/// this callback's blocking apply runs, then this callback's terminal receipt as
/// usual. The early receipt is owned by the Rust transport.
///
/// Plan: tasks/agent-doc/plan-ipc-corruption-and-duplicate-during-typing.md
/// `[#ipcpluginalready]`.
///
/// # Safety
///
/// Same as [`agent_doc_start_ipc_listener`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_start_ipc_listener_v2(
    project_root: *const c_char,
    callback: extern "C" fn(message: *const c_char) -> i32,
) -> i32 {
    let root_str = match unsafe { CStr::from_ptr(project_root) }.to_str() {
        Ok(s) => s.to_string(),
        Err(_) => return 0,
    };
    spawn_ipc_listener(root_str, "v2", move |msg| {
        let c_msg = match CString::new(msg) {
            Ok(c) => c,
            Err(_) => {
                return Some(r#"{"type":"receipt","status":"rejected"}"#.to_string());
            }
        };
        match callback(c_msg.as_ptr()) {
            1 => Some(r#"{"type":"receipt","status":"applied"}"#.to_string()),
            2 => Some(
                r#"{"type":"receipt","status":"applied","reason":"already_applied"}"#.to_string(),
            ),
            _ => Some(r#"{"type":"receipt","status":"rejected"}"#.to_string()),
        }
    })
}

fn spawn_ipc_listener<F>(root_str: String, label: &'static str, handler: F) -> i32
where
    F: Fn(&str) -> Option<String> + Send + Sync + 'static,
{
    if let Err(error) = agent_doc_ipc_io::set_local_build_id(concat!(
        env!("CARGO_PKG_VERSION"),
        "+",
        env!("AGENT_DOC_BUILD_TIMESTAMP")
    )) {
        eprintln!("[ffi] failed to initialize IPC build identity: {error}");
        return 0;
    }
    if NATIVE_GENERATION_QUIESCING.load(Ordering::SeqCst) {
        eprintln!("[ffi] refusing to start IPC listener while native generation is quiescing");
        return 0;
    }

    let mut generations = IPC_LISTENER_GENERATIONS.lock();
    if let Some(existing) = generations.remove(&root_str) {
        if !existing.thread.is_finished() {
            generations.insert(root_str, existing);
            return 1;
        }
        let _ = existing.thread.join();
    }

    let root = PathBuf::from(&root_str);
    let thread_root = root.clone();
    let shutdown = std::sync::Arc::new(AtomicBool::new(false));
    let thread_shutdown = std::sync::Arc::clone(&shutdown);
    let thread = match std::thread::Builder::new()
        .name(format!("agent-doc-ipc-listener-{label}"))
        .spawn(move || {
            let result = agent_doc_ipc_io::start_listener_with_logger_until(
                &thread_root,
                handler,
                agent_doc_ops_log_io::log_op,
                thread_shutdown,
            );
            if let Err(error) = result {
                eprintln!("[ffi] IPC listener {label} error: {error}");
            }
        }) {
        Ok(thread) => thread,
        Err(error) => {
            eprintln!("[ffi] failed to spawn IPC listener {label}: {error}");
            return 0;
        }
    };
    generations.insert(
        root_str,
        IpcListenerGeneration {
            root,
            shutdown,
            thread,
        },
    );
    1
}

fn stop_ipc_listener_generation(root_str: &str, timeout: std::time::Duration) -> bool {
    let Some(generation) = IPC_LISTENER_GENERATIONS.lock().remove(root_str) else {
        return true;
    };
    generation.shutdown.store(true, Ordering::SeqCst);
    let _ = agent_doc_ipc_io::wake_listener(&generation.root);
    let deadline = std::time::Instant::now() + timeout;
    while !generation.thread.is_finished() && std::time::Instant::now() < deadline {
        let _ = agent_doc_ipc_io::wake_listener(&generation.root);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    if generation.thread.is_finished() {
        let _ = generation.thread.join();
        true
    } else {
        IPC_LISTENER_GENERATIONS
            .lock()
            .insert(root_str.to_string(), generation);
        false
    }
}

/// Stop and join one IPC socket listener.
///
/// # Safety
///
/// `project_root` must be a valid, NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_stop_ipc_listener(project_root: *const c_char) -> c_int {
    let root_str = match unsafe { CStr::from_ptr(project_root) }.to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    if !stop_ipc_listener_generation(root_str, std::time::Duration::from_secs(7)) {
        eprintln!("[ffi] timed out joining IPC listener for {root_str}");
        0
    } else {
        1
    }
}

/// Quiesce every writable/background resource owned by this native generation.
///
/// Returns `1` only after all IPC accept/connection threads have joined and all
/// cdylib-hosted CRDT replicas have been dropped. A timeout returns `0`; the
/// caller must keep this generation loaded and may call
/// [`agent_doc_resume_after_reload_failure`] before restarting its listeners.
#[unsafe(no_mangle)]
pub extern "C" fn agent_doc_quiesce_for_reload(timeout_ms: i64) -> i32 {
    let timeout = std::time::Duration::from_millis(timeout_ms.max(1) as u64);
    NATIVE_GENERATION_QUIESCING.store(true, Ordering::SeqCst);
    agent_doc_editor_surface_io::quiesce_for_reload();
    let mut generations = {
        let mut registry = IPC_LISTENER_GENERATIONS.lock();
        registry.drain().collect::<Vec<_>>()
    };
    for (_, generation) in &generations {
        generation.shutdown.store(true, Ordering::SeqCst);
        let _ = agent_doc_ipc_io::wake_listener(&generation.root);
    }

    let deadline = std::time::Instant::now() + timeout;
    while generations
        .iter()
        .any(|(_, generation)| !generation.thread.is_finished())
        && std::time::Instant::now() < deadline
    {
        for (_, generation) in &generations {
            if !generation.thread.is_finished() {
                let _ = agent_doc_ipc_io::wake_listener(&generation.root);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    if generations
        .iter()
        .any(|(_, generation)| !generation.thread.is_finished())
    {
        let mut registry = IPC_LISTENER_GENERATIONS.lock();
        for (root, generation) in generations {
            registry.insert(root, generation);
        }
        NATIVE_GENERATION_QUIESCING.store(false, Ordering::SeqCst);
        agent_doc_editor_surface_io::resume_after_reload_failure();
        return 0;
    }
    for (_, generation) in generations.drain(..) {
        let _ = generation.thread.join();
    }
    agent_doc_ffi::close_all_replicas_for_reload();
    1
}

/// Re-open a quiesced generation after the replacement library failed to load.
#[unsafe(no_mangle)]
pub extern "C" fn agent_doc_resume_after_reload_failure() {
    NATIVE_GENERATION_QUIESCING.store(false, Ordering::SeqCst);
    agent_doc_editor_surface_io::resume_after_reload_failure();
}

/// Legacy ACK-content ABI retained only to fail old editor plugins closed.
///
/// Current editor plugins must publish lazily transport receipts through
/// `agent_doc_editor_patch_applied` / `agent_doc_editor_patch_rejected` and use
/// the capability-bearing editor content bridge below.
///
/// # Safety
///
/// `project_root` and `patch_id` must be valid, NUL-terminated UTF-8 strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_write_ack_content(
    project_root: *const c_char,
    patch_id: *const c_char,
    content: *const c_char,
) -> i32 {
    let root_str = match unsafe { CStr::from_ptr(project_root) }.to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let patch_id_str = match unsafe { CStr::from_ptr(patch_id) }.to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let _ = content;
    eprintln!(
        "[ffi] incompatible editor plugin for {root_str}: agent_doc_write_ack_content is no longer supported for patch_id {}; update/reinstall the plugin so it publishes lazily transport receipts",
        &patch_id_str[..patch_id_str.len().min(8)]
    );
    0
}

fn record_lazily_visible_write_receipt(
    project_root: &Path,
    file: &Path,
    patch_id_str: &str,
    content_str: &str,
    source: &str,
) -> anyhow::Result<()> {
    mark_embedded_editor_host();
    let proof =
        agent_doc_controller_io::project_controller::record_visible_write_commit_candidate_for_project_file(
            project_root,
            file,
            patch_id_str,
            content_str,
            source,
        )?;
    eprintln!(
        "[ffi] lazily visible-write receipt recorded: patch_id {} model_revision={} candidate_hash={} source={}",
        &patch_id_str[..patch_id_str.len().min(8)],
        proof.model_revision,
        proof.commit_candidate_hash,
        source
    );
    Ok(())
}

fn record_editor_patch_receipt(
    file_path: &str,
    patch_id: &str,
    actor_generation: u64,
    rejected_reason: Option<&str>,
) -> i32 {
    mark_embedded_editor_host();
    let canonical = Path::new(file_path)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(file_path));
    let document_hash = agent_doc_hash::document_id_for_path(&canonical);
    let event_suffix = match rejected_reason {
        Some(reason) => format!(
            "editor-patch-rejected-{patch_id}-{actor_generation}-{}",
            agent_doc_hash::content_hash(reason)
        ),
        None => format!("editor-patch-applied-{patch_id}-{actor_generation}"),
    };
    let generation_event = StateEvent::new(
        format!("{document_hash}:editor-patch-generation-{patch_id}-{actor_generation}"),
        StateFact::OwnerGenerationChanged {
            document_hash: document_hash.clone(),
            owner: agent_doc_state_backbone::StateOwner::EditorIpcBridge,
            generation: actor_generation,
        },
    );
    let receipt_event_id = format!("{document_hash}:{event_suffix}");
    let fact = match rejected_reason {
        Some(reason) => StateFact::EditorPatchRejected {
            document_hash: document_hash.clone(),
            patch_id: patch_id.to_string(),
            actor_generation,
            reason: reason.to_string(),
        },
        None => StateFact::EditorPatchApplied {
            document_hash: document_hash.clone(),
            patch_id: patch_id.to_string(),
            actor_generation,
        },
    };
    let receipt_event = StateEvent::new(receipt_event_id, fact);
    {
        let mut ledger = state_ledger().lock();
        ledger.append(generation_event.clone());
        ledger.append(receipt_event.clone());
    }

    let Some(project_root) = agent_doc_project_root_io::project_root_containing(&canonical) else {
        eprintln!(
            "[state-projection] editor patch receipt rejected for {file_path}: no agent-doc project root found"
        );
        return 0;
    };
    if let Err(err) =
        append_editor_state_event_from_ffi(&project_root, &canonical, &generation_event)
    {
        eprintln!(
            "[state-projection] editor patch receipt rejected for {file_path}: durable generation append failed: {err}"
        );
        return 0;
    }
    if let Err(err) = append_editor_state_event_from_ffi(&project_root, &canonical, &receipt_event)
    {
        eprintln!(
            "[state-projection] editor patch receipt rejected for {file_path}: durable receipt append failed: {err}"
        );
        return 0;
    }
    1
}

fn append_editor_state_event_from_ffi(
    project_root: &Path,
    file: &Path,
    event: &StateEvent,
) -> anyhow::Result<bool> {
    #[cfg(not(test))]
    {
        agent_doc_controller_io::project_controller::publish_editor_state_event_existing(
            project_root,
            file,
            event,
        )
    }
    #[cfg(test)]
    {
        let _ = file;
        agent_doc_controller_io::project_controller::append_state_event_for_test(
            project_root,
            event,
        )
    }
}

/// Record that the editor applied a queued patch as a lazily transport receipt.
///
/// # Safety
///
/// `file_path` and `patch_id` must be valid, NUL-terminated UTF-8 strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_editor_patch_applied(
    file_path: *const c_char,
    patch_id: *const c_char,
    actor_generation: u64,
) -> i32 {
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(path) => path,
        Err(_) => return 0,
    };
    let patch = match unsafe { CStr::from_ptr(patch_id) }.to_str() {
        Ok(patch) => patch,
        Err(_) => return 0,
    };
    record_editor_patch_receipt(path, patch, actor_generation, None)
}

/// Record that the editor rejected a queued patch as a lazily transport receipt.
///
/// # Safety
///
/// `file_path`, `patch_id`, and `reason` must be valid, NUL-terminated UTF-8 strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_editor_patch_rejected(
    file_path: *const c_char,
    patch_id: *const c_char,
    actor_generation: u64,
    reason: *const c_char,
) -> i32 {
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(path) => path,
        Err(_) => return 0,
    };
    let patch = match unsafe { CStr::from_ptr(patch_id) }.to_str() {
        Ok(patch) => patch,
        Err(_) => return 0,
    };
    let rejection = match unsafe { CStr::from_ptr(reason) }.to_str() {
        Ok(reason) => reason,
        Err(_) => return 0,
    };
    record_editor_patch_receipt(path, patch, actor_generation, Some(rejection))
}

/// Record editor-applied content through one lazily receipt capability-bearing
/// ABI call.
///
/// This is the preferred editor endpoint when the content came from the live
/// editor buffer. The lazily/controller receipt is authoritative; file-backed
/// live-buffer sidecars are not written from this path.
///
/// # Safety
///
/// All pointers must be valid, NUL-terminated UTF-8 strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_editor_content_applied_for_editor_v1(
    project_root: *const c_char,
    patch_id: *const c_char,
    file_path: *const c_char,
    content: *const c_char,
    editor_id: *const c_char,
    editor_kind: *const c_char,
    editor_version: *const c_char,
    capabilities_csv: *const c_char,
) -> i32 {
    let root_str = match unsafe { CStr::from_ptr(project_root) }.to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let patch_id_str = match unsafe { CStr::from_ptr(patch_id) }.to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(path) => path,
        Err(_) => return 0,
    };
    let text = match unsafe { CStr::from_ptr(content) }.to_str() {
        Ok(text) => text,
        Err(_) => return 0,
    };
    let _editor = match unsafe { CStr::from_ptr(editor_id) }.to_str() {
        Ok(editor) => editor,
        Err(_) => return 0,
    };
    let _kind = match unsafe { CStr::from_ptr(editor_kind) }.to_str() {
        Ok(kind) => kind,
        Err(_) => return 0,
    };
    let _version = match unsafe { CStr::from_ptr(editor_version) }.to_str() {
        Ok(version) => version,
        Err(_) => return 0,
    };
    let capabilities_raw = match unsafe { CStr::from_ptr(capabilities_csv) }.to_str() {
        Ok(capabilities) => capabilities,
        Err(_) => return 0,
    };
    let capabilities: Vec<&str> = capabilities_raw
        .split(',')
        .map(str::trim)
        .filter(|capability| !capability.is_empty())
        .collect();
    if !capabilities.contains(
        &agent_doc_document_realtime::editor_contract::LAZILY_TRANSPORT_RECEIPTS_CAPABILITY,
    ) {
        eprintln!(
            "[ffi] incompatible editor plugin for {path}: agent_doc_editor_content_applied_for_editor_v1 requires {}; update/reinstall the plugin so it publishes editor_patch_applied/editor_patch_rejected lazily receipts",
            agent_doc_document_realtime::editor_contract::LAZILY_TRANSPORT_RECEIPTS_CAPABILITY
        );
        return 0;
    }

    if let Err(err) = record_lazily_visible_write_receipt(
        Path::new(root_str),
        Path::new(path),
        patch_id_str,
        text,
        "editor_patch_applied_for_editor_v2",
    ) {
        eprintln!("[ffi] consolidated lazily receipt event failed for {path}: {err}");
        return 0;
    }
    eprintln!(
        "[ffi] lazily content proof recorded via CRDT/controller authority: {} bytes for patch_id {}",
        text.len(),
        &patch_id_str[..patch_id_str.len().min(8)]
    );
    1
}

/// Legacy ACK-content ABI retained only to fail old editor plugins closed.
///
/// # Safety
///
/// All pointers must be valid, NUL-terminated UTF-8 strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_write_ack_content_for_editor_v2(
    _project_root: *const c_char,
    patch_id: *const c_char,
    file_path: *const c_char,
    _content: *const c_char,
    _editor_id: *const c_char,
    _editor_kind: *const c_char,
    _editor_version: *const c_char,
    _capabilities_csv: *const c_char,
) -> i32 {
    let patch_id_str = match unsafe { CStr::from_ptr(patch_id) }.to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(path) => path,
        Err(_) => return 0,
    };
    eprintln!(
        "[ffi] incompatible editor plugin for {path}: agent_doc_write_ack_content_for_editor_v2 is no longer supported for patch_id {}; update/reinstall the plugin so it publishes lazily transport receipts",
        &patch_id_str[..patch_id_str.len().min(8)]
    );
    0
}

/// Return true when the current disk file still matches committed `HEAD` and
/// that committed document already contains the incoming response patch body.
///
/// This gives editor plugins a safe late-replay gate: a stale File Cache
/// Conflict accept should no-op only when both disk and HEAD prove that the
/// response is already committed. If disk has drifted away from HEAD, the
/// plugin must not use HEAD alone as proof because the patch may be needed to
/// restore a stale editor buffer.
///
/// # Safety
///
/// `file_path` and `content` must be valid, NUL-terminated UTF-8 strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_patch_content_already_committed(
    file_path: *const c_char,
    content: *const c_char,
) -> i32 {
    let file = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(s) => std::path::PathBuf::from(s),
        Err(_) => return 0,
    };
    let patch_content = match unsafe { CStr::from_ptr(content) }.to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let Some(head) = ffi_show_head(&file) else {
        return 0;
    };
    let Ok(current) = std::fs::read_to_string(&file) else {
        return 0;
    };
    if ffi_normalize_transient_agent_doc_markers(&current)
        != ffi_normalize_transient_agent_doc_markers(&head)
    {
        return 0;
    }
    ffi_response_already_applied(&head, patch_content) as i32
}

fn ffi_show_head(file: &std::path::Path) -> Option<String> {
    let parent = file.parent()?;
    let root = std::process::Command::new("git")
        .current_dir(parent)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|stdout| std::path::PathBuf::from(stdout.trim()))?;
    let relative = file.strip_prefix(&root).ok()?;
    let spec = format!("HEAD:{}", relative.to_string_lossy());
    std::process::Command::new("git")
        .current_dir(root)
        .args(["show", &spec])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
}

/// Recent prior-commit blob contents of `file` (newest first, up to `limit`
/// commits that touched the file). Used by the reconnect staleness check
/// (`#yzer`) to prove an editor buffer is stale committed content rather than
/// unsynced user edits. Best-effort: returns an empty vec on any git failure.
fn ffi_show_prior_blobs(file: &std::path::Path, limit: usize) -> Vec<String> {
    let Some(parent) = file.parent() else {
        return Vec::new();
    };
    let Some(root) = std::process::Command::new("git")
        .current_dir(parent)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|stdout| std::path::PathBuf::from(stdout.trim()))
    else {
        return Vec::new();
    };
    let Ok(relative) = file.strip_prefix(&root) else {
        return Vec::new();
    };
    let rel = relative.to_string_lossy().to_string();
    let commits = std::process::Command::new("git")
        .current_dir(&root)
        .args([
            "log",
            &format!("-n{limit}"),
            "--format=%H",
            "--",
            rel.as_str(),
        ])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .unwrap_or_default();
    commits
        .lines()
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .filter_map(|commit| {
            let spec = format!("{commit}:{rel}");
            std::process::Command::new("git")
                .current_dir(&root)
                .args(["show", &spec])
                .output()
                .ok()
                .filter(|output| output.status.success())
                .and_then(|output| String::from_utf8(output.stdout).ok())
        })
        .collect()
}

/// Decide how an editor plugin should reconcile its buffer with disk when it
/// (re)connects its IPC listener, and return the disk content to apply when a
/// re-read is warranted (`#yzer` / `#evmhplugin`).
///
/// Returns a JSON object (free with [`agent_doc_free_string`]):
/// ```json
/// {"decision": "reread_disk" | "keep_buffer" | "in_sync", "content": "<disk>"}
/// ```
/// `content` is present only for `reread_disk`. On any error the result is
/// `{"decision":"keep_buffer"}` (fail safe — never clobber the buffer on
/// uncertainty). Emits a `reconnect_buffer_decision` marker to the document's
/// `ops.log` so the path is verifiable from the logs.
///
/// # Safety
///
/// `project_root`, `file_path`, and `buffer_content` must be valid,
/// NUL-terminated UTF-8 strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_reconnect_buffer_decision(
    project_root: *const c_char,
    file_path: *const c_char,
    buffer_content: *const c_char,
) -> *mut c_char {
    use agent_doc_document_realtime::write_policy::{
        ReconnectBufferDecision, decide_reconnect_buffer,
    };
    let keep = || {
        CString::new(r#"{"decision":"keep_buffer"}"#)
            .unwrap()
            .into_raw()
    };
    let (Ok(file), Ok(buffer)) = (
        unsafe { CStr::from_ptr(file_path) }.to_str(),
        unsafe { CStr::from_ptr(buffer_content) }.to_str(),
    ) else {
        return keep();
    };
    let _ = project_root; // root is derived from the file's git toplevel
    let file_path_buf = std::path::PathBuf::from(file);
    let Ok(disk) = std::fs::read_to_string(&file_path_buf) else {
        return keep();
    };

    // Trailing-whitespace tolerant comparison: editor buffers routinely differ
    // from git blobs only by a trailing newline.
    let norm = |s: &str| s.trim_end().to_string();
    let buffer_n = norm(buffer);
    let disk_n = norm(&disk);

    let buffer_matches_disk = buffer_n == disk_n;
    let head = ffi_show_head(&file_path_buf);
    let disk_is_committed_head = head.as_deref().map(norm) == Some(disk_n.clone());
    let buffer_matches_prior_commit = ffi_show_prior_blobs(&file_path_buf, 10)
        .iter()
        .any(|blob| norm(blob) == buffer_n);

    let decision = decide_reconnect_buffer(
        buffer_matches_disk,
        disk_is_committed_head,
        buffer_matches_prior_commit,
    );

    agent_doc_ops_log_io::log_op(
        &file_path_buf,
        &format!(
            "reconnect_buffer_decision decision={} buffer_matches_disk={} disk_is_head={} buffer_is_prior_commit={} #yzer",
            decision.as_str(),
            buffer_matches_disk,
            disk_is_committed_head,
            buffer_matches_prior_commit,
        ),
    );

    let json = match decision {
        ReconnectBufferDecision::RereadDisk => serde_json::json!({
            "decision": decision.as_str(),
            "content": disk,
        }),
        _ => serde_json::json!({ "decision": decision.as_str() }),
    };
    match CString::new(json.to_string()) {
        Ok(c) => c.into_raw(),
        Err(_) => keep(),
    }
}

/// Record one real editor operation for op-capture (`#qnodemerge4wire` part 2).
///
/// The FFI-first ingestion point the thin editor reporters (JetBrains
/// `DocumentListener.documentChanged`, VS Code `onDidChangeTextDocument`) call to
/// feed the editor's *real* operations to the merge instead of a Myers
/// diff-guess. A replacement is reported as a `delete` followed by an `insert`.
///
/// `op_kind` is `"insert"` or `"delete"`. For `"insert"`, `insert_text` is the
/// inserted text and `delete_len` is ignored. For `"delete"`, `delete_len` is the
/// number of bytes removed at `offset` and `insert_text` may be null. `offset`
/// and `delete_len` are byte offsets/lengths into the buffer the op was captured
/// against, whose text hashes to `base_hash` (see
/// [`agent_doc_hash::content_hash`]). A later merge trusts
/// the op only when its resolved base hashes to the same value; otherwise the op
/// is silently disqualified and the merge falls back to the diff-guess (never
/// worse than today).
///
/// Returns `1` when the op was durably recorded, `0` on any error (bad UTF-8,
/// unknown kind, negative offset/len, or I/O failure) so the reporter can fall
/// back to the diff-guess path.
///
/// # Safety
///
/// `file_path`, `base_hash`, and `op_kind` must be valid, NUL-terminated UTF-8
/// strings. `insert_text` may be null (for deletes) or a valid NUL-terminated
/// UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_record_editor_op(
    file_path: *const c_char,
    base_hash: *const c_char,
    op_kind: *const c_char,
    offset: i64,
    insert_text: *const c_char,
    delete_len: i64,
) -> i32 {
    let (Ok(file), Ok(base), Ok(kind)) = (
        unsafe { CStr::from_ptr(file_path) }.to_str(),
        unsafe { CStr::from_ptr(base_hash) }.to_str(),
        unsafe { CStr::from_ptr(op_kind) }.to_str(),
    ) else {
        eprintln!("[op-capture] agent_doc_record_editor_op: non-UTF-8 argument; ignoring op");
        return 0;
    };
    let Ok(offset) = usize::try_from(offset) else {
        eprintln!("[op-capture] agent_doc_record_editor_op: negative offset {offset}; ignoring op");
        return 0;
    };
    let file_path_buf = std::path::PathBuf::from(file);

    let op = match kind {
        "insert" => {
            if insert_text.is_null() {
                eprintln!(
                    "[op-capture] agent_doc_record_editor_op: insert with null text; ignoring op"
                );
                return 0;
            }
            let Ok(text) = unsafe { CStr::from_ptr(insert_text) }.to_str() else {
                eprintln!(
                    "[op-capture] agent_doc_record_editor_op: non-UTF-8 insert text; ignoring op"
                );
                return 0;
            };
            agent_doc_merge::crdt::EditorOp::Insert {
                offset,
                text: text.to_string(),
            }
        }
        "delete" => {
            let Ok(len) = usize::try_from(delete_len) else {
                eprintln!(
                    "[op-capture] agent_doc_record_editor_op: negative delete_len {delete_len}; ignoring op"
                );
                return 0;
            };
            agent_doc_merge::crdt::EditorOp::Delete { offset, len }
        }
        other => {
            eprintln!(
                "[op-capture] agent_doc_record_editor_op: unknown op_kind {other:?}; ignoring"
            );
            return 0;
        }
    };

    let op_log_summary = match &op {
        agent_doc_merge::crdt::EditorOp::Insert { text, .. } => format!(
            "insert_bytes={} insert_non_ascii={}",
            text.len(),
            !text.is_ascii()
        ),
        agent_doc_merge::crdt::EditorOp::Delete { len, .. } => {
            format!("delete_len={len}")
        }
    };

    match agent_doc_op_capture_io::record_editor_op(&file_path_buf, base, op) {
        Ok(()) => {
            agent_doc_ops_log_io::log_op(
                &file_path_buf,
                &format!(
                    "{} kind={kind} offset={offset} {op_log_summary} base={} #qnodemerge4wire",
                    OpsLogEvent::EditorOpRecorded,
                    base.get(..12).unwrap_or(base),
                ),
            );
            1
        }
        Err(e) => {
            agent_doc_ops_log_io::log_op(
                &file_path_buf,
                &format!(
                    "{} kind={kind} error={e} #qnodemerge4wire",
                    OpsLogEvent::EditorOpRecordFailed
                ),
            );
            0
        }
    }
}

/// Record an ordered editor-op burst in one bounded state-ledger transaction.
///
/// `ops_json` is a JSON array of [`agent_doc_merge::crdt::EditorOp`] values,
/// e.g. `[{"kind":"insert","offset":3,"text":"x"}]`. Returns `1` when the
/// complete batch was durably recorded and `0` on malformed input or I/O
/// failure. The caller can safely fall back to the diff-guess path on failure.
///
/// # Safety
///
/// `file_path`, `base_hash`, and `ops_json` must be valid, NUL-terminated UTF-8
/// strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_record_editor_ops_json(
    file_path: *const c_char,
    base_hash: *const c_char,
    ops_json: *const c_char,
) -> i32 {
    let (Ok(file), Ok(base), Ok(ops_json)) = (
        unsafe { CStr::from_ptr(file_path) }.to_str(),
        unsafe { CStr::from_ptr(base_hash) }.to_str(),
        unsafe { CStr::from_ptr(ops_json) }.to_str(),
    ) else {
        eprintln!(
            "[op-capture] agent_doc_record_editor_ops_json: non-UTF-8 argument; ignoring batch"
        );
        return 0;
    };
    let Ok(ops) = serde_json::from_str::<Vec<agent_doc_merge::crdt::EditorOp>>(ops_json) else {
        eprintln!(
            "[op-capture] agent_doc_record_editor_ops_json: malformed ops JSON; ignoring batch"
        );
        return 0;
    };
    if ops.is_empty() {
        return 1;
    }

    let file_path_buf = std::path::PathBuf::from(file);
    let op_count = ops.len();
    match agent_doc_op_capture_io::record_editor_ops(&file_path_buf, base, ops) {
        Ok(()) => {
            agent_doc_ops_log_io::log_op(
                &file_path_buf,
                &format!(
                    "editor_ops_recorded count={op_count} base={} transaction=batch #qbasehashmemo",
                    base.get(..12).unwrap_or(base),
                ),
            );
            1
        }
        Err(error) => {
            agent_doc_ops_log_io::log_op(
                &file_path_buf,
                &format!(
                    "editor_ops_record_failed count={op_count} transaction=batch error={error} #qbasehashmemo"
                ),
            );
            0
        }
    }
}

/// Close the current editor-op epoch before applying a non-operator projection.
///
/// Captured operator operations are meaningful only within one uninterrupted
/// editor frontier. A remote/agent mutation changes that frontier even when the
/// CRDT merge base sidecar has not advanced yet; retaining the old sidecar would
/// concatenate later operator edits onto an obsolete base. Editor integrations
/// call this immediately before applying any non-operator text mutation.
///
/// Returns `1` when the epoch was cleared (including an already-empty epoch), or
/// `0` for invalid UTF-8 / I/O failure.
///
/// # Safety
///
/// `file_path` must be a valid, NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_clear_editor_op_epoch(file_path: *const c_char) -> i32 {
    let Ok(file) = (unsafe { CStr::from_ptr(file_path) }).to_str() else {
        eprintln!("[op-capture] agent_doc_clear_editor_op_epoch: non-UTF-8 path; retaining epoch");
        return 0;
    };
    let file_path_buf = std::path::PathBuf::from(file);
    match agent_doc_op_capture_io::clear_op_capture_for_editor_projection(&file_path_buf) {
        Ok(()) => {
            agent_doc_ops_log_io::log_op(
                &file_path_buf,
                "editor_op_epoch_closed cause=non_operator_projection action=cleared",
            );
            1
        }
        Err(err) => {
            agent_doc_ops_log_io::log_op(
                &file_path_buf,
                &format!("editor_op_epoch_close_failed cause=non_operator_projection error={err}"),
            );
            0
        }
    }
}

/// Compute the `base_hash` the editor op reporters must stamp on captured ops
/// (`#qnodemerge4wire`) so the write-time merge accepts them — the SHA256 hex of
/// the same CRDT merge base text [`agent_doc_merge_io::merge_contents_crdt_with_ops`]
/// resolves. The reporter calls this once per edit (cheap; the editor offsets it
/// pairs with are relative to this base) and passes the result to
/// [`agent_doc_record_editor_op`].
///
/// Returns a NUL-terminated string (the empty-text hash when no snapshot/CRDT
/// base exists yet), or null on a bad path / resolution error so the reporter
/// skips op capture for this edit and the merge falls back to the diff-guess —
/// never worse than today. Caller must free with `agent_doc_free_string`.
///
/// # Safety
///
/// `file_path` must be a valid, NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_document_base_hash(file_path: *const c_char) -> *mut c_char {
    let Ok(path) = (unsafe { CStr::from_ptr(file_path) }).to_str() else {
        eprintln!("[op-capture] agent_doc_document_base_hash: non-UTF-8 path; returning null");
        return std::ptr::null_mut();
    };
    match agent_doc_op_capture_io::current_base_hash_with(
        std::path::Path::new(path),
        |_doc, baseline| Ok(agent_doc_merge::crdt::CrdtDoc::from_text(baseline).encode_state()),
    ) {
        Ok(hash) => CString::new(hash)
            .map(|c| c.into_raw())
            .unwrap_or(std::ptr::null_mut()),
        Err(e) => {
            eprintln!("[op-capture] agent_doc_document_base_hash: {e}; returning null");
            std::ptr::null_mut()
        }
    }
}

fn ffi_normalize_transient_agent_doc_markers(content: &str) -> String {
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("<!-- agent:boundary:") {
                return None;
            }
            let stripped = if (trimmed.starts_with('#') || trimmed.starts_with("**"))
                && line.ends_with(" (HEAD)")
            {
                line.strip_suffix(" (HEAD)").unwrap_or(line)
            } else {
                line
            };
            Some(stripped.to_string())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn ffi_response_already_applied(doc: &str, response: &str) -> bool {
    let response_lines = ffi_normalized_response_lines(response);
    if response_lines.is_empty() {
        return false;
    }
    let doc_lines = ffi_normalized_response_lines(doc);
    doc_lines
        .windows(response_lines.len())
        .any(|window| window == response_lines.as_slice())
}

fn ffi_normalized_response_lines(content: &str) -> Vec<String> {
    content
        .lines()
        .filter_map(ffi_normalize_response_line)
        .collect()
}

fn ffi_normalize_response_line(line: &str) -> Option<String> {
    let raw = line.trim_end_matches('\r');
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed.starts_with("<!-- patch:")
        || trimmed.starts_with("<!-- /patch:")
        || trimmed.starts_with("<!-- agent:")
        || trimmed.starts_with("<!-- /agent:")
    {
        return None;
    }
    if let Some(stripped) = raw.strip_suffix(" (HEAD)") {
        let trimmed = stripped.trim_start();
        if trimmed.starts_with("### Re:") || trimmed.starts_with("**Re:") && trimmed.ends_with("**")
        {
            return Some(stripped.to_string());
        }
    }
    let trimmed_start = raw.trim_start();
    if let Some(stripped) = trimmed_start.strip_prefix("❯ ") {
        let indent_len = raw.len() - trimmed_start.len();
        return Some(format!("{}{}", &raw[..indent_len], stripped));
    }
    Some(raw.to_string())
}

/// Commit the document at `file_path` to git (Fix 4: plugin post-apply commit).
///
/// Defense-in-depth guarantee: editor plugins call this after successfully applying
/// a patch so the agent response is committed even when the shell-side `--commit`
/// was skipped (e.g., IPC timeout path). Uses a minimal inline git flow rather than
/// the full `git::commit` (which lives in the binary crate) — stages the document
/// file and commits with a timestamp message.
///
/// Returns `true` on success, `false` on failure (null pointer, invalid UTF-8, git error).
///
/// # Safety
///
/// `file_path` must be a valid NUL-terminated UTF-8 string, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_commit(file_path: *const c_char) -> i32 {
    if file_path.is_null() {
        return 0;
    }
    let path_str = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let path = std::path::Path::new(path_str);
    ffi_git_commit(path) as i32
}

/// Minimal git commit for the FFI context (no full git module dependency).
/// Stages the snapshot if present (via hash-object + update-index), falls back
/// to `git add`, then commits. Skips hooks, HEAD markers, and ops logging — those
/// live in the binary's git::commit. This is a best-effort backstop only.
fn ffi_git_commit(file: &std::path::Path) -> bool {
    let parent = file.parent().unwrap_or(file);
    // Unset git env vars inherited from parent hooks (GIT_DIR etc.) so git
    // discovers the correct repo from current_dir rather than the outer repo.
    let git_root_out = std::process::Command::new("git")
        .current_dir(parent)
        .env_remove("GIT_DIR")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_WORK_TREE")
        .args(["rev-parse", "--show-toplevel"])
        .output();
    let git_root = match git_root_out {
        Ok(o) if o.status.success() => {
            std::path::PathBuf::from(String::from_utf8_lossy(&o.stdout).trim().to_string())
        }
        _ => return false,
    };

    let doc_name = file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");
    // Use unix timestamp (chrono not available in lib crate); full datetime in binary git::commit
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let msg = format!("agent-doc({}): {}", doc_name, secs);

    // Stage the document file
    let add_ok = std::process::Command::new("git")
        .current_dir(&git_root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_WORK_TREE")
        .args(["add", &file.to_string_lossy()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !add_ok {
        eprintln!(
            "[ffi] agent_doc_commit: git add failed for {}",
            file.display()
        );
        return false;
    }

    std::process::Command::new("git")
        .current_dir(&git_root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_WORK_TREE")
        .args(["commit", "-m", &msg, "--no-verify"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Get the agent-doc library version.
///
/// Returns a NUL-terminated string like "0.26.1".
/// Caller must free with `agent_doc_free_string`.
#[unsafe(no_mangle)]
pub extern "C" fn agent_doc_version() -> *mut c_char {
    mark_embedded_editor_host();
    CString::new(env!("CARGO_PKG_VERSION")).unwrap().into_raw()
}

#[inline]
fn mark_embedded_editor_host() {
    // Unit tests exercise the legacy pure/local adapters without an IDE host.
    // Every shipped cdylib entrypoint that can cross the controller boundary
    // calls this before doing so.
    #[cfg(not(test))]
    {
        agent_doc_sqlite::state_store::forbid_state_db_connections_for_process();
        agent_doc_controller_io::project_controller::mark_embedded_native_host();
    }
}

fn observe_editor_current_from_ffi(
    project_root: &Path,
    file: &Path,
    editor_content: &str,
    source: &str,
) -> anyhow::Result<()> {
    #[cfg(not(test))]
    {
        let Some(document) =
            agent_doc_controller_io::project_controller::document_state_projection_existing(
                project_root,
                file,
            )?
        else {
            return Ok(());
        };
        let Some(pending) = document.document.pending_external_disk else {
            return Ok(());
        };
        let editor_hash = agent_doc_hash::content_hash(editor_content);
        let supersedes = if pending.expected_hash.is_empty() {
            !editor_hash.eq_ignore_ascii_case(&pending.target_hash)
        } else {
            agent_doc_document_realtime::external_disk_decision(
                &pending.expected_hash,
                &pending.target_hash,
                &editor_hash,
            ) == agent_doc_document_realtime::ExternalDiskDecision::EditorSupersedes
        };
        if supersedes {
            append_editor_convergence_from_ffi(project_root, file, pending, &editor_hash, source)?;
        }
        Ok(())
    }
    #[cfg(test)]
    {
        let _ = project_root;
        agent_doc_document_realtime_io::clear_pending_external_disk_decision_on_editor_edit(
            file,
            editor_content,
            source,
        )
        .map(|_| ())
    }
}

fn document_closed_from_ffi(project_root: &Path, file: &Path) -> anyhow::Result<bool> {
    #[cfg(not(test))]
    {
        // Reliable-sync Close membership is already sent to the controller,
        // which owns last-editor-close projection. The native library must not
        // independently materialize that durable transition.
        let _ = (project_root, file);
        Ok(false)
    }
    #[cfg(test)]
    {
        let _ = project_root;
        agent_doc_document_realtime_io::materialize_last_editor_close_through_authority(
            file,
            "last_editor_closed",
        )
    }
}

struct EditorReconnectProjection {
    content: Option<String>,
    external_disk_pending: bool,
}

fn editor_reconnect_projection_from_ffi(
    project_root: &Path,
    file: &Path,
    editor_content: &str,
) -> anyhow::Result<EditorReconnectProjection> {
    #[cfg(not(test))]
    {
        let Some(document) =
            agent_doc_controller_io::project_controller::document_state_projection_existing(
                project_root,
                file,
            )?
        else {
            return Ok(EditorReconnectProjection {
                content: None,
                external_disk_pending: false,
            });
        };
        let editor_hash = agent_doc_hash::content_hash(editor_content);
        if let Some(pending) = document.document.pending_external_disk {
            return match agent_doc_document_realtime::external_disk_decision(
                &pending.expected_hash,
                &pending.target_hash,
                &editor_hash,
            ) {
                agent_doc_document_realtime::ExternalDiskDecision::AcceptedInEditor => {
                    Ok(EditorReconnectProjection {
                        content: Some(pending.target_content),
                        external_disk_pending: true,
                    })
                }
                agent_doc_document_realtime::ExternalDiskDecision::PendingUserDecision => {
                    Ok(EditorReconnectProjection {
                        content: None,
                        external_disk_pending: true,
                    })
                }
                agent_doc_document_realtime::ExternalDiskDecision::EditorSupersedes => {
                    append_editor_convergence_from_ffi(
                        project_root,
                        file,
                        pending,
                        &editor_hash,
                        "editor_reconnect_superseded_external_disk",
                    )?;
                    Ok(EditorReconnectProjection {
                        content: None,
                        external_disk_pending: false,
                    })
                }
            };
        }
        let pending = document
            .document
            .pending_write_journal
            .last()
            .cloned()
            .or(document.document.pending_write);
        Ok(EditorReconnectProjection {
            content: pending.and_then(|pending| {
                editor_hash
                    .eq_ignore_ascii_case(&pending.target_hash)
                    .then_some(pending.target_content)
            }),
            external_disk_pending: false,
        })
    }
    #[cfg(test)]
    {
        let _ = project_root;
        let content = agent_doc_document_realtime_io::deferred_document_write_reconnect_content(
            file,
            editor_content,
        )?;
        let external_disk_pending =
            agent_doc_document_realtime_io::pending_external_disk_candidate(file).is_some();
        Ok(EditorReconnectProjection {
            content,
            external_disk_pending,
        })
    }
}

fn deferred_write_reconnect_propagated_from_ffi(
    project_root: &Path,
    file: &Path,
    editor_content: &str,
) -> anyhow::Result<bool> {
    #[cfg(not(test))]
    {
        let Some(document) =
            agent_doc_controller_io::project_controller::document_state_projection_existing(
                project_root,
                file,
            )?
        else {
            return Ok(false);
        };
        let Some(pending) = document.document.pending_external_disk else {
            return Ok(false);
        };
        let editor_hash = agent_doc_hash::content_hash(editor_content);
        if !editor_hash.eq_ignore_ascii_case(&pending.target_hash) {
            return Ok(false);
        }
        append_editor_convergence_from_ffi(
            project_root,
            file,
            pending,
            &editor_hash,
            "editor_crdt_reconnect_propagated",
        )?;
        Ok(true)
    }
    #[cfg(test)]
    {
        let _ = project_root;
        agent_doc_document_realtime_io::clear_pending_external_disk_decision_after_editor_propagation(
            file,
            editor_content,
            "editor_crdt_reconnect_propagated",
        )
    }
}

#[cfg(not(test))]
fn append_editor_convergence_from_ffi(
    project_root: &Path,
    file: &Path,
    pending: agent_doc_state_backbone::DocumentWriteIntentProjection,
    target_hash: &str,
    source: &str,
) -> anyhow::Result<()> {
    let document_hash = agent_doc_hash::document_id_for_path(file);
    let event = StateEvent::new(
        format!(
            "document-write-converged-{document_hash}-{}",
            pending.intent_id
        ),
        StateFact::DocumentWriteConverged {
            document_hash,
            intent_id: pending.intent_id,
            target_hash: target_hash.to_string(),
            source: source.to_string(),
            intent_source: pending.source,
        },
    );
    append_editor_state_event_from_ffi(project_root, file, &event)?;
    Ok(())
}

/// Process-global lazily state-backbone event ledger backing the FFI
/// state-projection exports. Phase 1 of `tasks/software/plan-lazily-ffi-state-projection.md`
/// (`#lzffistate1`): plugins become thin event-reporters + projection-renderers while
/// the binary owns the durable FSMs. Default-initialized on first access.
static STATE_LEDGER: std::sync::OnceLock<parking_lot::Mutex<EventLedger>> =
    std::sync::OnceLock::new();

fn state_ledger() -> &'static parking_lot::Mutex<EventLedger> {
    STATE_LEDGER.get_or_init(|| parking_lot::Mutex::new(EventLedger::new()))
}

/// Stamp a queue `StateFact` reported without an explicit `hosting_epoch` with
/// the document's current hosting epoch (`#xdocsuper3`). The binary owns the
/// gate so live producers stay thin: a queue fact reported during the current
/// hosting carries that epoch and is rejected as stale after a host/switch.
/// Facts that already carry an explicit `hosting_epoch` (e.g. an intentional
/// stale-replay test) are left untouched, as are non-queue facts.
fn stamp_queue_fact_hosting_epoch(ledger: &EventLedger, event: &mut StateEvent) {
    use agent_doc_state_backbone::StateFact;
    let current = ledger.document_hosting_epoch(event.document_hash());
    let Some(current) = current else {
        return;
    };
    match &mut event.fact {
        StateFact::QueueHeadSelected { hosting_epoch, .. }
        | StateFact::QueueHeadDeferred { hosting_epoch, .. }
        | StateFact::QueueHeadCompleted { hosting_epoch, .. }
        | StateFact::QueueWorklistProjected { hosting_epoch, .. }
            if hosting_epoch.is_none() =>
        {
            *hosting_epoch = Some(current);
        }
        _ => {}
    }
}

/// Read the binary's lazily-powered state projection for a document.
///
/// Returns a NUL-terminated JSON string of `DocumentStateProjection`
/// (document/queue/closeout/transport/supervisor/route/proof slices) for the given
/// `document_hash`, or `"null"` when no events have been recorded for it. The
/// projection is recomputed from the in-process event ledger on every call. Plugins
/// render this without re-deriving the durable FSMs (FFI-first rule).
///
/// Caller must free the returned pointer with `agent_doc_free_string`.
///
/// # Safety
///
/// `document_hash` must be a valid, NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_state_projection(document_hash: *const c_char) -> *mut c_char {
    let doc_hash = match unsafe { CStr::from_ptr(document_hash) }.to_str() {
        Ok(s) => s,
        Err(err) => {
            eprintln!(
                "[state-projection] agent_doc_state_projection: non-UTF-8 document_hash; returning null: {err}"
            );
            return CString::new("null").unwrap().into_raw();
        }
    };
    let json = match state_ledger().lock().project().document(doc_hash) {
        Some(doc) => serde_json::to_string(doc).unwrap_or_else(|err| {
            eprintln!(
                "[state-projection] agent_doc_state_projection: serialize failed for {doc_hash}: {err}"
            );
            "null".to_string()
        }),
        None => "null".to_string(),
    };
    CString::new(json)
        .unwrap_or_else(|_| CString::new("null").unwrap())
        .into_raw()
}

/// Reconcile an editor buffer that re-registers after a deferred document
/// write. Ordinary CRDT-delivery targets retain their proven merge lineage.
/// An explicit external-disk target remains pending while the editor shows its
/// pre-write cut and is cleared when a newer editor cut appears.
///
/// Returns non-null only when the result is byte-identical to the visible editor
/// cut. Divergent retained targets remain binary-owned semantic intents; an
/// editor refresh must publish its exact live baseline before normal delivery
/// replays them. This fail-closed filter protects older plugin jars that treated
/// a divergent result as permission to replace and save the whole document.
/// Caller must free a non-null result with [`agent_doc_free_string`].
///
/// # Safety
///
/// Both arguments must be valid, NUL-terminated UTF-8 strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_deferred_write_reconnect_content(
    file_path: *const c_char,
    editor_content: *const c_char,
) -> *mut c_char {
    let Ok(path) = (unsafe { CStr::from_ptr(file_path) }).to_str() else {
        eprintln!("[deferred-write] reconnect: non-UTF-8 file path; returning null");
        return std::ptr::null_mut();
    };
    let Ok(editor_content) = (unsafe { CStr::from_ptr(editor_content) }).to_str() else {
        eprintln!("[deferred-write] reconnect: non-UTF-8 editor content; returning null");
        return std::ptr::null_mut();
    };
    let file = std::path::Path::new(path);
    let recovered = deferred_write_reconnect_candidate(file, editor_content);
    let recovered = exact_editor_reregister_candidate(file, editor_content, recovered);
    recovered
        .and_then(|content| CString::new(content).ok())
        .map(CString::into_raw)
        .unwrap_or(std::ptr::null_mut())
}

/// Recover a deferred write after a replacement replica has registered the
/// exact visible editor baseline.
///
/// Unlike [`agent_doc_deferred_write_reconnect_content`], this entrypoint may
/// return a different document. The returned content is a validated semantic
/// replay over `editor_content`; callers must fence the still-visible editor
/// bytes, install through the editor API, persist safely, and publish the
/// resulting local CRDT delta.
///
/// Caller must free a non-null result with [`agent_doc_free_string`].
///
/// # Safety
///
/// Both arguments must be valid, NUL-terminated UTF-8 strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_deferred_write_post_register_content(
    file_path: *const c_char,
    editor_content: *const c_char,
) -> *mut c_char {
    let Ok(path) = (unsafe { CStr::from_ptr(file_path) }).to_str() else {
        eprintln!("[deferred-write] post-register reconnect: non-UTF-8 file path; returning null");
        return std::ptr::null_mut();
    };
    let Ok(editor_content) = (unsafe { CStr::from_ptr(editor_content) }).to_str() else {
        eprintln!(
            "[deferred-write] post-register reconnect: non-UTF-8 editor content; returning null"
        );
        return std::ptr::null_mut();
    };
    deferred_write_reconnect_candidate(std::path::Path::new(path), editor_content)
        .and_then(|content| CString::new(content).ok())
        .map(CString::into_raw)
        .unwrap_or(std::ptr::null_mut())
}

/// Reconstruct a retained post-registration write and project it through the
/// controller-owned CRDT authority. The ABI returns status only: editor bytes
/// must arrive through ordinary replica delivery so a queued EDT effect can be
/// fenced against retired, superseded, or model-less endpoints.
///
/// # Safety
///
/// Both pointers must be non-null, NUL-terminated UTF-8 strings that remain
/// valid for the duration of this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_deferred_write_post_register_project(
    file_path: *const c_char,
    editor_content: *const c_char,
) -> c_int {
    let Ok(path) = (unsafe { CStr::from_ptr(file_path) }).to_str() else {
        eprintln!(
            "[deferred-write] post-register projection: non-UTF-8 file path; refusing projection"
        );
        return 0;
    };
    let Ok(editor_content) = (unsafe { CStr::from_ptr(editor_content) }).to_str() else {
        eprintln!(
            "[deferred-write] post-register projection: non-UTF-8 editor content; refusing projection"
        );
        return 0;
    };
    let file = std::path::Path::new(path);
    let Some(recovered) = deferred_write_reconnect_candidate(file, editor_content) else {
        return 1;
    };
    if recovered == editor_content {
        return 1;
    }
    match agent_doc_document_realtime_io::apply_cp_write_through_relay_authority(
        file,
        editor_content,
        &recovered,
        "editor_post_register_projection",
    ) {
        Ok(Some(write)) if write.applied => {
            eprintln!(
                "[deferred-write] post-register intent projected for {}; target_hash={} targets={} live_editors={}",
                file.display(),
                write.content_hash,
                write.targets,
                write.live_editors,
            );
            1
        }
        Ok(Some(write)) => {
            eprintln!(
                "[deferred-write] post-register intent refused for {}; reason=projection_not_applied target_hash={}",
                file.display(),
                write.content_hash,
            );
            0
        }
        Ok(None) => {
            eprintln!(
                "[deferred-write] post-register intent refused for {}; reason=no_live_controller_model",
                file.display(),
            );
            0
        }
        Err(err) => {
            eprintln!(
                "[deferred-write] post-register intent projection failed for {}: {err}",
                file.display(),
            );
            0
        }
    }
}

fn deferred_write_reconnect_candidate(
    file: &std::path::Path,
    editor_content: &str,
) -> Option<String> {
    mark_embedded_editor_host();
    let project_root = agent_doc_project_root_io::project_root_containing(file)?;
    match editor_reconnect_projection_from_ffi(&project_root, file, editor_content) {
        Ok(projection) if projection.content.is_some() => projection.content,
        Ok(projection) if projection.external_disk_pending => None,
        Ok(_) => match agent_doc_capture_io::load_active(file) {
            Ok(Some(capture))
                if capture.committed_at.is_none()
                    && capture.discarded_at.is_none()
                    && !capture.response_body.trim().is_empty()
                    && !agent_doc_turn::response_replay::response_materialized_in_content(
                        &capture.response_body,
                        editor_content,
                    ) =>
            {
                match agent_doc_template_io::parse_template_patchback(
                    file,
                    &capture.response_body,
                    "editor_reconnect_capture_fallback",
                    agent_doc_ops_log_io::log_op,
                )
                .and_then(|plan| {
                    agent_doc_template_io::apply_patches(
                        editor_content,
                        &plan.patches,
                        &plan.unmatched,
                        file,
                    )
                }) {
                    Ok(content)
                        if agent_doc_turn::response_replay::response_materialized_in_content(
                            &capture.response_body,
                            &content,
                        ) =>
                    {
                        Some(content)
                    }
                    Ok(_) => None,
                    Err(err) => {
                        eprintln!(
                            "[deferred-write] capture fallback failed for {}: {err}",
                            file.display()
                        );
                        None
                    }
                }
            }
            Ok(_) => None,
            Err(err) => {
                eprintln!(
                    "[deferred-write] capture lookup failed for {}: {err}",
                    file.display()
                );
                None
            }
        },
        Err(err) => {
            eprintln!(
                "[deferred-write] reconnect failed for {}: {err}",
                file.display()
            );
            None
        }
    }
}

fn exact_editor_reregister_candidate(
    file: &std::path::Path,
    editor_content: &str,
    candidate: Option<String>,
) -> Option<String> {
    candidate.and_then(|content| {
        if content == editor_content {
            return Some(content);
        }
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "deferred_write_editor_reregister_held file={} editor_hash={} candidate_hash={} reason=live_editor_baseline_first visible_editor_mutation=none",
                file.display(),
                agent_doc_hash::content_hash(editor_content),
                agent_doc_hash::content_hash(&content),
            ),
        );
        None
    })
}

/// Settle a pending external-disk candidate after the editor has successfully
/// reset/seeded its CRDT replica from the exact visible target.
///
/// # Safety
/// Both arguments must be valid, NUL-terminated UTF-8 strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_deferred_write_reconnect_propagated(
    file_path: *const c_char,
    editor_content: *const c_char,
) -> i32 {
    mark_embedded_editor_host();
    let Ok(path) = (unsafe { CStr::from_ptr(file_path) }).to_str() else {
        return 0;
    };
    let Ok(content) = (unsafe { CStr::from_ptr(editor_content) }).to_str() else {
        return 0;
    };
    let file = Path::new(path);
    let result = agent_doc_project_root_io::project_root_containing(file)
        .context("no project root for propagated reconnect")
        .and_then(|project_root| {
            deferred_write_reconnect_propagated_from_ffi(&project_root, file, content)
        });
    match result {
        Ok(cleared) => i32::from(cleared),
        Err(err) => {
            eprintln!("[deferred-write] propagated settlement failed for {path}: {err}");
            0
        }
    }
}

/// Record a lazily state-backbone event from a plugin.
///
/// `fact_json` must be a JSON object deserializable as a `StateEvent`
/// (`{ "event_id": "...", "fact": { "type": "baseline_saved", ... } }`). The event
/// is appended to the in-process ledger; the projection's `seen_event_ids` dedupe
/// absorbs replays. Returns `1` on success, `0` on a parse/lock failure (logged to
/// stderr). Plugins trigger transitions by reporting events here rather than running
/// durable FSMs in-process (FFI-first rule).
///
/// # Safety
///
/// `document_hash` and `fact_json` must be valid, NUL-terminated UTF-8 strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_record_state_event(
    document_hash: *const c_char,
    fact_json: *const c_char,
) -> i32 {
    let doc_hash = match unsafe { CStr::from_ptr(document_hash) }.to_str() {
        Ok(s) => s,
        Err(err) => {
            eprintln!(
                "[state-projection] agent_doc_record_state_event: non-UTF-8 document_hash: {err}"
            );
            return 0;
        }
    };
    let fact_str = match unsafe { CStr::from_ptr(fact_json) }.to_str() {
        Ok(s) => s,
        Err(err) => {
            eprintln!(
                "[state-projection] agent_doc_record_state_event: non-UTF-8 fact_json for {doc_hash}: {err}"
            );
            return 0;
        }
    };
    let mut event: StateEvent = match serde_json::from_str(fact_str) {
        Ok(e) => e,
        Err(err) => {
            eprintln!(
                "[state-projection] agent_doc_record_state_event: failed to parse StateEvent JSON for {doc_hash}: {err}"
            );
            return 0;
        }
    };
    let mut ledger = state_ledger().lock();
    // `#xdocsuper3`: the binary owns the hosting-epoch gate (FFI-first).
    // Queue facts reported by a live producer without an explicit
    // `hosting_epoch` are stamped with the document's CURRENT hosting
    // epoch, so a later host/switch makes them stale automatically and
    // a producer never has to track the epoch itself.
    stamp_queue_fact_hosting_epoch(&ledger, &mut event);
    ledger.append(event);
    1
}

/// Subscribe to the agent-doc state projection for a document (`#lazilystatesync2`).
///
/// `last_epoch` is the caller's last-seen lazily epoch for this document. The
/// return value is a NUL-terminated JSON message in the canonical native lazily
/// wire (externally-tagged `IpcMessage`, `#lzsync` 3B clean split):
/// - `{ "Snapshot": { "epoch": .., "nodes": [..], "edges": [..], "roots": [..] } }`
///   when `last_epoch == 0` (cold read) or the document has no state yet — a full
///   graph image the consumer's `GraphView` folds once.
/// - `{ "Delta": { "base_epoch": <last_epoch>, "epoch": <current>, "ops": [..] } }`
///   when `0 < last_epoch < current_epoch` — the ordered ops the view folds
///   verbatim to converge from `last_epoch` to current.
/// - `{ "Delta": { "ops": [], .. } }` when the caller is already current.
///
/// The fold is a pure replay of deduped events, so delta application is
/// deterministic and idempotent: a re-emit/replay yields an empty (no-op) delta.
/// Because the ledger is append-only within a process lifetime, any
/// `last_epoch <= current_epoch` is satisfiable without a resync.
///
/// This complements [`agent_doc_state_projection`], which stays as the full
/// `DocumentStateProjection` JSON for round-trip/human reads (the cold path).
/// Plugins that want reactive updates subscribe here and fold the native message
/// into a generic lazily `GraphView` instead of re-rendering the snapshot.
///
/// Caller must free the returned pointer with `agent_doc_free_string`.
///
/// # Safety
///
/// `document_hash` must be a valid, NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_state_subscribe(
    document_hash: *const c_char,
    last_epoch: u64,
) -> *mut c_char {
    let doc_hash = match unsafe { CStr::from_ptr(document_hash) }.to_str() {
        Ok(s) => s,
        Err(err) => {
            eprintln!(
                "[state-projection] agent_doc_state_subscribe: non-UTF-8 document_hash; returning null: {err}"
            );
            return CString::new("null").unwrap().into_raw();
        }
    };
    let json = {
        let ledger = state_ledger().lock();
        // `#lzsync` 3B wire cutover (flipped): emit the canonical native lazily
        // wire (`IpcMessage` Snapshot/Delta) instead of the bespoke base64
        // `WireSubscribe` JSON. Plugins fold this through a generic lazily
        // `GraphView`. `build_delta` still produces the internal `WireDelta`
        // producer form; the state-wire bridge converts it here.
        let wire = agent_doc_state_wire::subscribe(&ledger, doc_hash, last_epoch);
        match agent_doc_state_wire::lazily_convert::wire_subscribe_to_ipc_message(&wire) {
            Ok(message) => serde_json::to_string(&message).unwrap_or_else(|err| {
                eprintln!(
                    "[state-projection] agent_doc_state_subscribe: serialize native IpcMessage failed: {err}"
                );
                "null".to_string()
            }),
            Err(err) => {
                eprintln!(
                    "[state-projection] agent_doc_state_subscribe: wire→native conversion failed: {err}"
                );
                "null".to_string()
            }
        }
    };
    CString::new(json)
        .unwrap_or_else(|_| CString::new("null").unwrap())
        .into_raw()
}

/// Canonical path-based document id (`document_id_for_path`) — the same id the
/// controller keys sessions/liveness by. The editor plugins use it as the
/// reliable-sync `document_hash` so the pushed liveness lines up with the
/// controller's projection (sidecar-retirement Phase 3C). Returns a heap string
/// to free with [`agent_doc_free_string`], or null on a non-UTF-8 path.
///
/// # Safety
///
/// `file_path` must be a NUL-terminated UTF-8 pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_document_id_for_path(file_path: *const c_char) -> *mut c_char {
    let Ok(path) = (unsafe { CStr::from_ptr(file_path) }).to_str() else {
        eprintln!("[reliable-sync] agent_doc_document_id_for_path: non-UTF-8 path; returning null");
        return std::ptr::null_mut();
    };
    let id = agent_doc_hash::document_id_for_path(Path::new(path));
    CString::new(id)
        .map(|c| c.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

fn enqueue_liveness_ops(
    project_root: &Path,
    document_hash: &str,
    ops: &[agent_doc_reliable_sync_io::liveness::LivenessOp],
) -> anyhow::Result<()> {
    mark_embedded_editor_host();
    let frame = agent_doc_reliable_sync_io::liveness::encode_liveness_frame(ops)?;
    agent_doc_controller_io::project_controller::enqueue_reliable_sync_frame(
        project_root,
        None,
        document_hash,
        frame,
        false,
    )?;
    Ok(())
}

fn flush_liveness_endpoint(project_root: &Path, document_hash: &str) -> anyhow::Result<u64> {
    mark_embedded_editor_host();
    Ok(
        agent_doc_controller_io::project_controller::flush_reliable_sync_channel(
            project_root,
            None,
            document_hash,
        )?
        .ack_through,
    )
}

fn document_op_channel_hash(file_path: &Path) -> String {
    format!(
        "{}:document-op",
        agent_doc_hash::document_id_for_path(file_path)
    )
}

fn enqueue_document_push_frame(
    project_root: &Path,
    file_path: &Path,
    frame: lazily::IpcMessage,
) -> anyhow::Result<()> {
    mark_embedded_editor_host();
    let channel_hash = document_op_channel_hash(file_path);
    agent_doc_controller_io::project_controller::enqueue_reliable_sync_frame(
        project_root,
        Some(file_path),
        &channel_hash,
        frame,
        true,
    )?;
    Ok(())
}

fn flush_document_push_endpoint(project_root: &Path, file_path: &Path) -> anyhow::Result<()> {
    mark_embedded_editor_host();
    let channel_hash = document_op_channel_hash(file_path);
    agent_doc_controller_io::project_controller::flush_reliable_sync_channel(
        project_root,
        Some(file_path),
        &channel_hash,
    )?;
    Ok(())
}

/// Enqueue a JSON batch of `LivenessOp`s through the controller-owned durable
/// push outbox (sidecar-retirement Phase 3C). The reloadable cdylib encodes the
/// frame but never opens SQLite. Returns `0` on success, `-1` on error.
///
/// # Safety
///
/// Non-null pointers must be NUL-terminated UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_reliable_sync_liveness_enqueue(
    project_root: *const c_char,
    document_hash: *const c_char,
    ops_json: *const c_char,
) -> c_int {
    let result = (|| -> anyhow::Result<()> {
        let project_root = unsafe { required_ffi_string(project_root, "project_root") }?;
        let document_hash = unsafe { required_ffi_string(document_hash, "document_hash") }?;
        let ops_json = unsafe { required_ffi_string(ops_json, "ops_json") }?;
        let ops: Vec<agent_doc_reliable_sync_io::liveness::LivenessOp> =
            serde_json::from_str(&ops_json).context("parse liveness ops_json")?;
        let root = PathBuf::from(&project_root);
        enqueue_liveness_ops(&root, &document_hash, &ops)
    })();
    match result {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("[ffi] agent_doc_reliable_sync_liveness_enqueue: {err:#}");
            -1
        }
    }
}

/// `#ctrlkillreregister` Tier 3 editor-side consumer: which documents this editor has
/// registered but holds no replica for.
///
/// The editor is the only process that can create its own replica, so it asks about
/// itself and repairs, instead of the controller pushing a rebuild request at it.
/// A push has to reach the editor — the failure behind `reload-lib reached 1/4
/// endpoints` — while this is driven by a process that is provably alive, because it
/// just called. It is therefore correct whichever side restarted: a controller that
/// lost its process-local hub, an editor that reconnected, or a registration that
/// arrived after any fan-out already ran.
///
/// `held_json` is a JSON array of document hashes the caller already has a replica
/// for; an up-to-date editor gets back `[]` and does nothing. Returns a JSON array of
/// registrations to rebuild, or null on error. Free with `agent_doc_string_free`.
///
/// # Safety
///
/// Non-null pointers must be NUL-terminated UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_peer_replicas_missing(
    project_root: *const c_char,
    pid: u64,
    held_json: *const c_char,
) -> *mut c_char {
    let result = (|| -> anyhow::Result<String> {
        let project_root = unsafe { required_ffi_string(project_root, "project_root") }?;
        let held: Vec<String> = match unsafe { optional_ffi_string(held_json, "held_json") }? {
            Some(raw) if !raw.trim().is_empty() => {
                serde_json::from_str(&raw).context("parse held_json")?
            }
            _ => Vec::new(),
        };
        let missing = agent_doc_controller_io::project_controller::peer_replicas_missing(
            &PathBuf::from(&project_root),
            pid,
            &held,
        )?;
        serde_json::to_string(&missing).context("serialize missing replica registrations")
    })();
    match result {
        Ok(json) => CString::new(json).unwrap_or_default().into_raw(),
        // Never swallow: an editor that cannot ask is one that stays stranded.
        Err(err) => {
            eprintln!("[ffi] agent_doc_peer_replicas_missing: {err:#}");
            ptr::null_mut()
        }
    }
}

/// Ask the controller to flush its durable push outbox through `reliable_sync`
/// (sidecar-retirement Phase 3C). Returns the ack cursor (`>= 0`) on success,
/// `-1` on error.
///
/// # Safety
///
/// Non-null pointers must be NUL-terminated UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_reliable_sync_liveness_flush(
    project_root: *const c_char,
    document_hash: *const c_char,
) -> i64 {
    let result = (|| -> anyhow::Result<u64> {
        let project_root = unsafe { required_ffi_string(project_root, "project_root") }?;
        let document_hash = unsafe { required_ffi_string(document_hash, "document_hash") }?;
        let root = PathBuf::from(&project_root);
        flush_liveness_endpoint(&root, &document_hash)
    })();
    match result {
        Ok(ack) => ack as i64,
        Err(err) => {
            eprintln!("[ffi] agent_doc_reliable_sync_liveness_flush: {err:#}");
            -1
        }
    }
}

/// Durably append and flush one incremental `Vec<TextOp>` produced by
/// `agent_doc_replica_diff`. The file-scoped controller request continuously feeds
/// the relay canonical; retries and controller downtime are absorbed by the
/// controller-owned SQLite outbox. Returns `0` after durable enqueue (including
/// retain-and-stall), `-1` on invalid input/internal error.
///
/// # Safety
///
/// Non-null pointers must be NUL-terminated UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_reliable_sync_document_op_push(
    project_root: *const c_char,
    file_path: *const c_char,
    delta_json: *const c_char,
) -> c_int {
    let result = (|| -> anyhow::Result<()> {
        let project_root = unsafe { required_ffi_string(project_root, "project_root") }?;
        let file_path = unsafe { required_ffi_string(file_path, "file_path") }?;
        let delta_json = unsafe { required_ffi_string(delta_json, "delta_json") }?;
        let Some(frame) =
            agent_doc_reliable_sync_io::document_op::encode_document_op_json_frame(&delta_json)?
        else {
            return flush_document_push_endpoint(Path::new(&project_root), Path::new(&file_path));
        };
        enqueue_document_push_frame(Path::new(&project_root), Path::new(&file_path), frame)
    })();
    match result {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("[ffi] agent_doc_reliable_sync_document_op_push: {error:#}");
            -1
        }
    }
}

/// Compatibility symbol for plugins that still link the retired text-adopt ABI.
///
/// Whole-editor recovery publication is no longer accepted. The controller's
/// retained Lazily projection is the only whole-document authority, so callers
/// receive `-1` and must consume the next controller bootstrap/delivery.
///
/// # Safety
///
/// Non-null pointers must be NUL-terminated UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_reliable_sync_text_adopt_push(
    project_root: *const c_char,
    file_path: *const c_char,
    text: *const c_char,
) -> c_int {
    let _ = (&project_root, &file_path, &text);
    -1
}

/// Retry the retained document-op suffix without enqueueing a new frame.
///
/// # Safety
///
/// Non-null pointers must be NUL-terminated UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_reliable_sync_document_op_flush(
    project_root: *const c_char,
    file_path: *const c_char,
) -> c_int {
    let result = (|| -> anyhow::Result<()> {
        let project_root = unsafe { required_ffi_string(project_root, "project_root") }?;
        let file_path = unsafe { required_ffi_string(file_path, "file_path") }?;
        flush_document_push_endpoint(Path::new(&project_root), Path::new(&file_path))
    })();
    match result {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("[ffi] agent_doc_reliable_sync_document_op_flush: {error:#}");
            -1
        }
    }
}

/// Compatibility-only no-op for plugins that still link the retired full-state-adopt ABI.
///
/// Sending the replica's full operation log here caused an editor/canonical feedback
/// loop and unbounded tombstone growth. The symbol remains exported so an older loaded
/// plugin cannot fail linkage while upgrading, but it refuses the request.
///
/// # Safety
///
/// Non-null pointers must be NUL-terminated UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_reliable_sync_push_full_state_adopt(
    project_root: *const c_char,
    file_path: *const c_char,
    full_state_json: *const c_char,
) -> c_int {
    let _ = (&project_root, &file_path, &full_state_json);
    // Retired after the 2026-07-13 live incident: pushing `encode_state` on every buffer
    // alignment fed canonical output back into the editor and grew tombstones without
    // bound. Keep the symbol as a permanent refusal for rolling plugin/native upgrades.
    -1
}

/// Inspect one actor through the project controller and return the same JSON
/// shape as `agent-doc admin inspect --json`.
///
/// Null or empty `project_root`, `document_path`, `session_id`, and `pane_id`
/// values are treated as absent.
///
/// # Safety
///
/// Non-null string pointers must be NUL-terminated UTF-8. Returned pointers must
/// be freed with [`agent_doc_free_string`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_admin_inspect_json(
    project_root: *const c_char,
    document_path: *const c_char,
    session_id: *const c_char,
    pane_id: *const c_char,
) -> FfiJsonResult {
    ffi_json_from_result((|| -> anyhow::Result<_> {
        let project_root = unsafe { optional_ffi_string(project_root, "project_root") }?;
        let document_path =
            optional_path(unsafe { optional_ffi_string(document_path, "document_path") }?);
        let session_id = unsafe { optional_ffi_string(session_id, "session_id") }?;
        let pane_id = unsafe { optional_ffi_string(pane_id, "pane_id") }?;
        let root = resolve_admin_root(project_root.as_deref(), document_path.as_deref())?;
        agent_doc_controller_io::project_controller::inspect_actor(
            &root,
            document_path.as_deref(),
            session_id.as_deref(),
            pane_id.as_deref(),
        )
    })())
}

/// Return the Project Controller-owned tmux focus projection for a project.
///
/// The response reports an active document only when the configured tmux
/// session's current window is `agent-doc`; other tmux windows intentionally
/// return an inactive projection so editor selection is not recalled from a
/// stale agent-doc pane.
///
/// # Safety
///
/// Non-null string pointers must be NUL-terminated UTF-8. Returned pointers must
/// be freed with [`agent_doc_free_string`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_tmux_focus_state_json(
    project_root: *const c_char,
) -> FfiJsonResult {
    ffi_json_from_result((|| -> anyhow::Result<_> {
        let project_root = unsafe { optional_ffi_string(project_root, "project_root") }?;
        let root = resolve_admin_root(project_root.as_deref(), None)?;
        agent_doc_controller_io::project_controller::tmux_focus_state(&root)
    })())
}

/// Report what an editor looks like right now to the controller-owned graph
/// (`#jbpluginlazilyeffects`).
///
/// `surface_json` is an `EditorSurface`: `{ "focused": "<path>", "visible":
/// ["<path>", ...], "columns": [{ "files": ["<path>", ...] }, ...],
/// "force_reconcile": false }`.
///
/// Note there is no `layout_synced`: the Project Controller observes tmux
/// locally, so a plugin never reports a fact it would have to ask the
/// controller for.
///
/// This is the entry point a plugin should call instead of choosing between
/// [`agent_doc_focus_document_pane_json`] and [`agent_doc_sync_tmux_layout_json`]
/// itself. Native transport adds `(client_id, generation, sequence)` and sends
/// the observation only to an existing controller. The controller derives
/// intent from it and the previous observation, publishes the accepted
/// projection, and owns the consequence `Effect`. An identical observation
/// costs nothing, so plugins need no dedup of their own.
///
/// Returns a receipt: `{ "intent": {...}, "idle": bool, "outcome": string|null,
/// "error": string|null }`.
///
/// # Safety
///
/// Non-null string pointers must be NUL-terminated UTF-8. Returned pointers must
/// be freed with [`agent_doc_free_string`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_editor_surface_observe_json(
    project_root: *const c_char,
    surface_json: *const c_char,
) -> FfiJsonResult {
    ffi_json_from_result((|| -> anyhow::Result<_> {
        let project_root =
            PathBuf::from(unsafe { required_ffi_string(project_root, "project_root") }?);
        let surface_json = unsafe { required_ffi_string(surface_json, "surface_json") }?;
        agent_doc_editor_surface_io::observe_from_json(&project_root, &surface_json)
    })())
}

/// Validate and enqueue one editor-surface observation without waiting for its
/// controller probe or tmux consequence.
///
/// Returns `1` when queued and `-1` for an invalid argument or payload.
///
/// # Safety
///
/// Non-null string pointers must be NUL-terminated UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_editor_surface_enqueue_json(
    project_root: *const c_char,
    surface_json: *const c_char,
) -> c_int {
    let Ok(project_root) = (unsafe { required_ffi_string(project_root, "project_root") }) else {
        return -1;
    };
    let Ok(surface_json) = (unsafe { required_ffi_string(surface_json, "surface_json") }) else {
        return -1;
    };
    match agent_doc_editor_surface_io::enqueue_from_json(Path::new(&project_root), &surface_json) {
        Ok(()) => 1,
        Err(error) => {
            eprintln!("[editor-surface] enqueue rejected: {error:#}");
            -1
        }
    }
}

/// Read the latest controller-published authority for one open document.
///
/// This is a cache read over the native Lazily graph. The editor surface owns
/// subscription membership; no controller request, SQLite read, or filesystem
/// probe occurs here.
///
/// # Safety
///
/// `project_root` and `document_path` must each be valid, NUL-terminated UTF-8
/// strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_document_authority_json(
    project_root: *const c_char,
    document_path: *const c_char,
) -> FfiJsonResult {
    ffi_json_from_result((|| -> anyhow::Result<_> {
        let project_root =
            PathBuf::from(unsafe { required_ffi_string(project_root, "project_root") }?);
        let document_path = unsafe { required_ffi_string(document_path, "document_path") }?;
        Ok(agent_doc_editor_surface_io::document_authority(
            &project_root,
            &document_path,
        ))
    })())
}

/// Read selected document × controller authority from the native Computed.
///
/// # Safety
///
/// `project_root` must be a valid, NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_current_document_authority_json(
    project_root: *const c_char,
) -> FfiJsonResult {
    ffi_json_from_result((|| -> anyhow::Result<_> {
        let project_root =
            PathBuf::from(unsafe { required_ffi_string(project_root, "project_root") }?);
        Ok(agent_doc_editor_surface_io::current_document_authority(
            &project_root,
        ))
    })())
}

/// Report the column layout tmux is currently showing at `project_root`.
///
/// The controller's half of the mirror. `layout_json` is a `TmuxLayout`:
/// `{ "columns": [{ "files": ["<path>", ...] }, ...] }`; pass a null pointer to
/// record that no tmux layout is known.
///
/// Drift observed here drives a reconcile on its own — no editor event is
/// required, and nothing about it is reported by a plugin.
///
/// # Safety
///
/// Non-null string pointers must be NUL-terminated UTF-8. Returned pointers must
/// be freed with [`agent_doc_free_string`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_editor_surface_observe_tmux_json(
    project_root: *const c_char,
    layout_json: *const c_char,
) -> FfiJsonResult {
    ffi_json_from_result((|| -> anyhow::Result<_> {
        let project_root =
            PathBuf::from(unsafe { required_ffi_string(project_root, "project_root") }?);
        let layout_json = unsafe { optional_ffi_string(layout_json, "layout_json") }?;
        let layout = match layout_json {
            Some(json) => Some(
                serde_json::from_str::<agent_doc_editor_surface::TmuxLayout>(&json)
                    .context("parse tmux layout json")?,
            ),
            None => None,
        };
        Ok(agent_doc_editor_surface_io::observe_tmux(
            &project_root,
            layout,
        ))
    })())
}

/// Forget an editor surface — the editor closed the project.
///
/// Discards that root's reconciled-layout history and unsubscribes its
/// consequence. Returns `1` when a surface was forgotten, `0` when none was
/// registered, and `-1` on a bad argument.
///
/// # Safety
///
/// Non-null string pointers must be NUL-terminated UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_editor_surface_forget(project_root: *const c_char) -> c_int {
    let Ok(project_root) = (unsafe { required_ffi_string(project_root, "project_root") }) else {
        return -1;
    };
    c_int::from(agent_doc_editor_surface_io::forget(Path::new(
        &project_root,
    )))
}

/// Focus the actor pane for a document through the Project Controller.
///
/// This centralizes pane selection behind the controller so JetBrains does not
/// run `agent-doc focus` or raw tmux commands for editor focus handoff.
///
/// # Safety
///
/// Non-null string pointers must be NUL-terminated UTF-8. Returned pointers must
/// be freed with [`agent_doc_free_string`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_focus_document_pane_json(
    project_root: *const c_char,
    document_path: *const c_char,
) -> FfiJsonResult {
    ffi_json_from_result((|| -> anyhow::Result<_> {
        let project_root = unsafe { optional_ffi_string(project_root, "project_root") }?;
        let document_path =
            PathBuf::from(unsafe { required_ffi_string(document_path, "document_path") }?);
        let root = resolve_admin_root(project_root.as_deref(), Some(&document_path))?;
        agent_doc_controller_io::project_controller::focus_document_pane(&root, &document_path)
    })())
}

/// Sync the tmux layout through the Project Controller.
///
/// `columns_json` is a JSON array of column strings using the same comma-joined
/// column representation as `agent-doc sync --col`.
///
/// # Safety
///
/// Non-null string pointers must be NUL-terminated UTF-8. Returned pointers must
/// be freed with [`agent_doc_free_string`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_sync_tmux_layout_json(
    project_root: *const c_char,
    columns_json: *const c_char,
    window: *const c_char,
    focus: *const c_char,
    no_autostart: c_int,
    exact_visible: c_int,
) -> FfiJsonResult {
    ffi_json_from_result((|| -> anyhow::Result<_> {
        let project_root = unsafe { optional_ffi_string(project_root, "project_root") }?;
        let columns_json = unsafe { required_ffi_string(columns_json, "columns_json") }?;
        let window = unsafe { optional_ffi_string(window, "window") }?;
        let focus = unsafe { optional_ffi_string(focus, "focus") }?;
        let columns: Vec<String> =
            serde_json::from_str(&columns_json).context("parse columns_json")?;
        let focus_path = focus.as_deref().map(Path::new);
        let root = resolve_admin_root(project_root.as_deref(), focus_path)?;
        agent_doc_controller_io::project_controller::sync_tmux_layout(
            &root,
            agent_doc_controller_io::project_controller::ControllerTmuxLayoutSyncInvocation {
                columns,
                window,
                focus,
                no_autostart: no_autostart != 0,
                exact_visible: exact_visible != 0,
                caller_kind: if no_autostart != 0 {
                    "automatic".to_string()
                } else {
                    "manual".to_string()
                },
                actor_bindings: Vec::new(),
            },
        )
    })())
}

/// Pause, resume, or drain queue work through the project controller and return
/// the same receipt JSON shape as `agent-doc admin queue ... --json`.
///
/// Pass `observed_generation = -1` when no generation guard is supplied.
///
/// # Safety
///
/// Non-null string pointers must be NUL-terminated UTF-8. Returned pointers must
/// be freed with [`agent_doc_free_string`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_admin_queue_control_json(
    project_root: *const c_char,
    document_path: *const c_char,
    action: *const c_char,
    observed_generation: i64,
    reason: *const c_char,
    item_id: *const c_char,
) -> FfiJsonResult {
    ffi_json_from_result((|| -> anyhow::Result<_> {
        let project_root = unsafe { optional_ffi_string(project_root, "project_root") }?;
        let document_path =
            optional_path(unsafe { optional_ffi_string(document_path, "document_path") }?);
        let action = unsafe { required_ffi_string(action, "action") }?;
        let observed_generation = optional_generation(observed_generation, "observed_generation")?;
        let reason = unsafe { optional_ffi_string(reason, "reason") }?;
        let item_id = unsafe { optional_ffi_string(item_id, "item_id") }?;
        let root = resolve_admin_root(project_root.as_deref(), document_path.as_deref())?;
        agent_doc_controller_io::project_controller::control_queue(
            &root,
            document_path.as_deref(),
            &action,
            observed_generation,
            reason.as_deref(),
            item_id.as_deref(),
        )
    })())
}

/// Reap a stale actor through the project controller and return the same
/// receipt JSON shape as `agent-doc admin reap --json`.
///
/// # Safety
///
/// Non-null string pointers must be NUL-terminated UTF-8. Returned pointers must
/// be freed with [`agent_doc_free_string`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_admin_reap_json(
    project_root: *const c_char,
    document_path: *const c_char,
    session_id: *const c_char,
    pane_id: *const c_char,
    observed_generation: i64,
    reason: *const c_char,
) -> FfiJsonResult {
    ffi_json_from_result((|| -> anyhow::Result<_> {
        let project_root = unsafe { optional_ffi_string(project_root, "project_root") }?;
        let document_path =
            optional_path(unsafe { optional_ffi_string(document_path, "document_path") }?);
        let session_id = unsafe { optional_ffi_string(session_id, "session_id") }?;
        let pane_id = unsafe { optional_ffi_string(pane_id, "pane_id") }?;
        let observed_generation = required_generation(observed_generation, "observed_generation")?;
        let reason = unsafe { required_ffi_string(reason, "reason") }?;
        let root = resolve_admin_root(project_root.as_deref(), document_path.as_deref())?;
        agent_doc_controller_io::project_controller::admin_reap(
            &root,
            document_path.as_deref(),
            session_id.as_deref(),
            pane_id.as_deref(),
            observed_generation,
            &reason,
        )
    })())
}

/// Handoff a document actor through the project controller and return the same
/// receipt JSON shape as `agent-doc admin handoff --json`.
///
/// # Safety
///
/// Non-null string pointers must be NUL-terminated UTF-8. Returned pointers must
/// be freed with [`agent_doc_free_string`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_admin_handoff_json(
    project_root: *const c_char,
    document_path: *const c_char,
    to_pane: *const c_char,
    observed_generation: i64,
    reason: *const c_char,
) -> FfiJsonResult {
    ffi_json_from_result((|| -> anyhow::Result<_> {
        let project_root = unsafe { optional_ffi_string(project_root, "project_root") }?;
        let document_path =
            PathBuf::from(unsafe { required_ffi_string(document_path, "document_path") }?);
        let to_pane = unsafe { required_ffi_string(to_pane, "to_pane") }?;
        let observed_generation = required_generation(observed_generation, "observed_generation")?;
        let reason = unsafe { required_ffi_string(reason, "reason") }?;
        let root = resolve_admin_root(project_root.as_deref(), Some(document_path.as_path()))?;
        agent_doc_controller_io::project_controller::admin_handoff(
            &root,
            &document_path,
            &to_pane,
            observed_generation,
            &reason,
        )
    })())
}

/// Repair controller compatibility projections and return the same receipt JSON
/// shape as `agent-doc admin repair-projection --json`.
///
/// Pass `observed_generation = -1` when no generation guard is supplied.
///
/// # Safety
///
/// Non-null string pointers must be NUL-terminated UTF-8. Returned pointers must
/// be freed with [`agent_doc_free_string`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_admin_repair_projection_json(
    project_root: *const c_char,
    document_path: *const c_char,
    projection: *const c_char,
    observed_generation: i64,
    reason: *const c_char,
) -> FfiJsonResult {
    ffi_json_from_result((|| -> anyhow::Result<_> {
        let project_root = unsafe { optional_ffi_string(project_root, "project_root") }?;
        let document_path =
            optional_path(unsafe { optional_ffi_string(document_path, "document_path") }?);
        let projection = unsafe { required_ffi_string(projection, "projection") }?;
        let observed_generation = optional_generation(observed_generation, "observed_generation")?;
        let reason = unsafe { optional_ffi_string(reason, "reason") }?;
        let root = resolve_admin_root(project_root.as_deref(), document_path.as_deref())?;
        agent_doc_controller_io::project_controller::repair_projection(
            &root,
            document_path.as_deref(),
            &projection,
            observed_generation,
            reason.as_deref(),
        )
    })())
}

/// Walk up from `path` to find the nearest ancestor containing `.agent-doc/`.
/// Delegates to [`agent_doc_fs::find_project_root`].
fn find_project_root_ffi(path: &std::path::Path) -> Option<std::path::PathBuf> {
    agent_doc_fs::find_project_root(path)
}

/// Resolve a file's agent-doc project root and the path relative to that root.
///
/// Walks up from the file's parent directory looking for the nearest ancestor
/// containing a `.agent-doc/` directory. Editor plugins use this to detect when
/// a file inside a submodule belongs to its own agent-doc project (with its own
/// sessions, snapshots, etc.) rather than the enclosing workspace.
///
/// Returns `FfiProjectPath` with:
/// - `project_root`: absolute path of the nearest `.agent-doc/`-containing
///   ancestor, or null if none exists.
/// - `relative_path`: `file_path` made relative to `project_root`, or null when
///   `project_root` is null or the relative computation fails.
///
/// Both strings (when non-null) must be freed with [`agent_doc_free_string`].
///
/// # Safety
///
/// `file_path` must be a valid, NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_resolve_project_path(
    file_path: *const c_char,
) -> FfiProjectPath {
    let null_result = FfiProjectPath {
        project_root: ptr::null_mut(),
        relative_path: ptr::null_mut(),
    };

    let path_str = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(s) => s,
        Err(_) => return null_result,
    };
    let path = std::path::Path::new(path_str);

    // Canonicalize when possible so ancestor walks resolve symlinks (submodule
    // worktrees often live inside symlinked trees). Fall back to the raw path
    // when canonicalization fails (e.g., file does not yet exist) — the walk
    // still works on lexical parents.
    let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

    let project_root = match find_project_root_ffi(&resolved) {
        Some(root) => root,
        None => return null_result,
    };

    let rel = match resolved.strip_prefix(&project_root) {
        Ok(p) => p.to_path_buf(),
        Err(_) => return null_result,
    };

    let root_c = match CString::new(project_root.to_string_lossy().as_ref()) {
        Ok(s) => s,
        Err(_) => return null_result,
    };
    let rel_c = match CString::new(rel.to_string_lossy().as_ref()) {
        Ok(s) => s,
        Err(_) => return null_result,
    };

    FfiProjectPath {
        project_root: root_c.into_raw(),
        relative_path: rel_c.into_raw(),
    }
}

/// Free a string returned by any `agent_doc_*` function.
///
/// # Safety
///
// `agent_doc_free_string` and `agent_doc_free_state` moved to
// `agent_doc_ffi` (Wave 5 / `#k9e1` proof-of-concept). They are
// re-exported below via `pub use agent_doc_ffi::*;`. The
// `force_link_core_ffi_symbols` function below references them to
// prevent the static linker from stripping them out of the main cdylib.
pub use agent_doc_ffi::*;

/// Prevent the static linker from stripping the `agent_doc_ffi`
/// symbols out of `libagent_doc.{so,dylib,dll}`. Called from `lib.rs`
/// via a constructor-style reference path so editor plugins
/// (JetBrains, VS Code) continue to find the symbols at runtime.
///
/// The function itself is never called — we just need the symbol
/// references to exist in the main crate's compilation unit so the
/// linker keeps them in the cdylib export table.
#[allow(dead_code)]
fn force_link_core_ffi_symbols() {
    use agent_doc_ffi::{
        FfiComponentList, FfiMergeResult, FfiPatchResult, agent_doc_apply_node_patches,
        agent_doc_apply_patch, agent_doc_apply_patch_with_boundary,
        agent_doc_apply_patch_with_caret, agent_doc_converge_queue_auto, agent_doc_crdt_merge,
        agent_doc_free_state, agent_doc_free_string, agent_doc_lossless_tree_capability,
        agent_doc_lossless_tree_project, agent_doc_lossless_tree_projection_current,
        agent_doc_lossless_tree_render, agent_doc_merge_crdt, agent_doc_merge_frontmatter,
        agent_doc_normalize_template_structure, agent_doc_parse_components,
        agent_doc_reposition_boundary_to_end, agent_doc_reposition_boundary_to_end_preserve_head,
        agent_doc_reposition_boundary_to_end_preserve_head_with_id,
        agent_doc_reposition_boundary_to_end_with_id, agent_doc_visual_tokens_json,
    };
    let _: unsafe extern "C" fn(
        *const c_char,
        *const c_char,
        *const c_char,
        *const c_char,
    ) -> FfiPatchResult = agent_doc_apply_patch;
    let _: unsafe extern "C" fn(
        *const c_char,
        *const c_char,
        *const c_char,
        *const c_char,
        i32,
    ) -> FfiPatchResult = agent_doc_apply_patch_with_caret;
    let _: unsafe extern "C" fn(
        *const c_char,
        *const c_char,
        *const c_char,
        *const c_char,
        *const c_char,
    ) -> FfiPatchResult = agent_doc_apply_patch_with_boundary;
    let _: unsafe extern "C" fn(*mut c_char) = agent_doc_free_string;
    let _: unsafe extern "C" fn(*mut u8, usize) = agent_doc_free_state;
    let _: unsafe extern "C" fn(*const c_char, *const c_char) -> FfiPatchResult =
        agent_doc_apply_node_patches;
    let _: extern "C" fn() -> *const c_char = agent_doc_lossless_tree_capability;
    let _: unsafe extern "C" fn(*const c_char) -> *mut c_char = agent_doc_lossless_tree_project;
    let _: unsafe extern "C" fn(*const c_char) -> *mut c_char = agent_doc_lossless_tree_render;
    let _: unsafe extern "C" fn(*const c_char, *const c_char) -> i32 =
        agent_doc_lossless_tree_projection_current;
    let _: unsafe extern "C" fn(*const c_char) -> FfiComponentList = agent_doc_parse_components;
    let _: unsafe extern "C" fn(*const c_char) -> *mut c_char = agent_doc_visual_tokens_json;
    let _: unsafe extern "C" fn(*const c_char, *const c_char) -> FfiPatchResult =
        agent_doc_merge_frontmatter;
    let _: unsafe extern "C" fn(*const c_char, std::os::raw::c_int) -> FfiPatchResult =
        agent_doc_converge_queue_auto;
    let _: unsafe extern "C" fn(*const c_char) -> FfiPatchResult =
        agent_doc_normalize_template_structure;
    let _: unsafe extern "C" fn(*const u8, usize, *const c_char, *const c_char) -> FfiMergeResult =
        agent_doc_crdt_merge;
    let _: unsafe extern "C" fn(*const c_char, *const c_char, *const c_char) -> *mut c_char =
        agent_doc_merge_crdt;
    // Editor-as-replica FFI (#crdtauth2).
    let _: unsafe extern "C" fn(u64, *const u8, usize) -> i32 = agent_doc_replica_open;
    let _: unsafe extern "C" fn(u64, u32, u32, *const c_char) -> i32 =
        agent_doc_replica_apply_local;
    let _: unsafe extern "C" fn(u64) -> *mut c_char = agent_doc_replica_text;
    let _: unsafe extern "C" fn(u64, *mut usize) -> *mut u8 = agent_doc_replica_state_vector;
    let _: unsafe extern "C" fn(u64, *const u8, usize, *mut usize) -> *mut u8 =
        agent_doc_replica_diff;
    let _: unsafe extern "C" fn(u64, *const u8, usize) -> i32 = agent_doc_replica_apply_update;
    let _: unsafe extern "C" fn(u64, *mut usize) -> *mut u8 = agent_doc_replica_encode_state;
    let _: unsafe extern "C" fn(u64) -> i32 = agent_doc_replica_close;
    let _: unsafe extern "C" fn(*const c_char, *const c_char, *const c_char) -> c_int =
        agent_doc_reliable_sync_document_op_push;
    let _: unsafe extern "C" fn(*const c_char, *const c_char, *const c_char) -> c_int =
        agent_doc_reliable_sync_text_adopt_push;
    let _: unsafe extern "C" fn(*const c_char, *const c_char) -> c_int =
        agent_doc_reliable_sync_document_op_flush;
    let _: unsafe extern "C" fn(*const c_char) -> FfiPatchResult =
        agent_doc_reposition_boundary_to_end;
    let _: unsafe extern "C" fn(*const c_char, *const c_char) -> FfiPatchResult =
        agent_doc_reposition_boundary_to_end_with_id;
    let _: unsafe extern "C" fn(*const c_char) -> FfiPatchResult =
        agent_doc_reposition_boundary_to_end_preserve_head;
    let _: unsafe extern "C" fn(*const c_char, *const c_char) -> FfiPatchResult =
        agent_doc_reposition_boundary_to_end_preserve_head_with_id;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_surface_event_ffi_writes_one_shared_cross_editor_schema() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let doc = tmp.path().join("tasks/session.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "body\n").unwrap();

        let root = CString::new(tmp.path().to_string_lossy().as_bytes()).unwrap();
        let source = CString::new("vscode").unwrap();
        let file = CString::new(doc.to_string_lossy().as_bytes()).unwrap();
        let surface = CString::new("vcs_refresh_save").unwrap();
        let action = CString::new("save_document").unwrap();
        let command = CString::new("save_document").unwrap();
        let patch = CString::new("patch-1").unwrap();
        let status = CString::new("saved").unwrap();

        let recorded = unsafe {
            agent_doc_record_editor_surface_event(
                root.as_ptr(),
                source.as_ptr(),
                file.as_ptr(),
                surface.as_ptr(),
                action.as_ptr(),
                command.as_ptr(),
                patch.as_ptr(),
                status.as_ptr(),
            )
        };
        assert_eq!(recorded, 1);

        let ops = std::fs::read_to_string(tmp.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops.contains("editor_surface_event source=vscode"));
        assert!(ops.contains("surface=vcs_refresh_save action=save_document"));
        assert!(ops.contains("agent_command=save_document status=saved"));
        assert!(ops.contains("file=tasks/session.md patch_id=patch-1 doc=session #cyh0"));
    }

    #[test]
    fn editor_reregister_never_returns_a_divergent_whole_document_candidate() {
        let file = std::path::Path::new("session.md");
        let live = "<!-- agent:exchange -->\n❯ live prompt\n<!-- /agent:exchange -->\n";
        let stale = "<!-- agent:exchange -->\n<!-- /agent:exchange -->\n";

        assert_eq!(
            exact_editor_reregister_candidate(file, live, Some(live.to_string())).as_deref(),
            Some(live),
        );
        assert_eq!(
            exact_editor_reregister_candidate(file, live, Some(stale.to_string())),
            None,
        );
    }

    #[test]
    fn post_register_reconnect_returns_semantic_replay_over_exact_editor_cut() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("session.md");
        let baseline = concat!(
            "---\nagent_doc_format: template\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: base — gpt-5\n\nBase response.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#deleted-unsaved]\n",
            "- do [#kept]\n",
            "<!-- /agent:queue -->\n",
        );
        let editor_cut = baseline.replace("- do [#deleted-unsaved]\n", "");
        let target = baseline.replace(
            "<!-- /agent:exchange -->",
            "### Re: retained — gpt-5\n\nRetained response.\n<!-- /agent:exchange -->",
        );
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::write(&file, baseline).unwrap();
        agent_doc_document_realtime_io::retain_deferred_document_write_target(
            &file,
            baseline,
            &target,
            "ffi_post_register_test",
            agent_doc_state_backbone::DocumentWriteDeferredReason::EditorOwnerWithoutRegisteredReplica,
        )
        .unwrap();

        let path = CString::new(file.to_string_lossy().as_bytes()).unwrap();
        let editor = CString::new(editor_cut.as_bytes()).unwrap();
        let ptr = unsafe {
            agent_doc_deferred_write_post_register_content(path.as_ptr(), editor.as_ptr())
        };
        assert!(!ptr.is_null());
        let recovered = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_string();
        unsafe { agent_doc_free_string(ptr) };

        assert!(recovered.contains("Retained response."));
        assert!(recovered.contains("- do [#kept]"));
        assert!(!recovered.contains("- do [#deleted-unsaved]"));
        assert_ne!(recovered, editor_cut);
        assert_eq!(
            exact_editor_reregister_candidate(&file, &editor_cut, Some(recovered)),
            None,
            "the legacy reconnect entrypoint must remain fail-closed",
        );

        let projected = unsafe {
            agent_doc_deferred_write_post_register_project(path.as_ptr(), editor.as_ptr())
        };
        assert_eq!(
            projected, 0,
            "a model-less endpoint must not admit an editor mutation",
        );
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            baseline,
            "post-registration FFI is transport-only and must not write disk",
        );
    }

    #[test]
    fn turn_projection_uses_committed_state_backbone_phase() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let doc = tmp.path().join("session.md");
        let content = "---\nagent_doc_session: session-1\n---\n\nbody\n";
        std::fs::write(&doc, content).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();
        agent_doc_cycle_state_io::mark_committed(
            &doc,
            "commit_success",
            Some(content),
            Some(content),
        )
        .unwrap();
        assert_eq!(
            agent_doc_cycle_state_io::load(&doc).unwrap().unwrap().phase,
            agent_doc_turn::CyclePhase::Committed
        );

        let doc_c = CString::new(doc.to_string_lossy().as_ref()).unwrap();
        let projection_ptr = unsafe { agent_doc_turn_projection(doc_c.as_ptr()) };
        let projection = unsafe { CStr::from_ptr(projection_ptr) }
            .to_str()
            .unwrap()
            .to_string();
        drop(unsafe { CString::from_raw(projection_ptr) });
        let projection: serde_json::Value = serde_json::from_str(&projection).unwrap();

        assert_eq!(projection["turn_in_flight"], false);
        assert_eq!(projection["state"], "idle");
    }

    #[test]
    fn state_projection_ffi_round_trip() {
        use agent_doc_state_backbone::{StateEvent, StateFact};

        let doc_hash = "state_projection_ffi_round_trip_doc";
        let event = StateEvent::new(
            "evt-ffi-roundtrip-1",
            StateFact::BaselineSaved {
                document_hash: doc_hash.to_string(),
                cycle_id: "cycle-1".to_string(),
                baseline_hash: "baseline-1".to_string(),
                baseline_path: None,
            },
        );
        let fact_json = CString::new(serde_json::to_string(&event).unwrap()).unwrap();
        let doc_hash_c = CString::new(doc_hash).unwrap();

        let rc = unsafe { agent_doc_record_state_event(doc_hash_c.as_ptr(), fact_json.as_ptr()) };
        assert_eq!(
            1, rc,
            "record_state_event should succeed on valid StateEvent JSON"
        );

        let proj_ptr = unsafe { agent_doc_state_projection(doc_hash_c.as_ptr()) };
        let proj = unsafe { CStr::from_ptr(proj_ptr) }
            .to_str()
            .unwrap()
            .to_string();
        drop(unsafe { CString::from_raw(proj_ptr) });

        assert_ne!(
            "null", proj,
            "projection should exist after recording an event"
        );
        assert!(
            proj.contains(doc_hash),
            "projection should reference the document hash: {proj}"
        );

        // Replaying the same event_id still appends (returns 1); the projection's
        // seen_event_ids dedupe absorbs the duplicate on the next project() call.
        let replay_rc =
            unsafe { agent_doc_record_state_event(doc_hash_c.as_ptr(), fact_json.as_ptr()) };
        assert_eq!(1, replay_rc, "replay record should still return success");

        // Unknown document_hash yields "null".
        let missing_c = CString::new("no-such-doc-hash").unwrap();
        let missing_ptr = unsafe { agent_doc_state_projection(missing_c.as_ptr()) };
        let missing = unsafe { CStr::from_ptr(missing_ptr) }
            .to_str()
            .unwrap()
            .to_string();
        drop(unsafe { CString::from_raw(missing_ptr) });
        assert_eq!(
            "null", missing,
            "unknown document_hash should project to null"
        );
    }

    #[test]
    fn state_projection_ffi_accepts_editor_patch_applied_event() {
        let doc_hash = "state_projection_ffi_applied_doc";
        let event_json = CString::new(format!(
            r#"{{
                "event_id":"{doc_hash}:applied",
                "fact":{{
                    "type":"editor_patch_applied",
                    "document_hash":"{doc_hash}",
                    "patch_id":"patch-applied",
                    "actor_generation":1
                }}
            }}"#
        ))
        .unwrap();
        let doc_hash_c = CString::new(doc_hash).unwrap();

        let rc = unsafe { agent_doc_record_state_event(doc_hash_c.as_ptr(), event_json.as_ptr()) };
        assert_eq!(1, rc, "applied lazily receipt events should be accepted");
    }

    #[test]
    fn state_projection_ffi_rejects_unknown_legacy_receipt_event() {
        let doc_hash = "state_projection_ffi_legacy_receipt_doc";
        let event_json = CString::new(format!(
            r#"{{
                "event_id":"{doc_hash}:legacy-receipt",
                "fact":{{
                    "type":"legacy_editor_receipt_observed",
                    "document_hash":"{doc_hash}",
                    "patch_id":"patch-legacy",
                    "actor_generation":1
                }}
            }}"#
        ))
        .unwrap();
        let doc_hash_c = CString::new(doc_hash).unwrap();

        let rc = unsafe { agent_doc_record_state_event(doc_hash_c.as_ptr(), event_json.as_ptr()) };
        assert_eq!(0, rc, "unknown legacy receipt events must be rejected");
    }

    #[test]
    fn editor_patch_applied_ffi_records_lazily_receipt() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let file = tmp.path().join("doc.md");
        std::fs::write(&file, "hello\n").unwrap();
        let canonical = file.canonicalize().unwrap();
        let doc_hash = agent_doc_hash::document_id_for_path(&canonical);
        let file_c = CString::new(canonical.to_string_lossy().as_ref()).unwrap();
        let patch_c = CString::new("patch-applied-ffi").unwrap();

        let rc = unsafe { agent_doc_editor_patch_applied(file_c.as_ptr(), patch_c.as_ptr(), 7) };
        assert_eq!(1, rc, "applied receipt should be recorded");

        let doc_hash_c = CString::new(doc_hash.clone()).unwrap();
        let proj_ptr = unsafe { agent_doc_state_projection(doc_hash_c.as_ptr()) };
        let proj = unsafe { CStr::from_ptr(proj_ptr) }
            .to_str()
            .unwrap()
            .to_string();
        drop(unsafe { CString::from_raw(proj_ptr) });
        assert!(
            proj.contains(r#""phase":"applied""#),
            "projection should contain applied transport receipt: {proj}"
        );

        let durable =
            agent_doc_controller_io::project_controller::load_state_backbone_projection(tmp.path())
                .unwrap();
        let document = durable
            .document(&doc_hash)
            .expect("durable receipt should project for the document");
        assert_eq!(
            document
                .transport
                .patches
                .get("patch-applied-ffi")
                .map(|patch| patch.phase),
            Some(agent_doc_state_backbone::TransportPatchPhase::Applied)
        );
    }

    #[test]
    fn editor_patch_rejected_ffi_records_durable_lazily_receipt() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let file = tmp.path().join("doc.md");
        std::fs::write(&file, "hello\n").unwrap();
        let canonical = file.canonicalize().unwrap();
        let doc_hash = agent_doc_hash::document_id_for_path(&canonical);
        let file_c = CString::new(canonical.to_string_lossy().as_ref()).unwrap();
        let patch_c = CString::new("patch-rejected-ffi").unwrap();
        let reason_c = CString::new("file_apply_failed").unwrap();

        let rc = unsafe {
            agent_doc_editor_patch_rejected(file_c.as_ptr(), patch_c.as_ptr(), 8, reason_c.as_ptr())
        };
        assert_eq!(1, rc, "rejected receipt should be recorded durably");

        let durable =
            agent_doc_controller_io::project_controller::load_state_backbone_projection(tmp.path())
                .unwrap();
        let document = durable
            .document(&doc_hash)
            .expect("durable rejection should project for the document");
        assert_eq!(
            document
                .transport
                .patches
                .get("patch-rejected-ffi")
                .map(|patch| patch.phase),
            Some(agent_doc_state_backbone::TransportPatchPhase::Rejected)
        );
        assert_eq!(
            document.transport.last_rejected_reason.as_deref(),
            Some("file_apply_failed")
        );
    }

    #[test]
    fn turn_projection_ffi_defaults_to_idle_for_unknown_document() {
        // No cycle state for this path → idle projection, valid JSON, not in flight.
        let path = CString::new("/nonexistent/agent-doc/turnproj.md").unwrap();
        let ptr = unsafe { agent_doc_turn_projection(path.as_ptr()) };
        let json = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_string();
        drop(unsafe { CString::from_raw(ptr) });

        let value: serde_json::Value = serde_json::from_str(&json).expect("valid projection JSON");
        assert_eq!(value["state"], "idle");
        assert_eq!(value["turn_in_flight"], false);
        assert_eq!(value["transition_authority"], "project_controller");
    }

    #[test]
    fn record_state_event_stamps_and_gates_queue_hosting_epoch() {
        use agent_doc_state_backbone::{StateEvent, StateFact};

        let doc_hash = "ffi_hosting_epoch_gate_doc";
        let doc_hash_c = CString::new(doc_hash).unwrap();

        let record = |event: &StateEvent| {
            let json = CString::new(serde_json::to_string(event).unwrap()).unwrap();
            let rc = unsafe { agent_doc_record_state_event(doc_hash_c.as_ptr(), json.as_ptr()) };
            assert_eq!(1, rc, "record_state_event should succeed");
        };
        let projection = || {
            let ptr = unsafe { agent_doc_state_projection(doc_hash_c.as_ptr()) };
            let json = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_string();
            drop(unsafe { CString::from_raw(ptr) });
            json
        };

        // Supervisor begins hosting on a pane (hosting epoch 1).
        record(&StateEvent::new(
            "ffi-host-1",
            StateFact::SupervisorHosting {
                document_hash: doc_hash.to_string(),
                pane_session: "%9:ffi-session".to_string(),
                lease_epoch: 1,
            },
        ));
        // A live producer reports a completed head WITHOUT a hosting_epoch; the
        // binary stamps it with epoch 1 so it is accepted now.
        record(&StateEvent::new(
            "ffi-done-1",
            StateFact::QueueHeadCompleted {
                document_hash: doc_hash.to_string(),
                node_key: "ffi-answered-head".to_string(),
                backlog_id: None,
                hosting_epoch: None,
            },
        ));
        assert!(
            projection().contains("ffi-answered-head"),
            "in-hosting completed head should be accepted and projected"
        );

        // Supervisor re-hosts at a higher lease epoch (switch boundary): the
        // answered-head residue is dropped by the reset.
        record(&StateEvent::new(
            "ffi-host-2",
            StateFact::SupervisorHosting {
                document_hash: doc_hash.to_string(),
                pane_session: "%9:ffi-session".to_string(),
                lease_epoch: 2,
            },
        ));
        assert!(
            !projection().contains("ffi-answered-head"),
            "fresh host must drop the prior hosting's answered-head residue"
        );

        // A stale producer replays the answered head stamped with the OLD epoch;
        // it must be rejected, not re-injected.
        record(&StateEvent::new(
            "ffi-stale-done",
            StateFact::QueueHeadCompleted {
                document_hash: doc_hash.to_string(),
                node_key: "ffi-answered-head".to_string(),
                backlog_id: None,
                hosting_epoch: Some(1),
            },
        ));
        assert!(
            !projection().contains("ffi-answered-head"),
            "stale-epoch queue replay must not re-inject the answered head"
        );
    }

    #[test]
    fn state_subscribe_ffi_emits_snapshot_then_delta() {
        use agent_doc_state_backbone::{StateEvent, StateFact};

        let doc_hash = "state_subscribe_ffi_round_trip_doc";
        let doc_hash_c = CString::new(doc_hash).unwrap();

        // Cold read with no events: snapshot, epoch 0, empty graph.
        let cold_ptr = unsafe { agent_doc_state_subscribe(doc_hash_c.as_ptr(), 0) };
        let cold = unsafe { CStr::from_ptr(cold_ptr) }
            .to_str()
            .unwrap()
            .to_string();
        drop(unsafe { CString::from_raw(cold_ptr) });
        assert!(
            cold.contains("\"Snapshot\""),
            "cold subscribe with no state must yield a native Snapshot: {cold}"
        );
        assert!(
            cold.contains("\"epoch\":0"),
            "empty cold read is epoch 0: {cold}"
        );

        // Record one baseline event.
        let baseline = StateEvent::new(
            "evt-subscribe-ffi-1",
            StateFact::BaselineSaved {
                document_hash: doc_hash.to_string(),
                cycle_id: "cycle-subscribe".to_string(),
                baseline_hash: "bl-subscribe".to_string(),
                baseline_path: None,
            },
        );
        let baseline_json = CString::new(serde_json::to_string(&baseline).unwrap()).unwrap();
        let rc =
            unsafe { agent_doc_record_state_event(doc_hash_c.as_ptr(), baseline_json.as_ptr()) };
        assert_eq!(1, rc, "record_state_event should succeed");

        // Cold read now carries the baseline node at epoch 1.
        let cold_ptr = unsafe { agent_doc_state_subscribe(doc_hash_c.as_ptr(), 0) };
        let cold = unsafe { CStr::from_ptr(cold_ptr) }
            .to_str()
            .unwrap()
            .to_string();
        drop(unsafe { CString::from_raw(cold_ptr) });
        assert!(
            cold.contains("\"epoch\":1"),
            "one accepted event bumps epoch: {cold}"
        );
        assert!(
            cold.contains("agent_doc.document.baseline"),
            "cold snapshot must include the baseline type_tag: {cold}"
        );

        // Warm read at last_epoch=1 (caller is current) yields a no-op delta.
        let delta_ptr = unsafe { agent_doc_state_subscribe(doc_hash_c.as_ptr(), 1) };
        let delta = unsafe { CStr::from_ptr(delta_ptr) }
            .to_str()
            .unwrap()
            .to_string();
        drop(unsafe { CString::from_raw(delta_ptr) });
        assert!(
            delta.contains("\"Delta\""),
            "warm read must yield a native Delta: {delta}"
        );
        assert!(
            delta.contains("\"base_epoch\":1") && delta.contains("\"epoch\":1"),
            "no-op delta keeps base_epoch == epoch == current: {delta}"
        );
        assert!(
            delta.contains("\"ops\":[]"),
            "caller-current delta must be empty: {delta}"
        );
    }

    #[test]
    fn record_editor_op_ffi_writes_base_keyed_sidecar() {
        use agent_doc_op_capture_io as op_capture;
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let doc = dir.path().join("plan.md");
        std::fs::write(&doc, "# plan\n").unwrap();

        let base_text = "hello world\n";
        let base_hash = agent_doc_hash::content_hash(base_text);

        let file_c = CString::new(doc.to_str().unwrap()).unwrap();
        let base_c = CString::new(base_hash.as_str()).unwrap();
        let insert_kind = CString::new("insert").unwrap();
        let delete_kind = CString::new("delete").unwrap();
        let text_c = CString::new("!").unwrap();

        // Insert op records (returns 1).
        let rc = unsafe {
            agent_doc_record_editor_op(
                file_c.as_ptr(),
                base_c.as_ptr(),
                insert_kind.as_ptr(),
                5,
                text_c.as_ptr(),
                0,
            )
        };
        assert_eq!(rc, 1, "valid insert op must record");

        // Delete op appends within the same epoch (null insert_text is allowed).
        let rc = unsafe {
            agent_doc_record_editor_op(
                file_c.as_ptr(),
                base_c.as_ptr(),
                delete_kind.as_ptr(),
                0,
                std::ptr::null(),
                3,
            )
        };
        assert_eq!(rc, 1, "valid delete op must record");

        // The consumer trusts the sidecar only against the matching base, and the
        // ops round-trip in editor order.
        let ops = op_capture::editor_ops_for_base(&doc, base_text)
            .unwrap()
            .expect("ops captured against the base must be trusted");
        assert_eq!(ops.len(), 2);
        assert_eq!(
            ops[0],
            agent_doc_merge::crdt::EditorOp::Insert {
                offset: 5,
                text: "!".into()
            }
        );
        assert_eq!(
            ops[1],
            agent_doc_merge::crdt::EditorOp::Delete { offset: 0, len: 3 }
        );

        // Unknown kind and negative offset fail closed (return 0, record nothing).
        let bad_kind = CString::new("replace").unwrap();
        let rc = unsafe {
            agent_doc_record_editor_op(
                file_c.as_ptr(),
                base_c.as_ptr(),
                bad_kind.as_ptr(),
                0,
                text_c.as_ptr(),
                0,
            )
        };
        assert_eq!(rc, 0, "unknown op_kind must fail closed");
        let rc = unsafe {
            agent_doc_record_editor_op(
                file_c.as_ptr(),
                base_c.as_ptr(),
                insert_kind.as_ptr(),
                -1,
                text_c.as_ptr(),
                0,
            )
        };
        assert_eq!(rc, 0, "negative offset must fail closed");
        let ops = op_capture::editor_ops_for_base(&doc, base_text)
            .unwrap()
            .unwrap();
        assert_eq!(ops.len(), 2, "failed ops must not be recorded");
    }

    #[test]
    fn clear_editor_op_epoch_ffi_prevents_cross_projection_replay() {
        use agent_doc_op_capture_io as op_capture;
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let doc = dir.path().join("plan.md");
        std::fs::write(&doc, "# plan\n").unwrap();

        let base_text = "before projection\n";
        let base_hash = agent_doc_hash::content_hash(base_text);
        let file_c = CString::new(doc.to_str().unwrap()).unwrap();
        let base_c = CString::new(base_hash.as_str()).unwrap();
        let delete_kind = CString::new("delete").unwrap();
        assert_eq!(
            unsafe {
                agent_doc_record_editor_op(
                    file_c.as_ptr(),
                    base_c.as_ptr(),
                    delete_kind.as_ptr(),
                    0,
                    std::ptr::null(),
                    1,
                )
            },
            1
        );
        assert!(op_capture::has_pending_editor_ops(&doc));

        assert_eq!(
            unsafe { agent_doc_clear_editor_op_epoch(file_c.as_ptr()) },
            1
        );
        assert!(
            !op_capture::has_pending_editor_ops(&doc),
            "a remote projection must end the previous op epoch"
        );
        assert!(
            op_capture::editor_ops_for_base(&doc, base_text)
                .unwrap()
                .is_none(),
            "later edits cannot inherit operations from the prior frontier"
        );
    }

    #[test]
    fn record_editor_op_ffi_preserves_non_ascii_byte_offsets() {
        use agent_doc_op_capture_io as op_capture;
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let doc = dir.path().join("plan.md");
        std::fs::write(&doc, "# plan\n").unwrap();

        let base_text = "café 日本 😀\n";
        let base_hash = agent_doc_hash::content_hash(base_text);
        let offset = "café ".len() as i64;
        let delete_len = "日本".len() as i64;

        let file_c = CString::new(doc.to_str().unwrap()).unwrap();
        let base_c = CString::new(base_hash.as_str()).unwrap();
        let insert_kind = CString::new("insert").unwrap();
        let delete_kind = CString::new("delete").unwrap();
        let insert_text = CString::new("世界").unwrap();

        let rc = unsafe {
            agent_doc_record_editor_op(
                file_c.as_ptr(),
                base_c.as_ptr(),
                delete_kind.as_ptr(),
                offset,
                std::ptr::null(),
                delete_len,
            )
        };
        assert_eq!(rc, 1, "valid non-ASCII delete op must record");
        let rc = unsafe {
            agent_doc_record_editor_op(
                file_c.as_ptr(),
                base_c.as_ptr(),
                insert_kind.as_ptr(),
                offset,
                insert_text.as_ptr(),
                0,
            )
        };
        assert_eq!(rc, 1, "valid non-ASCII insert op must record");

        let ops = op_capture::editor_ops_for_base(&doc, base_text)
            .unwrap()
            .expect("non-ASCII ops captured against the base must be trusted");
        assert_eq!(
            ops,
            vec![
                agent_doc_merge::crdt::EditorOp::Delete { offset: 6, len: 6 },
                agent_doc_merge::crdt::EditorOp::Insert {
                    offset: 6,
                    text: "世界".into()
                },
            ]
        );

        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("editor_op_recorded kind=delete offset=6 delete_len=6"),
            "delete byte evidence missing:\n{ops_log}"
        );
        assert!(
            ops_log.contains(
                "editor_op_recorded kind=insert offset=6 insert_bytes=6 insert_non_ascii=true"
            ),
            "insert byte evidence missing:\n{ops_log}"
        );
        assert!(
            !ops_log.contains('�'),
            "ops log should not contain mojibake:\n{ops_log}"
        );
    }

    #[test]
    fn sync_try_lock_supersedes_a_guard_wedged_past_the_bound() {
        // Acquire, then simulate a holder that wedged ~1 minute ago by back-dating the
        // recorded acquisition time. A 45s-bound acquire must self-heal (supersede), and
        // a fresh acquire afterward must defer (the new holder is in-flight).
        assert_eq!(agent_doc_sync_try_lock(), 1, "free guard acquires");
        SYNC_LOCK_ACQUIRED_AT_MS.store(
            sync_lock_now_epoch_ms().saturating_sub(60_000),
            Ordering::SeqCst,
        );
        assert_eq!(
            agent_doc_sync_try_lock(),
            1,
            "a guard wedged past the bound is superseded, not deferred forever"
        );
        // The superseding acquire reset the timestamp to now, so a second contender defers.
        assert_eq!(
            agent_doc_sync_try_lock(),
            0,
            "a fresh in-flight holder still serializes later syncs"
        );
        agent_doc_sync_unlock();
        assert_eq!(
            SYNC_LOCK_ACQUIRED_AT_MS.load(Ordering::SeqCst),
            0,
            "unlock clears the holder timestamp"
        );
        assert_eq!(
            agent_doc_sync_try_lock(),
            1,
            "guard is free again after unlock"
        );
        agent_doc_sync_unlock();
    }

    fn parse_visual_tokens_json(doc: &str) -> Vec<serde_json::Value> {
        let c_doc = CString::new(doc).unwrap();
        let ptr = unsafe { agent_doc_visual_tokens_json(c_doc.as_ptr()) };
        assert!(!ptr.is_null(), "visual token JSON should not be null");
        let json_str = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_string();
        unsafe { agent_doc_free_string(ptr) };
        serde_json::from_str(&json_str).unwrap()
    }

    fn ffi_json_value(result: FfiJsonResult) -> serde_json::Value {
        if !result.error.is_null() {
            let error = unsafe { CStr::from_ptr(result.error) }
                .to_str()
                .unwrap()
                .to_string();
            unsafe { agent_doc_free_string(result.error) };
            panic!("unexpected FFI error: {error}");
        }
        assert!(!result.json.is_null(), "FFI JSON should not be null");
        let json = unsafe { CStr::from_ptr(result.json) }
            .to_str()
            .unwrap()
            .to_string();
        unsafe { agent_doc_free_string(result.json) };
        serde_json::from_str(&json).unwrap()
    }

    fn utf16_len(text: &str) -> usize {
        text.encode_utf16().count()
    }

    #[test]
    fn parse_components_roundtrip() {
        let doc = "before\n<!-- agent:status -->\nhello\n<!-- /agent:status -->\nafter\n";
        let c_doc = CString::new(doc).unwrap();
        let result = unsafe { agent_doc_parse_components(c_doc.as_ptr()) };
        assert_eq!(result.count, 1);
        assert!(!result.json.is_null());
        let json_str = unsafe { CStr::from_ptr(result.json) }.to_str().unwrap();
        let parsed: Vec<serde_json::Value> = serde_json::from_str(json_str).unwrap();
        assert_eq!(parsed[0]["name"], "status");
        assert_eq!(parsed[0]["content"], "hello\n");
        unsafe { agent_doc_free_string(result.json) };
    }

    #[test]
    fn admin_queue_control_ffi_returns_typed_receipt_json() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/admin-ffi.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-admin-ffi\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        agent_doc_session_actor_io::record_session_start_direct(
            &doc,
            "session-admin-ffi",
            "%41",
            "@1",
            1,
        )
        .unwrap();

        let root = CString::new(dir.path().to_string_lossy().to_string()).unwrap();
        let document = CString::new(doc.to_string_lossy().to_string()).unwrap();
        let action = CString::new("pause").unwrap();
        let reason = CString::new("ffi pause").unwrap();
        let accepted = unsafe {
            agent_doc_admin_queue_control_json(
                root.as_ptr(),
                document.as_ptr(),
                action.as_ptr(),
                1,
                reason.as_ptr(),
                std::ptr::null(),
            )
        };
        let accepted = ffi_json_value(accepted);
        assert_eq!(accepted["operation_kind"], "queue_paused");
        assert_eq!(accepted["status"], "accepted");
        assert!(accepted["receipt_id"].as_u64().unwrap() > 0);

        let stale = unsafe {
            agent_doc_admin_queue_control_json(
                root.as_ptr(),
                document.as_ptr(),
                action.as_ptr(),
                0,
                reason.as_ptr(),
                std::ptr::null(),
            )
        };
        let stale = ffi_json_value(stale);
        assert_eq!(stale["status"], "rejected");
        assert_eq!(stale["failed_stage"], "stale_generation");
        assert_eq!(stale["current_generation"], 1);
    }

    #[test]
    fn admin_inspect_ffi_returns_queue_diagnostics_json() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/admin-inspect-ffi.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-inspect-ffi\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        agent_doc_session_actor_io::record_session_start_direct(
            &doc,
            "session-inspect-ffi",
            "%42",
            "@1",
            1,
        )
        .unwrap();

        let root = CString::new(dir.path().to_string_lossy().to_string()).unwrap();
        let document = CString::new(doc.to_string_lossy().to_string()).unwrap();
        let action = CString::new("drain").unwrap();
        let reason = CString::new("ffi drain").unwrap();
        let item_id = CString::new("next").unwrap();
        let receipt = unsafe {
            agent_doc_admin_queue_control_json(
                root.as_ptr(),
                document.as_ptr(),
                action.as_ptr(),
                1,
                reason.as_ptr(),
                item_id.as_ptr(),
            )
        };
        assert_eq!(ffi_json_value(receipt)["status"], "accepted");

        let inspection = unsafe {
            agent_doc_admin_inspect_json(
                root.as_ptr(),
                document.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        let inspection = ffi_json_value(inspection);
        assert_eq!(inspection["record"]["pane_id"], "%42");
        assert_eq!(inspection["queue_control"]["state"], "draining");
        assert!(
            inspection["admin_operations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|operation| operation["operation_kind"] == "queue_draining"
                    && operation["status"] == "accepted")
        );
    }

    #[test]
    fn visual_tokens_json_uses_utf16_document_offsets() {
        let doc = "\
é
😀
❯ do #qey0. spec-test-build-install-commit-push
### Re: café — gpt-5
- [recommended] next
";
        let tokens = parse_visual_tokens_json(doc);
        let find = |kind: &str| {
            tokens
                .iter()
                .find(|token| token["kind"] == kind)
                .unwrap_or_else(|| panic!("missing token kind {kind}"))
        };

        let prompt = find("prompt");
        assert_eq!(
            prompt["start"].as_u64().unwrap() as usize,
            utf16_len("é\n😀\n")
        );
        assert_eq!(
            prompt["end"].as_u64().unwrap() as usize,
            utf16_len("é\n😀\n❯ do #qey0. spec-test-build-install-commit-push")
        );

        let heading = find("response_heading");
        assert_eq!(
            heading["start"].as_u64().unwrap() as usize,
            utf16_len("é\n😀\n❯ do #qey0. spec-test-build-install-commit-push\n")
        );
        assert_eq!(
            heading["end"].as_u64().unwrap() as usize,
            utf16_len(
                "é\n😀\n❯ do #qey0. spec-test-build-install-commit-push\n### Re: café — gpt-5"
            )
        );

        let label = find("label_tag");
        assert_eq!(
            label["start"].as_u64().unwrap() as usize,
            utf16_len(
                "é\n😀\n❯ do #qey0. spec-test-build-install-commit-push\n### Re: café — gpt-5\n- "
            )
        );
        assert_eq!(
            label["end"].as_u64().unwrap() as usize,
            utf16_len(
                "é\n😀\n❯ do #qey0. spec-test-build-install-commit-push\n### Re: café — gpt-5\n- [recommended]"
            )
        );
    }

    #[test]
    fn apply_patch_replace() {
        let doc = "<!-- agent:output -->\nold\n<!-- /agent:output -->\n";
        let c_doc = CString::new(doc).unwrap();
        let c_name = CString::new("output").unwrap();
        let c_content = CString::new("new content\n").unwrap();
        let c_mode = CString::new("replace").unwrap();
        let result = unsafe {
            agent_doc_apply_patch(
                c_doc.as_ptr(),
                c_name.as_ptr(),
                c_content.as_ptr(),
                c_mode.as_ptr(),
            )
        };
        assert!(result.error.is_null());
        assert!(!result.text.is_null());
        let text = unsafe { CStr::from_ptr(result.text) }.to_str().unwrap();
        assert!(text.contains("new content"));
        assert!(!text.contains("old"));
        unsafe { agent_doc_free_string(result.text) };
    }

    #[test]
    fn apply_patch_with_boundary_marks_new_exchange_heading_with_head() {
        let doc = "<!-- agent:exchange patch=append -->\n### Re: earlier — gpt-5\nBody.\n<!-- agent:boundary:abc12345 -->\n<!-- /agent:exchange -->\n";
        let c_doc = CString::new(doc).unwrap();
        let c_name = CString::new("exchange").unwrap();
        let c_content = CString::new("### Re: latest — gpt-5\nNew body.\n").unwrap();
        let c_mode = CString::new("append").unwrap();
        let c_boundary = CString::new("abc12345").unwrap();
        let result = unsafe {
            agent_doc_apply_patch_with_boundary(
                c_doc.as_ptr(),
                c_name.as_ptr(),
                c_content.as_ptr(),
                c_mode.as_ptr(),
                c_boundary.as_ptr(),
            )
        };
        assert!(result.error.is_null());
        assert!(!result.text.is_null());
        let text = unsafe { CStr::from_ptr(result.text) }.to_str().unwrap();
        assert!(text.contains("### Re: earlier — gpt-5\n"));
        assert!(text.contains("### Re: latest — gpt-5 (HEAD)\n"));
        assert_eq!(text.matches("(HEAD)").count(), 1, "got:\n{text}");
        unsafe { agent_doc_free_string(result.text) };
    }

    #[test]
    fn normalize_template_structure_ffi_repairs_safe_duplicate_scaffold() {
        let doc = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#aaaa] keep me\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#aaaa] keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        let c_doc = CString::new(doc).unwrap();

        let result = unsafe { agent_doc_normalize_template_structure(c_doc.as_ptr()) };

        assert!(result.error.is_null());
        assert!(!result.text.is_null());
        let text = unsafe { CStr::from_ptr(result.text) }.to_str().unwrap();
        assert_eq!(text.matches("<!-- /agent:exchange -->").count(), 1);
        assert_eq!(text.matches("<!-- agent:queue -->").count(), 1);
        assert_eq!(text.matches("<!-- agent:backlog -->").count(), 1);
        unsafe { agent_doc_free_string(result.text) };
    }

    #[test]
    fn normalize_template_structure_ffi_rejects_mixed_duplicate_scaffold() {
        let doc = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n",
            "user typed into duplicated shell\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n"
        );
        let c_doc = CString::new(doc).unwrap();

        let result = unsafe { agent_doc_normalize_template_structure(c_doc.as_ptr()) };

        assert!(result.text.is_null());
        assert!(!result.error.is_null());
        let error = unsafe { CStr::from_ptr(result.error) }.to_str().unwrap();
        assert!(
            error.contains("mixed duplicate scaffold"),
            "unexpected error: {error}"
        );
        unsafe { agent_doc_free_string(result.error) };
    }

    #[test]
    fn normalize_template_structure_ffi_rejects_truncated_agent_comment() {
        let doc = concat!(
            "<!-- agent:queue -->\n",
            "- a\n",
            "<!-- /agent:queue ->\n",
            "<!-- /agent:exchange --\n",
        );
        let c_doc = CString::new(doc).unwrap();

        let result = unsafe { agent_doc_normalize_template_structure(c_doc.as_ptr()) };

        assert!(result.text.is_null());
        assert!(!result.error.is_null());
        let error = unsafe { CStr::from_ptr(result.error) }.to_str().unwrap();
        assert!(
            error.contains("malformed_agent_comment"),
            "unexpected error: {error}"
        );
        unsafe { agent_doc_free_string(result.error) };
    }

    #[test]
    fn merge_frontmatter_adds_field() {
        let doc = "---\nagent_doc_session: abc\n---\nBody\n";
        let fields = "model: opus";
        let c_doc = CString::new(doc).unwrap();
        let c_fields = CString::new(fields).unwrap();
        let result = unsafe { agent_doc_merge_frontmatter(c_doc.as_ptr(), c_fields.as_ptr()) };
        assert!(result.error.is_null());
        assert!(!result.text.is_null());
        let text = unsafe { CStr::from_ptr(result.text) }.to_str().unwrap();
        assert!(text.contains("model: opus"));
        assert!(text.contains("agent_doc_session: abc"));
        assert!(text.contains("Body"));
        unsafe { agent_doc_free_string(result.text) };
    }

    #[test]
    fn reposition_boundary_removes_stale() {
        let doc = "<!-- agent:exchange patch=append -->\ntext\n<!-- agent:boundary:aaaa1111 -->\nmore\n<!-- agent:boundary:bbbb2222 -->\n<!-- /agent:exchange -->\n";
        let c_doc = CString::new(doc).unwrap();
        let result = unsafe { agent_doc_reposition_boundary_to_end(c_doc.as_ptr()) };
        assert!(result.error.is_null());
        assert!(!result.text.is_null());
        let text = unsafe { CStr::from_ptr(result.text) }.to_str().unwrap();
        // Should have exactly one boundary marker at the end
        let boundary_count = text.matches("<!-- agent:boundary:").count();
        assert_eq!(
            boundary_count, 1,
            "should have exactly 1 boundary, got {}",
            boundary_count
        );
        // The boundary should be just before the close tag
        assert!(text.contains("more\n<!-- agent:boundary:"));
        assert!(text.contains(" -->\n<!-- /agent:exchange -->"));
        unsafe { agent_doc_free_string(result.text) };
    }

    #[test]
    fn reposition_boundary_with_id_reuses_requested_marker() {
        let doc = "<!-- agent:exchange patch=append -->\ntext\n<!-- agent:boundary:aaaa1111 -->\nmore\n<!-- /agent:exchange -->\n";
        let c_doc = CString::new(doc).unwrap();
        let c_id = CString::new("keep-this-id").unwrap();
        let result =
            unsafe { agent_doc_reposition_boundary_to_end_with_id(c_doc.as_ptr(), c_id.as_ptr()) };
        assert!(result.error.is_null());
        assert!(!result.text.is_null());
        let text = unsafe { CStr::from_ptr(result.text) }.to_str().unwrap();
        assert!(text.contains("<!-- agent:boundary:keep-this-id -->"));
        assert_eq!(text.matches("<!-- agent:boundary:").count(), 1);
        unsafe { agent_doc_free_string(result.text) };
    }

    #[test]
    fn reposition_preserve_head_keeps_head_markers() {
        let doc = "<!-- agent:exchange patch=append -->\n### Re: topic (HEAD)\ntext\n<!-- agent:boundary:aaaa1111 -->\n<!-- /agent:exchange -->\n";
        let c_doc = CString::new(doc).unwrap();
        let result = unsafe { agent_doc_reposition_boundary_to_end_preserve_head(c_doc.as_ptr()) };
        assert!(result.error.is_null());
        assert!(!result.text.is_null());
        let text = unsafe { CStr::from_ptr(result.text) }.to_str().unwrap();
        assert!(
            text.contains("### Re: topic (HEAD)"),
            "preserve_head FFI must keep (HEAD); got:\n{text}"
        );
        assert!(!text.contains("aaaa1111"), "old boundary gone");
        assert_eq!(text.matches("<!-- agent:boundary:").count(), 1);
        unsafe { agent_doc_free_string(result.text) };
    }

    #[test]
    fn reposition_preserve_head_with_id_keeps_head_and_id() {
        let doc = "<!-- agent:exchange patch=append -->\n### Re: topic (HEAD)\ntext\n<!-- agent:boundary:aaaa1111 -->\n<!-- /agent:exchange -->\n";
        let c_doc = CString::new(doc).unwrap();
        let c_id = CString::new("my-id").unwrap();
        let result = unsafe {
            agent_doc_reposition_boundary_to_end_preserve_head_with_id(
                c_doc.as_ptr(),
                c_id.as_ptr(),
            )
        };
        assert!(result.error.is_null());
        assert!(!result.text.is_null());
        let text = unsafe { CStr::from_ptr(result.text) }.to_str().unwrap();
        assert!(
            text.contains("### Re: topic (HEAD)"),
            "preserve_head FFI must keep (HEAD); got:\n{text}"
        );
        assert!(
            text.contains("<!-- agent:boundary:my-id -->"),
            "explicit id used; got:\n{text}"
        );
        assert_eq!(text.matches("<!-- agent:boundary:").count(), 1);
        unsafe { agent_doc_free_string(result.text) };
    }

    #[test]
    fn resolve_project_path_finds_nested_submodule() {
        use tempfile::TempDir;
        let outer = TempDir::new().unwrap();
        // Outer project root has .agent-doc/
        std::fs::create_dir_all(outer.path().join(".agent-doc")).unwrap();
        // Nested submodule with its own .agent-doc/
        let sub = outer.path().join("src/session-share");
        std::fs::create_dir_all(sub.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(sub.join("tasks")).unwrap();
        let doc = sub.join("tasks/claudescore.md");
        std::fs::write(&doc, "# test\n").unwrap();

        let c_path = CString::new(doc.to_str().unwrap()).unwrap();
        let result = unsafe { agent_doc_resolve_project_path(c_path.as_ptr()) };
        assert!(
            !result.project_root.is_null(),
            "project_root should be non-null"
        );
        assert!(
            !result.relative_path.is_null(),
            "relative_path should be non-null"
        );

        let root = unsafe { CStr::from_ptr(result.project_root) }
            .to_str()
            .unwrap();
        let rel = unsafe { CStr::from_ptr(result.relative_path) }
            .to_str()
            .unwrap();
        // Nearest .agent-doc/ is the submodule, not the outer project.
        let expected_root = sub.canonicalize().unwrap();
        assert_eq!(std::path::Path::new(root), expected_root);
        assert_eq!(rel, "tasks/claudescore.md");

        unsafe {
            agent_doc_free_string(result.project_root);
            agent_doc_free_string(result.relative_path);
        }
    }

    #[test]
    fn resolve_project_path_prefers_nearest_ancestor() {
        use tempfile::TempDir;
        let outer = TempDir::new().unwrap();
        std::fs::create_dir_all(outer.path().join(".agent-doc")).unwrap();
        let mid = outer.path().join("mid");
        std::fs::create_dir_all(mid.join(".agent-doc")).unwrap();
        let deep = mid.join("deep/subdir");
        std::fs::create_dir_all(&deep).unwrap();
        let doc = deep.join("doc.md");
        std::fs::write(&doc, "").unwrap();

        let c_path = CString::new(doc.to_str().unwrap()).unwrap();
        let result = unsafe { agent_doc_resolve_project_path(c_path.as_ptr()) };
        let root = unsafe { CStr::from_ptr(result.project_root) }
            .to_str()
            .unwrap();
        let rel = unsafe { CStr::from_ptr(result.relative_path) }
            .to_str()
            .unwrap();
        assert_eq!(
            std::path::Path::new(root),
            mid.canonicalize().unwrap(),
            "should prefer nearest (mid) over outer"
        );
        assert_eq!(rel, "deep/subdir/doc.md");
        unsafe {
            agent_doc_free_string(result.project_root);
            agent_doc_free_string(result.relative_path);
        }
    }

    #[test]
    fn resolve_project_path_file_directly_in_root() {
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let doc = tmp.path().join("plan.md");
        std::fs::write(&doc, "").unwrap();

        let c_path = CString::new(doc.to_str().unwrap()).unwrap();
        let result = unsafe { agent_doc_resolve_project_path(c_path.as_ptr()) };
        assert!(!result.project_root.is_null());
        let rel = unsafe { CStr::from_ptr(result.relative_path) }
            .to_str()
            .unwrap();
        assert_eq!(rel, "plan.md");
        unsafe {
            agent_doc_free_string(result.project_root);
            agent_doc_free_string(result.relative_path);
        }
    }

    #[test]
    fn crdt_merge_no_base() {
        let c_ours = CString::new("hello world").unwrap();
        let c_theirs = CString::new("hello world").unwrap();
        let result =
            unsafe { agent_doc_crdt_merge(ptr::null(), 0, c_ours.as_ptr(), c_theirs.as_ptr()) };
        assert!(result.error.is_null());
        assert!(!result.text.is_null());
        let text = unsafe { CStr::from_ptr(result.text) }.to_str().unwrap();
        assert_eq!(text, "hello world");
        unsafe {
            agent_doc_free_string(result.text);
            agent_doc_free_state(result.state, result.state_len);
        };
    }
}

#[cfg(test)]
mod ack_content_tests {
    use super::*;
    use std::ffi::CString;
    use tempfile::TempDir;

    #[test]
    fn test_write_ack_content_legacy_abi_is_rejected() {
        let tmp = TempDir::new().unwrap();
        let project_root = CString::new(tmp.path().to_str().unwrap()).unwrap();
        let patch_id = CString::new("test-patch-id-123").unwrap();
        let content = CString::new("hello world").unwrap();

        let result = unsafe {
            agent_doc_write_ack_content(project_root.as_ptr(), patch_id.as_ptr(), content.as_ptr())
        };
        assert_eq!(result, 0, "legacy no-capability ABI should fail closed");

        let sidecar = tmp
            .path()
            .join(".agent-doc/ack-content/test-patch-id-123.md");
        assert!(
            !sidecar.exists(),
            "legacy ABI must not write a projection sidecar at {:?}",
            sidecar
        );
    }

    #[test]
    fn test_editor_content_applied_for_editor_v1_records_receipt_without_live_buffer_sidecar() {
        let tmp = TempDir::new().unwrap();
        let doc = tmp.path().join("session.md");
        std::fs::write(&doc, "before\n").unwrap();
        let project_root = CString::new(tmp.path().to_str().unwrap()).unwrap();
        let patch_id = CString::new("test-patch-id-ack-v2").unwrap();
        let file_path = CString::new(doc.to_string_lossy().to_string()).unwrap();
        let content = CString::new("before\n### Re: done\n").unwrap();
        let editor_id = CString::new("jetbrains:test").unwrap();
        let editor_kind = CString::new("jetbrains").unwrap();
        let editor_version = CString::new("test").unwrap();
        let capabilities = CString::new(format!(
            "{},{}",
            agent_doc_document_realtime::editor_contract::OPERATOR_TEXT_AUTHORITY_CAPABILITY,
            agent_doc_document_realtime::editor_contract::LAZILY_TRANSPORT_RECEIPTS_CAPABILITY
        ))
        .unwrap();

        let result = unsafe {
            agent_doc_editor_content_applied_for_editor_v1(
                project_root.as_ptr(),
                patch_id.as_ptr(),
                file_path.as_ptr(),
                content.as_ptr(),
                editor_id.as_ptr(),
                editor_kind.as_ptr(),
                editor_version.as_ptr(),
                capabilities.as_ptr(),
            )
        };
        assert_eq!(result, 1, "should return 1 on success");

        assert!(
            !tmp.path()
                .join(".agent-doc/ack-content/test-patch-id-ack-v2.md")
                .exists(),
            "current lazily receipt ABI must not write an ack-content compatibility sidecar"
        );
        let proof =
            agent_doc_controller_io::project_controller::visible_write_commit_candidate_for_patch_file(
                &doc,
                "test-patch-id-ack-v2",
            )
            .expect("consolidated receipt should publish a visible-write projection");
        assert_eq!(
            proof.commit_candidate_hash,
            agent_doc_hash::content_hash(
                &agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(
                    "before\n### Re: done\n"
                )
            )
        );
        assert_eq!(
            proof.source, "editor_patch_applied_for_editor_v2",
            "visible-write projection should record the editor ABI source"
        );

        assert!(
            !tmp.path().join(".agent-doc/live-buffer").exists(),
            "editor-applied receipt must not create live-buffer sidecars"
        );
    }

    #[test]
    fn test_lazily_current_observed_v1_does_not_touch_legacy_sidecar() {
        let tmp = TempDir::new().unwrap();
        let doc = tmp.path().join("session.md");
        let doc_content = "---\nagent: claude\nagent_doc_format: template\n---\n\n### Re: remote\n";
        std::fs::write(&doc, doc_content).unwrap();
        let file_path = CString::new(doc.to_string_lossy().to_string()).unwrap();
        let content = CString::new(doc_content).unwrap();
        let editor_id = CString::new("jetbrains:test").unwrap();
        let editor_kind = CString::new("jetbrains").unwrap();
        let editor_version = CString::new("test").unwrap();
        let capabilities = CString::new(format!(
            "{},{}",
            agent_doc_document_realtime::editor_contract::OPERATOR_TEXT_AUTHORITY_CAPABILITY,
            agent_doc_document_realtime::editor_contract::LAZILY_TRANSPORT_RECEIPTS_CAPABILITY
        ))
        .unwrap();

        unsafe {
            agent_doc_lazily_current_observed_v1(
                file_path.as_ptr(),
                content.as_ptr(),
                editor_id.as_ptr(),
                editor_kind.as_ptr(),
                editor_version.as_ptr(),
                capabilities.as_ptr(),
                1,
            )
        };

        assert!(
            !tmp.path().join(".agent-doc/live-buffer").exists(),
            "the current edit hot path must not create legacy sidecars"
        );
    }

    #[test]
    fn test_editor_content_applied_for_editor_v1_requires_lazily_receipt_capability() {
        let tmp = TempDir::new().unwrap();
        let doc = tmp.path().join("session.md");
        std::fs::write(&doc, "before\n").unwrap();
        let project_root = CString::new(tmp.path().to_str().unwrap()).unwrap();
        let patch_id = CString::new("test-patch-id-missing-cap").unwrap();
        let file_path = CString::new(doc.to_string_lossy().to_string()).unwrap();
        let content = CString::new("before\n### Re: done\n").unwrap();
        let editor_id = CString::new("jetbrains:test").unwrap();
        let editor_kind = CString::new("jetbrains").unwrap();
        let editor_version = CString::new("test").unwrap();
        let capabilities = CString::new(
            agent_doc_document_realtime::editor_contract::OPERATOR_TEXT_AUTHORITY_CAPABILITY,
        )
        .unwrap();

        let result = unsafe {
            agent_doc_editor_content_applied_for_editor_v1(
                project_root.as_ptr(),
                patch_id.as_ptr(),
                file_path.as_ptr(),
                content.as_ptr(),
                editor_id.as_ptr(),
                editor_kind.as_ptr(),
                editor_version.as_ptr(),
                capabilities.as_ptr(),
            )
        };
        assert_eq!(
            result, 0,
            "current editor bridge should reject plugins without lazily receipt support"
        );
        assert!(
            !tmp.path()
                .join(".agent-doc/ack-content/test-patch-id-missing-cap.md")
                .exists(),
            "missing-capability call must not write a projection sidecar"
        );
        assert!(
            !tmp.path().join(".agent-doc/live-buffer").exists(),
            "missing-capability call must not create live-buffer proof"
        );
    }

    #[test]
    fn test_write_ack_content_for_editor_v2_legacy_abi_is_rejected() {
        let tmp = TempDir::new().unwrap();
        let doc = tmp.path().join("session.md");
        std::fs::write(&doc, "before\n").unwrap();
        let project_root = CString::new(tmp.path().to_str().unwrap()).unwrap();
        let patch_id = CString::new("test-patch-id-old-v2").unwrap();
        let file_path = CString::new(doc.to_string_lossy().to_string()).unwrap();
        let content = CString::new("before\n### Re: done\n").unwrap();
        let editor_id = CString::new("jetbrains:test").unwrap();
        let editor_kind = CString::new("jetbrains").unwrap();
        let editor_version = CString::new("test").unwrap();
        let capabilities = CString::new(format!(
            "{},{}",
            agent_doc_document_realtime::editor_contract::OPERATOR_TEXT_AUTHORITY_CAPABILITY,
            agent_doc_document_realtime::editor_contract::LAZILY_TRANSPORT_RECEIPTS_CAPABILITY
        ))
        .unwrap();

        let result = unsafe {
            agent_doc_write_ack_content_for_editor_v2(
                project_root.as_ptr(),
                patch_id.as_ptr(),
                file_path.as_ptr(),
                content.as_ptr(),
                editor_id.as_ptr(),
                editor_kind.as_ptr(),
                editor_version.as_ptr(),
                capabilities.as_ptr(),
            )
        };
        assert_eq!(result, 0, "legacy receipt-named v2 ABI should fail closed");
        assert!(
            !tmp.path()
                .join(".agent-doc/ack-content/test-patch-id-old-v2.md")
                .exists(),
            "legacy receipt-named v2 ABI must not write a projection sidecar"
        );
    }

    #[test]
    fn patch_content_already_committed_requires_disk_to_match_head() {
        use std::process::Command;
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        macro_rules! git {
            ($($arg:expr),+) => {
                Command::new("git")
                    .current_dir(root)
                    .env_remove("GIT_DIR")
                    .env_remove("GIT_INDEX_FILE")
                    .env_remove("GIT_WORK_TREE")
                    .args([$($arg),+])
                    .output()
                    .unwrap()
            };
        }

        git!["init"];
        git!["config", "user.email", "test@test.com"];
        git!["config", "user.name", "Test"];

        let doc = root.join("doc.md");
        let committed = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: committed — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, committed).unwrap();
        git!["add", "doc.md"];
        git!["commit", "-m", "committed response", "--no-verify"];

        let file_path = CString::new(doc.to_string_lossy().as_ref()).unwrap();
        let patch_content = CString::new("### Re: committed — gpt-5\n\nDone.\n").unwrap();
        assert_eq!(
            unsafe {
                agent_doc_patch_content_already_committed(
                    file_path.as_ptr(),
                    patch_content.as_ptr(),
                )
            },
            1,
            "committed disk content should prove the patch is already present"
        );

        std::fs::write(
            &doc,
            "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n",
        )
        .unwrap();
        assert_eq!(
            unsafe {
                agent_doc_patch_content_already_committed(
                    file_path.as_ptr(),
                    patch_content.as_ptr(),
                )
            },
            0,
            "HEAD alone must not be enough when disk drifted away from HEAD"
        );
    }

    // --- Fix 4: agent_doc_commit FFI export ---

    #[test]
    fn agent_doc_commit_returns_false_for_null() {
        let result = unsafe { agent_doc_commit(std::ptr::null()) };
        assert_eq!(result, 0, "null path should return 0");
    }

    #[test]
    fn ffi_git_commit_commits_staged_file() {
        use std::process::Command;
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        // Helper: run git command isolated from any parent git hook env vars.
        // Pre-commit hooks set GIT_DIR/GIT_INDEX_FILE which would confuse
        // git commands targeting the temp repo.
        macro_rules! git {
            ($($arg:expr),+) => {
                Command::new("git")
                    .current_dir(root)
                    .env_remove("GIT_DIR")
                    .env_remove("GIT_INDEX_FILE")
                    .env_remove("GIT_WORK_TREE")
                    .args([$($arg),+])
                    .output()
                    .unwrap()
            };
        }

        // Set up minimal git repo
        git!["init"];
        git!["config", "user.email", "test@test.com"];
        git!["config", "user.name", "Test"];

        // Commit initial file so HEAD exists
        let readme = root.join("README.md");
        std::fs::write(&readme, "# test\n").unwrap();
        git!["add", "README.md"];
        git!["commit", "-m", "initial", "--no-verify"];

        // Create a document file (not yet committed)
        let doc = root.join("session.md");
        std::fs::write(&doc, "# content\n").unwrap();

        // ffi_git_commit should stage + commit the doc
        let ok = ffi_git_commit(&doc);
        assert!(ok, "ffi_git_commit should succeed for a valid git repo");

        // Verify git log contains the commit
        let log = git!["log", "--oneline", "-2"];
        let log_str = String::from_utf8_lossy(&log.stdout);
        assert!(
            log_str.contains("agent-doc(session):"),
            "git log should contain agent-doc commit, got:\n{log_str}"
        );
    }
}
