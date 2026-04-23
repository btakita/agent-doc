---
description: "Interactive document session — respond to user edits in a markdown file. TRIGGER: user invokes /agent-doc <file>. ALL-OF: (1) file is a markdown session document, (2) CLI is installed, (3) write+commit are executed every cycle without exception."
user-invocable: true
argument-hint: "<file>"
agent-doc-version: "0.33.12"
---

# agent-doc

Interactive document session — respond to user edits in a markdown document.

## Harness Compatibility

This skill works across multiple agent harnesses (Claude Code, Codex, Cursor, etc.). The core workflow is identical; only invocation and tool dispatch differ. See [runbooks/harness-invocation.md](runbooks/harness-invocation.md) for harness-specific patterns and [runbooks/commit.md](runbooks/commit.md) for the response commit boundary.

## Invocation

```
/agent-doc <FILE>
/agent-doc claim <FILE>
/agent-doc compact <FILE>
/agent-doc compact exchange <FILE>
```

Arguments: `FILE` — path to the session document (e.g., `plan.md`).

**Note:** Slash commands (`/agent-doc`) are Claude Code-specific. Other harnesses receive the document path directly.
## Core Principles

- **Document is the UI** — the user's edits ARE the prompt; respond in the document AND the console.
- **Preserve user edits** — never overwrite; let `agent-doc write --stream` merge.
- **Show progress** — stream your response in the console so the user sees real-time feedback.

## Workflow

### 0. Pre-flight (single command)

**Detect subcommands** before running the normal workflow:

- `claim <FILE>` → run `agent-doc claim <FILE>` via Bash and stop.
- `compact <FILE>` → run `agent-doc compact <FILE>` then `agent-doc commit <FILE>` and stop.
- `compact exchange <FILE>` → follow [runbooks/compact-exchange.md](runbooks/compact-exchange.md) and stop.

**Auto-update skill:** Run `agent-doc --version` and compare against `agent-doc-version` in this file's frontmatter. If the binary is newer, run `agent-doc skill install --reload compact`; if output contains `SKILL_RELOAD=compact`, prompt the user to run `/compact` (or equivalent) and re-invoke the skill, then stop. If `agent-doc` is missing or versions match, skip. See [runbooks/harness-invocation.md](runbooks/harness-invocation.md) for harness-specific prompting.

**Run preflight:** `agent-doc preflight <FILE>` via Bash. Preflight recovers orphaned responses, auto-attempts recovery+commit for open `response_captured` / `write_applied` cycles, and only auto-closes an open `preflight_started` cycle when `recover` replays a pending/captured response first; otherwise it fails closed before diffing. It also reads claims and computes the diff. It prints JSON. Key fields: `no_changes`, `claims`, `diff`, `baseline_file`, `slash_commands`, `builtin_commands`, `effective_tier`, `required_tier`, `suggested_tier`, `model_switch`, `model_switch_tier`, `agent_model`, `diff_type`.

- If `no_changes: true` → tell the user nothing changed and stop.
- Print any `claims` to the console as a record.
- Use `baseline_file` as `--baseline-file` for every subsequent response-persistence command. Do NOT save your own baseline — preflight's copy is taken at a stable post-commit point.
- First cycle only: if the document is not yet in context, run `agent-doc read <FILE>` to fetch HEAD content. Do NOT read the snapshot file directly.

### 0b. Execute slash commands (if any)

If `slash_commands` or `builtin_commands` is non-empty, handle each **before** responding. See [runbooks/harness-invocation.md](runbooks/harness-invocation.md) for harness-specific dispatch.

- **Skill commands** (`slash_commands`): Claude Code invokes via the `Skill` tool (strip leading `/`, pass remaining args). Other harnesses skip these.
- **Built-in commands** (`builtin_commands`): Write a document note instructing the user to run it at the terminal (e.g., `/compact`, `/clear`). Skip all others.

Trust the preflight output — do not re-validate code fences or blockquotes.

