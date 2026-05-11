---
description: "Interactive markdown session. TRIGGER: user invokes /agent-doc <file>. Requires a markdown session document, installed CLI, and write+commit every cycle."
user-invocable: true
argument-hint: "<file>"
agent-doc-version: "0.33.16"
---

# agent-doc

Interactive document session — respond to user edits in a markdown document.

## Harness Compatibility

This shared hot path serves Claude Code, Codex, Cursor, and direct harnesses. Invocation wording is harness-specific; workflow and closeout are shared. Use [runbooks/harness-invocation.md](runbooks/harness-invocation.md) for harness details and [runbooks/commit.md](runbooks/commit.md) for closeout.

## Invocation

```
/agent-doc <FILE>
/agent-doc claim <FILE>
/agent-doc compact <FILE>
/agent-doc compact exchange <FILE>
```

Arguments: `FILE` — path to the session document (e.g., `plan.md`).

**Note:** Slash commands (`/agent-doc`) are Claude Code-specific. Other harnesses receive the document path directly.

## Hot Path Digest

- **Document is the UI** — user edits ARE the prompt; respond in the document and console.
- **Harness-native `agent-doc` entrypoints start the binary-owned response cycle** — treat `/agent-doc <FILE>`, `agent-doc <FILE>`, or equivalent as executable workflow start, not a generic document-editing request. Do not manually patch the final assistant response into the document, and do not report success before `agent-doc finalize <FILE>` or `agent-doc write --commit <FILE>` completes. Codex/direct-exec paths also run `agent-doc session-check <FILE>`.
- **Imperative edits are executable directives** — `do #id`, `fix this`, `run tests`, `build + install`, `commit + push`, and similar edits authorize repo work. Do not require the same instruction to be repeated in chat.
- **Project-scoped remote hosts** — globally approved SSH commands, ambient SSH config, and unrelated project history are not evidence that a named remote host belongs to the current document's project. Use a named remote host only when the current user prompt, this session document/frontmatter, project-local `.agent-doc/config.toml`, or project-local runbooks explicitly identify it; otherwise ask or record a follow-up to confirm the intended host.
- **MCP auth / OAuth steps are sub-steps, not closeout boundaries** — after auth/browser approval, resume and still finish through `finalize` / `write --commit` plus `session-check`.
- **Manual repo commits keep the session document on the finalize path** — stage and commit only the intended non-session repo files first, stop on any stage failure, verify the staged diff still matches the intended path set, then let `finalize` / `write --commit` own the session document.
- **Rare routing / tmux / startup-miss invariants live in runbooks** — consult [runbooks/harness-invocation.md](runbooks/harness-invocation.md), [runbooks/commit.md](runbooks/commit.md), and [runbooks/code-enforced-directives.md](runbooks/code-enforced-directives.md) for route/start/sync/session-check or sibling `src/tmux-router` work.
- Preserve user edits; let `agent-doc write --stream` merge. Stream useful console status.

## Workflow

### 0. Pre-flight

Detect subcommands before the normal workflow:

- `claim <FILE>` → run `agent-doc claim <FILE>` via Bash and stop.
- `compact <FILE>` → run `agent-doc compact <FILE> --commit` and stop.
- `compact exchange <FILE>` → follow [runbooks/compact-exchange.md](runbooks/compact-exchange.md) and stop.

**Auto-update skill:** Compare `agent-doc --version` to `agent-doc-version`. If newer, run the active-harness install: Claude Code `agent-doc skill install --harness claude --reload compact`; Codex `agent-doc skill install --harness codex --reload restart`; other harnesses `agent-doc skill install`. If install says already up to date, treat this file as stale duplicate instructions, use installed harness instructions, and continue with the task. Stop only on a real `SKILL_RELOAD=...`; see [runbooks/harness-invocation.md](runbooks/harness-invocation.md).

Run `agent-doc preflight <FILE>`. Preflight owns recovery before diffing and prints the cycle contract: `baseline_file`, `no_changes`, `claims`, `slash_commands`, `builtin_commands`, `orchestration_request`, `prompt_presets_requested`, tier/model fields, `agent_model`, `diff_type`, and the diff contract.

- If `no_changes: true` → tell the user nothing changed and stop.
- Print any `claims` to the console as a record.
- Use `baseline_file` as `--baseline-file` for every subsequent response-persistence command. Do NOT save your own baseline — preflight's copy is taken at a stable post-commit point.
- First cycle only: if the document is not yet in context, run `agent-doc read <FILE>` to fetch HEAD content. Do NOT read the snapshot file directly.

