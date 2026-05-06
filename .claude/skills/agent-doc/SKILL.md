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
- **Harness-native `agent-doc` entrypoints start the binary-owned response cycle** — when the user invokes `/agent-doc <FILE>`, `agent-doc <FILE>`, or the equivalent entrypoint for the active harness, treat that as an executable workflow entry rather than a generic document-editing request. Do not manually patch the final assistant response into the document, and do not report success before `agent-doc finalize <FILE>` or `agent-doc write --commit <FILE>` completes. For Codex/direct-exec paths, run the explicit `agent-doc session-check <FILE>` backstop before ending the turn.
- **Imperative edits are executable directives** — when the user writes `do #id`, `go`, `fix this`, `run tests`, `build + install`, `commit + push`, or similar inside the session document, treat that as authorization to perform the requested repo work from the document context. Do not require the same instruction to be repeated in chat.
- **Manual repo commits must keep the session document on the finalize path** — when a turn includes ordinary repo `commit + push` work, do not stage or commit the active session document in that manual git commit. Commit only the non-session repo files first, let `agent-doc finalize <FILE>` or `agent-doc write --commit <FILE>` own the session-document closeout commit, then push after the binary-owned closeout so the response commit is included.
- **Local tmux stack work spans sibling repos** — in the `agent-loop` workspace, `src/tmux-router` is a first-class development target for `src/agent-doc`. When a task moves generic tmux pane/session behavior out of `agent-doc`, update the sibling crate too and rely on the workspace cargo patch at `.cargo/config.toml` so local builds exercise the moved code.
- **Fresh route panes stay authoritative** — once `agent-doc route` creates a fresh pane for a document, later geometry-only registry churn must not hand dispatch back to an older same-session pane and make the fresh pane disposable.
- **Optimistic fresh-restart retries stay explicit** — when a routed Codex fresh-restart retry never gets back to a dispatch-ready prompt, record the `startup_miss` against the original routed pane and preserve the canonical document path instead of silently treating a replacement pane as the successful retry target.
- **Required SSH fresh retries must drop stale resumed prelude text** — when a resumed Codex streaming turn for a document with `required_ssh_targets` emits assistant prelude text before the failing SSH command, rely on the binary-owned buffering/retry path: the resumed prelude stays buffered until required SSH is proven safe or the turn completes, and a required-SSH fresh retry must discard that stale prelude instead of letting it leak into the committed response.
- **Passive mixed-root sync must stay fail-safe** — when `agent-doc sync --no-autostart` leaves any visible file blocked, preserve the current visible `agent-doc` window layout and warn instead of collapsing the remaining foreign pane set into a new authoritative layout.
- **Preserve user edits** — never overwrite; let `agent-doc write --stream` merge.
- **Show progress** — stream your response in the console so the user sees real-time feedback.

## Workflow

### 0. Pre-flight (single command)

**Detect subcommands** before running the normal workflow:

- `claim <FILE>` → run `agent-doc claim <FILE>` via Bash and stop.
- `compact <FILE>` → run `agent-doc compact <FILE> --commit` and stop.
- `compact exchange <FILE>` → follow [runbooks/compact-exchange.md](runbooks/compact-exchange.md) and stop.

**Auto-update skill:** Run `agent-doc --version` and compare against `agent-doc-version` in this file's frontmatter. If the binary is newer, run `agent-doc skill install --harness claude --reload compact`; if output contains `SKILL_RELOAD=compact`, prompt the user to run `/compact` and re-invoke the skill, then stop. If the command instead reports that the Claude instructions are already up to date, treat the current prompt as stale instruction drift, continue this turn, and use the installed Claude skill as authoritative. If `agent-doc` is missing or versions match, skip. See [runbooks/harness-invocation.md](runbooks/harness-invocation.md) for harness-specific prompting.

