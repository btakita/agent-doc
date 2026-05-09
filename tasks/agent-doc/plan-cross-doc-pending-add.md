# Plan: Cross-document --pending-add targets wrong file

## Problem

The `#agent-doc-bug` prompt preset instructs:
> Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md

When the skill executes this, it runs `agent-doc write <current-file> --pending-add "..."` — targeting the **current session document's** backlog instead of the specified target file. The backlog item ends up in the wrong document.

**Observed:** Backlog item for a prefix-strip bug added to `tasks/buildparty-investor-demo/dev.md` instead of `tasks/agent-doc/agent-doc-bugs2.md`.

**Contributing factor:** `agent-doc-bugs2.md` was deleted (commit `c452dcc0`, 2026-04-30). Even if the skill had attempted to write to it, the file wouldn't exist. But the skill never attempted — it defaulted to the current document.

## Root Cause

The skill's pending-add logic always targets the current session document. When a prompt preset or user instruction says "add to the backlog of [other-file]", the skill has no mechanism to:

1. Parse the target file path from the instruction
2. Run `agent-doc write <target-file> --pending-add "..."` against a different file
3. Validate that the target file exists before attempting the write
4. Error clearly if the target file doesn't exist

## Proposed Changes

### Phase 1: Skill-level cross-document pending-add

The skill (CLAUDE.md instructions + harness behavior) needs to recognize when a prompt preset or user instruction specifies a target file for backlog items and run `agent-doc write` against that file instead of the current document.

**Implementation:**
1. When executing `#agent-doc-bug` or similar presets that name a target backlog file, the skill should run:
   ```bash
   agent-doc write <target-file> --pending-add "id=<id> <text>"
   ```
2. If the target file doesn't exist, the skill should warn the user and offer to recreate it or add to the current document as fallback — not silently add to the current document.

### Phase 2: Binary-level `--pending-add-to` flag (optional)

Add a cross-document write flag to the binary:
```bash
agent-doc write <current-file> --pending-add-to <target-file> "id=<id> <text>"
```

This keeps the current session document as the primary file (for commit tracking, cycle management) while allowing the pending item to be written to a different file. The binary handles file validation and commits both files atomically.

### Phase 3: Target file existence check

When a prompt preset references a file path (e.g., `backlog: agent-loop/tasks/agent-doc/agent-doc-bugs2.md`), the skill should verify the file exists during planning/dispatch. If missing, surface the issue before attempting the write.

## Test Cases

1. Preset says "add to backlog of file-B.md" while in file-A.md session → item appears in file-B.md, not file-A.md
2. Target file doesn't exist → clear error message, no silent fallback to current document
3. Target file exists but has no `<!-- agent:backlog -->` component → clear error, not appended to exchange