### 0b. Slash Commands

Handle non-empty `slash_commands` or `builtin_commands` before responding. Claude Code invokes `slash_commands` via the `Skill` tool; other harnesses skip them. For `builtin_commands`, tell the user to run the command at the terminal. Trust preflight; do not re-validate fences or blockquotes. If `orchestration_request` is non-null, run `agent-doc orchestrate <FILE> --mode <orchestration_request.mode> --from-exchange` before manual response composition. If `prompt_presets_requested` is non-empty, let `orchestrate` expand the validated presets.

### 0c. Model Tier

Read `effective_tier`, `required_tier`, `suggested_tier`, and `model_switch_tier` from preflight; follow [runbooks/model-tier-gate.md](runbooks/model-tier-gate.md). For `run these in order`, `fan out`, or `after #a do #b`, prefer binary-owned `orchestration_request`; otherwise use [runbooks/command-synonyms.md](runbooks/command-synonyms.md) and [runbooks/compound-task-steering.md](runbooks/compound-task-steering.md).

### 0d. Plan / Dispatch

After preflight, run `agent-doc plan <FILE>` and treat `prompt_targets`, `execution_scope`, `repo_actions`, `required_commands`, `pending_mutations`, `handoff`, and `blockers` as the execution contract. Stop on `blockers`. If `handoff=orchestrate`, run the emitted `agent-doc orchestrate ...` command before manual response. If `handoff=compact`, follow the emitted compact/restart instruction and stop before repo work or finalization. Full contract: [runbooks/planning-dispatch.md](runbooks/planning-dispatch.md).

### 1. Respond

- Address the user's changes naturally in the console; that response is the document response.
- Reconcile the changed exchange tail oldest-first. Do not stop at the newest question; answer or group each unresolved prompt in that tail and each unresolved `prompt_target`; treat `content_edit` items as user corrections.
- If session-accretion supplies bounded context, use the included `### Re:` blocks as prompt-position anchors, not proof that older turns are absent.
- Execute from the planning record. If `execution_scope=plan_backlog_only`, stay in plan/backlog capture mode. Otherwise complete the requested repo work before persistence or stop on a blocker. Do not keep appending "starting/continuing" status prose while the requested work remains undone.

**Response header format (template mode):** use `### Re: topic` markdown headers — **not** bold (`**Re:**`). The `(HEAD)` boundary marker requires real headings. Use h4–h6 for sub-sections within a response.

**Model attribution:** always append the resolved model short name with a spaced em dash: `### Re: topic — gpt-5` or `### Re: topic — opus-4-6`. Use `preflight.agent_model` if non-null (from frontmatter); otherwise use your own model identity (you know what model you are). Never use the harness label (`codex`, `claude`) as the suffix, and never omit it.

**Streaming checkpoints:** for long responses, flush partial content at natural breakpoints; see [runbooks/streaming-checkpoints.md](runbooks/streaming-checkpoints.md). Prefer `<!-- patch:exchange -->`.

**`#agent-doc-bug` plan proof:** if the prompt contract requires a plan, create the plan file before closeout and cite every plan path. If `execution_scope=plan_backlog_only`, create plan/backlog items and explain the deferred implementation boundary instead of editing code.

### 1b. Update pending (template mode)

If `<!-- agent:backlog -->` (or legacy `agent:pending`) exists, mutate it only through granular `agent-doc write` flags: `--pending-add`, `--done <id>`, `--pending-edit "id=text"`, `--pending-reorder`, `--pending-gate`, `--pending-ungate`. Full-replace via `<!-- patch:backlog -->` is rejected; see [runbooks/pending-ops.md](runbooks/pending-ops.md). For `<!-- agent:icebox -->`, use `<!-- replace:icebox -->`.

Completed/reaped items live under canonical `<!-- agent:done -->`; legacy `agent:backlog-done` and `agent:pending-done` tags require `agent-doc migrate`.

**Pending capture rule:** if the response creates concrete follow-up work, add it to `agent:backlog` in the same cycle. Put new items at the beginning of `agent:backlog`; if you are extending an ordered batch already in pending, insert the new item adjacent to its predecessor. If the item is only a recommendation, include `[recommended]`.

