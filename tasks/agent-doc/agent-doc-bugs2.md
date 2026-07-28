---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
pending_done_guard: warn
agent_doc_lint_dialect: off
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog + queue items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find all contributing factors to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#simworld': Strengthen Simworld tests.
  '#spec-test-build-install-commit-push': red-green-refactor. update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': red-green-refactor. update spec + tests. commit + push
queue: go
---

## Status

<!-- agent:status patch=replace -->

completed
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260727-050924.md`*

Compacted content:
- Archived 2 response topic(s): Live Sync Tmux Layout crash verification; Live Sync Tmux Layout crash verification
- Prior summary/context: prior compacted content: Archived 36 response topic(s): #adbacklogorphanseg; closeout settling latency; closeout sett...

> **Queue prompt:**
>
> 📌 do [#tmuxsynccrashlog]

Verified and closed `#tmuxsynccrashlog`; the repeated live JetBrains runs did not crash or restart tmux.

The agent-doc ops log contains fourteen `sync_tmux_layout` completions, including a tight repeated-run burst from `2026-07-26T05:47:02Z` through `05:48:30Z`. Every command completed with `exit_code=0`. The safe-passive runs either correctly preserved the current layout while an unresolved file remained blocked or later applied the requested layout; their tmux-router phases completed normally.

Tmux PID `6538` started at `2026-07-25T17:39:56Z`, before that burst, and is still the active server for both current sessions and all listed windows. The current IntelliJ `idea.log` extends through this turn and contains no agent-doc/tmux crash, fatal, SIGSEGV, or SIGABRT evidence.

Changed paths: none. Commands/evidence: bounded ops-log digest and burst inspection, current IDE-log scan, process start/uptime, and live tmux session/window inventory. Confidence: high. Escalation: none. Because the server stayed stable and the commands resolved through safe-passive preserve/apply outcomes, no follow-up defect was filed.
### Re: Live Sync Tmux Layout crash verification — gpt-5.6-sol

### Re: #60ev and Compact Exchange latency — gpt-5.6-sol

