---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
agent: claude
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  "#spec-test-commit-push": update spec + tests. commit + push

---

## Status

<!-- agent:status patch=replace -->
✅ Fixed the frontmatter body-slice bug in `src/agent-doc/src/frontmatter.rs` so repeated frontmatter rewrites no longer accumulate blank lines before the first body heading. Added regression coverage for exact body preservation across repeated parse/write cycles, then verified the crate with `cargo test`, `cargo build --release`, and `cargo install --path .`. The code change is committed and pushed in `src/agent-doc` as `f4b9f59` on `20260422/codex-agent-backend`.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Compacted exchange. Key decisions: move orchestration and post-preflight dispatch toward binary-owned routing; shared orchestrate prompt is still not a first-class feature; frontmatter prompt presets were planned and #fppr Option A was implemented in src/agent-doc; the cancelled Codex Stop-hook replay plan is implemented in src/agent-doc as acc7478, which blocks transcript-shaped last_assistant_message payloads from being replayed and stores blocked payloads only for diagnostics. Open work remains #fppr and #ha3h.

❯ #fppr: If I change the frontmatter while a claude/codex session is active, will agent-doc pick up the changes?
### Re: #fppr frontmatter changes in an active session — gpt-5

Yes for the next `agent-doc` cycle, but not always immediately and not for every setting.

What picks up frontmatter changes on the next cycle:
- `agent-doc preflight` rereads the document each run, so frontmatter-driven routing like `prompt_presets`, `model`, `env`, and similar document-derived fields are picked up on the next preflight/respond cycle
- `agent-doc plan` and `agent-doc orchestrate` also reread the current file, so `#fppr`-style preset changes are picked up the next time those commands run

What does **not** hot-reload into an already-running harness process:
- startup-time harness settings such as `agent_args`, `claude_args`, and `codex_args` are resolved once in `start` and then held for the lifetime of that running Claude/Codex process
- anything already prepared/sent for an in-flight orchestrate step will keep using the prompt that was built at launch time

So the practical answer for `#fppr` is:
- if you edit `prompt_presets` in frontmatter and then trigger the next `preflight` / `orchestrate` cycle, `agent-doc` should use the updated presets
- if you want changed harness startup args to apply, restart that active Claude/Codex session
- if a task was already launched before the edit, that already-running task will not retroactively re-read the updated preset text

❯ do #fppr. update spec + tests. commit + push
### Re: #fppr prompt presets for orchestrate — gpt-5

Implemented the remaining `#fppr` contract in `src/agent-doc`:
- frontmatter now round-trips `prompt_presets`
- preflight emits and validates `prompt_presets_requested`
- orchestrate resolves `preset ...` / `presets ...` directives and expands labeled preset blocks into sequential, dag, and parallel task prompts
- spec and skill docs now describe the binary-owned preset flow

Verification:
- `cargo test`
- `cargo build --release`
- `cargo install --path .`

Committed and pushed in `src/agent-doc` as `ed38f01` on `20260422/codex-agent-backend`.

❯ How can I define a preset?
### Re: defining a prompt preset — gpt-5

Define it in the document frontmatter under `prompt_presets`, then reference it from the exchange with `preset <name>` or `presets <a>, <b>`.

Example:

```yaml
---
prompt_presets:
  "#1": |
    Today is 2026-04-25.
    Keep the work tree clean.
  release-check: |
    Run the full verification suite before finalize.
---
```

Then in `agent:exchange`:

```md
synchronous orchestra
preset #1
- do #prep
- do #report
```

Or with multiple presets:

```md
parallel orchestra
presets #1, release-check
- do #a
- do #b
```

`agent-doc orchestrate` expands those into each task prompt as labeled blocks like `(preset #1)` before the task text.

❯ Is this agent docs's pre-compact state, why did the prompt "When I call an orchestrate, can I define a prompt that will be applied to every orchestrate task in one place?" not have a prepended `❯ ` in the patchback?
### Re: missing `❯` prefix on that orchestrate prompt — gpt-5

No, that is not a special pre-compact state. It is the normal pre-`finalize` working-tree state for a user edit inside `agent:exchange`.

