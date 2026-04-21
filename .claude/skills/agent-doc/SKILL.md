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
/agent-doc compact <FILE>
/agent-doc compact exchange <FILE>
```

Arguments: `FILE` — path to the session document (e.g., `plan.md`).

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

**Auto-update skill:** Run `agent-doc --version` and compare against `agent-doc-version` in this file's frontmatter. If the binary is newer, run `agent-doc skill install --reload compact`; if output contains `SKILL_RELOAD=compact`, prompt the user via `AskUserQuestion` to run `/compact` and re-run `/agent-doc`, then stop. If `agent-doc` is missing or versions match, skip.

**Run preflight:** `agent-doc preflight <FILE>` via Bash. Preflight recovers orphaned responses, commits the previous cycle, reads claims, and computes the diff. It prints JSON. Key fields: `no_changes`, `claims`, `diff`, `baseline_file`, `slash_commands`, `builtin_commands`, `effective_tier`, `required_tier`, `model_switch`, `agent_model`, `diff_type`.

- If `no_changes: true` → tell the user nothing changed and stop.
- Print any `claims` to the console as a record.
- Use `baseline_file` as `--baseline-file` for every subsequent `agent-doc write`. Do NOT save your own baseline — preflight's copy is taken at a stable post-commit point.
- First cycle only: if the document is not yet in context, run `agent-doc read <FILE>` to fetch HEAD content. Do NOT read the snapshot file directly.

### 0b. Execute slash commands (if any)

If `slash_commands` or `builtin_commands` is non-empty, handle each **before** responding.

- **Skill commands** (`slash_commands`): invoke each via the `Skill` tool. Strip the leading `/`; pass remaining args. Log `Running: /command args`. Example: `/caveman` → `Skill { skill: "caveman" }`.
- **Built-in commands** (`builtin_commands`): cannot invoke via Skill. Write a document note instructing the user to run it at the terminal (e.g., `/compact`, `/clear`). Skip all others.

Trust the preflight output — do not re-validate code fences or blockquotes.

### 0c. Model tier gate

Preflight composes `effective_tier` from inline `/model`, `<!-- agent:model -->`, frontmatter, and a diff heuristic. Tier handling rules (`required_tier` blocks, `model_switch` is acknowledged, mismatch is advisory): see [runbooks/model-tier-gate.md](runbooks/model-tier-gate.md).

### 1. Respond

- Address the user's changes naturally in the console — the console response IS the document response.
- Respond to new `## User` blocks, inline annotations (blockquotes, comments, edits to previous responses), and structural changes.

**Response header format (template mode):** use `### Re: topic` markdown headers — **not** bold (`**Re:**`). The `(HEAD)` boundary marker requires real headings. Use h4–h6 for sub-sections within a response.

**Model attribution:** always append your own model short name with a spaced em dash: `### Re: topic — opus-4-6`. Use `preflight.agent_model` if non-null (from frontmatter); otherwise use your own model identity (you know what model you are). Never omit the suffix.

**Streaming checkpoints:** for long multi-topic responses, flush partial content at natural breakpoints so the user sees progress. Full procedure + baseline re-save pattern: [runbooks/streaming-checkpoints.md](runbooks/streaming-checkpoints.md).

**Prefer wrapping exchange responses in `<!-- patch:exchange -->`** for clarity. Raw (unwrapped) content also works via boundary synthesis.

### 1b. Update pending (template mode)

If the document has an `<!-- agent:pending -->` component, mutations go through granular flags on `agent-doc write` (`--pending-add`, `--pending-done <id>`, `--pending-edit "id=text"`, `--pending-clear`, `--pending-reorder`, `--pending-gate`, `--pending-ungate`). Full-replace via `<!-- replace:pending -->` or `<!-- patch:pending -->` is rejected. If `pending_reordered: true`, skip reorder this cycle. Full contract: [runbooks/pending-ops.md](runbooks/pending-ops.md).

### 2. Write back

Pipe the response through `agent-doc write --stream` — it handles patch parsing, CRDT merge, atomic write, and snapshot update:

```bash
cat <<'RESPONSE' | agent-doc write <FILE> --baseline-file <preflight.baseline_file> --stream --origin skill
<your response — patch blocks for template mode, or plain text for inline mode>
RESPONSE
```

**IMPORTANT: Do NOT use the Edit tool for write-back.** It is prone to "file modified since read" errors when the user edits concurrently.

Document format, frontmatter fields, append vs template mode conventions, and component naming: [runbooks/document-format.md](runbooks/document-format.md).

### 3. Commit

Immediately after `agent-doc write` succeeds, run `agent-doc commit <FILE>`. The selective commit stages only the snapshot content so the user's working-tree edits stay visible as gutter changes.

**Never use `git commit -m "$(date ...)"` or any `$()` substitution** — always use `agent-doc commit`.

## Runbooks

- [runbooks/compact-exchange.md](runbooks/compact-exchange.md) — `compact exchange` operation
- [runbooks/transfer-extract.md](runbooks/transfer-extract.md) — `transfer` / `extract` operations
- [runbooks/pending-ops.md](runbooks/pending-ops.md) — pending mutation contract
- [runbooks/model-tier-gate.md](runbooks/model-tier-gate.md) — tier precedence + gate behavior
- [runbooks/streaming-checkpoints.md](runbooks/streaming-checkpoints.md) — checkpoint flush pattern
- [runbooks/document-format.md](runbooks/document-format.md) — frontmatter + component conventions
- [runbooks/code-enforced-directives.md](runbooks/code-enforced-directives.md) — which invariants live in the binary

## Success Criteria

- User sees streaming response in the Claude console.
- Document is updated and user's concurrent edits are preserved.
- Snapshot is updated for the next cycle's diff.