**Run preflight:** `agent-doc preflight <FILE>` via Bash. Preflight recovers orphaned responses, auto-attempts recovery+commit for open `response_captured` / `write_applied` cycles, and auto-closes an open `preflight_started` cycle only when `recover` replays a pending/captured response first, when exact hash / safe historical `HEAD` proof shows the old cycle is already closed, or when an empty `preflight_started` cycle has no capture and is older than the bounded stale timeout; otherwise it fails closed before diffing. It also reads claims and computes the diff. It prints JSON. Key fields: `no_changes`, `claims`, `diff`, `baseline_file`, `slash_commands`, `builtin_commands`, `orchestration_request`, `prompt_presets_requested`, `effective_tier`, `required_tier`, `suggested_tier`, `model_switch`, `model_switch_tier`, `agent_model`, `diff_type`.

- If `no_changes: true` → tell the user nothing changed and stop.
- Print any `claims` to the console as a record.
- Use `baseline_file` as `--baseline-file` for every subsequent response-persistence command. Do NOT save your own baseline — preflight's copy is taken at a stable post-commit point.
- First cycle only: if the document is not yet in context, run `agent-doc read <FILE>` to fetch HEAD content. Do NOT read the snapshot file directly.

### 0b. Execute slash commands (if any)

If `slash_commands` or `builtin_commands` is non-empty, handle each **before** responding. See [runbooks/harness-invocation.md](runbooks/harness-invocation.md) for harness-specific dispatch.

- **Skill commands** (`slash_commands`): Claude Code invokes via the `Skill` tool (strip leading `/`, pass remaining args). Other harnesses skip these.
- **Built-in commands** (`builtin_commands`): Write a document note instructing the user to run it at the terminal (e.g., `/compact`, `/clear`). Skip all others.

Trust the preflight output — do not re-validate code fences or blockquotes.

**Binary-owned orchestration request:** if `orchestration_request` is non-null, run `agent-doc orchestrate <FILE> --mode <orchestration_request.mode> --from-exchange` before composing any manual response. Treat this as a blocking dispatch requirement, not advisory prose. If the orchestrate run completes the requested batch cleanly, stop instead of manually simulating the same work in a second response.

**Prompt preset validation:** if `prompt_presets_requested` is non-empty, treat it as proof that preflight already validated the referenced frontmatter `prompt_presets`. Do not manually inline the preset text in the skill; let `agent-doc orchestrate` expand those presets into each task prompt so sequential, `dag`, and `parallel` stay consistent.

### 0c. Model tier gate

Preflight composes `effective_tier` from inline `/model`, `<!-- agent:model -->`, frontmatter, and a diff heuristic. `required_tier` is the hard gate, `suggested_tier` is advisory, and `model_switch_tier` is the resolved tier for the user's inline `/model` request. Full gate behavior: [runbooks/model-tier-gate.md](runbooks/model-tier-gate.md).

**Orchestration intent:** if the user asks to coordinate multiple document tasks in natural language (for example `run these in order`, `chain these`, `fan out these tasks`, `after #a do #b`), route through `agent-doc orchestrate` instead of manually simulating the batch. Prefer the binary-owned `orchestration_request` field from preflight when present; otherwise use [runbooks/command-synonyms.md](runbooks/command-synonyms.md) to map the phrasing to `--mode sequential|parallel|dag`.

**Compound task steering:** if one directive mixes the primary task with clear follow-up clauses (for example `do #ntoc. Add to today's news. commit + push`), normalize it into explicit sequential or dependency-ordered steps before execution instead of treating the entire prose line as one opaque task. Use [runbooks/compound-task-steering.md](runbooks/compound-task-steering.md) for the normalization rules. Keep that steering in the skill/runbook layer unless the user already supplied explicit orchestration metadata.

### 0d. Planning / dispatch

After `preflight`, run `agent-doc plan <FILE>` and use the emitted planning record as the execution contract for the cycle. The planning record is binary-owned and includes:

- `prompt_targets` — ordered prompts that still require a response
- `execution_scope` — whether this cycle may execute repo work normally or must stay in plan/backlog capture only mode
- `repo_actions` — concrete repo work to finish before persistence
- `required_commands` — binary/harness commands the cycle must run
- `pending_mutations` — pending items that must be resolved this cycle
- `handoff` — whether to continue in place or hand off to `orchestrate`, `compact`, `claim`, or another binary surface
- `blockers` — concrete reasons to fail closed instead of improvising

If `blockers` is non-empty, surface the blocker and stop rather than freelancing around it. If `handoff` is `orchestrate`, execute the corresponding `agent-doc orchestrate ...` command from `required_commands` before composing any manual response. Full contract: [runbooks/planning-dispatch.md](runbooks/planning-dispatch.md).

### 1. Respond

- Address the user's changes naturally in the console — the console response IS the document response.
- Respond to new `## User` blocks, prompt-bearing inline edits (blockquotes, comments, edits to previous responses), and structural changes.
- Reconcile the changed exchange tail oldest-first. Do not stop at the newest question; the turn is incomplete until each unresolved prompt in that tail is answered or explicitly grouped into one response. Concretely, each `prompt_target` must be answered or grouped, while `content_edit` items are user corrections to incorporate and `recovery_artifact` / `boundary_artifact` items are normalization signals instead of ordinary conversation.
- Execute the cycle from the planning record instead of re-reading the raw diff ad hoc. If `execution_scope=plan_backlog_only`, stay in plan/backlog capture mode for this cycle even if the raw prompt sounds imperative; do not start repo implementation until a later explicit `do #id ...` turn. Otherwise, if the user edit requests implementation, tests, builds, benchmarks, commits, or pushes, do that work before persistence or stop on a concrete blocker. Do not keep appending "starting/continuing" status prose while the requested work remains undone.

**Response header format (template mode):** use `### Re: topic` markdown headers — **not** bold (`**Re:**`). The `(HEAD)` boundary marker requires real headings. Use h4–h6 for sub-sections within a response.

**Model attribution:** always append the resolved model short name with a spaced em dash: `### Re: topic — gpt-5` or `### Re: topic — opus-4-6`. Use `preflight.agent_model` if non-null (from frontmatter); otherwise use your own model identity (you know what model you are). Never use the harness label (`codex`, `claude`) as the suffix, and never omit it.

**Streaming checkpoints:** for long multi-topic responses, flush partial content at natural breakpoints so the user sees progress. Full procedure + baseline re-save pattern: [runbooks/streaming-checkpoints.md](runbooks/streaming-checkpoints.md).

**Prefer wrapping exchange responses in `<!-- patch:exchange -->`** for clarity. Raw (unwrapped) content also works via boundary synthesis.

**`#agent-doc-bug` plan proof:** when the prompt contract says to create a plan, create the plan file before closeout and cite each plan path explicitly in the response (for example `Plan: tasks/agent-doc/plan-foo.md`). `finalize` / `session-check` now fail closed if the response covers multiple bug reports but cites fewer existing plan files than the contract requires.
**`#agent-doc-bug` scope:** when `agent-doc plan` returns `execution_scope=plan_backlog_only`, the prompt is a report/planning contract. Create the plan file(s), capture the backlog item(s), and explain the deferred implementation boundary instead of editing code in that same cycle.

### 1b. Update pending (template mode)

If the document has an `<!-- agent:backlog -->` (or legacy `<!-- agent:pending -->`) component, mutations go through granular flags on `agent-doc write` (`--pending-add`, `--pending-done <id>`, `--pending-edit "id=text"`, `--pending-clear`, `--pending-reorder`, `--pending-gate`, `--pending-ungate`). Custom pending IDs should use `--pending-add "id=spec1 text"`; leading `--pending-add "[#spec1] text"` is accepted for compatibility but is not the canonical form. Full-replace via `<!-- replace:backlog -->` or `<!-- patch:backlog -->` is rejected. If `pending_reordered: true`, skip reorder this cycle. Full contract: [runbooks/pending-ops.md](runbooks/pending-ops.md).

