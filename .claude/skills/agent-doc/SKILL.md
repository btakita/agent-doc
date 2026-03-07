---
description: Submit a session document to an AI agent and append the response
user-invocable: true
argument-hint: "<file>"
agent-doc-version: "0.9.6"
execution-mode: hybrid
---

# agent-doc submit

Interactive document session — respond to user edits in a markdown document.

## Invocation

```
/agent-doc <FILE>
/agent-doc claim <FILE>
```

- `/agent-doc <FILE>` — run the document session workflow (diff, respond, write back)
- `/agent-doc claim <FILE>` — claim a file for the current tmux pane (run `agent-doc claim <FILE>` via Bash, then stop)

Arguments: `FILE` — path to the session document (e.g., `plan.md`)

## Core Principles

- **Document is the UI** — the user's edits ARE the prompt; respond in the document AND the console
- **Preserve user edits** — never overwrite; read the file fresh before writing
- **Show progress** — stream your response in the console so the user sees real-time feedback
- **Maintain session** — the document is a living conversation, not a one-shot

## Execution Model

Controlled by the `execution-mode` frontmatter field (default: `hybrid`).

| Mode | Behavior | Token Cost |
|------|----------|-----------|
| **`hybrid`** (default) | First doc direct, 2nd+ concurrent docs use background subagent | Optimal — no overhead for single doc, scales for multi-doc |
| **`parallel`** | Every `/agent-doc` spawns a background subagent | Higher (~7x), but all docs process independently |
| **`sequential`** | Never use subagents, fully sequential | Cheapest — B waits for A to complete |

**Why direct processing is safe:** Filesystem locking (flock + atomic rename) in agent-doc v0.9.5+ serializes concurrent access to documents and snapshots. The document IS the shared state (blackboard pattern) — no context duplication needed.

### Hybrid Mode (Default)

- Process the first `/agent-doc <FILE>` directly in the main agent context
- If a second `/agent-doc <FILE2>` arrives while the first is still processing, dispatch it via `Agent(run_in_background: true)`
- Same-document re-submit: filesystem locks serialize access; second invocation processes after the first completes

### Parallel Mode

- Every `/agent-doc <FILE>` invocation spawns a background subagent via `Agent(run_in_background: true)`
- All documents process independently and concurrently

### Sequential Mode

- Process every `/agent-doc <FILE>` directly in the main agent context
- Document B blocks until Document A's full cycle completes

### Subagent Prompt (when used)

The subagent prompt must include:
- The full document cycle (steps 1-5 below)
- The absolute path to `<FILE>`
- The absolute path to the snapshot
- The project root path (for `.agent-doc/` operations)

## Workflow

### 0. Pre-flight checks

**Detect claim:** If the first argument is `claim`, run `agent-doc claim <FILE>` via Bash and stop. Do not proceed with the document session workflow. Print the output to confirm the claim.

**Auto-update skill:** Run `agent-doc --version` and compare against the `agent-doc-version` in this file's frontmatter. If the binary version is newer, run `agent-doc skill install` to update this SKILL.md, then continue with the updated instructions. If `agent-doc` is not installed or the version matches, skip this step.

**Check claims log:** Read `.agent-doc/claims.log` (if it exists). Print each line to the console as a record of IDE-triggered claims. Then truncate the file (write empty string). This gives a permanent record in the Claude session of claims made from the editor plugin.

### 1. Read the document and snapshot

- Read `<FILE>` to get current content
- Read the snapshot at `.agent-doc/snapshots/<hash>.md` where `<hash>` is SHA256 of the canonical file path
  - If no snapshot exists, treat this as the first submit (entire document is new)

### 2. Compute the diff
- Compare snapshot (previous state) against current content
- **Strip comments** from both sides before comparing:
  - HTML comments: `<!-- ... -->` (including multiline)
  - Link reference comments: `[//]: # (...)` (single-line)
  - Comments are a user scratchpad — adding, editing, or removing comments should NOT trigger a response
  - Uncommenting text (removing the markers) IS a real change and triggers a response
  - The snapshot stores full content including comments; stripping is only for diff comparison
- The diff represents what the user changed since the last submit
- If no diff (content unchanged after comment stripping), tell the user nothing changed and stop

#### Stale snapshot recovery (idempotent detection)

If there IS a diff, check whether the diff consists entirely of complete `## Assistant` and `## User` exchange blocks that were already written to the document by a previous cycle whose snapshot update was interrupted (e.g., by context compaction).

**Detection:** Compare the diff content against what's already in the document. If the document already contains `## Assistant` blocks after the snapshot's last position, and the trailing `## User` block is empty (no new user content), the previous cycle completed the write but not the snapshot update.

**Action:** Skip steps 3-4 (no re-response needed). Update the snapshot to match the current document state, print "[snapshot recovery] Snapshot synced — previous cycle completed but snapshot was stale", and stop.

**When NOT to recover:** If there is any new user content (new `## User` block with text, inline annotations, edits to previous responses), process normally — the user has new input that needs a response, even if the snapshot is stale. In this case, just update the snapshot baseline to the current document state before computing the real diff against only the new user content.

### 3. Respond 
- Address the user's changes naturally in the console (this gives real-time streaming feedback)
- Respond to:
  - New `## User` blocks
  - Inline annotations (blockquotes, comments, edits to previous responses)
  - Structural changes (deletions, reorganization)
- Your console response IS the document response — they should be the same content

### 4. Write back to the document 
After responding, update the document file:

1. **Re-read the file** (user may have edited during your response)
2. Append your response as:
   ```
   ## Assistant

   <your response>

   ## User

   ```
3. Use the Edit tool to append (not Write — preserves user edits made during response)
4. Update the snapshot to match the new document state

### 5. Git integration (optional)

If the document is in a git repo:
- Before responding: `agent-doc commit <FILE>` (git add + commit with auto-generated timestamp)
- **NEVER use `git commit -m "$(date ...)"` or any `$()` substitution** — always use `agent-doc commit`
- After writing response: do NOT commit (leave as uncommitted for diff gutters)

## Document Format

Session documents use YAML frontmatter:

```yaml
---
session: <uuid or null>
agent: <name or null>
model: <model or null>
branch: <branch or null>
---
```

The body alternates `## User` and `## Assistant` blocks. Inline annotations (blockquotes, comments) within any block are valid prompts.

## Snapshot Storage

- Location: `.agent-doc/snapshots/` relative to the project root (where the document lives)
- Filename: `sha256(canonical_path) + ".md"`
- Contains the full document content after the last submit
- **IMPORTANT:** Always use absolute paths for snapshot read/write operations. CWD may drift to submodule directories during a session.

## Success Criteria

- User sees streaming response in the Claude console
- Document is updated with the response (user can see it in their editor)
- User edits made during response are preserved (not overwritten)
- Snapshot is updated for the next submit's diff computation
