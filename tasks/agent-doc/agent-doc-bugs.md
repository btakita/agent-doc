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
*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260425-001747.md`*

❯ Please plan a command for agent-doc to orchestrate sequential tasks. It could be `sequential`, `chain`, or another good name. I also plan on support additional orchestration patterns such as fan out, fan in, dependency graph, etc.

❯ See src/session-share/tasks/claudescore-3.md

❯ Here is the prompt I used:
```
Please run the following prompt chain sequentially in a claude-opus-4-6 subagent. I don't want to accumulate context and I want to avoid compaction so please follow the instructions. Inject each prompt into the document. Follow the full agent-doc lifecycle for each chained task. e.g. Write the patchback into this document after each task completes.
- do #bt0q. Add tests. Run benchmarks. Add to standing today's news. Use Line Graphs. Include runs from previous 2 days + today. commit + push
- do #keab. Add tests. Run benchmarks. Add to standing today's news. Use Line Graphs. Include runs from previous 2 days + today. commit + push
- do #1vqr. Add tests. Run benchmarks. Add to standing today's news. Use Line Graphs. Include runs from previous 2 days + today. commit + push
- do #t107. Add tests. Run benchmarks. Add to standing today's news. Use Line Graphs. Include runs from previous 2 days + today. commit + push
- do #09wc. Add tests. Run benchmarks. Add to standing today's news. Use Line Graphs. Include runs from previous 2 days + today. commit + push
- do #18nq. Add tests. Run benchmarks. Add to standing today's news. Use Line Graphs. Include runs from previous 2 days + today. commit + push
```
### Re: orchestration command design — gpt-5

I would not make `chain` or `sequential` the primary top-level command. `agent-doc` already has `parallel`, and you want future `fan-out`, `fan-in`, and dependency-graph execution, so the stable surface should be:

```bash
agent-doc orchestrate <FILE> --mode sequential
agent-doc orchestrate <FILE> --mode parallel
agent-doc orchestrate <FILE> --mode dag
```

If you want short sugar later, `agent-doc chain <FILE>` can just alias `orchestrate --mode sequential`, but it should not be the main taxonomy.

Why this shape:
- `sequential` is an execution mode, not the product surface.
- `parallel` already exists in `src/agent-doc/src/parallel.rs`, so a new sibling command like `chain` would split orchestration across unrelated verbs.
- `orchestrate` gives you one place for shared policy: fresh agent vs resume, worktree isolation, failure handling, per-step commit/session-check, and prompt injection into the document.

For the prompt shape in `src/session-share/tasks/claudescore-3.md`, the first MVP should enforce:
1. Fresh agent process per step. Do not reuse conversation state across steps.
2. Before each step runs, append the exact prompt into `exchange` so the document remains the system of record.
3. Execute one full `agent-doc` lifecycle per step: `preflight -> requested repo work -> finalize --stream -> session-check`.
4. Stop on the first concrete blocker by default. Add `--continue-on-error` later if you need batch semantics.
5. Track step state explicitly (`queued`, `running`, `committed`, `failed`) so recovery is deterministic.

Suggested CLI:

```bash
agent-doc orchestrate tasks/...md --mode sequential \
  --agent claude --model claude-opus-4-6 \
  --from-file chain.md
```

If you want document-native extraction, add a second input path later:

```bash
agent-doc orchestrate tasks/...md --mode sequential --from-exchange
```

That mode should extract the latest changed fenced block or bullet list from the exchange tail and turn each bullet into one step.

Implementation order:
1. Build `orchestrate --mode sequential` first with fresh per-step execution and binary-owned closeout.
2. Move the current `parallel` spawn/worktree logic behind the same orchestration engine.
3. Add fan-in / DAG only after step state and recovery semantics are stable.

❯ Add Step 3 into agent:pending
### Re: add Step 3 to pending — gpt-5

Added. I promoted Step 3 into `agent:pending` as its own recommended follow-up item rather than folding it into the step-2 refactor task, since it is a distinct later phase.

❯ You did not add Step 3 in the correct order. It should have been after #9pw9. Please update skill/runboot and/or agent-doc binary to understand where the pending item should be added.
### Re: pending insertion order guidance — gpt-5

Fixed. I updated the `agent-doc` source guidance so later additions from an already-captured ordered batch are no longer described as blind front-insertions. The bundled skill and `pending-ops.md` now tell the agent to keep a newly promoted Step 3 adjacent to its predecessor by adding it with a canonical custom id and reordering it into place in the same cycle.

I added regression checks for that guidance, then verified `src/agent-doc` with `cargo test`, `cargo build --release`, and `cargo install --path .`. The source change is committed in `src/agent-doc` as `6b7346d` (`docs: capture ordered pending insertion guidance`).

I also corrected this document's pending order so the Step 3 item now sits after `#9pw9`.

