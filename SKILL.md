---
description: Submit a session document to an AI agent and append the response
user-invocable: true
argument-hint: "<file>"
agent-doc-version: "0.9.9"
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

## Workflow

### 0. Pre-flight checks

**Detect claim:** If the first argument is `claim`, run `agent-doc claim <FILE>` via Bash and stop. Do not proceed with the document session workflow. Print the output to confirm the claim.

**Auto-update skill:** Run `agent-doc --version` and compare against the `agent-doc-version` in this file's frontmatter. If the binary version is newer, run `agent-doc skill install` to update this SKILL.md, then continue with the updated instructions. If `agent-doc` is not installed or the version matches, skip this step.

**Recover orphaned responses:** Run `agent-doc recover <FILE>` via Bash. If a pending response exists (from a previous cycle interrupted by context compaction), it will be written to the document automatically. Print the output to confirm recovery. This must run before computing the diff.

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

### 3. Respond

- Address the user's changes naturally in the console (this gives real-time streaming feedback)
- Respond to:
  - New `## User` blocks
  - Inline annotations (blockquotes, comments, edits to previous responses)
  - Structural changes (deletions, reorganization)
- Your console response IS the document response — they should be the same content

### 4. Write back to the document

After responding, use `agent-doc write` to atomically append the response:

1. **Save a baseline copy** of the document content (before step 3) to a temp file
2. **Pipe your response** through `agent-doc write`:
   ```bash
   echo "<your response>" | agent-doc write <FILE> --baseline-file <baseline_tmp>
   ```
3. `agent-doc write` handles:
   - Appending `## Assistant\n\n<response>\n\n## User\n\n`
   - 3-way merging if the user edited during your response
   - Atomic file write (flock + tempfile + rename)
   - Snapshot update

**IMPORTANT:** Do NOT use the Edit tool for write-back. Use `agent-doc write` via Bash.
The Edit tool is prone to "file modified since read" errors when the user edits concurrently.

**Baseline file:** Before generating your response (step 3), save the current document to a temp file:
```bash
cp <FILE> /tmp/agent-doc-baseline-$$.md
```
Then pass it as `--baseline-file` so the 3-way merge can detect user edits accurately.

### 5. Git integration (optional)

If the document is in a git repo:
- Before responding: `agent-doc commit <FILE>` (git add + commit with auto-generated timestamp)
- **NEVER use `git commit -m "$(date ...)"` or any `$()` substitution** — always use `agent-doc commit`
- After writing response: do NOT commit (leave as uncommitted for diff gutters)

## Document Format

Session documents use YAML frontmatter:

```yaml
---
agent_doc_session: <uuid or null>
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