If you need to rewrite `<!-- agent:icebox -->`, use a template patch block in the response body: `<!-- replace:icebox --> ... <!-- /replace:icebox -->`. Do not try to route icebox mutations through `--pending-add`; those flags still target backlog only.

**Pending capture rule:** if your response identifies concrete follow-up work that should be tracked, add it to `agent:backlog` in the same cycle. Do not leave pending-worthy next steps as exchange-only prose.

**Recommendations vs accepted work:** if the item is only a recommendation and the user has not accepted it yet, include `recommended` in the pending text (for example, `[recommended] Add regression coverage for X`). Do not silently promote hypotheticals or mutually exclusive options the user has not chosen.

**Plan-backed pending items:** if you are adding a pending task that depends on a dedicated plan document, create the plan file first and include that exact plan file path in the pending item text in the same cycle. Do not add an ambiguous pending item first and defer the plan-file path to a later cycle.

**Default ordering:** new pending items belong at the beginning of `agent:backlog`. When adding multiple new items in one cycle, preserve the order you presented them in. Exception: if you are adding a later step from an ordered batch that is already partially captured in pending, insert the new item adjacent to its predecessor instead of blindly prepending it above earlier steps. Use a canonical custom id on add plus `--pending-reorder` in the same cycle so a newly-added Step 3 lands after the existing Step 2 item (for example, after `#9pw9`). If the predecessor is not already present, fall back to normal front insertion. If the user explicitly reorders pending and preflight reports `pending_reordered: true`, do not reorder existing items that cycle beyond the front insertion behavior for genuinely new work.

**Promotion heuristic (when to add pending in the same cycle):** if your response ends with a numbered list of distinct, actionable recommendations (e.g., "What I'd recommend: 1. ..., 2. ..., 3. ..."), and either pending is currently empty OR the user asked for a backlog / "tasks" / "todo", add each recommendation as a pending item in the same cycle. The pending component is where backlog lives. Skip promotion when items are hypotheticals, options you're asking the user to choose between, or already captured elsewhere.

**`do #id` closeout rule:** when the user directs `do #id ...` and that id already exists in `agent:backlog`, decide the pending outcome before persistence. If the work completed in this cycle, include `--pending-done <id>` in the same write/finalize command. If the task is code-complete but waiting on an external gate, use `--pending-gate <id>`. If it stays open, say why concretely in the response. Do not leave completed `#id` work open in pending by accident.

### 2. Persist the response (MANDATORY — never skip)

Complete the requested implementation, verification, build/install, and local inspection work for this turn **before** this step. The response persistence command is the final document-mutation boundary for the cycle, not an intermediate progress checkpoint.

**Agent harnesses own full-suite verification:** if you changed code, tests, build logic, or instruction surfaces, run the full project verification suite explicitly after the edits and before `finalize` / `write --commit`. Do not rely on a pre-commit hook to do this for you.

**Do not waive red suites as "unrelated" or "flaky":** when the full verification suite is failing, treat that as a real blocker even if the failing tests do not appear to touch the code you just changed. Fix the failure in the same turn or report the concrete blocker and capture the follow-up in backlog; do not declare success while the suite is red.

**Session document staging rule:** if the turn also asks for ordinary repo `commit + push`, keep the session document out of that manual git commit. The allowed order is: finish the non-session repo work, commit only those non-session files if needed, run `agent-doc finalize <FILE>` or `agent-doc write --commit <FILE>` so the session document crosses its own binary-owned commit boundary, then push after closeout.

For the normal response cycle, pipe the response through `agent-doc finalize --stream` so the write crosses the commit boundary in one binary-owned path. **This step is MANDATORY every cycle unless the user explicitly told you to leave the response uncommitted.**

```bash
cat <<'RESPONSE' | agent-doc finalize <FILE> --baseline-file <preflight.baseline_file> --stream --origin skill
<your response — patch blocks for template mode, or plain text for inline mode>
RESPONSE
```

