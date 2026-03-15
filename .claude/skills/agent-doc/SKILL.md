---
description: Submit a session document to an AI agent and append the response
user-invocable: true
argument-hint: "<file>"
agent-doc-version: "0.17.5"
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

**Auto-update skill:** Run `agent-doc --version` and compare against the `agent-doc-version` in this file's frontmatter. If the binary version is newer, run `agent-doc skill install --reload compact` to update this SKILL.md. If the output contains `SKILL_RELOAD=compact`, tell the user "Skill updated — run /compact to reload" and stop (do not proceed with the document session). If `agent-doc` is not installed or the version matches, skip this step.

**Recover orphaned responses:** Run `agent-doc recover <FILE>` via Bash. If a pending response exists (from a previous cycle interrupted by context compaction), it will be written to the document automatically. Print the output to confirm recovery. This must run before computing the diff.

**Check claims log:** Read `.agent-doc/claims.log` (if it exists) as a **foreground** Bash call — never use `run_in_background` (instant commands cause phantom task accumulation). Print each line to the console as a record of IDE-triggered claims. Then truncate the file (write empty string). This gives a permanent record in the Claude session of claims made from the editor plugin.

### 0b. Commit previous response

Before reading the document, commit any uncommitted changes from the previous cycle. This keeps the git gutter clean for the new response while preserving green gutter visibility for the *last* response between cycles:

```bash
agent-doc commit <FILE>
```

If there are no uncommitted changes, this is a no-op.

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

### 3. Respond (with streaming checkpoints for template mode)

- Address the user's changes naturally in the console (this gives real-time streaming feedback)
- Respond to:
  - New `## User` blocks
  - Inline annotations (blockquotes, comments, edits to previous responses)
  - Structural changes (deletions, reorganization)
- Your console response IS the document response — they should be the same content

**Streaming checkpoints (template/stream mode):**
When responding to a document with multiple user questions/topics, flush partial responses at natural breakpoints so the user sees progress in their editor:

1. After completing each logical section (e.g., answering one question), flush the accumulated response so far:
   ```bash
   echo '<partial response as patch blocks>' | agent-doc write <FILE> --baseline-file <baseline_tmp> --stream
   ```
2. **Re-save the baseline** after each checkpoint flush (the document has changed):
   ```bash
   cp <FILE> /tmp/agent-doc-baseline-$$.md
   ```
3. Continue responding to the next section, then flush again
4. The final write-back (step 4) writes the complete response

**When to checkpoint:** After each `### Re:` section, after completing a code implementation summary, or after any response block that takes >15s to generate. Skip checkpoints for short single-topic responses.

**All writes use `--stream` (CRDT merge)** — this eliminates merge conflicts when the user edits the document during response generation.

### 4. Write back to the document

Check the document's `agent_doc_mode` frontmatter field (aliases: `mode`, `response_mode`).

#### 4a. Append mode (default — no `agent_doc_mode` or `agent_doc_mode: append`)

Use `agent-doc write --stream` to atomically append the response:

1. **Save a baseline copy** of the document content (before step 3) to a temp file
2. **Pipe your response** through `agent-doc write`:
   ```bash
   echo "<your response>" | agent-doc write <FILE> --baseline-file <baseline_tmp> --stream
   ```
3. `agent-doc write --stream` handles:
   - Appending `## Assistant\n\n<response>\n\n## User\n\n`
   - **IPC-first write:** tries IPC to JetBrains/VS Code plugin (via `.agent-doc/patches/`) before falling back to direct disk write — avoids "externally modified" dialogs and preserves cursor/undo state
   - CRDT merge if the user edited during your response (conflict-free)
   - Atomic file write (flock + tempfile + rename) on IPC fallback
   - Snapshot update

#### 4b. Template mode (`agent_doc_mode: template`)

Template-mode documents use named components (`<!-- agent:name -->...<!-- /agent:name -->`).
The agent responds with **patch blocks** that target specific components.

1. **Save a baseline copy** of the document content (before step 3) to a temp file
2. **Format your response as patch blocks:**
   ```markdown
   <!-- patch:output -->
   Your response content here.
   <!-- /patch:output -->

   <!-- patch:status -->
   Updated status line.
   <!-- /patch:status -->
   ```
   - Each `<!-- patch:name -->` targets the corresponding `<!-- agent:name -->` component
   - Content outside patch blocks goes to `<!-- agent:output -->` (auto-created if missing)
   - Component modes (replace/append/prepend) are configured in `.agent-doc/components.toml`
3. **Pipe through `agent-doc write` with `--stream` flag:**
   ```bash
   echo "<your patch response>" | agent-doc write <FILE> --baseline-file <baseline_tmp> --stream
   ```
4. `agent-doc write --stream` handles:
   - Parsing patch blocks from the response
   - Applying each patch to the matching component
   - **IPC-first write:** tries IPC to IDE plugin before falling back to direct disk write
   - CRDT merge if the user edited during your response (conflict-free)
   - Atomic file write + snapshot update (on IPC fallback)

**Template document conventions:**
- `<!-- agent:input -->` — user writes prompts here
- `<!-- agent:output -->` — agent responds here (or use patch blocks for multiple components)
- `<!-- agent:exchange -->` — shared conversation surface (user and agent both write inline)
- Other components (status, architecture, etc.) are agent-managed via patch blocks

**IMPORTANT:** Do NOT use the Edit tool for write-back. Use `agent-doc write` via Bash.
The Edit tool is prone to "file modified since read" errors when the user edits concurrently.

**Baseline file:** Before generating your response (step 3), save the current document to a temp file:
```bash
cp <FILE> /tmp/agent-doc-baseline-$$.md
```
Then pass it as `--baseline-file` so the 3-way merge can detect user edits accurately.

### 5. Git integration

**Do NOT commit after writing the response.** The response stays uncommitted so the green git gutter bar delineates exactly what the agent wrote. The commit happens at the *start* of the next cycle (step 0b).

- **NEVER use `git commit -m "$(date ...)"` or any `$()` substitution** — always use `agent-doc commit`
- The previous response is committed in step 0b, before the next diff computation
- This gives the user green gutter visibility between cycles, with automatic cleanup on the next submit

## Document Format

Session documents use YAML frontmatter:

```yaml
---
agent_doc_session: <uuid or null>
agent: <name or null>
model: <model or null>
branch: <branch or null>
agent_doc_mode: <append | template>  # optional, default: append
---
```

**Append mode** (default): The body alternates `## User` and `## Assistant` blocks. Inline annotations (blockquotes, comments) within any block are valid prompts.

**Template mode** (`agent_doc_mode: template`): The body contains named components (`<!-- agent:name -->...<!-- /agent:name -->`). The agent responds with patch blocks targeting specific components. See step 4b.

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
