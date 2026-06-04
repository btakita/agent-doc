---
agent_doc_session: 3d9ff8e4-bafd-4e1f-b48d-550d3c976904
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
queue_active: true
---



# lazily-rs: Reactive Signals Library

<!-- agent:status -->

- [x] Phase 1: Research & design
- [x] Phase 2: Core signal primitives
- [x] Phase 3: Derived/computed signals
- [x] Phase 4: Effect system
- [x] Phase 5: Testing & benchmarks
- [x] Phase 6: Documentation & publish
<!-- /agent:status -->

<!-- agent:exchange -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260604-230119.md`*

Compacted content:
- Archived 2 response topic(s): [#mvbj] #ipc1; [#4rxf] #ipc2
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57...

### Re: [#jwwc] #ipc3 — Investigate CRDT vs Raft for lazily-distributed — opus-4-8

Investigated and committed (`lazily-rs` `933e8a8`, pushed to `main`). New **Multi-writer coordination: CRDT vs Raft (`lazily-distributed`)** section in `SPEC.md`. **Conclusion: CRDT (cell-plane HLC registers), not Raft** — with a narrow single-writer authority for irreversible effects.

##### The reactive structure collapses most of the question
Only **cells / source slots** are writable. **Derived slots** (`computed`/`memo`) and the dependency topology are pure deterministic functions of the cells — they recompute, they're never written. So coordination is needed *only on the small cell plane*; the entire derived graph converges for free once cells converge (given replicated compute closures — the same prerequisite #ipc2 flagged for mirror-lazy).

##### Why CRDT over Raft
The roadmap is **availability-first** — P2P signaling (#yxjw), offline peers. Raft buys a global total order (quorum round-trip per write, no minority-partition writes, leader election over WAN) that the reactive model doesn't need, since derived state is already deterministic and the writable surface is small. CRDT gives local-first writes that merge on reconnect.

- Each replicated cell = a **register CRDT keyed by a hybrid logical clock (HLC)**: **LWW-register** default ("current value"), **MV-register** when dropping a concurrent write is unacceptable, **PN-counter** opt-in for additive cells.
- The **PartialEq guard** and **memo equality suppression** still hold — *post-merge*: a merge yielding an equal value invalidates nothing.
- #ipc2's single monotonic `ipc_epoch` generalizes to **per-peer causal stamps** (HLC/dot on each `CellSet`); commutative/associative/idempotent merge means out-of-order delivery self-heals — no snapshot-resync.

##### The narrow exception — irreversible effects
CRDT convergence is right for *state*, wrong for **effects with irreversible external actions** (email, charge, webhook) — merges could fire them zero/twice. Gate those behind a **single-writer effect authority** (a designated peer, or a small Raft group owning *only the effect-intent log*). Hybrid: **CRDT state plane + single-writer/Raft effect plane**, with #39c5 `RemoteOp` allowlist gating what a peer may trigger at all.

Open prototype gates (deferred): HLC skew bounds + LWW/MV default per cell category; closure-replication encoding (gates how much derived plane can live remotely); delta-state vs op-based CRDT wire encoding (reusing `lazily-serde`).

Design-only — `cargo test --test spec_compliance` green (101). The `ipc1→ipc2→ipc3` distributed-design arc is now complete. Queue next: `[#q5r1] #39c5` (implement RemoteOp enum + allowlist) — an *implement* task, not design.

Note: your queue tag edit `auto`→`go` is an unrecognized attribute (preflight ignores it); the queue stays active as `persisted`, but the Claude-Code auto-loop only fires from `auto`. Restore `auto` if you want hands-off draining.
<!-- agent:boundary:5d77879e -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue 

<!-- agent:queue preset="#spec-test-build-install-commit-push" auto -->
- ~[#4rxf]~
- ~[#jwwc]~
- [#q5r1]
- [#4cn0]
- do [#mb9b]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog queue -->
- [ ] [#q5r1] #39c5 Implement RemoteOp enum + per-peer permission allowlist
- [ ] [#4cn0] #yxjw Build Cloudflare Worker signaling server
- [ ] [#mb9b] #vd5v Leverage typed cache for lock-free cached reads in ThreadSafeContext
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#ipc1] [recommended] Design lazily-serde feature gate (trait bounds vs type-erased closures)
- [/] [#ipc2] [recommended] Design lazily-ipc snapshot + incremental update protocol
- [/] [#ipc3] [recommended] Investigate CRDT vs Raft trade-offs for lazily-distributed
- [/] [#39c5] #sig1 [recommended] Implement RemoteOp enum + per-peer permission allowlist for lazily-distributed remote access
- [/] [#yxjw] #sig2 [recommended] Build Cloudflare Worker signaling server for internet-scale peer discovery
- [/] [#vd5v] Future: leverage typed cache for lock-free cached reads in ThreadSafeContext (#lazilyperf38)
<!-- /agent:review -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/lazily-rs.done.md -->

<!-- completed work archived in tasks/software/lazily-rs.done.md -->
<!-- /agent:done -->
