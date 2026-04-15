---
description: Submit a session document to an AI agent and append the response
user-invocable: true
argument-hint: "<file>"
agent-doc-version: "0.32.0"
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

**Detect compact:** If the first argument is `compact`, handle compaction and stop:
- `compact exchange <FILE>` → read and follow the compact exchange runbook: [runbooks/compact-exchange.md](runbooks/compact-exchange.md)
- `compact <FILE>` → run `agent-doc compact <FILE>` then `agent-doc commit <FILE>`

Do not proceed with the normal document session workflow after handling compact.

**Auto-update skill:** Run `agent-doc --version` and compare against the `agent-doc-version` in this file's frontmatter. If the binary version is newer, run `agent-doc skill install --reload compact` to update this SKILL.md. If the output contains `SKILL_RELOAD=compact`, use `AskUserQuestion` to prompt the user: "SKILL.md was updated. Run /compact to reload the skill, then re-run /agent-doc." Stop and do not proceed with the document session. If `agent-doc` is not installed or the version matches, skip this step.

**Run preflight:** Execute `agent-doc preflight <FILE>` via Bash. This single command handles:
- Recovering orphaned responses (from interrupted cycles)
- Committing previous cycle changes (git gutter management)
- Reading and truncating the claims log
- Computing the diff (with comment stripping)

The command outputs JSON to stdout:
```json
{
  "recovered": false,
  "committed": true,
  "claims": [],
  "diff": "unified diff text or null",
  "no_changes": false,
  "baseline_file": "/path/to/.agent-doc/baselines/<hash>.md",
  "slash_commands": [],
  "builtin_commands": [],
  "effective_tier": "med",
  "required_tier": null,
  "suggested_tier": "med",
  "model_switch": null,
  "model_switch_tier": null
}
```

- If `no_changes` is `true`, tell the user nothing changed and stop
- Print any `claims` entries to the console as a record
- The `diff` field contains the user's changes since the last snapshot
- **`baseline_file`** — use this path as `--baseline-file` when calling `agent-doc write`. It is taken AFTER preflight commits any gap, so it always reflects the stable pre-response file state. Do NOT save your own baseline before preflight — a pre-preflight copy may be stale if a concurrent write occurred between the copy and preflight.
- **First cycle only:** if the document is not yet in context, run `agent-doc read <FILE>` to fetch HEAD content before responding
- **Do NOT read the snapshot file directly** — use `agent-doc read <FILE>` if HEAD content is needed

### 0b. Execute slash commands (if any)

If preflight returns a non-empty `slash_commands` or `builtin_commands` array, handle each **before** generating your response.

**Skill commands** (`slash_commands` — non-built-ins): Invoke each via the `Skill` tool:
- Strip the leading `/` to get the skill name and args
- Log each: `Running: /command args`
- Example: `/agent-doc ./plan.md` → `Skill { skill: "agent-doc", args: "./plan.md" }`
- Example: `/caveman` → `Skill { skill: "caveman" }`

**Built-in commands** (`builtin_commands` — Claude Code built-ins): Cannot invoke via Skill. Write a note to the document for each:
- `/compact` → document note: "Run `/compact` at the terminal to compact the conversation context."
- `/clear` → document note: "Run `/clear` at the terminal to clear the conversation history."
- All other built-ins → log to console and skip (cannot execute Claude Code built-ins from within the session)

**Guards:** `parse_slash_commands` already filtered out code-fenced lines, blockquotes, and non-added lines — trust the output. Do not re-validate.

### 0c. Model tier gate (harness-agnostic)

Preflight composes an `effective_tier` (`low | med | high`) from these sources, highest precedence first:

1. Inline `/model <x>` command in the diff (stripped from downstream diff)
2. `<!-- agent:model -->` component content
3. `agent_doc_model_tier` frontmatter field
4. Diff heuristic (`suggested_tier`)

Use the tier to gate execution:

- If `model_switch` is set (user typed `/model <x>`), acknowledge the switch in your response and proceed. The user explicitly chose this tier.
- If `required_tier` is set (from component or frontmatter) and the current session's tier is **lower** than `required_tier`, tell the user:
  > "This document requires `<required_tier>` tier. Run `/model <x>` in your terminal, then re-run `/agent-doc`."
  and stop. Do not generate a response.
- Otherwise, compare `effective_tier` against the current session model. If you (the running agent) are a lower tier than `effective_tier` and the task looks substantial (`diff_type` ∈ {`approval`, `implementation`, `architecture`}, or long diff), suggest escalating in the response but still proceed best-effort.
- `effective_tier = auto` means "no preference" — always proceed.

