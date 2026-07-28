/**
 * FFI bindings to libagent_doc via koffi.
 *
 * Loads the shared library and exposes typed wrappers for the C ABI functions.
 * Falls back gracefully when koffi is not installed or the library is not found.
 *
 * Library resolution order:
 * 1. $AGENT_DOC_LIB_PATH (explicit override)
 * 2. <project_root>/.agent-doc/lib/libagent_doc.so (project-local)
 * 3. ~/.local/lib/libagent_doc.so (user-local)
 * 4. System library search (dlopen fallback)
 */

import * as path from 'path';
import * as os from 'os';
import * as fs from 'fs';
import * as crypto from 'crypto';
import { createRequire } from 'node:module';
import {
    GraphView,
    agentDocProjectionFromView,
    applyIpcMessageToView,
    type AgentDocProjection,
} from './stateMirror.js';

const requireFromExtension = createRequire(import.meta.url);
/** Owned Rust C strings must remain pointers until decoded and explicitly freed. */
export const OWNED_C_STRING_POINTER = 'void*' as const;

// Result struct returned by FFI functions that produce text
interface FfiPatchResult {
    text: any; // koffi pointer
    error: any; // koffi pointer
}

interface FfiComponentList {
    json: any;
    error: any;
    count: number;
}

interface FfiJsonResult {
    json: any;
    error: any;
}

export interface ResolvedProjectPath {
    projectRoot: string;
    relativePath: string;
}

export interface VisualToken {
    kind: string;
    start: number;
    end: number;
}

export interface StateBackboneEvent {
    event_id: string;
    causation_id?: string;
    fact: Record<string, unknown> & {
        type: string;
        document_hash: string;
    };
}

export interface ProjectionSummary {
    routeReadiness?: string;
    routePaneId?: string;
    latestTransportPatchId?: string;
    latestTransportPhase?: string;
    proofMarkers: number;
}

let lib: any = null;
let koffi: any = null;
let loaded = false;
let loadAttempted = false;
let loadedPath: string | null = null;
let loadedMtime: number = 0;
let currentLockFile: string | null = null;
const stateGenerations = new Map<string, number>();

export function libMtimeChanged(filePath: string, storedMtime: number): boolean {
    try {
        const currentMtime = fs.statSync(filePath).mtimeMs;
        return currentMtime !== storedMtime && currentMtime > 0;
    } catch {
        return false;
    }
}

/**
 * Copy the freshly-installed shared library to a unique, per-mtime path so
 * `koffi.load` (and the underlying `dlopen`) actually maps the NEW native code.
 *
 * Loading the canonical install path in place does NOT pick up a new build:
 * `dlopen` returns the already-mapped handle for an unchanged path, so the
 * extension host keeps running stale native code after `make install` /
 * `agent-doc lib-install` until a full window reload. Copying to
 * `libagent_doc-<mtime><ext>` under `cacheRoot` gives each install a distinct
 * inode, forcing a real load so hot-reload works without reloading the window.
 * Prunes stale copies from earlier installs. Returns the shadow path, or null on
 * failure so the caller can fall back to the canonical path (never worse).
 */
export function nativeShadowCopyPath(canonicalPath: string, mtime: number, cacheRoot: string): string | null {
    try {
        const ext = path.extname(canonicalPath) || '.so';
        fs.mkdirSync(cacheRoot, { recursive: true });
        const dest = path.join(cacheRoot, `libagent_doc-${Math.floor(mtime)}${ext}`);
        const srcSize = fs.statSync(canonicalPath).size;
        let needCopy = true;
        try {
            needCopy = fs.statSync(dest).size !== srcSize;
        } catch {
            needCopy = true;
        }
        if (needCopy) {
            fs.copyFileSync(canonicalPath, dest);
            for (const name of fs.readdirSync(cacheRoot)) {
                const full = path.join(cacheRoot, name);
                if (name.startsWith('libagent_doc-') && full !== dest) {
                    try { fs.unlinkSync(full); } catch { /* best-effort prune */ }
                }
            }
        }
        return dest;
    } catch {
        return null;
    }
}

function shadowCopyForLoad(canonicalPath: string, mtime: number): string {
    const cacheRoot = path.join(os.tmpdir(), `agent-doc-native-${process.pid}`);
    const shadow = nativeShadowCopyPath(canonicalPath, mtime, cacheRoot);
    if (!shadow) {
        console.log('[agent-doc/native] shadow copy failed; loading canonical path in place (may keep stale native code until window reload)');
        return canonicalPath;
    }
    return shadow;
}

export function writePidLock(libPath: string): void {
    try {
        const resolved = fs.realpathSync(libPath);
        const pid = process.pid;
        const lockPath = `${resolved}.pid.${pid}`;
        fs.writeFileSync(lockPath, '');
        currentLockFile = lockPath;
    } catch {
        // best-effort
    }
}

export function removePidLock(): void {
    if (!currentLockFile) return;
    try {
        fs.unlinkSync(currentLockFile);
    } catch {
        // already removed or never created
    }
    currentLockFile = null;
}

function resetBindings(): void {
    _reposition_boundary_to_end = null;
    _reposition_boundary_to_end_preserve_head = null;
    _reposition_boundary_to_end_preserve_head_with_id = null;
    _normalize_template_structure = null;
    _apply_node_patches = null;
    _admin_inspect_json = null;
    _tmux_focus_state_json = null;
    _focus_document_pane_json = null;
    _sync_tmux_layout_json = null;
    _editor_surface_observe_json = null;
    _editor_surface_forget = null;
    _admin_queue_control_json = null;
    _admin_reap_json = null;
    _admin_handoff_json = null;
    _admin_repair_projection_json = null;
    _visual_tokens_json = null;
    _lazily_current_observed_v1 = null;
    _document_closed_for_editor = null;
    _deferred_write_reconnect_content = null;
    _deferred_write_reconnect_propagated = null;
    _resolve_project_path = null;
    _document_id_for_path = null;
    _is_session_document = null;
    _reliable_sync_liveness_enqueue = null;
    _reliable_sync_liveness_flush = null;
    _reliable_sync_document_op_push = null;
    _reliable_sync_text_adopt_push = null;
    _reliable_sync_document_op_flush = null;
    _peer_replicas_missing = null;
    _free_state = null;
    _free_string = null;
    _version = null;
    _state_projection = null;
    _state_subscribe = null;
    _record_state_event = null;
    _editor_content_applied_for_editor_v1 = null;
    _editor_patch_applied = null;
    _editor_patch_rejected = null;
    _record_editor_surface_event = null;
    _record_editor_op = null;
    _document_base_hash = null;
    _replica_open = null;
    _replica_apply_local = null;
    _replica_apply_update = null;
    _replica_state_vector = null;
    _replica_diff = null;
    _replica_encode_state = null;
    _replica_text = null;
    _replica_close = null;
}

const LIB_NAME = process.platform === 'darwin' ? 'libagent_doc.dylib' : 'libagent_doc.so';
export const EDITOR_PLUGIN_KIND = 'vscode';
export const EDITOR_PLUGIN_VERSION = '0.2.62';
const OPERATOR_TEXT_AUTHORITY_CAPABILITY = 'operator_text_authority_v1';
const LAZILY_TRANSPORT_RECEIPTS_CAPABILITY = 'lazily_transport_receipts_v1';
// #ctrlkillreregister Tier 3: this extension calls agent_doc_peer_replicas_missing
// about itself on activation and on controller-transport recovery, so the
// controller's Tier 1 restart fan-out must stop pushing rebuild requests at it. The
// token is the retirement condition and travels on the registration, which is part of
// the same replicated liveness plane — so the push retires per peer with no flag day.
// Kept in sync with
// agent_doc_document_realtime::editor_contract::PEER_REPLICA_PULL_CAPABILITY.
const PEER_REPLICA_PULL_CAPABILITY = 'peer_replica_pull_v1';
export const EDITOR_CAPABILITY_LIST = [
OPERATOR_TEXT_AUTHORITY_CAPABILITY,
LAZILY_TRANSPORT_RECEIPTS_CAPABILITY,
PEER_REPLICA_PULL_CAPABILITY,
];
const EDITOR_CAPABILITIES = EDITOR_CAPABILITY_LIST.join(',');

function findLibrary(projectRoot?: string): string | null {
    // 1. Explicit env var
    const envPath = process.env.AGENT_DOC_LIB_PATH;
    if (envPath && fs.existsSync(envPath)) return envPath;

    // 2. Project-local
    if (projectRoot) {
        const projectLib = path.join(projectRoot, '.agent-doc', 'lib', LIB_NAME);
        if (fs.existsSync(projectLib)) return projectLib;
    }

    // 3. User-local
    const userLib = path.join(os.homedir(), '.local', 'lib', LIB_NAME);
    if (fs.existsSync(userLib)) return userLib;

    // 4. Cargo target (development)
    if (projectRoot) {
        // Walk up to find src/agent-doc/target/release/
        let dir = projectRoot;
        const root = path.parse(dir).root;
        while (dir !== root) {
            const candidate = path.join(dir, 'src', 'agent-doc', 'target', 'release', LIB_NAME);
            if (fs.existsSync(candidate)) return candidate;
            dir = path.dirname(dir);
        }
    }

    return null;
}