### 0c. Model tier gate

Preflight composes `effective_tier` from inline `/model`, `<!-- agent:model -->`, frontmatter, and a diff heuristic. `required_tier` is the hard gate, `suggested_tier` is advisory, and `model_switch_tier` is the resolved tier for the user's inline `/model` request. Full gate behavior: [runbooks/model-tier-gate.md](runbooks/model-tier-gate.md).

### 1. Respond

- Address the user's changes naturally in the console — the console response IS the document response.
- Respond to new `## User` blocks, inline annotations (blockquotes, comments, edits to previous responses), and structural changes.

**Response header format (template mode):** use `### Re: topic` markdown headers — **not** bold (`**Re:**`). Exchange ordering and model attribution expect real headings. Use h4–h6 for sub-sections within a response.

**Model attribution:** always append the resolved model short name with a spaced em dash: `### Re: topic — gpt-5` or `### Re: topic — opus-4-6`. Use `preflight.agent_model` if non-null (from frontmatter); otherwise use your own model identity (you know what model you are). Never use the harness label (`codex`, `claude`) as the suffix, and never omit it.

**Streaming checkpoints:** for long multi-topic responses, flush partial content at natural breakpoints so the user sees progress. Full procedure + baseline re-save pattern: [runbooks/streaming-checkpoints.md](runbooks/streaming-checkpoints.md).

**Prefer wrapping exchange responses in `<!-- patch:exchange -->`** for clarity. Raw (unwrapped) content also works via boundary synthesis.

### 1b. Update pending (template mode)

If the document has an `<!-- agent:pending -->` component, mutations go through granular flags on `agent-doc write` (`--pending-add`, `--pending-done <id>`, `--pending-edit "id=text"`, `--pending-clear`, `--pending-reorder`, `--pending-gate`, `--pending-ungate`). Full-replace via `<!-- replace:pending -->` or `<!-- patch:pending -->` is rejected. If `pending_reordered: true`, skip reorder this cycle. Full contract: [runbooks/pending-ops.md](runbooks/pending-ops.md).

**Pending capture rule:** if your response identifies concrete follow-up work that should be tracked, add it to `agent:pending` in the same cycle. Do not leave pending-worthy next steps as exchange-only prose.

**Recommendations vs accepted work:** if the item is only a recommendation and the user has not accepted it yet, include `recommended` in the pending text (for example, `[recommended] Add regression coverage for X`). Do not silently promote hypotheticals or mutually exclusive options the user has not chosen.

**Default ordering:** new pending items belong at the beginning of `agent:pending`. When adding multiple new items in one cycle, preserve the order you presented them in. If the user explicitly reorders pending and preflight reports `pending_reordered: true`, do not reorder existing items that cycle beyond the front insertion behavior for genuinely new work.

**Promotion heuristic (when to add pending in the same cycle):** if your response ends with a numbered list of distinct, actionable recommendations (e.g., "What I'd recommend: 1. ..., 2. ..., 3. ..."), and either pending is currently empty OR the user asked for a backlog / "tasks" / "todo", add each recommendation as a pending item in the same cycle. The pending component is where backlog lives. Skip promotion when items are hypotheticals, options you're asking the user to choose between, or already captured elsewhere.

### 2. Persist the response (MANDATORY — never skip)

Complete the requested implementation, verification, build/install, and local inspection work for this turn **before** this step. The response persistence command is the final document-mutation boundary for the cycle, not an intermediate progress checkpoint.

**Agent harnesses own full-suite verification:** if you changed code, tests, build logic, or instruction surfaces, run the full project verification suite explicitly after the edits and before `finalize` / `write --commit`. Do not rely on a pre-commit hook to do this for you.

For the normal response cycle, pipe the response through `agent-doc finalize --stream` so the write crosses the commit boundary in one binary-owned path. **This step is MANDATORY every cycle unless the user explicitly told you to leave the response uncommitted.**