do #gkke. update spec + tests. build + install for local testing. commit + push 
### Re: orchestrate command implementation — gpt-5

Implemented in `src/agent-doc` and pushed on `20260422/codex-agent-backend` as `0beb3c8` (`feat: add orchestrate command surface`).

`agent-doc orchestrate <FILE>` now exists with `--mode sequential|parallel|dag`, shared task resolution from `--task`, `--from-file`, and `--from-exchange`, and a real sequential path that injects each task into `agent:exchange`, runs `preflight`, sends one fresh agent request with no resume/session reuse, then persists each step through `finalize` followed by `session-check`. `--mode parallel` routes through the existing fan-out engine, and `--mode dag` now fails explicitly as reserved future work.

I updated the command spec / README / version notes and added orchestration-focused unit coverage for task extraction, boundary-aware prompt injection, fresh-session dispatch, and the sequential finalize path.

Verification in `src/agent-doc`:
- `cargo test orchestrate::`
- `cargo build --release`
- `cargo install --path .`
- `agent-doc orchestrate --help`

I also ran the full `cargo test`; the crate compiles, but the run is still blocked by existing unrelated failures in `autoclaim::tests::{autoclaim_focuses_pane_with_claim, autoclaim_syncs_layout_with_multiple_files, autoclaim_no_claim_skips_focus}`.

❯ do #9pw9. update spec + tests. build + install for local testing. commit + push
### Re: parallel orchestration refactor — gpt-5

Implemented in `src/agent-doc` and pushed on `20260422/codex-agent-backend` as `43a1000` (`refactor: route legacy parallel through orchestrate`).

`agent-doc parallel` now routes through the shared orchestrate dispatcher instead of bypassing it, so the legacy command and `agent-doc orchestrate --mode parallel` share task normalization and mode routing. I kept the existing parallel backend intact, preserved the legacy empty-task compatibility path, and added regression coverage for shared parallel dispatch plus the compatibility behavior.

I also updated the command spec, README, and version notes to describe the compatibility surface.

Verification in `src/agent-doc`:
- `cargo test orchestrate::`
- `cargo test`
- `cargo build --release`
- `cargo install --path .`
- `agent-doc orchestrate --help`

do #d5xb. update spec + tests. build + install for local testing. commit + push  
### Re: DAG orchestration implementation — gpt-5

Implemented in `src/agent-doc` and pushed on `20260422/codex-agent-backend` as `f0c0afc` (`feat: execute dag orchestration batches`).

`agent-doc orchestrate --mode dag` now accepts dependency-aware task entries such as:

```md
- do #prep. Prepare context
- [after=#prep] do #bench. Run benchmarks
- [id=report after=#prep,#bench] Summarize both results
```

The DAG path validates duplicate / missing / cyclic dependencies, derives ids from `id=...` or the first `#token` in the prompt, then executes the graph in deterministic topological order through the normal single-document lifecycle: inject prompt, `preflight`, fresh agent request, `finalize`, `session-check`. Because all nodes write back to the same session document, sibling nodes are still serialized; fan-in is implemented through multiple `after=` dependencies, while worktree fan-out remains `--mode parallel`.

I updated the command spec, README, version notes, and CLI help text to match the shipped behavior.

Verification in `src/agent-doc`:
- `cargo test`
- `cargo build --release`
- `cargo install --path .`
- `agent-doc orchestrate --help`

❯ do #osyn. update spec + tests. build + install for local testing. commit + push
### Re: orchestrate synonym recognition — gpt-5

Implemented in `src/agent-doc` and pushed on `20260422/codex-agent-backend` as `2d94740` (`docs: add orchestrate synonym guidance`).

