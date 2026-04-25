---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent: codex
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
---



## Status

<!-- agent:status patch=replace -->
✅ Fixed the frontmatter body-slice bug in `src/agent-doc/src/frontmatter.rs` so repeated frontmatter rewrites no longer accumulate blank lines before the first body heading. Added regression coverage for exact body preservation across repeated parse/write cycles, then verified the crate with `cargo test`, `cargo build --release`, and `cargo install --path .`. The code change is committed and pushed in `src/agent-doc` as `f4b9f59` on `20260422/codex-agent-backend`.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260425-042620.md`*

❯ do #plnk. update spec + tests. build + install for local testing. commit + push
### Re: #plnk pending plan-file guidance — gpt-5

Implemented in `src/agent-doc` and pushed on `20260422/codex-agent-backend` as `6b7c7ec` (`docs: require plan paths in pending guidance`).

What changed:
- `runbooks/pending-ops.md` now requires plan-backed pending items to create the plan file first and include the exact plan-file path in the pending text, with a concrete example
- `SKILL.md` now carries the same rule in the pending-mutation guidance so the agent applies it during normal response cycles
- `specs/pending-system.md` now records the rule at the spec layer instead of leaving it as runbook-only guidance
- added bundled-runbook/install assertions in `src/skill.rs` plus installed-skill coverage in `tests/test_cli.rs` so the rule is enforced in the shipped skill surface

Verification in `src/agent-doc`:
- `cargo fmt`
- `cargo test plan_backed_items`
- `cargo test test_skill_md_contains_required_steps`
- `cargo test`
- `cargo build --release`
- `cargo install --path .`

❯ In src/session-share, `agent-doc tasks/claudescore-3.md` has as "Today is 2026-04-25. Synchronous orcestra." prompt. However, agent-doc orcestrate was not called. Why?

❯ I asked `/btw` in the running claudescore-3.md session:
```
  /btw Did the "Today is 2026-04-25. Synchronous orcestra." task run /agent-doc orcestrate?
  /btw How should agent-doc fix this issue?

    file instead of using agent-doc write.

    The pattern matches how other invariants migrated from "agent should do X" to "binary enforces X" — preflight
    detects it, emits a structured field, and the skill instruction makes it a blocking requirement rather than a
    suggestion.