function ensureLoaded(projectRoot?: string): boolean {
    if (loaded) {
        if (loadedPath && libMtimeChanged(loadedPath, loadedMtime)) {
            console.log(`[agent-doc/native] mtime changed, reloading from ${loadedPath}`);
            removePidLock();
            resetBindings();
            try {
                const currentMtime = fs.statSync(loadedPath).mtimeMs;
                lib = koffi.load(shadowCopyForLoad(loadedPath, currentMtime));
                loadedMtime = currentMtime;
                writePidLock(loadedPath);
                verifyVersion(loadedPath);
            } catch (e: any) {
                console.log(`[agent-doc/native] reload failed, keeping previous: ${e.message}`);
            }
        }
        return true;
    }
    if (loadAttempted) return false;
    loadAttempted = true;

    try {
        koffi = requireFromExtension('koffi');
    } catch {
        console.log('[agent-doc/native] koffi not available, FFI disabled');
        return false;
    }

    const libPath = findLibrary(projectRoot);
    if (!libPath) {
        console.log('[agent-doc/native] libagent_doc not found, FFI disabled');
        return false;
    }

    try {
        loadedPath = libPath;
        loadedMtime = fs.statSync(libPath).mtimeMs;
        lib = koffi.load(shadowCopyForLoad(libPath, loadedMtime));
        loaded = true;
        writePidLock(libPath);
        process.on('exit', removePidLock);
        verifyVersion(libPath);
        return true;
    } catch (e: any) {
        console.log(`[agent-doc/native] failed to load ${libPath}: ${e.message}`);
        return false;
    }
}

/** Proves the ESM extension can resolve its packaged native dependency. */
export function koffiModuleAvailable(): boolean {
    try {
        requireFromExtension.resolve('koffi');
        return true;
    } catch {
        return false;
    }
}

/**
 * Force a fresh `koffi.load` for the typed `reload_library` intent, bypassing
 * the lazy mtime-equality guard in [ensureLoaded]. Keeps the previous bindings
 * on failure (the reason is logged, never swallowed silently).
 */
export function forceReloadLib(projectRoot?: string): boolean {
    if (!ensureLoaded(projectRoot)) return false;
    const target = loadedPath;
    if (!target || !koffi) return false;
    try {
        removePidLock();
        resetBindings();
        const currentMtime = fs.statSync(target).mtimeMs;
        lib = koffi.load(shadowCopyForLoad(target, currentMtime));
        loadedMtime = currentMtime;
        writePidLock(target);
        verifyVersion(target);
        console.log(`[agent-doc/native] forceReload: reloaded from ${target}`);
        return true;
    } catch (e: any) {
        console.log(`[agent-doc/native] forceReload failed, keeping previous: ${e.message}`);
        return false;
    }
}

// Lazy function bindings (resolved on first call)
let _reposition_boundary_to_end: any = null;
let _reposition_boundary_to_end_with_id: any = null;
let _reposition_boundary_to_end_preserve_head: any = null;
let _reposition_boundary_to_end_preserve_head_with_id: any = null;
let _normalize_template_structure: any = null;
let _apply_node_patches: any = null;
let _admin_inspect_json: any = null;
let _tmux_focus_state_json: any = null;
let _focus_document_pane_json: any = null;
let _sync_tmux_layout_json: any = null;
let _editor_surface_observe_json: any = null;
let _editor_surface_forget: any = null;
let _admin_queue_control_json: any = null;
let _admin_reap_json: any = null;
let _admin_handoff_json: any = null;
let _admin_repair_projection_json: any = null;
let _visual_tokens_json: any = null;
let _lazily_current_observed_v1: any = null;
let _document_closed_for_editor: any = null;
let _deferred_write_reconnect_content: any = null;
let _deferred_write_reconnect_propagated: any = null;
let _resolve_project_path: any = null;
let _document_id_for_path: any = null;
let _is_session_document: any = null;
let _reliable_sync_liveness_enqueue: any = null;
let _reliable_sync_liveness_flush: any = null;
let _reliable_sync_document_op_push: any = null;
let _reliable_sync_text_adopt_push: any = null;
let _reliable_sync_document_op_flush: any = null;
let _peer_replicas_missing: any = null;
let _free_state: any = null;
let _free_string: any = null;
let _version: any = null;
let _state_projection: any = null;
let _turn_projection: any = null;
let _state_subscribe: any = null;
let _record_state_event: any = null;
let _editor_content_applied_for_editor_v1: any = null;
let _editor_patch_applied: any = null;
let _editor_patch_rejected: any = null;
let _record_editor_surface_event: any = null;
let _record_editor_op: any = null;
let _document_base_hash: any = null;
let _replica_open: any = null;
let _replica_apply_local: any = null;
let _replica_apply_update: any = null;
let _replica_state_vector: any = null;
let _replica_diff: any = null;
let _replica_encode_state: any = null;
let _replica_text: any = null;
let _replica_close: any = null;

