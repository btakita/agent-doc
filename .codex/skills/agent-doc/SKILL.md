---
description: "Interactive markdown session for Codex. TRIGGER: user writes agent-doc <file> as a normal Codex message. Requires a markdown session document, installed CLI, and write+commit every cycle. Do not use slash commands; Codex rejects project-defined /agent-doc."
user-invocable: true
argument-hint: "<file>"
agent-doc-version: "0.35.264"
---

# agent-doc

Interactive document session — respond to user edits in a markdown document.

## Harness Compatibility

This shared hot path serves Claude Code, Codex, OpenCode, Cursor, and direct harnesses. Invocation wording is harness-specific; workflow and closeout are shared. Use [runbooks/harness-invocation.md](runbooks/harness-invocation.md) for harness details and [runbooks/commit.md](runbooks/commit.md) for closeout.

## Dynamic Context Map

Use this SKILL.md as the hot-path router. Load linked files only when their branch is active: invocation and harness drift → [runbooks/harness-invocation.md](runbooks/harness-invocation.md); preflight planning → [runbooks/planning-dispatch.md](runbooks/planning-dispatch.md); response and backlog updates → [runbooks/respond.md](runbooks/respond.md) plus [runbooks/pending-ops.md](runbooks/pending-ops.md); persistence and manual repair → [runbooks/persist-closeout.md](runbooks/persist-closeout.md) plus [runbooks/commit.md](runbooks/commit.md); context-authoring policy → [runbooks/dynamic-context.md](runbooks/dynamic-context.md); durable concept definitions and vocabulary → [okf/index.md](okf/index.md). Do not copy runbook or OKF detail back into this file unless it is required every cycle.

## Invocation

Codex does not support project-defined slash commands. Do **not** type `/agent-doc`; the Codex CLI will reject it before these instructions run.

In Codex, invoke agent-doc by writing one of these as a normal message:

```
agent-doc <FILE>
agent-doc claim <FILE>
agent-doc compact <FILE>
agent-doc compact exchange <FILE>
```

Arguments: `FILE` — path to the session document (e.g., `plan.md`).

Claude Code slash-command equivalents are `/agent-doc <FILE>`, `/agent-doc claim <FILE>`, `/agent-doc compact <FILE>`, and `/agent-doc compact exchange <FILE>`.
## Hot Path Digest

