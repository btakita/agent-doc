# Submodule-Aware Patch Routing in agent-doc

**Date:** 2026-04-11  
**Status:** Design Review  
**Owner:** Brian Takita

## Problem Statement

When agent-doc runs on a file in a git submodule (e.g., `src/session-share/tasks/claudescore.md`), it writes IPC patches to the **submodule's** `.agent-doc/patches/` directory. However, the JetBrains plugin only monitors the **parent repository's** `.agent-doc/patches/` directory for IPC messages, causing patches to accumulate unread and never apply to the editor.

**Impact:** Submodule documents receive no visual feedback in the IDE, breaking the agent-doc workflow for any file in a submodule.

## Solution Overview

Detect when a file is in a submodule context and automatically route IPC patches to the **parent repository's** `.agent-doc/patches/` directory. The JSON patch structure already includes the canonical absolute file path, so the plugin can identify and apply the correct patch regardless of which `.agent-doc/` directory it's stored in.

## Architecture

### 1. Submodule Detection

Use the existing `git::resolve_to_git_root()` function to determine the actual git repository root:

- **For regular files:** Returns the git root (same as current directory)
- **For submodule files:** Returns the superproject root (parent repository)

Compare the resolved root against the canonical file path to detect submodule context:
- If `superproject_root != git_root_of_file_parent`, the file is in a submodule

### 2. Patch Directory Routing

**Current behavior (broken):**
```rust
let patches_dir = project_root.join(".agent-doc/patches");
```

**New behavior:**
```rust
let canonical = file.canonicalize()?;
let (superproject_root, _) = git::resolve_to_git_root(&canonical)?;
let patches_dir = superproject_root.join(".agent-doc/patches");
```

### 3. Impact on IPC Paths

**Socket IPC:** Already correct (payload includes canonical file path)  
**File-based IPC:** Only directory changes; hash-based patch filename remains the same

The JSON payload in both paths already includes the absolute file path:
```json
{
  "file": "/absolute/path/to/src/session-share/tasks/claudescore.md",
  "patches": [...]
}
```

The plugin reads `"file"` to determine the target, so the patch location is transparent.

### 4. Initialization Consistency

The `agent-doc init` command should only create `.agent-doc/` in the **project root** (parent repository). Submodule sessions automatically route patches to the parent via the detection logic above.

Existing `init.rs` behavior should already be correct if it targets `find_project_root()`. Verify in code review.

## Changes Required

### File: `src/agent-doc/src/write.rs`

**Function: `try_ipc()`**

1. After acquiring canonical path, call `git::resolve_to_git_root()` to get superproject root
2. Use superproject root for patches directory resolution (both socket and file paths)
3. Keep all other logic unchanged (payload structure, hash computation, etc.)

**Pseudocode:**
```rust
let canonical = file.canonicalize()?;
let hash = snapshot::doc_hash(file)?;

// NEW: Resolve to superproject root (handles submodules automatically)
let (superproject_root, _) = git::resolve_to_git_root(&canonical)?;

// Use superproject root for patches directory
let patches_dir = superproject_root.join(".agent-doc/patches");
```

### File: `src/agent-doc/src/git.rs` (optional helper)

No new functions needed. Enhance existing `resolve_to_git_root()` documentation to clarify superproject handling.

## Testing

### Unit Tests (`src/agent-doc/src/write.rs`)

1. **Submodule patch directory:** Verify that when a file is in a submodule, patches are written to parent's `.agent-doc/patches/`
2. **Regular file patch directory:** Verify non-submodule files still write to current project root
3. **Canonical path in payload:** Verify JSON patches include absolute file path (unchanged)
4. **Socket and file paths:** Verify both IPC paths use superproject root

### Integration Tests

1. **Submodule session end-to-end:**
   - Initialize submodule: `agent-doc init src/session-share`
   - Run `/agent-doc src/session-share/tasks/claudescore.md`
   - Verify patch written to `parent/.agent-doc/patches/`, not `submodule/.agent-doc/patches/`
   - Verify plugin reads and applies patch correctly

2. **Cross-repository isolation:**
   - Verify patches for multiple submodules don't interfere (all route to same parent, but are distinguished by file path in JSON)

## Backward Compatibility

- ✅ Non-submodule files unaffected (same behavior)
- ✅ IPC JSON structure unchanged (payload already includes canonical path)
- ✅ Plugin requires no changes (reads from existing parent `.agent-doc/patches/`)
- ✅ Existing `.agent-doc/` initialization still works (improved behavior for submodules)

## Success Criteria

1. Submodule patch writes correctly to parent `.agent-doc/patches/`
2. Plugin reads and applies patches from parent directory
3. Visual feedback appears in editor for submodule documents
4. Non-submodule files continue to work as before
5. Tests pass (unit + integration)

## Open Questions

None. Design is self-contained with clear file changes and testing strategy.