function bindFunctions(): void {
    if (_reposition_boundary_to_end && _normalize_template_structure) return;

    // Define the FfiPatchResult struct
    const FfiPatchResultType = koffi.struct('FfiPatchResult', {
        text: 'char*',
        error: 'char*',
    });

    const FfiJsonResultType = koffi.struct('FfiJsonResult', {
        json: 'char*',
        error: 'char*',
    });

    const FfiProjectPathType = koffi.struct('FfiProjectPath', {
        project_root: OWNED_C_STRING_POINTER,
        relative_path: OWNED_C_STRING_POINTER,
    });

    _reposition_boundary_to_end = lib.func('agent_doc_reposition_boundary_to_end', FfiPatchResultType, ['str']);
    _reposition_boundary_to_end_with_id = lib.func(
        'agent_doc_reposition_boundary_to_end_with_id',
        FfiPatchResultType,
        ['str', 'str'],
    );
    _reposition_boundary_to_end_preserve_head = lib.func(
        'agent_doc_reposition_boundary_to_end_preserve_head',
        FfiPatchResultType,
        ['str'],
    );
    _reposition_boundary_to_end_preserve_head_with_id = lib.func(
        'agent_doc_reposition_boundary_to_end_preserve_head_with_id',
        FfiPatchResultType,
        ['str', 'str'],
    );
    _normalize_template_structure = lib.func('agent_doc_normalize_template_structure', FfiPatchResultType, ['str']);
    try {
        _apply_node_patches = lib.func('agent_doc_apply_node_patches', FfiPatchResultType, ['str', 'str']);
    } catch (e: any) {
        console.log(`[agent-doc/native] apply_node_patches unavailable: ${e.message}`);
        _apply_node_patches = null;
    }
    try {
        _admin_inspect_json = lib.func('agent_doc_admin_inspect_json', FfiJsonResultType, ['str', 'str', 'str', 'str']);
        _tmux_focus_state_json = lib.func('agent_doc_tmux_focus_state_json', FfiJsonResultType, ['str']);
        _focus_document_pane_json = lib.func(
            'agent_doc_focus_document_pane_json',
            FfiJsonResultType,
            ['str', 'str'],
        );
        _sync_tmux_layout_json = lib.func(
            'agent_doc_sync_tmux_layout_json',
            FfiJsonResultType,
            ['str', 'str', 'str', 'str', 'int', 'int'],
        );
        _editor_surface_observe_json = lib.func(
            'agent_doc_editor_surface_observe_json',
            FfiJsonResultType,
            ['str', 'str'],
        );
        _editor_surface_forget = lib.func('agent_doc_editor_surface_forget', 'int', ['str']);
        _admin_queue_control_json = lib.func(
            'agent_doc_admin_queue_control_json',
            FfiJsonResultType,
            ['str', 'str', 'str', 'int64', 'str', 'str'],
        );
        _admin_reap_json = lib.func(
            'agent_doc_admin_reap_json',
            FfiJsonResultType,
            ['str', 'str', 'str', 'str', 'int64', 'str'],
        );
        _admin_handoff_json = lib.func(
            'agent_doc_admin_handoff_json',
            FfiJsonResultType,
            ['str*', 'str', 'str', 'int64', 'str'],
        );
        _admin_repair_projection_json = lib.func(
            'agent_doc_admin_repair_projection_json',
            FfiJsonResultType,
            ['str', 'str', 'str', 'int64', 'str'],
        );
    } catch (e: any) {
        console.log(`[agent-doc/native] admin controller wrappers unavailable: ${e.message}`);
        _admin_inspect_json = null;
        _tmux_focus_state_json = null;
        _focus_document_pane_json = null;
        _sync_tmux_layout_json = null;
        _editor_surface_observe_json = null;
        _editor_surface_forget = null;
        _admin_queue_control_json = null;
        _admin_reap_json = null;
        _admin_handoff_json = null;
        _admin_repair_projection_json = null;
    }
    _visual_tokens_json = lib.func('agent_doc_visual_tokens_json', OWNED_C_STRING_POINTER, ['str']);
    try {
        _document_id_for_path = lib.func(
            'agent_doc_document_id_for_path',
            OWNED_C_STRING_POINTER,
            ['str'],
        );
        _is_session_document = lib.func('agent_doc_is_session_document', 'int', ['str']);
        _reliable_sync_liveness_enqueue = lib.func(
            'agent_doc_reliable_sync_liveness_enqueue',
            'int',
            ['str', 'str', 'str'],
        );
        _reliable_sync_liveness_flush = lib.func(
            'agent_doc_reliable_sync_liveness_flush',
            'int64',
            ['str', 'str'],
        );
    } catch (e: any) {
        console.log(`[agent-doc/native] reliable-sync liveness wrappers unavailable: ${e.message}`);
        _document_id_for_path = null;
        _is_session_document = null;
        _reliable_sync_liveness_enqueue = null;
        _reliable_sync_liveness_flush = null;
    }
    try {
        _reliable_sync_document_op_push = lib.func(
            'agent_doc_reliable_sync_document_op_push',
            'int',
            ['str', 'str', 'str'],
        );
        _reliable_sync_text_adopt_push = lib.func(
            'agent_doc_reliable_sync_text_adopt_push',
            'int',
            ['str', 'str', 'str'],
        );
        _reliable_sync_document_op_flush = lib.func(
            'agent_doc_reliable_sync_document_op_flush',
            'int',
            ['str', 'str'],
        );
    } catch (e: any) {
        console.log(`[agent-doc/native] reliable-sync document wrappers unavailable: ${e.message}`);
        _reliable_sync_document_op_push = null;
        _reliable_sync_text_adopt_push = null;
        _reliable_sync_document_op_flush = null;
    }
    try {
        _peer_replicas_missing = lib.func(
            'agent_doc_peer_replicas_missing',
            'char*',
            ['str', 'uint64', 'str'],
        );
    } catch (e: any) {
        // An older cdylib without the export. The controller's Tier 1 fan-out still
        // covers this extension, so the caller falls back rather than being stranded.
        console.log(`[agent-doc/native] peer replica pull unavailable: ${e.message}`);
        _peer_replicas_missing = null;
    }
    try {
        _document_closed_for_editor = lib.func(
            'agent_doc_document_closed_for_editor',
            'void',
            ['str', 'str'],
        );
    } catch (e: any) {
        console.log(`[agent-doc/native] per-editor live-buffer ABI unavailable: ${e.message}`);
        _document_closed_for_editor = null;
    }
    try {
        // #falsetyping-guard: v3 adds the replica-churn provenance flag
        // (no_unsaved_operator_edits) as a trailing int.
        _lazily_current_observed_v1 = lib.func(
            'agent_doc_lazily_current_observed_v1',
            'void',
            ['str', 'str', 'str', 'str', 'str', 'str', 'int'],
        );
    } catch (e: any) {
        console.log(`[agent-doc/native] live-buffer provenance ABI unavailable: ${e.message}`);
        _lazily_current_observed_v1 = null;
    }
    try {
        _deferred_write_reconnect_content = lib.func(
            'agent_doc_deferred_write_reconnect_content',
            'char*',
            ['str', 'str'],
        );
        _deferred_write_reconnect_propagated = lib.func(
            'agent_doc_deferred_write_reconnect_propagated',
            'int32',
            ['str', 'str'],
        );
    } catch (e: any) {
        console.log(`[agent-doc/native] deferred reconnect ABI unavailable: ${e.message}`);
        _deferred_write_reconnect_content = null;
        _deferred_write_reconnect_propagated = null;
    }
    _resolve_project_path = lib.func('agent_doc_resolve_project_path', FfiProjectPathType, ['str']);
    _free_state = lib.func('agent_doc_free_state', 'void', ['void*', 'size_t']);
    // These Rust functions return owned C allocations. Bind them as opaque
    // pointers so Koffi does not eagerly convert `char *` to a JS string before
    // the wrapper can decode and release the original allocation.
    _free_string = lib.func('agent_doc_free_string', 'void', [OWNED_C_STRING_POINTER]);
    _version = lib.func('agent_doc_version', OWNED_C_STRING_POINTER, []);
    try {
        _state_projection = lib.func(
            'agent_doc_state_projection',
            OWNED_C_STRING_POINTER,
            ['str'],
        );
        _record_state_event = lib.func('agent_doc_record_state_event', 'int32', ['str', 'str']);
        _editor_content_applied_for_editor_v1 = lib.func(
            'agent_doc_editor_content_applied_for_editor_v1',
            'int32',
            ['str', 'str', 'str', 'str', 'str', 'str', 'str', 'str'],
        );
        _editor_patch_applied = lib.func('agent_doc_editor_patch_applied', 'int32', ['str', 'str', 'uint64']);
        _editor_patch_rejected = lib.func('agent_doc_editor_patch_rejected', 'int32', ['str', 'str', 'uint64', 'str']);
    } catch (e: any) {
        console.log(`[agent-doc/native] state projection ABI unavailable: ${e.message}`);
        _state_projection = null;
        _record_state_event = null;
        _editor_content_applied_for_editor_v1 = null;
        _editor_patch_applied = null;
        _editor_patch_rejected = null;
    }
    try {
        _record_editor_surface_event = lib.func(
            'agent_doc_record_editor_surface_event',
            'int32',
            ['str', 'str', 'str', 'str', 'str', 'str', 'str', 'str'],
        );
    } catch (e: any) {
        console.log(`[agent-doc/native] editor-surface event ABI unavailable: ${e.message}`);
        _record_editor_surface_event = null;
    }
    try {
        // Project Controller→plugin turn-state projection (Shared Foundation parity with the JB
        // plugin). Optional so an older cdylib without the symbol does not break
        // the rest of the bindings.
        _turn_projection = lib.func(
            'agent_doc_turn_projection',
            OWNED_C_STRING_POINTER,
            ['str'],
        );
    } catch (e: any) {
        console.log(`[agent-doc/native] turn projection ABI unavailable: ${e.message}`);
        _turn_projection = null;
    }
    try {
        // #r5at lazily-js reactive mirror: warm subscribe (snapshot/delta) over
        // the FFI state backbone. Optional so an older cdylib without the symbol
        // does not break the rest of the bindings.
        _state_subscribe = lib.func(
            'agent_doc_state_subscribe',
            OWNED_C_STRING_POINTER,
            ['str', 'uint64'],
        );
    } catch (e: any) {
        console.log(`[agent-doc/native] state subscribe ABI unavailable: ${e.message}`);
        _state_subscribe = null;
    }
    try {
        // #qnodemerge4wire Phase 4 editor-op reporters. Optional so an older
        // cdylib without the symbols does not break the rest of the bindings.
        _record_editor_op = lib.func(
            'agent_doc_record_editor_op',
            'int32',
            ['str', 'str', 'str', 'int64', 'str', 'int64'],
        );
        _document_base_hash = lib.func(
            'agent_doc_document_base_hash',
            OWNED_C_STRING_POINTER,
            ['str'],
        );
    } catch (e: any) {
        console.log(`[agent-doc/native] editor-op capture ABI unavailable: ${e.message}`);
        _record_editor_op = null;
        _document_base_hash = null;
    }
    try {
        // #crdtauth5 editor-as-replica node. Optional so an older cdylib keeps
        // the extension on the existing patch-file path.
        _replica_open = lib.func('agent_doc_replica_open', 'int32', ['uint64', 'void*', 'size_t']);
        _replica_apply_local = lib.func('agent_doc_replica_apply_local', 'int32', [
            'uint64',
            'uint32',
            'uint32',
            'str',
        ]);
        _replica_apply_update = lib.func('agent_doc_replica_apply_update', 'int32', [
            'uint64',
            'void*',
            'size_t',
        ]);
        _replica_state_vector = lib.func(
            'agent_doc_replica_state_vector',
            'void*',
            ['uint64', koffi.out(koffi.pointer('size_t'))],
        );
        _replica_diff = lib.func(
            'agent_doc_replica_diff',
            'void*',
            ['uint64', 'void*', 'size_t', koffi.out(koffi.pointer('size_t'))],
        );
        _replica_encode_state = lib.func(
            'agent_doc_replica_encode_state',
            'void*',
            ['uint64', koffi.out(koffi.pointer('size_t'))],
        );
        _replica_text = lib.func(
            'agent_doc_replica_text',
            OWNED_C_STRING_POINTER,
            ['uint64'],
        );
        _replica_close = lib.func('agent_doc_replica_close', 'int32', ['uint64']);
    } catch (e: any) {
        console.log(`[agent-doc/native] replica ABI unavailable: ${e.message}`);
        _replica_open = null;
        _replica_apply_local = null;
        _replica_apply_update = null;
        _replica_state_vector = null;
        _replica_diff = null;
        _replica_encode_state = null;
        _replica_text = null;
        _replica_close = null;
    }
}

function verifyVersion(libPath: string): void {
    try {
        bindFunctions();
        const ptr = _version();
        if (ptr) {
            const version = koffi.decode(ptr, 'char', -1);
            _free_string(ptr);
            console.log(`[agent-doc/native] loaded libagent_doc v${version} from ${libPath}`);
        } else {
            console.log(`[agent-doc/native] agent_doc_version() returned null — possible ABI mismatch at ${libPath}`);
        }
    } catch (e: any) {
        console.log(`[agent-doc/native] agent_doc_version() failed — ABI mismatch at ${libPath}: ${e.message}`);
    }
}

/**
 * #qnodemerge4wire Phase 4: convert a VS Code UTF-16 change range (offset+length
 * in the OLD document text) to the UTF-8 BYTE units the EditorOp capture expects.
 * Pure + exported for unit testing the non-ASCII offset semantics. `byteOffset`
 * is the UTF-8 length of the prefix before the change; `deleteBytes` is the UTF-8
 * length of the replaced span.
 */