The tier comparison is advisory within a session — the skill cannot switch the running model, but it can surface the mismatch so the user can `/model` and re-invoke.

### 1. Respond (with streaming checkpoints for template mode)

- Address the user's changes naturally in the console (this gives real-time streaming feedback)
- Respond to:
  - New `## User` blocks
  - Inline annotations (blockquotes, comments, edits to previous responses)
  - Structural changes (deletions, reorganization)
- Your console response IS the document response — they should be the same content

**Response header format (template mode):**
Use `### Re:` markdown headers for response sections — NOT bold text (`**Re: ...**`). The `(HEAD)` boundary marker requires actual markdown headings to function. Bold text pseudo-headers break the gutter boundary.

Markdown supports h1-h6. In template mode exchange components (typically under `## Exchange`), use `### Re:` (h3) for response headers. This leaves h4-h6 for sub-sections within each response.

**Model attribution in `### Re:` headers:**
When `preflight.agent_model` is set (non-null), append it to each `### Re:` header with a spaced em dash:
```
### Re: topic — sonnet-4-6
```
When `agent_model` is null or absent, use the plain format `### Re: topic` without attribution. This provides at-a-glance model tracking directly in the document history.

**Streaming checkpoints (template/stream mode):**
When responding to a document with multiple user questions/topics, flush partial responses at natural breakpoints so the user sees progress in their editor:

1. After completing each logical section (e.g., answering one question), flush the accumulated response so far:
   ```bash
   cat <<'RESPONSE' | agent-doc write <FILE> --baseline-file <baseline_tmp> --stream --origin skill
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

**Preferred: wrap exchange responses in `<!-- patch:exchange -->`:** For template-mode documents, wrapping responses in `<!-- patch:exchange -->` targets the component directly and is the cleanest path. Raw (unwrapped) content goes through boundary-synthesis, which is also correct — the binary deduplicates and clears `unmatched` when synthesis occurs. Both paths are valid; explicit wrapping is preferred for clarity.

### 1b. Update pending (template mode) — granular ops only

If the document has an `<!-- agent:pending -->` component, mutations go through **granular
flags** on `agent-doc write` (`--pending-add`, `--pending-done <id>`, `--pending-edit
"id=text"`, `--pending-clear`, `--pending-reorder`, `--pending-gate`, `--pending-ungate`).
Full-replace via `<!-- replace:pending -->` or `<!-- patch:pending -->` is **forbidden** in
normal cycles — the binary rejects them.

Preflight lazy-backfills stable 4-char hash IDs and GFM checkboxes on items that lack them.
If preflight returns `pending_reordered: true`, skip reorder this cycle — respect the user's
expressed priority for at least one cycle.

**Full contract** (item shape, all flags, escape hatch, multi-flag example):
[runbooks/pending-ops.md](runbooks/pending-ops.md). See also
`src/agent-doc/specs/pending-system.md` for the spec.

### 2. Write back to the document

Check the document's `agent_doc_mode` frontmatter field (aliases: `mode`, `response_mode`).

#### 2a. Append mode (default — no `agent_doc_mode` or `agent_doc_mode: append`)

Use `agent-doc write --stream` to atomically append the response:

1. **Save a baseline copy** of the document content (before step 1) to a temp file
2. **Pipe your response** through `agent-doc write`:
   ```bash
   cat <<'RESPONSE' | agent-doc write <FILE> --baseline-file <baseline_tmp> --stream --origin skill
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
   cat <<'RESPONSE' | agent-doc write <FILE> --baseline-file <baseline_tmp> --stream --origin skill
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

**Baseline file:** Use the `baseline_file` path from the preflight JSON output as `--baseline-file`. Do NOT save your own baseline before preflight — a pre-preflight copy may be stale if the file was modified concurrently between the copy and preflight. The preflight-saved baseline is always taken at a stable post-commit point.

For streaming checkpoints, re-save the baseline after each flush:
```bash
cp <FILE> /tmp/agent-doc-baseline-$$.md
```
Pass this updated copy as `--baseline-file` for subsequent checkpoint writes.

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

## Runbooks

When the user requests one of these operations, read and follow the linked runbook:

- `compact exchange` — [runbooks/compact-exchange.md](runbooks/compact-exchange.md)
- `transfer` / `extract` — [runbooks/transfer-extract.md](runbooks/transfer-extract.md)
- pending mutations (granular ops contract) — [runbooks/pending-ops.md](runbooks/pending-ops.md)

## Success Criteria

- User sees streaming response in the Claude console
- Document is updated with the response (user can see it in their editor)
- User edits made during response are preserved (not overwritten)
- Snapshot is updated for the next submit's diff computation
