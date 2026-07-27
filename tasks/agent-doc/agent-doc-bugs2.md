---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
resume: 68af54ca-b852-448f-95ed-24f29b695261
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

done
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260726-152719.md`*

Compacted content:
- Archived 13 response topic(s): #jbsurfaceswap; preflight ↔ session-check reactive unification; session-check, timeouts, and the preflight round trip; 10 more
- Prior summary/context: prior compacted content: Archived 10 response topic(s): #retainedsettlereactive; #reactivemap; complete the settle_re...

### Re: #adbacklogorphanseg — opus-5

Landed (`7d68b27bd`) and applied — both orphan lines are gone from this document's `agent:backlog`, with the deletion recorded in `ops.log`.

**My first version was wrong, and an existing test caught it.** I dropped *all* non-blank non-item text, reasoning that `parse_pending_edit_payload` already refuses that shape so the component contract must forbid it. `backfill_preserves_interleaved_headers_and_blank_lines` failed on the first run: `### Active` / `### Later` section headers inside a backlog body are a supported, tested feature. That refusal governs an *edit payload*, which is narrower than the stored-content contract — I had over-read it, and the blanket rule would have deleted operator content. This is precisely the risk I named when I declined to land it earlier, and it was real.

So the predicate is two observed corruption signatures rather than a category:

1. an `### Re:` heading — the *exchange response* form, which reaches a tracked-work body only by being spliced there, where a legitimate section header is a topic label;
2. a line carrying `] [#id]` — an item whose leading `- [ ` was eaten, i.e. a destroyed item boundary.

Headers, prose, blank spacing and continuation all survive. Mutation-checked in both directions: disabling the drop fails the new test, and widening it back to all non-item text fails the header test. That second check is the one that matters — it pins the narrowing, not just the fix.

**The deletion is never silent.** `backfill_reporting_dropped_text` returns every removed segment; the IO caller logs each verbatim as `backlog_non_item_text_dropped` with byte count and escaped newlines, plus a stderr note. The two removals here are in the log: 24 bytes (`### Re: #bbpe — gpt-5`) and 1634 bytes (the `] [#fmgc]` fragment). Both were duplicates of content that still exists elsewhere — `#bbpe` is intact in `agent:review` and `#fmgc` was reaped on 2026-07-23 — so nothing was actually lost, and git has it regardless.

Verified after applying: 31 items intact, `#closeoutwaitchurn` and `#jbedtledgerverify` both present, diff is 1 insertion and 3 deletions — exactly the two orphan lines. The first invocation reported a 5s controller timeout *after* the write, so the change had already persisted; the second run confirmed "already canonical".

`backfill` keeps its original signature for callers that hold no file path to log against. Full suite 8153/8153, clippy clean.

### Re: closeout settling latency — opus-5

Measured first. Across eight closeout cycles the breakdown is unambiguous:

| phase | cost |
|---|---|
| `git_commit` | **4.6–22.5 s** |
| `session_check` | 2.9–4.2 s |
| everything else | ~50 ms each |

So "settling takes too long" is `git_commit` plus a flat `session_check`, not the CRDT/editor ACK I had been treating as the wait.

**What I fixed** (`9e7ce2f79`). This repo has 69 submodules, and `tracked_modified_paths` was paying two whole-tree scans for information it then discarded:

- `git status --porcelain=v1 --untracked-files=no --ignored=no` costs **284 ms** because it descends into every submodule. With `--ignore-submodules=all` it is **6 ms**, and the two outputs differ by *exactly* the gitlink lines the function throws away three lines later under `#side-effect-exclude-submodules` ("a dirty submodule gitlink is NEVER an agent-doc cycle side-effect"). The exclusion was already the intent; asking git for them first was pure cost.
- `submodule_paths` ran `git submodule status`, which walks all 69 — **154 ms** — to get the registered path set. That set is config: `git config --file .gitmodules --get-regexp` returns it in **~0 ms**. `git submodule status` also reports live checkout state this caller never reads, so the cheap source is the more precise one too.

**What I have not fixed, and will not claim.** Those two save ~432 ms per call, and `tracked_modified_paths` has few callers — so they are **not** the bulk of a 5–22 s `git_commit`. I stopped short of asserting a win I cannot yet show end to end. The next step is per-subprocess instrumentation inside the commit path rather than more inspection: the leading candidate is `candidate_repos` in `partial_staging.rs`, which passes `--ignore-submodules=none` and *means* it (it is hunting dirty submodule repos, 341 ms) — the real question there is whether that guard needs to run on every closeout at all, which is a semantics decision, not a flag.

`session_check`'s flat ~3.5 s is the other half and is separately attributable: an earlier perf line put `inspect_with_warnings` at 2665 ms of a single invocation — a ~30-guard sweep paid per call. Filed together as `#closeoutgitsubmodulescan` with the measurements, so the remainder is picked up with the numbers attached rather than re-derived.

Full suite 8153/8153, clippy clean. `candidate_repos` semantics deliberately untouched.

### Re: closeout settling latency — opus-5

Measured rather than guessed, and the headline is that **the phase name was lying**.

Across eight cycles the breakdown looked like `git_commit` 4.6–22.5 s, `session_check` 2.9–4.2 s, everything else ~50 ms. So I went at git: this repo has 69 submodules, and `tracked_modified_paths` was paying two whole-tree scans for information it discarded three lines later. `git status --porcelain=v1 --untracked-files=no --ignored=no` costs 284 ms because it descends into every submodule; with `--ignore-submodules=all` it is 6 ms, and the outputs differ by *exactly* the gitlink lines `#side-effect-exclude-submodules` already throws away. `submodule_paths` ran `git submodule status` (walks all 69, 154 ms) for a path set that `git config --file .gitmodules --get-regexp` returns in ~0 ms. Both fixed in `9e7ce2f79` — a genuine 47× on those calls.

**It did not move the number.** Next closeout on the new binary: `git_commit:10513ms`. So I instrumented instead of inspecting further, and found the actual answer: `commit-io`'s *own* internal `commit.git_commit` timer reads **9–19 ms**, and the git plumbing under it (`read-tree`, `write-tree`, `commit-tree`) is 1–4 ms each. Two different timers were sharing one name — one 19 ms, one 10513 ms.

The outer phase is not git at all. It spans editor-authority resolution plus CRDT convergence inside `commit_for_authority`, and a controller round trip to record the committed baseline. Both are waits on other processes. Split into `commit_authority` and `record_committed_baseline` in `2b2f13669` so the breakdown names what it is waiting on.

I am recording this as a diagnostic failure, not a tidy-up. A phase label pointing at the wrong subsystem cost me a full build-install-measure cycle chasing git — the same shape as the anonymous relay read line I fixed earlier today in `3aded9377`, where the log proved time was spent but attributed it to nothing. Both times the instrument was the bottleneck, not the code.

So the honest state: the submodule scans are fixed and worth having, the real cost is inter-process convergence, and it is now labelled truthfully so the next pass starts from the right place instead of my third guess. `session_check`'s flat ~3.5 s remains separately attributable — an earlier perf line put `inspect_with_warnings` at 2665 ms of one invocation, a ~30-guard sweep paid per call.

### Re: interrupted turn, edited prompt, replacement dispatch — opus-5

> **Queue prompt:**
>
> I Interrupted a running Run Agent Doc turn, modified the prompt, then attempted to start a new Run Agent Doc. We should always send `agent-doc <file>` for both starting a replacement turn in this scenario and realtime steering (either by user text selection or diff): `failed to converge queued dispatch for job-offers.md through editor IPC/disk: route_dispatch_queue: editor delivery ACK recovery did not settle within 8000ms and the exact canonical target lacks active retained-delivery proof; refusing closeout`. I attempted to steer but `Could not steer the active turn: authoritative supervisor rejected selected-text steering: turn steering requires an authoritative actor with an active busy turn`.

Filed as `#adreplacementturndispatch` with both exact errors, because it is two distinct failures and one of them is already tracked.

The steering rejection is `#steergateidle`, already in backlog: steering is gated on an active busy turn and rejects otherwise, where per your earlier report an idle pane should simply dispatch the selection as a normal trigger. Your instruction here — always send `agent-doc <file>` for steering too — is the same conclusion from the other direction: there should not be a separate steering channel that can be refused on actor state.

The convergence failure is separate and new. A replacement turn after an interrupt carries an edited prompt with no retained-delivery proof, so the 8 s ACK recovery window expires and the dispatch is *refused* rather than falling back to the plain `agent-doc <file>` trigger — which is exactly the fallback you are asking for. I have not fixed either; this arrived at the end of a turn whose work was already committed, and I would rather hand you an accurate report of both than a rushed change to the dispatch path.

### Re: reload + recycle — opus-5

Done. `admin reload-lib` delivered the cdylib v0.35.28 typed reload to 1/1 editor endpoints (0 restart required, 0 unavailable), and `admin recycle` took: the controller is a fresh pid and `session-check` no longer reports the stale-binary warning, so the supervisor is mapping the current build.

This is also the measurement I was missing. The corrected phase labels from `2b2f13669` only appear once a closeout runs on the fresh binary, so this cycle's own closeout is the first breakdown that will show `commit_authority` and `record_committed_baseline` split apart instead of the single mislabelled `git_commit` that sent me into git for a cost that was never there.

Worth noting against the earlier finding: this reload is itself the thing I measured as expensive — 307 reload/reregister events and 838 full replica bootstraps at ~290-326 KB each across 12 installs today. Running it once deliberately is cheap; the defect is that a library reload forces a *full* bootstrap rather than an incremental resync, which is what made every closeout today pay a re-registration before it could converge. That sits next to `#ctrlkillreregister` and is the real remaining target for settling latency — not git, and not the closeout guards.

### Re: #ctrlkillreregister — opus-5

Already complete. I verified rather than re-implemented, and the item's REMAINING text is stale.

Its two remaining clauses were "have the JetBrains and VS Code plugins call `agent_doc_peer_replicas_missing` on reconnect/startup, then retire the Tier 1 fan-out as a compatibility shim." Both are in place:

