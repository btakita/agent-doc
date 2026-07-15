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
queue: stop
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
### Re: rollback-proof, self-reconnecting agent-doc closeout — gpt-5

> **Queue prompt:** /goal Implement backlog item(s): #aqt3

The queue reference `#aqt3` has no matching live backlog item, so it was stale phantom work and no tsift implementation was executed for it.


- Responses remain console-only while work is in progress, then cross the document, snapshot, commit, and session-check boundary once as one complete buffered body. Boundary canonicalization leaves one terminal `agent:boundary` marker.
- A committed response can no longer be rolled back if an editor or CRDT authority later replays the captured pre-response baseline. With narrow capture/HEAD proof, agent-doc restores the committed HEAD through authority CAS and clears the obsolete deferred lineage.
- Recovery failures during preflight now abort the cycle. They can no longer degrade to a warning followed by a generic commit of stale content.
- Staleness is handled continuously: every existing turn-stage check still requests safe supervisor/controller recycling, and a route that becomes stale during preflight queue convergence now schedules the same idle-boundary recycle before the next attempt.
- Every stale supervisor/controller recycle now also emits a forced editor-replica refresh. JetBrains and VS Code replace cached open-document forwarders after controller replacement or native-library reload, covering both zero-replica timing windows without an IDE restart, repair command, or disk fallback.
- Retrying an interrupted closeout now atomically supersedes the uncommitted response tail after the last unchanged committed response while preserving later operator prompts. It removes stale/partial assistant copies, installs the latest complete response, and canonicalizes the document to one terminal boundary.
- Response materialization is now proved semantically against the projected document, so the expected terminal `(HEAD)` annotation cannot make a successfully materialized response look absent.
- Git closeout now builds the session-document commit in a private index rooted at the observed HEAD, creates the commit with `commit-tree`, and advances HEAD with compare-and-swap. Unrelated staged files are excluded from the commit and remain staged.


The Haiven session also passes `session-check` with exactly one boundary. No `agent-doc repair` command and no forced disk write were used; this historical interrupted cycle is crossing the binary-prescribed transactional `write --commit` path once, and future closeout/recovery stays automatic.

### Re: rollback-proof, self-reconnecting agent-doc closeout — gpt-5

> **Queue prompt:** /goal Implement backlog item(s): #aqt3

The queue reference `#aqt3` has no matching live backlog item, so it was stale phantom work and no tsift implementation was executed for it.

The fragmented-response, stale-supervisor, and idle-thrash failure class is fixed across agent-doc `0.34.117` through `0.34.125`, together with tsift `0.1.77`.

