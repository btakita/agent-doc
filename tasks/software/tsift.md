---
agent_doc_session: tsift-v0.1
resume: 019dd526-7958-77e3-9a41-92b8944fc550
agent: codex
claude_model: claude-opus-4-6
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
---

## Status

<!-- agent:status patch=replace -->
Project created. Repo: https://github.com/btakita/tsift. Submodule: src/tsift. Spec extracted to src/tsift/SPEC.md.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260513-033937.md`*

Compacted content:
- Archived 4 response topic(s): #s1k1 search workflow; #v6y3 search scale guards; tsift log survey; 1 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ do [#sprm]. spec-test-build-install-commit-push
### Re: #sprm — gpt-5

Done. Implemented `#sprm` in `src/tsift` and pushed the tsift submodule commit.

`session-review --next-context` now derives active prompt targets for agent-doc template documents from the live `agent:exchange` tail after the latest response boundary, instead of treating historical transcript aggregates as current work. The prompt-target filters also ignore frontmatter prompt presets, copied slash-command examples, and completed backlog archive rows, so prompt presets, compacted/resolved prompts, and repeated old `do ...` directives do not reappear as active next-context work.

I documented the contract in `SPEC.md` and added regressions for markdown/session-digest filtering, copied Codex frontmatter prompt presets, and session-review preferring the live exchange prompt over repeated historical transcript prompts.

Verification:
- `cargo test` passed: 562 unit tests and 92 integration tests.
- `make check` passed: clippy with `-D warnings`, then the full test target.
- `cargo build --release` passed.
- `cargo install --path . --force` installed `tsift 0.1.42` locally.
- `tsift session-review --next-context --json /home/brian/work/btakita/agent-loop/tasks/software/tsift.md` now reports only `do [#sprm]. spec-test-build-install-commit-push` in `active_prompt_targets`.
- GitHub CI `25777027413` passed (`clippy + test`). There is no tmux CI leg in the tsift workflow.

Commits:
- tsift: `df3133e` (`fix: keep next-context prompts live`), pushed to `origin/main`.
- agent-loop: `103eb91bd` (`chore: update tsift submodule`) committed locally; parent push follows this binary-owned document closeout so the session document stays out of the manual repo commit.
❯ agent-doc has SimWorld. Should SimWorld be extracted so tsift can use it to speed up test suites? What library would be a good candidate? tmux-router? Does tsift use tmux-router? Should the shared SimWorld be a standalone library?
### Re: SimWorld extraction — gpt-5

Short answer: do not extract `agent-doc`'s `SimWorld` wholesale for `tsift` yet. The useful part is the pattern, not the current domain model.

What I found:
- `agent-doc`'s `SimWorld` is strongly tied to agent-doc closeout state: template documents, response capture/write/commit phases, backlog guards, route actor lifecycle, and sync-layout ownership projections.
- `tsift` does not use `tmux-router`. `src/tsift/Cargo.toml` has no `tmux-router` dependency, and `cargo tree -p tsift --invert tmux-router` reports no matching package. The `tmux-router` entry in `src/tsift/Cargo.lock` is only an unused workspace patch marker.
- `agent-doc` does use `tmux-router`, and its SimWorld has sync-layout cases because agent-doc owns tmux pane/session behavior. That does not map cleanly to tsift's main risk surface.

For `tsift`, the best candidate for SimWorld-style acceleration is a local deterministic test harness around pure workflow state, especially:
- session transcript classification: `session-digest`, `session-cost`, `session-review`, `context-pack`;
- workspace/index status state: missing/stale/fresh scoped indexes, lock states, summary cache states;
- rewrite/digest-runner routing decisions.

That would let most matrix coverage run in memory with fixed seeds and compact traces, while keeping a smaller number of CLI integration tests for real subprocess, filesystem, sqlite, git, and timeout behavior. It should reduce suite cost more directly than bringing in tmux-router.

If we extract anything, the right shape is a tiny standalone dev/test crate that provides only generic simulation machinery: deterministic RNG, command schedule runner, seed corpus budgets, trace formatting, and coverage counters. The domain worlds should stay in each consumer (`agent-doc` closeout world, `tsift` index/session/rewrite world, maybe later `tmux-router` layout world). I would only create that standalone crate after a `tsift`-local prototype proves the same runner API is useful in both projects.

So the sequence I would use is:
1. Build a `tsift`-local `sim_world` for the session/rewrite/status state machines.
2. Move expensive CLI matrix tests behind a smaller smoke set only after the in-memory model covers the same decision invariants.
3. Extract a shared crate only for the generic runner once both `agent-doc` and `tsift` have near-identical harness code.

`tmux-router` is a good separate candidate for its own SimWorld-style layout projection tests, but it is not the right shared library for tsift.
<!-- agent:boundary:8206d8fe -->
<!-- /agent:exchange -->
###
<!--
-->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#sops] Teach `tsift log-digest` to parse structured agent-doc `file=...`, event, pane, and session fields as anchors; the 99,637-line ops log currently reports `file_ref_groups=0` and `no file anchors detected` despite path-bearing records.
- [ ] [#snav] Add a status/release regression proving `tsift status --fix` refreshes stale Code Navigation instructions after a binary version bump; current local status reports instructions at 0.1.41 while the installed binary is 0.1.42.
<!-- /agent:backlog -->

<!-- agent:icebox -->
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
<!-- /agent:icebox -->

## Completed / Reaped
k
<!-- agent:done -->
- 2026-05-12 [#686r] Add a DCI-style tsift benchmark: compare exact/chained rg, lexical BM25, and hybrid search on multi-hop code/repo tasks, tracking localization, tool calls, latency, and token budget.
- 2026-05-12 [#s1k1] Add a composable search workflow surface or documented recipes that preserve result handles across search --exact, search, explain, summarize, and digest expansion steps.
- 2026-05-12 [#v6y3] Add breadth/scale guards to search envelopes: high-hit warnings, corpus-size/tool-budget signals, and concrete next-narrowing commands before agents fan out too broadly.
- 2026-05-12 [#sflt] Fix session-review/session-digest failure classification so successful test summaries and generic instruction text like `failures:` are not reported as unresolved failures; preserve real failing assertions/exits with command and session anchors.
- 2026-05-12 [#sprm] Tighten `tsift session-review --next-context` prompt-target extraction so frontmatter prompt presets, examples, archived/resolved prompts, and repeated historical directives do not appear as active prompt targets.
<!-- /agent:done -->

## Completed / Reaped

<!-- agent:pending-done -->
- 2026-05-09 [#ctxp] Add context-pack to the token-savings fixture/regression, including diff/test/log/next-context inputs and a fail-under threshold for its envelope preview.
- 2026-05-10 [#real] Add a real-session token-savings benchmark from recent tsift/agent-doc transcripts, covering session-review and context-pack paths that dominate actual prompt volume.
- 2026-05-10 [#budg] Tune the response-budget presets and auto caps against measured token-savings data so envelope previews stay compact without hiding required follow-up commands.
<!-- /agent:pending-done -->

<!--
-->
