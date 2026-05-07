---
description: "Interactive document session — respond to user edits in a markdown file. TRIGGER: user invokes /agent-doc <file>. ALL-OF: (1) file is a markdown session document, (2) CLI is installed, (3) write+commit are executed every cycle without exception."
user-invocable: true
argument-hint: "<file>"
agent-doc-version: "0.33.16"
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
- **Harness-native `agent-doc` entrypoints start the binary-owned response cycle** — treat `/agent-doc <FILE>`, `agent-doc <FILE>`, or the harness-equivalent entrypoint as executable workflow start, not a generic document-editing request. Do not manually patch the final assistant response into the document, and do not report success before `agent-doc finalize <FILE>` or `agent-doc write --commit <FILE>` completes. For Codex/direct-exec paths, run the explicit `agent-doc session-check <FILE>` backstop before ending the turn.
- **Imperative edits are executable directives** — `do #id`, `fix this`, `run tests`, `build + install`, `commit + push`, and similar document edits authorize the underlying repo work. Do not require the same instruction to be repeated in chat.
- **MCP auth / OAuth steps are sub-steps, not closeout boundaries** — if a tool pauses for authentication or browser approval, resume the same managed turn and still finish through `finalize` / `write --commit` plus `session-check`.
- **Manual repo commits must keep the session document on the finalize path** — stage and commit only the intended non-session repo files first, stop on any stage failure, verify the staged diff still matches the intended path set, then let `agent-doc finalize <FILE>` or `agent-doc write --commit <FILE>` own the session-document closeout commit.
- **Hook-visible Codex dispatch-only reroutes must prove the submit, not just acceptance** — if `agent-doc route --dispatch-only <FILE>` reuses a live Codex pane and visible hook tracking never records a routed submission proof for the bare reopen, stop with the accepted-but-unproven error instead of treating pane acceptance alone as success.
- **Late legacy-owner proof still blocks normal auto-start** — if stale registered-pane cleanup exposes a live associated pane later in the same route pass, fail closed with explicit claim/repair guidance instead of silently cold-starting around that ownership ambiguity.
- **Safe-passive sync preserves layout before protected panes can force visible growth** — if `sync --no-autostart` would need to attach a newly requested pane while already-visible unwanted panes are still protected by open cycles, preserve the current visible layout and warn instead of expanding into a larger pane mix that cannot detach cleanly yet.
- **Rare routing / tmux / startup-miss invariants live in the runbooks and repo docs** — consult [runbooks/harness-invocation.md](runbooks/harness-invocation.md), [runbooks/commit.md](runbooks/commit.md), and [runbooks/code-enforced-directives.md](runbooks/code-enforced-directives.md) when the turn touches route/start/sync/session-check edge cases or sibling `src/tmux-router` work.
- **Preserve user edits** — never overwrite; let `agent-doc write --stream` merge.
- **Show progress** — stream your response in the console so the user sees real-time feedback.

## Workflow

### 0. Pre-flight (single command)

**Detect subcommands** before running the normal workflow:

- `claim <FILE>` → run `agent-doc claim <FILE>` via Bash and stop.
- `compact <FILE>` → run `agent-doc compact <FILE> --commit` and stop.
- `compact exchange <FILE>` → follow [runbooks/compact-exchange.md](runbooks/compact-exchange.md) and stop.

**Auto-update skill:** Run `agent-doc --version` and compare against `agent-doc-version` in this file's frontmatter. If the binary is newer, run the install command for the active harness: Claude Code `agent-doc skill install --harness claude --reload compact`; Codex `agent-doc skill install --harness codex --reload restart`; other harnesses `agent-doc skill install`. If the install output says the active harness instructions are already up to date, treat this file as stale duplicate instructions, use the installed harness-specific instructions as authoritative for the rest of the turn, and continue with the task. Only stop when the command prints a real `SKILL_RELOAD=...` marker for the active harness. See [runbooks/harness-invocation.md](runbooks/harness-invocation.md) for harness-specific prompting.

**Run preflight:** `agent-doc preflight <FILE>` via Bash. Preflight owns recovery before diffing and prints the JSON contract for the rest of the cycle. Key fields: `no_changes`, `claims`, `diff`, `baseline_file`, `slash_commands`, `builtin_commands`, `orchestration_request`, `prompt_presets_requested`, `effective_tier`, `required_tier`, `suggested_tier`, `model_switch`, `model_switch_tier`, `agent_model`, and `diff_type`.

- If `no_changes: true` → tell the user nothing changed and stop.
- Print any `claims` to the console as a record.
- Use `baseline_file` as `--baseline-file` for every subsequent response-persistence command. Do NOT save your own baseline — preflight's copy is taken at a stable post-commit point.
- First cycle only: if the document is not yet in context, run `agent-doc read <FILE>` to fetch HEAD content. Do NOT read the snapshot file directly.