**Cross-document pending rule:** if a prompt preset or user instruction names another backlog file, add the item to that target with `--pending-add-to <target-file> "<item>"` on the final `agent-doc finalize` command. Do not satisfy an explicit target by running `--pending-add` against the current session document. If the target is missing or lacks a backlog component, stop on the binary error and report the blocker.

**Plan-backed pending items:** create the plan file first and include that exact plan file path in the pending text.

**`do #id` closeout rule:** when the user directs `do #id ...`, record the pending outcome before persistence: `--done <id>` if completed, `--pending-gate <id>` if code-complete but externally blocked, or explain concretely why it stays open. `session-check` enforces the `pending_done_guard`.

### 2. Persist the response (MANDATORY — never skip)

Complete requested implementation, verification, build/install, and local inspection **before** this step. The response persistence command is the final document-mutation boundary for the cycle, not an intermediate progress checkpoint.

**Agent harnesses own full-suite verification:** if you changed code, tests, build logic, or instruction surfaces, run the full project verification suite explicitly after edits and before `finalize` / `write --commit`. Do not rely on a pre-commit hook. Do not waive red suites as "unrelated" or "flaky".

**Session document staging rule:** for ordinary repo `commit + push`, keep the session document out of that manual git commit. Resolve the exact intended non-session path set first, stage only that set, stop on any stage failure, verify `git diff --cached --name-only` still matches the intended set, commit only that validated set, then let `finalize` / `write --commit` close the session document before push.

For the normal cycle, pipe the response through `agent-doc finalize --stream` so write+commit happen in one binary-owned path. **This step is MANDATORY every cycle unless the user explicitly told you to leave the response uncommitted.**

```bash
cat <<'RESPONSE' | agent-doc finalize <FILE> --baseline-file <preflight.baseline_file> --stream --origin skill
<your response — patch blocks for template mode, or plain text for inline mode>
RESPONSE
```

`finalize` requires the cycle to reach `committed` and the post-commit `session-check` guard to pass before success, including prompt-only exchange-tail checks. `agent-doc write --commit <FILE>` shares that fail-closed boundary for repair writes. If `finalize`, `write --commit`, or `repair` surfaces a `session-check` interruption, continue recovery instead of reporting success. `session-check` also enforces pending capture / `pending_done_guard`. Use [runbooks/commit.md](runbooks/commit.md) and [runbooks/harness-invocation.md](runbooks/harness-invocation.md) for the full closeout contract.

After `finalize` / `write --commit`, do not start more long-running task work for that same turn. Codex hooks in `.codex/hooks.json` and `.codex/config.toml` are a fail-closed backstop, not a replacement for explicit closeout.

**IMPORTANT: Do NOT use the Edit tool for write-back.** It is prone to "file modified since read" errors when the user edits concurrently.

**IMPORTANT: The response content MUST include `<!-- patch:exchange -->` blocks for template-mode documents.** If the heredoc is empty or contains only raw text without patch markers, the binary will warn (`0 template patches found`) and the response can be lost.

**Manual repair / missed patchback rule (all harnesses):** if the user's prompt is already present in the document, do **not** patch the assistant response directly into the file. Use `agent-doc write --commit <FILE>` so repair crosses the normal snapshot/commit boundary in one path. Do not document or follow a manual-repair flow that stops after bare `agent-doc write`. Direct file patching is only acceptable for inserting a missing user prompt into `exchange` before the response exists.

Document format, frontmatter, component naming, and commit-boundary exceptions: [runbooks/document-format.md](runbooks/document-format.md), [runbooks/commit.md](runbooks/commit.md).

## Runbooks

Use runbooks for detail that is not needed every turn. Key runbooks: [runbooks/harness-invocation.md](runbooks/harness-invocation.md), [runbooks/planning-dispatch.md](runbooks/planning-dispatch.md), [runbooks/pending-ops.md](runbooks/pending-ops.md), [runbooks/commit.md](runbooks/commit.md), [runbooks/split-spec-files.md](runbooks/split-spec-files.md). `split-spec-files` applies across agent-doc-managed surfaces; custom root files stay opt-in unless they still match the generated baseline. Full catalog: `compact-exchange`, `transfer-extract`, `model-tier-gate`, `command-synonyms`, `compound-task-steering`, `streaming-checkpoints`, `document-format`, `code-enforced-directives`.
