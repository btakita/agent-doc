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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260613-054654.md`*

Compacted content:
- Archived 2 response topic(s): do #cicacheguard; do #tscodereview
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: do #logclassfn — opus-4-8

> **Queue prompt:**
>
> [#logclassfn]


Done. Fixed `classify_generic_signal` (packages/tsift-digest/src/log_digest.rs) to catch the canonical terminal-failure lines that carry no `error:`/panic prefix and are often the **only** failure line in a transcript:

- cargo libtest `test result: FAILED`
- test-runner counts — pytest `1 failed, 3 passed`, jest `Tests: 2 failed` (anchored to a **non-zero** `<n> failed` token)
- GNU make `*** [..] Error N` (anchored on the make `*** ` sigil + error marker)
- `Segmentation fault` / `core dumped`
- Rust assertion panics — `assertion failed:` and the `` assertion `left == right` failed `` form

All token-anchored so passing summaries (`test result: ok ... 0 failed`, `0 failed`) and prose (`the build failed to ...`) are **not** misclassified — this preserves the existing session-digest `0 failed`/`test result: ok` non-failure invariant.

- **Spec:** updated `specs/digests-sessions.md` classifier paragraph.
- **Tests:** added `classifies_test_runner_and_build_failure_summaries` (positive) + `failure_summary_classification_is_token_anchored` (false-positive guard), and a `cargo-test-failure-summary` fixture-gate case (required `test result: FAILED` survives; forbidden `30 passed` proves the passing summary isn't flagged). All pass; `make check` green; release+install rebuilt (tsift 0.1.69).
- **Commit + push:** tsift `2c68310` → `origin/main`; superproject pointer `fcec59f8e`. CI run triggered (pending; local verification is the gate and is green).

Next in queue: `#logfoldloss`.

### Re: do #logfoldloss — opus-4-8

