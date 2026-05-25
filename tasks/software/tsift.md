---
agent_doc_session: tsift-v0.1
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
Completed: #gdbrefreshdelta was already closed; #gdbcmcache, #gdbpathpolicy, and #gdbengineproof are implemented in tsift ab9f8a9 and agent-loop e09157f61. Local verification passed (`make check`, `cargo build`, `cargo install --path .`); latest tsift CI run 26372477579 is green.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260525-082913.md`*

Compacted content:
- Archived 6 response topic(s): do [#cachelookupshift]; do [#gdbvacconvex]; queue updated; 3 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
complete [#gdbvacproof]. #spec-test-build-install-commit-push
### Re: complete #gdbvacproof — gpt-5

Closed `#gdbvacproof` with live Convex reconciliation, guarded tombstone pruning, and post-prune cache verification.

Changed `src/tsift` to make Convex `snapshotMeta` cheap at large scale: it now returns required index metadata plus a projection-hash lookup by `projectionMetaId`, and `convex-sync --remote-snapshot` uses that hash to skip full row-page snapshot diff when the remote projection already matches the local graph. It still falls back to paginated row diff when the hash is missing or mismatched. I also fixed `graph-db compact --apply --prune-tombstones` so cached operator stats are recomputed after VACUUM and tombstone deletion instead of reporting stale tombstone counts.

Operational proof captured in `src/tsift/plans/gdbvacproof-evidence.md`: the final live apply reconciled 358,439 nodes and 608,739 edges in 19,344 successful chunks/receipts. The destructive prune gate compared local and remote projection hashes and both were `fb0439ab06c8d08f615ade87b374adf118587943ad29d970650c0e2a0f982257` before pruning. The guarded prune removed 1,240,258 tombstones, reduced the SQLite file from 1,875,492,864 bytes to 1,016,492,032 bytes, reduced freelist pages from 112,683 to 0, and left cached status reporting 0 tombstones with compaction `not_needed`. The scan-cost sample moved from 12 ms pre-prune to 10 ms post-prune.

Verification completed: `git diff --check`, `cargo test convex_sync_remote_snapshot_uses_`, `make check`, `cargo build`, `cargo install --path .`, and `tsift --version` reporting `tsift 0.1.62`. Latest GitHub Actions `CI` run `26411462514` was `completed success`, with both `clippy + test` and `live Convex graph acceptance` successful. I also checked workflows/jobs and found no tmux-specific CI leg for this repo.

Commits: `src/tsift` is `04cfb35a952d6252bd958091f4dec90ec6fba86c` (`fix: close graph db vacuum proof`) and has been pushed to `github.com:btakita/tsift.git` `main`. The outer repo submodule-pointer commit is `120c1c381f3b705a66b0d0517dd4efd724b4af17` (`chore: update tsift vacuum proof`); the outer `main` push follows immediately after this required agent-doc write/commit boundary so the session-document commit is included in the pushed history.
<!-- agent:boundary:f6c8e1a2 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- ~do [#cachelookupshift]~
- ~do [#gdbvacconvex]~
- ~do [#convexsnapshotscale]~
- ~do [#convexscopedeval]~
- ~complete [#gdbvacproof]~
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
<!-- /agent:review -->
###
<!--
Can we increase max-hop caps?
Could FalkorDB help with performance?
-->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
