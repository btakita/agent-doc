---
description: Submit a session document to an AI agent and append the response
user-invocable: true
argument-hint: "<file>"
agent-doc-version: "0.28.0"
---

# agent-doc

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

### 0. Pre-flight (single command)

**Detect claim:** If the first argument is `claim`, run `agent-doc claim <FILE>` via Bash and stop. Do not proceed with the document session workflow. Print the output to confirm the claim.

**Auto-update skill:** Run `agent-doc --version` and compare against the `agent-doc-version` in this file's frontmatter. If the binary version is newer, run `agent-doc skill install --reload compact` to update this SKILL.md. If the output contains `SKILL_RELOAD=compact`, use `AskUserQuestion` to prompt the user: "SKILL.md was updated. Run /compact to reload the skill, then re-run /agent-doc." Stop and do not proceed with the document session. If `agent-doc` is not installed or the version matches, skip this step.

**Run preflight:** Execute `agent-doc preflight <FILE>` via Bash. This single command handles:
- Recovering orphaned responses (from interrupted cycles)
- Committing previous cycle changes (git gutter management)
- Reading and truncating the claims log
- Computing the diff (with comment stripping)
- Reading the document HEAD

The command outputs JSON to stdout:
```json
{
  "recovered": false,
  "committed": true,
  "claims": [],
  "diff": "unified diff text or null",
  "no_changes": false,
  "document": "full document content"
}
```

- If `no_changes` is `true`, tell the user nothing changed and stop
- Print any `claims` entries to the console as a record
- The `document` field contains the full HEAD content (no separate `Read` needed)
- The `diff` field contains the user's changes since the last snapshot
- **Do NOT read the snapshot file directly** — the preflight output provides everything needed

### 1. Respond (with streaming checkpoints for template mode)

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
   cat <<'RESPONSE' | agent-doc write <FILE> --baseline-file <baseline_tmp> --stream
   <partial response as patch blocks>
   RESPONSE
   ```
2. **Re-save the baseline** after each checkpoint flush (the document has changed):
   ```bash
   cp <FILE> /tmp/agent-doc-baseline-$$.md
   ```
3. Continue responding to the next section, then flush again
4. The final write-back (step 2) writes the complete response

**When to checkpoint:** After each `### Re:` section, after completing a code implementation summary, or after any response block that takes >15s to generate. Skip checkpoints for short single-topic responses.

**All writes use `--stream` (CRDT merge)** — this eliminates merge conflicts when the user edits the document during response generation.

### 1b. Update pending (template mode)

If the document has an `<!-- agent:pending -->` component, **every response MUST include a `<!-- patch:pending -->` block** reflecting the current state. This is not optional — pending drift makes task lists unreliable.

**What to update each cycle:**
- Items completed during this response → remove or mark `([done])`
- New items discovered → add with context
- Active work items → move to top
- Reprioritize based on current work direction

### 2. Write back to the document

Check the document's `agent_doc_mode` frontmatter field (aliases: `mode`, `response_mode`).

#### 2a. Append mode (default — no `agent_doc_mode` or `agent_doc_mode: append`)

Use `agent-doc write --stream` to atomically append the response:

1. **Save a baseline copy** of the document content (before step 1) to a temp file
2. **Pipe your response** through `agent-doc write`:
   ```bash
   cat <<'RESPONSE' | agent-doc write <FILE> --baseline-file <baseline_tmp> --stream
   <your response>
   RESPONSE
   ```
3. `agent-doc write --stream` handles:
   - Appending `## Assistant\n\n<response>\n\n## User\n\n`
   - CRDT merge if the user edited during your response (conflict-free)
   - Atomic file write (flock + tempfile + rename)
   - Snapshot update

#### 2b. Template mode (`agent_doc_mode: template`)

Template-mode documents use named components (`<!-- agent:name -->...<!-- /agent:name -->`).
The agent responds with **patch blocks** that target specific components.

1. **Save a baseline copy** of the document content (before step 1) to a temp file
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
   cat <<'RESPONSE' | agent-doc write <FILE> --baseline-file <baseline_tmp> --stream
   <your patch response>
   RESPONSE
   ```
4. `agent-doc write --stream` handles:
   - Parsing patch blocks from the response
   - Applying each patch to the matching component
   - CRDT merge if the user edited during your response (conflict-free)
   - Atomic file write + snapshot update

**Template document conventions:**
- `<!-- agent:input -->` — user writes prompts here
- `<!-- agent:output -->` — agent responds here (or use patch blocks for multiple components)
- `<!-- agent:exchange -->` — shared conversation surface (user and agent both write inline)
- Other components (status, architecture, etc.) are agent-managed via patch blocks

**IMPORTANT:** Do NOT use the Edit tool for write-back. Use `agent-doc write` via Bash.
The Edit tool is prone to "file modified since read" errors when the user edits concurrently.

**Baseline file:** Before generating your response (step 1), save the current document to a temp file:
```bash
cp <FILE> /tmp/agent-doc-baseline-$$.md
```
Then pass it as `--baseline-file` so the 3-way merge can detect user edits accurately.

### 3. Git integration

**Commit immediately after writing the response.** After `agent-doc write` completes, run `agent-doc commit <FILE>`. The selective commit stages only the snapshot content (agent response), leaving user edits in the working tree as uncommitted. This gives the user:
- Agent response → committed (no gutter)
- Heading with `(HEAD)` marker → modified (blue gutter, visual boundary)
- User's new input → uncommitted (green gutter)

- **NEVER use `git commit -m "$(date ...)"` or any `$()` substitution** — always use `agent-doc commit`
- Step 0b also calls `agent-doc commit` as a safety net (no-op if already committed)

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

**Template mode** (`agent_doc_mode: template`): The body contains named components (`<!-- agent:name -->...<!-- /agent:name -->`). The agent responds with patch blocks targeting specific components. See step 2b.

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