export function utf16RangeToUtf8Bytes(
    oldText: string,
    rangeOffset: number,
    rangeLength: number,
): { byteOffset: number; deleteBytes: number } {
    const byteOffset = Buffer.byteLength(oldText.slice(0, rangeOffset), 'utf-8');
    const deleteBytes = Buffer.byteLength(
        oldText.slice(rangeOffset, rangeOffset + rangeLength),
        'utf-8',
    );
    return { byteOffset, deleteBytes };
}

/**
 * #qnodemerge4wire Phase 4: resolve the base hash captured editor ops must be
 * stamped with so the write-time merge accepts them (null when unavailable →
 * the reporter skips capture and the merge falls back to the diff-guess).
 */
export function documentBaseHash(filePath: string, projectRoot?: string): string | null {
    if (!ensureLoaded(projectRoot)) return null;
    bindFunctions();
    if (!_document_base_hash) return null;
    let ptr: any = null;
    try {
        ptr = _document_base_hash(filePath);
        if (!ptr) return null;
        return koffi.decode(ptr, 'char', -1);
    } catch (e: any) {
        console.warn(`[agent-doc/native] document_base_hash error: ${e.message}`);
        return null;
    } finally {
        if (ptr) _free_string(ptr);
    }
}

/**
 * #qnodemerge4wire Phase 4: record one real editor op for CRDT-based op replay.
 * `offset`/`deleteLen` are UTF-8 BYTE units (the caller converts from VS Code's
 * UTF-16 offsets). `opKind` is `'insert'` (with `insertText`, `deleteLen=0`) or
 * `'delete'` (with `insertText=null`, `deleteLen=byteLen`). Returns true when the
 * op was durably recorded.
 */
export function recordEditorOp(
    filePath: string,
    baseHash: string,
    opKind: 'insert' | 'delete',
    offset: number,
    insertText: string | null,
    deleteLen: number,
    projectRoot?: string,
): boolean {
    if (!ensureLoaded(projectRoot)) return false;
    bindFunctions();
    if (!_record_editor_op) return false;
    try {
        return _record_editor_op(filePath, baseHash, opKind, offset, insertText ?? '', deleteLen) === 1;
    } catch (e: any) {
        console.warn(`[agent-doc/native] record_editor_op error: ${e.message}`);
        return false;
    }
}

function copyStateBuffer(ptr: any, len: number): Uint8Array | null {
    if (!ptr) return null;
    try {
        if (len <= 0) return Buffer.alloc(0);
        // koffi.view is borrowed native memory. Buffer.from(ArrayBuffer) keeps
        // sharing that allocation, so returning it after agent_doc_free_state
        // produces a use-after-free and corrupts replica updates in flight.
        // Copy through the borrowed Buffer while the allocation is still live.
        const borrowed = Buffer.from(koffi.view(ptr, len));
        return Buffer.from(borrowed);
    } finally {
        if (_free_state) _free_state(ptr, len);
    }
}

export class NativeReplicaNode {
    private clientId: number | null = null;

    constructor(private readonly projectRoot?: string) {}

    open(clientId: number, initState?: Uint8Array | null): boolean {
        if (!ensureLoaded(this.projectRoot)) return false;
        bindFunctions();
        if (!_replica_open) return false;
        const init = initState && initState.length > 0 ? Buffer.from(initState) : null;
        try {
            const ok = _replica_open(clientId, init, init?.length ?? 0) === 0;
            if (ok) this.clientId = clientId;
            return ok;
        } catch (e: any) {
            console.warn(`[agent-doc/native] replica_open error: ${e.message}`);
            return false;
        }
    }

    applyLocal(clientId: number, offset: number, deleteLen: number, insert: string): boolean {
        if (!ensureLoaded(this.projectRoot)) return false;
        bindFunctions();
        if (!_replica_apply_local) return false;
        try {
            return _replica_apply_local(clientId, offset, deleteLen, insert) === 0;
        } catch (e: any) {
            console.warn(`[agent-doc/native] replica_apply_local error: ${e.message}`);
            return false;
        }
    }

    applyUpdate(clientId: number, update: Uint8Array): boolean {
        if (update.length === 0) return true;
        if (!ensureLoaded(this.projectRoot)) return false;
        bindFunctions();
        if (!_replica_apply_update) return false;
        try {
            const bytes = Buffer.from(update);
            return _replica_apply_update(clientId, bytes, bytes.length) === 0;
        } catch (e: any) {
            console.warn(`[agent-doc/native] replica_apply_update error: ${e.message}`);
            return false;
        }
    }

    stateVector(): Uint8Array | null {
        if (this.clientId == null) return null;
        if (!ensureLoaded(this.projectRoot)) return null;
        bindFunctions();
        if (!_replica_state_vector) return null;
        const lenOut = [0];
        try {
            const ptr = _replica_state_vector(this.clientId, lenOut);
            return copyStateBuffer(ptr, Number(lenOut[0] ?? 0));
        } catch (e: any) {
            console.warn(`[agent-doc/native] replica_state_vector error: ${e.message}`);
            return null;
        }
    }

    diff(theirStateVector: Uint8Array): Uint8Array | null {
        if (this.clientId == null) return null;
        if (!ensureLoaded(this.projectRoot)) return null;
        bindFunctions();
        if (!_replica_diff) return null;
        const lenOut = [0];
        const frontier = Buffer.from(theirStateVector);
        try {
            const ptr = _replica_diff(this.clientId, frontier, frontier.length, lenOut);
            return copyStateBuffer(ptr, Number(lenOut[0] ?? 0));
        } catch (e: any) {
            console.warn(`[agent-doc/native] replica_diff error: ${e.message}`);
            return null;
        }
    }

    encodeState(): Uint8Array | null {
        if (this.clientId == null) return null;
        if (!ensureLoaded(this.projectRoot)) return null;
        bindFunctions();
        if (!_replica_encode_state) return null;
        const lenOut = [0];
        try {
            const ptr = _replica_encode_state(this.clientId, lenOut);
            return copyStateBuffer(ptr, Number(lenOut[0] ?? 0));
        } catch (e: any) {
            console.warn(`[agent-doc/native] replica_encode_state error: ${e.message}`);
            return null;
        }
    }

    text(): string | null {
        if (this.clientId == null) return null;
        if (!ensureLoaded(this.projectRoot)) return null;
        bindFunctions();
        if (!_replica_text) return null;
        let ptr: any = null;
        try {
            ptr = _replica_text(this.clientId);
            if (!ptr) return null;
            return koffi.decode(ptr, 'char', -1);
        } catch (e: any) {
            console.warn(`[agent-doc/native] replica_text error: ${e.message}`);
            return null;
        } finally {
            if (ptr) _free_string(ptr);
        }
    }

    close(clientId?: number): void {
        const id = clientId ?? this.clientId;
        if (id == null) return;
        this.clientId = null;
        if (!ensureLoaded(this.projectRoot)) return;
        bindFunctions();
        if (!_replica_close) return;
        try {
            _replica_close(id);
        } catch (e: any) {
            console.warn(`[agent-doc/native] replica_close error: ${e.message}`);
        }
    }
}

export function documentHash(filePath: string): string {
    let canonical: string;
    try {
        canonical = fs.realpathSync(filePath);
  } catch {
    canonical = path.resolve(filePath);
  }
  return crypto.createHash('sha256').update(canonical, 'utf-8').digest('hex');
}

// #lzpkgwire: this VS Code extension is compiled as CommonJS, while
// @lazily/js is an ESM package. The extension keeps this plugin-local bridge as
// canonical for runtime packaging, and native.test.ts pins these pure helpers
// against @lazily/js so the duplicate adapter cannot silently drift.
export function buildStateEvent(
  documentHashValue: string,
  type: string,
  fields: Record<string, unknown>,
  eventSuffix: string,
): StateBackboneEvent {
    return {
        event_id: `${documentHashValue}:${eventSuffix}`,
        fact: {
            type,
            document_hash: documentHashValue,
            ...fields,
        },
    };
}

export function projectionSummary(projection: any): ProjectionSummary | null {
    if (!projection || typeof projection !== 'object') return null;
    const route = projection.route ?? {};
    const transport = projection.transport ?? {};
    const proof = projection.proof ?? {};
    const patches = transport.patches && typeof transport.patches === 'object'
        ? Object.entries(transport.patches as Record<string, any>)
        : [];
    const sortedPatches = patches.sort(([a], [b]) => a.localeCompare(b));
    const latest = sortedPatches.length > 0 ? sortedPatches[sortedPatches.length - 1] : undefined;
    return {
        routeReadiness: typeof route.readiness === 'string' ? route.readiness : undefined,
        routePaneId: typeof route.pane_id === 'string' ? route.pane_id : undefined,
        latestTransportPatchId: latest?.[0],
        latestTransportPhase: typeof latest?.[1]?.phase === 'string' ? latest[1].phase : undefined,
        proofMarkers: proof.markers && typeof proof.markers === 'object'
            ? Object.keys(proof.markers).length
            : 0,
    };
}

