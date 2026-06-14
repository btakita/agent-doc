---
agent_doc_session: 3d9ff8e4-bafd-4e1f-b48d-550d3c976904
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#bugs-observed': Please add any bugs from observed behavior to the backlog.
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: start
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260614-053050.md`*

Compacted content:
- Archived 5 response topic(s): #advance-review; do [#lzwebrtcwire]; do [#lzpub011prep]; 2 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57...

### Re: session recovery (stuck cycle + uncommitted compaction) — opus-4-8

> **Queue prompt:**
>
> #advance-review



No new prompt this session — recovered a wedged state instead.

**What was wrong**
- A stuck cycle (`cycle-1781410314210`) sat in `write_applied`/discarded, blocking preflight. Its body was a duplicate `### Re: #advance-review` whose real response is already committed and archived.
- The working tree carried a fresh compaction (archive `…-053050`) while HEAD was still pre-compact (the 4 `### Re:` responses) — the compaction stale-snapshot case.

**Recovery (committed `7ca156478`)**
- `reset --from-current --preserve-session` → `commit` (not plain `commit`, which would have un-compacted): landed the compacted exchange at HEAD and cleared the stuck cycle. Working tree now clean, 0 `### Re:` headings at HEAD.

**Residual — operator territory (not re-run)**
- The `agent:queue` block still holds an **unstruck** `- #advance-review` under a `priority go` marker, even though that item's response was already given and archived ("Archived 5 response topic(s): #advance-review…"). That re-presented it with go-mode auto. I did **not** re-answer it — that would be churn.
- Paused queue dispatch IPC-safely (`admin queue pause`, receipt 40) to stop the stale-replay flood. Did **not** auto-loop.
- To fully settle: remove the `go` token and strike/clear `#advance-review` from the queue block (its work is done), then `resume` if you want the queue live again. Holding off on editing the queue marker myself since the route-owned supervisor (pid 968752) is live and races queue mutations.

### Re: session recovery (stuck cycle + uncommitted compaction) — opus-4-8


No new prompt this session — recovered a wedged state instead.

**What was wrong**
- A stuck cycle (`cycle-1781410314210`) sat in `write_applied`/discarded, blocking preflight. Its body was a duplicate `### Re: #advance-review` whose real response is already committed and archived.
- The working tree carried a fresh compaction (archive `…-053050`) while HEAD was still pre-compact (the 4 `### Re:` responses) — the compaction stale-snapshot case.

**Recovery (committed `7ca156478`)**
- `reset --from-current --preserve-session` → `commit` (not plain `commit`, which would have un-compacted): landed the compacted exchange at HEAD and cleared the stuck cycle. Working tree now clean, 0 `### Re:` headings at HEAD.

**Residual — operator territory (not re-run)**
- The `agent:queue` block still holds an **unstruck** `- #advance-review` under a `priority go` marker, even though that item's response was already given and archived ("Archived 5 response topic(s): #advance-review…"). That re-presented it with go-mode auto. I did **not** re-answer it — that would be churn.
- Paused queue dispatch IPC-safely (`admin queue pause`, receipt 40) to stop the stale-replay flood. Did **not** auto-loop.
- To fully settle: remove the `go` token and strike/clear `#advance-review` from the queue block (its work is done), then `resume` if you want the queue live again. Holding off on editing the queue marker myself since the route-owned supervisor (pid 968752) is live and races queue mutations.

### Re: Add a reactive Signal (eager, no intermediate unset, v1→v2) — opus-4-8

> **Queue prompt:**
>
> Add a reactive Signal. Where we have Slot->Cell->Signal...where the Signal eagerly computes its value when invalidated. There should be no intermediate unset value. The recalculation should be v1->v2.


Added a first-class **`Signal`** primitive to lazily-rs (`src/signal.rs` + `ctx.signal()`), one step beyond `Slot` on the `Slot → Cell → Signal` progression. Committed `66dcdbc`, pushed to `origin/main`.

**The distinction that drove the design**
- A `Slot`/`computed` is **lazy**: invalidation only marks it dirty; it recomputes on the next read. During the dirty window an observer effectively sees a stale/“to-be-recomputed” value.
- A `Signal` is **eager**: it recomputes the instant a dependency is invalidated — *before the invalidating `set_cell`/`set`/`batch` call returns*. So after `n.set(&ctx, 5)` the signal already holds its new value; no read is needed to drive it.
- **No intermediate unset value:** the backing store is invalidated by dirty-marking, never hard-cleared, so the value transitions directly `v1 → v2` and is never observed as `None`/unset.

**How it's built (reuses the tested core, not a parallel engine)**
A signal is a **memoized backing slot + a small puller effect**:
- `ctx.memo(compute)` gives the value cell with a `PartialEq` guard (equal recompute ⇒ no downstream churn) and **glitch-free, pull-based** recomputation.
- a puller `effect` does `ctx.get_rc(&slot)` after every invalidation, supplying the eagerness. `get_rc` avoids deep-cloning the value on each refresh.

Because recomputation is pull-based (depth-first refresh of dependencies before self), a derived-signal **diamond** `D = f(A, g(A))` never surfaces the mixed new-`A`/old-`g(A)` glitch intermediate — the test asserts `D` only ever takes `[6, 18]`, never the glitch `10`.