- **Document is the UI** — user edits ARE the prompt; respond in the document and console.
- **Harness-native `agent-doc` entrypoints start the binary-owned response cycle** — treat `/agent-doc <FILE>`, `agent-doc <FILE>`, or equivalent as executable workflow start, not a generic document-editing request. Do not manually patch the final assistant response into the document, and do not report success before the binary resolves and commits the turn through `agent-doc respond <FILE>` (`finalize` compatibility alias) or `agent-doc write --commit <FILE>`. A semantically complete response checkpoint may already exist in Lazily/the document, but it is not proof that queue/backlog closeout or commit ran. Codex/OpenCode paths operating through direct exec also run `agent-doc session-check <FILE>` as the explicit distrust guard. Connected clients consume the terminal `agent_doc_finalize` result, including its queue-continuation fields, and do not invoke a second session check; the next turn's admit/preflight re-evaluates the same document integrity guards.
- **Imperative edits are executable directives** — `do #id`, `fix this`, `run tests`, `build + install`, `commit + push`, and similar edits authorize repo work. Do not require the same instruction to be repeated in chat.
- **Project-scoped remote hosts** — globally approved SSH commands, ambient SSH config, and unrelated project history are not evidence that a named remote host belongs to the current document's project. Use a named remote host only when the current user prompt, this session document/frontmatter, project-local `.agent-doc/config.toml`, or project-local runbooks explicitly identify it; otherwise ask or record a follow-up to confirm the intended host.
- **Preflight is binary-owned (`#preflightinbinary`, `#hookcontractlost`)** — the turn context must contain the `[agent-doc] cycle contract (preflight already ran in the binary; ...)` success marker. Consume the contract sealed by that marker; never shell back to `agent-doc preflight <FILE>` or poll authority from the model turn. The hook always names its outcome on stdout, so read three states: that marker ⇒ admitted; `[agent-doc] cycle contract UNAVAILABLE (preflight admission failed; ...)` plus its `reason:`/`remedy:` lines ⇒ the hook ran and preflight refused, so report the reason and stop without recreating admission; neither ⇒ the hook never ran, a fail-closed harness-admission defect to repair.
- **Missed response repair is not child-agent dispatch** — when preflight/commit says the prior response is already committed and no new assistant body exists, recover through `agent-doc write --commit <FILE>` instead of rerunning an empty response cycle.
- **MCP auth / OAuth steps are sub-steps, not closeout boundaries** — after auth/browser approval, resume and still finish through the connected `agent_doc_finalize` boundary (or `finalize` / `write --commit` plus `session-check` on a direct-exec path).
- **Manual repo commits keep the session document on the finalize path** — stage and commit only the intended non-session repo files first, stop on any stage failure, verify the staged diff still matches the intended path set, then let `finalize` / `write --commit` own the session document.
- **Dispatch proof language must preserve scope** — when reading route diagnostics, `proof=accepted proof_scope=accepted_only` means pane-input acceptance only. Do not describe Claude Code/OpenCode dispatch-only routes as consumed/submitted unless logs show dispatch-start proof.
- **Starting actor reroutes are prompt-gated** — if route diagnostics mention a `starting` authoritative actor, treat dispatch as valid only after the live pane shows a harness-specific dispatch-ready prompt; otherwise the route path must fail closed before input.
- **Rare routing / tmux / startup-miss invariants live in runbooks** — consult [runbooks/harness-invocation.md](runbooks/harness-invocation.md), [runbooks/commit.md](runbooks/commit.md), and [runbooks/code-enforced-directives.md](runbooks/code-enforced-directives.md) for route/start/sync/session-check or sibling `src/tmux-router` work.
- Preserve user edits through Lazily-owned semantic response checkpoints and the final seal. Operator-visible document text is authoritative: never recover, patch, or hook-closeout by replacing it with `content_ours`, a snapshot, or a lazily visible-write receipt if that would drop operator text. Snapshots and incomplete token captures are backup/audit state, not hot-path authority; fail closed or retry through the editor instead. Checkpoint only complete `### Re:` sections with balanced fences/markers; stream incomplete progress only to the console.

## Workflow

### 0. Pre-flight

Detect subcommands before the normal workflow:

- `claim <FILE>` → run `agent-doc claim <FILE>` via Bash and stop.
- `compact <FILE>` → run `agent-doc compact <FILE> --commit` and stop.
- `compact exchange <FILE>` → follow [runbooks/compact-exchange.md](runbooks/compact-exchange.md) and stop.

**Auto-update skill:** Compare `agent-doc --version` to `agent-doc-version`. If newer, run `agent-doc skill install --harness codex` without a reload request. After a real update, re-read the installed `.codex/skills/agent-doc/SKILL.md` completely and continue the same turn under those instructions. Do not call `agent-doc session restart-supervisor`, stop, or ask the user to restart: replacing an active Codex child interrupts the conversation, while the explicit re-read loads the updated workflow in place. If install says already up to date, treat it as stale instruction drift, continue this turn, and use the installed Codex instructions. If `agent-doc` is missing or versions match, skip. See [runbooks/harness-invocation.md](runbooks/harness-invocation.md).

**Preflight runs in the binary (`#preflightinbinary`)** — the `UserPromptSubmit` hook runs it when the `agent-doc <FILE>` trigger arrives, so the contract is already in context and sealed by the trailing `[agent-doc] cycle contract ...` success marker. Do **not** run `agent-doc preflight <FILE>` from the model turn. A missing marker is a fail-closed harness-admission defect, not a fallback path. Preflight owns recovery before diffing and prints the cycle contract: `no_changes`, `warnings`, `claims`, `slash_commands`, `builtin_commands`, `orchestration_request`, `prompt_presets_requested`, tier/model fields, `agent_model`, `diff_type`, and the diff contract.

- If `no_changes: true` → tell the user nothing changed and stop.
- Surface any `warnings`; for `harness_mismatch`, note that the document-declared agent differs from the active harness and continue with the active harness attribution/closeout path.
- Print any `claims` to the console as a record.
- The cycle baseline is binary-owned: preflight captures it into `state.db` cycle state at a stable post-commit point, and `respond` / `write --commit` read it from there. There is no `--baseline-file` flag on any response-persistence command — do NOT pass one, and do NOT save your own baseline.
- First cycle only: if the document is not yet in context, run `agent-doc read <FILE>` to fetch HEAD content. Do NOT read the snapshot file directly.

