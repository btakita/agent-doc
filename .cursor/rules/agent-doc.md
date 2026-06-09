---
description: "Interactive markdown session. TRIGGER: user invokes /agent-doc <file>. Requires a markdown session document, installed CLI, and write+commit every cycle."
user-invocable: true
argument-hint: "<file>"
agent-doc-version: "0.34.0"
---

# agent-doc

Interactive document session — respond to user edits in a markdown document.

## Harness Compatibility

This shared hot path serves Claude Code, Codex, OpenCode, Cursor, and direct harnesses. Invocation wording is harness-specific; workflow and closeout are shared. Use [runbooks/harness-invocation.md](runbooks/harness-invocation.md) for harness details and [runbooks/commit.md](runbooks/commit.md) for closeout.

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
- **Harness-native `agent-doc` entrypoints start the binary-owned response cycle** — treat `/agent-doc <FILE>`, `agent-doc <FILE>`, or equivalent as executable workflow start, not a generic document-editing request. Do not manually patch the final assistant response into the document, and do not report success before `agent-doc finalize <FILE>` or `agent-doc write --commit <FILE>` completes. Response text visible in the console but absent from the session document is a closeout violation — the response does not exist until it crosses `finalize` or `write --commit`. Codex/OpenCode/direct-exec paths also run `agent-doc session-check <FILE>`.
- **Imperative edits are executable directives** — `do #id`, `fix this`, `run tests`, `build + install`, `commit + push`, and similar edits authorize repo work. Do not require the same instruction to be repeated in chat.
- **Project-scoped remote hosts** — globally approved SSH commands, ambient SSH config, and unrelated project history are not evidence that a named remote host belongs to the current document's project. Use a named remote host only when the current user prompt, this session document/frontmatter, project-local `.agent-doc/config.toml`, or project-local runbooks explicitly identify it; otherwise ask or record a follow-up to confirm the intended host.
- **Missed response repair is not child-agent dispatch** — when preflight/commit says the prior response is already committed and no new assistant body exists, recover through `agent-doc write --commit <FILE>` instead of rerunning an empty response cycle.
- **MCP auth / OAuth steps are sub-steps, not closeout boundaries** — after auth/browser approval, resume and still finish through `finalize` / `write --commit` plus `session-check`.
- **Manual repo commits keep the session document on the finalize path** — stage and commit only the intended non-session repo files first, stop on any stage failure, verify the staged diff still matches the intended path set, then let `finalize` / `write --commit` own the session document.
- **Dispatch proof language must preserve scope** — when reading route diagnostics, `proof=accepted proof_scope=accepted_only` means pane-input acceptance only. Do not describe Claude Code/OpenCode dispatch-only routes as consumed/submitted unless logs show dispatch-start proof.
- **Starting actor reroutes are prompt-gated** — if route diagnostics mention a `starting` authoritative actor, treat dispatch as valid only after the live pane shows a harness-specific dispatch-ready prompt; otherwise the route path must fail closed before input.
- **Rare routing / tmux / startup-miss invariants live in runbooks** — consult [runbooks/harness-invocation.md](runbooks/harness-invocation.md), [runbooks/commit.md](runbooks/commit.md), and [runbooks/code-enforced-directives.md](runbooks/code-enforced-directives.md) for route/start/sync/session-check or sibling `src/tmux-router` work.
- Preserve user edits; let `agent-doc write --stream` merge. Stream useful console status.

## Workflow

### 0. Pre-flight

Detect subcommands before the normal workflow:

- `claim <FILE>` → run `agent-doc claim <FILE>` via Bash and stop.
- `compact <FILE>` → run `agent-doc compact <FILE> --commit` and stop.
- `compact exchange <FILE>` → follow [runbooks/compact-exchange.md](runbooks/compact-exchange.md) and stop.

**Auto-update skill:** Compare `agent-doc --version` to `agent-doc-version`. If newer, run `agent-doc skill install --harness claude --reload restart` unless `agent_doc_auto_compact` is explicitly set in frontmatter or `.agent-doc/config.toml`. On `SKILL_RELOAD=restart`, ask the user to restart Claude Code and re-invoke `/agent-doc <FILE>`, then stop. Use `--reload compact` and ask for `/compact` only when that explicit opt-in exists. If already up to date, treat as stale instruction drift, continue this turn, and use the installed Claude skill. If `agent-doc` is missing or versions match, skip. See [runbooks/harness-invocation.md](runbooks/harness-invocation.md).

Run `agent-doc preflight <FILE>`. Preflight owns recovery before diffing and prints the cycle contract: `baseline_file`, `no_changes`, `warnings`, `claims`, `slash_commands`, `builtin_commands`, `orchestration_request`, `prompt_presets_requested`, tier/model fields, `agent_model`, `diff_type`, and the diff contract.