- **JetBrains** calls `pullMissingReplicas` on `plugin-startup` (`PluginLifecycleListener`) and on `controller-transport-recovered` (`CrdtReplicaManager`).
- **VS Code** calls it on `activation` and on `controller-transport-recovered` (`crdtReplica.ts`).
- **The Tier 1 push is retired per peer**, gated on `peer_replica_pull_v1` via `peer_repairs_itself`.

The live log settles it: **573 skips against 22 pushes**, and the timestamps are the proof — every push is from 01:30–01:46, before the capable plugin generation, while the skips continue through 15:53. So the fan-out is behaving exactly as the design describes: a shim that retires itself per peer from converged state, with no flag day. An old plugin would still get pushed at; the current one is not.

**What this does not fix, which matters given why you asked.** `#ctrlkillreregister` covers a replica that is *missing* after controller death, and the repair now triggers correctly from the editor side. My latency measurement is about the *cost of that repair once triggered*: with the corrected phase labels, `commit_authority` is 11858 ms of a 16866 ms closeout, against `record_committed_baseline` at 88 ms and an internal git timer of 9–19 ms. The correlation is 12 installs → 307 reload/reregister events → 838 full `crdt_replica_register` bootstraps at ~290–326 KB each.

So the remaining problem is that a reload or restart forces a **full** bootstrap where a delta would do. The relay already exposes `canonical_state_vector()` and `CurrentRevision` already carries it, so the editor could send its state vector and receive only what it lacks. Filed as `#replicareloadincremental` with the measurements and a verification method — measure `commit_authority` across a reload with nothing else changed — so it is not re-derived from scratch.

### Re: session cleanup and why dead actors were never purged — opus-5

Cleaned up, and the "not being purged" part had a real root cause rather than a missing sweep.

**Cleanup.** `agent-doc gc` removed 42 stale files and reclaimed **81 MB** from `state.db`. No orphaned temp-root controllers remain — the ones from `make check` runs self-terminated correctly via the serve loop's missing-root check. The fleet is 3 live sessions (`%1` mine, `job-offers` `%4`, `lazily` `%5`), all left alone, plus the 2 dead records. `resync` reported one issue I deliberately did **not** auto-fix: `job-offers.md` (pane `%4`) is in the `stash` window — that is another session's live pane, so moving it is the operator's call via `resync --fix`.

**Root cause of the un-purged dead actors** (`tmux-router` `da78dc4`). `prune_dead_actors` judges a record by its age from `last_transition` against `DEAD_ACTOR_PRUNE_AFTER` (1h). The registry prune was resetting that clock: for every document absent from the incoming registry it wrote a `registry_prune` transition and set `closed`/empty pane/empty window **unconditionally**, including rows already in exactly that state. Each pass therefore recorded a no-op transition and made the record one prune-interval *younger*. No dead actor could ever reach 1h, so none were ever purged.

The evidence from `state.db` is unambiguous — `caller=tmux-router reason=registry_prune gen 1551->1551 pane ->`: same generation, empty-to-empty pane, a transition recording that nothing happened. And the ages named it: both closed actors read **443s, identical to my live document**, because one sweep had just touched all three. The `#actorprune` comment's 251 accumulated rows are this same mechanism.

`already_pruned_closed()` now skips the write when the row is already in the target state, while still deleting the stale `registry_entries` row. A transition log should record transitions; nothing changing is not one.

**Verified on the live system, not just in tests.** After install, three consecutive registry prunes wrote **zero** new transitions for the two closed documents — 312 and 320, unchanged — where each pass previously added one apiece. Their ages now climb (838s → 964s) instead of resetting, so they will age past 3600s and be pruned. Mutation-checked both directions: with the guard disabled three prunes write three transitions, and the test separately asserts that closing a genuinely *live* actor still records its transition, so the guard cannot be widened into silence.

This is the fourth instance today of the same underlying shape — a fact recorded for an event that did not happen, which then defeats a downstream decision. The phantom backlog id, the mislabeled `git_commit` phase, the anonymous relay read, and now a no-op transition that made a retention policy unreachable.

agent-doc: 8153/8153, clippy clean. tmux-router: 135 lib tests, clippy clean. Only `src/registry.rs` staged there — `src/tmux.rs` is a concurrent session's edit.

### Re: should we background commit_authority off the hot path? — opus-5

No — and I want to give you the reasoning rather than just the answer, because there is a version of backgrounding that is already happening and a version that would be dangerous.

**It is already backgrounded on the slow path.** When the convergence budget expires, the write intent is retained in `state.db` and the supervisor/session-check resumes it. That path ran repeatedly today: `respond` refuses, the intent persists, and the closeout settles minutes later without losing the response. So backgrounding is the existing *fallback*. The question is whether to make it the default.

**Making it the default trades a measured latency for an unmeasured risk.** Three specific reasons:

1. **The commit is the closeout boundary.** `respond` is specified to reach `committed`; if it returns before committing, "captured but not committed" becomes the normal state instead of the exceptional one. Nearly every wedge I fixed today lived in exactly that gap — a reaped id with no response record, a retained intent blocking preflight while session-check reported ok, a phase that looked terminal but was not.
2. **Queue-drain correctness depends on it.** The auto-loop continues only after a proven closeout. Backgrounding means item N+1 begins while N's commit is still in flight — concurrent cycles against one document, which is precisely the generator of the `#capturebacklogatomic` splice whose damage I spent part of today repairing.
3. **It would hide the signal.** The 11s is currently visible and attributable. Moved off the hot path it becomes invisible latency in a supervisor, and today demonstrated twice that an unattributable cost is the expensive kind.

**What the data says the real fix is.** Across 522 convergence waits: `delivery_ack_pending` 457 (88%), `controller_model_backpressure` 53 (10%), `compare_and_swap_raced` 12 (2%). So `commit_authority` is the editor confirming delivery — not git (internal timer 9-19ms), not the controller baseline round trip (72-88ms), and not poll oversleep (backoff is 25ms initial / 250ms max, so at most 250ms of added latency).

**And I need to correct myself.** I filed `#replicareloadincremental` as *the* cause. It is not: only 64 of 210 waits had a `crdt_replica_register` within 8s, so full-bootstrap-on-reload accounts for roughly 30%. That is a real contributor and worth fixing, but I overstated it, and I have updated the item to say so with the numbers.

The remaining ~70% is genuinely unexplained, and the correct next step is instrumentation rather than a fourth hypothesis: decompose the ACK round trip with editor-side timestamps — plugin receives the intent, applies it to the buffer, emits the visible receipt, binary observes it. Both of today's wrong guesses cost a build-install-recycle each and both collapsed instantly once a log line carried an attribution field. I would rather hand you that than a speculative change to the write path, which is the one place in this system where being wrong loses operator text.

### Re: backlog + queue items for the convergence work — opus-5

Filed five, top-down in the order I would actually work them. The backlog component carries the `queue` attribute, so go-mode mirrors them into `agent:queue` in the same order.

1. **`#ackeditorstamps`** — instrument the editor half of the ACK. `2ea5c0475` gave the binary's own states a per-write breakdown, but the editor stays opaque: `delivery_ack_pending=11000ms` still cannot distinguish "received late" from "applied slowly" from "applied fast, receipt slow". Four editor-side timestamps fix that. First because it is cheap and it decides whether item 5 applies at all.
2. **`#crdtwaitsubscribe`** — replace the convergence poll with a subscription. The measured justification is **~18 controller round trips per write** (6,983 write-path reads / 390 acknowledged writes), i.e. an 11 s wait generating load against the controller it is waiting on — the same self-feeding shape as this morning's idle-watch storm.
3. **`#crdtwaitsplit`** — separate controller backpressure from delivery convergence. Different conditions, different remedies, currently one loop and one backoff.
4. **`#sessioncheckguardsweep`** — the other measured phase, a flat 2.9–4.2 s (once 11.3 s), with the per-guard attribution recorded.
5. **`#crdtdeliverybatch`** — filed `[recommended]` and explicitly gated on item 1.

Two things I want on the record because they cut against what was asked.

**The reactive-graph move is right, but not for poll oversleep.** I measured that: the backoff is 25 ms initial / 250 ms max, so sleeping accounts for ≤2% of an 11,000 ms wait. Selling the refactor on latency-per-sleep would be selling a number I have already disproved. The honest case is the ~18 generated round trips per write, and the item says so in those words so nobody re-derives the wrong rationale later.

**Batching probably does not fit the shape we measured**, so it is filed as an option with its precondition rather than as agreed work. `delivery_ack_pending` is 457 of 522 waits and reads as one long stretch per write; batching helps throughput across writes, not the latency of a single ACK. If item 1's profile shows many short delivery cycles instead, it becomes the right lever and the item says exactly that.

Also carried forward, already filed and not duplicated: `#replicareloadincremental` (corrected today — a ~30% contributor, not the cause), `#closeoutwaitchurn` (which I hit again this turn when a held foreground-finalize lease blocked a write with no way to wait), `#steergateidle`, and `#adreplacementturndispatch`.

Item 2 carries an explicit caution: this is the write path, the one place where being wrong loses operator text, so the deadline and retained-intent fallback must stay intact through the refactor.

### Re: do [#ackeditorstamps] — opus-5