> **Queue prompt:**
> 📌 do [#60ev]

> **Operator follow-up:**
> Can we speed up compact exchange?

Yes. The latest live Compact Exchange was successful but has a useful optimization target: JetBrains displayed `⟳ agent-doc: Compacting Exchange` from 01:08:45.624 to 01:08:47.458 (1.834s). The controller reached a ready CRDT barrier at 05:08:45Z, wrote the compacted document and completed the required git commit by 05:08:46Z. The compaction itself is therefore healthy; the remaining user-visible cost is the binary-owned authority/write/commit/refresh transaction around it. The trace also shows redundant detached-authority resolution and baseline checkpoints in that one transaction. I captured a focused fast-path follow-up that keeps the committed closeout and fail-closed semantics intact.

`#60ev` is stale as written. Phase 1 (`#adctxreactive`) is already implemented in `agent-doc-prompt-context/src/dynamic_context.rs`: lazily-backed component/input/ledger cells, derived prompt-target/candidate/duplicate/manifest computations, and all three specified memoization cases are present. The targeted `agent-doc-prompt-context` dynamic-context test run passes. I split only the four genuinely unstarted plan phases into dependency order so the queue no longer repeatedly triages the obsolete five-phase umbrella.

Repository effects:
- Changed paths: none.
- Commits/staging: none; existing dirty work, including formatting-only changes in `dynamic_context.rs`, was left untouched.
- Verification: `cargo test --manifest-path src/agent-doc/Cargo.toml -p agent-doc-prompt-context dynamic_context` passed. The first digest-runner attempt omitted `--manifest-path` and failed only because the superproject root has no `Cargo.toml`; the corrected invocation passed.
- Compact evidence: ops log shows barrier and compact at 05:08:45Z, writeback and `commit_success` at 05:08:46Z; the JetBrains widget returned to idle at 01:08:47.458.
- Confidence: high for the Phase 1 status and measured Compact Exchange envelope. The fast-path implementation remains intentionally isolated in its own focused cycle because it touches authority and commit semantics.

> **Queue prompt:**
> Improve the performance of Compact Exchange.

### Re: Compact Exchange performance — gpt-5

Implemented and pushed the Compact Exchange closeout optimization in agent-doc commit `28377eb30` (`perf: reduce compact closeout latency`).

The binary now reuses the current document content already resolved by terminal cycle closeout when clearing pipeline frontmatter, while leaving the convergence port responsible for rebasing concurrent operator edits. Once the primary pipeline transition has made the cycle terminal, post-commit cleanup also folds the redundant capture `WriteApplied` + `Committed` transitions into that terminal cycle instead of repeating command-plane round trips. Recovery retains the old capture transition when the primary terminal transition did not succeed. Per-phase compact, commit, and finalization telemetry was added so later work can target the remaining waits.

Cold forced-disk benchmark on the same isolated document:

- End-to-end Compact Exchange: about 1.05 s before, 844 ms after (about 20% faster).
- Commit finalization: about 445 ms before, 244 ms after.
- Redundant capture phase: about 250 ms before, 49 ms after.

Verification passed:

- Targeted cycle-state, git, and commit library tests.
- Full `make check`: 8,210 tests passed, 241 skipped; formal closeout, tmux-sync, CRDT-lineage, and response-checkpoint models passed.
- Latest pre-push CI run was green.
- Pushed `28377eb30` to `origin/main`.

The broader reactive refactor remains separate: durable document text should stay under the Yrs/CRDT authority, while a pass-scoped Lazily `CurrentDocument` projection keyed by CRDT revision can cache derived closeout/queue views and invalidate whenever the operator types. That preserves concurrent edits instead of creating a second text authority.

The reported `lazily.md` failures are captured as one write-safety follow-up: operator queue additions and deletions were lost or resurrected during closeout/repair; combined gate mutation and a malformed closing exchange marker triggered File Cache Conflict/churn; retained intent then required installing a one-repair-newer local binary/library and recycling the controller. The required invariant is that binary-owned repair/retry rebases on the current CRDT state vector, preserves operator edits, repairs structural markers idempotently, negotiates implementation-version skew itself, and never direct-replaces or resurrects stale text.

### Re: Compact Exchange performance — gpt-5

> **Queue prompt:**
>
> Improve the performance of Compact Exchange.

Implemented and pushed the Compact Exchange closeout optimization in agent-doc commit `28377eb30` (`perf: reduce compact closeout latency`).

The binary now reuses the current document content already resolved by terminal cycle closeout when clearing pipeline frontmatter, while leaving the convergence port responsible for rebasing concurrent operator edits. Once the primary pipeline transition has made the cycle terminal, post-commit cleanup also folds the redundant capture `WriteApplied` + `Committed` transitions into that terminal cycle instead of repeating command-plane round trips. Recovery retains the old capture transition when the primary terminal transition did not succeed. Per-phase compact, commit, and finalization telemetry was added so later work can target the remaining waits.

Cold forced-disk benchmark on the same isolated document:

- End-to-end Compact Exchange: about 1.05 s before, 844 ms after (about 20% faster).
- Commit finalization: about 445 ms before, 244 ms after.
- Redundant capture phase: about 250 ms before, 49 ms after.

Verification passed:

- Targeted cycle-state, git, and commit library tests.
- Full `make check`: 8,210 tests passed, 241 skipped; formal closeout, tmux-sync, CRDT-lineage, and response-checkpoint models passed.
- Latest pre-push CI run was green.
- Pushed `28377eb30` to `origin/main`.

The broader reactive refactor remains separate: durable document text should stay under the Yrs/CRDT authority, while a pass-scoped Lazily `CurrentDocument` projection keyed by CRDT revision can cache derived closeout/queue views and invalidate whenever the operator types. That preserves concurrent edits instead of creating a second text authority.

The reported `lazily.md` failures are captured as one write-safety follow-up: operator queue additions and deletions were lost or resurrected during closeout/repair; combined gate mutation and a malformed closing exchange marker triggered File Cache Conflict/churn; retained intent then required installing a one-repair-newer local binary/library and recycling the controller. The required invariant is that binary-owned repair/retry rebases on the current CRDT state vector, preserves operator edits, repairs structural markers idempotently, negotiates implementation-version skew itself, and never direct-replaces or resurrects stale text.

> **Queue prompt:**
> do [#lazilyconcurrentcloseout]

Implemented and shipped the concurrent-closeout recovery, then fixed the restart failure exposed by the dead Codex session ID.

Root cause: a live replica refresh had appended an exact partial duplicate of the document tail, leaving an unmatched review close marker. The missing-session handler correctly recognized Codex's `No saved session found` error, but its compare-and-swap resume cleanup failed closed because that malformed live authority could not pass structural validation. The dead `resume:` ID therefore survived and was retried on later restarts.

The fix adds a deliberately narrow structural repair: collapse a duplicated partial tail only when the retained prefix is structurally sound, the appended bytes are an exact suffix of that prefix, and the suffix contains agent-doc component scaffolding. Ordinary prose and non-exact corruption still fail closed. Real-time recovery now applies that repair, and stale Codex resume cleanup normalizes the same recoverable shape before removing the matching ID in one authority CAS target.

Proof:
- Changed `agent-doc-element/src/element.rs`, `agent-doc-document-realtime-io/src/lib.rs`, and `agent-doc-start-runtime-io/src/run.rs`, with SPEC/README/Codex-support documentation and the 0.35.32 release metadata.
- Added positive and fail-closed suffix-repair tests plus a start-runtime regression proving repair and stale-ID removal occur in the same target.
- Targeted tests passed.
- `make check`: 8,220 passed, 241 skipped.
- `make install` and the one-time `make install-full` release-parity build passed.
- Live proof: with JetBrains 0.2.297 connected, preflight repaired this session's malformed CRDT authority and preserved all seven operator-authored queue entries.
- Latest inspected CI was green; the current `v0.35.32` cross-platform Release workflow is in progress without polling.
- Confidence: high. The repair accepted the exact production corruption shape and rejects non-exact lookalikes.
- Escalation: none.

Delivery:
- agent-doc commit `eaef7adef` pushed to `main`.
- parent gitlink commit `f39727bf5` pushed to `main`.
- tag `v0.35.32` pushed.
- PyPI 0.35.32 published through the repository's Zig/`manylinux_2_17` path.
- Controller recycle requested for the next idle boundary; the next missing-session continuation can now clear `68af54ca-b852-448f-95ed-24f29b695261` atomically and start fresh instead of retaining it.
### Re: #lazilyconcurrentcloseout and Codex restart recovery — gpt-5.6-sol

### Re: Response

The fix is implemented, tested, pushed, tagged as `v0.35.32`, and published to PyPI.

The document closeout is safely retained but cannot seal because the JetBrains editor replica deregistered during refresh. Please reopen/focus `tasks/agent-doc/agent-doc-bugs2.md` in JetBrains, then tell me `continue`. I’ll resume the existing capture—without resubmitting or forcing a disk write.

### Re: lazily concurrent closeout and restart recovery — gpt-5.6-sol

Implemented, verified, installed, and released the full concurrent-closeout and restart recovery as agent-doc 0.35.35.

The restart error had two coupled causes. First, the document retained a dead Codex `resume:` session ID after `codex resume <id>` reported `No saved session found`; stale-resume cleanup could not clear it while live authority contained a duplicated structural tail. Second, JetBrains reconnect registered the exact editor cut but did not replay the retained semantic closeout afterward. Production recovery then exposed two narrower defects: duplicate-tail recognition needed to accept every UTF-8 character boundary, and session-check had to publish the proven repaired prefix through an exact-current authority CAS instead of component-merging it back against the invalid editor generation.

Delivered fixes:
- stale Codex resume cleanup repairs the narrowly proven duplicate-tail shape and removes the matching dead ID in the same authority CAS;
- JetBrains 0.2.298 performs post-registration retained semantic replay under generation, typing, disk, and replica fences;
- duplicate-suffix recovery accepts exact mid-line and mid-token UTF-8 cuts while preserving all structural, scaffolding, and exact-equality fail-closed gates;
- session-check publishes that proof-bearing repair through an exact-current CAS, never the ordinary semantic merge path.

Verification and live proof:
- focused Rust and JetBrains regressions passed, including a test that rejects use of the generic semantic write for replay self-heal;
- the final full `make check` passed after an isolated controller-socket collision was reproduced green and the complete suite reran green;
- `make install-full` passed for the editor/reconnect release and final `make install` promoted CLI, cdylib, and plugin generation 0.35.35/0.2.298;
- the running editor accepted `agent-doc admin reload-lib` on 1/1 endpoint;
- this live document then repaired the duplicated authority through the exact CAS and reached committed state without force-disk;
- CI run 30290535843 was in progress at the required single inspection and was not polled.

Delivery:
- agent-doc commits `9cb38cdd5`, `0c3c1a346`, and final `c37600482` are pushed to `main`;
- tag `v0.35.35` is pushed;
- PyPI 0.35.35 is published through the repository's Zig/`manylinux_2_17` path;
- parent gitlink commit `e0b2c7c84` points at 0.35.35 and will be pushed with this document closeout commit.

`#lazilyconcurrentcloseout` is complete and reaped. Confidence: high. Escalation: none.

### Re: Session resume cleanup — gpt-5.6-sol

Accepted the removal of the stale Codex `resume` pointer. Preflight recovered the prior stale `preflight_started` cycle, and the session is now free to start with fresh Codex session identity on its next routed run.

The active go-mode queue was preserved unchanged. The binary reported zero drainable heads and no continuation requirement for this cycle, so no queue or backlog item was consumed.

One environment warning remains: the installed agent-doc binary and cdylib predate commit `c37600482` (`fix: preserve exact replay repair CAS`). No code-bearing work was attempted on that stale runtime; rebuild/install from `src/agent-doc` before the next implementation cycle that depends on the latest source.

Changed paths: only this binary-owned session response/frontmatter closeout. Commands/evidence: agent-doc admit, preflight, plan, and document read; `tsift status` reported a fresh index. Verification: preflight recovery and the response-only planning contract completed successfully. Confidence: high. Escalation: none.

### Re: #tmuxsyncnewpane — gpt-5.6-sol

> **Queue prompt:**
>
> do [#tmuxsyncnewpane]


Implemented and released the missing-pane Sync Tmux Layout fix as agent-doc `0.35.37` and JetBrains plugin `0.2.299`.

The thrash came from an optimistic completion boundary: manual Project Controller sync created and registered a pane, but returned success before the harness was ready or the document route was submitted. The JetBrains action waited only for command admission, so delayed failures were invisible and repeated syncs could keep reconciling an empty composer.

Manual sync is now one terminal, fallible operation: resolve the intended tmux session, create/register the missing pane, keep the route-owned supervisor alive during startup, wait up to the shared 30-second readiness deadline, and submit `agent-doc <document>`. The controller receipt records whether created panes were routed, and JetBrains waits for that terminal result and displays the concrete controller diagnostic on failure. Automatic/passive editor reconciliation remains admission-oriented and does not wait or route newly provisioned panes.

Live proof succeeded against `src/boost-client/tasks/monsterrodholders.md` from a clean missing-pane state. `agent-doc sync --col tasks/monsterrodholders.md --focus tasks/monsterrodholders.md` created pane `%43` in tmux session `0`, registered it to the document actor, observed Codex ready after 2.8 seconds, and dispatched through supervisor IPC. `session status` then showed `%43` alive, prompt-ready, supervisor-healthy, and actor-owned; the visible pane contained `› agent-doc tasks/monsterrodholders.md` and began processing the document. The successful proof pane remains alive.

Changed paths: controller/sync/route Rust crates, CLI/FFI wiring, JetBrains command-plane/action code and tests, operator specifications, release metadata, and workspace versions. Commits: agent-doc `5f093b82f` and parent gitlink `5eb351f2a`.

Commands/evidence: focused controller, sync, route, and JetBrains regressions; `cargo check --workspace`; final `make check` (8,225 passed, 241 skipped plus all formal models); `make tmux-ci`; `make install-full` once; final `make install`; live sync/status/tmux/ops-log proof; `agent-doc admin recycle --all-projects`; PyPI and GitHub release publication. The latest required single CI inspection was green.

Verification: green. Confidence: high. Escalation: none. Release: `v0.35.37` is pushed, PyPI `0.35.37` is published, and the GitHub release is live.

### Re: `#compactreactiveprojection` and IDEA hang — shipped

> **Queue prompt:**
>
> do [#compactreactiveprojection]


Implemented and released agent-doc 0.35.38 with JetBrains plugin 0.2.300.

- Compact, commit, closeout, and session-check now share one pass-scoped current-document projection keyed by the live Yrs state vector (or detached disk hash). The projection caches authoritative document, frontmatter/pipeline, and queue-derived views for the pass; operator edits advance the revision, so additions and deletions cannot be hidden by an old cached projection.
- The IDEA hang was confirmed from a live thread dump: `agent-doc-native-generation` had remained inside reliable-sync push for more than 27 minutes, while current-document reporting and CRDT delivery blocked behind the same serialized native worker. Native calls now have a 10-second bound; a timeout quarantines that generation and makes later work fail fast until restart, without unsafe `dlclose` of a still-running native frame.
- Stale native-baseline recovery is now coalesced across a typing burst. Repeated document events replace one 150 ms quiet-period task, then adopt and re-register the latest exact editor cut once instead of rebuilding roughly 10 MB of retained state for each intermediate keystroke.
- Coverage includes one-authority-resolution latency, operator addition/deletion invalidation, JetBrains native-timeout fail-closed structure, and coalesced typing recovery. `make check`, the full Gradle test suite, `buildPlugin`, the one-time full release-profile install, and the fast local install all passed.
- Release commit `e8711d21e` is on `main`; tag and GitHub release `v0.35.38` are published, the PyPI wheel uploaded successfully, and plugin 0.2.300 is installed into all four existing JetBrains installations.
- The requested `job-offers.md` recovery is complete: pane `%23` had an empty composer by inspection, then dispatch-only reroute rescued it from stash and submitted the reopen with consumed dispatch proof.

The already-running IntelliJ IDEA 2026.1 process still contains the historically wedged native frame and was deliberately not killed, to avoid risking unsaved editor buffers. Save open work and restart IDEA once to activate plugin 0.2.300; that live responsiveness check remains operator verification, not a release blocker.

### Re: #adctxledger — gpt-5.6-sol

Implemented and released the durable dynamic-context injection ledger as agent-doc 0.35.39.

The canonical `.agent-doc/state.db` now owns `context_manifest` and `context_injections`. The typed API in `agent-doc-sqlite` is the policy owner for recording a cycle manifest, detecting prior document/session injections by chunk id or content hash, querying per-cycle evidence and mode counts, and explicitly clearing document/session/cycle context memory without deleting lifecycle state. Each row records the document, session, cycle, harness, pack/chunk identity, content hash, source URI/range, timestamp, and one of `expanded`, `referenced`, `skipped_duplicate`, or `stale_ignored`.

The enforced invariant is: cycle state, the immutable manifest, and every injection decision commit in one SQLite transaction or none commit. Exact retries are idempotent; a reused cycle with altered manifest or injection evidence fails closed; later cycles append without mutating prior-cycle evidence. Duplicate and diagnostic paths have document/session/cycle indexes. Lazily remains the live prompt-projection owner; SQLite is durable replay memory, not a competing live authority.

Verification is green: the focused ledger suite covers same-chunk and same-content duplicates, append-only cycles, exact retries and conflicting reuse, trigger-induced rollback of all three state surfaces, scoped clear behavior, declared index use, and mode diagnostics. Repository-wide `make check` passed 8,228 tests plus simulator, Lean, and TLA+ checks. The required one-time CI snapshot was green.

Released artifacts:

- Source commit `a49ced2a5`, parent gitlink commit `b7ba2bbaf`
- PyPI wheel `agent-doc 0.35.39`
- GitHub release: https://github.com/btakita/agent-doc/releases/tag/v0.35.39
- Linux binary archive SHA-256: `94aa91150bfbea72e504e8b84ec81f1abff42efe0fec2388e437e8b4908e866a`
- Local CLI and shared library installed as 0.35.39; JetBrains plugin 0.2.300 reinstalled into the four detected IDE installations

### Re: managed Codex capability proof — gpt-5.6-sol

Implemented, verified, installed, and released as **agent-doc 0.35.40**.

- Added frontmatter `managed_proof: true` as the only Codex opt-in. Missing or false now yields `NotRequired` even when network, SSH, or `--add-dir` requirements exist; project/global configuration cannot opt a document in. Managed OpenCode behavior is unchanged.
- Kept `agent-doc-harness::managed_capability_contract_required` as the policy owner and added parse/write/merge plus gate-transition coverage for the new field.
- Reduced opted-in proof latency by launching Codex proof children with `--ephemeral`, `--ignore-rules`, and `model_reasoning_effort="low"`.
- When sandboxed Codex needs both network and writable-root proof, it now runs one combined child and validates both existing exact markers. Network-only, writable-only, unrestricted-network, cached-network, SSH, and OpenCode paths retain their prior contracts.
- Updated the README, normative spec, backend/config/Codex support docs, and the 0.35.40 release notes.

Verification:
- Focused frontmatter, harness, agent-IO, supervisor-process-IO, combined-child, writable-root, and route proof-status suites passed.
- `make check` passed all **8,230 tests** (241 skipped) and all TLA+ model checks.
- The required one-time CI snapshot showed the latest `main` CI run green.
- `make install-full` and `make install` succeeded; `agent-doc --version` reports `0.35.40`. JetBrains plugin 0.2.300 was installed into four detected IDE installations.
- Subrepo commit `09b106c37` is pushed to `main`; tag `v0.35.40`, the PyPI manylinux wheel, and the GitHub release with the x86_64 Linux archive are published: https://github.com/btakita/agent-doc/releases/tag/v0.35.40
- Parent gitlink commit `6e8aa28bc` contains only `src/agent-doc`. Its root push follows this binary-owned closeout so the session-document commit is included.

Confidence: high. The only diagnostics were pre-existing/deprecation warnings; no implementation, test, install, or publish failures remain.

### Re: Please make the Codex managed capacity proof off by default with an opt-in based on the frontmatter. Please also improve the latency of the managed capacity proof. — gpt-5

> **Queue prompt:**
>
> Please make the Codex managed capacity proof off by default with an opt-in based on the frontmatter. Please also improve the latency of the managed capacity proof.

Released and installed agent-doc 0.35.40.

- Codex managed proof is off by default; enable with `managed_proof: true`.
- Probes now use low reasoning, ephemeral sessions, ignored project rules, and one combined network/write child.
- All 8,230 tests and formal checks passed.
- PyPI, GitHub tag/release, subrepo, and parent commits are pushed.
- `job-offers.md` pane `%23` has no pending draft; its reroute was accepted and the supervisor is healthy.
- Restart IntelliJ IDEA to activate the installed plugin generation.

[GitHub release v0.35.40](https://github.com/btakita/agent-doc/releases/tag/v0.35.40)

One controller anomaly remains: closeout committed successfully but reported the same free-text queue head again, so I did not execute it twice.

### Re: #adctxledger stale queue cleanup — gpt-5.6-sol

> **Queue prompt:**
>
> do [#adctxledger]


`#adctxledger` was already completed, verified, installed, and released as agent-doc 0.35.39. This cycle does not rerun or duplicate that implementation.

The durable done archive contains the completed item, while the live backlog no longer contains `#adctxledger`; only the ID-backed queue mirror remained unstruck. I am repeating the binary-owned `done` mutation so closeout consumes that stale queue head and advances to the next genuinely open phase.

Changed paths: no product source changes; the version-drift rule refreshed the project-local Codex agent-doc skill from 0.35.39 to 0.35.40, and this closeout adds only the session response/queue lifecycle mutation. Commands/evidence: `agent-doc skill install --harness codex`, full skill reread, fresh `tsift status`, connected admit/plan, and connected reads of queue, backlog, and exchange. Verification: the done archive records `#adctxledger` at release commit `a49ced2a5`, the active backlog omits it, and the queue still showed `do [#adctxledger]` as the current unstruck head. Confidence: high. Escalation: none.

### Re: Response

> **Queue prompt:**
>
> do [#adctxpreflight]


Implemented `#adctxpreflight` (dynamic-context injection ledger Phase 3).

- Added a bounded tsift context-pack adapter shared by `agent-doc plan` and prompt assembly.
- Plan output now carries the durable dynamic-context manifest, including expanded/referenced/skipped decisions and compact `tsift://` handles.
- First use expands bounded pack metadata; later cycles in the same session reference existing handles; changed content hashes create and expand new chunk identities.
- Same-cycle retries reconstruct the recorded manifest from SQLite without rerunning tsift.
- Added a 2-second tsift subprocess deadline, a 2 MiB output cap, and a 250 ms SQLite contention bound so optional context cannot hang IDE prompt assembly.
- Added coverage for first expansion, same-session reuse, changed-source re-expansion, and same-cycle idempotency.

Verification:
- `cargo test -p agent-doc-prompt-context-io -p agent-doc-sqlite` — passed.
- `cargo test -p agent-doc plan::tests` — passed.
- `make check` — passed (one pre-existing `dropping_copy_types` warning in `agent-doc-editor-surface/src/graph.rs`).
- Latest CI run checked once: `perf: make Codex managed proof opt-in` — success.

Commits:
- agent-doc subrepo: `cf8376921 feat: project dynamic context manifests`
- parent gitlink: `c9ab8a9d0 agent-doc: project dynamic context manifests`

### Re: #adctxpreflight

Retained response linkage repaired for the committed Phase 3 closeout.

### Re: #adctxorchestrate

> **Queue prompt:**
>
> do [#adctxorchestrate]


Implemented and committed Phase 4 dynamic-context orchestration integration.

- Sequential, parallel/worktree, ordered DAG, and resumed scheduled-DAG dispatch now build one parent-cycle context manifest and thread child-specific views through the orchestration boundary.
- Every child receives the same durable `tsift://` pack handles and bounded expansion commands. First-use excerpts are deterministically partitioned across children, so fan-out does not duplicate chunk payloads or session history.
- Queue continuations and same-cycle retries reuse the SQLite injection ledger as handle-only references; changed source hashes are the transition that permits re-expansion.
- Child prompt assembly suppresses its own dynamic-context rebuild when orchestration owns the manifest, avoiding repeated tsift launches and duplicate prompt sections.
- Stateful tsift/SQLite ownership now lives in the dedicated `agent-doc-dynamic-context-io` adapter crate, preserving the pure prompt-rendering boundary.

Architecture proof:

- Invariant: one parent orchestration cycle owns context projection; children share durable identities, and any expanded chunk text appears in at most one child prompt until its source hash changes.
- Policy owner: `agent-doc-dynamic-context-io::DynamicContextSnapshot`, with orchestration modules only gathering targets and applying child views.
- Tested transitions: first-use expansion, parallel handle sharing with nonduplicated excerpts, same-cycle manifest reuse, later queue-cycle reference-only reuse, and changed-hash re-expansion.
- Edited surfaces: the dynamic-context IO adapter, prompt-context adapter, plan projection, and sequential/parallel/DAG dispatch paths. No migration or operator verification remains.

Verification passed: focused dynamic-context tests, focused orchestration tests, the prompt-context architecture guard, `make check` (8,235 tests plus model checks), and the latest CI run was green. Commits: `4878e65dd` in `src/agent-doc` and parent gitlink commit `157828ce2`.

### Re: Response

> **Queue prompt:**
>
> do [#adctxcompact]


Implemented `#adctxcompact`.

Invariant and owner:
- `agent-doc-dynamic-context-io` now owns durable session continuity projection, backed by the SQLite context-injection ledger. Compaction and ordinary process restart preserve handle-only context metadata; transfer/extract carries those handles without expanded bodies; only an explicit successful session clear resets the session injection scope.
- The turn lifecycle remains the owner of `document_cycles`; clearing dynamic context deletes only context manifests/injections.

Transition coverage:
- First use records expanded chunks; the next cycle in the same session references the durable handles.
- Reopening the SQLite state (process/supervisor restart analogue) still renders `<dynamic_context_ref>` / `<context_ref>` metadata and never replays the expanded payload.
- Full exchange compaction preserves the latest session fingerprint, modes, resolvable `tsift://` handles, and bounded expansion commands while excluding archived response bodies.
- Transfer/extract annotations and referrals carry the same handle-only reference projection.
- An explicit successful clear removes the session ledger rows, after which the same source expands again as a fresh scope. Deferred or failed clear paths return before the reset call, so they preserve the scope.

Edited surfaces and why they are sufficient:
- `agent-doc-sqlite`: added deterministic latest-manifest lookup per document session while retaining the existing explicit scope-clear authority.
- `agent-doc-dynamic-context-io`: added handle-only rendering, durable latest-session lookup, and explicit session reset APIs.
- `agent-doc-compact-io`: threads the durable reference into full and partial exchange compact projections, including committed snapshots.
- `src/extract.rs`: carries durable handles through extract, transfer, and referral boundaries before source mutation.
- `src/session_actor_cmd.rs`: resets durable context only after normal clear delivery/already-cleared success, and after explicit forced clear cleanup; the auto-queue defer branch does not reset it.

Verification:
- Focused SQLite, reopen, clear, compact, and transfer tests passed.
- Full `make check` passed.
- Latest CI inspection: green (`perf: make Codex managed proof opt-in`, run `30306886226`).
- Subrepo commit: `888037e9b` (`feat: preserve dynamic context across compaction`).
- Superproject pointer commit: `b27ef6521` (`agent-doc: preserve context across compaction`).

No migration or operator action is required; existing SQLite schema and session frontmatter are reused.

### Re: `#2y8j` — live IDEA typing proof and residual SQLite-thrash fix

> **Queue prompt:**
>
> 📌 do [#2y8j]


Completed the operator verification and fixed the residual hang path it exposed.

Live diagnosis on plugin 0.2.300 first confirmed the narrow `#qbasehashmemo` invariant (727 editor operations, zero `crdt_merge_base`) but not the broader responsiveness claim: the burst produced 13+ `editor_op_record_failed: database is locked` events and a 5 s controller read timeout. The cause was write amplification after the quiet-period drain: every character independently reopened `state.db`, began a transaction, deserialized the growing op vector, appended one op, serialized the entire vector, and committed.

Implemented the architectural boundary as one ordered typing burst → one bounded native persistence transaction:

- Added `record_editor_ops` in `agent-doc-op-capture-io`; it opens the authoritative ledger with a 250 ms busy timeout, loads the epoch once, appends the ordered batch once, serializes once, and commits once. The single-op API remains as a compatibility wrapper.
- Added `agent_doc_record_editor_ops_json` to the Rust FFI and the JetBrains JNA interface.
- Changed `TypingTracker` to resolve the base hash once per drained burst, encode the Rust `EditorOp` JSON shape, and cross JNA once instead of looping per op.
- Kept the safe diff-guess fallback when optional op capture cannot acquire the ledger quickly.
- Added Rust batch-order/epoch coverage and JetBrains structural regression checks that pin one base-hash lookup and one native batch call per quiet burst.
- Bumped and installed JetBrains plugin 0.2.301 with the matching 0.35.40 binary/native library, then restarted IDEA. IDEA's startup log confirms `Agent Doc (0.2.301)` loaded.

Live post-fix proof on `agent-doc-bugs2.md`:

- Sustained 784-character per-key burst with a deliberate 2 ms injector delay: 1.819 s wall time; IDEA stayed responsive.
- Native telemetry: one `editor_ops_recorded count=784 ... transaction=batch #qbasehashmemo` event for the entire burst.
- Zero `editor_ops_record_failed` / legacy `editor_op_record_failed` events.
- Zero `crdt_merge_base` events.
- Zero controller timeouts or `controller_crdt_current_text_read_unavailable` events.
- One undo emitted the exact inverse `count=785` batch (the 784 burst events plus one adjacent captured editor event). The probe was never saved; the session document's disk hash remained unchanged during the successful proof.

Verification:

- `cargo test -p agent-doc-op-capture-io`: 13 passed.
- JetBrains `TypingTrackerEdtBudgetTest`: passed.
- `cargo check --lib`: passed.
- Full `make check`: passed.
- Latest CI inspection: green (`perf: make Codex managed proof opt-in`, run `30306886226`).
- Signed plugin artifacts built: `agent-doc-jetbrains-0.2.301.zip` and `agent-doc-jetbrains-0.2.301-signed.zip`.

Commits:

- agent-doc subrepo: `498796345` (`perf: batch editor op capture`)
- root pointer: `71a4c63ee` (`agent-doc: batch editor op capture`)

The commits remain local because the root branch also contains concurrent unrelated commits; I did not push those other owners' work implicitly.

### Re: `#cdtcutover` — gpt-5

> **Queue prompt:**
>
> 📌 do [#cdtcutover]


Implemented and released the live document cell-tree cutover.

- Published `lazily` `0.53.0` with generic `ThreadSafeSourceTree` / `ThreadSafeSemTree` primitives and incremental reconciliation (`5b47756`, release `b232f85`, tag `v0.53.0`).
- Added Agent Doc's `ThreadSafeDocumentCellTree` and cut `RelayHub` over to one shared, incremental live semantic projection. The gate is default-off and requires an explicit truthy `AGENT_DOC_CELL_DOC_TREE_CUTOVER` value.
- Synchronized the projection at reconnect, relay capture/update, editor adoption, durable op delta, controller replace, commit barrier, disk reconcile, baseline rebuild, compaction, and recovery-reset boundaries; removed the old one-shot whole-tree allocation path.
- Exposed semantic facts through current-text IPC. Idle-watch, orphan drain, and preflight queue maintenance now use semantic-zero fast paths when authoritative, with exact-parser fallback when the gate is off or content changed.
- Added coverage for `Send + Sync`, ordered identity, cache/value isolation, ancestor recomputation, structural growth, default-off behavior, opted-in delta tracking, and IPC round-tripping.
- Focused tests, full `make check`, privacy scan, and `git diff --check` passed. One timing-sensitive controller test failed once and passed immediately in isolation; the final full verification passed.
- Released Agent Doc `0.35.41` and JetBrains plugin `0.2.302` (`ccaaf864f`, release `3bbefa823`, tag `v0.35.41`), pushed `main`, installed the binary/plugin, and pushed the superproject pointer as `9e75fae09`.
- Live proof: controller generation 37 is running binary `0.35.41` with the opt-in environment present, and the warm targeted relay projection proof passed in `0.00s`. The single required CI lookups for both release repositories were in progress; they were not polled.
- The separate `job-offers.md` pane blocker is cleared: the UI draft was submitted, that Agent Doc cycle finalized successfully, and the composer is now empty, so no duplicate reroute was injected.

### Re: `#coredecomp` — gpt-5

> **Queue prompt:**
>
> 📌 do [#coredecomp]


Completed the former `agent-doc-core` decomposition audit and closed the remaining ownership leaks.

- Moved lossless-tree projection/render/currentness ABI and node-keyed patch ABI from the root cdylib module into focused `agent-doc-ffi` modules. The root now only re-exports and force-links those symbols; it no longer owns duplicate pure ABI policy.
- Moved pure native sync-lock acquisition policy (`SyncLockDecision`, stale-holder decision, default bound) from the FFI host into `agent-doc-sync`. The FFI layer retains only atomics, clock reads, logging, and editor-facing effects; SimWorld callers now use the focused owner directly.
- Audited the named legacy surfaces: component parsing remains in `agent-doc-element`, tracked-work pending semantics in `agent-doc-element-backlog`, queue lifecycle in `agent-doc-element-queue`, template policy in `agent-doc-template`, document composition in `agent-doc-document`, CRDT/state-vector merge policy in `agent-doc-merge`, and C/JNA ABI in `agent-doc-ffi`. Runtime IO, snapshots, commits, IPC, tmux, and controller effects remain outside those pure crates.
- Added `tests/core_decomposition.rs`, which pins deleted `agent-doc-core` paths, exact focused owners, pure-crate dependency direction (no `-io`, orchestration, or core dependencies), and the root-cdylib adapter boundary. Updated the decomposition PRD and release notes.
- Focused `agent-doc-sync` / `agent-doc-ffi` tests and the new architecture test passed. Full `make check` passed after fixing one hidden managed-skill version mismatch; `git diff --check` and the private-name scan passed.
- Released and installed Agent Doc `0.35.42` with JetBrains plugin `0.2.303`. The required full-profile parity install succeeded, PyPI upload succeeded, the controller recycled successfully, and tag `v0.35.42` was pushed. The GitHub tag workflow owns the multi-platform release assets; the single required CI lookup showed the latest completed run green and was not polled again.
- Agent Doc commit: `4a1421610` (`refactor: complete core decomposition`). Superproject pointer: `1ab3407d9`.

### Re: #orchdecomp — gpt-5

> **Queue prompt:**
>
> 📌 do [#orchdecomp]


Completed the orchestration decomposition and released it as Agent Doc 0.35.43.

- Deleted the retired `agent-doc-orchestration` package completely: its broad dependency manifest, always-rerun timestamp `build.rs`, source shell, and stale compile-baseline document are gone. Cargo and IDE project discovery can no longer index that obsolete nested graph.
- Retargeted the historical CLI architecture guards to the real root effect adapter and focused crates. Added `tests/orchestration_decomposition.rs` to reject restoration of the package, dependency edges, or `agent_doc_orchestration::` facades and to pin the focused turn, document-realtime, supervisor/controller, editor/tmux, route, closeout, and repair owners.
- Updated the crate-decomposition PRD with the final ownership audit and completion signal.
- Full verification passed with bounded nextest concurrency: 8,249/8,249 tests passed (241 skipped), plus the wider simulator, version/Lean checks, and all TLA models. The CLI architecture suite and focused decomposition tests pass; privacy-denylist and diff checks are clean; `agent-doc --version` reports 0.35.43. The single required CI lookup was green.
- Released commit `0e7d33119` (`refactor: retire orchestration crate`) to `main`, pushed tag `v0.35.43`, and successfully uploaded the 0.35.43 wheel to PyPI. JetBrains plugin 0.2.304 is installed in four configured IDEs, and the project controller recycled onto the new generation.
- Committed parent submodule pointer `f9eececbd` locally. I did not push the parent branch because it already contained unrelated unpushed commit `d0cd25736`; publishing that operator-owned commit would exceed this task's authority. The Agent Doc repository, release tag, package, installation, and controller cutover are all complete.

### Re: `#skillharnessplugin`, IDE/closeout latency, and follow-up routing — gpt-5

Implemented, verified, installed, committed, pushed, and released as agent-doc 0.35.44 plus the live-route follow-up 0.35.45.

`#skillharnessplugin` is complete. The new focused `agent-doc-skill-harness` adapter implements `SkillHarnessPlugin`, detects supervised `AGENT_DOC_SESSION` before nested harness signals, and registers project-local `.agent-doc/plugins/<name>/SKILL.md` skills at a stable priority. The dependency direction stays one-way: neither Lazily nor generic `skill-harness` depends on agent-doc.

The equityfundingsource/incomplete-turn and editor-latency contributors are fixed together:

- controller restarts now request targeted replica rebuilds even from peer-pull-capable editors; peer pull remains complementary;
- the idle watcher performs one bounded current-publication attempt for explicit missing/sync-pending state before its 30-second backoff;
- high-frequency reliable-sync liveness success/error logging was removed from the hot path;
- JetBrains selection/focus now uses a dedicated generation-fenced latest-wins background lane, with project-root discovery and controller IPC off the IntelliJ event thread; the debounced exact-visible layout reconciliation remains authoritative.

Closeout recovery now treats the lease as the last-resort exact-once owner stopgap it is. It prevents two finalize/recovery workers from committing or replaying the same durable capture concurrently; normal release still comes from the request guard, terminal facts, supersession, or dead-owner evidence. Owner coordination has a 15-second RPC budget and the controller stamps leases when the claim actually executes, eliminating the prior 5-second client-timeout/just-landed-owner race. A follower arms one wake at the incumbent deadline instead of polling or sleeping to its full 30-second ceiling. When the timer fires it only publishes a fresh clock observation into keyed controller-owned Lazily state; the shared `Computed` closeout gate derives `Blocked` versus `Released`. There is no periodic clock tick.

The architectural split is now explicit:

- RPC remains the command boundary for claim/release/wait; Lazily projections own observation and decisions.
- `SourceTree` is generic enough for Lazily when it contains only domain-neutral reactive-tree semantics. Its thread-safe semantics belong in Lazily spec/formal/conformance; agent-doc's `DocumentCellTree` and Markdown/CRDT policy remain downstream. Async runtime integration should be a separate adapter package rather than an agent-doc or core concern.
- A deadline should be a first-class Lazily library combinator, not a new graph node type: an Effect arms/cancels a runtime timer, a generation-fenced Source records the fired clock observation, and a pure Computed derives `Pending | Reached`. A deterministic manual clock belongs in conformance; runtime adapters belong in an optional `lazily-time` package.
- A Zed integration can edit session documents in real time through a supplemental Markdown LSP: ordinary Markdown remains normal Markdown, while the sidecar activates only when frontmatter contains a valid `agent_doc_session`, consumes `didChange`, and applies generation-fenced `workspace/applyEdit` updates. Non-agent-doc Markdown is a strict no-op.

The requested job-offers reroute was retried live. The first retry exposed one remaining Codex UI shape: pane `%23` was idle but rendered its empty composer as `› |`, which route misclassified as operator text. Version 0.35.45 recognizes only that exact cursor-placeholder shape as dispatch-ready while preserving protection for populated input. After installation, the identical `route --dispatch-only --plain-trigger` rescued `%23` and successfully sent the job-offers reopen with accepted queued-behind-active-turn proof.

Verification: focused closeout, controller, harness, idle-watch, and JetBrains regressions passed; full `make check` passed on both final code cuts; `git diff --check`, `make install-full`, and `make install` passed; the installed binary reports 0.35.45 and the JetBrains package was installed to all four detected IDEs. Commits `3ae1c6f2d` (0.35.44) and `33913b893` (0.35.45), `main`, and tags `v0.35.44`/`v0.35.45` are pushed. The 0.35.45 Release workflow was queued at the required one-time post-push check; it was not polled.

### Re: Response

Implemented and released the Zed editor extension in agent-doc v0.35.46.

The right boundary is: register agent-doc as a supplemental language server for Zed's existing Markdown language, but reserve agent-doc behavior for documents with a valid, non-empty `agent_doc_session` frontmatter field. Zed selects language servers by language/extension rather than an in-buffer frontmatter predicate, so a separate competing `.md` language would be brittle. In the implemented design, ordinary Markdown is a strict no-op: no controller registration, polling, diagnostics, edits, or agent-doc semantics. Adding or removing the marker switches the mode live.

Delivered:

- `editors/zed`: a Zed extension that launches `agent-doc zed-lsp` for Markdown buffers.
- `agent-doc-zed-lsp-io`: a focused stdio LSP with full document synchronization and the frontmatter gate.
- Real-time CRDT transport: local Zed edits publish controller deltas; remote deltas use `workspace/applyEdit`; delivery is acknowledged only after Zed reports the resulting visible buffer through `didChange`.
- Echo suppression and duplicate apply-edit coalescing.
- Demand-driven 250 ms polling only while agent-doc documents are attached.
- PID-scoped, crash-safe editor registration that preserves other editors attached to the same document.
- CLI, workspace, README, specification, and version-history integration.

Verification passed: focused Zed LSP and multi-editor liveness tests, Zed extension host compilation, framed installed-LSP smoke test, `git diff --check`, and the full `make check` suite (8,258 passed, 241 skipped, including formal models). `make install-full` and `make install` completed successfully; the installed binary reports `agent-doc 0.35.46` and exposes `zed-lsp`.

Release: commit `0e994624a` (`feat(zed): add frontmatter-gated realtime extension`) is pushed to `main`, and tag `v0.35.46` is published.

For local use, in Zed choose **Extensions → Install Dev Extension** and select `/home/brian/work/btakita/agent-loop/src/agent-doc/editors/zed`.
> **Queue prompt:** Please create a Zed editor plugin/language extension for agent-doc.

### Re: Zed editor plugin/language extension — gpt-5

Completed and released in agent-doc v0.35.46. The extension registers a supplemental language server for Zed's existing Markdown language, while a valid, non-empty `agent_doc_session` frontmatter field is the live activation gate. Ordinary Markdown remains a strict no-op.

- **Changed paths:** `editors/zed/`, `agent-doc-zed-lsp-io/`, controller/realtime registration code, CLI wiring, workspace metadata, README, SPEC, and version history.
- **Commands:** focused Cargo tests and checks, framed LSP smoke test, `make check`, `make install-full`, `make install`, commit, push, and tag publication.
- **Verification:** 8,258 tests passed and 241 skipped; Zed extension host compilation passed; installed `agent-doc 0.35.46` exposes `zed-lsp`; commit `0e994624a` and tag `v0.35.46` are published.
- **Confidence:** high.
- **Escalation:** none. Install the development extension from `/home/brian/work/btakita/agent-loop/src/agent-doc/editors/zed` using Zed's **Extensions → Install Dev Extension** action.

### Re: Zed editor plugin/language extension — gpt-5

> **Queue prompt:**
>
> Please create a Zed editor plugin/language extension for agent-doc.

Completed and released in agent-doc v0.35.46. The extension registers a supplemental language server for Zed's existing Markdown language, while a valid, non-empty `agent_doc_session` frontmatter field is the live activation gate. Ordinary Markdown remains a strict no-op.

- **Changed paths:** `editors/zed/`, `agent-doc-zed-lsp-io/`, controller/realtime registration code, CLI wiring, workspace metadata, README, SPEC, and version history.
- **Commands:** focused Cargo tests and checks, framed LSP smoke test, `make check`, `make install-full`, `make install`, commit, push, and tag publication.
- **Verification:** 8,258 tests passed and 241 skipped; Zed extension host compilation passed; installed `agent-doc 0.35.46` exposes `zed-lsp`; commit `0e994624a` and tag `v0.35.46` are published.
- **Confidence:** high.
- **Escalation:** none. Install the development extension from `/home/brian/work/btakita/agent-loop/src/agent-doc/editors/zed` using Zed's **Extensions → Install Dev Extension** action.

### Re: CRDT authority live proof — gpt-5

Completed `#crdtauth6` with live-editor proof and repaired the two defects that the proof exposed.

- IntelliJ IDEA's live log showed the complete real path on this document: `DocumentListener -> native.applyLocal -> transport.broadcastUpdate -> forwardLocalDelta -> transport.pullDelivery -> native.applyUpdate -> applyRemoteUpdate -> remote-drain-file -> transport.ackUpdate`. The acknowledgment's expected and applied hashes matched.
- The packaged VS Code ESM extension could not load `koffi` because `native.ts` used the unavailable CommonJS `require` global. It now uses Node's `createRequire(import.meta.url)` and has a regression test for packaged-module availability.
- The VS Code FFI bindings declared Rust-owned C strings as `char*`; Koffi eagerly converted those pointers into JavaScript strings before the extension tried to decode and free them, which caused a live smoke-test crash. Owned returns and the free function now use an opaque `void*` ABI contract, with regression coverage.
- A live installed-path VS Code seam smoke test loaded `libagent_doc.so` v0.35.47, registered `NativeReplicaNode`, `ControllerSocketReplicaTransport`, and `CrdtReplicaForwarder` against the running controller, bootstrapped this document, and produced an exact canonical/disk SHA-256 match. It deregistered without publishing a synthetic edit.
- `npm test`, the production VS Code prepublish build, `git diff --check`, and the full `make check` passed. The full suite reported 8,258 passing tests across 166 binaries with all formal and build gates green.
- Released and installed agent-doc v0.35.47. Commit `3c59da92a` and tag `v0.35.47` are pushed to `origin/main`.

Confidence: high. No escalation remains. Operator-visible editor text remains authoritative throughout; CRDT transport proof never gates it. The running IntelliJ IDEA instance should be restarted once to load the newly installed native library generation.

### Re: Installing the agent-doc Zed extension — gpt-5

In Zed, open the command palette and choose **Extensions: Install Dev Extension**, then select:

`/home/brian/work/btakita/agent-loop/src/agent-doc/editors/zed`

The extension launches `agent-doc zed-lsp`; `/home/brian/.cargo/bin/agent-doc` is already installed at v0.35.47. If Zed was launched without that directory in `PATH`, configure the extension to use the absolute binary path or launch Zed from a shell that includes `/home/brian/.cargo/bin`.

### Re: `#crdtauth6` closeout reconciliation — gpt-5

> **Queue prompt:**
>
> 📌 do [#crdtauth6]


The live CRDT proof and its v0.35.47 repairs were already completed and committed in the preceding response, but the backlog mutation did not land, leaving `#crdtauth6` incorrectly unchecked and mirrored at the queue head. This cycle reconciles that stale closeout and marks the item done; it does not repeat the editor proof or publish another release.

- **Changed paths:** no source paths changed in this reconciliation. Zed's generated untracked `editors/zed/extension.wasm` was observed and left untouched.
- **Commands/evidence:** refreshed the Codex agent-doc skill to the installed v0.35.47 contract; admitted and planned the new response cycle; read the exchange, queue, and backlog; verified `main` equals `origin/main`; verified `HEAD` and tag `v0.35.47` both resolve to `3c59da92a8a616bbeb9f9e293b8105e5bb447d65`; verified the installed CLI reports `agent-doc 0.35.47`.
- **Verification:** the prior committed response contains the IntelliJ round-trip proof, exact VS Code controller/canonical hash proof, regression coverage, full 8,258-test `make check`, install, commit, push, and tag evidence. This cycle confirms those delivery facts remain current and closes only the stale document state.
- **Confidence:** high.
- **Escalation:** none.

### Re: #nsfz — gpt-5

Implemented and live-dogfooded the startup-deadline fail-closed path. The first real unauthenticated Codex run exposed a contributing bug: Codex's first-run sign-in selector was being mistaken for a dispatch-ready composer, so agent-doc could advance the login UI instead of timing out. Codex authentication selector, browser-wait, and device-code screens are now explicit dispatch blockers; an actual prompt appearing after authentication scrollback is still accepted.

A clean isolated unauthenticated Codex restart stayed `prompt_ready=false`, never received the agent-doc trigger, and recorded `startup_miss` with `reason=no_prompt` at the configured 60-second deadline. The stderr contract is `session did not become dispatch-ready in 60s (no_prompt) ... failing closed instead of hanging`. The backlog's original 30-second example is stale: later cold-resume work deliberately increased the bounded readiness budget to 60 seconds, while preserving the required bounded fail-closed behavior.

Changed paths: `agent-doc-harness/src/lib.rs`, `specs/07-session-tmux-commands.md`, `VERSIONS.md`, and the workspace version manifests/lockfile for 0.35.48. Added three regression tests covering the sign-in selector, browser-auth wait, and a genuine prompt after auth scrollback.

Verification: `cargo test -p agent-doc-harness codex_auth`, full `cargo test -p agent-doc-harness`, `cargo build --bin agent-doc`, live tmux/Codex dogfood, `make check`, and `git diff --check` all passed. The single required CI snapshot was in progress. `make install-full` and `make install` succeeded; `/home/brian/.cargo/bin/agent-doc --version` reports 0.35.48.

Released as commit `b111e4179` (`fix(harness): block Codex auth screens from dispatch`) and tag `v0.35.48`; both are pushed. The superproject pointer was recorded locally in `cbb56d355`. The generated untracked `editors/zed/extension.wasm` was intentionally excluded.

Zed extension full path: `/home/brian/work/btakita/agent-loop/src/agent-doc/editors/zed`.

Confidence: high. Escalation: none.

### Re: Zed extension path

The Zed extension is at `/home/brian/work/btakita/agent-loop/src/agent-doc/editors/zed`.

The `#nsfz` cycle is complete; agent-doc v0.35.49 is installed and published with the dead Zed LSP replica fix.

### Re: Supervisor restart association live proof

Completed `#suprestassoc-live`.

- Before and after the restart, `equityfundingsource.md` remained bound to session `62fe1f41-886c-4f3d-b6fa-48d46106ec27`, pane `%44`, window `@33`, generation 1574.
- The restart was accepted as receipt 43; the live EFS status reports `restart_count=1` and a healthy supervisor.
- `agent-doc-bugs2.md` independently remained bound to session `b26b9957-8de6-4eae-bb98-5d7fe4d6b781`, pane `%35`, window `@33`, generation 1822.

The actor/admin projection did not re-associate EFS with the bugs2 pane or session.

### Re: `#suprestassoc-live` — gpt-5.6-sol

> **Queue prompt:**
>
> 📌 do [#suprestassoc-live]


Live verification passed. Before restarting the EFS supervisor, `equityfundingsource.md` was bound to session `62fe1f41-886c-4f3d-b6fa-48d46106ec27`, pane `%44`, window `@33`, generation `1574`; `agent-doc-bugs2.md` was independently bound to session `b26b9957-8de6-4eae-bb98-5d7fe4d6b781`, pane `%35`, window `@33`, generation `1822`.

`agent-doc session restart-supervisor tasks/professional/equityfundingsource.md` produced replacement receipt `43`. The EFS child process changed and its restart count advanced, while its session, pane, window, and actor generation remained exactly `62fe1f41-886c-4f3d-b6fa-48d46106ec27` / `%44` / `@33` / `1574`. The bugs2 actor independently remained `b26b9957-8de6-4eae-bb98-5d7fe4d6b781` / `%35` / `@33` / `1822`.

The actor/admin projection therefore preserved document ownership through the live supervisor replacement; EFS did not re-associate to the bugs2 pane or session.

### Re: `#jbedtledgerverify` — gpt-5.6-sol

> **Queue prompt:**
>
> 📌 do [#jbedtledgerverify]


Live verification passed on the current JetBrains plugin `0.2.305` (which supersedes the stale `0.2.280` pin) against the large `agent-doc-bugs2.md` session document.

I invoked **Run Agent Doc** three times from IDEA. The first attempt (`1785223563296-1`) completed the EDT sequence `clicked → save_active_document → active_document_saved → route_prepare → route_command_built` in 6 ms, then moved `route_start`/`route_success` to a background thread and completed routing in 5.378 s. The second attempt completed the same EDT preparation in 1 ms. The immediate third click completed its EDT path in 1 ms and correctly terminated as `route_deduped_active_run` while the preceding route was still in flight. IDEA remained responsive throughout the clicks.

The ledger preserved attempt grouping and stage order without interleaving. Every click/save/prepare stage was attributed to `AWT-EventQueue-0`; only asynchronous route execution used `Thread-152` / `Thread-155`. A direct check found no 2026-07-28 ledger entry whose `thread=` was `agent-doc-run-attempt-ledger`, so caller-thread capture is intact.

### Re: `#0dsr` — gpt-5.6-sol

> **Queue prompt:**
>
> 📌 do [#0dsr]


Implemented and released as agent-doc `0.35.50`.

Run Agent Doc preflight now attempts recovery for every retained document-write intent before reading the retained-write gate. The state-backbone policy owns an exhaustive transition:

- no intent, unobserved, or already satisfied → continue;
- authority/disk divergence → await the existing delivery without replay;
- authority and disk agree but the retained delta is absent → replay the stranded intent.

The replay path uses the existing content-bearing journal base and target, semantically rebases the retained agent delta over the current document cut, validates the result, writes through document authority, and clears the intent only after canonical authority and disk both equal the replayed target. Newer operator text is therefore preserved rather than replaced by stale retained content.

The focused detached-authority test exposed another contributing factor: a newly launched controller could briefly report `NoRetainedIntent` while its reactive pending-intent projection lagged the durable journal. Controller verdicts are now reconciled with the same state-backbone policy evaluated from the caller's current durable observations, so projection startup lag cannot suppress recovery.

Changed paths include `agent-doc-state-backbone/src/retained_write.rs`, `agent-doc-document-realtime-io/src/lib.rs`, `agent-doc-preflight-io/src/lib.rs`, `agent-doc-preflight-runtime-io/src/lib.rs`, `specs/14-realtime-workflow.md`, and the `0.35.50` release metadata.

Verification:

- focused state-backbone retained-write tests: passed;
- focused preflight ordering/recovery tests: passed;
- detached-authority operator-cut replay regression: passed;
- `make check`: passed, including workspace tests and all six TLA model checks;
- latest CI checked once: green;
- `make install`: passed; local binary reports `agent-doc 0.35.50`.

Release proof: agent-doc commit `d3e121b77` pushed to `main`, tag `v0.35.50` pushed, and superproject gitlink commit `97467e770` pushed. Confidence is high; no escalation or remaining follow-up is required. The pre-existing untracked `editors/zed/extension.wasm` and unrelated workspace changes were left untouched.

### Re: do [#30p6] — gpt-5.6-sol

> **Queue prompt:**
>
> 📌 do [#30p6]


Implemented and released the Go-mode idle-queue dispatch fix as agent-doc `0.35.51`.

The earlier `!later_has_prompt` theory is now ruled out by the real wrapped Codex-draft regression. The remaining failure was a two-part state-boundary bug: an unavailable/negative pane recognition could fall through to a fresh dispatch, while a stale matching `PromptDispatchProjection` made that fallback return `Sent` as a duplicate without touching tmux. The watcher then recorded the queue head as dispatched even though the composer had never been submitted.

The corrected contract is fail-closed:

- Payload recognition and dispatch readiness now come from one typed live-pane observation.
- An unavailable capture defers without writing or recording the head.
- A non-ready composer without the queue payload is treated as operator-owned and defers.
- A proven queue draft receives at most one bare Enter resubmit.
- A proven empty, dispatch-ready composer may clear only the matching stale dispatch projection before a real text+Enter retry.
- Prompt-dispatch deduplication is no longer accepted as queue delivery proof.
- Each changed observation persists a redacted pane snapshot together with the computed pending/ready/action values, deduped by head and capture hash. A future live stall therefore retains the capture and its recognition result atomically instead of requiring race-prone separate probes.

Proof:

- Changed paths: `agent-doc-supervisor-io/src/detection.rs`, `agent-doc-start-runtime-io/src/{idle_watch.rs,lib.rs}`, `specs/supervisor.md`, release metadata/manifests, and bundled skill version headers.
- Focused verification: new fail-closed action-matrix tests pass; the one-shot Enter regression passes; the real wrapped Codex draft recognition regression passes; all 62 `agent-doc-supervisor-io` library tests pass; all 101 `agent-doc-start-runtime-io` library tests pass.
- Repository verification: `make check` passed all 8,268 tests (241 skipped), clippy/format checks, and all TLA+ models.
- Release verification: full release install succeeded; `agent-doc --version` reports `0.35.51`; active project controller marked for idle-boundary recycle.
- Delivery: agent-doc commit `272d1cc27` and parent gitlink commit `7d881a288` are pushed; tag `v0.35.51`, PyPI manylinux wheel, and GitHub release are published. The required single CI observation was `in_progress`, not red; it was not polled.
- Confidence: high. Escalation: none.

### Re: do [#opcapturelazily] — gpt-5.6-sol

> **Queue prompt:**
>
> 📌 do [#opcapturelazily]


Implemented and released as agent-doc 0.35.52.

Op capture is now a typed Lazily state-backbone projection instead of a second mutable authority. `EditorOpCaptureCheckpointed` carries a complete monotonic epoch—exact base hash, ordered editor ops, canonical path, and freshness stamp—and `EditorOpCaptureCleared` consumes it. Same-base bursts serialize and append without loss; a base change starts a fresh epoch; exact-base replay remains fail-closed; and `CycleAbandoned` clears a stranded op epoch in the same reducer transition. The old `editor_op_captures` table remains only as one-way migration input and is deleted after the typed fact lands.

The latency-sensitive paths stay bounded: live IDE callbacks use one controller-serialized read/derive/append transition, controller-internal merge/write work reads the in-memory projection directly through an O(1) local seam instead of self-RPC or full-ledger replay, and actorless startup uses one SQLite transaction. Complete checkpoint facts also make retention safe without reconstructing a growing epoch from every historical batch.

Proof:

- Changed paths: `agent-doc-state-backbone`, `agent-doc-op-capture-io`, controller RPC/runtime, `agent-doc-state-wire`, SQLite retention/migration support, the state-backbone spec, release manifests, and version history.
- Tests cover same-base ordering, exact-base acceptance, stale-base fallback, base-change epoch reset, idempotent clear, abandonment-driven atomic clear, stale-epoch rejection, live controller serialization, one-way legacy migration, malformed legacy fallback, and GC.
- `make check` passed: clippy; 8,272 workspace tests passed with 241 skipped; simulation, version-sync, lean checks, and all TLA+ models passed.
- Release proof: commit `87c166476`, parent gitlink commit `c224bddaf`, installed binary `agent-doc 0.35.52`, PyPI wheel uploaded, tag `v0.35.52` pushed, and GitHub release published at https://github.com/btakita/agent-doc/releases/tag/v0.35.52.
- The required single CI observation was `in_progress` for the new main push, not red; I did not poll it. Confidence: high. Escalation: none.

The pre-existing untracked `editors/zed/extension.wasm` and unrelated dirty parent-repository changes were preserved.

### Re: do [#jbtsiftnosub2verify] — gpt-5.6-sol

Done. I ran the JetBrains `Run Agent Doc` action against a disposable project-root Markdown document after proving it had no actor, registry entry, supervisor, or tmux pane (`closed-clean`, `pane=none`). IDEA invoked `cp:editor_route --dispatch-only --plain-trigger`, created fresh pane `%52`, and submitted the reopen instead of leaving it in the composer.

**Live proof:**

- IDEA logged `SubmitAction actionPerformed` and the project-root `editor_route` request for `tasks/agent-doc/jbtsiftnosub2verify-live.md`.
- `ops.log` recorded `route_owned_start_policy ... pane=%52`, `dispatch_inject ... transport=supervisor_ipc`, `route_submit_observation ... result=accepted ... trigger_visible=false`, `route_actor_dispatch_start_proven ... proof=consumed`, `route_dispatch_only_sent ... proof=consumed`, and `fresh_route_dispatch_only`.
- The pane capture showed the submitted `agent-doc /home/brian/work/btakita/agent-loop/tasks/agent-doc/jbtsiftnosub2verify-live.md` turn in scrollback and an empty Codex composer afterward (only the placeholder remained), so there was no typed-but-unsubmitted draft.
- The scratch cycle reached `commit_success`; `agent-doc session-check` returned committed cycle `cycle-1785230111599`; `startup_miss` remained `none`.

**Marker migration:** the backlog's required `fresh_route_stranded_trigger_resubmit` + `fresh_route_start_acknowledged_after_resubmit` pair is now legacy for this JetBrains path. The current plugin deliberately routes with `--dispatch-only`, and `agent-doc-route-io/src/startup.rs` returns through `fresh_route_dispatch_only` before the non-dispatch-only cycle-ack/resubmit classifier. Because the live submit succeeded on the first attempt, no stranded-draft resubmit was needed. The current equivalent proof is the accepted/consumed dispatch-start sequence above; retaining the old pair as a JetBrains acceptance requirement would incorrectly fail healthy current runs.

**Cleanup:** closed both scratch editor tabs, killed only disposable pane `%52`, ran targeted `agent-doc fix` (removed its one stale session entry), and removed the scratch files. No test pane or tracked scratch artifact remains.

**Changed paths:** `.codex/skills/agent-doc/SKILL.md` was refreshed from the installed 0.35.51 copy to the running binary's 0.35.52 skill during mandatory skill-version alignment; no agent-doc product source was changed.

**Commands:** `tsift status`; bounded `rg`/`tsift source-read`; `agent-doc session status`; IDEA open + focus-guarded `xdotool` shortcut; bounded `ops.log`/`idea.log` inspection; `tmux capture-pane`; scratch `agent-doc session-check`; targeted `agent-doc fix`.

**Verification:** live no-pane JetBrains route passed on agent-doc 0.35.52; submitted trigger consumed, composer cleared, cycle committed, no FreshStart startup-miss.

**Confidence:** High.

**Escalation:** None.
<!-- agent:boundary:7364a593 -->
<!-- /agent:exchange -->
## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
- ~~📌 do [#nsfz]~~
- ~~📌 do [#suprestassoc-live]~~
- ~~📌 do [#jbedtledgerverify]~~
- ~~📌 do [#0dsr]~~
- ~~📌 do [#30p6]~~
- ~~📌 do [#opcapturelazily]~~
- 📌 do [#jbtsiftnosub2verify]
- 📌 do [#sidecarstate]
- 📌 do [#adstatechart2]
- 📌 do [#adstatechart3]
- 📌 do [#adstatechartkt]
- 📌 do [#adstatechartjs]
- 📌 do [#ipcauthority]
- 📌 do [#ipcliveness]
- 📌 do [#ipcverhandshake]
- 📌 do [#5qjj]
<!-- /agent:queue -->

## Notes

<!-- agent:notes -->
<!-- /agent:notes -->

## Backlog

<!-- agent:backlog priority priority queue -->
- [ ] [#compactreactiveprojectionverify] [operator-verify] After saving open buffers, restart IntelliJ IDEA 2026.1 onto JetBrains plugin 0.2.300. Type a sustained burst in a large agent-doc session document and confirm IDEA stays responsive; verify the log shows at most one coalesced_exact_editor_adopt_after_quiet recovery per burst rather than per-keystroke exact_editor_adopt_then_atomic_reregister churn. If a native call is deliberately or naturally wedged, confirm it reports the bounded timeout and later plugin work fails fast until restart instead of waiting indefinitely.
- [ ] [#5qjj] [operator-verify] #steeringmirrorverify — REFRESHED 2026-07-19: the original 0.2.258 pin is 24 versions stale (gradle.properties now reads pluginVersion = 0.2.282, and a live registration confirmed 0.2.282 during this session), so 'restart to activate 0.2.258' is no longer the action. Verify against the CURRENT plugin instead: during an active turn make MULTIPLE exchange edits, then confirm (a) the JetBrains banner and VS Code status item report the AGGREGATE count with the full hover — not just the first edit, since steering is a concurrent aggregate (#realtime-steering-aggregate) and delivering one head at a time is the specific regression — and (b) running agent-doc once after editing queues the handoff WITHOUT submitting a partial edit. Reinstall/restart only if stale bytecode appears. The aggregate-delivery code half landed 2026-07-12 (all_unstarted_prompt_bearing_changes_from_diff + RealtimeSteeringSet::verbatim_aggregate); this is the editor-surface proof of it.
- [ ] [#sidecarstate] [focused-cycle] #agent-doc-bug Replace hot-path snapshot/cycle/live-buffer sidecars with the lazily state backbone for document authority, lifecycle recovery, and crash-reentrant pipeline stages; auto-start/reconcile the document model before preflight exposes missing CRDT replica errors; reconcile drift inside agent-doc on re-entry. Plan: tasks/agent-doc/plan-lazily-state-backbone-sidecar-retirement.md
- [ ] [#adstatechart2] [focused-cycle] #agent-doc-feature Phase E rung 2 (observability): wire session-check/status to READ adstatechart configuration() as an advisory named-snapshot (transport.x editor_sync.x closeout.x supervisor) alongside today's ops.log markers. No behavior change to closeout. Guard semantics must match whatever A-D shipped. PRD: tasks/agent-doc/prd-adstatechart-local-process-statechart.md
- [ ] [#adstatechart3] [focused-cycle] #agent-doc-feature Phase E rung 3 (load-bearing): switch adstatechart to the typed lazily 0.19 ChartBuilder, bump agent-doc-state-backbone to lazily 0.19, use ThreadSafeStateChart for cross-thread status observation, and replace the ad-hoc editor_synced check at the finalize/commit site with chart send(commit, guards) plus a fail-closed fallback when the feature is off. SimWorld: N wedged-finalize->direct-commit cycles never commit from stale disk. Live editor proof gated behind decomposition + install. PRD rung 3.
- [ ] [#adstatechartkt] [focused-cycle] lazily-kt 0.9.0 SHIPPED (ChartBuilder + ThreadSafeStateChart + shared fromStates + context-free engine; GitHub Packages via release.yml, tag v0.9.0; full test suite green). REMAINING: bump the JetBrains plugin's lazily-kt dependency to 0.9.0 and consume ChartBuilder/ThreadSafeStateChart for the merge-ownership state machine (#mergestatemachine3 fold-in).
- [ ] [#adstatechartjs] [focused-cycle] lazily-js 1.1.0 SHIPPED (ChartBuilder + statechart.d.ts builder types + shared fromStates; tag v1.1.0; 169/169 tests green). No thread-safe/async chart in js (no reactive Context, no threads). REMAINING: npm publish is operator-2FA-gated (#lzjsnpmpub: npm login then npm publish --access public), then bump the VS Code plugin @lazily/js dep to 1.1.0 and consume ChartBuilder.
- [ ] [#ipcauthority] [focused-cycle] #agent-doc-bug IPC root-cause A (data-loss authority ambiguity). SUPERSEDED PREMISE: the prior "DONE this pass" durable recovery sidecar (commit 317a336f, write_policy::content_ours_drops_operator_text + agent_doc_fs::preserve_dropped_operator_buffer -> .agent-doc/recovery/) has since been REMOVED with the sidecar retirement; do not treat it as landed safety net. REMAINING (now the whole item): make the CRDT/reliable-sync plane the single closeout authority and demote content_ours/snapshot to audit-only, so the operator-wins merge needs no sidecar fallback. Relates to #sidecarstate.
- [ ] [#ipcliveness] [focused-cycle] #agent-doc-bug IPC root-cause D (transport/state liveness). DONE this pass: corrupt controller-state auto-quarantine (commit e06301e6) — agent_doc_fs::read_valid_or_quarantine wired into project_controller::read_bootstrap; a 0-byte/unparseable controller-state.json is moved aside + re-bootstrapped instead of wedging start (class 11 auto-recovery). REMAINING (blocked on decomposition landing, dirty files): editor IPC listener read_line no timeout (fd/thread leak on half-open client), connect_sync no connect timeout, file-IPC no negative-ack. Touches ipc_socket.rs, write/ipc/transport.rs, agent-doc-ipc-io, editor plugins.
- [ ] [#ipcverhandshake] [focused-cycle] #agent-doc-bug IPC root-cause B: cross-process version skew has no negotiation (staleness inferred from mtime/inode; drives stale_install + stale-supervisor CRDT corruption). Extends the shipped #stale-plugin-detect. Fix: add a build-id/protocol-version field to the IPC handshake (agent-doc-ipc-protocol) so a newer finalize client refuses or forces-reload on mismatch. status.rs already carries injected_binary_version to extend to the wire.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#preflightinbinary] [operator-verify] Live proof for #preflightinbinary AFTER the default flip (5ce9b06e6). The flip has landed: SKILL.md now says do NOT run preflight when the '[agent-doc] cycle contract ...' marker is present, and the hook is registered in this project's .claude/settings.json under UserPromptSubmit alongside turn-status. Needing a live session to confirm: (a) invoke /agent-doc <FILE> in a FRESH Claude Code session and confirm the marker + contract appear in context before the agent runs any tool, and that the agent does NOT shell to 'agent-doc preflight'; (b) confirm exactly ONE cycle id is opened for that turn — grep .agent-doc/logs/ops.log for preflight_diff_start, and confirm 'repaired stale preflight_started cycle' does NOT appear; that line is the tell that the agent still ran preflight and the round trip was merely absorbed by repair rather than removed; (c) confirm an ordinary prompt that merely mentions agent-doc produces no marker and no cycle; (d) confirm a Codex/OpenCode turn (no Claude hook, so no marker) still runs preflight explicitly and closes out normally. Scope note: reusable_by_reentrant_preflight is defence in depth only — start_preflight runs at preflight step 4, after repair at step 1, so in the live pipeline repair closes any prior open cycle before the guard sees it. What removes the double-open is the flip itself.
- [/] [#jbsurfaceswap] [operator-verify] Live IDE proof for #jbsurfaceswap (agent-doc 27beeae2e, JB plugin 0.2.296). Both plugins now report one EditorSurface observation per event and the graph derives the intent; the planners are deleted. With the IDE restarted onto 0.2.296, confirm in a real session: (a) clicking between two side-by-side split editors moves the tmux active pane (derived Focus); (b) changing the editor split layout reconciles tmux (derived Sync) and shows the 'Sync: --col ... [focus: ...]' hint exactly once; (c) repeat focus events on the same editor cost nothing — the JB log line '[layout-sync] observe: ... intent=idle' with no controller command; (d) swap two tmux panes by hand, then click an editor tab WITHOUT changing the split, and confirm the pulled tmux probe reconciles the layout instead of only moving focus (#tmuxsyncstate); (e) closing the project releases the root (no growth in surface graphs across open/close cycles). Headless tests cover the fold, the probe seam, and the observation wire shape, but not the debounce/generation interaction with real editor event storms.
- [/] [#jbpluginlazilyeffects] #agent-doc-feature The JetBrains/VS Code plugin should drive lazily state + Effects, not one-shot CLI commands. Today every editor action (file selected, layout changed, Sync Tmux Pane) shells a fresh agent-doc process that re-observes the world from scratch and returns one answer that is already stale, with no subscription and no invalidation. Keep the CLI supported as an entrypoint, but the plugin path should be: plugin reports an event -> FFI sets a Source in the document scope -> an Effect drives the tmux/pane consequence. Same Shared Foundation rule already in AGENTS.md, applied to observation rather than business logic. Consumes the derived owner map from syncownerreactive, so order it after that.
- [/] [#boundaryaccumverify] [operator-verify] After restarting any JetBrains IDE still on the older package generation, reproduce a wedged finalize/direct-commit handoff and confirm visible document, snapshot, and committed HEAD each retain exactly one agent:boundary marker; report any boundary_invariant_violation that does not self-heal.
- [/] [#r4gq] [operator-verify] Live JB Run Agent Doc dispatch-hang proof (#kjw0 fix, shipped 0.34.66 build): after the recycled build is running, trigger Run Agent Doc into a BUSY codex pane (mid-turn, 'esc to interrupt' visible) and confirm it returns FAST with a queued-behind-active-turn outcome (pane shows queued status) instead of hanging ~21s then a dispatch-start-unproven notification. Grep ops.log for route_dispatch_start_queued_behind_active_turn (present on success) and confirm accepted_without_dispatch_start_proof is ABSENT for the busy case. Root cause: route injected the trigger into a mid-turn pane; codex queued it behind the active turn so the UserPromptSubmit hook could not advance within the 10s proof budget, and the direct-pane path paid TWO 10s waits (initial + late-resubmit) then filed a false bug. Fix: short_circuit_dispatch_start_when_pane_busy in route/dispatch.rs detects busy_proof_line and returns RoutedDispatchStartProof::AcceptedQueuedBehindActiveTurn (accepted, not fail-closed).
- [/] [#zqv8] [operator-verify] #jbrelaylive Restart IntelliJ IDEA 2026.1 to activate the installed JetBrains plugin 0.2.247, rerun Run Agent Doc on tasks/professional/equityfundingsource.md while the dim Summarize recent commits suggestion is visible, and confirm dispatch succeeds without a protected-input refusal or a long UI freeze.
- [/] [#compactindcellsverify] [operator-verify] With agent-doc 0.35.3 and JetBrains plugin 0.2.276 active, run Compact Exchange while deleting several queue items and editing a post-boundary prompt; confirm the deletions and prompt survive, compaction commits, and ops.log records compact_rebased_authoritative_siblings without compact_dropped_non_exchange_item.
- [/] [#crdtpushdrainverify] [operator-verify] #crdtpushdrain live proof: after an IDE restart loads JetBrains plugin 0.2.278, run Compact Exchange on a document that has been idle for >30s and confirm ops.log `compact_crdt_relay_acknowledged ... wait_ms=` drops well below the prior ~4.5s floor (expect roughly the pull/apply round trip), with no `compact_crdt_ack_recovery_signal ... reason=ack_recovery_force_refresh` line needed to unstick it. Restart is required because reload-lib only swaps the cdylib, not the Kotlin plugin.
- [/] [#capturebacklogatomic] #agent-doc-bug Capture response plus backlog mutations atomically and let preflight supersede an uncommittable capture. LIVE RECURRENCE 2026-07-22 (#8ybz): retained #bbpe closeout with --backlog-gate spliced the exchange heading/body into the #fmgc backlog row, erased its item boundary, and minted synthetic id #8ybz from the fragment. This is the exact cross-component case missing from the response-only retry regression. Preserve exchange + granular backlog mutations as one cycle intent and add a retained-ACK/CAS-race regression proving component boundaries remain disjoint. Plan: tasks/agent-doc/plan-capture-includes-backlog-mutations.md
- [/] [#bbpe] [operator-verify] Confirm tasks/brookebrodack-dev.md now arms after the IDE picks up the reinstalled plugin: restart the JetBrains IDE (or reopen the doc) to clear editor_attached_model_missing, then run agent-doc preflight tasks/brookebrodack-dev.md and expect queue_maintenance_not_persisted GONE and queue_supervisor_drainable true. The #qstartinert code fix is shipped + installed (agent-doc 9111a3381); only the editor-authority persistence blocker (#5d9f) remains between the computed queue and the persisted one
- [/] [#hvw0] [operator-verify] Rotate the CARGO_REGISTRY_TOKEN secret on github.com/btakita/skill-harness — the current one returns 403 authentication failed, so CI cannot publish to crates.io (v0.1.3 was published manually from the passage token btak/CARGO_REGISTRY_TOKEN). Needs a repo-secret write, which is operator-gated. Until it is rotated, every tagged release will now FAIL loudly at the publish step (that is the intended new behavior after 32836ad; it previously reported success)
- [/] [#c8tb] [operator-verify] Live #jbroutebusystall proof (0b49105e1): with the supervisor socket down (route logs `route_dispatch_only_authoritative_degraded_direct_pane supervisor_health=no_socket`) and the pane mid-turn, click JB Run Agent Doc. Expect `route_start` -> `route_success` in the attempt ledger to drop from ~17s to under ~2s, with `route_existing_pane_busy_active_turn_skip_wait` in ops.log and the SAME outcome as before (`route_existing_pane_already_running`, pane focused, no duplicate injection). Unit tests pin the cue gating but cannot prove the live latency win.
- [/] [#operatorverifylive] [operator-verify] #relaylockpoison live proof: with the JB plugin loaded, force a replica panic (the crash-2026-07-09 shape: agent_doc_replica_apply_local) and confirm later replica calls still succeed instead of returning -1/null for the life of the IDE. This is the behavior the whole migration exists to guarantee and it cannot be proven without a host IDE.
- [/] [#compacttombgc] [recommended] Fenced Compact Exchange CRDT epoch — RE-SCOPED 2026-07-19 away from tombstone GC. The #bootstrapsize measurement (crdt_sync.rs) on a real 57KB document found 88,630 ops and ZERO tombstones, so TextCrdt tombstone GC would reclaim nothing on the observed shape; the payload is pre-compaction INSERT history. The valuable half is the epoch itself: rebootstrap attached replicas from the compacted snapshot and reject/component-rebase older-epoch deltas, which discards that insert history — the only mechanism that makes compaction actually shrink the register bootstrap. Keep tombstone GC as a trailing step for delete-heavy documents, not the headline. Still needs the all-replica stability frontier before any epoch fence is safe.
- [/] [#multilineheadbullet] [focused-cycle] Make multiline `---` queue prompts addressable — SHARPENED 2026-07-19. Damage is already PREVENTED (#qconsumenostrike fail-closed guard, 5b3761dfc); this is the remaining correctness half. Refined finding: the ENTRY model already handles multiline correctly — document_queue::parse surfaces the multiline head as a Prompt (preflight lists it), mark_first_n_prompts_completed targets it, and render emits Completed multiline as `~~~done` fences. The mismatch is that markdown_ast::item_nodes (used for IPC node ops and AST-keyed consume) enumerates only list items, so it returns the NEIGHBOUR for the same logical head — observed as node_ops=[queue:1:neighbour:0] while consumed_text was the multiline head. So the fix is to reconcile the two ENUMERATORS, not the entry model: either teach item_nodes the `---` block shape, or render multiline heads with a bullet + indented continuation that both round-trips through parse_spans and yields a node. Needs one clean reproduction to confirm which of the two paths actually mutated the document in the observed strikes.
- [/] [#zq8e] Editor-authority wedge blocks ALL queue maintenance (root of the #goal drain stall). Evidence in ops.log for this doc: at 19:32:45 `controller_crdt_current_text source=preflight_queue_maintenance` returns `status=current live_editors=1 delivery_converged=true`; 2s later the SAME queue_maintenance path logs `document_model_ensure_start initial_state=editor_attached_model_missing`, a `crdt_replica_register` succeeds (`data_kind=ok generation_fenced=true bootstrap_bytes=1312802`), then `document_model_ensure_republish_observer_not_ready` -> `document_model_ensure_failed` after 5s. EVERY `document_model_ensure_start` this session reports missing-replica across unrelated sources (queue_maintenance, pending_add_sync, controller_recycle_request) while the controller consistently reports current. Two observers of one document disagree; the ensure-path observer does not see a replica the controller demonstrably has. NOTE: likely already owned by the `src/agent-doc-editor-authority-fix` worktree (commits `preserve attached editor authority during retry`, `bound idle authority replay`) — coordinate before landing a rival fix in src/agent-doc.
- [/] [#7n20] Finish the ensure-window liveness fix across the CLI/controller process boundary. `bbc50c26e` extends the 400ms missing-replica ensure window when a `crdt_replica_register` proves the editor is alive, but the counter is process-global: on the split path the register lands controller-side while `ensure_document_model` runs CLI-side, so live telemetry still shows `window_extended=false` and the wedge persists. Two options: carry the liveness proof across IPC, or run the ensure controller-side where the registration already is. Do NOT route preflight's initial current-doc resolve through the ensure path — tried in this cycle and reverted; it violates `preflight_missing_editor_model_pauses_without_reading_disk_authority` and made preflight resolve to DISK content instead of waiting for the attached editor.
- [/] [#realtimesalient] #agent-doc-feature Write salient responses into the document in REAL TIME rather than buffering until finalize. Today only complete `### Re:` sections cross response-checkpoint and partial output is console-only (SKILL.md: "stream incomplete progress only to the console"). Requires deciding what counts as salient mid-turn, and preserving the invariant that incomplete token prefixes never become authoritative document text — so likely a distinct node kind (progress/salient) that is replaced by the final response rather than appended to it. Pairs with #promptselect threading.
- [/] [#mtds] [operator-verify] #panedraftunblocker live proof: on a Claude pane whose footer reads `⏵⏵ bypass permissions on · 1 shell · ← for agents` (the variant without `(shift+tab to cycle)`), with an EMPTY composer and no active turn, trigger JB Run Agent Doc. Expect dispatch to succeed instead of refusing with `timed_out` / `unblocker=wait_for_dispatch_ready_prompt`. Then repeat with unsent text typed in the composer and expect the new refusal naming the draft with `unblocker=submit_or_clear_pane_draft`, and NO injection over that text. Finally confirm a genuinely mid-turn pane (live spinner, bare `❯`) still refuses as busy. Fixed in 17848a927, installed 0.35.6; needs the supervisor idle-boundary recycle to pick up the fresh binary.
- [/] [#promptselect] #agent-doc-feature Selection-scoped `Run Agent Doc`: send ONLY the editor text selection as the prompt. Needs (a) a dedicated CLI verb — `agent-doc prompt "<text>"` (or `steer`; `run` is taken) accepting multi-line via stdin/cat, (b) JB+VSCode `Run Agent Doc` hooked to it so a non-empty selection routes through `prompt` instead of the whole-document trigger, (c) acknowledgement: agent-doc echoes the prompt into the document with the prompt prefix plus salient response, (d) responses attached UNDER their originating prompt — a threaded prompt->response layer in the Document Tree rather than a flat exchange tail. Must compose with realtime steering (#realtime-steering-verbatim) since a selection sent mid-turn IS steering. Design pass needed before implementation.
- [/] [#agentsignals] [future/separate] Add a realtime agent:signals element after core realtime convergence is stable. Start with deterministic built-in signals for tests, queue, supervisor, and ipc; design custom user-defined signals as plugin-backed tracked work with Rust as the first target and future multi-language support; use tasks/software/realtime-signals-patchboard.md as the GUI/semi-programmatic reference; keep MLP enhancement optional/experimental and not load-bearing for deterministic states.
- [/] [#steeringobservableset] #agent-doc-bug Phase 6 durable graph adoption: back RealtimeSteeringSet with a lazily reactive OBSERVABLE SET (not a drain-one-head QueueCell) keyed on the body-aware identity (Phase 3.1), reconciled on the reliable-sync plane; session-check reads set-emptiness (all directives settled) instead of re-diffing HEAD↔buffer. Plan: src/agent-doc/tasks/agent-doc/plan-exchange-tree-seqcrdt-and-ipc-unify.md (Phase 6). [recommended]
- [/] [#crde] JetBrains Run Agent Doc route/dispatch failed after bounded submit/start proof retries #jbrunautobug #agent-doc-bug failure_class=accepted_without_dispatch_start_proof document=/home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md stage=dispatch_only_dispatch_start_proof pane=%83 actor_generation=1214 editor_attempt_id=1783978030079-4 dispatch_proof_state=accepted_without_dispatch_start_proof elapsed_ms=10000 diagnostic_path=none ops_log_path=/home/brian/work/btakita/agent-loop/.agent-doc/logs/ops.log ops_log_marker=route_submit_issue(issue=accepted_without_dispatch_start_proof,phase=dispatch_only_dispatch_start_proof,result=accepted_without_dispatch_start_proof) [symptom-key invariant=run_agent_doc_route_dispatch_failure document=71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad component=route/dispatch_only_dispatch_start_proof content_hash=sha256:3251e072b5c1857ba06e9ba613e1004b5eefef103db7003d7dcf7efe0d698ea8]
  evidence: JetBrains Run Agent Doc route/dispatch failed after bounded submit/start proof retries #jbrunautobug #agent-doc-bug failure_class=accepted_without_dispatch_start_proof document=/home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md stage=dispatch_only_dispatch_start_proof pane=%83 actor_generation=1214 editor_attempt_id=1783978020608-2 dispatch_proof_state=accepted_without_dispatch_start_proof elapsed_ms=10000 diagnostic_path=none ops_log_path=/home/brian/work/btakita/agent-loop/.agent-doc/logs/ops.log ops_log_marker=route_submit_issue(issue=accepted_without_dispatch_start_proof,phase=dispatch_only_dispatch_start_proof,result=accepted_without_dispatch_start_proof)
- [/] [#supboundarylivelock] #agent-doc-bug Supervisor idle-boundary recycle can livelock, leaving a stale binary mapped indefinitely. supervisor_recycle_action (agent-doc-supervisor/src/lifecycle.rs:124) returns None on `!turn_boundary` for everything except write_wedged/editor_delivery_stale, and turn_boundary = prompt_visible && !turn_active (idle_watch.rs:1898) never holds for a continuously-active pane. The only bypass, stale_recycle_safe_checkpoint (idle_watch.rs:136) = supervisor_stale && inflight_handlers == 0, is gated on staleness, so an explicit `admin recycle` on a NON-stale busy supervisor can never be consumed and silently expires at DEFAULT_RECYCLE_REQUEST_TTL_SECS=900 (recycle_request.rs:17) with the ledger phase left Requested. --force does not help: rpc.rs:5905 only changes the reason string, and idle_watch.rs:2403 collapses the request to a plain bool. MAX_CYCLE_OPEN_DEFER_TICKS escalation cannot rescue it either (idle_watch.rs:2253 only accrues the streak AT a boundary). OBSERVED 2026-07-18: supervisor pid 2040014 stayed stale across this whole session even though it IS stale (so the checkpoint bypass should apply and inflight_connection_handlers is an in-process RAII-guarded atomic that cannot leak) - the stale-side path needs its own trace. Fixing the non-stale side means a mid-turn execve, which is exactly what the boundary gate exists to prevent, so this needs a design decision (manufacture a boundary via the recycle-yield path? bound the defer? re-arm instead of expiring?), not a quick patch. Related shipped fixes: #dispatchgateleak + #authorityfactretention (submodule 566a72f6d).
- [/] [#jbrunsteering] #agent-doc-bug JB `Run Agent Doc` while a turn is already running should feed the prompt in as realtime steering (#realtime-steering-verbatim / #realtime-steering-aggregate) instead of interrupting the active turn. Operator-reported: invoking it mid-turn produced "Interrupted · What should Claude do instead?" rather than surfacing the new prompt to the running turn. NOT STARTED.
- [/] [#ay80] CI red on main, pre-existing and unrelated to #jbsteerinterrupt: `finalize_skips_ignored_untracked_session_doc` (tests/finalize_integration.rs:248) fails in GitHub Actions with `Unexpected stderr, failed var.contains(skipped ignored untracked path scratch/session.md)` but PASSES locally (7627/7627 green). Run 29657026491 on b7e17f4d5. Likely a CI-environment gitignore/tmpdir difference; reproduce with the CI runner git config.
- [/] [#5d9f] Second session observed that finalize's commit boundary persists queue maintenance through a path standalone `agent-doc queue sync` never reaches — the queue landed on the next write after three straight sync failures plus an `admin reload-lib`. Two persistence paths with different reachability is itself the bug surface. Determine why the commit path succeeds where queue sync fails, and route queue sync through whatever the commit boundary does. Result: queue text populates but stays UNARMED (`queue_drainable_head_count: 0`, `queue_continuation_required: false`), so the auto-loop still never gets the head.
- [/] [#jbroutasync] #agent-doc-bug Honor submit.deadline_ms in route dispatch (NARROWED — the async-endpoint half moved to #jbrouteasyncchannel). CONFIRMED: deadline_ms is only ever written (rpc.rs:1116, command_plane.rs:93) and never read by dispatch; routed_cycle_ack_timeout is 30s with a live child / 15s without (agent-doc-controller/src/dispatch.rs:2099-2103); the plugin sends deadlineMs = waitForReadySeconds*1000 for editor_route (CpcRouteClient.kt:281), observed as timeout_ms=15000 in ops.log. Net effect: with a live child the client gives up at 15s while the controller waits to 30s, so the operator sees a timeout while the route is still running. FIX: cap the ack wait at min(client deadline, routed_cycle_ack_timeout). PLUMBING REQUIRED (why this is not a one-liner): the deadline lives in the controller RPC layer but the wait happens in agent-doc-route-io at cycle_ack.rs:334, and no type in that crate carries a deadline. Thread an Option<Duration> through require_routed_cycle_ack (cycle_ack.rs:313) and its 3 call sites (authoritative_dispatch.rs:987, authoritative_dispatch.rs:1142, pane_resolution.rs) plus the controller→route boundary. NOTE test_cli.rs:16811 pins the exact expression 'routed_cycle_ack_timeout(live_child_for_file, cfg!(test))' and must be updated to pin the invariant, not the syntax. Verify with a live route against a busy pane: the client-side timeout and the controller-side give-up must coincide.
- [/] [#f1s3] #queuenest phase 2 — `ol` -> `after=` compilation, per tasks/agent-doc/plan-queue-nested-list-grouping.md. PHASE 1 SHIPPED in b67dd3582 (indent-aware parse retaining depth, leaf-only execution so group parents are never runnable heads, byte-preserving render round-trip). REMAINING: parse `1.` ordered-list markers (today they fall through to Freeform), and compile a nested ol to `after=` edges at ingress next to #queueingressorder, keeping `after=` the single source of truth so reordering an ol re-derives edges. BLOCKED ON A DESIGN ANSWER the plan itself records as open: do groups need ids (`[#epic]`) for `after=` to reference a whole group? Compiling an ordered GROUP to edges needs to know whether a group is addressable. Second open question (does `priority=` on a parent apply to all its leaves?) affects the sort, not the compilation, and can be settled later.
- [/] [#editormodelmissing] [operator-verify] #agent-doc-bug editor_attached_model_missing permanently wedges every write needing editor authority (agent-doc-bugs2 gen=1518, equityfundingsource gen=1452; healthy docs are gen 14-82). NARROWED 2026-07-18: survives admin recycle x2, admin reload-lib, AND a full IntelliJ restart, so it is NOT a plugin model-rebuild problem as first filed. Root cause is a one-way latch in editor-attachment state: TransportProjection.editor_generation (agent-doc-state-backbone/src/lib.rs:3394) is SET on StateFact::OwnerGenerationChanged (line ~1985) and never cleared — there is no editor-detach/close StateFact in the enum at all. document_has_live_editor_transport (agent-doc-document-realtime/src/crdt_authority.rs:165) therefore returns MultiReplica forever once an editor has ever attached, with NO liveness proof, even though the sibling authority_from_liveness (line 122) exists precisely to demote a stale listener via pid liveness. The ONLY predicate that flips it false is TransportPatchPhase::ForceDiskFallback — which is exactly why --force-disk is the sole escape the operator ever found. Second candidate latch, same shape: observe_editor_open's durable reliable-sync receiver journal may replay a stale editor-open assertion across an IDE restart (resolve_closed_editor_disk_fallback_current_doc, agent-doc-document-realtime-io/src/lib.rs:3774 bails only when it reports open). DECISIVE EXPERIMENT before coding: instrument which of the two reports attached — log editor_generation + observe_editor_open together at the EditorAttachedMissingReplica branch (agent-doc-crdt-relay-io/src/lib.rs:696). Both present identically, and the fix differs by which. DO NOT ship a speculative authority-demotion change: wrongly demoting a live editor lets the binary overwrite an operator buffer, which is the exact failure every guard on this path exists to prevent. Also worth shipping regardless: admin detect reported 'No fleet anomalies detected' while two documents were hard-wedged — add an attached-but-no-replica detector so this is visible instead of silent.
- [/] [#bbdqprio] #agent-doc-bug brookebrodack-dev.md: agent:queue carries the priority attribute but a cycle's response explained next tasks without reordering the queue by backlog priority. Hypothesis: same failure class as the queue_maintenance_not_persisted warning seen in agent-doc-bugs2.md this cycle — preflight COMPUTES the reorder (logs 'queue: sorted do-prompts by backlog priority') and then fails to persist it because editor authority stayed in editor_attached_model_missing, so the previous queue text stays in effect and the reorder silently evaporates. Verify by correlating a brookebrodack-dev preflight's 'queue: sorted' line against the resulting document; if confirmed, the fix is to make a non-persisted queue maintenance pass fail loudly (or retry) instead of logging a warning three lines after printing work it did not do. Do NOT run preflight against brookebrodack-dev.md while pane %9 owns it.
- [/] [#falsetyping-guard] #agent-doc-bug provenance-aware visible-write guard. INTEGRATED to main (merge on 5bcfc07b) + binary/cdylib installed + recycled; guard tests 7/7 green; JB 0.2.205 zip + VS Code 0.2.38 compiled. REMAINING (operator): install JB plugin zip (build/distributions/agent-doc-jetbrains-0.2.205.zip) + reload IDE, install VS Code 0.2.38, then live proof — type during replica churn (no wedge) + genuine unsaved edit still fails closed. NOTE: bugs2.md can still wedge until the IDE plugin is reloaded (loaded plugin still calls FFI v2 → conservative fail-closed).
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#qloop1] #agent-doc-feature Define durable queue loop annotation syntax and parser/rendering behavior: mark loop-only queue/backlog entries, keep annotation/state out of prompt completion proof, and expose a stable current-item marker for editor/supervisor surfaces.
- [ ] [#qloop2] #agent-doc-feature Implement idle-loop scheduling for annotated loop items: when normal drainable queue work is empty, dispatch loop items in deterministic sequence, persist the loop cursor/last-run state, and prove regular queue items always preempt loop work.
- [ ] [#qloop3] #agent-doc-feature Add loop poll controls: global default poll period plus per-loop override, next-run time gates, bounded backoff/jitter, and tests proving no tight spin when there are no relevant events.
- [ ] [#qloop4] #agent-doc-feature Add event-aware loop scheduling: record deterministic event counters/timestamps and let loop items schedule earlier/later based on event frequency before any model-assisted scheduling hook.
- [ ] [#qobs1] #agent-doc-feature Add a bounded observer telemetry snapshot surface: supervisor/session actors expose compact per-document state (cycle phase, queue head, actor readiness, IPC inflight, recent typed ops-log markers, and artifact handles) without dumping raw transcripts by default.
- [ ] [#qobs2] #agent-doc-feature Implement deterministic observer rules that scan session-check/preflight/ops-log snapshots for annotated issues and recurring failure patterns, then file deduped session-doc backlog bugs with evidence paths and stable issue fingerprints.
- [ ] [#qobs3] #agent-doc-feature Add a sidecar observer agent-doc loop that can monitor all running project sessions from snapshot artifacts, rate-limit bug filing, avoid pane interference, and route created bug items to the correct session document or a configured observer inbox.
- [ ] [#qobs4] #agent-doc-feature Add model-assisted observer triage as advisory-only: feed compact snapshot/context-pack handles to a subscription or local model, require deterministic evidence before auto-filing bugs, and record model provenance/confidence on suggestions.
- [ ] [#qlm1] #agent-doc-feature Define queue item backend-selection syntax for local-model execution, separate from queued `/model <tier>` switches: support explicit per-item backend/model annotations on normal and loop queue items without changing prompt text or completion proof.
- [ ] [#qlm2] #agent-doc-feature Implement a local-model queue dispatcher backend that can run eligible queue/loop items through a configured local model, capture stdout/structured results, and write responses through the normal agent-doc closeout boundary.
- [ ] [#qlm3] #agent-doc-feature Add local-model dispatch safety gates and telemetry: capability checks, GPU/resource lease integration, fallback policy, provenance logs, and tests proving local-model queue items cannot be silently marked complete without a committed response/proof.
- [ ] [#exttasksources] #agent-doc-feature Add external task source adapters that project PM-tool tasks into the active document/work graph as source-backed nodes, keeping local queue/closeout authority document-owned and making remote push-back explicit and audited. PRD: tasks/agent-doc/prd-external-task-sources.md
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