### 0b. Slash Commands

Handle non-empty `slash_commands` or `builtin_commands` before responding. Claude Code invokes `slash_commands` via the `Skill` tool; other harnesses skip them. For `builtin_commands`, tell the user to run the command at the terminal. Trust preflight; do not re-validate fences or blockquotes. If `orchestration_request` is non-null, run `agent-doc orchestrate <FILE> --mode <orchestration_request.mode> --from-exchange` before manual response composition. If `prompt_presets_requested` is non-empty, let `orchestrate` expand the validated presets.

### 0c. Model Tier

Read `effective_tier`, `required_tier`, `suggested_tier`, and `model_switch_tier` from preflight; follow [runbooks/model-tier-gate.md](runbooks/model-tier-gate.md). For `run these in order`, `fan out`, or `after #a do #b`, prefer binary-owned `orchestration_request`; otherwise use [runbooks/command-synonyms.md](runbooks/command-synonyms.md) and [runbooks/compound-task-steering.md](runbooks/compound-task-steering.md).

### 0d. Plan / Dispatch

After preflight, run `agent-doc plan <FILE>` and treat `prompt_targets`, `execution_scope`, `repo_actions`, `required_commands`, `pending_mutations`, `handoff`, and `blockers` as the execution contract. Stop on `blockers`. If `handoff=orchestrate`, run the emitted `agent-doc orchestrate ...` command before manual response. If `handoff=compact`, follow the emitted compact/restart instruction and stop before repo work or finalization. Full contract: [runbooks/planning-dispatch.md](runbooks/planning-dispatch.md).

### 1. Respond

- Address the user's changes naturally in the console; that response is the document response.
- Keep routine progress in the console; persist a standalone pre-closeout conclusion with `agent-doc salient-checkpoint <FILE>` (non-final, no queue-answer/commit authority, removed by final response). See the streaming runbook.
- Reconcile the changed exchange tail oldest-first. Do not stop at the newest question; answer or group each unresolved prompt in that tail and each unresolved `prompt_target`; treat `content_edit` items as user corrections.
- Execute from the planning record. If `execution_scope=plan_backlog_only`, stay in plan/backlog capture mode. Otherwise complete the requested repo work before persistence or stop on a blocker. Do not keep appending "starting/continuing" status prose while the requested work remains undone. When draining a free-text queue head (no `#id`), quote it as a `> **Queue prompt:**` blockquote so `#ftstrike` can strike it (see [runbooks/respond.md](runbooks/respond.md), `#qdeferstrike`).

**Response header format (template mode):** use `### Re: topic` markdown headers — **not** bold (`**Re:**`). The `(HEAD)` boundary marker requires real headings.

**Model attribution:** always append the resolved model short name with a spaced em dash: `### Re: topic — gpt-5` or `### Re: topic — opus-4-6`. Use `preflight.agent_model` if non-null; otherwise use your own model identity. Never use the harness label (`codex`, `claude`) as the suffix, and never omit it.

Full detail (session-accretion anchors, streaming checkpoints, `#agent-doc-bug` plan proof): [runbooks/respond.md](runbooks/respond.md).

### 1b. Update backlog (template mode)

Mutate `<!-- agent:backlog -->` only through granular `agent-doc write` flags (`--backlog-add`, `--done <id>`, `--backlog-edit "id=text"`, `--backlog-gate`, `--backlog-ungate`, `--backlog-reorder`, `--review-add`/`--review-edit`, `--icebox-add*`); full-replace via `patch:backlog`/`patch:review` is rejected.

