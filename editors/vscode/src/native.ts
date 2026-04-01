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
    if (loaded) return true;
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
        console.log(`[agent-doc/native] loaded ${libPath}`);
        return true;
    } catch (e: any) {
        console.log(`[agent-doc/native] failed to load ${libPath}: ${e.message}`);
        return false;
    }
}

// Lazy function bindings (resolved on first call)
let _reposition_boundary_to_end: any = null;
let _is_idle: any = null;
let _await_idle: any = null;
let _document_changed: any = null;
let _is_tracked: any = null;
let _free_string: any = null;

function bindFunctions(): void {
    if (_reposition_boundary_to_end) return;

    // Define the FfiPatchResult struct
    const FfiPatchResultType = koffi.struct('FfiPatchResult', {
        text: 'char*',
        error: 'char*',
    });

    _reposition_boundary_to_end = lib.func('agent_doc_reposition_boundary_to_end', FfiPatchResultType, ['str']);
    _is_idle = lib.func('agent_doc_is_idle', 'bool', ['str', 'int64']);
    _await_idle = lib.func('agent_doc_await_idle', 'bool', ['str', 'int64', 'int64']);
    _document_changed = lib.func('agent_doc_document_changed', 'void', ['str']);
    _is_tracked = lib.func('agent_doc_is_tracked', 'bool', ['str']);
    _free_string = lib.func('agent_doc_free_string', 'void', ['char*']);
}

/**
 * Reposition boundary marker to end of exchange component.
 * Returns the updated document, or null if FFI is unavailable/errors.
 */
export function repositionBoundaryToEnd(doc: string, projectRoot?: string): string | null {
    if (!ensureLoaded(projectRoot)) return null;
    bindFunctions();

    const result = _reposition_boundary_to_end(doc);
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
 * Check if FFI is available and loaded.
 */
export function isAvailable(projectRoot?: string): boolean {
    return ensureLoaded(projectRoot);
}