`❯ ` is added by the binary write path, not at raw edit time. The relevant path is `write::normalize_user_prompts_in_exchange`, which derives prompt-prefix targets from the diff and prefixes them during write-back. In this cycle, `preflight` classified your added line as a `prompt_target` with no matching `recovery_artifact`, which means agent-doc sees it as plain user input that still needs the normal write/finalize pass.

So the rule is:
- a bare prompt before the next `finalize` can be normal
- a bare prompt plus an already-inserted `### Re:` response is not normal; `session-check` treats that as a likely bypassed patchback and reports the missing `❯ ` target
- `compact exchange` is separate; it summarizes/replaces exchange content, but it is not what owns prompt prefixing

So the short answer is: this looks like pre-next-cycle / pre-finalize state, not specifically pre-compact state.

❯ Plan next steps with the "binary-owned prose grammar". Such as how subagents should orchestrate their subtasks. However, src/claudescore tasks/docs.md did finish #ntoc in <10m, so Claude Code figured out how to execute the tasks quickly. The user may instruct suboptimal instructions. Perhaps we could have a plan mode as well to discuss how to orchestrate? Or maybe we should just rely on the agent harness to orchestrate?
### Re: binary-owned prose grammar + plan mode — sonnet-4-6

**Division of responsibility (current):**

Binary owns structure; harness owns strategy. The grammar already exists:
- `sequential/parallel/dag orchestra` keywords declare execution mode
- `preset <name>` expands labeled blocks into each task's prompt
- Task bullets (`- do #id`) are the unit of dispatch
- DAG edges via `depends: #a`

The `#ntoc` result confirms harness-side strategy works — Claude Code reasoned about task boundaries and execution order without explicit direction. The binary grammar provides a machine-readable target the harness can route through `agent-doc orchestrate`.

**The suboptimal-instructions risk:**

When the user writes orchestration intent directly (e.g., `parallel orchestra` when tasks actually conflict), the binary executes as written. The harness can only catch this if it sees the expanded plan before spawning.

**What's missing: `orchestrate --plan` (dry-run)**

Adding `--plan` to `orchestrate` would:
1. Parse the exchange directive (mode + presets + task list)
2. Print the expanded task prompts in full
3. Exit without spawning — lets harness or user inspect before committing

This is the narrowest fix. It gives the harness a verification step without introducing a new concept.

**On a full plan mode:**

A dedicated `&lt;!-- agent:plan --&gt;` component is probably overkill for now. The exchange already serves as a discussion surface — the user can ask "should these be parallel?" and the agent responds before writing the `parallel orchestra` directive. Binary-owned plan mode only adds value if the binary could synthesize an `orchestra` block from prose intent, which requires task-semantic understanding the binary doesn't have. Harness reasoning + binary grammar is the right split.

**Recommended next step:**

Add `agent-doc orchestrate --plan` (dry-run) — concrete, low-effort, closes the verification gap without redesigning the orchestration model.
❯ do #0z3v. preset #spec-test-commit-push
### Re: do #0z3v orchestrate --plan — sonnet-4-6

Implemented `agent-doc orchestrate --plan` in `src/agent-doc` at commit `5547909` on `20260422/codex-agent-backend`.

**What was added:**

- `--plan` flag on `orchestrate` (all three modes: sequential, parallel, dag)
- After resolving tasks and applying preset blocks, prints each task's fully expanded prompt and exits — no agent calls, no git writes
- Output format:

```
[orchestrate] plan — N task(s) (no execution)
[orchestrate] step 1/N: <label>
[orchestrate] --- prompt ---
[orchestrate] <preset block if any>
[orchestrate] <task prompt>
[orchestrate] --- end prompt ---
```

- `--dry-run` remains as-is (prints labels only, before preset expansion)
- `--plan` is the deeper verification: shows exactly what the agent would receive

**Tests:** 3 new tests — sequential exits without finalize calls, preset is expanded in output, parallel exits without calling the parallel runner.

**Spec:** `specs/07-commands.md` updated with `--dry-run` / `--plan` section and example output.

**Verification:** `cargo test orchestrate` (25 passed), `cargo build --release`, `cargo install --path .`.

❯ In src/session-share/tasks/claudescore.md, why are there multiple `<!-- no-pending-capture -->` comments?
### Re: multiple `<!-- no-pending-capture -->` in claudescore.md — opus-4-6