### 0b. Execute slash commands (if any)

If `slash_commands` or `builtin_commands` is non-empty, handle them **before** responding. Claude Code invokes `slash_commands` via the `Skill` tool; other harnesses skip them. For `builtin_commands`, write a note telling the user to run the command at the terminal. Trust the preflight output — do not re-validate code fences or blockquotes. If `orchestration_request` is non-null, run `agent-doc orchestrate <FILE> --mode <orchestration_request.mode> --from-exchange` before composing any manual response. If `prompt_presets_requested` is non-empty, treat that as already validated and let `orchestrate` expand the presets.

### 0c. Model tier gate

Read `effective_tier`, `required_tier`, `suggested_tier`, and `model_switch_tier` from preflight and follow [runbooks/model-tier-gate.md](runbooks/model-tier-gate.md). For natural-language batching such as `run these in order`, `fan out`, or `after #a do #b`, prefer the binary-owned `orchestration_request`; otherwise use [runbooks/command-synonyms.md](runbooks/command-synonyms.md) and [runbooks/compound-task-steering.md](runbooks/compound-task-steering.md).

### 0d. Planning / dispatch

After `preflight`, run `agent-doc plan <FILE>` and treat its `prompt_targets`, `execution_scope`, `repo_actions`, `required_commands`, `pending_mutations`, `handoff`, and `blockers` as the execution contract. If `blockers` is non-empty, stop. If `handoff=orchestrate`, run the emitted `agent-doc orchestrate ...` command before any manual response. Full contract: [runbooks/planning-dispatch.md](runbooks/planning-dispatch.md).

### 1. Respond

- Address the user's changes naturally in the console — the console response IS the document response.
- Reconcile the changed exchange tail oldest-first. Do not stop at the newest question; each unresolved prompt in that tail must be answered or grouped, while each unresolved `prompt_target` must be answered or grouped and `content_edit` items are user corrections to incorporate.
- If session-accretion replaced the full exchange tail with a bounded recent-context pack, treat the included `### Re:` blocks as prompt-position anchors (enclosing response for inline edits, immediately previous response for tail follow-ups), not as proof that unrelated older turns are absent.
- Execute the cycle from the planning record instead of re-reading the raw diff ad hoc. If `execution_scope=plan_backlog_only`, stay in plan/backlog capture mode. Otherwise, complete the requested repo work before persistence or stop on a concrete blocker. Do not keep appending "starting/continuing" status prose while the requested work remains undone.

**Response header format (template mode):** use `### Re: topic` markdown headers — **not** bold (`**Re:**`). The `(HEAD)` boundary marker requires real headings. Use h4–h6 for sub-sections within a response.

**Model attribution:** always append the resolved model short name with a spaced em dash: `### Re: topic — gpt-5` or `### Re: topic — opus-4-6`. Use `preflight.agent_model` if non-null (from frontmatter); otherwise use your own model identity (you know what model you are). Never use the harness label (`codex`, `claude`) as the suffix, and never omit it.

**Streaming checkpoints:** for long responses, flush partial content at natural breakpoints. Procedure: [runbooks/streaming-checkpoints.md](runbooks/streaming-checkpoints.md).
**Prefer wrapping exchange responses in `<!-- patch:exchange -->`** for clarity. Raw content also works via boundary synthesis.
**`#agent-doc-bug` plan proof:** if the prompt contract requires a plan, create the plan file before closeout and cite every plan path in the response. If `execution_scope=plan_backlog_only`, create the plan/backlog items and explain the deferred implementation boundary instead of editing code in that same cycle.

### 1b. Update pending (template mode)

If the document has `<!-- agent:backlog -->` (or legacy `agent:pending`), mutate it only through the granular `agent-doc write` flags such as `--pending-add`, `--pending-done <id>`, `--pending-edit "id=text"`, `--pending-reorder`, `--pending-gate`, and `--pending-ungate`. Full-replace via `<!-- patch:backlog -->` is rejected. Full contract: [runbooks/pending-ops.md](runbooks/pending-ops.md).

If you need to rewrite `<!-- agent:icebox -->`, use a template patch block (`<!-- replace:icebox --> ... <!-- /replace:icebox -->`) rather than backlog flags.

**Pending capture rule:** if the response creates concrete follow-up work, add it to `agent:backlog` in the same cycle. Put new items at the beginning of `agent:backlog`; if you are extending an ordered batch already in pending, insert the new item adjacent to its predecessor. If the item is only a recommendation, include `[recommended]`.

