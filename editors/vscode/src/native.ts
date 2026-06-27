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
import {
    StateGraphMirror,
    type MirrorProjectionSummary,
} from './stateMirror';

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
    _admin_queue_control_json = null;
    _admin_reap_json = null;
    _admin_handoff_json = null;
    _admin_repair_projection_json = null;
    _visual_tokens_json = null;
    _is_idle = null;
    _await_idle = null;
    _document_changed = null;
    _is_tracked = null;
    _document_changed_digest_for_editor = null;
    _document_changed_digest_content_for_editor = null;
    _document_closed_for_editor = null;
    _resolve_project_path = null;
    _free_state = null;
    _free_string = null;
    _version = null;
    _state_projection = null;
    _state_subscribe = null;
    _record_state_event = null;
    _record_editor_op = null;
    _document_base_hash = null;
    _replica_open = null;
    _replica_apply_local = null;
    _replica_apply_update = null;
    _replica_encode_state = null;
    _replica_text = null;
    _replica_close = null;
}

const LIB_NAME = process.platform === 'darwin' ? 'libagent_doc.dylib' : 'libagent_doc.so';

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
                lib = koffi.load(loadedPath);
                loadedMtime = fs.statSync(loadedPath).mtimeMs;
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
        koffi = require('koffi');
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
        lib = koffi.load(libPath);
        loaded = true;
        loadedPath = libPath;
        loadedMtime = fs.statSync(libPath).mtimeMs;
        writePidLock(libPath);
        process.on('exit', removePidLock);
        verifyVersion(libPath);
        return true;
    } catch (e: any) {
        console.log(`[agent-doc/native] failed to load ${libPath}: ${e.message}`);
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
let _admin_queue_control_json: any = null;
let _admin_reap_json: any = null;
let _admin_handoff_json: any = null;
let _admin_repair_projection_json: any = null;
let _visual_tokens_json: any = null;
let _is_idle: any = null;
let _await_idle: any = null;
let _document_changed: any = null;
let _document_changed_digest: any = null;
let _document_changed_digest_content: any = null;
let _document_changed_digest_for_editor: any = null;
let _document_changed_digest_content_for_editor: any = null;
let _document_closed_for_editor: any = null;
let _is_tracked: any = null;
let _resolve_project_path: any = null;
let _free_state: any = null;
let _free_string: any = null;
let _version: any = null;
let _state_projection: any = null;
let _state_subscribe: any = null;
let _record_state_event: any = null;
let _reconnect_buffer_decision: any = null;
let _record_editor_op: any = null;
let _document_base_hash: any = null;
let _replica_open: any = null;
let _replica_apply_local: any = null;
let _replica_apply_update: any = null;
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
        project_root: 'char*',
        relative_path: 'char*',
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
        _admin_queue_control_json = null;
        _admin_reap_json = null;
        _admin_handoff_json = null;
        _admin_repair_projection_json = null;
    }
    _visual_tokens_json = lib.func('agent_doc_visual_tokens_json', 'char*', ['str']);
    _is_idle = lib.func('agent_doc_is_idle', 'bool', ['str', 'int64']);
    _await_idle = lib.func('agent_doc_await_idle', 'bool', ['str', 'int64', 'int64']);
    _document_changed = lib.func('agent_doc_document_changed', 'void', ['str']);
    _document_changed_digest = lib.func('agent_doc_document_changed_digest', 'void', ['str', 'int64', 'str']);
    _document_changed_digest_content = lib.func('agent_doc_document_changed_digest_content', 'void', ['str', 'str']);
    try {
        _document_changed_digest_for_editor = lib.func(
            'agent_doc_document_changed_digest_for_editor',
            'void',
            ['str', 'int64', 'str', 'str'],
        );
        _document_changed_digest_content_for_editor = lib.func(
            'agent_doc_document_changed_digest_content_for_editor',
            'void',
            ['str', 'str', 'str'],
        );
        _document_closed_for_editor = lib.func(
            'agent_doc_document_closed_for_editor',
            'void',
            ['str', 'str'],
        );
    } catch (e: any) {
        console.log(`[agent-doc/native] per-editor live-buffer ABI unavailable: ${e.message}`);
        _document_changed_digest_for_editor = null;
        _document_changed_digest_content_for_editor = null;
        _document_closed_for_editor = null;
    }
    _is_tracked = lib.func('agent_doc_is_tracked', 'bool', ['str']);
    _resolve_project_path = lib.func('agent_doc_resolve_project_path', FfiProjectPathType, ['str']);
    _free_state = lib.func('agent_doc_free_state', 'void', ['void*', 'size_t']);
    _free_string = lib.func('agent_doc_free_string', 'void', ['char*']);
    _version = lib.func('agent_doc_version', 'char*', []);
    try {
        _state_projection = lib.func('agent_doc_state_projection', 'char*', ['str']);
        _record_state_event = lib.func('agent_doc_record_state_event', 'int32', ['str', 'str']);
    } catch (e: any) {
        console.log(`[agent-doc/native] state projection ABI unavailable: ${e.message}`);
        _state_projection = null;
        _record_state_event = null;
    }
    try {
        // #r5at lazily-js reactive mirror: warm subscribe (snapshot/delta) over
        // the FFI state backbone. Optional so an older cdylib without the symbol
        // does not break the rest of the bindings.
        _state_subscribe = lib.func('agent_doc_state_subscribe', 'char*', ['str', 'uint64']);
    } catch (e: any) {
        console.log(`[agent-doc/native] state subscribe ABI unavailable: ${e.message}`);
        _state_subscribe = null;
    }
    try {
        // #yzer reconnect-reread (VS Code parity with the JB plugin). Optional so an
        // older cdylib without the symbol does not break the rest of the bindings.
        _reconnect_buffer_decision = lib.func(
            'agent_doc_reconnect_buffer_decision',
            'char*',
            ['str', 'str', 'str'],
        );
    } catch (e: any) {
        console.log(`[agent-doc/native] reconnect_buffer_decision ABI unavailable: ${e.message}`);
        _reconnect_buffer_decision = null;
    }
    try {
        // #qnodemerge4wire Phase 4 editor-op reporters. Optional so an older
        // cdylib without the symbols does not break the rest of the bindings.
        _record_editor_op = lib.func(
            'agent_doc_record_editor_op',
            'int32',
            ['str', 'str', 'str', 'int64', 'str', 'int64'],
        );
        _document_base_hash = lib.func('agent_doc_document_base_hash', 'char*', ['str']);
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
        _replica_encode_state = lib.func(
            'agent_doc_replica_encode_state',
            'void*',
            ['uint64', koffi.out(koffi.pointer('size_t'))],
        );
        _replica_text = lib.func('agent_doc_replica_text', 'char*', ['uint64']);
        _replica_close = lib.func('agent_doc_replica_close', 'int32', ['uint64']);
    } catch (e: any) {
        console.log(`[agent-doc/native] replica ABI unavailable: ${e.message}`);
        _replica_open = null;
        _replica_apply_local = null;
        _replica_apply_update = null;
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

export interface ReconnectDecision {
    decision: string;
    content?: string;
}

/**
 * Parse the JSON returned by `agent_doc_reconnect_buffer_decision` into a typed
 * decision. Pure (no FFI) so it is unit-testable. Returns null on malformed JSON
 * or a missing/invalid `decision` field (fail safe — the caller keeps the buffer).
 */
export function parseReconnectDecision(json: string): ReconnectDecision | null {
    try {
        const parsed = JSON.parse(json);
        if (!parsed || typeof parsed.decision !== 'string') return null;
        const out: ReconnectDecision = { decision: parsed.decision };
        if (typeof parsed.content === 'string') out.content = parsed.content;
        return out;
    } catch {
        return null;
    }
}

/**
 * #yzer — ask the binary how the editor should reconcile its buffer with disk on
 * (re)connect. The binary owns the staleness decision (disk==HEAD vs buffer==a
 * prior commit blob); the plugin is a thin caller and only re-reads when told to,
 * so genuine unsynced user edits are never clobbered (editor wins, #editorbufwin).
 * Returns the decision, or null if the FFI is unavailable/errors (fail safe).
 * VS Code parity for the JB plugin's `agent_doc_reconnect_buffer_decision` usage.
 */
export function reconnectBufferDecision(
    projectRoot: string,
    filePath: string,
    buffer: string,
): ReconnectDecision | null {
    if (!ensureLoaded(projectRoot)) return null;
    bindFunctions();
    if (!_reconnect_buffer_decision) return null;
    let ptr: any = null;
    try {
        ptr = _reconnect_buffer_decision(projectRoot, filePath, buffer);
        if (!ptr) return null;
        const json = koffi.decode(ptr, 'char', -1);
        return parseReconnectDecision(json);
    } catch (e: any) {
        console.warn(`[agent-doc/native] reconnect_buffer_decision error: ${e.message}`);
        return null;
    } finally {
        if (ptr) _free_string(ptr);
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
        return Buffer.from(koffi.view(ptr, len));
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

// #r5at lazily-js reactive mirror — the VS Code counterpart of the JB plugin's
// StateProjectionBridge per-document StateGraphMirror (#n529 / #lazilystatesync3).
// Keyed by documentHash (canonical-path SHA-256); re-subscription lazily re-creates
// the mirror from a fresh cold snapshot, so aggressive eviction is safe.
const stateMirrors = new Map<string, StateGraphMirror>();

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
    let mirror = stateMirrors.get(docHash);
    if (!mirror) {
        mirror = new StateGraphMirror();
        stateMirrors.set(docHash, mirror);
    }
    const lastEpoch = mirror.isInitialized ? mirror.epoch : 0;
    const raw = stateSubscribe(docHash, lastEpoch, projectRoot);
    if (!raw) return null;
    if (!mirror.applyMessage(raw)) return null;
    try {
        const parsed = JSON.parse(raw);
        return typeof parsed?.type === 'string' ? parsed.type : null;
    } catch {
        return null;
    }
}

/**
 * Reactive summary derived from the per-document mirror's tracked cells
 * (`#r5at`). Call after {@link subscribeMirrorForFile}. Returns null when the
 * mirror has not been initialized yet.
 */
export function mirrorSummaryForFile(filePath: string): MirrorProjectionSummary | null {
    const mirror = stateMirrors.get(documentHash(filePath));
    if (!mirror || !mirror.isInitialized) return null;
    return mirror.summary();
}

/** The current mirror epoch for [filePath], or null if never initialized (#r5at). */
export function mirrorEpochForFile(filePath: string): number | null {
    const mirror = stateMirrors.get(documentHash(filePath));
    return mirror && mirror.isInitialized ? mirror.epoch : null;
}

/**
 * Reactive read for consumers (`#r5at`, the VS Code analog of the JB
 * `reactiveSummaryForFile`): advance the per-document mirror by absorbing FFI
 * deltas since its current epoch, then derive the summary from tracked cells.
 *
 * Drop-in replacement for the cold {@link stateProjectionForFile} +
 * {@link projectionSummary} pull on the read path:
 *  - The reactive {@link MirrorProjectionSummary} cannot carry
 *    `latestTransportPatchId` (the patch id is the node's `slot_id`, not a wire
 *    payload field). When `includeTransportPatchId` and the mirror summary leaves
 *    it undefined, backfill that one field from the cold projection.
 *  - When the mirror never initializes (FFI unavailable / no recorded state yet),
 *    fall back to the cold pull so a cold-start read still surfaces a summary.
 *
 * Returns null only when both the reactive mirror and the cold pull are empty.
 */
export function reactiveSummaryForFile(
    filePath: string,
    projectRoot?: string,
    includeTransportPatchId: boolean = true,
): MirrorProjectionSummary | null {
    subscribeMirrorForFile(filePath, projectRoot);
    const mirror = mirrorSummaryForFile(filePath);
    if (!mirror) {
        // Cold-start fallback: mirror never initialized (no FFI / no state yet).
        const cold = stateProjectionForFile(filePath, projectRoot);
        const summary = cold ? projectionSummary(cold) : null;
        if (!summary) return null;
        return {
            routeReadiness: summary.routeReadiness,
            routePaneId: summary.routePaneId,
            latestTransportPatchId: summary.latestTransportPatchId,
            latestTransportPhase: summary.latestTransportPhase,
            proofMarkers: summary.proofMarkers,
        };
    }
    if (includeTransportPatchId && mirror.latestTransportPatchId == null) {
        // Narrow backfill: the wire delta does not carry the patch id, so pull
        // just that field from the cold projection while keeping every other
        // field reactive (from the mirror's tracked cells).
        const cold = stateProjectionForFile(filePath, projectRoot);
        const coldPatchId = cold ? projectionSummary(cold)?.latestTransportPatchId : undefined;
        if (coldPatchId != null) {
            return { ...mirror, latestTransportPatchId: coldPatchId };
        }
    }
    return mirror;
}

/**
 * Evict the per-document mirror for [filePath] (`#r5at`, the VS Code analog of
 * the JB `evictForFile`). Called when the editor tab/document closes so a reused
 * path (move/symlink/reopen) does not surface the prior document's stale state.
 */
export function evictStateMirrorForFile(filePath: string): void {
    stateMirrors.delete(documentHash(filePath));
}

/** Test-only: number of live per-document mirrors (eviction coverage). */
export function debugStateMirrorCount(): number {
    return stateMirrors.size;
}

/**
 * Test-only seam (`#r5at`): seed the per-document mirror by applying a
 * lazily-spec snapshot/delta message directly, bypassing the FFI subscribe call.
 * Lets consumer-observation tests assert the read path derives the summary from
 * the reactive mirror's tracked cells rather than the cold pull (FFI is
 * unavailable in plugin unit tests). Returns whether the message applied.
 */
export function seedStateMirrorMessageForTest(filePath: string, message: string): boolean {
    const docHash = documentHash(filePath);
    let mirror = stateMirrors.get(docHash);
    if (!mirror) {
        mirror = new StateGraphMirror();
        stateMirrors.set(docHash, mirror);
    }
    return mirror.applyMessage(message);
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

export function recordEditorAckObserved(
    filePath: string,
    patchId: string | undefined,
    generation: number | null,
    projectRoot?: string,
): void {
    if (!patchId || generation == null) return;
    recordFactForFile(
        filePath,
        'editor_ack_observed',
        { patch_id: patchId, actor_generation: generation },
        `editor-ack-${patchId}-${generation}`,
        projectRoot,
    );
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

/** Whether the loaded cdylib exposes the reconnect-reread decision FFI. */
export function hasReconnectBufferDecision(): boolean {
    try {
        bindFunctions();
    } catch {
        return false;
    }
    return Boolean(_reconnect_buffer_decision);
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
 * Non-blocking idle check.
 * Returns true if no document_changed event within debounceMs.
 * Returns true if FFI is unavailable (don't block callers).
 */
export function isIdle(filePath: string, debounceMs: number, projectRoot?: string): boolean {
    if (!ensureLoaded(projectRoot)) return true;
    bindFunctions();
    return _is_idle(filePath, debounceMs);
}

/**
 * Block until idle for debounceMs, or timeoutMs expires.
 * Returns true if idle was reached.
 * Returns true if FFI is unavailable (don't block callers).
 */
export function awaitIdle(filePath: string, debounceMs: number, timeoutMs: number, projectRoot?: string): boolean {
    if (!ensureLoaded(projectRoot)) return true;
    bindFunctions();
    return _await_idle(filePath, debounceMs, timeoutMs);
}

/**
 * Record a document change event for debounce tracking.
 */
export function documentChanged(filePath: string, projectRoot?: string): void {
    if (!ensureLoaded(projectRoot)) return;
    bindFunctions();
    _document_changed(filePath);
}

/**
 * Record a document change event plus the editor-visible buffer digest.
 */
export function documentChangedDigest(
    filePath: string,
    contentLen: number,
    contentHash: string,
    projectRoot?: string,
    editorId?: string,
): void {
    if (!ensureLoaded(projectRoot)) return;
    bindFunctions();
    if (editorId && _document_changed_digest_for_editor) {
        _document_changed_digest_for_editor(filePath, contentLen, contentHash, editorId);
    } else {
        _document_changed_digest(filePath, contentLen, contentHash);
    }
}

/**
 * Record a document change plus the editor's FULL visible buffer content (#pcp6).
 * Mirrors the JetBrains plugin: lets the CLI visible-write reconcile guard
 * positively confirm the editor buffer equals on-disk content (no unsaved edit
 * ahead of disk) instead of inferring from a len/hash digest. The text stays
 * local to the project `.agent-doc/` state dir.
 */
export function documentChangedDigestContent(
    filePath: string,
    content: string,
    projectRoot?: string,
    editorId?: string,
): void {
    if (!ensureLoaded(projectRoot)) return;
    bindFunctions();
    if (editorId && _document_changed_digest_content_for_editor) {
        _document_changed_digest_content_for_editor(filePath, content, editorId);
    } else {
        _document_changed_digest_content(filePath, content);
    }
}

/**
 * Clear this editor instance's durable live-buffer sidecar on close.
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

/**
 * Check if file is tracked (at least one document_changed call).
 */
export function isTracked(filePath: string, projectRoot?: string): boolean {
    if (!ensureLoaded(projectRoot)) return false;
    bindFunctions();
    return _is_tracked(filePath);
}

/**
 * Resolve the agent-doc project root for a file path.
 * Walks up from the file looking for the nearest `.agent-doc/` ancestor.
 * Returns { projectRoot, relativePath } or null if FFI unavailable or no ancestor found.
 */
export function resolveProjectPath(
    filePath: string,
    projectRoot?: string,
): { projectRoot: string; relativePath: string } | null {
    if (!ensureLoaded(projectRoot)) return null;
    bindFunctions();

    const result = _resolve_project_path(filePath);
    try {
        if (!result.project_root || !result.relative_path) return null;
        const root = koffi.decode(result.project_root, 'char', -1);
        const rel = koffi.decode(result.relative_path, 'char', -1);
        return { projectRoot: root, relativePath: rel };
    } finally {
        if (result.project_root) _free_string(result.project_root);
        if (result.relative_path) _free_string(result.relative_path);
    }
}

/**
 * Check if FFI is available and loaded.
 */
export function isAvailable(projectRoot?: string): boolean {
    return ensureLoaded(projectRoot);
}