- Responses remain console-only while work is in progress, then cross the document, snapshot, commit, and session-check boundary once as one complete buffered body. Boundary canonicalization leaves one terminal `agent:boundary` marker.
- A committed response can no longer be rolled back if an editor or CRDT authority later replays the captured pre-response baseline. With narrow capture/HEAD proof, agent-doc restores the committed HEAD through authority CAS and clears the obsolete deferred lineage.
- Recovery failures during preflight now abort the cycle. They can no longer degrade to a warning followed by a generic commit of stale content.
- Staleness is handled continuously: every turn-stage check requests recycling, and the supervisor now hot-reloads at the first handler-safe checkpoint in any turn stage. `execve` preserves the harness child and durable cycle checkpoint; it no longer waits indefinitely for a closed cycle or an idle prompt.
- Every stale supervisor/controller recycle now also emits a forced editor-replica refresh. JetBrains and VS Code replace cached open-document forwarders after controller replacement or native-library reload, covering both zero-replica timing windows without an IDE restart, repair command, or disk fallback.
- Retrying an interrupted closeout now atomically supersedes the uncommitted response tail after the last unchanged committed response while preserving later operator prompts. It removes stale/partial assistant copies, installs the latest complete response, and canonicalizes the document to one terminal boundary.
- Once the latest response is already live controller authority, it supersedes every older retained response target. Deferred-write composition can no longer merge the stale target back over it; an exact stale editor cut receives the latest response, later operator prompts are preserved, and reconnect materialization still emits exactly one boundary—including on the first turn before any response has been committed.
- Settled-operator rebases, chained deferred targets, and editor reconnects now all canonicalize boundary control state from the newest target after CRDT composition. A stale editor branch can no longer delete the target boundary or retain its older boundary, while malformed or boundary-free branches remain a no-op instead of minting protocol state.
- A forced JetBrains reconnect now compare-and-swaps the reconciled deferred target into the visible IntelliJ document under a non-operator mutation guard before it registers and swaps the replacement replica. Pending local work or editor drift fails closed, so queue consumption cannot be ACKed by a target-only replica while stale IDEA text later replays an old boundary or queue state.
- Response materialization is now proved semantically against the projected document, so the expected terminal `(HEAD)` annotation cannot make a successfully materialized response look absent.
- Git closeout now builds the session-document commit in a private index rooted at the observed HEAD, creates the commit with `commit-tree`, and advances HEAD with compare-and-swap. Unrelated staged files are excluded from the commit and remain staged.
- Quiescent queue observation is now a lazy derived projection. An editor-attached supervisor compares the compact canonical CRDT state vector plus liveness/convergence state; a detached supervisor compares disk metadata. It materializes, hashes, serializes, and reparses full markdown only when that revision changes, a compact probe fails, or the 60-second safety reconciliation is due.
- The JetBrains 0.2.255 stack frame was a separate synchronization defect, not another lazy-state candidate: delivery-ack and library-reload watchers called `FileDocumentManager.getDocument` without IntelliJ model read access. Plugin 0.2.256 captures only the open file/document references inside a bounded read action, then releases it before asynchronous CRDT re-registration; no polling or durable state was added.
- Controller startup records the project-root incarnation and fails closed if a temporary root is deleted or recreated during spawn. On Unix it retains an open handle to the original root until startup completes, preventing immediate inode reuse from disguising replacement. This prevents detached controllers from resurrecting `/tmp/.tmp*` roots; 70 verified historical orphan controllers were stopped. In-place upgrades also reap only exact, already-exited `agent-doc` child zombies.
- tsift transcript context now resolves an embedded Claude/Codex working directory before status, indexing, diff, or graph work. The reproduced JSONL hang had incorrectly selected `/home/brian` and accumulated 14.5 CPU-hours; the installed `0.1.77` completes the same case in under one second against the bounded `src/boost-client` root.

The complete agent-doc gate passed 7,658 tests with 240 skipped. The idle supervisors sampled at 0.30%, 0.70%, and 0.80% CPU, down from roughly 2–2.25%, and their full-text reads occur once per safety interval instead of twice per second. The shared controller's remaining live load was traced entirely to this still-active tsift turn's pre-upgrade supervisor issuing two full reads per second; it will jump directly to `0.34.125` at this closeout boundary. Commits `4fc70f0e`, `565169d6`, `9a48253f`, `7429e3a7`, `611833e0`, `646e6164`, `2422c184`, `ebdf5868`, `9bb17a52`, and `8dc38335` are pushed, tags `v0.34.117` through `v0.34.125` are published, and the installed CLI, native library, and skill report `0.34.125`. The exact incarnation-reuse regression and the full gate pass locally; the latest agent-doc CI run is pending. tsift commit `9db6e80` and tag `v0.1.77` are pushed and installed with green CI. IntelliJ 2026.1 loaded JetBrains plugin `0.2.256`, then exercised delivery-ack re-registration for Haiven and tsift without another read-access assertion; its banner refresher remains event-driven/coalesced.

The Haiven session also passes `session-check` with exactly one boundary. No `agent-doc repair` command and no forced disk write were used; this historical interrupted cycle is crossing the binary-prescribed transactional `write --commit` path once, and future closeout/recovery stays automatic.
<!-- agent:boundary:f29fac18 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
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