**Plan-backed pending items:** create the plan file first and include that exact plan file path in the pending text.

**`do #id` closeout rule:** when the user directs `do #id ...`, record the pending outcome before persistence: `--pending-done <id>` if completed, `--pending-gate <id>` if code-complete but externally blocked, or explain concretely why it stays open. `session-check` enforces the `pending_done_guard`.

### 2. Persist the response (MANDATORY — never skip)

Complete the requested implementation, verification, build/install, and local inspection work for this turn **before** this step. The response persistence command is the final document-mutation boundary for the cycle, not an intermediate progress checkpoint.

**Agent harnesses own full-suite verification:** if you changed code, tests, build logic, or instruction surfaces, run the full project verification suite explicitly after the edits and before `finalize` / `write --commit`. Do not rely on a pre-commit hook. Do not waive red suites as "unrelated" or "flaky".

**Session document staging rule:** when the turn also asks for ordinary repo `commit + push`, keep the session document out of that manual git commit. Resolve the exact intended non-session path set first, stage only that set, stop on any stage failure, verify `git diff --cached --name-only` still matches the intended set, commit only that validated set, then let `finalize` / `write --commit` close the session document before the push.

For the normal response cycle, pipe the response through `agent-doc finalize --stream` so the write crosses the commit boundary in one binary-owned path. **This step is MANDATORY every cycle unless the user explicitly told you to leave the response uncommitted.**

```bash
cat <<'RESPONSE' | agent-doc finalize <FILE> --baseline-file <preflight.baseline_file> --stream --origin skill
<your response — patch blocks for template mode, or plain text for inline mode>
RESPONSE
```

`finalize` reuses the normal write pipeline, then requires the cycle to reach `committed` and the post-commit `session-check` guard to pass before it exits successfully. `agent-doc write --commit <FILE>` shares that fail-closed boundary for repair writes on real session documents. If `finalize`, `write --commit`, or `repair` surfaces a `session-check` interruption, continue recovery instead of reporting success. `session-check` also enforces the pending capture / `pending_done_guard` backstops. Use [runbooks/commit.md](runbooks/commit.md) and [runbooks/harness-invocation.md](runbooks/harness-invocation.md) for the full closeout contract.

After `finalize` / `write --commit`, do not start more long-running task work for that same turn. The Codex install writes `.codex/hooks.json` and `.codex/config.toml` as a fail-closed backstop, but those hooks do not replace the explicit `finalize` / `write --commit` path.

**IMPORTANT: Do NOT use the Edit tool for write-back.** It is prone to "file modified since read" errors when the user edits concurrently.

**IMPORTANT: The response content MUST include `<!-- patch:exchange -->` blocks for template-mode documents.** If the heredoc is empty or contains only raw text without patch markers, the binary will warn (`0 template patches found`) and the response can be lost.

**Manual repair / missed patchback rule (all harnesses):** if the user's prompt is already present in the document, do **not** patch the assistant response directly into the file. Use `agent-doc write --commit <FILE>` so the repair crosses the normal snapshot/commit boundary in one path. Do not document or follow a manual-repair flow that stops after bare `agent-doc write`. Direct file patching is only acceptable for inserting a missing user prompt into `exchange` before the response exists.

Document format, frontmatter fields, append vs template mode conventions, and component naming: [runbooks/document-format.md](runbooks/document-format.md). Commit-boundary exceptions and anti-patterns live in [runbooks/commit.md](runbooks/commit.md).

## Runbooks

Reusable authoring rules such as [runbooks/split-spec-files.md](runbooks/split-spec-files.md) apply across agent-doc-managed harness instruction surfaces. Leave custom root instruction files opt-in unless they still match the generated baseline.

Key runbooks:

- [runbooks/harness-invocation.md](runbooks/harness-invocation.md) — harness-specific invocation patterns
- [runbooks/planning-dispatch.md](runbooks/planning-dispatch.md) — post-preflight execution contract
- [runbooks/pending-ops.md](runbooks/pending-ops.md) — backlog mutation rules
- [runbooks/commit.md](runbooks/commit.md) — response commit boundary and exceptions
- [runbooks/split-spec-files.md](runbooks/split-spec-files.md) — stable-index spec splitting rule

The full bundled catalog lives in `runbooks/`, including `compact-exchange`, `transfer-extract`, `model-tier-gate`, `command-synonyms`, `compound-task-steering`, `streaming-checkpoints`, `document-format`, and `code-enforced-directives`.

## Success Criteria

- User sees streaming response in the agent console.
- Document is updated and user's concurrent edits are preserved.
- Snapshot is updated for the next cycle's diff.
