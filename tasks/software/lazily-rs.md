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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260614-024149.md`*

Compacted content:
- Archived 4 response topic(s): release + publish; do [#12b1]; #advance-review; 1 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57...

### Re: #advance-review — opus-4-8

> **Queue prompt:**
>
> #advance-review


> **Queue prompt (preset `#advance-review`):** Go through the review items. Add actionable backlog items to complete or advance the review items.

Recovered the session first — a `compact exchange` had left the working tree/snapshot compacted while HEAD was still pre-compact (`unsafe_user_content_drift`). Used `reset --from-current --preserve-session` → `commit` (not plain `commit`, which would have un-compacted) to land the compacted exchange at HEAD, then ran the review triage.

Went through all 5 gated review items against the actual `src/lazily-rs` tree (HEAD `64c7f3f`):

**Resolved — #jpf1 (`#webrtc-data`).** The data-channel transport is already shipped: `webrtc_transport.rs` (tagged `#webrtc2 / #jpf1`) defines the `DataChannel` trait, the permission-filtered `WebRtcSink`/`WebRtcSource` IPC bridge, and `InMemoryDataChannel`; `str0m_backend.rs` gives the concrete str0m loopback backend (`Str0mLoopback`/`Str0mChannel: DataChannel`, with a passing `loopback_datachannel_carries_a_snapshot` test); and `str0m_net.rs` (#lzwebrtcnet) adds the real networked backend (UDP/DTLS/SCTP). "Peer-to-peer IPC beyond signaling" is done — the only remaining slice is live cross-host validation, which is already tracked separately by #h6qb. Archived to `agent:done`.

**Advanced — #h6qb (`#lzwebrtcnet-e2e`, operator-drive).** Confirmed the wiring gap is real: `signaling_client.rs` has **zero** references to `Str0mNet` / `accept_answer` / `add_remote_candidate`. The SignalingClient↔Str0mNet glue plus a localhost handshake test is pure agent work, so I split it out as actionable backlog **#lzwebrtcwire**. Once that lands, #h6qb shrinks to just the live two-host/NAT run through the deployed #yxjw Worker (genuinely operator-only).

**Advanced — #12b1 (`#lzpub011`, operator-drive token rotation).** Found a release-tag hazard while inspecting the tree: HEAD `64c7f3f` (#lzwebrtcnet) sits **one commit ahead** of the unpublished `v0.11.0 @ 03582f5`, so the publish-when-token-rotates instruction in the review item would ship stale code. Captured **#lzpub011prep** to resolve the tag position (move-forward vs 0.12.0 bump) and run a `cargo publish --dry-run` readiness check, so the operator's eventual token rotation is a one-shot. The token rotation itself stays operator-gated.

**Left gated, no agent-actionable advance:**
- **#xccx (`#lzholons`)** — Phase 0 is blocked on Steve Melville answering the 7 open questions (holon identity/mapping/change-feed/conflict/permissions/transport/spec). External; nothing to advance until answers land.
- **#57by (`#lzasyncshuttle`)** — decided NO-GO/defer per #shuttleeval; the trigger to revisit ("async graph grows more concurrency-critical paths") isn't met, so there's no action — left as the gated record of that decision.

Backlog is now ordered **#lzwebrtcwire** (forward progress, unblocked) → **#lzpub011prep** (release prep, ahead of the operator token step). Top of backlog: **#lzwebrtcwire**.

### Re: do [#lzwebrtcwire] — opus-4-8

