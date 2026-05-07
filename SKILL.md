---
description: "Interactive document session — respond to user edits in a markdown file. TRIGGER: user invokes /agent-doc <file>. ALL-OF: (1) file is a markdown session document, (2) CLI is installed, (3) write+commit are executed every cycle without exception."
user-invocable: true
argument-hint: "<file>"
agent-doc-version: "0.33.16"
---

# agent-doc

Interactive document session — respond to user edits in a markdown document.

## Harness Compatibility

This skill works across Claude Code, Codex, Cursor, and direct harnesses. The workflow is shared; only invocation and command dispatch differ. Use [runbooks/harness-invocation.md](runbooks/harness-invocation.md) for harness details and [runbooks/commit.md](runbooks/commit.md) for closeout.

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
- **Harness-native `agent-doc` entrypoints start the binary-owned response cycle** — treat `/agent-doc <FILE>`, `agent-doc <FILE>`, or equivalent as executable workflow start, not a generic document-editing request. Do not manually patch the final assistant response into the document, and do not report success before `agent-doc finalize <FILE>` or `agent-doc write --commit <FILE>` completes. For Codex/direct-exec paths, run `agent-doc session-check <FILE>` before ending the turn.
- **Imperative edits are executable directives** — `do #id`, `fix this`, `run tests`, `build + install`, `commit + push`, and similar document edits authorize the underlying repo work. Do not require the same instruction to be repeated in chat.
- **MCP auth / OAuth steps are sub-steps, not closeout boundaries** — if a tool pauses for authentication or browser approval, resume the same managed turn and still finish through `finalize` / `write --commit` plus `session-check`.
- **Manual repo commits must keep the session document on the finalize path** — stage and commit only the intended non-session repo files first, stop on any stage failure, verify the staged diff still matches the intended path set, then let `agent-doc finalize <FILE>` or `agent-doc write --commit <FILE>` own the session-document closeout commit.
- **Rare routing / tmux / startup-miss invariants live in the runbooks and repo docs** — consult [runbooks/harness-invocation.md](runbooks/harness-invocation.md), [runbooks/commit.md](runbooks/commit.md), and [runbooks/code-enforced-directives.md](runbooks/code-enforced-directives.md) when the turn touches route/start/sync/session-check edge cases or sibling `src/tmux-router` work.
- **Preserve user edits** — never overwrite; let `agent-doc write --stream` merge.
- **Show progress** — stream useful status in the console.

## Workflow

### 0. Pre-flight

**Detect subcommands** before running the normal workflow:

- `claim <FILE>` → run `agent-doc claim <FILE>` via Bash and stop.
- `compact <FILE>` → run `agent-doc compact <FILE> --commit` and stop.
- `compact exchange <FILE>` → follow [runbooks/compact-exchange.md](runbooks/compact-exchange.md) and stop.

**Auto-update skill:** Run `agent-doc --version` and compare against `agent-doc-version` in this file's frontmatter. If the binary is newer, run the install command for the active harness: Claude Code `agent-doc skill install --harness claude --reload compact`; Codex `agent-doc skill install --harness codex --reload restart`; other harnesses `agent-doc skill install`. If the install output says the active harness instructions are already up to date, treat this file as stale duplicate instructions, use the installed harness-specific instructions as authoritative for the rest of the turn, and continue with the task. Only stop when the command prints a real `SKILL_RELOAD=...` marker for the active harness. See [runbooks/harness-invocation.md](runbooks/harness-invocation.md) for harness-specific prompting.

**Run preflight:** `agent-doc preflight <FILE>` via Bash. Preflight owns recovery before diffing and prints the JSON contract for the cycle. Use `baseline_file`, `no_changes`, `claims`, `slash_commands`, `builtin_commands`, `orchestration_request`, `prompt_presets_requested`, tier/model fields, `agent_model`, `diff_type`, and the diff contract.

- If `no_changes: true` → tell the user nothing changed and stop.
- Print any `claims` to the console as a record.
- Use `baseline_file` as `--baseline-file` for every subsequent response-persistence command. Do NOT save your own baseline — preflight's copy is taken at a stable post-commit point.
- First cycle only: if the document is not yet in context, run `agent-doc read <FILE>` to fetch HEAD content. Do NOT read the snapshot file directly.

