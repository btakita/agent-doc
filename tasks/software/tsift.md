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
<!-- agent:boundary:c8ca7a35 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- do [#tsreview2]
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#gdblockcover] graph-db advisory write lock does not cover all writers (root-cause concurrency, HIGH conf, med-high sev). #gdbwritelock claims the lock covers all writer callers but two paths bypass acquire_graph_db_write_lock: (1) compact --apply issues DELETE+wal_checkpoint(TRUNCATE)+VACUUM at packages/tsift-cli/src/commands/infra.rs:1133 -> packages/tsift-sqlite/src/lib.rs:2025; (2) memory upsert_projection write at packages/tsift-cli/src/commands/memory.rs:976. Both race a concurrent graph-db refresh that holds the lock -> the exact database-is-locked / VACUUM-vs-WAL collision the lock was added to prevent. Fix: wrap both call sites in crate::acquire_graph_db_write_lock(&graph_db) + add concurrent regression coverage.
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
