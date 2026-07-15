---
agent_doc_session: tsift-v0.1
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#bugs-observed': Please add any bugs from observed behavior to the backlog.,
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: start
---

## Status

<!-- agent:status patch=replace -->

complete
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260713-161601.md`*

Compacted content:
- Archived 5 response topic(s): session compaction; release + publish; #015t; 2 more
- Prior summary/context: ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54...

Implement the following to support:
```
agent-doc command failed (exit 1): [agent-doc] Tagged pre-compact state as agent-doc/tsift/pre-compact-183 Error: compact: refused editor convergence for tasks/software/tsift.md because live editor buffer unknown lacks required capability operator_text_authority_v1
```

#spec-test-build-install-commit-push
### Re: Compact Exchange stale-sidecar convergence — gpt-5

Implemented and released the fix.

The failure was caused by quarantined `{stem}.stale-*` live-buffer files being re-enumerated as current editor snapshots. The newest quarantined file was providerless, so Compact Exchange entered the capability gate as `editor_id=unknown` and refused convergence even though the active JetBrains sidecar advertised `operator_text_authority_v1`.

The debounce enumerator now excludes `.stale-*` quarantine files. Active legacy/providerless and named editor snapshots retain their existing fail-closed behavior, so unsaved editor-buffer protection is unchanged. The realtime workflow spec, debounce unit coverage, and shared compact/write convergence fixture cover the distinction.

Verification completed:

- `cargo test -p agent-doc-debounce -p agent-doc-write-converge-io -p agent-doc-compact-io`
- `make check`
- clean detached-worktree `make install-full`
- CI run `29267280455` for commit `7fc71c19` completed successfully

Release completed as agent-doc `0.34.89`: commit `7fc71c19` was pushed to `main`, tag `v0.34.89` was pushed, and the GitHub Release completed. The multi-platform trusted-publishing PyPI workflow is still in progress and is non-blocking. A concurrent `0.34.90` release landed as a descendant of `7fc71c19`; it includes this fix and is now the installed local binary. Crates.io accepted the changed `agent-doc-debounce` 0.34.89 package before the newer descendant superseded the remaining older graph publication.

No additional backlog follow-up is needed because the regression is specified, covered, locally installed, pushed, and present in the current successor release.

### Re: agent-doc response integrity and stale-supervisor recovery — gpt-5

Fixed, recovered, installed, and released.

The response path is now final-only. Streaming partials stay in recovery sidecars and can never become visible session-document responses; the final write, structural validation, snapshot, commit, and terminal session-check form one transaction. Mandatory integrity checks reject fragmented component trees, duplicate or inline boundaries, and malformed response structure before commit. Normal closeout no longer depends on repair.

The stale-state causes were fixed across the complete lifecycle:

- Repair now uses exact realtime-authority CAS rather than merging a stale disk buffer.
- Zero-replica repair can prove and project its retained target without leaving authority and disk split.
- Multi-stage repair clears all superseded deferred intents, so an intermediate projection cannot replay after commit.
- `session-check` proves exact authority/disk equality, settles transient-only committed-tail drift from `HEAD`, and fails closed on real content divergence.
- A committed response retained while the editor has zero registered replicas now resumes automatically through the ordinary CRDT delivery/ACK barrier after replica reattach. It materializes disk only when `HEAD`, canonical target, retained base, hashes, and bytes all match; newer editor text invalidates resumption instead of being overwritten.
- Stale supervisors are detected and scheduled for safe-boundary recycle at every turn stage. The installed v0.34.115 supervisor recycled automatically during this recovery; a second session-check no longer reported stale binary state.

Haiven is clean without a repair command. Disk, realtime authority, and Git `HEAD` all hash to `ff1a841117dda823c8788299673f0fb7eb058d387727a0f415dcb06cb3495e6b`; the session document is clean, contains exactly one `agent:boundary`, and contains none of the earlier fragment signatures. Its response was already committed, and the previously uncommitted WebSocket design document is now committed as `6d3e6706c` (`docs: clarify WebSocket hub state ownership`). The user’s separately staged `haiven-websocket-hub-takehome-orig.md` remains staged and untouched.

agent-doc releases `0.34.110` through `0.34.115` contain the complete fix sequence. The final implementation commit is `0eb322ac`, installed locally as v0.34.115 and published at https://github.com/btakita/agent-doc/releases/tag/v0.34.115. Full verification passed: 7,639 tests passed, 240 skipped, with `make check` exit 0. The newest CI run was in progress and the Release/PyPI jobs were queued when checked; local verification and release creation succeeded.
<!-- agent:boundary:ad38e84d -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
/goal Implement backlog item(s): #aqt3
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#g5kw] What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
- [ ] [#4z0w] Can we increase max-hop caps?
- [ ] [#cg52] Could FalkorDB help with performance?
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->

<!-- agent:notes -->
• Ran tsift rewrite --run 'git log -5 --oneline --decorate'
└ tsift rewrite: no rewrite: no supported tsift rewrite matched this command; `--run` executes only rewritten commands; run the original command directly if intended

#spec-test-build-install-commit-push
---
route duplicate prompt residue guard failed: failed to parse components: unclosed component: `agent:notes` without matching close
<!-- /agent:notes -->
