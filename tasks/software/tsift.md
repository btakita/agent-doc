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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260520-223719.md`*

Compacted content:
- Archived 1 response topic(s): graphdb next steps
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
### Re: graph-db operator workflows — gpt-5

Implemented `#gref`, `#guse`, and `#gliv`.

For `#gref`, I added explicit graph-db operator commands: `tsift graph-db --path . refresh --json` materializes `.tsift/graph.db` without a throwaway `traverse` call, and `tsift graph-db --path . status --json` inspects the current projection without refreshing. The reports include projection version, content hash, source watermark, node/edge counts, tombstone counts, and next commands for `doctor`, `drift`, and `convex-sync`.

For `#guse`, I added `tsift graph-db --path . evidence <backlog-id-or-job-handle> --depth 3 --limit 8 --json`. It resolves backlog ids and job packet handles, returns reachable `worker_context` and `source_handle` rows, shortest paths, recommended next commands, and compiled fixture coverage. The graph-db conformance suite now locks backlog-id and job-handle evidence behavior.

For `#gliv`, I promoted the live Convex graph acceptance path into CI. The new job exits as a documented skip until `TSIFT_LIVE_CONVEX_GRAPH_URL` is configured for a dedicated deployment; once present, it runs the ignored live acceptance test as a remote graph-db node/kind/neighborhood/path parity gate. README, SPEC, and operator examples now document the workflow.

Graphify icebox answer: I did not remove those icebox items. They are not completed by this batch; LLM entity extraction, HTML visualization, and embedding similarity are distinct semantic/visual feature ideas. They can be deleted if we are explicitly descoping that Graphify layer, but they should not be reaped as done from these graph-db operator changes.

Verification:
- `cargo test --test graph_db_conformance graph_db_`
- `make ci-full`
- `cargo build`
- `cargo install --path .`
- `tsift --version` -> `tsift 0.1.42`
- Latest completed GitHub `btakita/tsift` CI run `26193348809` passed; the tsift workflow has no tmux CI leg.

Commits prepared:
- `src/tsift`: `cd3463c feat: add graph db operator workflows`
- Superproject gitlink: `6e860392c chore: update tsift graph db workflows`

Push follows after this document closeout, per agent-doc commit sequencing.

<!-- agent:boundary:06909d3e -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- do [#gref]
- do [#guse]
- do [#gliv]
- Can the graphify icebox items be removed?
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#atvp] Unblock crates.io publication for tsift by replacing the upstream git-only sift dependency, publishing a compatible upstream crate, or vendoring a small stable search adapter; rerun cargo publish --dry-run before enabling TSIFT_ENABLE_CRATES_PUBLISH.
- [ ] [#g6kf] Confirm the v0.1.42 GitHub release completes after the macOS x86_64 Actions runner leaves the queue; if it remains stuck, remove or replace that matrix target and retag a fixed release.
<!-- /agent:backlog -->

<!-- agent:icebox -->
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