export function compactProjectionSummary(summary: ProjectionSummary): string {
    return `route=${summary.routeReadiness ?? 'unknown'} pane=${summary.routePaneId ?? '-'} `
        + `transport=${summary.latestTransportPatchId ?? '-'}:${summary.latestTransportPhase ?? '-'} `
        + `proof_markers=${summary.proofMarkers}`;
}

export function stateProjection(documentHashValue: string, projectRoot?: string): any | null {
    if (!ensureLoaded(projectRoot)) return null;
    bindFunctions();
    if (!_state_projection) return null;
    let ptr: any = null;
    try {
        ptr = _state_projection(documentHashValue);
        if (!ptr) return null;
        const raw = koffi.decode(ptr, 'char', -1);
        if (!raw || raw === 'null') return null;
        return JSON.parse(raw);
    } catch (e: any) {
        console.warn(`[agent-doc/native] state_projection error: ${e.message}`);
        return null;
    } finally {
        if (ptr) _free_string(ptr);
    }
}

export function stateProjectionForFile(filePath: string, projectRoot?: string): any | null {
    return stateProjection(documentHash(filePath), projectRoot);
}
/**
 * Project Controller→plugin turn-state projection for a document path:
 * `{state, turn_in_flight, transition_authority, realtime_steering?}`. Observe it
 * to render turn-in-flight UI, project realtime steering onto the banner/status
 * label, and decide whether a forwarded operator prompt starts a fresh turn or
 * would collide with an in-flight response (the double-append guard). Returns
 * null when the ABI is unavailable or on error — callers treat null as
 * "idle / unknown". Parity with the JB `agent_doc_turn_projection`.
 */
export function turnProjectionForFile(filePath: string, projectRoot?: string): any | null {
    if (!ensureLoaded(projectRoot)) return null;
    bindFunctions();
    if (!_turn_projection) return null;
    let ptr: any = null;
    try {
        ptr = _turn_projection(filePath);
        if (!ptr) return null;
        const raw = koffi.decode(ptr, 'char', -1);
        if (!raw || raw === 'null') return null;
        return JSON.parse(raw);
    } catch (e: any) {
        console.warn(`[agent-doc/native] turn_projection error: ${e.message}`);
        return null;
    } finally {
        if (ptr) _free_string(ptr);
    }
}

// #lzsync 3B generic materialized view — the VS Code counterpart of the JB plugin's
// StateProjectionBridge per-document GraphView. Keyed by documentHash (canonical-path
// SHA-256); re-subscription lazily re-creates the view from a fresh cold snapshot, so
// aggressive eviction is safe.
const stateMirrors = new Map<string, GraphView>();

/**
 * Pull a raw `agent_doc_state_subscribe(documentHash, lastEpoch)` message
 * (snapshot when lastEpoch==0 / uninitialized, delta thereafter). Returns the
 * JSON string, or null when the FFI/symbol is unavailable or no state exists.
 * Pure FFI wrapper — exported for diagnostics; mirror callers use
 * {@link subscribeMirrorForFile}.
 */
export function stateSubscribe(
    documentHashValue: string,
    lastEpoch: number,
    projectRoot?: string,
): string | null {
    if (!ensureLoaded(projectRoot)) return null;
    bindFunctions();
    if (!_state_subscribe) return null;
    let ptr: any = null;
    try {
        ptr = _state_subscribe(documentHashValue, lastEpoch);
        if (!ptr) return null;
        const raw = koffi.decode(ptr, 'char', -1);
        if (!raw || raw === 'null' || raw === '') return null;
        return raw;
    } catch (e: any) {
        console.warn(`[agent-doc/native] state_subscribe error: ${e.message}`);
        return null;
    } finally {
        if (ptr) _free_string(ptr);
    }
}

/**
 * `#ctrlkillreregister` Tier 3 — which of THIS extension's registrations the
 * controller currently holds no replica for.
 *
 * Killing the controller strands a live editor: hydration restores the durable
 * liveness plane so the editor still reads as registered, but the relay hub holding
 * its replica is process-local, died with the old controller, and nothing rehydrates
 * it. The controller used to push a rebuild request at each survivor — and a push has
 * to *reach* its endpoint, the failure behind `reload-lib reached 1/4 endpoints`.
 *
 * The pull inverts it: the extension is the only process that can create its own
 * replica, so it asks about itself and repairs. There is no endpoint to fail to
 * reach, because the asking process is provably alive.
 *
 * `heldDocumentHashes` is what the caller already has a replica for. Returns the raw
 * JSON array of `EditorRegistration` objects, or null when the question could not be
 * asked at all (ABI missing, controller unreachable) — which is deliberately distinct
 * from `'[]'` ("asked, nothing to do"). Parity with the JB `PeerReplicaPull`.
 */
export function peerReplicasMissing(
    projectRoot: string,
    pid: number,
    heldDocumentHashes: readonly string[],
): string | null {
    if (!ensureLoaded(projectRoot)) return null;
    bindFunctions();
    if (!_peer_replicas_missing) return null;
    let ptr: any = null;
    try {
        ptr = _peer_replicas_missing(projectRoot, pid, JSON.stringify([...heldDocumentHashes]));
        if (!ptr) return null;
        const raw = koffi.decode(ptr, 'char', -1);
        if (!raw || raw === 'null' || raw === '') return null;
        return raw;
    } catch (e: any) {
        // Never swallow: an editor that cannot ask is one that stays stranded.
        console.warn(`[agent-doc/native] peer_replicas_missing error: ${e.message}`);
        return null;
    } finally {
        if (ptr) _free_string(ptr);
    }
}

/** Whether the loaded cdylib exposes the reactive subscribe FFI (#r5at). */
export function hasStateSubscribe(projectRoot?: string): boolean {
    if (!ensureLoaded(projectRoot)) return false;
    bindFunctions();
    return Boolean(_state_subscribe);
}

/**
 * Advance the per-document mirror by absorbing any FFI snapshot/delta since its
 * current epoch (`#r5at`). First call (uninitialized mirror) requests a cold
 * snapshot; subsequent calls request a delta from the mirror's current epoch.
 * Returns the applied message type (`"snapshot"`/`"delta"`) or null when FFI is
 * unavailable / no state yet.
 */
export function subscribeMirrorForFile(filePath: string, projectRoot?: string): string | null {
    const docHash = documentHash(filePath);
    let view = stateMirrors.get(docHash);
    if (!view) {
        view = new GraphView();
        stateMirrors.set(docHash, view);
    }
    const lastEpoch = view.isInitialized ? view.epoch : 0;
    const raw = stateSubscribe(docHash, lastEpoch, projectRoot);
    if (!raw) return null;
    return applyIpcMessageToView(view, raw);
}

/**
 * Reactive summary derived from the per-document mirror's tracked cells
 * (`#r5at`). Call after {@link subscribeMirrorForFile}. Returns null when the
 * mirror has not been initialized yet.
 */
export function mirrorSummaryForFile(filePath: string): AgentDocProjection | null {
    const view = stateMirrors.get(documentHash(filePath));
    if (!view || !view.isInitialized) return null;
    return agentDocProjectionFromView(view);
}

/** The current view epoch for [filePath], or null if never initialized (#lzsync 3B). */
export function mirrorEpochForFile(filePath: string): number | null {
    const view = stateMirrors.get(documentHash(filePath));
    return view && view.isInitialized ? view.epoch : null;
}

/**
 * Reactive read for consumers (`#r5at`, the VS Code analog of the JB
 * `reactiveSummaryForFile`): advance the per-document mirror by absorbing FFI
 * deltas since its current epoch, then derive the summary from tracked cells.
 *
 * Drop-in replacement for the cold {@link stateProjectionForFile} +
 * {@link projectionSummary} pull on the read path:
 *  - When the view never initializes (FFI unavailable / no recorded state yet),
 *    fall back to the cold pull so a cold-start read still surfaces a projection.
 *
 * Returns null only when both the reactive view and the cold pull are empty.
 */
export function reactiveSummaryForFile(
    filePath: string,
    projectRoot?: string,
): AgentDocProjection | null {
    subscribeMirrorForFile(filePath, projectRoot);
    const view = mirrorSummaryForFile(filePath);
    if (view) return view;
    // Cold-start fallback: the view never initialized (no FFI / no state yet).
    const cold = stateProjectionForFile(filePath, projectRoot);
    const summary = cold ? projectionSummary(cold) : null;
    if (!summary) return null;
    return {
        routeReadiness: summary.routeReadiness ?? null,
        routePaneId: summary.routePaneId ?? null,
        latestTransportPhase: summary.latestTransportPhase ?? null,
        proofMarkers: summary.proofMarkers,
    };
}

/**
 * Evict the per-document view for [filePath] (`#lzsync` 3B, the VS Code analog of
 * the JB `evictForFile`). Called when the editor tab/document closes so a reused
 * path (move/symlink/reopen) does not surface the prior document's stale state.
 */
export function evictStateMirrorForFile(filePath: string): void {
    stateMirrors.delete(documentHash(filePath));
}

/** Test-only: number of live per-document views (eviction coverage). */
export function debugStateMirrorCount(): number {
    return stateMirrors.size;
}

/**
 * Test-only seam (`#lzsync` 3B): seed the per-document view by applying a native
 * lazily snapshot/delta message directly, bypassing the FFI subscribe call. Lets
 * consumer-observation tests assert the read path derives the projection from the
 * folded view rather than the cold pull (FFI is unavailable in plugin unit tests).
 * Returns whether the message applied.
 */