```
### Re: orchestration miss — gpt-5

Why `agent-doc orchestrate` did not run:

- The prompt in `src/session-share/tasks/claudescore-3.md` was plain natural-language text plus a list, not a slash command.
- Preflight only emitted `slash_commands` / `builtin_commands` and prompt-bearing prose. It had no binary-owned field saying "this diff is a batch orchestration request."
- The installed skill already had advisory guidance to map phrases like `synchronous` / `chain` / `orchestrate` to `agent-doc orchestrate --mode sequential`, but that translation still depended on the model following prose instructions. Claude missed it.

I fixed the enforcement gap in `src/agent-doc`:

- `src/diff.rs` now detects natural-language orchestration requests when the user adds a batch-style preamble plus a multi-item task list and resolves `sequential` / `parallel` / `dag`
- `src/preflight.rs` now emits `orchestration_request` in the JSON output
- `SKILL.md` now treats `orchestration_request` as a blocking dispatch: run `agent-doc orchestrate <FILE> --mode <mode> --from-exchange` before any manual response, and stop if that batch completed the turn
- added regression coverage for the exact `Today is 2026-04-25. Synchronous orcestra.` shape plus preflight/skill-install assertions

Verification:
- `cargo test`
- `cargo build --release`
- `cargo install --path .`
- `agent-doc skill install --harness claude --reload compact`

The short answer is: it missed because orchestration intent lived only in skill prose. The fix is to move detection into preflight so the agent gets a structured, non-optional dispatch signal.

❯ Does agent-doc preflight use the agent model?
### Re: preflight model usage — gpt-5

Not in the sense of "preflight consults the live agent model to reason about the task." `agent-doc preflight` is binary-owned: it classifies the diff, detects commands/orchestration, and computes `suggested_tier` from structural signals.

The model-related parts are narrower:
- It scans inline `/model <x>` and emits `model_switch` / `model_switch_tier`.
- It reads frontmatter `model` only to populate `agent_model` for header attribution.
- The helper for `agent_model` is frontmatter-only; it explicitly does not read the shell model env.

So the running agent model is used by the skill after preflight, when it compares the current session tier against `effective_tier` / `required_tier`. Preflight itself does not use the live model for semantic analysis.

❯ Would it be helpful to have the preflight use the agent model to classify or plan?
### Re: preflight model usage follow-up — gpt-5

Probably not inside `preflight` itself.

Using the live agent model to classify or plan would make preflight less deterministic, harder to test, slower, and more harness-dependent. The repo's current direction is the opposite: keep preflight binary-owned and structural, and push as much routing signal as possible into that deterministic layer before any model runs.

What *is* useful:
- keep extending structural detection in preflight when we find misses (`orchestration_request`, `model_switch`, diff heuristics, pending/path/code-block signals)
- let the skill or orchestrator use the running model *after* preflight for semantic work like planning, decomposition, or subagent dispatch
- if we ever want a model-assisted step, make it a separate explicit phase after preflight, not part of the core preflight contract

So my recommendation is: no model-based planning/classification inside `agent-doc preflight`; yes to a later model-driven dispatch/planning layer that consumes preflight's structured output.

❯ add recommendation to pending
### Re: pending capture — gpt-5

Added the recommendation to `agent:pending` as a `[recommended]` item rather than silently promoting it to accepted work.

❯ Will #1g42 be more effective than just having the model process the raw preflight output?
### Re: #1g42 effectiveness — gpt-5

Yes, if the goal is reliable behavior rather than best-effort behavior.

Just handing the model raw preflight output is still mostly "the model noticed the right thing and chose the right action." That helps, but it preserves the same failure mode as the orchestration miss: the signal exists, yet dispatch still depends on prompt-following.

`#1g42` is stronger because it creates an explicit phase boundary:
- preflight produces structured facts
- a dedicated planning/dispatch step consumes those facts
- the skill/orchestrator can require specific outputs or actions from that step

So `#1g42` gives you a more testable contract and a narrower place to harden dispatch logic. Raw preflight output alone is useful context, but it is weaker as an enforcement mechanism.

❯ Create a dedicated plan file for #1g42.
### Re: #1g42 plan file — gpt-5

Created `tasks/agent-doc/plan-post-preflight-dispatch-phase.md` for `#1g42` and updated the pending item to point at that exact plan path in the same cycle.

The plan turns the earlier recommendation into a concrete design artifact: it defines the problem, keeps `preflight` deterministic, compares a skill-side planning record against a future binary-owned `agent-doc plan` command, and lists the likely runbook/skill surfaces to change.

No code/test/build work was needed for this cycle because the requested change was the dedicated plan document itself.

❯ synchronous orcestra
❯ - do #1g42 Option A. update spec + tests. build + install for local testing. commit + push
❯ - do #1g42 Option B. update spec + tests. build + install for local testing. commit + push
### Re: #1g42 Option A + Option B — gpt-5

Implemented `#1g42` in `src/agent-doc` and pushed `7dcae48` on `20260422/codex-agent-backend` (`feat: add post-preflight planning command`).

I landed Option A and Option B together as one atomic change because the skill contract and the binary command need to ship in sync.

What changed:
- added `agent-doc plan <FILE>` in `src/plan.rs` and wired it through `src/main.rs`; it emits a structured planning record with `prompt_targets`, `repo_actions`, `required_commands`, `pending_mutations`, `handoff`, and `blockers`
- updated the bundled skill, runbooks, and `specs/07-commands.md` so post-preflight dispatch now runs through that planning record instead of raw diff/prose improvisation
- kept orchestration routing binary-owned: the planner consumes the orchestration signal and emits an explicit `agent-doc orchestrate ... --from-exchange` handoff when needed
- added planner tests plus skill-install/runbook assertions

