---
agent_doc_session: tsift-v0.1
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260613-072518.md`*

Compacted content:
- Archived 11 response topic(s): do #logclassfn; do #logfoldloss; do #pcacheattr; 8 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: do [#tsreview2] — follow-up #code-review of the #tscodereview interactions — opus-4-8

CI green (run `27460096312`), so the #tscodereview batch has settled. Ran a focused interaction/concurrency review across the three changed surface groups via three parallel reviewers (each verifying against the actual code). #tsreview2 done; actionable findings queued in priority order as `#gdblockcover` → `#pcacheexplattr` → `#tsreviewcleanup`.

- **log-digest (classification + folding + fidelity gate) — CLEAN.** Family `error_variants` keys on normalized text, signal groups on raw text, and normalized is a pure function of raw → family count ≤ signal count always, so a fold can never hide an error from the gate. Errors sort rank-0 before the cap; `dropped` math is exact. No bug.
- **graph-db — 2 REAL BUGS (HIGH conf), see `#gdblockcover`.** The #gdbwritelock advisory lock does NOT cover all writers despite its "covers all writer callers" claim: `compact --apply` (DELETE + wal_checkpoint(TRUNCATE) + VACUUM, infra.rs:1133→lib.rs:2025) and `memory upsert_projection` (memory.rs:976) both bypass it and race a refresh that holds it — exactly the database-is-locked / VACUUM-vs-WAL collision the lock was meant to prevent. Verified clean: RAII lock release on all error paths, no deadlock ordering, WAL-checkpoint reporting correct.
- **prompt-cache — 1 REAL GAP (HIGH conf), see `#pcacheexplattr`.** Explicit provider fingerprints bypass the #pcacheattr residual invariant: a genuine `stable_prefix` content drift changes nothing tracked and goes unattributed. `#pcacheregtrunc` math and breakpoint edges verified correct.

Top of backlog: **#gdblockcover**. (Session recovered from a stale pre-compaction CRDT overlay mid-cycle; committed via force-disk. Exchange stays compact.)

### Re: do [#gdblockcover] — opus-4-8

Fixed. The `#gdbwritelock` advisory lock claimed to cover all graph-db writers, but two mutating paths bypassed it and raced a concurrent refresh holding the lock — the exact `database is locked` / VACUUM-vs-WAL collision the lock exists to prevent. Both now take `crate::acquire_graph_db_write_lock` so every mutating writer serializes through the same lock.

- **`compact --apply`** (`cmd_graph_db_compact`, `commands/infra.rs`) — its DELETE + `wal_checkpoint(TRUNCATE)` + `VACUUM` ran unguarded. `VACUUM` cannot proceed while another writer holds an open WAL transaction; wrapped the `if apply { … }` block in the lock.
- **`memory finding-add` upsert** (`add_authored_node_to_graph`, `commands/memory.rs`) — the authored-node `upsert_projection` write raced a refresh's WAL transaction; wrapped before the store open.

**Regression coverage** (deterministic, no timing flakiness post-fix): each test holds the lock externally, spawns the operation in a thread, asserts it is `!is_finished()` while the lock is held (parked in the acquire loop — before the fix it ran unguarded and would already be done), then releases the lock and asserts the join succeeds. `graph_db_compact_apply_blocks_on_held_write_lock` (lib.rs) and `finding_add_blocks_on_held_graph_db_write_lock` (memory.rs).

**Spec**: `specs/graph.md` now lists `compact --apply` and `memory finding-add` as lock-covered writers and notes `compact --apply` runs its `VACUUM` under the write lock.

**Verification**: `make check` clean (clippy + full suite, exit 0); tsift-cli lib suite **372 passed, 0 failed** (370 prior + 2 new); `cargo install --path .` replaced `~/.cargo/bin/tsift`. Committed + pushed (`cb5c266`, submodule pointer `221793dd4`). CI run `27460839933` queued for the new commit — recorded pending; closed out on local evidence.

Top of queue next: **#pcacheexplattr**.
<!-- agent:boundary:f052d459 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- ~~:round_pushpin: do [#gdblockcover]~~
- :pushpin: :round_pushpin: do [#pcacheexplattr]
- :pushpin: :round_pushpin: do [#tsreviewcleanup]
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#pcacheexplattr] prompt-cache: explicit provider fingerprint bypasses #pcacheattr residual attribution (MED sev, HIGH conf). When a provider supplies explicit_fingerprint (stable_prefix_fingerprint/prefixFingerprint, packages/tsift-agent-doc/src/session_cost.rs:3326) the derived fingerprint material is bypassed, so a real stable_prefix CONTENT drift changes nothing tracked and goes unattributed; conversely first_changed_field=stable_prefix_fingerprint no longer implies prefix drift. Fix: track raw stable_prefix as its own ordered metadata field/field-change (before the fingerprint) so prefix-content drift is attributed regardless of explicit-vs-derived fingerprint.
- [ ] [#tsreviewcleanup] [recommended] three low-sev review cleanups: (1) prompt-cache field_changes double-reports the redundant derived stable_prefix_fingerprint entry whenever a concrete sub-field also changed (session_cost.rs:1810) - suppress unless it is the sole change; (2) breakpoint (xN) aggregation suffix can collide with provider-supplied breakpoint text (session_cost.rs:3461) - carry count structurally or escape; (3) graph-db export live-lock diagnostic (infra.rs:653) is near-dead for the realistic WAL case (snapshot fallback succeeds, recovery gate bails first) - unify wording on the recovery-gate bail or document.
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