**API**
```rust
let n = ctx.cell(1);
let doubled = ctx.signal(|ctx| n.get(ctx) * 2); // materialized now: 2
n.set(&ctx, 5);                                  // doubled is already 10 — eager
assert_eq!(doubled.get(&ctx), 10);
```
`signal<T>` needs `T: PartialEq + 'static` (memo guard); `get_signal` also needs `Clone`. Plus `get_signal_rc`, `dispose_signal` (reverts to lazy), `is_signal_active`, and the `SignalHandle` method aliases.

**Spec + tests + build**
- `SPEC.md`: new `### Signal` section (semantics, glitch-freedom, type bounds, disposal).
- `tests/signal.rs`: 9 integration tests — eager init, eager recompute without a read, `v1→v2` no-unset (via a watcher effect: `[100,200,300]`), memo guard skips equal recompute, chained signals, diamond glitch-freedom, batch coalescing (`[11,22]`, no `12`/`21`), dispose-reverts-to-lazy, equal-value no-op.
- `make check` green (fmt/clippy/build/default + tokio + async + loom + benchmark checks); release build clean.

**One trade-off, noted:** the puller effect is a dependent of its own slot, so an actual value change triggers exactly one extra *no-op* puller run (it pulls, finds nothing dirty, returns). Bounded and harmless; user-facing effect counts are unaffected (only the internal puller double-fires).

**CI:** run `27490089664` is **queued** at push time — recording as pending per the test-bearing workflow; closing out on local `make check` evidence.

**Follow-up queued:** `#lzsignalparity` [recommended] — extend `Signal` to `ThreadSafeContext` and `AsyncContext`, which still expose only lazy slots/memo.
<!-- agent:boundary:a79e1c56 -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue 

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- do [#3dmm]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#x7sp] Update documentation for inclusion of Signal into the architecture. Update the protocol as well.
- [ ] [#vzk9] Create a briantakita.me blog post + linkedin summary draft explaining the lazily Slot->Cell->Signal composition. Include use cases. Add diagrams to post to linkedin.
- [ ] [#ny9k] Create a briantakita.me blog + linkedin summary draft explaining the full lazily architecture. Include use cases. Add diagrams to post to linkedin.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#h6qb] [recommended] [operator-drive] #lzwebrtcnet-e2e=End-to-end cross-host validation of the Str0mNet networked backend through the LIVE #yxjw signaling Worker (two real hosts / NAT, not localhost). The localhost two-socket round trip (tests/str0m_net.rs) already proves the real UDP/DTLS/SCTP/timer path in-process; this remaining slice needs two live endpoints + the deployed Worker so it cannot be agent-run headless. Wire SignalingClient offer/answer/ice to Str0mNet::{offer,answer,accept_answer,add_remote_candidate,local_candidate} and confirm a Snapshot crosses between hosts.
- [/] [#12b1] #lzpub011=[operator-drive] Finish lazily-rs v0.11.0 publish — crates.io upload + GitHub Release are BLOCKED on a revoked crates.io token (403 auth failed on both the env CARGO_REGISTRY_TOKEN and passage btak/CARGO_REGISTRY_TOKEN; both are the same expired cio-prefixed token). Operator: rotate the crates.io token (browser/passkey 2FA), store at passage btak/CARGO_REGISTRY_TOKEN, then from src/lazily-rs run: CARGO_REGISTRY_TOKEN=$(passage btak/CARGO_REGISTRY_TOKEN) cargo publish --no-verify (commit 03582f5 + tag v0.11.0 already pushed), then gh release create v0.11.0 --generate-notes.
- [/] [#xccx] #lzholons=[operator-drive] Map Holons ↔ lazily-rs integration. Phase 0 (discovery) is blocked on Steve Melville answering the 7 OPEN QUESTIONS (holon identity, holon↔node/edge mapping, change feed, write/conflict model, permissions mapping, preferred transport, spec reference). Plan: tasks/software/plan-map-holons-integration.md. Unblocks once answers land; then Phase 1 read-only inbound adapter.
- [/] [#57by] #lzasyncshuttle=[operator-gated, NO-GO for now per #shuttleeval] Exhaustive Shuttle async-resolve-loop model. Spike result: Shuttle can model async (has an executor, unlike Loom) but is randomized-not-exhaustive, needs ~140 cfg-swaps + a custom notifier (no tokio::watch equiv) in the hot path, and the two #k03k windows are already deterministically covered. Defer until the async graph grows more concurrency-critical paths. See tasks/software/plan-lazily-async-loom.md.
<!-- /agent:review -->

## Icebox

<!-- agent:icebox -->
- [ ] [#pw26] #lzmapspike=[recommended] Spike: lazily-rs as a host-side reactive derivation + Snapshot/Delta UI-sync cache for MAP holons (evomimic/map-holons, Holochain). Model committed holons as reactive graph nodes; derived holon views/relationships as memoized computeds that recompute only on upstream change; stream lazily-ipc Snapshot/Delta to the TS UI / DAHN graph visualizers. Native host/ layer only (AsyncContext needs tokio; zomes are wasm32/short-lived/single-threaded so in-zome use is limited to single-threaded Context). Do NOT use lazily distributed/signaling — Holochain DHT owns P2P/persistence/validation/consensus. Gate on MAP host-layer actually recomputing derived holon state.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/lazily-rs.done.md -->

<!-- completed work archived in tasks/software/lazily-rs.done.md -->
<!-- /agent:done -->
