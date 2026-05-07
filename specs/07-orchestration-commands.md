> Extracted from [07-commands.md](07-commands.md)

# Orchestration Commands

This file covers binary-owned planning/orchestration and the queue surface that reuses the same document lifecycle.

## plan

`agent-doc plan <FILE>`

- Emits the binary-owned planning record for the current cycle:
  - `prompt_targets`
  - `execution_scope`
  - `repo_actions`
  - `required_commands`
  - `pending_mutations`
  - `handoff`
  - `blockers`
- The implementation reuses the same prompt/diff classifiers that power `preflight`.
- `pending_mutations.resolve_existing` signals that a matching open backlog/icebox item will need `--pending-done <id>` if completed this cycle.
- `pending_mutations.expect_add` signals that the response likely needs new backlog capture.
- `execution_scope=plan_backlog_only` suppresses repo implementation work for report/planning contracts such as `#agent-doc-bug`.
- Copied `prompt_presets` frontmatter defines reusable prompts but does not invoke them; plan/preflight must ignore preset definition lines when expanding preset references from added diff text.

## orchestrate

`agent-doc orchestrate <FILE> --mode sequential|parallel|dag [--task TEXT ...] [--from-file TASKS.md] [--from-exchange] [--agent NAME] [--model MODEL] [--dry-run] [--plan]`

- Natural-language orchestration requests are normalized by the skill/runbook layer into this command; the CLI itself expects explicit tasks and mode.
- Task resolution combines repeated `--task` entries, optional task extraction from a file, and optional task extraction from the newest exchange tail.
- Batch-level `preset` / `presets` directives request frontmatter `prompt_presets` and are validated before execution.

### `--dry-run` and `--plan`

- `--dry-run` prints the task labels and exits before preset expansion.
- `--plan` prints the fully expanded per-task prompt after preset expansion and before execution.

### Shared subprocess arg rules

- Orchestration subprocesses inherit explicit harness arg resolution from document frontmatter and config.
- Streaming subprocess stderr is surfaced explicitly on failure instead of collapsing into an "empty response" error.
- When the live session has warn/block accretion and the current diff still contains prompt targets, fresh orchestration agent requests use a bounded response-context pack instead of replaying the full exchange tail. The pack must include the active prompt targets, the current `### Session Summary` block when present, the head of `agent:backlog`, and the `### Re:` turns anchored to the prompt positions in `exchange` (enclosing response for inline edits, immediately previous response for tail prompts).

### Command classification

- Queue/orchestration items starting with `/` are commands; everything else is a prompt.
- Dispatch priority is:
  1. Inline execution for binary-owned commands such as `/model` or `/compact`
  2. Supervisor IPC
  3. `tmux send-keys` through the authoritative pane
  4. Fail closed

## `--mode sequential`

Sequential orchestration reuses the same document lifecycle once per task:

1. Inject `❯ <task>` into `agent:exchange`.
2. Run `agent-doc preflight <FILE>`.
3. Expand requested presets into the concrete task prompt.
4. Send exactly one fresh backend request with no resume/fork carryover.
5. Persist the final response through `agent-doc finalize <FILE> --baseline-file ...`.
6. Run `agent-doc session-check <FILE>` and stop on the first failure.

Additional rules:

- Sequential orchestration requires normal git-backed finalize; `--no-git` is invalid.
- CRDT streaming is provisional only until the final `finalize -> session-check` closeout succeeds.
- Child template responses must already contain a real `patch:exchange` block; orchestration must not rely on write-path synthesis to salvage malformed transcript output.

## `--mode parallel`

- Resolves tasks and presets first, then hands them to the existing worktree fan-out backend.
- The legacy `agent-doc parallel` entry is only a compatibility wrapper over this same dispatch path.

## `--mode dag`

- Supports `id=` and `after=` / `deps=` metadata prefixes on task lines.
- Fails fast on unknown deps, duplicate ids, and dependency cycles.
- Executes in deterministic topological source order.
- Every ready node still runs through the normal single-document lifecycle: inject prompt -> `preflight` -> fresh agent request -> `finalize` -> `session-check`.
- DAG mode is dependency-aware but not concurrent against one session document. Real concurrency belongs to `--mode parallel`.

## `agent:queue`

The `agent:queue` component batches prompts inside the document.

### Syntax

- Single-line prompts use flush-left `- ` list items.
- Multi-line prompts use `~~~prompt ... ~~~` or bare `--- ... ---` fences.
- Control fences:
  - `--- start` / `~~~start`
  - `--- start at <time>` / `--- start <time>`
  - `--- stop` / `~~~stop`

### Data model

- Queue entries are parsed as `Prompt`, `StartFence`, or `StopFence`.
- Activation resolution considers `auto`, inline start fences, exchange-triggered `do queue` / `run queue`, and persisted `queue_active`.

### Preflight queue behavior

Preflight owns queue activation/deactivation and emits queue state fields such as:

- `queue_prompts`
- `queue_active`
- `queue_deferred`
- `queue_start_at`
- `queue_trigger`
- `queue_halted`

Before emitting queue state, preflight may:

- consume a bare start fence
- strip `auto` on drain
- persist `queue_active`
- halt on a stop fence
- halt on a future time gate
- halt when the current head prompt was edited since the snapshot

### Post-commit queue consumption

- After a successful response closeout, required closeouts consume the first prompt from the queue in the same locked read/parse/write cycle.
- The first prompt must be removable from both the live document and the snapshot in a provably identical way; otherwise strict closeouts fail before commit.
- If the queue drains, `auto` is removed and `queue_active` is cleared.
