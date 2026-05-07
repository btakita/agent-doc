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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260507-141722.md`*

Active backlog:
- [ ] [#tsum] Extend context-pack/digest surfaces to surface cached symbol/file summaries behind stable handles for touched files, failures, and tag_alias families

Icebox:
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
❯ do [#tsum]. spec-test-build-install-commit-push
### Re: #tsum — gpt-5

Completed `#tsum`.

Implemented context-pack summary refs so cached summary snippets now surface behind stable handles on diff-touched files, test failures, log signals, log file refs, and log symbol/tag-alias family rows. Each `summary_refs` item carries a bounded summary preview plus a concrete `tsift summarize --file ...` or `tsift summarize <symbol>` expansion command. Human context-pack output also prints those summary handles when present.

Updated `SPEC.md` for the new context-pack surface and tightened project-root resolution so child temp repos are not captured by ambient system-temp project markers; this fixed the full test suite in the current environment where `/tmp/.tsift` and `/tmp/.git` already existed.

Verification completed:
- `cargo test context_pack`
- `cargo test resolve_project_root_or_canonical_path`
- `make check`
- `cargo build`
- `cargo install --path . --force`

Pushed:
- `btakita/tsift` `main`: `8f26390` (`feat: surface context-pack summary refs`)
- parent `agent-loop` `main`: `9e238c8ab` (`chore: update tsift submodule`)
❯ Please look at the latest tagpath updates. What are the #next-steps to integrate to save tokens?
### Re: tagpath token-saving integration — gpt-5

Latest `tagpath` updates relevant to `tsift`:

- `v0.6.1` is tagged and published; local `src/tagpath` only has the version bump dirty in `Cargo.toml` / `Cargo.lock`.
- New library APIs are available for `family`, `query`, and `ontology` through `src/tagpath/src/lib.rs`.
- The new downstream fixture at `src/tagpath/fixtures/tsift-token-savings.json` defines measurable savings contracts: search `12 raw symbols -> 2 families` with at least `40%` savings, explain `12 -> 2` with at least `35%`, and session-review `18 -> 3` with at least `45%`.
- `tsift` already has partial integration: search preview groups repeated symbol hits by canonical tag family, explain/next-context/context-pack expose `tag_alias`, and context-pack summary refs keep cached summaries behind stable handles.

The highest-leverage next step is to import the `tagpath` fixture into `tsift` as a regression test before doing more feature work. That turns "save tokens" into a failing test contract and prevents later preview changes from drifting back toward raw symbol dumps.

Backlog added in priority order:

1. `#tfxt` — add a `tsift` fixture-backed token-savings regression test using `tagpath`'s `fixtures/tsift-token-savings.json` for search, explain, and session-review previews, then expand the contract to context-pack.
2. `#tfam` — replace `tsift`'s local tag alias/query normalization helpers with `tagpath` v0.6.1 `family` / `query` APIs so every preview surface shares one canonical family model.
3. `#tont` — load tagpath ontology refs in `context-pack` and summary-ref surfaces so stable domain tag docs can be referenced by handle/path instead of repeated prose.
4. `#tbud` — add a CI-friendly token-savings report surface comparing raw vs envelope previews per command.

Top backlog item: `#tfxt`.

Verification while reviewing:

- `cargo test tsift_token_savings_fixture` in `src/tagpath` passed.
❯ do [#tbud]. spec-test-build-install-commit-push
### Re: #tbud — gpt-5

Completed `#tbud`.

Added `tsift token-savings --fixture <path>` as a CI-friendly report surface for preview compression contracts. It reads fixture cases with raw symbol rows, compact tagpath families, and minimum savings thresholds; reports byte deltas, estimated token deltas via `ceil(utf8_bytes / 4)`, savings percentages, and per-case pass/fail; supports `--json`, `--fail-under`, and the global `--envelope` wrapper.

Updated `SPEC.md` with the new command and added unit coverage for CLI parsing plus fixture-threshold reporting. The tsift lockfile now points at local `tagpath` `v0.6.1`, matching the token-savings fixture work.

Verification completed:

- `cargo test token_savings`
- `cargo run -- token-savings --fixture ../tagpath/fixtures/tsift-token-savings.json --fail-under --json`
- `make check`
- `cargo build`
- `cargo install --path . --force`
- installed `tsift token-savings --fixture /home/brian/work/btakita/agent-loop/src/tagpath/fixtures/tsift-token-savings.json --fail-under`

Pushed:

- `btakita/tsift` `main`: `6e67f69` (`feat: add token savings report`)
- parent `agent-loop` `main`: `14c757cb4` (`chore: update tsift submodule`)
<!-- agent:boundary:0ed18b8e -->
<!-- /agent:exchange -->
###

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#tont] [recommended] Load tagpath ontology refs in tsift context-pack and summary-ref surfaces so stable domain tag docs can be referenced by handle/path instead of repeated prose.
- [ ] [#tfam] Replace tsift's local tag alias/query normalization helpers with tagpath v0.6.1 family/query APIs so search, explain, session-review, and context-pack share one canonical family model.
- [ ] [#tfxt] Add a tsift-side fixture-backed token-savings regression test using tagpath's fixtures/tsift-token-savings.json for search, explain, and session-review previews before expanding the contract to context-pack.
<!-- /agent:backlog -->

<!-- agent:icebox -->
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
<!-- /agent:icebox -->

<!--
-->