```bash
cat <<'RESPONSE' | agent-doc finalize <FILE> --baseline-file <preflight.baseline_file> --stream --origin skill
<your response — patch blocks for template mode, or plain text for inline mode>
RESPONSE
```

`finalize` reuses the normal write pipeline, then requires the cycle to reach `committed`. Use [runbooks/commit.md](runbooks/commit.md) for the default/exception contract.

**End-of-turn guard:** after any final response persistence command (`agent-doc finalize <FILE> ...` for the normal path, or `agent-doc write --commit <FILE> ...` for manual repair), run `agent-doc session-check <FILE>`. If it exits nonzero, the cycle is still open or the document shows a likely direct assistant patchback that bypassed `agent-doc`: do **not** report success, and continue recovery instead of ending the turn. The only self-heal exception is already-committed historical snapshot drift that `session-check` can prove from `HEAD`.

After `finalize` / `write --commit`, do not start more long-running task work for that same turn. The only allowed follow-up is the immediate `session-check`, minimal recovery if it fails, and concise result reporting.

**Codex hook backstop:** the Codex install also writes `.codex/hooks.json` plus `.codex/config.toml` with `features.codex_hooks = true`. Those `UserPromptSubmit` / `Stop` hooks track the active `agent-doc` file. On `Stop`, the hook first tries to finish the response cycle deterministically from `last_assistant_message` by replaying the normal write/commit path; if the cycle still cannot be closed, it falls back to capture-and-block / fail-closed behavior instead of silently letting a bypassed patchback leave the harness. Treat that as a safety backstop, not a replacement for explicitly running `finalize` / `write --commit` and `session-check`.

**IMPORTANT: Do NOT use the Edit tool for write-back.** It is prone to "file modified since read" errors when the user edits concurrently.

**IMPORTANT: The response content MUST include `<!-- patch:exchange -->` blocks for template-mode documents.** If the heredoc is empty or contains only raw text without patch markers, the binary will warn (`0 template patches found`) and only apply normalization — the response will be silently lost. Context compaction can drop the response between generation and the write command; if this happens, re-generate the response before piping.

**Manual repair / missed patchback rule (all harnesses):** if the user's prompt is already present in the document and you are repairing a missed patchback, do **not** patch the assistant response directly into the file. Use `agent-doc write --commit <FILE>` for the response write-back so the repair crosses the normal snapshot/commit boundary in one path. Do not document or follow a manual-repair flow that stops after bare `agent-doc write`. Direct file patching is only acceptable for inserting a missing user prompt into `exchange` before the response exists in the document.

Document format, frontmatter fields, append vs template mode conventions, and component naming: [runbooks/document-format.md](runbooks/document-format.md). Commit-boundary exceptions and anti-patterns live in [runbooks/commit.md](runbooks/commit.md).

## Runbooks

- [runbooks/harness-invocation.md](runbooks/harness-invocation.md) — harness-specific invocation patterns
- [runbooks/compact-exchange.md](runbooks/compact-exchange.md) — `compact exchange` operation
- [runbooks/transfer-extract.md](runbooks/transfer-extract.md) — `transfer` / `extract` operations
- [runbooks/pending-ops.md](runbooks/pending-ops.md) — pending mutation contract
- [runbooks/model-tier-gate.md](runbooks/model-tier-gate.md) — tier precedence + gate behavior
- [runbooks/streaming-checkpoints.md](runbooks/streaming-checkpoints.md) — checkpoint flush pattern
- [runbooks/document-format.md](runbooks/document-format.md) — frontmatter + component conventions
- [runbooks/commit.md](runbooks/commit.md) — response commit boundary and exceptions
- [runbooks/code-enforced-directives.md](runbooks/code-enforced-directives.md) — which invariants live in the binary

## Success Criteria

- User sees streaming response in the agent console.
- Document is updated and user's concurrent edits are preserved.
- Snapshot is updated for the next cycle's diff.
