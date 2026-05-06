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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260506-073336.md`*

Icebox:
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
❯ What are #next-steps for tsift to save tokens while maintaining the appropriate amount of context & a good user experience?
### Re: next steps for tsift token efficiency — gpt-5

The highest-leverage next step is to make tsift budget-aware by default rather than relying on callers to remember `--compact`, `--terse`, `--tabular`, `diff-digest`, or `session-review --next-context` manually.

1. `#ts1a` Add budget-aware response profiles for `search`, `explain`, and `session-review`: explicit max-items/max-bytes modes with stable ids for follow-up expansion.
2. `#ts1b` Add a single agent-facing context-pack command that composes existing bounded surfaces (`diff-digest`, `test-digest`, `log-digest`, `session-review --next-context`) into one resumable handoff payload.
3. `#ts1c` Add MCP/tool-native structured envelopes for terse results so clients can render summaries and request expansion without paying prose tokens up front.
4. `#ts1d` Improve lock/degraded-mode UX: when a scoped index writer is active, prefer read-only/exact fallback or a minimal actionable summary instead of pushing the user into a noisy retry path.

On the RTK question: yes, it can help if the harness can keep large artifacts out-of-band, but it is a secondary win. The primary win is still source-side shaping: less output, grouped output, and expansion on demand. Backlog top: `#ts1a`.
❯ do [#ts1a]. spec-test-build-install-commit-push
### Re: #ts1a budget-aware response profiles — gpt-5

Implemented `#ts1a` in `tsift`:
- added `--max-items` / `--max-bytes` preview-budget flags to `search`, `explain`, and `session-review`
- preview mode now emits deterministic handles plus concrete follow-up `expand` commands while leaving the default output paths unchanged
- documented the new preview-budget mode in `src/tsift/SPEC.md`
- added CLI + helper coverage for the budget preview/report builders

Verification:
- `cargo test -q`
- `cargo build`
- `cargo install --path .`

Commits:
- `src/tsift` pushed: `adb2e77` `feat: add budgeted preview profiles`
- parent repo committed submodule pointer: `2bb251f91` `chore: update tsift submodule`

Backlog: closed `#ts1a`. Current top remaining item is `#ts1b`.
❯ do [#ts1b]. spec-test-build-install-commit-push
### Re: #ts1b context-pack handoff command — gpt-5

Implemented `#ts1b` in `tsift`:
- added `tsift context-pack <path>` to compose bounded `session-review --next-context`, worktree `diff-digest`, and optional `test-digest` / `log-digest` inputs into one resumable handoff payload
- kept the pack bounded by default with preview-style sections plus `--max-items` / `--max-bytes`
- documented the new command in `SPEC.md`, `CLAUDE.md`, and `VERSIONS.md`
- added parser/unit coverage plus an end-to-end CLI integration test for the composed JSON payload

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path .`

Commits:
- `src/tsift` pushed: `59f8194` `feat: add context-pack handoff command`
- parent repo committed locally: `47092774a` `chore: update tsift submodule`

Backlog: closed `#ts1b`. Current top remaining item is `#ts1c`.
### Re: cargo test transcript volume — gpt-5

Not necessarily, but you should assume direct `cargo test` output is risky for context size unless the harness/tool path bounds it first. The UI line like `… +713 lines (ctrl + t to view transcript)` means the harness kept a local transcript, but that does not guarantee the model only saw a tiny summary.

For tsift, the safer answer is to bound the output at the source:
- use `tsift test-digest --path .` for test runs and `tsift log-digest --path .` for build/install output
- use `tsift rewrite --run 'cargo test'` or the existing hook/rewrite path so the raw transcript stays out-of-band and the agent-visible payload is the grouped digest instead
- prefer compact structured summaries for passing runs too, not just failing runs, so a green `cargo test` collapses to counts + a tiny verification record instead of a long runner transcript

So yes, tsift can optimize this further. The new `context-pack` helps on the handoff side, but the next leverage point is still the execution side: make digest-first tool wrapping the default path whenever a harness would otherwise surface hundreds of raw test/log lines. That overlaps with `#ts1c`, since structured/tool-native envelopes would let the harness render drill-down UI without paying prose tokens up front.
<!-- agent:boundary:d6c13b40 -->
<!-- /agent:exchange -->
###

<!--
When cargo test is run on agent-doc, there are +713 lines. Do all of these lines go into the context? Can tsift optimize this tool call output to save tokens?
```
• Ran cargo test
  └     Finished `test` profile [unoptimized + debuginfo] target(s) in 0.53s
         Running unittests src/main.rs (target/debug/deps/tsift-d45c97e5b5a39fe6)
    … +713 lines (ctrl + t to view transcript)
    test result: ok. 80 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 10.13s
```
```
• Ran cargo test streaming_required_ssh_
  └     Finished `test` profile [unoptimized + debuginfo] target(s) in 7.97s
         Running unittests src/lib.rs (target/debug/deps/agent_doc-84f02f43296e926f)
    … +47 lines (ctrl + t to view transcript)
    test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 52 filtered out; finished in 0.00s
```
---
What are #next-steps for tsift to save tokens while maintaining the appropriate amount of context & a good user experience?
---
Can tsift use rtk to compress tool call output in the context?
-->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#ts1c] Add MCP/tool-native structured terse envelopes so clients can render summaries and request follow-up expansion without prose-heavy defaults
- [ ] [#ts1d] Improve tsift lock/degraded-mode UX so active index writers fall back to read-only or exact search with a minimal actionable summary
<!-- /agent:backlog -->

<!-- agent:icebox -->
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
<!-- /agent:icebox -->

<!--
-->