- If `no_changes: true` → tell the user nothing changed and stop.
- Surface any `warnings`; for `harness_mismatch`, note that the document-declared agent differs from the active harness and continue with the active harness attribution/closeout path.
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
- Execute from the planning record. If `execution_scope=plan_backlog_only`, stay in plan/backlog capture mode. Otherwise complete the requested repo work before persistence or stop on a blocker. Do not keep appending "starting/continuing" status prose while the requested work remains undone.

**Response header format (template mode):** use `### Re: topic` markdown headers — **not** bold (`**Re:**`). The `(HEAD)` boundary marker requires real headings.

**Model attribution:** always append the resolved model short name with a spaced em dash: `### Re: topic — gpt-5` or `### Re: topic — opus-4-6`. Use `preflight.agent_model` if non-null; otherwise use your own model identity. Never use the harness label (`codex`, `claude`) as the suffix, and never omit it.

Full detail (session-accretion anchors, streaming checkpoints, `#agent-doc-bug` plan proof): [runbooks/respond.md](runbooks/respond.md).

### 1b. Update pending (template mode)

Mutate `<!-- agent:backlog -->` only through granular `agent-doc write` flags (`--pending-add`, `--done <id>`, `--pending-edit "id=text"`, `--pending-gate`, `--pending-ungate`, `--pending-reorder`, `--review-add`/`--review-edit`); full-replace via `patch:backlog`/`patch:review` is rejected.

**Pending capture rule:** if the response creates concrete follow-up work, add it to `agent:backlog` in the same cycle. Put new items at the beginning of `agent:backlog`; if you are extending an ordered batch already in pending, insert the new item adjacent to its predecessor. If the item is only a recommendation, include `[recommended]`.

**Plan-backed pending items:** create the plan file first and include that exact plan file path in the pending text. For multi-phase implementation work, prefer one backlog ID per actionable phase (for example `#crdtrespfx1`, `#crdtrespfx2`) instead of one parent ID that gets repeatedly `--pending-gate`d after partial progress; keep the parent plan file as context, but queue and close out concrete phase IDs.

**`do #id` closeout rule:** record `--done` (completed), `--pending-gate` (code-complete, awaiting review/external validation), or explain concretely why it stays open — `session-check` enforces `pending_done_guard`.

**Complete over gate (default to finishing the work).** A turn's job is to **complete** its task, not to file a tracking item. Strongly prefer `--done`: do the implementation, tests, build/install, and verification this cycle and close the item. Use `--pending-gate` / `agent:review` (a gated `[/]` item) **only under exceptional conditions** — work that is genuinely blocked on something this turn cannot do (a required live editor/pane verification, an external approval, a CI/billing outage), and say exactly what unblocks it. Do **not** gate to avoid effort, to "track for later" what you could finish now, or to record a hypothesis. When follow-up work is real but not blocked, put it in `agent:backlog` as an **actionable** item, not in `agent:review`. If a gated review item's blocking condition is not actually met or is stale, convert it to an actionable backlog item (or `--done` it if already satisfied) and remove the review item — keep `agent:review` small (target < 10) so the actionable list stays legible. Prefer automated completion detection (log/state checks the binary can evaluate) over a human-gated review item wherever possible.

Full detail (cross-document rule, icebox, `agent:done`): [runbooks/respond.md](runbooks/respond.md) and [runbooks/pending-ops.md](runbooks/pending-ops.md).

### 2. Persist the response (MANDATORY — never skip)

Complete requested implementation, verification, build/install, and local inspection **before** this step. The response persistence command is the final document-mutation boundary for the cycle, not an intermediate progress checkpoint. For the normal cycle, pipe the response through `agent-doc finalize --stream` so write+commit happen in one binary-owned path. **This step is MANDATORY every cycle unless the user explicitly told you to leave the response uncommitted.**

**Agent harnesses own full-suite verification:** if you changed code, tests, build logic, or instruction surfaces, run the full project verification suite explicitly after edits and before `finalize` / `write --commit`. Do not rely on a pre-commit hook. Do not waive red suites as "unrelated" or "flaky".

**Tmux CI review for test-bearing turns:** when the cycle runs tests or changes test/build/instruction surfaces, inspect the latest CI tmux-test result. Check the latest run status with `gh run list --workflow CI --limit 1`; if it is already red after runner startup, run `make tmux-ci` locally, fix it, and add deterministic SimWorld coverage for the regression class. If the latest run is queued or in progress, record that it is still pending and continue the turn from local verification evidence instead of waiting for CI to finish. Do not use `gh run watch` as a closeout gate unless the user explicitly asks. An empty-step job with no logs because GitHub never started a runner (for example billing/spending-limit exhaustion) is an external CI-start blocker, not a code/tmux regression; record the annotation and continue with local evidence.

After `finalize` / `write --commit`, do not start more long-running task work for that same turn. Codex hooks in `.codex/hooks.json` and `.codex/config.toml` are a fail-closed backstop, not a replacement for explicit closeout.