export function seedStateMirrorMessageForTest(filePath: string, message: string): boolean {
    const docHash = documentHash(filePath);
    let view = stateMirrors.get(docHash);
    if (!view) {
        view = new GraphView();
        stateMirrors.set(docHash, view);
    }
    return applyIpcMessageToView(view, message) !== null;
}

export function recordStateEvent(
    documentHashValue: string,
    event: StateBackboneEvent,
    projectRoot?: string,
): boolean {
    if (!ensureLoaded(projectRoot)) return false;
    bindFunctions();
    if (!_record_state_event) return false;
    try {
        return _record_state_event(documentHashValue, JSON.stringify(event)) === 1;
    } catch (e: any) {
        console.warn(`[agent-doc/native] record_state_event error: ${e.message}`);
        return false;
    }
}

function nextStateGeneration(filePath: string, owner: string): number {
    const key = `${documentHash(filePath)}:${owner}`;
    const next = (stateGenerations.get(key) ?? 0) + 1;
    stateGenerations.set(key, next);
    return next;
}

function recordFactForFile(
    filePath: string,
    type: string,
    fields: Record<string, unknown>,
    eventSuffix: string,
    projectRoot?: string,
): boolean {
    const hash = documentHash(filePath);
    return recordStateEvent(hash, buildStateEvent(hash, type, fields, eventSuffix), projectRoot);
}

function recordOwnerGeneration(filePath: string, owner: string, generation: number, projectRoot?: string): void {
    recordFactForFile(
        filePath,
        'owner_generation_changed',
        { owner, generation },
        `owner-${owner}-${generation}`,
        projectRoot,
    );
}

export function recordEditorPatchQueued(filePath: string, patchId?: string, projectRoot?: string): number | null {
    if (!patchId) return null;
    const generation = nextStateGeneration(filePath, 'editor_ipc_bridge');
    recordOwnerGeneration(filePath, 'editor_ipc_bridge', generation, projectRoot);
    recordFactForFile(
        filePath,
        'editor_patch_queued',
        { patch_id: patchId, actor_generation: generation },
        `editor-patch-queued-${patchId}-${generation}`,
        projectRoot,
    );
    return generation;
}

export function recordEditorPatchApplied(
    filePath: string,
    patchId: string | undefined,
    generation: number | null,
    projectRoot?: string,
): void {
    if (!patchId || generation == null) return;
    if (!ensureLoaded(projectRoot)) return;
    bindFunctions();
    if (!_editor_patch_applied) {
        console.warn('[agent-doc/native] incompatible agent-doc native library: missing agent_doc_editor_patch_applied; reinstall the plugin/native library');
        return;
    }
    try {
        const ok = _editor_patch_applied(filePath, patchId, generation) === 1;
        if (!ok) {
            console.warn(`[agent-doc/native] editor_patch_applied receipt rejected for ${patchId}`);
        }
    } catch (e: any) {
        console.warn(`[agent-doc/native] editor_patch_applied ABI error: ${e.message}`);
    }
}

export function recordEditorPatchRejected(
    filePath: string,
    patchId: string | undefined,
    generation: number | null,
    reason: string,
    projectRoot?: string,
): void {
    if (!patchId || generation == null) return;
    if (!ensureLoaded(projectRoot)) return;
    bindFunctions();
    if (!_editor_patch_rejected) {
        console.warn('[agent-doc/native] incompatible agent-doc native library: missing agent_doc_editor_patch_rejected; reinstall the plugin/native library');
        return;
    }
    try {
        const ok = _editor_patch_rejected(filePath, patchId, generation, reason) === 1;
        if (!ok) {
            console.warn(`[agent-doc/native] editor_patch_rejected receipt rejected for ${patchId}`);
        }
    } catch (e: any) {
        console.warn(`[agent-doc/native] editor_patch_rejected ABI error: ${e.message}`);
    }
}

export function recordEditorContentApplied(
    projectRoot: string | undefined,
    patchId: string | undefined,
    filePath: string,
    content: string,
    editorId: string,
): boolean {
    if (!patchId) return true;
    if (!ensureLoaded(projectRoot)) return false;
    bindFunctions();
    if (!_editor_content_applied_for_editor_v1) {
        console.warn('[agent-doc/native] incompatible agent-doc native library: missing agent_doc_editor_content_applied_for_editor_v1; reinstall the plugin/native library');
        return false;
    }
    if (!projectRoot) {
        console.warn('[agent-doc/native] project root is required for editor content receipts');
        return false;
    }
    try {
        const ok = _editor_content_applied_for_editor_v1(
            projectRoot,
            patchId,
            filePath,
            content,
            editorId,
            EDITOR_PLUGIN_KIND,
            EDITOR_PLUGIN_VERSION,
            EDITOR_CAPABILITIES,
        ) === 1;
        if (!ok) {
            console.warn(`[agent-doc/native] editor content receipt rejected for ${patchId}`);
        }
        return ok;
    } catch (e: any) {
        console.warn(`[agent-doc/native] editor content receipt ABI error: ${e.message}`);
        return false;
    }
}

export function recordEditorSurfaceEvent(
    projectRoot: string,
    source: string,
    filePath: string,
    surface: string,
    action: string,
    agentCommand: string,
    patchId: string | undefined,
    status: string,
): boolean {
    if (!ensureLoaded(projectRoot)) return false;
    bindFunctions();
    if (!_record_editor_surface_event) {
        console.warn('[agent-doc/native] incompatible agent-doc native library: missing agent_doc_record_editor_surface_event; reinstall the plugin/native library');
        return false;
    }
    try {
        const ok = _record_editor_surface_event(
            projectRoot,
            source,
            filePath,
            surface,
            action,
            agentCommand,
            patchId ?? '',
            status,
        ) === 1;
        if (!ok) {
            console.warn(`[agent-doc/native] editor surface event rejected: ${action} status=${status}`);
        }
        return ok;
    } catch (e: any) {
        console.warn(`[agent-doc/native] editor surface event ABI error: ${e.message}`);
        return false;
    }
}

export function recordEditorRetryRequested(
    filePath: string,
    patchId: string | undefined,
    generation: number | null,
    reason: string,
    projectRoot?: string,
): void {
    if (!patchId || generation == null) return;
    recordFactForFile(
        filePath,
        'editor_patch_retry_requested',
        { patch_id: patchId, actor_generation: generation, reason },
        `editor-retry-${patchId}-${generation}-${hashText(reason)}`,
        projectRoot,
    );
}

export function recordRouteDispatchStarted(filePath: string, routeKey: string, projectRoot?: string): number {
    const generation = nextStateGeneration(filePath, 'route_dispatch');
    recordOwnerGeneration(filePath, 'route_dispatch', generation, projectRoot);
    recordFactForFile(
        filePath,
        'route_readiness_observed',
        { actor_generation: generation, event: 'dispatch_authorized' },
        `route-authorized-${hashText(routeKey)}-${generation}`,
        projectRoot,
    );
    return generation;
}

export function recordRouteDispatchProven(
    filePath: string,
    generation: number,
    proofId: string,
    projectRoot?: string,
): void {
    recordFactForFile(
        filePath,
        'route_readiness_observed',
        { actor_generation: generation, event: 'dispatch_accepted' },
        `route-accepted-${hashText(proofId)}-${generation}`,
        projectRoot,
    );
    recordFactForFile(
        filePath,
        'dispatch_proof_observed',
        { actor_generation: generation, proof_id: proofId },
        `route-proof-${hashText(proofId)}-${generation}`,
        projectRoot,
    );
}

export function recordRouteBlocked(
    filePath: string,
    generation: number | null,
    reason: string,
    projectRoot?: string,
): void {
    if (generation == null) return;
    const reasonHash = hashText(reason);
    recordFactForFile(
        filePath,
        'route_readiness_observed',
        { actor_generation: generation, event: 'blocked' },
        `route-blocked-${generation}-${reasonHash}`,
        projectRoot,
    );
    recordFactForFile(
        filePath,
        'proof_marker_disproved',
        { marker: 'dispatch_start', source: reason.slice(0, 160) },
        `route-proof-disproved-${generation}-${reasonHash}`,
        projectRoot,
    );
}

function hashText(value: string): string {
    return crypto.createHash('sha256').update(value, 'utf-8').digest('hex').slice(0, 16);
}

/**
 * Reposition boundary marker to end of exchange component.
 * Returns the updated document, or null if FFI is unavailable/errors.
 */
export function repositionBoundaryToEnd(doc: string, projectRoot?: string, boundaryId?: string): string | null {
    if (!ensureLoaded(projectRoot)) return null;
    bindFunctions();

    const result = boundaryId
        ? _reposition_boundary_to_end_with_id(doc, boundaryId)
        : _reposition_boundary_to_end(doc);
    try {
        if (result.error) {
            const error = koffi.decode(result.error, 'char', -1);
            console.warn(`[agent-doc/native] reposition_boundary error: ${error}`);
            _free_string(result.error);
            return null;
        }
        if (!result.text) return null;
        const text = koffi.decode(result.text, 'char', -1);
        return text;
    } finally {
        if (result.text) _free_string(result.text);
    }
}