Added a bundled `runbooks/command-synonyms.md` and wired `SKILL.md` to use it when natural-language orchestration requests appear, so ordered phrases like `run these in order` / `chain these` map to `agent-doc orchestrate --mode sequential`, concurrency phrases like `fan out` / `concurrent` map to `--mode parallel`, and dependency phrasing maps to `--mode dag`. I also updated `specs/07-commands.md` to document the skill-side dispatch behavior and added regression coverage in `src/skill.rs` plus CLI-installed skill assertions in `tests/test_cli.rs`.

Verification in `src/agent-doc`:
- `cargo test`
- `cargo build --release`
- `cargo install --path .`

Local install check:
- `agent-doc skill install --harness codex`
- `agent-doc skill install --harness claude`

The regenerated `.codex/AGENTS.md`, `.codex/runbooks/command-synonyms.md`, `.claude/skills/agent-doc/SKILL.md`, and `.claude/skills/agent-doc/runbooks/command-synonyms.md` all contain the new orchestrate synonym guidance.

❯ In src/session-share, `agent-doc tasks/docs.md` with "Synchronous orchestra:" uses orchestrate. The `- do #ntoc. Add to today's news. commit + push` would clearly benefit from a fan-out. Can we either have compound instructions or have agent-doc help steer the running session to also orchestrate?
### Re: compound orchestration steering — gpt-5

Yes, but I would split it into two layers.

Right now the shipped surface only does outer-batch orchestration:
- `runbooks/command-synonyms.md` maps phrases like `synchronous`, `fan out`, and `after X do Y` to `agent-doc orchestrate <FILE> --mode ...`.
- `specs/07-commands.md` and `src/orchestrate.rs` then resolve explicit task lists from `--task`, `--from-file`, or `--from-exchange`.

That means `do #ntoc. Add to today's news. commit + push` is still just one opaque task string once execution begins. There is no second-pass rule that says “after the main repo task, orchestrate a news update too”, and I would not make that an implicit free-form parser in the binary. It is too ambiguous about scope, target document, ordering, and where the commit boundary lives.

What I'd do instead:
1. Short term: teach the skill/runbook to steer compound prose into explicit orchestration before execution. In this example, `Synchronous orchestra:` should stay `--mode sequential`, but the task line should be normalized into primary work plus an explicit follow-up task for the news session.
2. Long term: if you want binary-owned composition, use explicit metadata rather than sentence heuristics. Example:
```md
- [id=ntoc] do #ntoc
- [id=news-ntoc after=ntoc] do "Add #ntoc result to today's news"
- [id=push-ntoc after=ntoc,news-ntoc] do "commit + push"
```
That keeps recovery and the `finalize -> session-check` boundary deterministic.

So yes: `agent-doc` should help steer this, but the first fix should be “normalize compound intent into explicit orchestration” rather than “silently infer nested fan-out from arbitrary prose.” I added a recommended pending item for that steering layer.

❯ do #cmps. update spec + tests. build + install for local testing. commit + push
### Re: compound orchestration steering — gpt-5

Implemented in `src/agent-doc` and pushed on `20260422/codex-agent-backend` as `50d01ff` (`docs: steer compound orchestration tasks`).

This stays in the skill/runbook layer rather than the CLI parser:
- added bundled `runbooks/compound-task-steering.md` so compound lines like `do #ntoc. Add to today's news. commit + push` get normalized into explicit sequential or dependency-ordered steps before execution
- wired that guidance into `SKILL.md`, `specs/07-commands.md`, and the bundled runbook install path in `src/skill.rs`
- added regression coverage for bundled-runbook presence/content plus installed-skill output in `src/skill.rs` and `tests/test_cli.rs`

Verification in `src/agent-doc`:
- `cargo test`
- `cargo build --release`
- `cargo install --path .`

❯ do #opcc. update spec + tests. build + install for local testing. commit + push
### Re: orchestrate preflight cycle reuse — gpt-5

Implemented in `src/agent-doc` and pushed on `20260422/codex-agent-backend` as `d4d3928` (`fix: reuse open preflight cycle in orchestrate`).

What changed:
- `src/orchestrate.rs` now lets sequential task 1 reuse a matching open `preflight_started` cycle instead of recursively calling `preflight`; it recomputes the current diff locally after prompt injection and reuses the existing baseline file.
- `tests/run_integration.rs` adds a CLI regression that reproduces `preflight -> orchestrate` on the same document and verifies the first step succeeds.
- `specs/07-commands.md` now documents the inherited-cycle behavior and its task-1-only scope.