> **Queue prompt:**
>
> do [#ackeditorstamps]


Done, plus two things that had to be fixed before the queue could reach this item at all. Taking them in the order they blocked each other.

**The session started wedged, and the escape was `agent-doc commit`.** Preflight refused with a retained `pending_write` (`reason=crdt_delivery_ack_pending`, `cause=payload_absent_from_converged_content`) whose own message says to retry only `session-check`. That never converges here, and the ops log says why: the retained intent targets `2be92974` (85054 bytes) while the converged content moved to `1e405ae3` (85183 bytes) when the queue mirror landed 4 seconds later. The gate waits for an exact hash the document has already superseded, so it waits forever. `reload-lib` did not help — `editor_convergence` was `ok` the whole time, the wedge was the gate, not the transport. `doctor` classified it `recoverable`, and its printed repair (`write --commit`) returned "empty response — nothing to write" because the response body was already on disk. `agent-doc commit` cleared it in one shot.

**Then three preflight runs silently walked the queue past two of your heads.** `#ackeditorstamps` and `#crdtwaitsubscribe` came back stamped `⏭️`, and the turn was re-pointed at `#crdtwaitsplit` — an item whose own text says "land after `#crdtwaitsubscribe`". This is `#adpreflightreentryskip`, which was supposedly closed this morning: the guard requires a prior cycle to have "come back" before its head can be skipped, and accepted a terminal phase as that proof. Preflight's own step 2 commits the document — including the `🚧`/`⏭️` marker it just wrote — so an empty `preflight_started` cycle is promoted straight to `committed`. The re-entrant preflight the guard exists to reject therefore satisfied it. `document_cycles` has the receipts: `cycle-1785091429188` and `cycle-1785091492165`, both `committed`, neither with a capture in the ledger.

Fixed in `b15557527`: require the capture or the response hash. A phase proves only that *something* committed, and preflight is one of the things that commits. I also stopped a carried skip from outliving the stall it recorded — a skipped head is never selected, so it could never resolve itself out of the set, which is how your top item silently stopped being worked; any resolved id now re-offers them. Live proof rather than a green test: the very next preflight selected `do [#ackeditorstamps]` and the two bogus `⏭️` markers cleared themselves. That commit also fixes the clippy error that had CI red on main since 18:32 — unrelated to this work, but it was failing `make check` for everyone.

**`#ackeditorstamps` itself** (`7b84cfb51`). Four stamps, three legs. The editor stamps *received* at the pull parse (so every pull path carries it, not just the one call site), *applied* on the EDT before the executor hop (so the hop's latency lands in the next leg where it belongs, not inside the apply), and *receipt* at the send — the last instant the editor half owns. The controller stamps *observed* as the ACK lands and logs `crdt_replica_ack_editor_profile file=… profile=[received_to_applied=Nms applied_to_receipt=Nms receipt_to_observed=Nms received_to_observed=Nms]`.

Two decisions in there worth naming, because both are ways this could have reported a confident wrong number:

- **A leg renders only when both its endpoints are stamped.** An older plugin, or a self-echo ACK that never crossed the buffer, leaves a stamp absent — and `0` is a valid-looking `u64` that would have reported the Unix epoch as a timestamp and a ~55-year apply. A missing leg is a fact; a fabricated one is a bug that looks like data. When nothing is derivable the whole line is suppressed, so an un-updated replica adds no noise.
- **A backwards leg reports `skew`, not `0`.** These are two processes with two clocks. Clamping a negative delta reads as "instant", which is exactly the wrong conclusion to hand whoever profiles this next.

You said batching "probably does not fit the shape we measured" and gated `#crdtdeliverybatch` on this item. The profile is what settles it: if `received_to_applied` dominates, the delivery is fine and the apply is slow, and batching deliveries changes nothing; if the cost is in `applied_to_receipt` / `receipt_to_observed`, the receipt path is the target. Either way it is one log line rather than a fourth hypothesis costing a build+install+recycle.

**On the lazily release** — confirmed on crates.io, `lazily 0.51.0`, and the local checkout ships `ThreadSafeQueueCell`, `AsyncQueueCell`, and both work-queue flavors. I bumped every agent-doc crate to it and `make check` is green on the new version, so `#crdtwaitsubscribe` is now unblocked at the dependency level: the poll it replaces is the `~18 controller round trips per write` figure, and a thread-safe queue is the primitive that argument was waiting on. That is the next queue head, so I have left the item text alone rather than pre-committing its design.

Verification: `make check` green (clippy + tests + the TLA model check), 131 preflight-io tests, the new controller-io profile test, plugin tests green at 0.2.297, `make install` + `admin recycle` + `reload-lib` done, both commits pushed with the gitlink bumped. CI runs `30215848121` and `30216394078` were still in progress at closeout, recorded rather than waited on.

One thing I cannot close myself: the three editor stamps come from Kotlin, and the plugin does not hot-reload. Until you restart the IDE, the profile line is correctly *absent* rather than wrong. Filed as `[operator-verify]` with what to look for.

### Re: the retained-hash gate, and three questions about enums — opus-5

Fixed, and the answers turn out to be the same finding.

**The gate.** A closeout writes more than once: the response cell, then a `pending_write` carrying response+backlog, then the `pending_add_sync` queue mirror whose target is a superset. Interrupt it between the last two and the middle intent is retained against bytes its own successor already replaced.

Nothing could clear that. Exact byte equality never fires — the target hash will not reappear. The rebased-payload proof never fires either, and this is the part worth sitting with: `carries_response_payload` is hash equality against the closeout's *response cell*, and that hash belongs to the **earlier** write. So the one intent shape that actually strands is the one shape the escape hatch cannot reach. The gate then refuses every future cycle while reporting `delivery_ack_pending`, which reads as a slow transport rather than a permanent one.

The new proof asks what the intent was *for*: it adds content, so compare what it was going to add against what converged. Every non-blank added line present ⇒ the successor carried it. That is strictly stronger than "the document changed", so content that never carried the write cannot satisfy it, and an unknown or empty delta proves nothing rather than everything — a deletion-only or baseline-less intent stays on exact bytes. The line set is unordered on purpose: a superseding write may place the same lines elsewhere, and a positional diff would call that a miss.

I also stopped `session-check` telling you to retry in the one state where retrying cannot work. When the planes have already converged there is no delivery to wait for, so it now names the command that resolves it. Yesterday that guidance cost a full escalation through retries, `reload-lib`, `repair`, `autofix`, and the `write --commit` that `doctor` recommends — which answers "empty response — nothing to write", because the body is already in the working tree.

**Should the write settlements be in an enum?** They already are — `SettlementVerdict` / `SatisfiedProof` / `UnsettledCause` — and that is the evidence for your other question. Adding today's proof was one new variant, one match arm, and tests. The exhaustive match told me every place that had to change.

**Should the intents be in an enum?** Yes, and this bug is the argument. `DocumentWriteIntentProjection.source` is a `String`, and behavior already keys off it — `source == "post_commit_reposition"`, `starts_with("serialized_atomic_write")`, `starts_with("force_disk")` — across three crates, by prefix in some places and equality in others, with nothing enumerating the values. So the type cannot say that `pending_write` and `pending_add_sync` are consecutive stages of one closeout, and settlement had to answer "was I superseded by my own successor?" with a content diff instead of an ordering comparison. Filed as `#adwritesourceenum`. One constraint that has to be designed in, not discovered: `source` is serialized into `state.db`, so the enum needs an `Unknown(String)` that round-trips — a hard deserialization error would wedge exactly the cross-version case this is supposed to make safe. I would keep the delta proof either way; it covers operator rebases that no stage ordering can express.

**What other free-text should be typed?** The big one is the ops.log vocabulary: **812** distinct event names emitted as `format!` literals against **310** call sites that match log text with `contains(...)`. Producer and consumer share no vocabulary, so a renamed event silently stops matching — and the failure is invisible, because the matcher just returns `false`. That is a correctness surface: `autofix` plans repairs from those lines, `doctor` and `session-check` classify recovery state from them. Filed as `#adopseventenum`, scoped to the events that are *parsed* — starting from the 310 `contains()` sites and working backwards. Typing all 812 emitters would be churn without payoff, and the ones only humans read should stay free text. Below those two: `--origin` write origins, and the `reason=` strings that are not yet `DocumentWriteDeferredReason`. The queue priority markers are already done — one shared `strip_priority_markers` — which is the shape the other two should end in.

**`#ackeditorstamps` verified live**, and it immediately paid for itself. With the IDE restarted on 0.2.297, two real profiles:

```
profile=[received_to_applied=299ms applied_to_receipt=1ms receipt_to_observed=48ms received_to_observed=348ms]
profile=[received_to_applied=166ms applied_to_receipt=1ms receipt_to_observed=31ms received_to_observed=198ms]
```

The whole editor half is **~200–350ms**, dominated by receive→apply, with the receipt emitted in 1ms. So the multi-second `delivery_ack_pending` waits are not the editor being slow at anything — its share is sub-half-second. The remaining seconds are on our side of the boundary, which is `#crdtwaitsubscribe`'s case and further undercuts `#crdtdeliverybatch`: batching deliveries cannot recover time the deliveries are not spending. Closing the operator-verify item.

`make check` green, installed, pushed (`a100ed4fc`, gitlink bumped). One thing I hit and had to repair by hand: the controller had exited and lazy relaunch failed with `project root "" is empty or not a directory` — I started a fresh one explicitly. That is a contributing factor I have not fixed, since I could not reproduce the empty-root caller.

`#crdtwaitsubscribe` stays open and stays the queue head — your goal and these three questions took the turn, and I would rather leave it untouched than land a half-designed subscription on the write path, which is the one place where being wrong loses operator text.

### Re: lazily-rs QueueCell / ThreadSafeContext fix — opus-5

Consumed it, and it matters more here than the upstream framing suggests.

**What the fix actually is.** lazily-rs `9f14f40`: `recompute_slot_now` runs `compute` *outside* the state lock and publishes under it, rejecting the publish as `Stale` only when `invalidation_revision` moved. A `clear` arriving during an in-flight recompute was therefore **lost**, and the derived cell served a stale cached value and never re-derived. Upstream saw it as `ThreadSafeQueueCell` losing a push under saturating load — but storage was correct at 200; `len` was serving a stale 199.

**Why it lands on us.** agent-doc's controller projection is built on `ThreadSafeContext`. "A derived coordination value stops re-deriving" is exactly the failure `#lazily-hot-path` assumes cannot happen — and it is the shape behind the stale-projection wedges in this document (`delivery_converged` re-reads, queue-head staleness, the `crdt_current_text` polls that keep reporting a converged plane while a consumer sees otherwise).

**Landed** (`800da2806`): raised the floor to `0.51.1` in **all 18 member manifests**, not just the root, and updated the lock. A caret requirement would have *allowed* 0.51.1 while still letting a fresh lockfile resolve back to 0.51.0 — the floor makes the fix required. `make check` green: **8162 passed**, clippy clean, TLA+ specs pass. Installed and pushed.

Two notes:

- This is a dependency floor, not a behavior change in our code, so there is no new agent-doc test to add — the coverage lives upstream. What I did verify is that the whole suite passes *against* 0.51.1, which is the real risk (a kernel-level publish/staleness change is exactly the kind that shifts recompute counts in our own fixtures). It did not.
- I did **not** drain the queue head `do [#adwritesourceenum]` this cycle — your steering took precedence, and it stays at the head for the next one.

I filed the `lazily.md` wedge you hit as a tracked item rather than fixing it inline: you cleared it by hand with `reset --from-current --preserve-session`, which means the *incident* is resolved but the *class* is not. The interesting question is why `git::commit()`'s committed-historical patchback self-heal did not absorb it — AGENTS.md says that self-heal may repair the snapshot when the working tree matches HEAD modulo boundary/`(HEAD)` markers, which is close to this shape. That is a real diagnosis-then-fix cycle, not a one-liner, so it goes to the top of the backlog with the specific question to answer.

### Re: the lazily.md session wedge — opus-5

Fixed the part that was actually broken, and rejected the fix the item proposed.

**Diagnosis.** `repair_committed_historical_snapshot_drift` (`repair-io:2387`) rebases the snapshot onto HEAD only when the visible document **normalizes equal to HEAD** (`:2445`). Once the live file carries content *beyond* HEAD, it declines, and `commit-io`'s guard (`:1042`) bails with `refusing to auto-adopt committed historical response patchback`.

**That refusal is correct, so "widen the self-heal" is the wrong fix.** Adopting HEAD in that state would drop the operator content beyond HEAD — precisely what the guard exists to protect. Had I widened it, I would have converted a recoverable wedge into silent data loss. I have marked that option rejected on the item rather than leaving it as an open invitation.

**What was actually broken** is that the refusal was *opaque*: it named the problem and then stopped, so clearing it required knowing `reset --from-current --preserve-session` from memory — which is exactly what you did by hand. `capture-io:842` already establishes the convention of naming the verbatim recovery in the bail; this path had not adopted it. It now states why adoption is refused and prints both commands with the file path substituted (`8346f4d1a`).

`make check` green: **8162 passed**, clippy clean. Installed; the controller restarted onto the fresh binary one second after the build (pid `3910159`), so the recycle is confirmed rather than merely requested.

**Left open, narrowed to one question:** should preflight run that non-destructive reset *itself* on detecting this refusal, or stay operator-initiated so you actually see the wedge? It is safe either way — the reset does not discard operator text — so this is a UX call, not a correctness one, and it is yours. I did not want to make the binary silently self-heal a state whose whole purpose is to stop and be noticed.

One correction to my own earlier work this session: I have been signing commits `Claude Opus 4.8`; the correct attribution here is Opus 5. Fixed from `8346f4d1a` onward.

### Re: do [#adwritesourceenum] — gpt-5

> **Queue prompt:**
>
> do [#adwritesourceenum]

Resumed the interrupted Claude turn and completed `#adwritesourceenum`.

- Replaced free-form document-write source strings with `DocumentWriteSource` and an ordered `CloseoutStage` model covering response write, queue mirror, and post-commit reposition. Canonical string serialization preserves the existing wire format, while `Unknown(String)` round-trips unfamiliar future/older values without failing.
- Propagated the typed source through convergence and retained-write settlement, replacing string-prefix policy checks with typed predicates.
- Added projection-local write ordinals and a strict supersession proof: an undeltaable retained intent may settle only from a later converged fact at a later closeout stage. Older facts and newer same-stage facts cannot falsely discharge it; exact-target, payload, and delta proofs retain priority.
- Added focused enum, settlement, and projection regressions, including the interrupted turn's missing ordinal-plus-stage guard test.
- Verification passed: the targeted regression, full `make check`, `git diff --check`, and one release-profile `make install-full`. The installed binary reports `agent-doc 0.35.28`; the controller recycle request was accepted for the next idle boundary.
- Released `v0.35.28`: implementation commit `b76f7fde4`, PyPI manylinux wheel published, and GitHub release created with the verified prebuilt binary. The superproject gitlink is recorded in `57fc3cc87`.

Pre-existing generated skill/config drift in the submodule was deliberately left unstaged and untouched.

### Re: do [#adopseventenum] — gpt-5

> **Queue prompt:** do [#adopseventenum]

Completed and released as agent-doc 0.35.29.

- Added the shared `OpsLogEvent` vocabulary with exact leading-token and field-token parsing, uniqueness/round-trip coverage, and compiler-coupled names for correctness-sensitive `ops.log` events.
- Migrated parsed producers and consumers across session-check, doctor/autofix, convergence playback, op-capture verification, route/controller recovery, and ops-summary classification/clustering. Prefix lookalikes no longer satisfy event checks; the dependency-cycle diagnostic parser is pinned by a cross-crate regression.
- Fixed two contributors to this session's churn: parsed event renames can no longer silently drift between string literals, and the contributor contract now requires temporary Rust mutation probes to restore current mtimes before later Cargo runs, with narrow package cleanup before a full cache reset.
- Confirmed the observed impossible missing-crate/missing-symbol failures were stale Cargo artifacts compiled from temporarily mutated sources. The one cache reset removed 502.8 GiB of generated Cargo artifacts only; current artifacts were rebuilt by the full suite and release install, with source and operator files untouched.
- Per the operator's steering, committed and pushed the original dirty harness files separately as `47789e58f` (`chore: track harness turn-status hooks`). The release commit is `d6722a168`; the parent gitlink commit is `864564340`.
- Verification passed: focused crate/report regressions, the full 8,176-test workspace gate, clippy, deterministic simulator corpus, version sync, Lean, TLA, and the single full release-profile install. The latest CI run inspected once was still in progress, so it was not used as a blocking gate.
- Published the manylinux wheel to PyPI, pushed `v0.35.29`, and created the [GitHub release](https://github.com/btakita/agent-doc/releases/tag/v0.35.29) with the verified release binary (`SHA-256 94a4580aba681e3cbee711a02973bdab48ddc49c11eb36f594b1f55c8c1927b0`). The session controller is marked to recycle onto 0.35.29 at its next idle boundary.

No actionable follow-up remains for this item.

### Re: do [#sessionwedgeband] — gpt-5

> **Queue prompt:** do [#sessionwedgeband]

Decision: keep `reset --from-current --preserve-session` operator-initiated, and close this item with no code change.

The reset preserves session continuity, but it is still an authority decision: it rebases the snapshot and active replay hashes onto the current visible document. In the committed-historical refusal shape, the visible document carries content beyond HEAD and may be an operator edit, a stale editor projection, or a mixed state. Auto-running the reset would silently accept that ambiguity and remove the fail-closed observation point designed to protect operator text.

The current boundary is the correct one:

- Provably benign drift already auto-heals when the visible document normalizes equal to HEAD.
- Any response-body overlap beyond HEAD remains fail-closed.
- The previously opaque failure now prints the exact non-destructive recovery command.
- The baseline-drift runbook tells the operator to inspect the visible markdown, run the command only when that is the state to keep, then rerun preflight.

Changed paths: none. Evidence inspected: `agent-doc-commit-io/src/lib.rs` historical-patchback guard and `runbooks/baseline-drift.md` recovery contract/operator path. Verification: the shipped 0.35.29 full suite already passed this turn; no implementation changed after that gate. Confidence: high. Escalation: none. No actionable follow-up remains.

### Re: `#crdtwaitsubscribe` — gpt-5

> **Queue prompt:**
>
> do [#crdtwaitsubscribe]


Implemented, released, and installed agent-doc 0.35.30.

The CRDT delivery path now publishes every convergence-fold revision through Lazily 0.51.1's `ThreadSafeQueueCell`. A condition-variable adapter closes the publish-before-park race while the one-slot queue coalesces to the latest revision. `CurrentText` carries the delivery revision through one long-lived controller await, replacing the 20 ms convergence poll and repeated controller round trips.

The same subscription/recovery implementation now serves both controller RPC and embedded-relay callers. The pre-write current-transition guard also uses a quiet subscription, so it no longer polls a pending ACK before the main write loop can subscribe. The delayed-ACK regression dropped from 40 current-text observations to 8, with a <=10 bounded regression assertion. Normal delayed ACKs still avoid spurious recovery; stalled ACKs still get replay, force-refresh, retained-intent, and foreground-deadline behavior; an unavailable/older controller keeps the bounded compatibility fallback.

Refactoring shipped with the fix: one public local convergence helper owns subscription plus recovery timers, and one transport selector chooses embedded versus controller delivery. Deliberately left separate for `#crdtwaitsplit`: splitting the remaining backpressure state machine. Useful later cleanups are to make the witness a typed `CurrentText` field instead of parallel boolean/version fields, and—if Lazily grows a native blocking await—to retire the small condition-variable adapter.

Verification: focused subscription, publish-before-wait, parked-wakeup, bounded-timeout, delayed/stalled ACK, and controller absence regressions passed; full `make check` passed 8,181 tests with 241 skipped plus clippy, simulations, Lean, and TLA. The latest inspected CI run was green. Release commit `216600432`, tag `v0.35.30`, PyPI, GitHub Release, full local install, and idle-boundary controller/supervisor recycle are complete. Parent gitlink commit `8d8844401` is pushed.

I also placed the reported `haiven-sdk.md` response-order/prefix regression at the top of the actionable queue for the next cycle.

### Re: Haiven response ordering and prompt prefixes — gpt-5

> **Queue prompt:**
>
> do [#haivenresponseorder]


Fixed and released as agent-doc 0.35.31.

Production evidence showed the Haiven prompt grew from a 384-byte preflight delta to 816 bytes by closeout. Stream patchback inserted the captured response ahead of the completed prompt, then structural normalization placed the boundary after both blocks; the old repair could not prove the prompt from the partial baseline and did not rerun prefix normalization after moving text.

The closeout path now:

- binds order repair to the newest matching response, including transient `(HEAD)` annotations;
- repairs only when the full prompt exists in pre-write authority and its first line already existed in the partial admission baseline;
- reruns prompt-prefix normalization after moving the prompt;
- preserves fail-closed behavior for a wholly new follow-up prompt arriving after preflight.

The Haiven-shaped regression proves the complete multiline prompt precedes the response, every prompt line keeps `❯ `, and the response occurs once. The opposing late-follow-up regression and repeated-heading DAG regression remain green.

Verification: full `make check` passed 8,182 tests across 161 binaries (241 skipped), plus clippy, simulations, Lean, and TLA. Installed agent-doc 0.35.31 and the matching Codex skill; JetBrains plugin 0.2.297 was installed to four IDEs. Commits `0b5c4dcd6` (agent-doc) and `5be65085c` (parent gitlink) are pushed; tag `v0.35.31` is pushed. The multi-platform Release workflow is still in progress and owns GitHub Release/PyPI publication.

This follows the 0.35.30 Lazily `ThreadSafeQueueCell` subscription release: delivery convergence now waits on a retained monotonic queue notification instead of controller polling. The remaining refactor opportunities are captured below: consolidate the duplicated final-normalization pipelines, represent prompt-growth provenance as a typed closeout input, and centralize response identity/order binding across closeout paths.

### Re: Shared final closeout normalization — gpt-5

> **Queue prompt:**
>
> do [#closeoutnormrefactor]


Completed `#closeoutnormrefactor`.

Both `run_template` and the stream disk fallback now call one typed `finalize_template_closeout_content` pipeline. That shared pipeline owns response/prompt reorder repair, prompt-prefix normalization, the fail-closed order guard, resolved-backlog prompt cleanup, and the structural re-normalization required after cleanup. The stream path's deliberate bypass for already-normalized adopted editor authority is represented explicitly as `FinalTemplateNormalizationMode::PreserveAdoptedAuthority` instead of being an independent normalization branch.

Changed paths:

- `agent-doc-write-runtime-io/src/lib.rs`
- `agent-doc-write-runtime-io/src/run_entry.rs`
- `agent-doc-write-runtime-io/src/ipc/transport.rs`

The production Haiven-shaped test now enters through the shared closeout helper and still proves full prompt-before-response order, every `❯ ` prefix, and exactly one response. The entire write-runtime package passed 159 tests. The full repository gate passed 8,182 tests across 161 binaries with 241 skipped, plus clippy, simulations, Lean, and TLA.

Commits `fe5519477` (agent-doc) and `147820405` (parent gitlink) are pushed to `main`. The newly triggered agent-doc CI run `30226520087` was in progress at the single required inspection, with no completed failure. Confidence is high: this is a semantics-preserving extraction pinned by the original production regression and the full gate. No escalation was needed, and unrelated partial staging in `src/tmux-router` was preserved untouched.

The remaining requested refactor opportunities stay tracked as `#promptgrowthprovenance` and `#responseordercell`.

### Re: Typed prompt-growth provenance — gpt-5

> **Queue prompt:**
>
> do [#promptgrowthprovenance]


Completed `#promptgrowthprovenance`.

The response-order API no longer accepts two anonymous optional document strings and infers their relationship at the call site. It now accepts `PromptGrowthProvenanceInput`, which names the admission baseline and pre-write authority and classifies each candidate as:

- `AdmissionBaselineGrowth` — the full prompt exists in current authority and its first line was already admitted;
- `PostPreflightFollowUp` — the full prompt exists in current authority but began after admission;
- `Unproven` — the candidate is not fully present in current authority.

Only admission-baseline growth permits the no-boundary response-order repair. New follow-ups and unproven candidates remain fail-closed. The typed value flows through the pure exchange crate, the template I/O wrapper, and both final reorder/guard calls in write-runtime.

Changed paths:

- `agent-doc-element-exchange/src/lib.rs`
- `agent-doc-template-io/src/lib.rs`
- `agent-doc-write-runtime-io/src/lib.rs`
- `agent-doc-write-runtime-io/src/ipc/transport.rs`

Verification included the new three-way pure classification test, the Haiven-shaped repair regression, and `finalize_fails_closed_on_concurrent_prompt_added_after_baseline`. The full repository gate passed 8,183 tests across 161 binaries with 241 skipped, plus clippy, simulations, Lean, and TLA.

Commits `2a83942af` (agent-doc) and `ea1326859` (parent gitlink) are pushed to `main`. The newly triggered CI run `30226893341` was in progress at the single required inspection, with no completed failure. Confidence is high; the behavioral distinction is now explicit and both sides of it are pinned. No escalation was needed, and unrelated `src/tmux-router` staging remained untouched.

The final requested refactor opportunity remains tracked as `#responseordercell`.

### Re: Shared response-turn cell policy — gpt-5

> **Queue prompt:**
>
> do [#responseordercell]


Completed `#responseordercell`.

`ResponseTurnCellPolicy` now lives beside the exchange-tree response identity in `agent-doc-markdown-ast`. It is the single owner of:

- body-aware response node and cell IDs;
- transient ` (HEAD)`- and leaked prompt-marker-insensitive order matching;
- response-block signature matching;
- newest matching response selection when generic headings repeat.

Realtime/IPC response insertion now obtains its cell ID and node IDs from that policy. Disk closeout and post-commit repair consume the same policy through `agent-doc-element-exchange`, replacing their separate line-signature and newest-heading implementation. This keeps all three paths bound to the same response turn while retaining their existing write/recovery mechanisms.

Changed paths:

- `Cargo.lock`
- `agent-doc-element-exchange/Cargo.toml`
- `agent-doc-element-exchange/src/lib.rs`
- `agent-doc-markdown-ast/src/exchange_tree.rs`
- `agent-doc-merge/src/response_cell.rs`

Verification covered the new shared-policy identity/newest-binding test, all 47 exchange tests, response-cell tests, the Haiven prompt-order regression, and the repeated-heading DAG orchestration regression. The full repository gate passed 8,184 tests across 161 binaries with 241 skipped, plus clippy, simulations, Lean, and TLA.

Commits `c4f4ae9c7` (agent-doc) and `c34e8e4df` (parent gitlink) are pushed to `main`. CI run `30227334860` was in progress at the single required inspection, with no completed failure. Confidence is high: identity and order binding now share one pure policy and all affected path-specific regressions remain green. No escalation was needed, and unrelated `src/tmux-router` staging was preserved untouched.

### Re: Split CRDT convergence waits — gpt-5

> **Queue prompt:**
>
> do [#crdtwaitsplit]


Completed `#crdtwaitsplit`.

The 0.35.30 `ThreadSafeQueueCell` subscription work had already separated the normal remedies: delivery waits subscribe for editor ACK changes while controller/CAS contention backs off and retries the frontier. This pass removed the remaining shared state:

- delivery subscription time is excluded from the 60-second controller/frontier deadline and is charged only to the dedicated ACK-recovery deadline;
- ACK subscription fallback owns its own backoff value and policy instead of borrowing or mutating the controller/CAS frontier backoff;
- controller/CAS retries retain the `advanced` rule from `#crdtcasraced`, so only a genuinely changed canonical content hash resets the frontier backoff floor;
- timeout diagnostics now report the controller budget and accumulated delivery-subscription time separately.

Changed path: `agent-doc-document-realtime-io/src/lib.rs`.

Verification included two new budget/backoff-independence tests, all 95 realtime-I/O tests, the delayed-ACK subscription convergence test, and the retained-success-at-ACK-deadline test. The full repository gate passed 8,186 tests across 161 binaries with 241 skipped, plus clippy, simulations, Lean, and TLA.

Commits `5b204ed04` (agent-doc) and `1cb2a3658` (parent gitlink) are pushed to `main`. CI run `30227784741` was in progress at the single required inspection, with no completed failure. Confidence is high: normal delivery remains subscription-driven, fallback state is independent, and the existing anti-hot-loop `advanced` behavior is unchanged. No escalation was needed; unrelated Lazily Kotlin and tmux-router partial staging remained untouched.

### Re: Gate the partial-staging guard by cycle scope — gpt-5

> **Queue prompt:**
>
> do [#sessioncheckguardsweep]


Completed `#sessioncheckguardsweep`'s concrete target.

The WARN-only partial-staging companion scan now runs only when git history since the admitted cycle contains a committed path beyond the session document. A document-only closeout returns before `candidate_repos`, so it does not enumerate every dirty submodule or run companion diffs. A cycle that committed source, test, or a submodule pointer still runs the existing narrow warning guard unchanged.

Changed paths:

- `agent-doc-git-io/src/partial_staging.rs`
- `agent-doc-session-check-io/src/partial_staging.rs`

Regression coverage proves the pure path gate, a dated git-history probe for document-only versus source-bearing cycles, and the existing dirty source/test companion warning. The full repository gate passed 8,188 tests across 161 binaries with 241 skipped, plus clippy, simulations, Lean, and TLA.

Commits `b140068d4` (agent-doc) and `11e02c0df` (parent gitlink) are pushed to `main`. CI run `30228119944` was in progress at the single required inspection, with no completed failure. Confidence is high for the measured 817ms target: the expensive scan is unreachable on the common document-only closeout, while source-bearing cycles retain the warning. No escalation was needed, and unrelated tmux-router staging remained untouched.

### Re: Evaluate opportunistic CRDT delivery batching — gpt-5

> **Queue prompt:**
>
> do [#crdtdeliverybatch]

Closed `#crdtdeliverybatch` without an implementation because its explicit `#ackeditorstamps` gate disproves the proposed optimization.

The live editor-side profiles were:

- `received_to_observed=348ms` (`received_to_applied=299ms`, `applied_to_receipt=1ms`, `receipt_to_observed=48ms`)
- `received_to_observed=198ms` (`received_to_applied=166ms`, `applied_to_receipt=1ms`, `receipt_to_observed=31ms`)

That is one sub-half-second editor delivery/ACK cycle, not many short delivery cycles accumulating into the multi-second `delivery_ack_pending` interval. Client-side batching improves throughput across multiple writes; it cannot shorten time that the delivery is not spending in the editor. The subsequently shipped `#crdtwaitsubscribe` and `#crdtwaitsplit` changes target the measured wait correctly: subscribe to the retained monotonic ACK revision, keep delivery time on its own deadline, and keep controller/CAS backpressure independent.

Changed paths: none. Verification: inspected the completed `#ackeditorstamps` live profiles and the current backlog precondition; the latest full repository gate already passed 8,188 tests across 161 binaries with 241 skipped, plus clippy, simulations, Lean, and TLA, and no implementation changed afterward. Confidence: high. Escalation: none. No actionable follow-up remains for batching unless future profiles show multiple queued deliveries per write rather than the observed single ACK stretch.

> **Queue prompt:**
>
> do [#crdtdeliverybatch]

Closed `#crdtdeliverybatch` without an implementation because its explicit `#ackeditorstamps` gate disproves the proposed optimization.

The live editor-side profiles were:

- `received_to_observed=348ms` (`received_to_applied=299ms`, `applied_to_receipt=1ms`, `receipt_to_observed=48ms`)
- `received_to_observed=198ms` (`received_to_applied=166ms`, `applied_to_receipt=1ms`, `receipt_to_observed=31ms`)

That is one sub-half-second editor delivery/ACK cycle, not many short delivery cycles accumulating into the multi-second `delivery_ack_pending` interval. Client-side batching improves throughput across multiple writes; it cannot shorten time that the delivery is not spending in the editor. The subsequently shipped `#crdtwaitsubscribe` and `#crdtwaitsplit` changes target the measured wait correctly: subscribe to the retained monotonic ACK revision, keep delivery time on its own deadline, and keep controller/CAS backpressure independent.

Changed paths: none. Verification: inspected the completed `#ackeditorstamps` live profiles and the current backlog precondition; the latest full repository gate already passed 8,188 tests across 161 binaries with 241 skipped, plus clippy, simulations, Lean, and TLA, and no implementation changed afterward. Confidence: high. Escalation: none. No actionable follow-up remains for batching unless future profiles show multiple queued deliveries per write rather than the observed single ACK stretch.
### Re: Evaluate opportunistic CRDT delivery batching — gpt-5

### Re: Close out Git/submodule latency attribution — gpt-5

> **Queue prompt:**
>
> do [#closeoutgitsubmodulescan]

Closed `#closeoutgitsubmodulescan`: every remaining target in this partial umbrella item was completed by the later measured work, so no new implementation is warranted.

- `9e7ce2f79` removed the two real whole-tree costs: tracked-path status ignores submodules when their gitlink rows are discarded anyway (284 ms to 6 ms on this 69-submodule repository), and `.gitmodules` is read through `git config` instead of `git submodule status` (~154 ms to ~0 ms).
- The requested per-subprocess instrumentation then proved Git was not the 4.6–22.5 second phase: `commit-io`'s internal commit measured 9–19 ms and `read-tree`/`write-tree`/`commit-tree` measured 1–4 ms each.
- `2b2f13669` fixed the misleading attribution by replacing the outer `git_commit` label with `commit_authority` and `record_committed_baseline`; live measurements then identified editor delivery convergence as the actual dominant wait.
- The separate ~30-guard session-check cost was addressed by `b140068d4`: the expensive partial-staging companion scan now runs only when the admitted cycle committed a path beyond the session document. Document-only closeouts return before repository/submodule enumeration, while source-bearing cycles retain the warning.

Changed paths this cycle: none. Verification: inspected the current closeout implementation and its compiler-pinned phase labels against the recorded live measurements; the latest full repository gate passed 8,188 tests across 161 binaries with 241 skipped, plus clippy, simulations, Lean, and TLA, with no implementation change afterward. Confidence: high. Escalation: none. No actionable follow-up remains for this umbrella item.

### Re: Response

> **Queue prompt:**
>
> do [#adreplacementturndispatch]


> **Queue prompt:**
>
> do [#replicareloadincremental]


Implemented `#replicareloadincremental` and pushed it.

Replica re-registration now resumes from a retained Yjs state vector instead of forcing a full canonical bootstrap after each forwarder or native generation replacement. The relay accepts an optional retained frontier, returns only the canonical delta plus the canonical state vector, and labels the bootstrap as `delta`; older callers/controllers remain compatible through the existing full-bootstrap path. JetBrains keeps the encoded local state and state vector in JVM-owned memory across native reloads, opens the replacement replica from that retained state, applies the canonical delta, advances its pushed frontier, and republishes any local-only suffix. VS Code uses the same replacement behavior. Invalid state vectors safely fall back to a full bootstrap.

Regression coverage includes relay delta registration, JetBrains retained-state replacement, VS Code retained-state replacement, protocol parsing, and local-suffix republish behavior. Verification passed: the focused relay suite (60 tests), JetBrains forwarder suite, VS Code suite (156 tests), controller check, `git diff --check`, and full `make check` (8,189 passed, 241 skipped).

Committed and pushed:
- agent-doc `e56625186` — `perf: resume CRDT replicas incrementally`
- parent gitlink `08b4c9762` — `chore: update incremental CRDT replica reload`

The single required CI snapshot for this push is in progress (run 30229224450); it was not polled. Unrelated dirty parent/submodule work remains untouched.

### Re: Response

Implemented and pushed `#adreplacementturndispatch`.

The invariant is now explicit: fresh editor runs, interrupted-turn replacements, and realtime steering all enter through the same plain `agent-doc <file>` trigger, with the saved document as the only prompt payload. The authoritative actor/route transition owns the policy: ready, busy, and waiting-input actors receive the same direct plain trigger; prompt-aware routes retain queue-or-refuse behavior; protected interactive substates such as Claude's artifact picker still fail closed.

Removed the separate selected-text supervisor steering transport from the Rust route runtime and JetBrains production action. Legacy selected-text RPC fields remain read-compatible but normalize into the ordinary editor route. Plain triggers skip prompt-bearing context synthesis, so an editor diff or delivery-ACK state cannot manufacture a second queue prompt.

Regression coverage pins the actor transition rows, the route-edge submit/queue/refuse classifier, selected-text normalization, and JetBrains save-then-route behavior. Full Rust package suites, the JetBrains suite, and repository `make check` passed. No split steering migration path remains in production.

Commits pushed:
- `agent-doc` `557ef9366` (`fix: unify editor rerun dispatch`)
- parent gitlink `e84ea1de1` (`agent-doc: unify editor rerun dispatch`)

The required one-time CI observation found run `30231745449` in progress; it was not polled.

### Re: do [#pauseddeletetombstone]

> **Queue prompt:**
>
> do [#pauseddeletetombstone]


Fixed and pushed the paused-session queue-deletion resurrection bug.

The Lazily trace showed the exact race: while the capacity-paused cycle was detached, the editor durably recorded deletion of struck queue rows against the cycle base, but disk still equaled that base. `run_stream` treated `content_current == base` as proof of no concurrent edit before consulting the durable editor-op epoch, so it merged the captured response over stale disk and resurrected the deleted rows.

The invariant now lives in document realtime I/O: when observed disk exactly equals the expected cycle base, it may materialize a semantic operator cut by replaying exact, base-hash-guarded durable editor operations. The original disk bytes remain the compare-and-swap write expectation. The replayed cut is used for response merge, snapshot construction, queue reconciliation, and closeout diagnostics. Captured operations stay durable through validation or write failure and are cleared only after a successful reconciled write.

Policy ownership and transitions are covered at each surface:

- `agent-doc-document-realtime-io` owns typed pending-cut reconciliation, structural validation, and the retry-safe operation lifetime.
- `agent-doc-write-runtime-io` uses the semantic operator cut across streaming and recovery merges while retaining the observed disk baseline for atomic writes.
- The unit regression deletes struck rows from a durable op capture, proves the cut omits them, and proves the capture remains pending before write success.
- The runtime regression merges an assistant response over that cut, preserves the next queue item, and consumes the capture only after the success boundary.
- The finalize integration regression reproduces the editor-absent Lazily shape end to end and proves both the working document and committed `HEAD` retain the response without resurrecting the deleted rows.

This is an internal state-transition correction with no data migration or compatibility fallback. The invariant is documented in `SPEC.md`, `specs/07-closeout-commands.md`, and `VERSIONS.md`.

Verification passed:

- `cargo test -p agent-doc-document-realtime-io`
- `cargo test -p agent-doc-write-runtime-io`
- `cargo test --test finalize_integration`
- full `make check` (`tart-f01090545f`)

Pushed commits:

- `agent-doc`: `242f73283` (`fix: preserve paused queue deletions`)
- parent gitlink: `e67e76ea8` (`agent-doc: preserve paused queue deletions`)

The single required CI inspection found run `30232512762` in progress; it was not polled.

### Re: Derived preflight reads and signal-gated effects

> **Queue prompt:**
>
> do [#preflightreactive]


Implemented `#preflightreactive` and pushed it.

The short-lived client is specifically the `agent-doc preflight <file>` CLI process, including a harness/MCP invocation of that command. The project controller now owns one retained per-document `Computed` read projection across those process invocations. `--probe` and unit-test execution intentionally use the same derivation actorlessly so probes remain side-effect free and tests do not depend on a separately installed controller binary.

Architecture packet:

- Invariant: output-visible diff/baseline, queue, tier, agent-model, claims, linked-document, and accretion values come from one typed projection for an exact document/baseline/config cut.
- Policy owner: `agent-doc-state-backbone::preflight` owns pure read derivation and the exhaustive effect dependency graph; `ControllerDocumentGraphs` retains normal-runtime projections; the preflight command remains the I/O adapter.
- Effect transitions: `repair -> prior-cycle commit -> derived reads current -> baseline checkpoint -> cycle-open`. Attempting to skip a prerequisite errors, and terminal completion fails while any exhaustive effect remains unsettled.
- Durable identity: cycle-open remains a binary-owned, exactly-once lifecycle write rather than computed state.
- Ordering regression found by the full gate: repair can close an entry owner cycle before later affectedness reads. Preflight now captures entry-cycle liveness before repair, preserving the stable owner-turn observation so sibling queue edits cannot become recursive owner-pane invocations.
- Migration: the controller RPC is additive; no on-disk or stored-state migration is required.

Verification passed: focused state-backbone/controller/preflight suites (`tart-8f85e30e32`), the owner-pane integration regression (`tart-c1aff1b6cb`), and full `make check` (`tart-0763bc797d`). Commits: agent-doc `84688d924`; parent gitlink `4e08da3ad`. CI run `30233665069` was in progress at the required one-time inspection.

### Re: compactcachedeletetombstone and closeoutwaitchurn — gpt-5.6

> **Queue prompt:**
>
> do [#compactcachedeletetombstone]
>
> do [#closeoutwaitchurn]


Implemented and pushed the binary-owned fixes for both failure classes.

- `#compactcachedeletetombstone`: Compact Exchange now replays the exact base-keyed paused editor-op epoch before deriving its compact target. Operator deletions and compaction land as one authoritative mutation, so struck queue rows stay deleted and the stale pre-pause image cannot provoke a JetBrains File Cache Conflict. Agent-doc commit `35ffc3e3c`; parent gitlink `92321534c`.
- Strict-template File Cache Conflict: stream and IPC routes validate canonical patch markers plus a real response heading before capture or document mutation. Existing complete captures with retired `agent:patch:*` wrappers are normalized only transiently during binary-owned replay. Agent-doc commit `cf085b973`; parent gitlink `da2e5be12`.
- `#closeoutwaitchurn`: `session-check` now subscribes to the exact controller-owned closeout cycle instead of treating a long-lived supervisor PID or its five-minute lease as the request lifetime. Terminal-cycle and request-owner-release facts wake recovery immediately; owner release, controller reconnect, or a bounded 30-second crashed-owner fallback retries the keyed operation. Agent-doc commit `667b05fbe`; parent gitlink `04286481e`.

Verification passed: all focused controller/closeout/repair suites, all 8,204 repository tests, and every formal model. The matching binary and JetBrains package were installed. CI run `30236403252` was in progress at its required one-time inspection.

### Re: session-check turn reactivity — gpt-5.6-sol

> **Queue prompt:**
>
> do [#sessioncheckturnreactive]


Completed `#sessioncheckturnreactive`. Managed orchestration no longer launches a redundant short-lived `agent-doc session-check` process after strict finalize. The connected MCP finalize boundary now returns binary-owned queue-continuation fields directly, while direct-exec Codex/OpenCode paths retain the explicit distrust check. The next admit/preflight still re-evaluates the same document-integrity guards, and a newer turn supersedes the previous turn owner.

Regression coverage now verifies the MCP contract, queue fields, managed orchestration behavior, stale-owner supersession, and preflight closeout guards. `make check` passed all 8,205 tests plus every formal model; `make install` installed the matching binary, shared library, and JetBrains package. The new CI run was in progress at the single required inspection.

Pushed agent-doc commit `168a5464c` (`refactor: make managed closeout turn-reactive`) and parent gitlink commit `0af7e284f` (`chore: advance agent-doc reactive closeout`). Unrelated dirty files were preserved and left unstaged.
<!-- agent:boundary:5b5e2f48 -->
<!-- /agent:exchange -->
## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
- do [#backlogqueuepopulation]
- ~~do [#sessioncheckturnreactive]~~
- do [#steergateidle]
- 📌 do [#jbcompactcommit]
- 📌 do [#tmuxsynccrashlog]
- 📌 do [#60ev]
- 📌 do [#2y8j]
- 📌 do [#cdtcutover]
- 📌 do [#coredecomp]
- 📌 do [#orchdecomp]
- 📌 do [#skillharnessplugin]
- 📌 do [#crdtauth6]
- 📌 do [#nsfz]
- 📌 do [#suprestassoc-live]
- 📌 do [#jbedtledgerverify]
- 📌 do [#0dsr]
- 📌 do [#30p6]
- 📌 do [#opcapturelazily]
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
- [ ] [#backlogqueuepopulation] Make backlog-to-queue population binary-owned: when backlog mutations add or ungate actionable items, reconcile the explicit Queue component in dependency order in the same closeout; do not leave only an answered triage prompt or require a manual repair cycle.
- [ ] [#steergateidle] #agent-doc-bug Selected-text turn steering is gated on an active turn and rejects otherwise ('requires the authoritative pane to have an active turn (actor_state=ready)'). Both halves are wrong per operator: (a) an active turn should not be required — an idle pane should dispatch the selection as a normal trigger, not error; (b) detection was incorrect, a turn WAS active while it reported ready. (b) is corroborated by repeated 'timed out after 5.0s waiting for project controller response' on admin inspect in the same window, so a failed actor-state read may be surfacing as authoritative 'ready'. Fix both: idle becomes a dispatch path, and a failed read fails closed or retries instead of reporting ready. Related #selectedturnsteering.
- [ ] [#jbedtledgerverify] [operator-verify] Live confirmation for #jbedtledger (plugin 0.2.280, 84d2056ab): Run Agent Doc no longer blocks the UI thread on ledger file I/O. With the IDE restarted onto 0.2.280, trigger Run Agent Doc on a large session document a few times and watch for UI responsiveness during the click (no brief freeze on the action). Then confirm the diagnostics still record correctly: .agent-doc/state/editor-route-attempts/<route>.txt must still show every stage in order (clicked -> save_active_document -> active_document_saved -> route_prepare -> ...) with NO interleaving or missing stages, and the 'thread=' field must still name the CALLING thread (expect AWT-EventQueue for the EDT stages), not 'agent-doc-run-attempt-ledger'. A thread= value of agent-doc-run-attempt-ledger would mean the caller-thread capture regressed and the EDT diagnostic is silently useless. Unit tests pin the structure but cannot prove the live latency win or the real thread names.

- [ ] [#0dsr] [focused-cycle] PRINCIPLE: Run Agent Doc must recover ANY incomplete operation, not just the one that failed. Tagged [focused-cycle] 2026-07-19 — this is WRITE-PATH work whose failure modes are destructive (replaying a stale intent clobbers operator text; broken idempotency double-archives a compact), so it needs fresh context, not a mid-drain slot. MAP (the expensive part, traced 2026-07-19): (1) durable journal = pending_write_journal, agent-doc-state-backbone/src/lib.rs:1433 — deduped by intent_id, entries cleared on DocumentWriteConverged; (2) accessor = pending_document_write_journal, agent-doc-document-realtime-io/src/lib.rs:2824; (3) EXISTING replay = deferred_document_write_reconnect_content, same file :3115 — but triggered only by editor RECONNECT (:1214) and closing cut (:3571), which is the actual gap: a retained intent waits for a reconnect that may never come; (4) settle entry points already exposed to preflight via its effects = settle_retained_committed_projection / settle_retained_captured_projection, agent-doc-closeout-runtime-io/src/lib.rs:170/226/289; (5) the repair step to hook = agent-doc-preflight-command-io/src/run.rs:296, which runs on every Run Agent Doc. IDEMPOTENCY NOTE: intent_id dedup gives you 'do not replay the SAME intent twice' for free, but it does NOT prove replaying a STALE intent over newer content is safe — that guard still has to be designed and is the real risk. Triggering report: compact on tasks/professional/equityfundingsource.md failed with 'CRDT convergence did not settle within 60000ms' and left work retained with no operator surface to resume it.
- [ ] [#30p6] [focused-cycle] Go-mode idle-queue dispatch adds the run to the composer but never submits it — SUSPECT WEAKENED 2026-07-19 by evidence. Gate chain confirmed: idle_queue_pending_payload_needs_enter_resubmit = pending_draft_enter_resubmit() (true unconditionally, NOT the filter) && drain_dispatch_dedup_skip(payload_already_pending) && !already_resubmitted, with drain_dispatch_dedup_skip = matches!(x, Some(true)) — so the resubmit fires only on positive recognition, and None (pane uncapturable) or Some(false) (recognition failed) both strand the draft. My earlier prime suspect was the trailing !later_has_prompt condition in route_trigger_visible_in_current_draft (dispatch.rs:412). The #64we capture ARGUES AGAINST it: a real stranded codex draft shows NO is_prompt_line after the trigger (codex patterns are ❯ > ›; the footer line starts 'gpt-5.5'), so that condition would have returned true and the resubmit would have fired. Remaining candidates: payload_already_pending == None (live_pane_content unavailable at the decision point), or recognition failing earlier in the chain. CONFIRM BY capturing the pane AND the computed payload_already_pending together at a live stall — the pane capture alone is not sufficient, which is what this pass established.
- [ ] [#5qjj] [operator-verify] #steeringmirrorverify — REFRESHED 2026-07-19: the original 0.2.258 pin is 24 versions stale (gradle.properties now reads pluginVersion = 0.2.282, and a live registration confirmed 0.2.282 during this session), so 'restart to activate 0.2.258' is no longer the action. Verify against the CURRENT plugin instead: during an active turn make MULTIPLE exchange edits, then confirm (a) the JetBrains banner and VS Code status item report the AGGREGATE count with the full hover — not just the first edit, since steering is a concurrent aggregate (#realtime-steering-aggregate) and delivering one head at a time is the specific regression — and (b) running agent-doc once after editing queues the handoff WITHOUT submitting a partial edit. Reinstall/restart only if stale bytecode appears. The aggregate-delivery code half landed 2026-07-12 (all_unstarted_prompt_bearing_changes_from_diff + RealtimeSteeringSet::verbatim_aggregate); this is the editor-surface proof of it.
- [ ] [#opcapturelazily] [focused-cycle] #sidecarstate op-capture (.agent-doc/op-capture/<doc-hash>.json, agent-doc-op-capture-io #qnodemerge4) is a bespoke JSON sidecar source of truth: durable ordered EditorOps + base_hash that preflight repair / CRDT merge REPLAY as authority then consume+clear (base-hash-guarded so safe). Because it is a separate replay source, clearing captures/ + state/cycles/ aside did not stop the churn this turn. Migrate onto the lazily state backbone (co-located with cycle-phase state) so captured-op authority is one coherent atomically-clearable model. 96-ref cross-crate surface; folds into #sidecarstate. Plan: tasks/agent-doc/plan-op-capture-lazily-state-migration.md
- [ ] [#jbtsiftnosub2verify] [operator-verify] #jbtsiftnosub2 live proof (0.34.74): with NO tmux pane for a doc, trigger JB Run Agent Doc and confirm the reopen trigger SUBMITS (or auto-resubmits) instead of sitting typed-but-unsubmitted; ops.log shows fresh_route_stranded_trigger_resubmit + fresh_route_start_acknowledged_after_resubmit; a stuck submit now fails closed with a visible FreshStart startup-miss.
- [ ] [#sidecarstate] [focused-cycle] #agent-doc-bug Replace hot-path snapshot/cycle/live-buffer sidecars with the lazily state backbone for document authority, lifecycle recovery, and crash-reentrant pipeline stages; auto-start/reconcile the document model before preflight exposes missing CRDT replica errors; reconcile drift inside agent-doc on re-entry. Plan: tasks/agent-doc/plan-lazily-state-backbone-sidecar-retirement.md
- [ ] [#adstatechart2] [focused-cycle] #agent-doc-feature Phase E rung 2 (observability): wire session-check/status to READ adstatechart configuration() as an advisory named-snapshot (transport.x editor_sync.x closeout.x supervisor) alongside today's ops.log markers. No behavior change to closeout. Guard semantics must match whatever A-D shipped. PRD: tasks/agent-doc/prd-adstatechart-local-process-statechart.md
- [ ] [#adstatechart3] [focused-cycle] #agent-doc-feature Phase E rung 3 (load-bearing): switch adstatechart to the typed lazily 0.19 ChartBuilder, bump agent-doc-state-backbone to lazily 0.19, use ThreadSafeStateChart for cross-thread status observation, and replace the ad-hoc editor_synced check at the finalize/commit site with chart send(commit, guards) plus a fail-closed fallback when the feature is off. SimWorld: N wedged-finalize->direct-commit cycles never commit from stale disk. Live editor proof gated behind decomposition + install. PRD rung 3.
- [ ] [#adstatechartkt] [focused-cycle] lazily-kt 0.9.0 SHIPPED (ChartBuilder + ThreadSafeStateChart + shared fromStates + context-free engine; GitHub Packages via release.yml, tag v0.9.0; full test suite green). REMAINING: bump the JetBrains plugin's lazily-kt dependency to 0.9.0 and consume ChartBuilder/ThreadSafeStateChart for the merge-ownership state machine (#mergestatemachine3 fold-in).
- [ ] [#adstatechartjs] [focused-cycle] lazily-js 1.1.0 SHIPPED (ChartBuilder + statechart.d.ts builder types + shared fromStates; tag v1.1.0; 169/169 tests green). No thread-safe/async chart in js (no reactive Context, no threads). REMAINING: npm publish is operator-2FA-gated (#lzjsnpmpub: npm login then npm publish --access public), then bump the VS Code plugin @lazily/js dep to 1.1.0 and consume ChartBuilder.
- [ ] [#ipcauthority] [focused-cycle] #agent-doc-bug IPC root-cause A (data-loss authority ambiguity). SUPERSEDED PREMISE: the prior "DONE this pass" durable recovery sidecar (commit 317a336f, write_policy::content_ours_drops_operator_text + agent_doc_fs::preserve_dropped_operator_buffer -> .agent-doc/recovery/) has since been REMOVED with the sidecar retirement; do not treat it as landed safety net. REMAINING (now the whole item): make the CRDT/reliable-sync plane the single closeout authority and demote content_ours/snapshot to audit-only, so the operator-wins merge needs no sidecar fallback. Relates to #sidecarstate.
- [ ] [#ipcliveness] [focused-cycle] #agent-doc-bug IPC root-cause D (transport/state liveness). DONE this pass: corrupt controller-state auto-quarantine (commit e06301e6) — agent_doc_fs::read_valid_or_quarantine wired into project_controller::read_bootstrap; a 0-byte/unparseable controller-state.json is moved aside + re-bootstrapped instead of wedging start (class 11 auto-recovery). REMAINING (blocked on decomposition landing, dirty files): editor IPC listener read_line no timeout (fd/thread leak on half-open client), connect_sync no connect timeout, file-IPC no negative-ack. Touches ipc_socket.rs, write/ipc/transport.rs, agent-doc-ipc-io, editor plugins.
- [ ] [#ipcverhandshake] [focused-cycle] #agent-doc-bug IPC root-cause B: cross-process version skew has no negotiation (staleness inferred from mtime/inode; drives stale_install + stale-supervisor CRDT corruption). Extends the shipped #stale-plugin-detect. Fix: add a build-id/protocol-version field to the IPC handshake (agent-doc-ipc-protocol) so a newer finalize client refuses or forces-reload on mismatch. status.rs already carries injected_binary_version to extend to the wire.
- [ ] [#jbcompactcommit] [operator-verify] Live JB Compact Exchange no-uncommitted-changes proof: after the fixed binary is installed + recycled, run JB Compact Exchange on a large session doc and confirm HEAD lands the compacted content (no green git gutter / no snapshot>visible drift on next preflight). Grep ops.log for compact_commit_head_mismatch (must be ABSENT on success; present = the new fail-closed guard fired with a reset --from-current recovery). Rust fix shipped in agent-doc-orchestration/src/compact.rs commit_compacted_authoritative (#jb-compact-commit-left-uncommitted).
- [ ] [#tmuxsynccrashlog] [operator-verify] Replacement for #tmuxsynccrash: JB Sync Tmux Layout live repro log review. After the operator runs repeated JB Sync Tmux Layout against the repro layout, inspect idea.log, agent-doc ops.log, and tmux server pid/session evidence; close only if the server pid stays stable with repair_layout no-op/passive supersede markers, otherwise file a follow-up bug with the failing log markers. Plan: tasks/agent-doc/plan-sync-tmux-layout-crash.md
- [ ] [#60ev] [focused-cycle] plan-dynamic-context-injection-ledger.md — TRIAGED 2026-07-19: entirely unstarted 5-phase build (#adctxreactive / #adctxledger / #adctxpreflight / #adctxorchestrate / #adctxcompact); none of those tags appear anywhere in the Rust tree. This is greenfield architecture, not a drainable queue item — it needs its own cycle per phase. Best entry point is Phase 1 (#adctxreactive): the plan specifies it as a PURE, testable lazily-rs-backed projection module that performs no SQLite writes and does not call tsift (callers feed it snapshots), with three named memoization tests. That phase can land and be verified in isolation with no runtime risk to live documents, and everything else sequences behind it.
- [ ] [#2y8j] [operator-verify] #qbasehashmemo live typing proof: after the supervisor auto-installs 0.34.49 and recycles, type a sustained burst in a large doc (equityfundingsource.md or agent-doc-bugs2.md) in the JB editor and confirm typing is smooth; grep ops.log to confirm crdt_merge_base no longer fires per keystroke during a typing burst.
- [ ] [#cdtcutover] [focused-cycle] #agent-doc-feature FUTURE/SEPARATE rung — wire the evented CellDocTree model into the live finalize/write path (replace whole-tree reconcile at finalize with update_to per-node deltas; expose SemTree queries to preflight/supervisor drainable computations). Large, live-supervisor-critical, gated, own focused cycle + operator live proof. Plan: tasks/agent-doc/plan-celldoctree-evented-reflection.md
- [ ] [#coredecomp] [focused-cycle] Keep the former `agent-doc-core` God-crate surface decomposed into owned pure crates. Audit `pending.rs`, `queue_item_lifecycle.rs`, `template.rs`, `crdt_sync.rs`, `component.rs`, and related modules; move element-local semantics into `agent-doc-element-*`, document composition into `agent-doc-document`, merge/CRDT merge policy into `agent-doc-merge`, and FFI ABI into `agent-doc-ffi`. Do not recreate `agent-doc-core` as a compatibility facade or migration alias, and do not move runtime IO, snapshots, commits, IPC, tmux, or controller effects into pure crates.
- [ ] [#orchdecomp] [focused-cycle] Decompose the `agent-doc-orchestration` God crate along effect boundaries. Audit turn lifecycle, document realtime scheduling, queue/exchange lifecycle, closeout/commit, editor IPC, supervisor/controller state, tmux integration, recovery/repair, and session routing; move stable responsibilities into `agent-doc-turn`, `agent-doc-document`/document-realtime, `agent-doc-supervisor`/controller-state crates, `agent-doc-editor`, `agent-doc-tmux`, and narrow closeout/recovery crates as appropriate. Leave `agent-doc-orchestration` as a shrinking effect adapter only, not a deprecated compatibility facade; callers should move directly to the owning focused crate and pure merge/document code should stay out of orchestration.
- [ ] [#skillharnessplugin] [focused-cycle] Create the agent-doc integration plugin for `skill-harness`: implement an agent-doc-owned `SkillHarnessPlugin` adapter, define its detection and skill-path contract without depending on `agent-doc-core`, register/document the integration path, and add tests proving `skill-harness` can resolve agent-doc through the plugin boundary after `agent-doc-core` removal.
- [ ] [#crdtauth6] [operator-verify] CRDT authority live proof: verify the existing JB DocumentListener -> CrdtReplicaForwarder.forwardLocalDelta path, Kotlin Unix-socket ReplicaTransport, VS Code koffi replica seam, and Rust fan-out stay connected in a live editor session. This is proof/parity work only; operator-visible text is already authoritative and must not wait on this item. Plan: tasks/agent-doc/plan-crdt-authority-model.md
- [ ] [#nsfz] [operator-verify] #startupdeadline live dogfood: drive a real harness that never reaches a dispatch-ready prompt (e.g. auth-walled/hung TUI) and confirm the session fails closed within 30s with a 'session did not become dispatch-ready in Ns' stderr diagnostic + a recorded startup_miss marker, instead of hanging. Code-complete + tested (make check 5101 green, release build green); needs a live editor/pane eyeball.
- [ ] [#suprestassoc-live] [operator-verify] Restart the `tasks/professional/equityfundingsource.md` supervisor in a live editor/tmux setup and verify the actor/admin projection keeps EFS bound to its own pane/session without re-associating to `tasks/agent-doc/agent-doc-bugs2.md`.
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