Each `<!-- no-pending-capture -->` is a **per-cycle guard suppressor** — it tells `session-check` not to flag a missing `--pending-add` for that specific response cycle. There are three because three separate response cycles each contained recommendation-like content that the author intentionally chose not to promote to pending:

1. **Line 450** — response listed a validation approach (docker compose, cdk synth, deploy) that reads like recommendations but was explanatory, not actionable backlog.
2. **Line 551** — follows a "committed and pushed" confirmation. The subsequent response about CDK stack completeness listed gaps that could trigger the pending-capture guard.
3. **Line 616** — response listed 4 pre-deploy fixes. The user then explicitly wrote "Add next steps to pending" on the next line, meaning the promotion was intentionally deferred to the next cycle rather than done inline.

Each suppressor is scoped to exactly one response cycle — they don't carry forward. That's why you see multiples: every cycle where the author said "I know this looks like I should be adding pending items, but I'm deliberately not" needs its own annotation.

❯ These comments are noisy. How can we not have them in the committed response patchback?
### Re: noisy `<!-- no-pending-capture -->` in committed documents — opus-4-6

The noise happens because `session-check` scans the *committed response body* for the marker. Agents embed it in exchange content to suppress warnings, so it becomes permanent document clutter. Three options:

**Option A: `--no-pending-capture-guard` flag on `finalize`/`write`** — the agent passes it on the command line when it intentionally skips `--pending-add`. The binary checks the flag instead of scanning the response body. Nothing touches the document.

**Option B: Strip-after-check** — the binary checks for the marker as today, but strips it before committing. The marker is ephemeral signaling, not persisted.

**Option C: Document-level `pending_capture_guard: off`** — already exists in frontmatter. Turns off the guard entirely. All-or-nothing, works today with zero code changes.

**Recommendation:** Option A. It moves the suppression signal to the command boundary where it belongs — same surface as `--pending-add` and `--pending-done`. The inline `<!-- no-pending-capture -->` marker stays supported for backward compat but stops being the recommended path.

❯ Do Option B. preset #spec-test-commit-push
### Re: Option B — strip guard markers before commit — opus-4-6

Implemented strip-after-check for `<!-- no-pending-capture -->` and `<!-- no-pending-done-guard -->`.

**What changed in `src/agent-doc/src/git.rs`:**

- Added `strip_guard_markers()` — removes lines that are exclusively a guard marker. Inline occurrences (marker embedded in other text) are preserved.
- `stage_snapshot_for_commit()` now calls `strip_guard_markers()` after `strip_head_markers()`, so the committed blob never contains the markers.
- `strip_guard_markers_from_disk()` — post-commit cleanup strips markers from the snapshot and working-tree file so they match the committed blob.
- `normalize_transient_agent_doc_markers()` now includes guard markers, so snapshot/file comparisons don't produce phantom diffs from guard markers alone.
- Two new unit tests: standalone markers removed, inline markers preserved.

**Spec updated** in `specs/07-commands.md` to document that guard markers are ephemeral signaling stripped before commit.

**Verification:** `cargo test` (all pass except pre-existing `detect_install_env_treats_codex_thread_id_as_codex`), `cargo build --release`, `cargo install --path .`.

Committed as `b1a4cb9` on `20260422/codex-agent-backend` and pushed.

❯ Why is src/session-share `agent-doc tasks/claudescore.md` not using the orchestrate command?
```
synchronous orchestra
preset #spec-test-commit-push
- do #cenv2
- do #cenv3
- do #cenv4
```
```
  /btw Is this current agent-doc run using the orchestrate command?

    Error: empty response from streaming orchestrate step

    The orchestrate binary needs the harness to produce streamed responses for each step, but it couldn't get them in
    this execution context. So the main agent is now falling back to executing the three tasks (#cenv2, #cenv3, #cenv4)
     manually in sequence — same work, just driven by the skill layer instead of the binary's orchestration loop.
```
### Re: orchestrate empty response in claudescore.md — opus-4-6

Two compounding issues in the orchestrate pipeline:

**1. Frontmatter args not forwarded to subprocesses.** `run_ordered_task_step` in `orchestrate.rs` spawns fresh Claude/Codex subprocesses via `CliAgentRunner::send_fresh_streaming`, which calls `agent::resolve_streaming_for_file` → `build_backend_command` → `default_base_args()`. Those default args include `--permission-mode acceptEdits`, not the `--dangerously-skip-permissions` specified in the document's frontmatter `claude_args`. The subprocess either stalls on a permission prompt or fails with insufficient permissions.