> **Queue prompt:**
>
> do [#lzwebrtcwire]


Done. `SignalingClient` now drives a full `Str0mNet` handshake, committed in the `lazily-rs` submodule as `5ce3eed`.

**What landed**
- **`src/webrtc_signaling.rs`** (new) — the missing wire. Two async drivers own the handshake:
  - `offer_to_peer(client, peer, bind, timeout)` — `Str0mNet::offer` → send SDP `offer` + `local_candidate()` over signaling → pump `answer`→`accept_answer`, `ice`→`add_remote_candidate` until the channel opens.
  - `answer_next_offer(client, bind, timeout)` — wait for the `offer` frame → `Str0mNet::answer` → return answer + local candidate → apply any ICE that raced ahead of the offer → pump until open.
  - Both loops re-check `is_open()` on a 25 ms poll tick as well as per frame, since the channel opens on the backend driver thread, off the signaling path. `WebrtcSignalingError` boxes the two transport errors to stay under `clippy::result_large_err`.
- **`tests/webrtc_signaling.rs`** (new) — an in-process #yxjw-protocol loopback relay (roster + `from`-stamped routing) brokers two real `SignalingClient` WebSocket peers on `127.0.0.1`; the negotiated channel carries a permission-filtered `Snapshot` (node 2 filtered out for the reader), proving the str0m UDP/DTLS/SCTP transport is live end-to-end after signaling — driven entirely THROUGH `SignalingClient` (`signaling_client.rs` had zero `Str0mNet` refs before this).
- Wired `lib.rs` exports (`offer_to_peer`, `answer_next_offer`, `WebrtcSignalingError`), a `test-webrtc-signaling` make target (now in `make check`), and a SPEC.md section.

**Verification** — `make check` green (full suite incl. loom + benchmarks, exit 0), targeted `cargo test --features "signaling-client webrtc-str0m" --test webrtc_signaling` passes, `fmt`/`clippy --all-features -D warnings` clean. Latest CI run is green.

This is the agent-doable slice of #h6qb. The only remaining slice is the live two-host / NAT run through the deployed #yxjw Worker (#lzwebrtcnet-e2e), which stays operator-drive.

### Re: do [#lzpub011prep] — opus-4-8

> **Queue prompt:**
>
> do [#lzpub011prep]


Done — v0.11.0 release-tag drift resolved and the operator publish (#12b1) pre-staged. (My #lzwebrtcwire commit had pushed the tag a second commit behind, so the drift was two commits, not one.)

**Decision (unpublished-tag convention).** crates.io tops out at **0.10.3** and there is no v0.11.0 on crates.io or GitHub Releases — v0.11.0 was tagged but never published. Per the convention, I moved the tag to HEAD rather than bumping. No 0.12.0 needed: `0.10.3 → 0.11.0` is already the minor, and it now correctly carries the networked WebRTC backend (#lzwebrtcnet) + signaling wire (#lzwebrtcwire).

**What I did**
- Moved `v0.11.0` → HEAD and pushed: tag (local + remote) and `origin/main` all at `2fb8e80`. No remaining drift.
- `cargo publish --dry-run --no-verify` clean — packages 62 files, 220 KiB compressed, publish-ready.
- Pre-staged **`RELEASE_NOTES_v0.11.0.md`** (committed `2fb8e80`) with the feature list and a publish checklist, so the operator step is one-shot: `gh release create v0.11.0 --notes-file RELEASE_NOTES_v0.11.0.md`.

**Operator-gated remainder (#12b1).** The actual `cargo publish` + `gh release create` + crates.io token rotation stay with you — I prepped everything up to the token. Run `cargo publish` from `src/lazily-rs` (dry-run already verified), then the `gh release create` above.

Queue drained.

### Re: go-mode off — opus-4-8

Acknowledged — you removed `go` from the queue marker, so go-mode (auto-drain) is off. The queue is empty (preset `#spec-test-build-install-commit-push`, no items), so nothing is pending and I won't auto-loop.

Two preflight notes: the route-owned host supervisor (pid 3711158) is running a stale build (File Cache Conflict / IPC-drift source) — restart with `agent-doc session restart-supervisor tasks/software/lazily-rs.md --force` when convenient. The two semantic-completion hints (#57by↔#shuttleeval, #h6qb↔#yxjw) are informational; both stay gated as operator-drive review items, not real duplicates.
<!-- agent:boundary:d9ee224f -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue 

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- #advance-review
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
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