Verification:
- `cargo test sequential_orchestration_reuses_matching_open_preflight_cycle_for_first_step`
- `cargo test orchestrate_reuses_open_preflight_cycle_for_first_step`
- `cargo test`
- `cargo build --release`
- `cargo install --path .`

❯ do Option A from tasks/agent-doc/plan-orchestrate-preflight-cycle-conflict.md. update spec + tests. build + install for local testing. commit + push
### Re: Option A from `plan-orchestrate-preflight-cycle-conflict.md` — gpt-5

Implemented in `src/agent-doc` and pushed on `20260422/codex-agent-backend` as `68fcc0f` (`fix: make preflight reruns idempotent`).

What changed:
- `src/preflight.rs` now attempts the normal snapshot-only `git::commit` closeout for an open `preflight_started` cycle before failing, so a second `agent-doc preflight` can safely continue when the prior cycle is already durable and later edits are only working-tree drift.
- `src/orchestrate.rs` no longer carries a task-1-only preflight reuse branch; sequential orchestration now always injects the prompt and reruns `preflight`, with the idempotency handled at the preflight boundary itself.
- `specs/07-commands.md` now documents that `preflight` is the idempotent boundary and that sequential orchestration always uses the same preflight path for every task.
- Updated coverage in `src/preflight.rs`, `src/orchestrate.rs`, and `tests/run_integration.rs` for rerunning preflight and for orchestrating with an already-open preflight cycle.

Verification in `src/agent-doc`:
- `cargo test --manifest-path src/agent-doc/Cargo.toml`
- `cargo build --manifest-path src/agent-doc/Cargo.toml --release`
- `cargo install --path src/agent-doc`

Outcome:
- The `preflight -> orchestrate -> preflight` conflict is fixed at the root preflight boundary instead of through a special orchestrate-only task-1 reuse path.

❯ do #gdir. update spec + tests. build + install for local testing. commit + push
### Re: submodule gitdir access — gpt-5

Implemented in `src/agent-doc` and pushed on `20260422/codex-agent-backend` as `fad7d54` (`fix: add submodule gitdir access for workspace sessions`).

What changed:
- `agent-doc` now detects when a session document lives in a submodule whose real git metadata is outside the working tree and automatically appends harness `--add-dir` access for the external gitdirs the lifecycle actually writes through: the submodule `.git/modules/...` gitdir and the superproject `.git` used for parent-pointer updates.
- The same supplement now applies across the interactive supervisor path (`start`) and fresh-agent backend launches used by `run`, `stream`, and `orchestrate`, so workspace-write Claude/Codex sessions stop tripping permission failures in the middle of the normal git closeout path.
- Claude streaming now preserves configured extra args like `--add-dir` when it switches from JSON to `stream-json`, instead of dropping them on the streaming path.
- Updated the command/config/git specs plus `README.md` / `SPEC.md`, and added regression coverage for external-gitdir discovery and the Claude streaming arg-preservation case.

Verification in `src/agent-doc`:
- `cargo test external_git_dirs_for_submodule_include_submodule_and_parent_gitdirs`
- `cargo test streaming_args_replace_output_format_and_preserve_add_dir`
- `cargo test`
- `cargo build --release`
- `cargo install --path .`

❯ In src/session-share, `agent-doc tasks/docs.md`, the "Synchronous orchestra" uses orchestrate. The response patchback should stream the full response of each agent. Then the summary. The response of each agent is not included in docs.md. How can we make the orchestrate command stream patchback each task's response?
### Re: orchestrate step patchback streaming — gpt-5

Right now `orchestrate` does **not** have a streaming patchback path for sequential steps. In [`src/agent-doc/src/orchestrate.rs:460`](/home/brian/work/btakita/agent-loop/src/agent-doc/src/orchestrate.rs:460), each step does:

1. inject prompt
2. run `preflight`
3. call blocking `send_fresh`
4. call `finalize`
5. call `session-check`

That means the document only gets one binary-owned write after the child agent is already finished. There is no checkpoint loop that can stream the child response into `exchange` while the step is running.