Verification:
- `cargo test`
- `cargo build --release`
- `cargo install --path .`
- `agent-doc skill install --harness codex --reload restart`
- `agent-doc skill install --harness claude --reload compact`

I marked `#1g42` done for this cycle. I left the pre-existing unstaged `src/agent-doc/Cargo.lock` change out of the commit.

When I call an orchestrate, can I define a prompt that will be applied to every orchestrate task in one place?
### Re: shared orchestrate prompt — gpt-5

Not as a first-class `agent-doc orchestrate` option today.

What exists now:
- The CLI surface is only `--task`, `--from-file`, `--from-exchange`, `--agent`, and `--model` in addition to the mode flag; there is no `--shared-prompt` or prompt-prefix flag in the command contract.
- Sequential and DAG orchestration do give every step the full current session document, because each step builds the agent prompt from the diff plus the entire `<document>`. So if you put batch-level guidance in the session doc, every sequential/DAG step will see it.
- Parallel orchestration is different: it writes each task text directly to `.agent-doc-prompt.txt` and runs that task in isolation. There is no shared per-batch prompt layer there beyond the normal project instruction files / backend defaults.

So the practical answer is:
- `sequential` / `dag`: yes indirectly, by putting the shared instruction in the session document before running `orchestrate`
- dedicated `orchestrate` prompt prefix: no, not currently
- `parallel`: no equivalent shared task-prefix surface today

If you want this as an explicit product feature, the right shape is probably something like `agent-doc orchestrate ... --shared-prompt-file <path>` or `--task-prefix <text>` so the behavior is binary-owned instead of relying on document placement.

❯ Please create a dedicated plan and pending item.
❯ Please create a dedicated plan and pending item for frontmatter prompt presets. Perhaps the prompt preset can be invoked using `preset #1` for the preset labeled `#1` in the frontmatter. The preflight and/or prepare prompt step should expand insert the actual preset text with a `(preset #1)` prefix.
### Re: frontmatter prompt presets plan — gpt-5

Created `tasks/agent-doc/plan-frontmatter-prompt-presets.md` and added a new open pending item `#fppr` pointing at that exact plan path in the same cycle.

The draft treats this as a binary-owned feature rather than a skill-only convention:
- define reusable `prompt_presets` in document frontmatter
- allow exchange-side invocation like `preset #1`
- have `preflight` detect and validate preset references as structured data
- expand the selected preset during prompt preparation with a visible `(preset #1)` prefix
- keep that expansion path consistent across `sequential`, `dag`, and `parallel`

No code, tests, build, or install work was needed for this cycle because the requested change was the dedicated design artifact plus backlog capture.
<!-- agent:boundary:4d34ec08 -->
<!-- /agent:exchange -->

<!--
Why did the prompt "When I call an orchestrate, can I define a prompt that will be applied to every orchestrate task in one place?" not have a prepended `❯ ` in the patchback?
-->

<!--
Plan next steps with the "binary-owned prose grammar". Such as how subagents should orchestrate their subtasks. However, src/claudescore tasks/docs.md did finish #ntoc in <10m, so Claude Code figured out how to execute the tasks quickly. The user may instruct suboptimal instructions. Perhaps we could have a plan mode as well to discuss how to orchestrate? Or maybe we should just rely on the agent harness to orchestrate?
-->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] [#fppr] Add frontmatter prompt presets for orchestrated tasks; support exchange-side `preset #1` style references that expand into the prepared task prompt with a visible `(preset #1)` label. Plan: tasks/agent-doc/plan-frontmatter-prompt-presets.md
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Blocked until multi-user workspace support exists; issue #40 explicitly says not to ship this before that prerequisite. Tracked: https://github.com/btakita/agent-doc/issues/40
<!-- /agent:pending -->

<!--
- [ ] [#pdar] [recommended] Evaluate optional binary auto-resolution only for the narrow case where the prompt is explicitly do #id and there is exactly one matching open pending item
-->
