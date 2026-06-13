---
agent_doc_session: tsift-v0.1
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
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

### Re: do [#pcacheexplattr] — opus-4-8

> **Queue prompt:**
>
> :pushpin: :round_pushpin: do [#pcacheexplattr]


Fixed. The prompt-cache prefix-drift attribution had a hole: when a provider supplies an **explicit** `stable_prefix_fingerprint` (`stablePrefixFingerprint`/`prefix_fingerprint`/`prefixFingerprint`, `session_cost.rs:3317`), the derived fingerprint material — which folds in the raw `stable_prefix` — is bypassed (`explicit_fingerprint.unwrap_or_else(...)`, `:3346`). So a genuine stable-prefix **content** drift changed nothing tracked and went unattributed; conversely `first_changed_field=stable_prefix_fingerprint` no longer implied a prefix change (the provider could rotate the fingerprint on its own).

Root fix: track the raw `stable_prefix` as its own ordered metadata field, independent of the fingerprint.

- **Metadata** — added `SessionCostPromptCacheMetadata.stable_prefix: Option<String>` (`:69`), populated from the same `stable_prefix`/`stablePrefix`/`cached_prefix`/`promptPrefix` extraction already used to build the derived fingerprint, so it is captured whether or not the fingerprint is explicit.
- **Attribution order** — `prompt_cache_field_changes` now pushes `stable_prefix` **immediately before** `stable_prefix_fingerprint` (after cache_key/breakpoints/routing_affinity/provider). Because the raw prefix precedes the composite, a real prefix-content drift is named `stable_prefix` (the concrete cause) regardless of explicit-vs-derived fingerprint. The fingerprint is the first changed field only when no tracked sub-field — including the raw prefix — moved, e.g. a provider rotating an explicit fingerprint independently of content.

**Tests** (JSON-driven through `compute`, no struct literals to touch):
- Renamed the derived-path test → `prompt_cache_drift_attributes_pure_prefix_content_change_to_stable_prefix`: now asserts `first_changed_field == "stable_prefix"` and two ordered changes (`stable_prefix`, then the derived `stable_prefix_fingerprint`) — the more-specific attribution.
- Added `prompt_cache_drift_attributes_prefix_change_under_explicit_fingerprint`: explicit `stable_prefix_fingerprint` held **constant** across turns while the prefix content drifts — proves the drift is now attributed to `stable_prefix` alone (the only reported change), which was invisible before this fix.

**Spec**: `specs/digests-sessions.md` §4 now lists raw stable-prefix in the attribution metadata, documents the ordering (concrete sub-fields, then raw prefix, then composite fingerprint), and spells out the explicit-fingerprint bypass rationale.

**Verification**: `make check` clean (clippy + full suite, exit 0); 14 `prompt_cache` tests pass including both above. `cargo install --path .` replaced `~/.cargo/bin/tsift`. Committed + pushed (tsift `b58eff6`, superproject pointer `1ee506ff0`). CI run `27461255690` queued for the new commit — recorded pending; closed out on local evidence.

(Registered the new `#advance-review` preset from frontmatter — no action needed this cycle.) Top of queue next: **#tsreviewcleanup**.

### Re: do [#tsreviewcleanup] — opus-4-8

> **Queue prompt:**
>
> :pushpin: :round_pushpin: do [#tsreviewcleanup]


All three low-sev review cleanups done.

**1. Suppress the redundant derived `stable_prefix_fingerprint` field-change** (`session_cost.rs`). A *derived* fingerprint is a pure function of the tracked sub-fields (provider, cache_key, stable_prefix, breakpoints), so whenever it flips one of those already-reported sub-fields changed too — the fingerprint entry was a redundant echo (and after #pcacheexplattr, doubly so alongside the new `stable_prefix` field). Added `stable_prefix_fingerprint_explicit` (serde-skipped internal flag) set when the provider supplies the fingerprint; `prompt_cache_field_changes` now reports the fingerprint entry **only on the explicit path** (independent signal). On the derived path it's dropped, so `first_changed_field` names the concrete cause and the list stays clean.

**2. Escape literal `(xN)` breakpoint text** (`session_cost.rs`). A provider breakpoint whose text already ends in a `(xN)`-shaped suffix is escaped to `(\xN)` before aggregation, so a single literal `foo (x2)` can no longer serialize identically to two plain `foo` breakpoints (which aggregate to `foo (x2)`). That collision had hidden real cache-boundary drift behind a false match. The genuine count suffix never contains a backslash, so escaped literals are unambiguous and never double-escaped.

**3. Unify the graph-db export recovery-gate wording** (`commands/infra.rs`). The resilient read-only open used by snapshot-export succeeds via a snapshot/recovery fallback in the realistic WAL-locked case rather than erroring, so the open's `map_live_lock` mapping is near-dead there and the live-lock signal actually surfaces at the recovery gate — which previously said the vaguer "refused recovered read path … resolve the live database lock". Both export gates now route through one `graph_db_snapshot_recovery_live_lock_error` helper that gives the same actionable guidance as `map_live_lock`: "a concurrent graph-db refresh or snapshot-import is in progress (database is locked) … wait for it to finish before retrying the export".

**Tests**: derived-path + cache_key drift tests now assert the concrete cause only (fingerprint echo suppressed); the prompt-cache effectiveness fixture and its inline twin require `stable_prefix` instead of the derived fingerprint for prefix-content drift; new `prompt_cache_breakpoint_literal_count_suffix_does_not_collide_with_aggregation`; the WAL-lock recovery test asserts the unified wording.

**Spec**: `specs/digests-sessions.md` documents the derived-echo suppression, the explicit-only fingerprint reporting, and the breakpoint count-escape rule.

**Verification**: `make check` clean (clippy + full suite, exit 0); the four touched/new prompt-cache + graph-db tests pass. `cargo install --path .` replaced `~/.cargo/bin/tsift`. Committed + pushed (tsift `44e8e04`, superproject pointer `f5725f374`). The prior **#pcacheexplattr** CI run `27460096312`→`27461255690` is now **green**; this commit's run `27461628541` is queued — recorded pending, closed out on local evidence.

Queue drained — `#gdblockcover`, `#pcacheexplattr`, `#tsreviewcleanup` all done. Backlog is empty; the #tsreview2 follow-up batch is fully landed. (Exchange is large — supervisor owns the recycle between drained items.)
<!-- agent:boundary:222fc734 -->
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
