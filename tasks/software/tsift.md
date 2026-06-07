---
agent_doc_session: tsift-v0.1
resume: 019e85d7-f9ee-79f3-b597-4ed352495bbd
agent: codex
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
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260607-054307.md`*

Compacted content:
- Archived 5 response topic(s): [#trt1inject]; incremental markdown reparse - gpt-5; release + publish; 2 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: lazily-rs IPC for tsift/agent-doc — gpt-5

Yes, but mostly as a data-plane primitive, not as the main fix for tsift/agent-doc IPC correctness.

A blob arena would help if lazily-rs exposes it as a shared leaf crate or small IPC substrate that agent-doc and tsift can use without coupling either project to lazily-rs internals. The useful shape is: control messages stay small and carry `blob_id`, byte length, content hash, producer epoch, TTL/lease, and fallback path; large payloads live in the arena. That would fit agent-doc/tsift surfaces that move large patch bodies, sidecar snapshots, context packs, digest-runner artifacts, session-review output, verbose test/build logs, and maybe editor preview payloads.

A SHM ring buffer is less compelling as the first tsift/agent-doc adoption target. The current painful agent-doc IPC failures are not primarily throughput failures; they are ordering, proof, generation, and convergence failures: late IPC patches, stale live buffers, boundary reposition drift, and committed snapshot vs visible document mismatch. SHM can make those failures faster unless the protocol also has strict sequence numbers, actor/session generations, idempotent apply, ack proof tied to committed content hashes, and deterministic recovery.

So the practical split I would use:

- Keep the control plane boring: socket/file IPC with request ids, generations, content hashes, and explicit commit proof.
- Add the blob arena as an optional payload plane for large bodies and artifacts.
- Keep durable file fallback mandatory, because agent-doc closeout needs recoverability after crashes, editor restarts, and late patch delivery.
- Treat SHM ring transport as a later optimization for sustained streaming paths only after metrics show copy or file IO dominates.
- Do not let shared memory represent mutable markdown/session state directly; only pass immutable payload references plus verification metadata.

For tsift specifically, the best early use would be artifact-heavy commands: `digest-runner`, `session-review`, `context-pack`, maybe large search/explain previews. For agent-doc, the best early use would be patch/sidecar payload indirection while preserving the existing commit-boundary invariants. That would improve payload handling without weakening the proof model.

Bottom line: the blob arena likely benefits tsift IPC with agent-doc if it is designed as an immutable, content-addressed payload layer. SHM ring buffer should wait until correctness and recovery are locked down and measurement shows transport throughput is actually the limiting factor.
<!-- agent:boundary:32cc4493 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" -->
- In lazily-rs, I'm implementing IPC support. This includes a blob arena for IPC transport in progress and potentially a SHM ring buffer for IPC transport. Would this benefit tsift IPC with agent-doc?
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority -->
- [ ] [#surrealbulk] SurrealDB projection performance (consolidates surrealbulk + surrealbulk-close + surreal-batch-bfs + surreal-fixture): replace per-row upsert loops with bulk/transactional row load + one-pass secondary-index rebuild; add batch frontier expansion; record SurrealDB samples in graph-db-performance-history.json fixture; re-run backend-eval benchmarks, update SPEC.md/BENCHMARKS.md table, measure refresh vs load separately.
- [/] [#e2e-install] E2E verify opencode-tsift install in a fresh opencode project: opencode plugin opencode-tsift && opencode then /tsift-status returns healthy (consolidates e2e-install + full-e2e-verify). Live-verify only; npm publish + OIDC auth already done (opencode-tsift@0.1.63 on npm).
- [/] [#7qky] [recommended] AGENT-DOC-SCOPED (drive from an agent-doc session, not tsift): re-measure Codex redundant runbook reads after the v0.34.0 retain-dont-reread nudge accumulates live Codex sessions; compare to baseline (Claude 11.4% / Codex 12.6% / OpenCode 5.3%). If Codex rate does not drop, build a session-scoped content-dedup LEDGER in a shared LEAF crate (register(id,hash)/should_deliver->Full|Stub; exemptions via hash-change+compaction; session-id from harness). GATED on accumulating post-nudge Codex sessions.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#rankdefault] Promote ranked_neighborhood into default graph lookup/context-pack/evidence output once quality gates pass; compare ranked vs unranked on real backlog/code queries (useful-hit density, duplicate-name precision, handle coverage, latency, emitted tokens). Also integrate the shipped #memgraphrag1 decay-weighted memory ranking so memory + code share one ranking over the unified graph.
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
