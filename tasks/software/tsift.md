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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260613-052903.md`*

Compacted content:
- Archived 1 response topic(s): next-steps to fix tsift bugs
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: do #cicacheguard — opus-4-8

> **Queue prompt:**
>
> [#cicacheguard]


Done. The v0.1.69 CI re-run I triggered last cycle went **green** (`27369227934` is now `[ok]`) — confirming that red was a transient cache-service abort, not code.

Then I hardened the step so it can't fail the suite again: both `actions/cache@v4` steps in `.github/workflows/ci.yml` (the `clippy + test` and `live Convex graph acceptance` jobs) now carry `continue-on-error: true`. A cache-*service* outage (as opposed to a normal cache miss) now falls through to a cold build instead of aborting the job.

- **Spec/tests:** N/A — a GitHub Actions YAML resilience tweak with no Rust surface to unit-test; rationale documented inline in the workflow.
- **Verify:** YAML validated, `make check` green (clippy `-D warnings` clean, 49 tests ok, 0 failed).
- **Build/install:** skipped — no Rust code changed, so the binary is unchanged.
- **Commit + push:** tsift `96b42c4` pushed to `origin/main`; superproject pointer bumped (`cd6d56632`).

Next in the `go` queue: `#tscodereview`.

### Re: do #tscodereview — opus-4-8

> **Queue prompt:**
>
> [#tscodereview]


Done — ran a fresh concurrency/edge-case review across the three subsystems that landed since the last sweep, fanned out as three parallel review agents (graph-db/SQLite/snapshot, log-digest, prompt-cache). Each agent verified findings by executing the real crate code, not just reading it. **9 genuine root-cause findings captured to the backlog in priority order**; no fabricated/speculative items.

**Highest-severity findings:**
- `#logclassfn` (top) — log-digest drops canonical failure lines (`test result: FAILED`, pytest `N failed`, `make: *** Error 1`, `Segmentation fault`, Rust assert). The digest can silently omit the *only* failure line in a transcript.
- `#logfoldloss` — distinct error lines fold into one line-family and get truncated past the 12-signal cap, so 8+ failures vanish from the digest body.
- `#pcacheattr` — prompt-cache drift attribution always reports `stable_prefix_fingerprint` (the derived composite is pushed first), so it never names the real changed field; the one test can't catch it because it changes everything at once.
- `#gdbwritelock` — graph-db writes/refresh and snapshot-import rename take **no** cross-process lock (only `busy_timeout(5s)`); TOCTOU corruption + spurious `database is locked` under parallel agents. The fix reuses the existing `index.lock` discipline.

Then `#pcacheregtrunc`, `#loggatefidelity`, `#gdbwalcheckpoint`, and two `[recommended]` precision items (`#logfoldname`, `#pcachebp`).

**Verified sound** (not flagged): `#logsigfp` token-anchoring, unicode-safe tokenizer, read-only snapshot fallback / fail-closed export+refresh, `temp_table_active` single-conn re-entrancy guard, full-projection cache reuse (stale → correct cache-miss), no timestamp/path/run-id leakage into the cache fingerprint, scorecard ROI math.

No code changed this cycle — this is a review pass; each finding is its own actionable backlog item, so nothing to spec/test/build/install/commit in the tsift repo. Queue `#<id>` (e.g. `[#logclassfn]`) to execute any fix, exactly as you drove `#cicacheguard`.

**Top of backlog: `#logclassfn`.**
<!-- agent:boundary:66fe95fd -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority -->
- [ ] [#logclassfn] log-digest classify_generic_signal (packages/tsift-digest/src/log_digest.rs:1021-1048) misses canonical terminal-failure lines: `test result: FAILED`, pytest `N failed` summary, `make: *** [..] Error 1`, `Segmentation fault (core dumped)`, Rust `assertion ... failed`. These are often the ONLY failure line in a transcript, so the digest silently drops the failure (verified each -> signals=[]). Add token-anchored rules + required/forbidden fixtures. HIGH conf/sev (#tscodereview).
- [ ] [#logfoldloss] log-digest line-family folding (log_digest.rs:846-881) collapses semantically-distinct error lines into one template (first/last sample only) and MAX_SIGNALS=12 (:508) drops the rest, so 8+ distinct failures vanish from the digest body even when a raw-log artifact is attached. Exempt error-classified lines from folding or surface a distinct-error count/sample. HIGH conf, med-high sev (#tscodereview).
- [ ] [#pcacheattr] prompt-cache drift attribution wrong on default path: the derived composite fingerprint (packages/tsift-agent-doc/src/session_cost.rs:3283-3294, hashes provider+cache_key+stable_prefix+breakpoints) is pushed FIRST in prompt_cache_field_changes (:1770-1801), so first_changed_field is ALWAYS stable_prefix_fingerprint and never names the real cause (cache_key/breakpoints/routing/provider). The lone attribution test changes everything at once so it cannot catch this. Push concrete sub-fields before the derived fingerprint (or exclude it unless explicit). HIGH conf, med-high sev (#tscodereview).
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