```bash
cat <<'RESPONSE' | agent-doc finalize <FILE> --baseline-file <preflight.baseline_file> --stream --origin skill
<template mode: wrap response in `<!-- patch:exchange -->` … `<!-- /patch:exchange -->` (BOTH markers); inline mode: plain text, no markers>
RESPONSE
```

**IMPORTANT — patch markers:** template-mode responses MUST include both the opening AND closing `<!-- patch:exchange -->` … `<!-- /patch:exchange -->` markers, or the write is rejected (`malformed template patchback`) / lost (`0 template patches found`). **Do NOT use the Edit tool for write-back** (concurrent-edit "file modified" errors).

`finalize` requires the cycle to reach `committed` and post-commit `session-check` to pass; on any `session-check` interruption, continue recovery instead of reporting success. `write --commit` shares that fail-closed boundary for repair writes.

**Manual repair / missed patchback rule (all harnesses):** if the user's prompt is already present in the document, do **not** patch the assistant response directly into the file. Use `agent-doc write --commit <FILE>` so repair crosses the normal snapshot/commit boundary in one path. Do not document or follow a manual-repair flow that stops after bare `agent-doc write`. Direct file patching is only acceptable for inserting a missing user prompt into `exchange` before the response exists.

Full detail (ordering, full-suite + tmux-CI review, session-doc staging rule): [runbooks/persist-closeout.md](runbooks/persist-closeout.md), [runbooks/commit.md](runbooks/commit.md).

## Runbooks

Use runbooks for detail that is not needed every turn. Key runbooks: [runbooks/harness-invocation.md](runbooks/harness-invocation.md), [runbooks/planning-dispatch.md](runbooks/planning-dispatch.md), [runbooks/pending-ops.md](runbooks/pending-ops.md), [runbooks/commit.md](runbooks/commit.md), [runbooks/split-spec-files.md](runbooks/split-spec-files.md). `split-spec-files` applies across agent-doc-managed surfaces; custom root files stay opt-in unless they still match the generated baseline. Full catalog: `compact-exchange`, `transfer-extract`, `model-tier-gate`, `command-synonyms`, `compound-task-steering`, `streaming-checkpoints`, `document-format`, `code-enforced-directives`, `jb-cache-conflict`, `baseline-drift`.

**Read each runbook at most once per session.** If you already opened a runbook earlier this session, its content is still in context — reuse it instead of re-reading. Re-open the same runbook only when its content changed or your earlier copy was compacted away. Redundant re-reads waste context tokens and are heaviest on per-cycle shell harnesses (for example Codex re-`cat`ing closeout runbooks every cycle).
## Auto-loop while queue is active (Claude Code)

After a successful `agent-doc finalize` / `agent-doc write --commit` cycle whose `agent-doc session-check` returns OK, check preflight's queue fields:

- `preflight.queue_active == true`
- `preflight.queue_trigger == "auto"` **or** `preflight.queue_trigger == "persisted"` — `auto` is a start trigger only; once the queue is active, a persisted-active queue (`queue_active: true` with no `auto` attribute) is equally continuation-eligible (`#active-queue-persisted-no-continue`). Do not require the `auto` attribute to keep draining an already-active queue.
- `preflight.queue_prompts.len() >= 1`
- `preflight.user_intent_prompt_changes` is empty (a real user prompt mid-loop takes precedence; do NOT auto-loop over it). Managed-component state edits — queue activity toggle, queue item add/strike, backlog/review/done item edits, `queue_active:` frontmatter flip — appear in `prompt_bearing_changes` for compatibility but are filtered out of `user_intent_prompt_changes` so routine session bookkeeping does not block the auto-loop. Likewise, an edit the affectedness classifier scopes as independent of the current turn (`op_affectedness.turn_affected == false`, `#queue-no-stop-unrelated-edit`) is filtered out — an edit unrelated to the active turn never halts the drain; only a real user prompt, which edits the in-scope `exchange` tail and classifies as turn-affecting, preempts.

When all four hold, invoke the `Skill` tool with `skill: "loop"` and `args: "agent-doc <FILE>"` to drive the next cycle from the same Claude Code session. `/loop` self-paces the next invocation and terminates naturally when the queue drains, when the user interrupts, when `agent_doc_queue_max_iterations` (frontmatter or `.agent-doc/config.toml`) is hit, or when the environment hard-cap `AGENT_DOC_QUEUE_MAX_ITERATIONS_HARD_CAP` (default `50`) is exceeded.

Skip the auto-loop on any failed closeout, `session-check` interruption, or `lint-gate` block — those need explicit operator attention. Skip when `preflight.queue_active == false` (queue drained or halted).

This section is Claude-Code-specific. Codex auto-loops via its `Stop` hook in `.codex/hooks.json`; OpenCode currently has no auto-loop. See [runbooks/harness-invocation.md](runbooks/harness-invocation.md) and `tasks/agent-doc/plan-claude-code-queue-auto-loop.md`.
