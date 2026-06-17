---
agent_doc_session: 3d9ff8e4-bafd-4e1f-b48d-550d3c976904
agent: opencode
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
queue: stop
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260617-222852.md`*

Compacted content:
- Archived 3 response topic(s): #next-steps to fix lazily bugs; do #lzstr0mframe; do #lzstr0mpolldrive
- Prior summary/context: ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4af...

### Re: #advance-review — review triage, 2 actionable advances filed — glm-5.2

> **Queue prompt:**
>
> #advance-review

Went through all four `[operator-drive]` review items. **Two have completed their agent-doable slices** (no new work to file — adding items just to add them would violate "complete over gate"); **two have genuine agent-doable advances** that move the operator gate closer to executable.

**`#kb9b` (lazily-py v0.11.0 publish) — agent-doable slice complete, no new item.** The SHIP-STOPPER `#lzpysigfix` is fixed (commit `30907ea`), artifacts rebuilt in `src/lazily-py/dist/` (`lazily-0.11.0-{whl,tar.gz}`, confirmed present), twine check passed, fresh-venv install round-trip verified. Every remaining step (`tag v0.11.0 + push`, `gh release create`, `twine upload`) requires the PyPI token + browser/passkey 2FA — genuinely operator-only. lazily-py has no WebRTC/SCTP layer so the `#lzstr0mframe`/`#lzstr0mpolldrive` class doesn't apply. Nothing to file.

**`#kabc` (LinkedIn post + first comment) — agent-doable slice complete, no new item.** Main post is published and repaired (Corky edit applied, mail commit `07a9ea1`, superproject gitlink `e7e0b08c1`). The remaining operator follow-up is a single `corky linkedin comment <published-file> <comment-body>` from the already-drafted `mail/drafts/linkedin/2026-06-16-lazily-architecture.md`. One operator command; no agent-doable advance.

**`#h6qb` (live two-host WebRTC e2e) — agent-doable slice done; filed `#lzwebrtcrunbook` to advance the gate.** `#lzwebrtcwire` is DONE (localhost handshake covered by `tests/webrtc_signaling.rs`). With `#lzstr0mframe` (silent frame loss) and `#lzstr0mpolldrive` (send_to error surfacing) just landed this session, the networked backend is meaningfully more reliable for the live run. The remaining gate is a live two-host/NAT run through the deployed `#yxjw` Worker (operator-only). **`#lzwebrtcrunbook`** advances the gate by writing the operator runbook (two-host setup, exact commands, expected output, NAT troubleshooting, proof capture) so the live run is executable in one pass instead of improvised. `#h6qb` stays `[operator-drive]` — the runbook doesn't satisfy it.