### 0b. Slash Commands

If `slash_commands` or `builtin_commands` is non-empty, handle them **before** responding. Claude Code invokes `slash_commands` via the `Skill` tool; other harnesses skip them. For `builtin_commands`, write a note telling the user to run the command at the terminal. Trust the preflight output — do not re-validate code fences or blockquotes. If `orchestration_request` is non-null, run `agent-doc orchestrate <FILE> --mode <orchestration_request.mode> --from-exchange` before composing any manual response. If `prompt_presets_requested` is non-empty, treat that as already validated and let `orchestrate` expand the presets.

### 0c. Model Tier

Read `effective_tier`, `required_tier`, `suggested_tier`, and `model_switch_tier` from preflight and follow [runbooks/model-tier-gate.md](runbooks/model-tier-gate.md). For natural-language batching such as `run these in order`, `fan out`, or `after #a do #b`, prefer the binary-owned `orchestration_request`; otherwise use [runbooks/command-synonyms.md](runbooks/command-synonyms.md) and [runbooks/compound-task-steering.md](runbooks/compound-task-steering.md).

### 0d. Plan / Dispatch

After `preflight`, run `agent-doc plan <FILE>` and treat its `prompt_targets`, `execution_scope`, `repo_actions`, `required_commands`, `pending_mutations`, `handoff`, and `blockers` as the execution contract. If `blockers` is non-empty, stop. If `handoff=orchestrate`, run the emitted `agent-doc orchestrate ...` command before any manual response. If `handoff=compact`, follow the emitted compact/restart instruction and stop before repo work or response finalization. Full contract: [runbooks/planning-dispatch.md](runbooks/planning-dispatch.md).

### 1. Respond

- Address the user's changes naturally in the console; that response is the document response.
- Reconcile the changed exchange tail oldest-first. Do not stop at the newest question; each unresolved prompt in that tail and each unresolved `prompt_target` must be answered or grouped, and `content_edit` items are user corrections.
- If session-accretion supplies bounded context, use the included `### Re:` blocks as prompt-position anchors, not proof that older turns are absent.
- Execute from the planning record. If `execution_scope=plan_backlog_only`, stay in plan/backlog capture mode. Otherwise complete the requested repo work before persistence or stop on a blocker. Do not keep appending "starting/continuing" status prose while the requested work remains undone.

**Response header format (template mode):** use `### Re: topic` markdown headers — **not** bold (`**Re:**`). The `(HEAD)` boundary marker requires real headings. Use h4–h6 for sub-sections within a response.

**Model attribution:** always append the resolved model short name with a spaced em dash: `### Re: topic — gpt-5` or `### Re: topic — opus-4-6`. Use `preflight.agent_model` if non-null (from frontmatter); otherwise use your own model identity (you know what model you are). Never use the harness label (`codex`, `claude`) as the suffix, and never omit it.

**Streaming checkpoints:** for long responses, flush partial content at natural breakpoints; see [runbooks/streaming-checkpoints.md](runbooks/streaming-checkpoints.md). Prefer wrapping exchange responses in `<!-- patch:exchange -->`.

**`#agent-doc-bug` plan proof:** if the prompt contract requires a plan, create the plan file before closeout and cite every plan path in the response. If `execution_scope=plan_backlog_only`, create the plan/backlog items and explain the deferred implementation boundary instead of editing code in that same cycle.

### 1b. Update pending (template mode)

If the document has `<!-- agent:backlog -->` (or legacy `agent:pending`), mutate it only through granular `agent-doc write` flags: `--pending-add`, `--pending-done <id>`, `--pending-edit "id=text"`, `--pending-reorder`, `--pending-gate`, and `--pending-ungate`. Full-replace via `<!-- patch:backlog -->` is rejected; see [runbooks/pending-ops.md](runbooks/pending-ops.md). For `<!-- agent:icebox -->`, use `<!-- replace:icebox -->`.

**Pending capture rule:** if the response creates concrete follow-up work, add it to `agent:backlog` in the same cycle. Put new items at the beginning of `agent:backlog`; if you are extending an ordered batch already in pending, insert the new item adjacent to its predecessor. If the item is only a recommendation, include `[recommended]`.

**Plan-backed pending items:** create the plan file first and include that exact plan file path in the pending text.

