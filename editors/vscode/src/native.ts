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

let lib: any = null;
let koffi: any = null;
let loaded = false;
let loadAttempted = false;
let loadedPath: string | null = null;
let loadedMtime: number = 0;
let currentLockFile: string | null = null;

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
    _is_idle = null;
    _await_idle = null;
    _document_changed = null;
    _is_tracked = null;
    _resolve_project_path = null;
    _free_string = null;
    _version = null;
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
let _is_idle: any = null;
let _await_idle: any = null;
let _document_changed: any = null;
let _is_tracked: any = null;
let _resolve_project_path: any = null;
let _free_string: any = null;
let _version: any = null;

function bindFunctions(): void {
    if (_reposition_boundary_to_end) return;

    // Define the FfiPatchResult struct
    const FfiPatchResultType = koffi.struct('FfiPatchResult', {
        text: 'char*',
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
    _is_idle = lib.func('agent_doc_is_idle', 'bool', ['str', 'int64']);
    _await_idle = lib.func('agent_doc_await_idle', 'bool', ['str', 'int64', 'int64']);
    _document_changed = lib.func('agent_doc_document_changed', 'void', ['str']);
    _is_tracked = lib.func('agent_doc_is_tracked', 'bool', ['str']);
    _resolve_project_path = lib.func('agent_doc_resolve_project_path', FfiProjectPathType, ['str']);
    _free_string = lib.func('agent_doc_free_string', 'void', ['char*']);
    _version = lib.func('agent_doc_version', 'char*', []);
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