> **Queue prompt:**
>
> [#logfoldloss]


Done. Fixed the case where semantically distinct failures templatize identically (e.g. per-test `FAILED tests/test_<n>.py - assert <n> == <n>`) and fold into one line-family shown with only first/last samples — while distinct error signals past `MAX_SIGNALS=12` were dropped — so real failures vanished from the digest body.

Changes in `packages/tsift-digest/src/log_digest.rs`:
- **`error_variants`** on each line-family — counts the folded variants that classified as an error, so the hidden distinct-error count survives past first/last sampling (new JSON field, omitted when zero).
- **Error-bearing families rank first** before the `MAX_LINE_FAMILIES` cap, so a failure family is never truncated in favor of benign progress noise.
- **Error signals rank ahead of warnings** before the `MAX_SIGNALS` cap, so a high-occurrence warning can't crowd a distinct error out of the digest.
- **`digest_signal_text`** names the count (`family x20 (20 distinct errors): ...`), so it survives into the token-savings projection consumers and the gate read.

- **Spec:** updated the line-family paragraph in `specs/digests-sessions.md`.
- **Tests:** `line_family_surfaces_distinct_error_count_when_failures_fold` (20 distinct failures → `error_variants=20`, signal total countable, raw artifact recommended) + `error_signals_are_not_crowded_out_by_high_occurrence_warnings`. All pass; `make check` green; release+install rebuilt.
- **Commit + push:** tsift `2ee2009` → `origin/main`; superproject pointer `9c5245ebe`. CI triggered (pending; local gate green).

Next in queue: `#pcacheattr`.

### Re: do #pcacheattr — opus-4-8

> **Queue prompt:**
>
> [#pcacheattr]


Done. Fixed prompt-cache drift attribution: `first_changed_field` always reported `stable_prefix_fingerprint` because the **derived** composite fingerprint (a hash of provider + cache_key + stable_prefix + breakpoints) was pushed first in `prompt_cache_field_changes` — so any sub-field change flipped it and it always won attribution, never naming the real cause.

Fix in `packages/tsift-agent-doc/src/session_cost.rs`: order the concrete sub-fields (`cache_key`, `breakpoints`, `routing_affinity`, `provider`) **ahead of** the composite fingerprint. Now `first_changed_field` (and the scorecard's `suspected_invalidation_cause`) name the specific changed input; the fingerprint is only reported as first-changed when no tracked sub-field moved — i.e. the stable-prefix **content** itself drifted, which is the correct residual attribution.

- **Tests:** rewrote the weak everything-changes test into a focused single-field case (only `cache_key` varies → attributed to `cache_key`, unchanged fields absent from `field_changes`) plus a new prefix-only case (only prefix content drifts → attributed to `stable_prefix_fingerprint`). Updated the `session_cost_reports_prompt_cache_invalidation_diagnostics` integration test that had the same everything-changes weakness.
- **Spec:** documented the attribution ordering in `specs/digests-sessions.md`.
- **Verify:** `make check` green (clippy `-D warnings`, full suite); release+install rebuilt.
- **Commit + push:** tsift `636e561` → `origin/main`; superproject pointer `d89ba5f12`. CI triggered (pending; local gate green).

Next in queue: `#gdbwritelock`.

### Re: do #gdbwritelock — opus-4-8

> **Queue prompt:**
>
> [#gdbwritelock]


Done. graph-db refresh/write and snapshot-import relied only on SQLite `busy_timeout(5s)` for cross-process coordination, so two failure modes were possible (HIGH-sev TOCTOU corruption + spurious parallel failures). Added a bounded **flock advisory write lock** at `<graph.db-stem>.write.lock`:

- **Single write chokepoint:** acquired at the top of `write_traversal_graph_store_with_options` (lib.rs) — covers every writer caller (refresh and the other write entry points), held across `SqliteGraphStore::open` + `replace_projection_with_version`.
- **Snapshot-import rename window:** acquired before the `exists → sidecar-check → rename → rename` sequence (infra.rs) and held through post-install, so the sidecar check now runs **under the lock** and a concurrent refresh can't create `-wal`/`-shm` sidecars mid-rename and corrupt the imported db.
- **Bounded + fail-closed:** flock auto-releases if the holder dies; the acquire polls up to 15s then bails with a clear `another tsift graph-db writer is active …` diagnostic instead of hanging or corrupting. The same lock path makes refresh and import mutually exclusive.

- **Tests:** `graph_db_write_lock_serializes_concurrent_writers` (held lock blocks a second acquire, re-acquirable after release). Existing graph-db + conformance suites stay green.
- **Spec:** documented the write lock in `specs/graph.md`.
- **Verify:** `make check` green; release+install rebuilt.
- **Commit + push:** tsift `d755b45` → `origin/main`; superproject pointer `6f0c3feb0`. CI triggered (pending; local gate green).

Next in queue: `#pcacheregtrunc`.

### Re: do #pcacheregtrunc — opus-4-8

> **Queue prompt:**
>
> [#pcacheregtrunc]


Done. The effectiveness gate counted `read_create_regression` from the display-truncated (`truncate(6)`) diagnostics vec, and the session-level regression was pushed **last** — so a long degraded session with 6+ per-turn ratio-drop/creation-spike diagnostics truncated the regression away and silently passed `maximum_read_create_regressions: 0` when it genuinely regressed.

Fix in `packages/tsift-agent-doc/src/session_cost.rs`:
- **Gate counts from the raw signal:** a shared `prompt_cache_read_create_regression(cached, creation)` helper computes the regression directly from token totals, so `build_prompt_cache_effectiveness_report` no longer depends on the truncated display vec.
- **Display preserves it too:** `derive_prompt_cache_diagnostics` reserves a slot for the session-level regression before truncating per-turn diagnostics, so the human-visible list can't silently drop it either.

- **Tests:** `read_create_regression_survives_diagnostics_truncation` (8 creation-spike turns + overall regression → regression survives the 6-cap). Existing prompt-cache + real-session fixture gates stay green.
- **Spec:** noted the raw-signal count in `specs/digests-sessions.md`.

**Caught a masked regression:** this cycle's `make check` surfaced a clippy `-D warnings` failure in last cycle's `#gdbwritelock` code (a redundant `fs4::FileExt` import in the Drop impl — `unlock` resolves via std's inherent `File::unlock`). The prior cycle's RTK-wrapped `make check` had **masked the non-zero exit**. Fixed it here and verified with a comprehensive error grep (not just test-result lines). `make check` now genuinely green; release+install rebuilt.

- **Commit + push:** tsift `c942737` → `origin/main`; superproject pointer `8e4017c12`. CI triggered (pending; local gate green).

Next in queue: `#loggatefidelity`.

### Re: do #loggatefidelity — opus-4-8

> **Queue prompt:**
>
> [#loggatefidelity]


Done. The log-digest token-savings gate was structurally always-passing for large logs: savings ≈ `1 - capped_digest/raw`, and the digest is cap-bounded (12 signals / 8 families) while raw grows unbounded, so savings → ~100% no matter how many distinct failures were dropped past the caps. Only `required_signals` substring checks guarded fidelity, and they can't express "all N failures survived".

Fix in `packages/tsift-digest/src/log_digest.rs`:
- **Expose `error_signal_groups`** on the report — distinct error-severity signal groups counted **pre-cap**.
- **New per-case gate `maximum_dropped_error_signals`** (default 0): the gate compares total distinct errors against the inline errors the digest actually surfaces (`dropped_error_signals = error_signal_groups − shown`) and fails closed when distinct failures vanish past the cap without being acknowledged. The raw-log artifact still carries the dropped errors; a case can raise the bound to acknowledge expected overflow.

- **Tests:** `fixture_gate_flags_distinct_errors_dropped_past_the_cap` (20 distinct errors → gate fails at default 0, passes when acknowledged). Existing JSON fixture gate (all cases ≤12 distinct errors) stays green.
- **Spec:** documented the fidelity guard in `specs/digests-sessions.md`.
- **Verify:** `make check` green (comprehensive error grep, not just test-result lines); release+install rebuilt.
- **Commit + push:** tsift `d4ee76e` → `origin/main`; superproject pointer `a5ddf0398`. CI triggered (pending; local gate green).

Next in queue: `#gdbwalcheckpoint`.

### Re: do #gdbwalcheckpoint — opus-4-8

> **Queue prompt:**
>
> [#gdbwalcheckpoint]


Done. After a projection refresh, `replace_projection_with_version` ran `let _ = execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")`. That pragma reports its outcome in a **result row** `(busy, log, checkpointed)` rather than erroring, and `execute_batch` discards rows entirely — so a `busy` result (concurrent readers pinning the WAL → not truncated) was invisible, and the `-wal` file could grow unbounded across refreshes until a later snapshot export/import tripped the live-sidecar check.

Fix in `packages/tsift-sqlite/src/lib.rs`: read the checkpoint result via `query_row` and record a `wal_checkpoint:ok` / `:busy` / `:error` phase in `phase_timings` (which the refresh report already surfaces), so the outcome is observable instead of swallowed.

- **Tests:** `wal_checkpoint_outcome_is_recorded_not_swallowed` (happy path → `:ok`) and `wal_checkpoint_records_busy_when_a_reader_blocks_truncate` (a second connection holds a read transaction, deterministically pinning the WAL → `:busy`).
- **Spec:** documented the recorded outcome in `specs/graph.md`.
- **Verify:** `make check` green (comprehensive error grep); release+install rebuilt.
- **Commit + push:** tsift `7914b31` → `origin/main`; superproject pointer `ea5cf2fa3`. CI triggered (pending; local gate green).

Next in queue: `#logfoldname` (the `[recommended]` cargo-progress `<name>` over-fold precision fix).

### Re: do #logfoldname — opus-4-8

> **Queue prompt:**
>
> [#logfoldname]


Done. `templatize_line` folded the bare identifier immediately before a `v`-prefixed version into `<name>` regardless of position, so connectives in `Downgrading serde from v1.0.0` / `Updating tokio to v2.0.0` had their `from`/`to` token folded — collapsing the two into indistinct templates.

Fix in `packages/tsift-digest/src/log_digest.rs`: restrict the fold to the canonical `<Verb> <crate> v<version>` three-token shape — the version must be the third token (`out.len() == 2`) after a Title-case progress verb (`is_cargo_progress_verb`). Canonical `Compiling serde v1.0.193` still folds to `Compiling <name> <ver>`; `Downgrading serde from v1.0.0` now keeps `from` and stays distinct from the `to` variant.

- **Tests:** `templatize_line_folds_only_canonical_cargo_progress_shape` (canonical folds; connectives preserved + distinct; lowercase non-progress lines don't fold). Existing `log_digest_folds_near_duplicate_line_families` stays green.
- **Spec:** noted the canonical-shape restriction in `specs/digests-sessions.md`.
- **Verify:** `make check` green; release+install rebuilt.
- **Commit + push:** tsift `4e08a6d` → `origin/main`; superproject pointer `52d08b7f1`. CI triggered (pending; local gate green).

Next in queue: `#pcachebp` (the last `[recommended]` fix — breakpoint identity array-index instability).
<!-- agent:boundary:e3835f4f -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- ~~[#logfoldname]~~
- [#pcachebp]
- What are #next-steps for fixing bugs?
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority -->
- [ ] [#pcachebp] [recommended] prompt-cache breakpoint identity includes the JSON array index (session_cost.rs:3368-3372 via path content[N]), so inserting a non-cached content block ahead of the cached one (e.g. a leading thinking/text block) shifts content[0]->content[1] and reports false breakpoints/fingerprint drift though the cache boundary did not move. Make breakpoint identity position-independent (key by type:ephemeral + count, drop array indices). med conf, low-med sev (#tscodereview).
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