The good news is the machinery already exists elsewhere:
- [`src/agent-doc/src/agent/streaming.rs:41`](/home/brian/work/btakita/agent-loop/src/agent-doc/src/agent/streaming.rs:41) defines `StreamChunk`
- [`src/agent-doc/src/stream.rs:163`](/home/brian/work/btakita/agent-loop/src/agent-doc/src/stream.rs:163) resolves a streaming backend
- [`src/agent-doc/src/stream.rs:237`](/home/brian/work/btakita/agent-loop/src/agent-doc/src/stream.rs:237) already has the timed flush loop

So the clean fix is:

1. Add a sequential-step streaming path in `orchestrate.rs` instead of always using blocking `send_fresh`.
2. After `inject_prompt` + `preflight`, capture the current `exchange` content as the seed.
3. As chunks arrive, flush **seed exchange + in-progress task response** back into `exchange` with replace semantics.
4. On the final chunk, run the same imperative-response guard, then close the step with the normal `finalize -> session-check` boundary.
5. Keep the orchestration summary as a later parent response, not as a replacement for the step responses.

The important detail is step 3: you cannot reuse `stream.rs` unchanged by flushing only the partial response text, because its current replace-mode writer would overwrite the whole `exchange` component. For orchestrate, the streamed buffer has to be "existing exchange through injected prompt" plus the child response so far.

So the missing behavior is in the binary, not in `tasks/docs.md`. That document only showed the summary because the current orchestrate lifecycle has no per-step streaming write path.

If you want the smallest MVP, I would ship this first as `agent-doc orchestrate --stream-steps` for sequential mode and make it Claude-only initially, since Claude already has `stream-json` support. Once that is stable, make it the default sequential behavior and add Codex parity later.

❯ do #4qja. update spec + tests. build + install for local testing. commit + push
### Re: #4qja streaming orchestrate patchback — gpt-5

Implemented the sequential orchestration streaming path for CRDT docs and updated the spec/tests around it.

What changed:
- `src/orchestrate.rs`: streams step responses into `exchange` for built-in streaming backends, keeps `finalize` as the authoritative closeout, and trims the finalize payload to the unseen suffix so streamed headings are not duplicated.
- `src/agent/mod.rs`: only resolves streaming backends for built-in `claude`/`codex`; skips `junie` and custom non-streaming agents.
- `tests/run_integration.rs`: adds streaming orchestration coverage and separates streaming vs non-streaming mock config helpers.
- `specs/07-commands.md` and `README.md`: document sequential CRDT step streaming.

Verification:
- `cargo fmt --manifest-path Cargo.toml`
- `cargo test`
- `cargo build --release`
- `cargo install --path .`

Commit:
- `6f4e3cf` — `feat: stream orchestrate step patchback`

Push:
- `origin/20260422/codex-agent-backend`

❯ Why was #4qja not completed in pending?
### Re: pending state for #4qja — gpt-5

`#4qja` stayed open because the previous `do #4qja ...` cycle implemented and committed the work, but that closeout did not record the matching `--pending-done 4qja` mutation. So `exchange` shows the completed implementation while `agent:pending` was left in its earlier recommended `[ ]` state.

This was a response-lifecycle bookkeeping miss, not evidence that the implementation was still incomplete. I’m marking `#4qja` done in this cycle so preflight can reap it on the next turn.
<!-- agent:boundary:aeb3cfe9 -->
<!-- /agent:exchange -->

<!--
Plan next steps with the "binary-owned prose grammar". Such as how subagents should orchestrate their subtasks. However, src/claudescore tasks/docs.md did finish #ntoc in <10m, so Claude Code figured out how to execute the tasks quickly. The user may instruct suboptimal instructions. Perhaps we could have a plan mode as well to discuss how to orchestrate? Or maybe we should just rely on the agent harness to orchestrate?
-->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] [#4qja] [recommended] Stream per-step orchestrate patchback into exchange so each task response lands before the orchestration summary
- [ ] [#plnk] When creating a pending task with a dedicated plan file, always include the plan file path in the pending item text. Create the plan file first, then add the pending item referencing it. Update skill/runbook to enforce this ordering.
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Blocked until multi-user workspace support exists; issue #40 explicitly says not to ship this before that prerequisite. Tracked: https://github.com/btakita/agent-doc/issues/40
<!-- /agent:pending -->
