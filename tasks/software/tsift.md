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
<!-- agent:boundary:57a19537 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- [#logclassfn]
- [#logfoldloss]
- [#pcacheattr]
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