**`#xccx` (Holons integration) — gate real; filed `#lzholonsscan` to sharpen the questions.** Phase 0 is blocked on Steve Melville answering 7 OPEN QUESTIONS. **But the `evomimic/map-holons` repo has been actively evolving** since the original discovery phase (commits through 2026-06-15, 2 days ago): DanceV2 command ingress (PRs #547/#549/#545), inverse relationship persistence + HasInverse materialization, CollectionType schema, commit pass 2 relationship resolution, command contract alignment PRO1/PRO2. Several of these touch the same surface as the OPEN QUESTIONS (write/conflict model, holon↔node/edge mapping, holon identity). **`#lzholonsscan`** [recommended] re-scans the public commits and updates `plan-map-holons-integration.md` with a dated re-scan section, recording per-question: answered-by-code / sharpened / still-needs-Steve. Doesn't unblock `#xccx` alone, but shrinks the surface Steve has to answer.

**Preflight warning reviewed:** `semantic_completion_match` on `#h6qb` flags that `#lzwebrtcwire` is done — true, but the review item's *remaining* work is the live two-host run, which is correctly `[operator-drive]`. Gate is real; `#h6qb` stays open. The `stale_install` warning (cdylib predates `cc79c412`) is the recurring IPC/supervisor staleness — explains the `IPC proof insufficient` recoveries this session. Recommend `make install` in `src/agent-doc` before the next managed cycle.

**Backlog (priority order):** `#lzwebrtcrunbook` (concrete operator-enablement for the next live gate) → `#lzholonsscan` [recommended] (sharpen #xccx questions for Steve). Both are ready for `do #id` dispatch; neither is auto-implemented here (the `#advance-review` preset's task is to identify work, not execute it).

**Top of backlog: `#lzwebrtcrunbook`.**

### Re: do #lzwebrtcrunbook — live e2e driver + runbook shipped — glm-5.2

> **Queue prompt:**
>
> do [#lzwebrtcrunbook]

The runbook alone wouldn't be "executable in one pass" without a driver binary, so I shipped both halves. The operator can now run the live two-host gate with one command per side.

**`examples/webrtc_live_e2e.rs`** (lazily-rs `a876967`, +148 lines) — operator-facing CLI binary behind `signaling-client + webrtc-str0m`. Positional args: `<signaling_url> <session> <peer_id> [offer_to_peer_id] [bind_addr]`. The 4th arg selects offerer mode (`offer_to_peer`); its absence selects answerer mode (`answer_next_offer`). After the channel opens, the offerer sends a permission-filtered `Snapshot` (node 1 readable, node 2 denied), the answerer asserts it received `nodes=[1]` — the exact shape of `tests/webrtc_signaling.rs` and `tests/str0m_net.rs`, so a green run on two real hosts proves the live path matches the tested localhost path. `OPEN_TIMEOUT = RECV_TIMEOUT = 30 s`. Registered in `Cargo.toml` with `required-features = ["signaling-client", "webrtc-str0m"]`.

**`tasks/software/runbook-lazily-webrtc-live-e2e.md`** (superproject `8bdd32002`, +125 lines) — the operator procedure: prerequisites (deployed Worker + two hosts on different NATs + UDP firewall hole), build + execution commands for both sides, expected `[lazily-e2e] …` log output, timing budget, a troubleshooting matrix (answerer-not-joined-first, UDP blocked, symmetric NAT — host-only candidates can't traverse, signaling Worker down / `SIGNALING_MODE=allowlist`, `Str0mNetError::Closed` post-`#lzstr0mpolldrive` I/O error surfacing, permission-filter leak), and proof capture (both hosts' logs + candidate strings, topology, SHAs) for `#h6qb` closure.

**Scope note on symmetric NAT:** the runbook is honest that `Str0mNet` advertises only a host ICE candidate today (no STUN/TURN), so a symmetric-NAT-only target deployment cannot traverse without a follow-up module. That's out of scope for `#h6qb` (which is about validating the existing backend over a realistic-but-traversable NAT pair), but the troubleshooting section names it so the operator knows when to file a follow-up rather than chase a misconfiguration.

**Verification (local):**
- `cargo build --example webrtc_live_e2e --features "signaling-client webrtc-str0m"` clean.
- `cargo clippy --all-features --all-targets -- -D warnings` clean (fixed one `collapsible_if` lint in the answerer receive loop); `cargo fmt --check` clean.
- `cargo test --features "signaling-client webrtc-str0m" --test webrtc_signaling --test str0m_net` → **3 passed** (1 signaling + 2 str0m_net, including the `#lzstr0mframe` burst regression); no regressions.
- `cargo publish --dry-run --allow-dirty` clean.

**Commit + push:** lazily-rs submodule `a876967` → `lazily-hub/lazily-rs` main (`bbed68a..a876967`); superproject `8bdd32002` → `btakita/agent-loop` main (`35a8df61d..8bdd32002`, carries both the runbook and the submodule bump). CI run `27724380481` queued at closeout — finishing from local evidence per the AGENTS.md "in-progress CI" rule.

**`#h6qb` stays `[operator-drive]` gated** — the runbook + driver advance the gate (make it executable in one pass), they do not satisfy it. The live two-host run is still operator-only.

**Top of backlog: `#lzholonsscan`** (re-scan `evomimic/map-holons` recent commits to sharpen `#xccx`'s 7 OPEN QUESTIONS).

### Re: do #lzholonsscan — Phase 0 surface shrunk 7→4 — glm-5.2

> **Queue prompt:**
>
> do [#lzholonsscan]

Re-scanned `evomimic/map-holons` public sources (commits through `ab3239f` 2026-06-15, `ARCHITECTURE.md`, the [Layer 0: Holons](https://evomimic.gitbook.io/map-book/map-foundations/layer-0-holons.md) book page, and the [MAP Core Dev Docs](https://memetic-activation-platform.github.io/map-dev-docs/core/) reference) against the 7 OPEN QUESTIONS in `plan-map-holons-integration.md`. Wrote a dated re-scan section with per-question tags (answered-by-code / sharpened / still-needs-Steve).

**Three questions substantially resolved from public sources:**

- **Q2 (holon↔node/edge mapping) — answered-by-code.** Holons have typed relationships + properties + actions; relationships carry `holonic__attraction` (composition/aggregation/association continuum) and now have auto-materialized inverses (the recent inverse-relationship-persistence work). Maps cleanly: holon → `NodeSnapshot`, typed relationship → `EdgeSnapshot` with attraction as an edge weight/marker. *Open sub-question for Steve:* how to represent containment (holon-inside-holon).
- **Q6 (transport expectations) — answered-by-code.** `ARCHITECTURE.md` is explicit: the repo "mirrors runtime execution contexts" — **hApp (WASM/Holochain)** and **Host (Native Rust + TS)** are strictly separated, with `shared_crates/` obeying WASM constraints. lazily-rs (tokio + threads) can run **only in the Host context**, attached via **Conductura** (the host command/integration subsystem + Holochain receptor). WebRTC/WS/IPC transports are for **host↔host** peer traffic (UI sync, DAHN visualizers), **not** Holochain DHT traffic. → `#akp3` WebSocket priority confirmed for host↔UI; WebRTC for host↔host across NAT.
- **Q7 (references) — sharpened.** Canonical docs now exist: Layer 0 book (conceptual), repo `ARCHITECTURE.md` (workspace/build model), MAP Core Dev Docs (contributor-facing), and the DanceV2 PRO1/PRO2 command-contract PRs (#538, #544 — closest to a wire spec). Still no single `PROTOCOL.md`-equivalent.

**Four sharpened but still-needs-Steve:**

- **Q1 (holon identity)** — Holochain's `ActionHash`/`EntryHash` is implied; `NodeId(u64)` won't fit verbatim so the adapter needs a registry. *Confirm:* is there a separate semantic id stable across re-commits?
- **Q3 (change feed)** — DanceV2 + commit pass 2 introduce a typed command/result surface, but no documented event stream for external subscribers. *Confirm:* change-event feed exists, or diff-on-snapshot is the integration pattern?
- **Q4 (write authority)** — DanceV2 is the write path; Holochain source-chain + DHT is the authority. *Confirm:* does the adapter forward writes as DanceV2 commands from a single agent, or fan-out? How does this reconcile with lazily's single-writer-per-effect?
- **Q5 (permissions)** — MAP's per-property/per-relationship "willingness to share" is finer-grained than lazily's per-node `PeerPermissions`. *Confirm:* per-property filtering (drop permissions on the floor) or collapse to per-node (over-share)?

**Net effect on `#xccx` Phase 0:** the surface Steve has to answer shrinks from 7 questions to 4. Adapter scaffolding (Phase 1 read-only inbound) can start against the resolved 3 + working hypotheses on the open 4, then be corrected when Steve's answers land — no longer fully blocked on discovery.

**`#xccx` stays `[operator-drive]` gated.** The re-scan advances it (Phase 0 closer to unblocked, Phase 1 scaffoldable) but does not satisfy it — Steve's input is still required before the adapter design is settled.

**Commit + push:** superproject `824a305e3` → `btakita/agent-loop` main (`8bdd32002..824a305e3`). No lazily-rs submodule change (research/docs only).

**Queue drained.** All three `do #id` directives from this session's review-triage cycle (`#lzwebrtcrunbook`, `#lzholonsscan`, and earlier `#lzstr0mframe` / `#lzstr0mpolldrive`) are closed. Backlog empty; four `[operator-drive]` review items remain (`#kb9b`, `#kabc`, `#h6qb`, `#xccx`) — all advanced where the agent-doable slice existed, none auto-resolved.

### Re: release + publish — lazily-rs v0.12.0 shipped (crates.io + GitHub) — glm-5.2

> **Queue prompt:**
>
> :pushpin: release + publish


> **Queue prompt:**
>
> - release + publish   (preset `#spec-test-build-install-commit-push`)

Cut and published **v0.12.0** (not 0.11.2). The 3 commits since tag `v0.11.1` add a new public error variant `Str0mNetError::Backpressure` (`#lzstr0mframe`), and `Str0mNetError` is **not** `#[non_exhaustive]` — so adding a variant is a breaking change for exhaustive `match` arms. Per this repo's own convention (v0.11.1 notes: "No API changes… safe bump"), a breaking change cannot ride a patch bump. For a 0.x crate that maps to a minor bump → **0.12.0**. The break is scoped entirely to the opt-in `webrtc-str0m` feature; the core reactive primitives (`Context`/`ThreadSafeContext`/`AsyncContext`, slots/cells/effects/signals) are byte-unchanged.

**What shipped** (all pre-existing commits this session; the release commit is just the version bump + notes):
- **`#lzstr0mframe`** — silent frame loss under SCTP backpressure. `Channel::write` returns `Ok(false)` when the SCTP send buffer is full; the old `if ch.write(...).is_err() { break; }` discarded the `bool` and silently dropped every frame over the send window — violating the ordered/reliable DataChannel invariant. Now `Ok(false) | Err` re-queues + yields; bounded queue (`MAX_PENDING_FRAMES=1024`) + `Backpressure` variant cap memory.
- **`#lzstr0mpolldrive`** — `let _ = socket.send_to(...)` discarded ENOBUFS/ECONNREFUSED/ENETUNREACH/EBADF and silently lost ICE/DTLS/SCTP packets. Now retryable `WouldBlock`/`Interrupted` continue; any other error breaks the driver and surfaces `Closed`.
- New example `webrtc_live_e2e.rs` (already committed at `a876967`).

**Verification (local):**
- `cargo fmt --check`, `cargo clippy --all-features --all-targets -- -D warnings`, `cargo build --all-features` clean.
- `cargo test --features "signaling-client webrtc-str0m" --test str0m_net --test webrtc_signaling --test webrtc_transport` → **7/7 pass**, incl. the `#lzstr0mframe` regression `burst_of_frames_arrives_in_order_under_backpressure` (100 × 8 KiB, in-order).
- Full `make check` functional suites green (107 spec/integration + loom + stress + tokio + async + ffi + ipc + websocket + lean formal model).
- `cargo publish --dry-run` clean: **75 files, 242.5 KiB compressed**, verifies.

**Pre-existing, unrelated `make benchmark-check` red — filed `#lzbenchbudget`.** Deterministic (identical across 2 runs) and **identical at tag `v0.11.1`** (`BENCHMARKS.md` + `thread_safe.rs` are unchanged since v0.11.1, confirmed via `git diff --stat v0.11.1..HEAD`). Four lock-acquisition budget overshoots, all in `thread_safe` contention benchmarks — `src/str0m_net.rs` is the only source v0.12.0 touches and cannot affect those counts. The budgets went stale when `8665e29` + `38b18e4` changed `thread_safe.rs` without a `make benchmark-update`. v0.11.1 shipped with this same red gate. Holding a silent-frame-loss correctness fix hostage to an unrelated pre-existing benchmark budget would be wrong, so v0.12.0 ships; `#lzbenchbudget` (backlog) tracks the budget refresh.

**Publish proof (executed):**
- crates.io `max_version` verified live: **0.12.0** (`curl …/api/v1/crates/lazily` → top versions `['0.12.0','0.11.1','0.11.0',…]`).
- GitHub release: https://github.com/lazily-hub/lazily-rs/releases/tag/v0.12.0 (`gh release create … --notes-file RELEASE_NOTES_v0.12.0.md`).

**Commits + push (executed):**
- lazily-rs submodule `93d9286` (`chore: release v0.12.0`) + tag `v0.12.0` → `lazily-hub/lazily-rs` main (`a876967..93d9286`).
- superproject `ecd4c65c4` (`chore: release lazily-rs v0.12.0`, submodule gitlink only) → `btakita/agent-loop` main (`824a305e3..ecd4c65c4`).

**Queue drained.** Both heads resolved (`#lzholonsscan` was auto-struck by preflight as already-done; `release + publish` completed here). No `#operator-drive` gate opened by this release — `#h6qb` (live two-host run) is unchanged and still operator-only. Top of backlog: **`#lzbenchbudget`**.

### Re: release + publish — closeout repair pass (snapshot/HEAD realign) — glm-5.2

Repair closeout for the v0.12.0 release cycle. The release work itself is unchanged and complete (crates.io `0.12.0` live, GitHub release `v0.12.0`, submodule `93d9286`, superproject `ecd4c65c4`). The prior `finalize` reached `committed` (`8b31777b0`) but a subsequent bare `preflight` introduced a 1-byte snapshot/HEAD boundary drift and an orphaned `preflight_started` cycle. Recovery: `cancel` cleared the orphan; this `write --commit` realigns the snapshot to HEAD and retries the editor/CRDT delivery path so `session-check` clears. No queue, backlog, or review changes in this pass — `#lzbenchbudget` stays the sole backlog item; queue is `#lzbenchbudget` (not in-session drainable, `queue_continuation_required=false`).
<!-- agent:boundary:a41890de -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue 

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- do [#lzbenchbudget]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#lzbenchbudget] #lzbenchbudget=Refresh stale thread_safe instrumentation lock-acquisition budgets in src/lazily-rs/BENCHMARKS.md. make benchmark-check is red (deterministic, reproducible 2x): thread_safe_contention_same_slot_write_read_16 set_cell_invalidation 78>32; thread_safe_contention_independent_slots_16 lock_acquisitions 908>700, other 345>160, set_cell_invalidation 244>64. Pre-existing at tag v0.11.1 (BENCHMARKS.md + thread_safe.rs unchanged since), introduced by 8665e29 (SlotId casts) + 38b18e4 (eager Signal on ThreadSafeContext) without a budget refresh. Either run make benchmark-update to record the new baselines or investigate a real contention regression. Unrelated to the str0m_net v0.12.0 release content.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#kb9b] #lzpypub011=[operator-drive] Publish lazily-py v0.11.0: SHIP-STOPPER #lzpysigfix is FIXED (commit 30907ea). Build artifacts rebuilt in src/lazily-py/dist/ (lazily-0.11.0-{whl,tar.gz}, twine check PASSED, fresh-venv install round-trip verified). From src/lazily-py: tag v0.11.0 + push, gh release create v0.11.0 --generate-notes, then python -m twine upload dist/* (needs PyPI token + browser/passkey 2FA). Gated on the pre-publish live-test confirm per the testing-gate rule. Commit 30907ea + superproject gitlink eb93a75ac already pushed.
- [/] [#kabc] #lipostlazilyarch=[operator-drive] Main LinkedIn post is published and repaired: Corky edit updated mail/social/20260616-lazily-architecture-linkedin.md / https://www.linkedin.com/feed/update/urn:li:share:7472666756810190848 with the full body after the public post truncated at "The core is small: three primitives". Local metadata is committed in mail commit 07a9ea1 and superproject gitlink e7e0b08c1. Remaining operator follow-up: post the first comment from mail/drafts/linkedin/2026-06-16-lazily-architecture.md via `corky linkedin comment <published-file> <comment-body>` if still desired.
- [/] [#h6qb] [recommended] [operator-drive] #lzwebrtcnet-e2e=End-to-end cross-host validation of the Str0mNet backend through the LIVE #yxjw signaling Worker (two real hosts / NAT). Agent-doable slice (#lzwebrtcwire) is DONE: src/webrtc_signaling.rs wires SignalingClient↔Str0mNet and tests/webrtc_signaling.rs covers the localhost handshake. Remaining: live two-host/NAT run via deployed Worker — operator-only.
- [/] [#xccx] #lzholons=[operator-drive] Map Holons ↔ lazily-rs integration. Phase 0 (discovery) is blocked on Steve Melville answering the 7 OPEN QUESTIONS (holon identity, holon↔node/edge mapping, change feed, write/conflict model, permissions mapping, preferred transport, spec reference). Plan: tasks/software/plan-map-holons-integration.md. Unblocks once answers land; then Phase 1 read-only inbound adapter.
<!-- /agent:review -->

## Icebox

<!-- agent:icebox -->
- [ ] [#pw26] #lzmapspike=[recommended] Spike: lazily-rs as a host-side reactive derivation + Snapshot/Delta UI-sync cache for MAP holons (evomimic/map-holons, Holochain). Model committed holons as reactive graph nodes; derived holon views/relationships as memoized computeds that recompute only on upstream change; stream lazily-ipc Snapshot/Delta to the TS UI / DAHN graph visualizers. Native host/ layer only (AsyncContext needs tokio; zomes are wasm32/short-lived/single-threaded so in-zome use is limited to single-threaded Context). Do NOT use lazily distributed/signaling — Holochain DHT owns P2P/persistence/validation/consensus. Gate on MAP host-layer actually recomputing derived holon state.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/lazily-rs.done.md -->

<!-- completed work archived in tasks/software/lazily-rs.done.md -->
<!-- /agent:done -->
