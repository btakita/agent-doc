---
agent_doc_session: tsift-v0.1
resume: 019dd526-7958-77e3-9a41-92b8944fc550
agent: codex
claude_model: opus
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
queue_active: false
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260520-234158.md`*

Compacted content:
- Archived 4 response topic(s): graph semantic features; semantic similarity queue catchup; semantic graph extraction queue catchup; 1 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ Can tsift be used to assist planning agents to work on a different parts of the codebase in parallel, so there are no merge conflicts?
### Re: parallel planning with tsift — gpt-5

Yes, but I would frame it as conflict-reduction and conflict-screening, not a guarantee that Git will never produce a merge conflict.

`tsift` already has the right primitives for this: AST symbol extraction, call/import graph edges, per-submodule indexes, `communities`, `impact`, `diff-digest`, `context-pack`, and `graph-db evidence` packets with `worker_context` / `source_handle` rows. That is enough to let a planner turn backlog items or prompt targets into bounded write scopes before dispatching workers.

A practical workflow would be:

1. Refresh the graph/index: `tsift status --fix` or `tsift graph-db --path . refresh --json`.
2. For each proposed task, collect evidence: `tsift graph-db --path . evidence <backlog-id> --json` or `tsift --envelope context-pack <doc-or-task> --budget normal`.
3. Build a conflict matrix from the returned files, symbols, routes, tests, and neighboring graph nodes.
4. Assign workers only when their write scopes are disjoint enough: separate submodules are lowest risk, separate files are usually safe, separate symbols in the same file are medium risk, shared routes/config/tests/core helpers should be serialized or owned by one worker.
5. Put the chosen ownership directly into each worker prompt: owned files/symbols, read-only context, forbidden files, expected tests, and handoff handles to expand instead of rereading the world.
6. Before merge, run `tsift diff-digest --cached .` and `tsift impact .` to catch accidental overlap and shared test fallout.

The important limit is that `tsift` can identify likely overlap from source structure and session/backlog graph evidence, but it cannot prove semantic independence. Two changes can touch different files and still conflict through generated code, public contracts, migrations, config, tests, or formatting churn. So the right planner rule is: use `tsift` to make parallel work explicit and auditable, then fail closed when the graph shows shared ownership or when the final diffs overlap.
❯ Add #next-steps
### Re: next steps — gpt-5

I added one follow-up item for parallel-agent planning: `#pwcm`, a conflict-matrix/report item that uses `graph-db evidence`, `context-pack`, `diff-digest --cached`, and `impact` to assign worker scopes and fail closed on overlap.

Backlog priority is now: `#atvp`, `#g6kf`, `#pwcm`. The top backlog item is `#atvp`.
❯ do [#pwcm]. #spec-test-build-install-commit-push
### Re: #pwcm conflict matrix — gpt-5

Completed `#pwcm` and pushed the change.

Implemented `tsift conflict-matrix --path <session-or-repo> <target...>` as a planner gate for parallel worker dispatch. The report refreshes graph evidence, composes `graph-db evidence` for each backlog/job target, summarizes `context-pack`, consumes `diff-digest --cached` and cached `impact`, ranks candidate worker scopes, flags shared files/symbols/tests/config, and emits fail-closed worker ownership prompt blocks.

Updated docs and coverage:
- `src/tsift/SPEC.md` and `src/tsift/README.md` document the new command and planning contract.
- `src/tsift/tests/graph_db_conformance.rs` covers the compiled CLI path and ownership block output.
- Parser coverage was added for the new command in `src/tsift/src/main.rs`.

Verification passed:
- `cargo test conflict_matrix_cli_composes_planner_evidence_and_worker_ownership --test graph_db_conformance`
- `cargo test cli_parses_conflict_matrix_command`
- `make check`
- `cargo build`
- `cargo install --path . --force`
- `tsift --version` -> `tsift 0.1.42`
- `make ci-full`

CI/tmux review: `btakita/tsift` has no `tmux-ci` / `tmux-test` target or CI leg. Latest GitHub Actions `CI` run `26196265078` was green, including `Full CI suite` and live Convex acceptance.

Pushed commits:
- `btakita/tsift` `038cd07 feat: add planner conflict matrix`
- `btakita/agent-loop` `1ec0b85da chore: update tsift submodule`
### Re: release publishing — gpt-5

Completed `#atvp` and `#g6kf`.

For crates.io readiness, I removed the git-only upstream `sift` dependency and added an in-tree lexical search adapter that preserves the `sift`-shaped response/types used by the CLI. `Cargo.lock` no longer contains the `github.com/rupurt/sift` git source, and a clean `cargo publish --dry-run --locked` passes.

For the stalled release, the old `v0.1.42` run `25886542050` had cancelled after the `build x86_64-apple-darwin` job sat without steps for 24 hours. I removed that Intel macOS matrix target, updated installer/docs to direct macOS x86_64 users to `cargo install tsift`, retagged `v0.1.42` to the fixed commit, and confirmed Release run `26197985565` completed successfully. The GitHub Release is published: https://github.com/btakita/tsift/releases/tag/v0.1.42

Verification passed:

- `cargo check`
- targeted search/conflict-matrix tests
- `make ci-full`
- `cargo build`
- `cargo install --path . --force`
- `tsift --version` -> `tsift 0.1.42`
- `cargo publish --dry-run --locked`
- `btakita/tsift` CI run `26197980692` succeeded
- `btakita/tsift` Release run `26197985565` succeeded

CI/tmux note: `btakita/tsift` has no `tmux-ci` / `tmux-test` target. The parent `btakita/agent-loop` `Agent Doc` workflow run `26197999353` failed before any job steps or logs were created (`log not found` for `check` / `tmux-ci`), so I classified it as an external CI-start failure rather than a code or tmux regression.

Pushed commits:

- `btakita/tsift` `342e804 fix: unblock tsift release publishing`
- `btakita/agent-loop` `e9b474f chore: update tsift submodule`
- `v0.1.42` now points to `342e804`

I left `TSIFT_ENABLE_CRATES_PUBLISH` unset because `gh variable list` and `gh secret list` show no publish variable or `CARGO_REGISTRY_TOKEN` secret. I added a recommended backlog item for that external publishing switch.
<!-- agent:boundary:86f72fc7 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- do [#atvp]
- do [#g6kf]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#802b] [recommended] Add a CARGO_REGISTRY_TOKEN repo secret and set TSIFT_ENABLE_CRATES_PUBLISH=true when ready to publish tsift to crates.io from the release workflow.
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