/**
 * Reposition boundary marker to end of exchange component, preserving (HEAD) annotations.
 * Returns the updated document, or null if FFI is unavailable/errors.
 */
export function repositionBoundaryToEndPreserveHead(doc: string, projectRoot?: string, boundaryId?: string): string | null {
    if (!ensureLoaded(projectRoot)) return null;
    bindFunctions();

    const result = boundaryId
        ? _reposition_boundary_to_end_preserve_head_with_id(doc, boundaryId)
        : _reposition_boundary_to_end_preserve_head(doc);
    try {
        if (result.error) {
            const error = koffi.decode(result.error, 'char', -1);
            console.warn(`[agent-doc/native] reposition_boundary_preserve_head error: ${error}`);
            _free_string(result.error);
            return null;
        }
        if (!result.text) return null;
        const text = koffi.decode(result.text, 'char', -1);
        return text;
    } finally {
        if (result.text) _free_string(result.text);
    }
}

/**
 * Normalize/fail-close template structure before editor-visible IPC writes.
 * Returns null when FFI is unavailable or the shared Rust guard rejects the doc.
 */
export function normalizeTemplateStructure(doc: string, projectRoot?: string): string | null {
    if (!ensureLoaded(projectRoot)) return null;
    bindFunctions();

    const result = _normalize_template_structure(doc);
    try {
        if (result.error) {
            const error = koffi.decode(result.error, 'char', -1);
            console.warn(`[agent-doc/native] normalize_template_structure error: ${error}`);
            _free_string(result.error);
            return null;
        }
        if (!result.text) return null;
        return koffi.decode(result.text, 'char', -1);
    } finally {
        if (result.text) _free_string(result.text);
    }
}

/**
 * Apply node-keyed IPC patches through the shared Rust document model.
 * Returns null when FFI is unavailable or rejects the patch.
 */
export function applyNodePatches(doc: string, nodePatches: unknown[], projectRoot?: string): string | null {
    if (!ensureLoaded(projectRoot)) return null;
    bindFunctions();
    if (!_apply_node_patches) return null;

    const result = _apply_node_patches(doc, JSON.stringify(nodePatches));
    try {
        if (result.error) {
            const error = koffi.decode(result.error, 'char', -1);
            console.warn(`[agent-doc/native] apply_node_patches error: ${error}`);
            _free_string(result.error);
            return null;
        }
        if (!result.text) return null;
        return koffi.decode(result.text, 'char', -1);
    } finally {
        if (result.text) _free_string(result.text);
    }
}

/**
 * True when the loaded native library exposes node-keyed IPC patch application.
 */
export function canApplyNodePatches(projectRoot?: string): boolean {
    if (!ensureLoaded(projectRoot)) return false;
    bindFunctions();
    return Boolean(_apply_node_patches);
}

function optionalString(value?: string | null): string {
    return value ?? '';
}

function observedGeneration(value?: number | null): number {
    return value ?? -1;
}

function decodeJsonResult(result: FfiJsonResult, label: string): string | null {
    try {
        if (result.error) {
            const error = koffi.decode(result.error, 'char', -1);
            console.warn(`[agent-doc/native] ${label} error: ${error}`);
            _free_string(result.error);
            return null;
        }
        if (!result.json) return null;
        return koffi.decode(result.json, 'char', -1);
    } finally {
        if (result.json) _free_string(result.json);
    }
}

/**
 * Controller-backed `agent-doc admin inspect --json` wrapper.
 */
export function adminInspectJson(options: {
    projectRoot?: string | null;
    documentPath?: string | null;
    sessionId?: string | null;
    paneId?: string | null;
} = {}): string | null {
    if (!ensureLoaded(options.projectRoot ?? undefined)) return null;
    bindFunctions();
    if (!_admin_inspect_json) return null;
    return decodeJsonResult(
        _admin_inspect_json(
            optionalString(options.projectRoot),
            optionalString(options.documentPath),
            optionalString(options.sessionId),
            optionalString(options.paneId),
        ),
        'admin_inspect',
    );
}

/**
 * Project Controller-owned tmux focus projection.
 */
export function tmuxFocusStateJson(options: {
    projectRoot?: string | null;
} = {}): string | null {
    if (!ensureLoaded(options.projectRoot ?? undefined)) return null;
    bindFunctions();
    if (!_tmux_focus_state_json) return null;
    return decodeJsonResult(
        _tmux_focus_state_json(optionalString(options.projectRoot)),
        'tmux_focus_state',
    );
}

/**
 * Project Controller-owned document pane focus.
 */
export function focusDocumentPaneJson(options: {
    documentPath: string;
    projectRoot?: string | null;
}): string | null {
    if (!ensureLoaded(options.projectRoot ?? undefined)) return null;
    bindFunctions();
    if (!_focus_document_pane_json) return null;
    return decodeJsonResult(
        _focus_document_pane_json(
            optionalString(options.projectRoot),
            options.documentPath,
        ),
        'focus_document_pane',
    );
}

/**
 * Project Controller-owned tmux layout sync.
 */
export function syncTmuxLayoutJson(options: {
    columns: string[];
    projectRoot?: string | null;
    window?: string | null;
    focus?: string | null;
    noAutostart?: boolean;
    exactVisible?: boolean;
}): string | null {
    if (!ensureLoaded(options.projectRoot ?? undefined)) return null;
    bindFunctions();
    if (!_sync_tmux_layout_json) return null;
    return decodeJsonResult(
        _sync_tmux_layout_json(
            optionalString(options.projectRoot),
            JSON.stringify(options.columns),
            optionalString(options.window),
            optionalString(options.focus),
            options.noAutostart ? 1 : 0,
            options.exactVisible ? 1 : 0,
        ),
        'sync_tmux_layout',
    );
}

/**
 * Report one editor-surface observation and get the derived tmux intent back
 * (`#jbsurfaceswap`).
 *
 * `surfaceJson` is an `EditorSurface`; the receipt is
 * `{ intent, idle, outcome, error }`. This replaces choosing between
 * {@link focusDocumentPaneJson} and {@link syncTmuxLayoutJson} in the extension:
 * the extension reports, the graph decides.
 */
export function editorSurfaceObserveJson(options: {
    projectRoot: string;
    surfaceJson: string;
}): string | null {
    if (!ensureLoaded(options.projectRoot)) return null;
    bindFunctions();
    if (!_editor_surface_observe_json) return null;
    return decodeJsonResult(
        _editor_surface_observe_json(options.projectRoot, options.surfaceJson),
        'editor_surface_observe',
    );
}

/**
 * Release a project root's editor-surface graph — the editor closed the folder.
 */
export function editorSurfaceForget(projectRoot: string): boolean {
    if (!ensureLoaded(projectRoot)) return false;
    bindFunctions();
    if (!_editor_surface_forget) return false;
    try {
        return _editor_surface_forget(projectRoot) === 1;
    } catch (err: any) {
        console.warn(`[agent-doc/native] editor_surface_forget failed: ${err.message}`);
        return false;
    }
}

/**
 * Controller-backed `agent-doc admin queue pause|resume|drain --json` wrapper.
 */
export function adminQueueControlJson(options: {
    action: string;
    projectRoot?: string | null;
    documentPath?: string | null;
    observedGeneration?: number | null;
    reason?: string | null;
    itemId?: string | null;
}): string | null {
    if (!ensureLoaded(options.projectRoot ?? undefined)) return null;
    bindFunctions();
    if (!_admin_queue_control_json) return null;
    return decodeJsonResult(
        _admin_queue_control_json(
            optionalString(options.projectRoot),
            optionalString(options.documentPath),
            options.action,
            observedGeneration(options.observedGeneration),
            optionalString(options.reason),
            optionalString(options.itemId),
        ),
        'admin_queue_control',
    );
}

/**
 * Controller-backed `agent-doc admin reap --json` wrapper.
 */
export function adminReapJson(options: {
    observedGeneration: number;
    reason: string;
    projectRoot?: string | null;
    documentPath?: string | null;
    sessionId?: string | null;
    paneId?: string | null;
}): string | null {
    if (!ensureLoaded(options.projectRoot ?? undefined)) return null;
    bindFunctions();
    if (!_admin_reap_json) return null;
    return decodeJsonResult(
        _admin_reap_json(
            optionalString(options.projectRoot),
            optionalString(options.documentPath),
            optionalString(options.sessionId),
            optionalString(options.paneId),
            options.observedGeneration,
            options.reason,
        ),
        'admin_reap',
    );
}

/**
 * Controller-backed `agent-doc admin handoff --json` wrapper.
 */
export function adminHandoffJson(options: {
    documentPath: string;
    toPane: string;
    observedGeneration: number;
    reason: string;
    projectRoot?: string | null;
}): string | null {
    if (!ensureLoaded(options.projectRoot ?? undefined)) return null;
    bindFunctions();
    if (!_admin_handoff_json) return null;
    return decodeJsonResult(
        _admin_handoff_json(
            optionalString(options.projectRoot),
            options.documentPath,
            options.toPane,
            options.observedGeneration,
            options.reason,
        ),
        'admin_handoff',
    );
}

/**
 * Controller-backed `agent-doc admin repair-projection --json` wrapper.
 */
