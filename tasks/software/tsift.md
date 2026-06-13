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
<!-- agent:boundary:6e51f400 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- ~~[#pcacheattr]~~
- [#gdbwritelock]
- [#pcacheregtrunc]
- [#loggatefidelity]
- [#gdbwalcheckpoint]
- [#logfoldname]
- [#pcachebp]
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority -->
- [ ] [#gdbwritelock] graph-db write/refresh (tsift-cli/src/lib.rs:14037, tsift-sqlite/src/lib.rs:833) and snapshot-import rename (tsift-cli/src/commands/infra.rs:752-785) take NO cross-process lock; they rely only on SQLite busy_timeout(5s). Snapshot-import exists->sidecar-check->rename->rename races a concurrent `graph-db refresh` (TOCTOU -> graph.db corruption/lost refresh); parallel refreshes spuriously fail `database is locked` past 5s. Apply the existing index.lock/try_lock_exclusive discipline (tsift-index/src/index.rs:1565) to graph.db writes + the import rename window; map VACUUM-INTO lock error (infra.rs:650) to the live-lock diagnostic. med-high conf, HIGH sev (#tscodereview).
- [ ] [#pcacheregtrunc] prompt-cache read_create_regression gate false-negative: derive_prompt_cache_diagnostics pushes the session-level regression LAST (session_cost.rs:1649) then truncate(6) (:1668), and the effectiveness fixture gate counts regressions from the truncated vec (:682-688). A long degraded session with 6+ per-turn ratio-drop/creation-spike diagnostics truncates the regression away, so maximum_read_create_regressions:0 passes when the session genuinely regressed. Count from the raw signal pre-truncation (or push session-level regressions first). HIGH conf, med sev (#tscodereview).
- [ ] [#loggatefidelity] log-digest token-savings gate is structurally always-passing for large logs (log_digest.rs:265-322): savings ~= 1 - capped_digest/raw, digest is cap-bounded (12 signals/8 families) while raw grows unbounded, so savings -> 100% regardless of how much signal dropped past the caps; only required_signals substring checks guard fidelity and cannot express all-N-failures-survived. Add a distinct-error / dropped-past-cap coverage assertion. HIGH conf, med sev (#tscodereview).
- [ ] [#gdbwalcheckpoint] graph-db swallows WAL checkpoint failures: after a successful projection write, tsift-sqlite/src/lib.rs:1042-1046 does `let _ = ...wal_checkpoint(TRUNCATE)`. Concurrent readers block TRUNCATE (SQLITE_BUSY is normal), the error is discarded, -wal grows unbounded across refreshes, and a later snapshot-export/import trips fail_if_unsafe_sidecars (live -wal). Log/warn on checkpoint failure (surface in phase_timings/warnings) + consider PASSIVE fallback. HIGH conf, low-med sev (#tscodereview).
- [ ] [#logfoldname] [recommended] log-digest templatize_line <name> over-fold (log_digest.rs:872-878) rewrites the plain token before a v-prefixed version to <name> without checking the cargo `<Verb> <crate> v<ver>` shape, so `Downgrading serde from v1.0.0` and `Updating tokio to v2.0.0` both fold to `... <name> <ver>`, losing the from/to distinction. Restrict the fold to the cargo-progress shape. HIGH conf, low-med sev (#tscodereview).
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