**`do #id` closeout rule:** when the user directs `do #id ...`, record the pending outcome before persistence: `--pending-done <id>` if completed, `--pending-gate <id>` if code-complete but externally blocked, or explain concretely why it stays open. `session-check` enforces the `pending_done_guard`.

### 2. Persist the response (MANDATORY — never skip)

Complete requested implementation, verification, build/install, and local inspection work **before** this step. The response persistence command is the final document-mutation boundary for the cycle, not an intermediate progress checkpoint.

**Agent harnesses own full-suite verification:** if you changed code, tests, build logic, or instruction surfaces, run the full project verification suite explicitly after the edits and before `finalize` / `write --commit`. Do not rely on a pre-commit hook. Do not waive red suites as "unrelated" or "flaky".

**Session document staging rule:** when the turn also asks for ordinary repo `commit + push`, keep the session document out of that manual git commit. Resolve the exact intended non-session path set first, stage only that set, stop on any stage failure, verify `git diff --cached --name-only` still matches the intended set, commit only that validated set, then let `finalize` / `write --commit` close the session document before the push.

For the normal response cycle, pipe the response through `agent-doc finalize --stream` so the write crosses the commit boundary in one binary-owned path. **This step is MANDATORY every cycle unless the user explicitly told you to leave the response uncommitted.**

```bash
cat <<'RESPONSE' | agent-doc finalize <FILE> --baseline-file <preflight.baseline_file> --stream --origin skill
<your response — patch blocks for template mode, or plain text for inline mode>
RESPONSE
```

`finalize` reuses the normal write pipeline, then requires the cycle to reach `committed` and the post-commit `session-check` guard to pass before success. `agent-doc write --commit <FILE>` shares that fail-closed boundary for repair writes on real session documents. If `finalize`, `write --commit`, or `repair` surfaces a `session-check` interruption, continue recovery instead of reporting success. `session-check` also enforces the pending capture / `pending_done_guard` backstops. Use [runbooks/commit.md](runbooks/commit.md) and [runbooks/harness-invocation.md](runbooks/harness-invocation.md) for the full closeout contract.

After `finalize` / `write --commit`, do not start more long-running task work for that same turn. Codex hooks in `.codex/hooks.json` and `.codex/config.toml` are a fail-closed backstop, not a replacement for explicit closeout.

**IMPORTANT: Do NOT use the Edit tool for write-back.** It is prone to "file modified since read" errors when the user edits concurrently.

**IMPORTANT: The response content MUST include `<!-- patch:exchange -->` blocks for template-mode documents.** If the heredoc is empty or contains only raw text without patch markers, the binary will warn (`0 template patches found`) and the response can be lost.

**Manual repair / missed patchback rule (all harnesses):** if the user's prompt is already present in the document, do **not** patch the assistant response directly into the file. Use `agent-doc write --commit <FILE>` so the repair crosses the normal snapshot/commit boundary in one path. Do not document or follow a manual-repair flow that stops after bare `agent-doc write`. Direct file patching is only acceptable for inserting a missing user prompt into `exchange` before the response exists.

Document format, frontmatter, component naming, and commit-boundary exceptions live in [runbooks/document-format.md](runbooks/document-format.md) and [runbooks/commit.md](runbooks/commit.md).

## Runbooks

Use runbooks for detail that is not needed every turn. [runbooks/split-spec-files.md](runbooks/split-spec-files.md) applies across agent-doc-managed harness instruction surfaces; custom root instruction files stay opt-in unless they still match the generated baseline.

Key runbooks:

- [runbooks/harness-invocation.md](runbooks/harness-invocation.md) — harness-specific invocation patterns
- [runbooks/planning-dispatch.md](runbooks/planning-dispatch.md) — post-preflight execution contract
- [runbooks/pending-ops.md](runbooks/pending-ops.md) — backlog mutation rules
- [runbooks/commit.md](runbooks/commit.md) — response commit boundary and exceptions
- [runbooks/split-spec-files.md](runbooks/split-spec-files.md) — stable-index spec splitting rule

The full bundled catalog also includes `compact-exchange`, `transfer-extract`, `model-tier-gate`, `command-synonyms`, `compound-task-steering`, `streaming-checkpoints`, `document-format`, and `code-enforced-directives`.