**2. Stderr silently discarded.** `send_streaming` in `agent/claude.rs` pipes stderr (`Stdio::piped()`) but never reads it. `StreamIterator` only reads stdout. When the subprocess fails, the error message is lost. `stream_step_response` in `orchestrate.rs` accumulates stdout chunks and bails at line 724 with "empty response from streaming orchestrate step" when no text arrives — but the actual error (permission denied, subprocess crash, etc.) was on stderr.

❯ Please add both of the compounding issue fixes to pending. Create a dedicated plan file as needed.

The skill fallback (`manually in sequence`) is correct behavior — the skill layer detected the orchestrate failure and drove the tasks sequentially itself. The binary orchestrate path needs both fixes before it can replace the skill-driven fallback.

**Also fixed: adjacent code fence dedup bug.** While investigating this, I discovered that `dedup_exchange_adjacent_lines` in `template.rs` was removing the second of two adjacent code fence lines (`` ``` ``), breaking `component::parse` for documents with back-to-back fenced code blocks (like the pasted orchestrate output). Fixed in `src/agent-doc` commit `ee3a1a9` on `20260422/codex-agent-backend`.
### Re: adding orchestrate fixes to pending — opus-4-6

Added both to pending with a dedicated plan file at `tasks/agent-doc/plan-orchestrate-subprocess-fixes.md`:

- **#orch1** — Frontmatter args (`claude_args`/`codex_args`) not forwarded to orchestrate subprocesses. `build_backend_command` uses `default_base_args()` instead of the document's permission settings.
- **#orch2** — Stderr silently discarded in subprocess streaming. `send_streaming` pipes stderr but never reads it, so subprocess failures surface as "empty response" with no diagnostics.

The plan covers both fixes with verification steps.

❯ do #orch1. preset #spec-test-commit-push
### Re: do #orch1 — forward frontmatter args to orchestrate subprocesses — opus-4-6

Fixed the root cause: `run_ordered_task_step` parsed frontmatter but never resolved `claude_args`/`codex_args` into the subprocess command, so subprocesses always got `--permission-mode acceptEdits` regardless of the document's settings.

**Changes in `src/agent-doc`:**

- **`agent/claude.rs`** — added `structural_base_args()` returning `["-p", "--output-format", "json"]` (structural minimum without permission settings)
- **`agent/codex.rs`** — added `structural_base_args()` returning `["exec", "--json"]` (structural minimum without sandbox settings)
- **`orchestrate.rs`** — added `resolve_orchestrate_agent_args()` using the same precedence chain as `start.rs` (`fm.agent_args > fm.<harness>_args > config.agent_args > config.<harness>_args`), and `build_effective_agent_config()` that merges structural base args with resolved frontmatter/config args. `run_ordered_task_step` now passes the effective config to all runner calls instead of `global_config.agents.get(agent_name)`.
- **`specs/07-commands.md`** — documented the frontmatter args forwarding contract in the orchestrate section

**Tests:** 8 new tests — 5 for `resolve_orchestrate_agent_args` (Claude frontmatter, Codex frontmatter, agent_args precedence, config fallthrough, none case) and 3 for `build_effective_agent_config` (Claude with args, Codex with args, none without args). All 1108 tests pass (1 pre-existing failure in `detect_install_env_treats_codex_thread_id_as_codex`).

**Verification:** `cargo test`, `cargo build --release`, `cargo install --path .`

Committed as `b700caf` on `20260422/codex-agent-backend` and pushed.
<!-- agent:boundary:8676d518 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] [#orch2] Stderr silently discarded in orchestrate subprocess streaming — plan: tasks/agent-doc/plan-orchestrate-subprocess-fixes.md
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Blocked until multi-user workspace support exists; issue #40 explicitly says not to ship this before that prerequisite. Tracked: https://github.com/btakita/agent-doc/issues/40
<!-- /agent:pending -->

<!--
- [ ] [#pdar] [recommended] Evaluate optional binary auto-resolution only for the narrow case where the prompt is explicitly do #id and there is exactly one matching open pending item
-->