`finalize` reuses the normal write pipeline, then requires the cycle to reach `committed` and the post-commit `session-check` guard to pass before it exits successfully. Use [runbooks/commit.md](runbooks/commit.md) for the default/exception contract.

**End-of-turn guard:** for git-backed docs, `agent-doc finalize <FILE> ...`, `agent-doc write --commit <FILE> ...`, and `agent-doc repair <FILE>` now run the `session-check` closeout guard internally before they return success. If one of those commands surfaces a `session-check` interruption, the cycle is still open or the document shows a likely direct assistant patchback that bypassed `agent-doc`: do **not** report success, and continue recovery instead of ending the turn. The only self-heal exception is already-committed historical snapshot drift that `session-check` can prove from `HEAD`.

For real session documents (`agent_doc_session` / legacy `session`), `agent-doc write --commit <FILE>` now shares `finalize`'s fail-closed commit boundary for response writes: non-git docs are rejected before mutation, commit failure is a command failure, and success means the cycle reached `committed`. Best-effort `--commit` remains only for non-session docs and `--pending-only` maintenance.

`session-check` still enforces two pending backstops: the pending-capture guard warns (or fails closed under `pending_capture_guard: strict` / project `[guards] pending_capture = "strict"`) when a committed response contains recommendation batches but no `--pending-add` / `--pending-add-gated` was recorded, and the pending-done guard warns (or fails closed under `pending_done_guard: strict` / project `[guards] pending_done = "strict"`) when a committed response appears to complete an existing `#id` task but no matching `--pending-done <id>` was recorded. `<!-- no-pending-capture -->` and `<!-- no-pending-done-guard -->` suppress the respective guard for that cycle. It also fails closed on likely bypassed patchbacks that leave bare prompt-target lines without the binary-owned `❯ ` prefix. Optional closeout sidecars such as cycle-state, capture, startup-miss, ops-log, pre-response, and CRDT files are advisory; if one disappears between discovery and read, treat that `NotFound` as absence instead of surfacing a transient `ENOENT`. On the binary side, `agent-doc repair` must still canonicalize those template-doc prompt prefixes even when the response body is already present and the repair outcome is `AlreadyApplied`. You can still run `agent-doc session-check <FILE>` directly for debugging or if a hook tells you to.

After `finalize` / `write --commit`, do not start more long-running task work for that same turn. The only allowed follow-up is minimal recovery if the binary-owned closeout guard failed, plus concise result reporting.

**Codex hook backstop:** the Codex install also writes `.codex/hooks.json` plus `.codex/config.toml` with `features.codex_hooks = true`. Those `UserPromptSubmit` / `Stop` hooks track the active `agent-doc` file across nested `.agent-doc` roots in the same workspace. On `Stop`, the hook first tries to finish the response cycle deterministically from `last_assistant_message`, but only when that payload validates as a single assistant closeout. If the payload looks like a transcript dump or full exchange patchback, the hook stores it only as diagnostics and blocks/fails closed instead of replaying it. Treat that as a safety backstop, not a replacement for explicitly running `finalize` / `write --commit`.

**IMPORTANT: Do NOT use the Edit tool for write-back.** It is prone to "file modified since read" errors when the user edits concurrently.

**IMPORTANT: The response content MUST include `<!-- patch:exchange -->` blocks for template-mode documents.** If the heredoc is empty or contains only raw text without patch markers, the binary will warn (`0 template patches found`) and only apply normalization — the response will be silently lost. Context compaction can drop the response between generation and the write command; if this happens, re-generate the response before piping.

**Manual repair / missed patchback rule (all harnesses):** if the user's prompt is already present in the document and you are repairing a missed patchback, do **not** patch the assistant response directly into the file. Use `agent-doc write --commit <FILE>` for the response write-back so the repair crosses the normal snapshot/commit boundary in one path. On real session docs, that path now fails closed unless the write reaches `committed`. Do not document or follow a manual-repair flow that stops after bare `agent-doc write`. Direct file patching is only acceptable for inserting a missing user prompt into `exchange` before the response exists in the document.

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
