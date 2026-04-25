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
<!-- agent:boundary:c845a44f -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] [#d5xb] [recommended] Add `fan-in` / DAG orchestration modes after step-state and recovery semantics are stable.
- [ ] [#gkke] [recommended] Implement `agent-doc orchestrate` with `--mode sequential`, fresh agent context per step, prompt injection into exchange, and per-step `finalize` + `session-check`.
- [ ] [#9pw9] [recommended] Refactor the existing `parallel` command behind the same orchestration engine before adding fan-in / DAG modes.
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Blocked until multi-user workspace support exists; issue #40 explicitly says not to ship this before that prerequisite. Tracked: https://github.com/btakita/agent-doc/issues/40
<!-- /agent:pending -->

<!-- 
Why are all of the commits done by Codex in agent-doc authored by `Test <test@test.com>`?
-->
