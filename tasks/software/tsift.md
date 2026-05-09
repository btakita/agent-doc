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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260507-225624.md`*

Active backlog:
- [ ] [#tfxt] Add a tsift-side fixture-backed token-savings regression test using tagpath's fixtures/tsift-token-savings.json for search, explain, and session-review previews before expanding the contract to context-pack.

Icebox:
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
❯ do [#tfxt]. spec-test-build-install-commit-push
### Re: #tfxt token-savings fixture — gpt-5

Completed `#tfxt`.

- Added a compiled CLI regression in `src/tsift/tests/exit_code.rs` that runs `tsift token-savings --fixture ../tagpath/fixtures/tsift-token-savings.json --fail-under --json` against tagpath's shared fixture.
- The test locks the current fixture-backed preview contract to exactly `search`, `explain`, and `session-review`, with every case passing and a positive estimated token delta.
- Updated `src/tsift/SPEC.md` to document that this fixture-backed gate intentionally excludes `context-pack` until its thresholds are added to the shared fixture.

Verification passed:

- `make check`
- `cargo build --release`
- `cargo install --path .`
- installed `tsift token-savings --fixture ../tagpath/fixtures/tsift-token-savings.json --fail-under --json`

Committed and pushed `src/tsift` `main`: `691fac6 test: cover token savings fixture`.
❯ Plan #next-steps for tsift to save tokens.
### Re: #next-steps token savings plan — gpt-5

Planned the token-saving follow-ups into the backlog in priority order.

Top backlog item: `#ctxp`.

1. `#ctxp` Add `context-pack` to the token-savings fixture/regression, including diff/test/log/next-context inputs and a fail-under threshold for its envelope preview.
2. `#real` Add a real-session token-savings benchmark from recent tsift/agent-doc transcripts, covering `session-review` and `context-pack` paths that dominate actual prompt volume.
3. `#budg` Tune the response-budget presets and auto caps against measured token-savings data so envelope previews stay compact without hiding required follow-up commands.

No repo code changed in this cycle. I checked the current fixture-backed savings gate with `cargo run -- token-savings --fixture ../tagpath/fixtures/tsift-token-savings.json --fail-under --json`; it still passes for `search`, `explain`, and `session-review` with an estimated 697-token delta. The main next gap is `context-pack`, which already exists but is still outside the savings fixture contract.
❯ do [#ctxp]. spec-test-build-install-commit-push
### Re: #ctxp context-pack token savings — gpt-5

Completed `#ctxp`.

- Extended `tsift token-savings` so fixture cases can include `context_pack_inputs` for raw `next_context`, `diff`, `test`, and `log` handoff rows, then compare those raw rows against compact context-pack section handles plus tagpath family rows.
- Updated `src/tsift/SPEC.md` and the compiled `tests/exit_code.rs` regression to lock the shared fixture contract to `search`, `explain`, `session-review`, and `context-pack`.
- The shared `tagpath` fixture now includes one `context-pack` case with next-context, diff, test, log, summary-ref, ontology-ref, and artifact-handle inputs plus a `55%` fail-under threshold.

Verification passed:

- `cargo run -- token-savings --fixture ../tagpath/fixtures/tsift-token-savings.json --fail-under --json`
- `cargo test --test exit_code token_savings_accepts_tagpath_preview_fixture`
- `make check`
- `cargo build --release`
- `cargo install --path .`
- installed `tsift token-savings --fixture ../tagpath/fixtures/tsift-token-savings.json --fail-under --json`

The updated installed gate passes with `4` cases, `1286` estimated tokens saved overall, and the `context-pack` case at `70.4%` savings against its `55%` threshold.

Committed and pushed:

- `src/tagpath` `main`: `1bcb017 test: add context-pack token savings fixture`
- `src/tsift` `main`: `e53fa00 test: include context-pack in token savings`
<!-- agent:boundary:215ed214 -->
<!-- /agent:exchange -->
###

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#real] Add a real-session token-savings benchmark from recent tsift/agent-doc transcripts, covering session-review and context-pack paths that dominate actual prompt volume.
- [ ] [#budg] Tune the response-budget presets and auto caps against measured token-savings data so envelope previews stay compact without hiding required follow-up commands.
<!-- /agent:backlog -->

<!-- agent:icebox -->
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:pending-done -->
- 2026-05-09 [#ctxp] Add context-pack to the token-savings fixture/regression, including diff/test/log/next-context inputs and a fail-under threshold for its envelope preview.
<!-- /agent:pending-done -->

<!--
-->