**Backlog capture rule:** if the response creates concrete follow-up work, add it to `agent:backlog` in the same cycle. Put new items at the beginning of `agent:backlog`. When one `agent-doc write` carries several `--backlog-add` flags, they land in flag order top-down — the first flag is topmost ("what you read is what you get") — and the `agent:queue` backlog mirror matches that order; for a specific interleave with existing items use `--backlog-add-after`/`--backlog-add-before`, which the queue mirror honours too — an anchored item lands directly after its anchor's queue head rather than at the top (`#queuemirrororder`). If you are extending an ordered batch already in backlog, insert the new item adjacent to its predecessor. To fix an already-queued order, use `--backlog-reorder`: it cascades into the `agent:queue` mirror, permuting only the named live heads among the slots they already occupy (`agent-doc queue sync` cannot — it skips ids already present). If the item is only a recommendation, include `[recommended]`.

**Plan-backed backlog items:** create the plan file first and include that exact plan file path in the backlog text. For multi-phase implementation work, prefer one backlog ID per actionable phase (for example `#crdtrespfx1`, `#crdtrespfx2`) instead of one parent ID that gets repeatedly `--backlog-gate`d after partial progress; keep the parent plan file as context, but queue and close out concrete phase IDs.

**`do #id` closeout rule:** record `--done` (completed), `--backlog-gate` (code-complete, awaiting review/external validation), or explain concretely why it stays open — `session-check` enforces `pending_done_guard`.

**Complete over gate (default to finishing the work).** A turn's job is to **complete** its task, not to file a tracking item. Strongly prefer `--done`: do the implementation, tests, build/install, and verification this cycle and close the item. Use `--backlog-gate` / `agent:review` (a gated `[/]` item) **only under exceptional conditions** — work that is genuinely blocked on something this turn cannot do (a required live editor/pane verification, an external approval, a CI/billing outage), and say exactly what unblocks it. Do **not** gate to avoid effort, to "track for later" what you could finish now, or to record a hypothesis. When follow-up work is real but not blocked, put it in `agent:backlog` as an **actionable** item, not in `agent:review`. If a gated review item's blocking condition is not actually met or is stale, convert it to an actionable backlog item (or `--done` it if already satisfied) and remove the review item — keep `agent:review` small (target < 10) so the actionable list stays legible. Prefer automated completion detection (log/state checks the binary can evaluate) over a human-gated review item wherever possible. **Diagnosis is not a deliverable (`#diagnose-then-fix`):** when a turn diagnoses a reported bug, the SAME turn fixes it — implement, add regression coverage, run the full suite, build/install, `--done`. Filing backlog items for defects you already understand is not closeout; it turns finished analysis back into unstarted work. "It spans several crates", "deserves a focused cycle", and "I did not want to land a partial change" are stalls: land what you have proven and leave only a genuinely blocked remainder, with survivors at the TOP of `agent:backlog`.

Full detail (cross-document rule, icebox, `agent:done`): [runbooks/respond.md](runbooks/respond.md) and [runbooks/pending-ops.md](runbooks/pending-ops.md).

### 2. Persist the response (MANDATORY — never skip)

Complete requested implementation, verification, build/install, and local inspection **before** this step. Complete response sections may already have crossed `agent-doc response-checkpoint <FILE>`; this step asks the binary to resolve the cumulative response, apply queue/backlog mutations, and commit once. For the normal cycle, pipe the response through `agent-doc respond --stream`. `finalize` remains an accepted compatibility alias, not a distinct required phase. **This step is MANDATORY every cycle unless the user explicitly told you to leave the response uncommitted.**

**Agent harnesses own full-suite verification:** if you changed code, tests, build logic, or instruction surfaces, run the full project verification suite explicitly after edits and before `finalize` / `write --commit`. Do not rely on a pre-commit hook. Do not waive red suites as "unrelated" or "flaky".

**Tmux CI review for test-bearing turns:** when the cycle runs tests or changes test/build/instruction surfaces, inspect the latest CI tmux-test result. Check the latest run status with `gh run list --workflow CI --limit 1`; if it is already red after runner startup, run `make tmux-ci` locally, fix it, and add deterministic SimWorld coverage for the regression class. If the latest run is queued or in progress, record that it is still pending and continue the turn from local verification evidence instead of waiting for CI to finish. Do not use `gh run watch` as a closeout gate unless the user explicitly asks. An empty-step job with no logs because GitHub never started a runner (for example billing/spending-limit exhaustion) is an external CI-start blocker, not a code/tmux regression; record the annotation and continue with local evidence.