export function adminRepairProjectionJson(options: {
    projection?: string;
    projectRoot?: string | null;
    documentPath?: string | null;
    observedGeneration?: number | null;
    reason?: string | null;
} = {}): string | null {
    if (!ensureLoaded(options.projectRoot ?? undefined)) return null;
    bindFunctions();
    if (!_admin_repair_projection_json) return null;
    return decodeJsonResult(
        _admin_repair_projection_json(
            optionalString(options.projectRoot),
            optionalString(options.documentPath),
            options.projection ?? 'all',
            observedGeneration(options.observedGeneration),
            optionalString(options.reason),
        ),
        'admin_repair_projection',
    );
}

/**
 * Collect visual token ranges for agent-doc-specific markdown constructs.
 */
export function visualTokens(doc: string, projectRoot?: string): VisualToken[] {
    if (!ensureLoaded(projectRoot)) return [];
    bindFunctions();

    const ptr = _visual_tokens_json(doc);
    try {
        if (!ptr) return [];
        const raw = koffi.decode(ptr, 'char', -1);
        const parsed = JSON.parse(raw);
        return Array.isArray(parsed) ? parsed as VisualToken[] : [];
    } catch (err: any) {
        console.warn(`[agent-doc/native] visual_tokens_json error: ${err.message}`);
        return [];
    } finally {
        if (ptr) _free_string(ptr);
    }
}

/**
 * Canonical path-based document id (`document_id_for_path`) — the reliable-sync
 * `document_hash` for a file (sidecar-retirement Phase 3C). Returns null when the
 * FFI is unavailable.
 */
export function documentIdForPath(filePath: string, projectRoot?: string): string | null {
    if (!ensureLoaded(projectRoot)) return null;
    bindFunctions();
    if (!_document_id_for_path) return null;
    const ptr = _document_id_for_path(filePath);
    try {
        if (!ptr) return null;
        const raw = koffi.decode(ptr, 'char', -1);
        return raw && raw.length > 0 ? raw : null;
    } catch (err: any) {
        console.warn(`[agent-doc/native] document_id_for_path error: ${err.message}`);
        return null;
    } finally {
        if (ptr) _free_string(ptr);
    }
}

/**
 * Resolve a document to the nearest agent-doc project root.
 *
 * Workspace folders are only a native-library loading hint. The Rust resolver
 * owns nested project/submodule selection so liveness, CRDT registration, and
 * Compact Exchange all address the same Project Controller.
 */
export function resolveProjectPath(
    filePath: string,
    projectRoot?: string,
): ResolvedProjectPath | null {
    if (!ensureLoaded(projectRoot)) return null;
    bindFunctions();
    if (!_resolve_project_path) return null;
    const result = _resolve_project_path(filePath);
    const rootPtr = result?.project_root;
    const relativePtr = result?.relative_path;
    try {
        if (!rootPtr || !relativePtr) return null;
        const resolvedRoot = koffi.decode(rootPtr, 'char', -1);
        const relativePath = koffi.decode(relativePtr, 'char', -1);
        if (!resolvedRoot || !relativePath) return null;
        return { projectRoot: resolvedRoot, relativePath };
    } catch (err: any) {
        console.warn(`[agent-doc/native] resolve_project_path error: ${err.message}`);
        return null;
    } finally {
        if (rootPtr) _free_string(rootPtr);
        if (relativePtr) _free_string(relativePtr);
    }
}

/**
 * Whether `filePath` is an agent-doc session document (frontmatter/opt-in
 * classified). Reliable-sync liveness must only report session documents so the
 * plane open-set matches the sidecar `open_agent_docs` scope. Returns false for a
 * non-session file, an unreadable path, or when the FFI is unavailable.
 */
export function isSessionDocument(filePath: string, projectRoot?: string): boolean {
    if (!ensureLoaded(projectRoot)) return false;
    bindFunctions();
    if (!_is_session_document) return false;
    try {
        return _is_session_document(filePath) === 1;
    } catch (err: any) {
        console.warn(`[agent-doc/native] is_session_document error: ${err.message}`);
        return false;
    }
}

/**
 * Enqueue a JSON `LivenessOp` batch into a document's durable reliable-sync push
 * outbox (`#lzsync` Phase 3C). No-op unless the controller dual-run flag is on.
 * Returns 0 on success, -1 on error / FFI unavailable.
 */
export function reliableSyncLivenessEnqueue(
    projectRoot: string,
    documentHash: string,
    opsJson: string,
): number {
    if (!ensureLoaded(projectRoot)) return -1;
    bindFunctions();
    if (!_reliable_sync_liveness_enqueue) return -1;
    try {
        return _reliable_sync_liveness_enqueue(projectRoot, documentHash, opsJson);
    } catch (err: any) {
        console.warn(`[agent-doc/native] reliable_sync_liveness_enqueue error: ${err.message}`);
        return -1;
    }
}

/**
 * Flush a document's durable reliable-sync push outbox to the controller.
 * Returns the ack cursor (>= 0) on success, -1 on error / FFI unavailable.
 */
export function reliableSyncLivenessFlush(projectRoot: string, documentHash: string): number {
    if (!ensureLoaded(projectRoot)) return -1;
    bindFunctions();
    if (!_reliable_sync_liveness_flush) return -1;
    try {
        return Number(_reliable_sync_liveness_flush(projectRoot, documentHash));
    } catch (err: any) {
        console.warn(`[agent-doc/native] reliable_sync_liveness_flush error: ${err.message}`);
        return -1;
    }
}

export function reliableSyncDocumentOpPush(
    projectRoot: string,
    filePath: string,
    deltaJson: string,
): boolean {
    if (!ensureLoaded(projectRoot)) return false;
    bindFunctions();
    if (!_reliable_sync_document_op_push) return false;
    try {
        return _reliable_sync_document_op_push(projectRoot, filePath, deltaJson) === 0;
    } catch (err: any) {
        console.warn(`[agent-doc/native] reliable_sync_document_op_push error: ${err.message}`);
        return false;
    }
}

export function reliableSyncTextAdoptPush(
    projectRoot: string,
    filePath: string,
    text: string,
): boolean {
    if (!ensureLoaded(projectRoot)) return false;
    bindFunctions();
    if (!_reliable_sync_text_adopt_push) return false;
    try {
        return _reliable_sync_text_adopt_push(projectRoot, filePath, text) === 0;
    } catch (err: any) {
        console.warn(`[agent-doc/native] reliable_sync_text_adopt_push error: ${err.message}`);
        return false;
    }
}

export function reliableSyncDocumentOpFlush(projectRoot: string, filePath: string): void {
    if (!ensureLoaded(projectRoot)) return;
    bindFunctions();
    if (!_reliable_sync_document_op_flush) return;
    try {
        _reliable_sync_document_op_flush(projectRoot, filePath);
    } catch (err: any) {
        console.warn(`[agent-doc/native] reliable_sync_document_op_flush error: ${err.message}`);
    }
}

/**
 * Record a document change plus the editor's FULL visible buffer content (#pcp6).
 * Mirrors the JetBrains plugin: lets the CLI visible-write reconcile guard
 * positively confirm the editor buffer equals on-disk content (no unsaved edit
 * ahead of disk) instead of inferring from a len/hash digest. Current content
 * stays in the Lazily CRDT.
 */
export function lazilyCurrentObserved(
    filePath: string,
    content: string,
    projectRoot?: string,
    editorId?: string,
    noUnsavedOperatorEdits?: boolean,
): void {
    if (!ensureLoaded(projectRoot)) return;
    bindFunctions();
    if (!editorId || !_lazily_current_observed_v1) return;
    _lazily_current_observed_v1(
        filePath,
        content,
        editorId,
        EDITOR_PLUGIN_KIND,
        EDITOR_PLUGIN_VERSION,
        EDITOR_CAPABILITIES,
        noUnsavedOperatorEdits ? 1 : 0,
    );
}

/**
 * Publish this editor instance's reliable-sync close for the document.
 */
export function documentClosedForEditor(
    filePath: string,
    projectRoot?: string,
    editorId?: string,
): void {
    if (!editorId) return;
    if (!ensureLoaded(projectRoot)) return;
    bindFunctions();
    if (_document_closed_for_editor) {
        _document_closed_for_editor(filePath, editorId);
    }
}

/** Resolve a Lazily-retained reconnect target against the exact live buffer. */
export function deferredWriteReconnectContent(
    filePath: string,
    editorContent: string,
    projectRoot?: string,
): string | null {
    if (!ensureLoaded(projectRoot)) return null;
    bindFunctions();
    if (!_deferred_write_reconnect_content) return null;
    const ptr = _deferred_write_reconnect_content(filePath, editorContent);
    if (!ptr) return null;
    try {
        return koffi.decode(ptr, 'char', -1);
    } finally {
        _free_string(ptr);
    }
}

export function deferredWriteReconnectPropagated(
    filePath: string,
    editorContent: string,
    projectRoot?: string,
): boolean {
    if (!ensureLoaded(projectRoot)) return false;
    bindFunctions();
    if (!_deferred_write_reconnect_propagated) return false;
    return _deferred_write_reconnect_propagated(filePath, editorContent) === 1;
}

/**
 * Check if FFI is available and loaded.
 */
export function isAvailable(projectRoot?: string): boolean {
    return ensureLoaded(projectRoot);
}