The `respond` command is the binary-owned turn-resolution and final document-mutation boundary for the cycle (`finalize` is its compatibility alias). After `respond` / `write --commit`, do not start more long-running task work for that same turn. Codex hooks in user-level `$CODEX_HOME/hooks.json` plus project-local `.codex/config.toml` are a bounded, fail-closed backstop, not a replacement for binary-owned closeout; the supervisor owns retries that outlive a hook budget.

```bash
cat <<'RESPONSE' | agent-doc respond <FILE> --stream --origin skill
<template mode: wrap response in `<!-- patch:exchange -->` … `<!-- /patch:exchange -->` (BOTH markers); inline mode: plain text, no markers>
RESPONSE
```

**IMPORTANT — patch markers:** template-mode responses MUST include both the opening AND closing `<!-- patch:exchange -->` … `<!-- /patch:exchange -->` markers, or the write is rejected (`malformed template patchback`) / lost (`0 template patches found`). **Do NOT use the Edit tool for write-back** (concurrent-edit "file modified" errors).

`respond` requires the cycle to reach `committed` and its binary-owned post-commit session check to pass; on any reported interruption, continue recovery instead of reporting success. `finalize` is its compatibility alias, and `write --commit` shares that fail-closed boundary for repair writes. A connected `agent_doc_finalize` call returns this terminal report and any queue continuation itself; only direct-exec harness paths add the explicit `agent-doc session-check <FILE>` distrust check.

**Manual repair / missed patchback rule (all harnesses):** if the user's prompt is already present in the document, do **not** patch the assistant response directly into the file. Use `agent-doc write --commit <FILE>` so repair crosses the normal snapshot/commit boundary in one path. Do not document or follow a manual-repair flow that stops after bare `agent-doc write`. Direct file patching is only acceptable for inserting a missing user prompt into `exchange` before the response exists. **Captured finalize is binary-owned (all harnesses):** invoke `finalize` once for a response. After the binary durably captures it, `controller_model_backpressure`, a pending ACK, CAS race, or retained visible response is owned by the keyed supervisor worker and harness closeout hook. Do not recapture, re-answer, alternate `finalize` with `write --commit`, kill the controller, or elect `--force-disk`; observe the terminal `session-check` result while the binary resumes the same capture with bounded retry and exact-once commit semantics. An external disk change under a live editor is a separate Lazily decision: exact accept plus replica propagation, a newer editor edit, an exact editor save-flush, or final editor close resolves it. The agent never component-merges that disk candidate.

Full detail (ordering, full-suite + tmux-CI review, session-doc staging rule): [runbooks/persist-closeout.md](runbooks/persist-closeout.md), [runbooks/commit.md](runbooks/commit.md).

## Vision workflow for non-vision models
Non-vision models cannot read `![alt](path.png)` images. Delegate to `agent-doc describe-image <IMAGE> [--provider openai|anthropic] [--prompt "<question>"]` and reason over the stdout text. See [runbooks/describe-image.md](runbooks/describe-image.md). Do not reinvent per-session `claude -p @img` invocations.

## Runbooks

Use runbooks for detail that is not needed every turn. Key runbooks: [runbooks/dynamic-context.md](runbooks/dynamic-context.md), [runbooks/harness-invocation.md](runbooks/harness-invocation.md), [runbooks/planning-dispatch.md](runbooks/planning-dispatch.md), [runbooks/pending-ops.md](runbooks/pending-ops.md), [runbooks/commit.md](runbooks/commit.md), [runbooks/split-spec-files.md](runbooks/split-spec-files.md). `split-spec-files` applies across agent-doc-managed surfaces; custom root files stay opt-in unless they still match the generated baseline. Full catalog: `compact-exchange`, `transfer-extract`, `model-tier-gate`, `command-synonyms`, `compound-task-steering`, `streaming-checkpoints`, `document-format`, `code-enforced-directives`, `jb-cache-conflict`, `baseline-drift`, `describe-image`.

**Read each runbook at most once per session.** If you already opened a runbook earlier this session, its content is still in context — reuse it instead of re-reading. Re-open the same runbook only when its content changed or your earlier copy was compacted away. Redundant re-reads waste context tokens and are heaviest on per-cycle shell harnesses (for example Codex re-`cat`ing closeout runbooks every cycle).
