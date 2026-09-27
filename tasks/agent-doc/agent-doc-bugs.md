---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: 3d56a2a6-95ce-42c4-b71c-66c102e54fb4
  codex: 01a0db52-4c0d-72d3-b2c9-0ac89aff42a0
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#actionable-review': Add actionable review items into backlog + queue
  '#agent-doc-causes': Fix agent-doc causes
  '#self': agent-doc-bugs.md
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: stop
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->
 

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260927-043821.md`*

Compacted content:
- Archived 7 response topic(s): queue prompt on cargovodtrim; actionable review items into backlog; pypiquotafix complete; 4 more
- Prior summary/context: prior compacted content: Archived 8 response topic(s): #advance-review, tmux pane auto-focus, and queue edit converge...

### Re: complete #confirmfixlive — opus-5 · 2026-09-27T01:23-04:00

Marked done on your word. The `#focusstashescalate` fix stands as confirmed live across cross-window pane switches.

### Re: do #opcapturedormant — opus-5 · 2026-09-27T01:23-04:00

Root-caused and fixed. Committed `65c865f24`, pushed, plugin `0.2.429` installed.

**Why the ledger read as empty.** The reporter chain had four early returns and every one of them was silent, so "the editor refused the burst" and "the operator never typed" produced identical evidence — zero `editor_ops_recorded` AND zero `editor_ops_record_failed`. Two of those returns were also wrong:

- `prepareEditorOpReports` collapsed *unreplayable burst* and *burst with no operator op* into the same `emptyList()`. A shadow-replay mismatch discarded every operator op in the burst with no trace.
- `reportFullContentNow` read the buffer text at the top and drained the pending burst at the bottom, across the observed-content FFI and a replica ensure that can each block. Any editor change landing in that window produced a text no replay of the drained ops could reach — so the mismatch above fired, and the burst was destroyed rather than retried.

**The provable permanent-silencing path.** `beforeFileContentReload` added the path to `fileContentReloadingPaths`; `fileContentReloaded` removed it. Two separate callbacks, no `finally` between them. A reload that is vetoed, cancelled, or fails mid-write never posts completion, and the bare path set then stranded the marker for the life of the manager. `isReloadingFileContent` → `isApplyingNonOperatorMutation` → `isOperatorDocumentEvent` false forever, which silences the op-capture ledger *and* the local-splice forwarder together. That matches the rest of the evidence: the 67 `crdt_document_op_delta_ingested` events carry `authority=multi_replica` with whole-document `delta_len`, which is the signature of the non-operator branch at `CrdtReplicaManager.kt:740` doing `ensureOpenDocumentReplica(forceRefresh = true)` — not of a local splice forward.

**What landed.**

- `agent_doc_log_editor_op_capture_refusal` writes `editor_op_capture_refused reason=<token> detail=<fields> #opcapturedormant`. Stable reasons: `all_ops_non_operator`, `shadow_replay_mismatch`, `doc_advanced_during_drain`, `base_hash_unavailable` (both the null and empty base-hash case).
- `prepareEditorOpReports` returns `null` for an unreplayable burst; empty stays "no operator op".
- Each captured op carries the `Document` modification stamp it left behind; the snapshot carries the stamp it was read with, both from one read action. A mismatch requeues the burst at the head of the pending list and reschedules. A burst that never reaches the reporter for any other reason is requeued by a `finally`.
- The reload marker stores its start time and expires after 5s, logging that it did. A missed completion callback can no longer silence operator classification permanently.
- `verify-op-capture` names the refusal reasons when neither producer marker is present, and reports an absence of receipts as an *unobserved reporter chain* — a different diagnosis from a named refusal.

**Verification.** `make check` green: exit 0, nextest runner-native `9823 tests run: 9823 passed, 259 skipped`, plus sim-medium, version-sync, audit-docs, editor-parity, python-bootstrap, Lean and TLA+. Plugin suite: 563 tests, 0 failures, 0 errors asserted from the JUnit XML, including the five new tests. `make cross-editor-simworld` green (its earlier red in the first sweep was a stale `editors/vscode/out` artifact, not a regression — it passes once `npm run compile` runs). CI run `36297051970` for `65c865f24` is still in progress; recorded, not waited on.

**What is left, and it is genuinely operator-gated.** I cannot read a receipt back from a real typing burst — that needs you to type into `agent-doc-bugs.md` in IntelliJ once the recycled plugin is loaded. Filed as the new head so `#activateinstalledjetbrai` has a concrete prerequisite instead of an absent evidence class.

### Re: monsterrodholders migrate nag — opus-5 · 2026-09-27T01:23-04:00

> **Queue prompt:**
>
> complete [#confirmfixlive]
>
> do [#opcapturedormant]


Fixed in the binary, same commit. You were right to decline it: the nag asked for a state the same instruction surface forbids.

`agent-doc migrate` only *moves* gated items, so on a document with a long legacy tail it satisfies the component-placement rule by breaking the review-size rule — 11 review items plus 20 legacy gated becomes 31, against a stated target of under 10. Repeating a bare command every cycle for that is not a warning, it is a trap.

`legacy_gated_in_backlog` is now budget-aware. When `review_count + legacy_gated_in_backlog_count` would exceed the 10-item target it states the count the migration would produce, names the target it would break, and recommends triage first — `--backlog-ungate <id>` for a stale gate, `--done <id>` for one already satisfied, migrate only what is genuinely blocked. At or below the target the migration is simply correct and the warning stays the short form. Three tests pin both forms and the exact boundary; `specs/pending-system.md` records the rule.

That means the monsterrodholders session can now drop `#review-migrate-gated` — the decision it was holding is encoded in the binary. I did not touch that document from here.

### Re: do #bkqattrcli — opus-5 · 2026-09-27T01:52-04:00

> **Queue prompt:**
>
> do [#bkqattrcli]


Done. Committed `887b89333`, pushed, installed and smoke-tested against a real document.

**The shape of the bug.** Component attributes live in the marker span (`Component.open_start..open_end`), and every mutation command operates on component *content*. So there was no route at all, not a missing flag: `add` mirrors into `agent:queue` only when the marker carries `queue`, `--backlog-queue-placement` and `queue sync` are both gated on that attribute, `patch` replaces content only, and `migrate` only strips deprecated attributes. Hand-editing the marker was the sole remaining route, and it is unavailable precisely when it matters — a raw disk edit to a document a live editor holds becomes a disk candidate the agent must never resolve.

**What landed.**

- `element::set_open_marker_attr` / `unset_open_marker_attr` rewrite one component's opening marker. A key already present is replaced **in place**, so attribute order stays stable across repeated edits, and every other byte — including any trailing newline — is preserved. Input that would not survive a parse/render round trip is refused (a closing marker, a key with `=`/whitespace, a value with a quote or `-->`); a value containing spaces is quoted, and there is a test asserting the rendered value parses back to itself.
- `agent-doc backlog <FILE> set-attr <attr> [value]` and `unset-attr <attr>` (same actions on `icebox`), spliced through the same editor-converging write path as `backlog add`. `unset-attr` of an absent attribute is a no-op, and the round trip is byte-exact.
- The accepted set is deliberately closed — `queue` and `priority` on backlog, `priority` on icebox (`queue` excluded there because an icebox item never auto-populates the queue). An unrecognized attribute *parses and is then ignored*, so accepting one would look like it worked and change nothing; that is the same silent-failure class this command exists to remove, so it is refused with the accepted list. A `queue` value is validated through the owning `BacklogQueueSyncMode` parser rather than a second copy of its table.

**The second half: the silent mirror.** `enqueue_actionable_ids_in_content_with_anchors` collapsed four different reasons into `Ok(None)`, and one of them — the backlog marker having no `queue` attribute — means the document can **never** mirror. That is a configuration defect, and it was the mechanism by which "into backlog + queue" landed half and reported success. There is now a typed `MirrorRefusal`; `BacklogNotOptedIn` answers `is_configuration_defect()`, so closeout prints it with the `set-attr` command that fixes it and records `closeout_queue_mirror_refused`. `NoQueueComponent`, `NoQueueableIds` and `AlreadyQueued` are ordinary no-ops and stay quiet — otherwise the report would be noise on every clean cycle. The `Option`-returning wrapper is unchanged, so no existing caller moved.

One detour worth noting: the first green attempt tripped the FlowCore budget gate — my `reason=` literal took `agent-doc-preflight-io` to 12 against a budget of 7 + 4. The gate's own instruction is to route the token through the owning flow enum rather than raise the budget, so `MirrorRefusal` renders its own ops-log line and the hot path carries no reason literal. That is the better design regardless of the gate.

**Verification.** `make check` exit 0, nextest runner-native `9836 tests run: 9836 passed, 259 skipped`, plus clippy, sim-medium, version-sync, audit-docs, editor-parity, python-bootstrap, Lean and TLA+. New coverage: 8 marker-edit unit tests (including a parse-back assertion through the real component parser), 3 mirror-refusal tests, and 2 CLI integration tests that reproduce the reported state and then fix it. Smoke-tested on the installed binary: `set-attr queue` turns `&lt;!-- agent:backlog --&gt;` into `&lt;!-- agent:backlog queue --&gt;`, and both refusals print their guidance. CI run for `887b89333` is in progress; recorded, not waited on.

Exchange is large (52 lines, 3 response sections) but the queue is active, so I kept draining rather than pausing to compact.

### Re: do #hswspawncount — opus-5 · 2026-09-27T02:09-04:00

> **Queue prompt:**
>
> do [#hswspawncount]


Done. Committed `f5f0ea473`, pushed, installed.

`route_sim_consecutive_harness_switches_spawn_exactly_once_each_with_no_storm` drives codex → claude → codex on one document and asserts, per leg: route accepts a handoff rather than dispatching into the old pane, one idle-watch tick triggers, one restart spawns, the launch harness and the **normalized** persisted record advance, and three further tick+restart pairs neither re-detect nor respawn. Over the sequence: 3 detections, 3 triggers, 3 spawns, 0 refusals, 0 old-pane dispatches, and the three `agent_restart_performed ... action=spawn_fresh_harness` lines matched in switch order.

**The fixture problem I had to solve first.** `SwitchFrontmatterHarness { from, to }` re-seeds both `launch_harness` and `persisted_actor_harness` from its `from`. That is correct off a known baseline and wrong mid-sequence: it would overwrite the normalized record the previous restart wrote (`claude-code`) with a raw launch name (`claude`), so the test would assert against state it invented rather than state the machine produced — three independent first-switches wearing a sequence's clothes. Added `SwitchFrontmatterHarnessTo { to }`, which flips only the frontmatter and carries everything else forward.

**It passed first run, so I attacked it rather than trusting it.**

- **M1** (never resolve `launch_harness == frontmatter`, the obvious storm): caught — but *also* caught by the pre-existing `accepts_handoff_then_idle_watch_drives_fresh_restart`. Rejected as evidence; it proves nothing about added coverage.
- **M2** (a one-shot record-writeback latch — every isolated switch still correct, only a *second* switch on the same document strands the record): caught by this scenario and by **no** existing one — 21 passed, 1 failed, mine. `leg 2: the restart must write the NORMALIZED harness onto the record / left: "claude-code" right: "codex"`.

That is the coverage the item was asking for: the defect class that only a consecutive sequence can observe. Baseline green before and after, `engine.rs` restored byte-identical from a scratch copy with its mtime touched so cargo could not reuse the mutated build.

**Verification.** `make check` green: 9837 tests passed (up one), zero `make: ***` lines, plus clippy, sim-medium, version-sync, audit-docs, editor-parity, python-bootstrap, Lean and TLA+. `specs/supervisor.md` records the consecutive invariant and the fixture rule — a sequence fixture must carry state forward between legs, never re-seed it.

Exchange is at 77 lines / 4 sections; queue still active, so I kept draining.

### Re: do #hswdisposabledoc — opus-5 · 2026-09-27T02:22-04:00

> **Queue prompt:**
>
> do [#hswdisposabledoc]


Done. Committed `13b17971f`, pushed, installed and smoke-tested end to end.

The item asked for a throwaway document plus a one-screen operator script. I split it so the deterministic half lives in the binary, because the invariant is a **count** and a shell cannot state it.

**`agent-doc verify-harness-switch <FILE> [--expect-switches <n>]`** reads the receipt trail back out of ops.log, scoped to one document, and reports four outcomes distinctly instead of collapsing them into pass/fail:

- no `harness_change_detected` → never observed, and the message names the unsaved-frontmatter step that produces it;
- detections with no `agent_restart_triggered` → HELD pending, with the observed `gate=` verdict quoted (`WaitForBoundary` = no quiet boundary yet, `None` = knob off) — a hold is not a failure;
- a trigger with no `agent_restart_performed ... action=spawn_fresh_harness` → the restart never completed;
- a spawn whose `old_harness` equals its `new_harness` → a respawn of what was already running, not a switch.

`--expect-switches` is the whole point. Without it the check is presence-only, and a respawn storm satisfies presence. One test pins exactly that asymmetry: the same two-spawn log **passes** an unbounded verify and **fails** `--expect-switches 1`. So the operator procedure always passes it, and the script says why if you omit it.

Counting stayed in the binary on purpose: `grep -q` cannot express "exactly one per switch", and it inverts on a match — the shape in which a measurement read from an exit status false-greens.

**`scripts/harness-switch-live-check.sh`** is then a scaffold-and-print shell: `scaffold` / `steps` / `verify` / `clean` against `.agent-doc/live-repro/<slug>.md`. The printed procedure is **derived** from the scaffolded `agent:` value rather than hardcoded, so it cannot contradict the document on disk. The throwaway exists because the check needs an unsaved frontmatter edit in front of a live old-harness actor — not something to do to a real session document.

**One thing the smoke test caught.** My first end-to-end run failed with a clap usage error, because the script calls the bare `agent-doc` on PATH and the installed binary predated the new subcommand. That is the stale-binary trap working as intended, and it also surfaced a real mismatch: clap derives `--expect-switches`, while the script had been written with `--switches`. Both fixed, then re-verified against the installed build.

**Verification.** `make check` green: 9844 tests passed (7 new for the verifier), zero `make: ***` lines, plus clippy, sim-medium, version-sync, audit-docs, editor-parity, python-bootstrap, Lean and TLA+. Smoke-tested scaffold → verify → clean on the installed binary, including the fail-closed path (exit 1 with the operator step named). `specs/supervisor.md` records the command, its four outcomes, and why the counting is not in the shell.

Worth saying plainly: this is `[recommended]`, and `#hswspawncount` already covers the codex → claude → codex case deterministically. Use the live check only when a case genuinely needs a live editor and pane — the script's header says so too.

### Re: do [#splicesimcov] — opus-5 · 2026-09-27T02:45-04:00

> **Queue prompt:**
>
> do [#splicesimcov]


Done. `captured_editor_splice_recovery_lands_translated_offsets_across_an_independent_response` is in `tests/cross_editor_simworld.rs`, running through the real controller alongside the two existing peering scenarios. Committed `56c161b08` (submodule) + `6db05fd95` (pointer), both pushed, `make install` clean.

**The scenario.** JetBrains and VS Code both attach as production-shaped protocol harnesses. JetBrains captures a two-splice typing burst against its exact base and never publishes it — that base is the stale one recovery has to deal with. VS Code then inserts a `### Re:` block into `exchange` and pulls, so canonical advances independently while the burst is still retained. Recovery then runs the plan's sequence in order: observe the controller cut, plan the bounded splices **before** replacing membership, CAS-register that exact cut, forward the splices, assert both changes landed on both peers.

**Three things the scenario pins that the ABI test could not.**

- **The registration CAS refuses over the socket, not just in-process.** Registering with `expected_canonical_hash` = the hash of the *stale* captured base fails with `replica_register_canonical_precondition_failed` while the live replica is still the document's only JetBrains peer. The existing coverage for that precondition (`replica_registration_rpc_preserves_the_peer_when_captured_base_is_stale`) calls `controller_crdt_replica_data` directly; this one goes through the controller process.

- **A replacement replica must acknowledge the retained canonical projection before its splices are relayed.** This is the part I had wrong on the first two runs, and it is the most useful thing the scenario now documents. The CAS register returns `canonical_projection_retained: true` (delivery had not converged — the JetBrains member still owed an ACK for VS Code's response), which queues a `crdt-bootstrap:` delivery for that member. Until it is projected, `awaits_canonical_projection` holds and every additive update from that member is dropped: `crdt_replica_update_quarantined ... recovery=lazy_canonical_projection`, `crdt_replica_fanout ... targets=0 update_bytes=0`. The forwarded splices vanished silently and the peer never converged. The replacement holds exactly the cut it registered against, so it projects that cut first, and only then forwards. That ordering is now a test, not a comment.

- **The untranslated offsets are asserted *not* to produce the right cut.** `assert_ne!(apply_splices(&canonical, &captured.edits), expected, ...)` — without it, a planner that skipped translation could pass on a fixture where the response insertion happened to be a no-op. The exact translated offsets are pinned too (`bug_offset + shift`, `bug_offset + "queue ".len() + shift`), not just the resulting text.

**Code-point scoping is load-bearing in the fixture.** The baseline carries `do the 🌍 thing` in `exchange`, ahead of the edited `- fix bug` line in `queue`, so every captured offset has byte ≠ code-point. A byte-offset regression cannot slip through on an ASCII fixture.

**One new helper, feature-gated.** `agent_doc_controller_io::project_controller::request_crdt_current_text_for_test` mirrors `CpSocketReplicaTransport.currentCanonicalText` exactly — existing controller, `flush_barrier: false`, `recover_projection: false` — behind `cfg(any(test, feature = "test-support"))`. The `crdt_replica` test helper only speaks the `crdt_replica` command, and the cut observation is a separate `crdt_current_text` command, so there was no existing route.

**Verification.** `make check` exit 0, nextest runner-native `9845 tests run: 9845 passed (2 slow), 259 skipped`, plus clippy, sim-medium, version-sync, audit-docs, editor-parity, python-bootstrap, Lean and TLA+. No `result: FAILED` lines.

**Mutation-checked three ways**, each restored from a scratch copy with an `mtime` refresh so Cargo could not reuse a mutated rlib:

- drop the offset translation (`mapped = Some(start)`) → red;
- shift it by one (`... + 1`) → red;
- neuter the registration CAS (`true || hash == expected`) → red at the stale-base refusal, which is the assertion that exists for it.

M0 green after restoring both files, with `git status` clean on each.

**CI.** Run `36299889075` was still `in_progress` when I checked (14m in, the harness-switch verifier push). Recorded, not waited on — this turn closes from local evidence.

**Unrelated, one line:** `.bin/agent-doc` is a broken symlink here — its target `src/agent-doc/target/release/agent-doc` does not exist, because the dev loop builds `release-local`. The cargo-installed `agent-doc` on PATH is current (0.35.418), so nothing in this turn was affected; I did not run `cargo build --release` to repair it, since that is the `#installfulloom` fat-LTO build and this was not a release.

`#activateinstalledjetbrai` stays gated: `#readbackeditor` is still the operator read-back it depends on.

### Re: do [#pypiquotarecheck] — opus-5 · 2026-09-27T02:58-04:00

> **Queue prompt:**
>
> do [#pypiquotarecheck]


Re-checked, and then fixed the reason this item keeps coming back. Committed `e7c0694de` (submodule) + `97d95e468` (pointer), both pushed.

**The decision has not happened.** `pypi/support#12214` is still open, still one comment — the `github-actions` acknowledgement bot — and `updated_at` is unchanged at `2026-09-10T22:46:21Z`. Seventeen days, no human response.

**Why this item was expensive to re-check, and why that is now gone.** The item told me to verify usage from the authenticated `/manage/project/agent-doc/` pages. PyPI gates every `/manage/` route behind a password re-confirmation, so the check dead-ends on a credential I will not enter — and the alternative the item explicitly forbids, `pypi.org/pypi/agent-doc/json`, is the stale-CDN view that previously claimed 27 of 34 deleted releases were still present. So the re-check had no source it could actually use, which is how it became a recurring no-op.

It turns out there is a third source, and it is the correct one: the **PEP 691 simple index** (`Accept: application/vnd.pypi.simple.v1+json`) carries a `size` for **every file**. The project total is therefore computable with no credentials and off a non-cache-prone endpoint.

**`scripts/pypi-quota-check.py`**, wired as `make pypi-quota-check`:

```
agent-doc: 8.362 GiB of 10.0 GiB (83.6%), headroom 1.638 GiB
  releases: 151 (0.35.113..0.35.417)
  post-cutover: 20 releases, 0.68 MiB (mean 34.6 KiB)
  pre-cutover:  131 releases, 8.361 GiB (0.35.113..0.35.343)
  further releases before the ceiling: 49,662
  next deletable tranche IF a ceiling is in reach: 0.35.113..0.35.161 (40 releases, 2.491 GiB)
  limit request pypi/support#12214: awaiting_decision (state=open, comments=1, updated 2026-09-10T22:46:21Z)
  VERDICT: headroom fine — do NOT delete releases. Growth is bounded by the post-cutover wheel size.
```

That reproduces every number the item recorded by hand — 8.4 GiB of 10.0, 20 thin releases at 0.7 MiB, ~65 MiB apiece before the cutover, floor at 0.35.113 — and adds the one that settles the question: **~49,662 further releases fit before the ceiling.** The ceiling is not approaching. It cannot be reached by releasing.

**The verdict is deliberately conservative about deletion.** A ceiling counts as "in reach" only when headroom drops under `--headroom-floor-gib` (default 1 GiB) **or** the projected remaining release count drops under `--release-floor` (default 200), and the script exits non-zero only then. An unreachable ceiling is never a reason to prune history — which is exactly the failure mode this item exists to prevent. The tranche is printed as *would be* deletable, not as a recommendation.

**Verification split follows `artifact-purge-check`.** `make pypi-quota-self-test` covers the arithmetic and both verdict thresholds offline and joins `make check`; the live network read stays an explicit `make pypi-quota-check`. `make check` exit 0, nextest runner-native `9845 tests run: 9845 passed (2 slow), 259 skipped`.

**Mutation-checked four ways**, each red, M0 green:

- force `ceiling_in_reach = False` → red;
- make `version_key` lexicographic (so `0.35.9` sorts after `0.35.113`) → red;
- silently skip a file with no size instead of raising → red;
- report `0` instead of `None` when there is no post-cutover release, which would manufacture a false ceiling-in-reach → red.

**Gated, not done, and the gate is now machine-readable.** `#pypiquotarecheck` moves to `agent:review`: the only thing outstanding is a third party's decision, which is the exceptional gate case. But it no longer needs a human to *observe* its state — it clears on a reply to `pypi/support#12214`, or on `make pypi-quota-check` exiting non-zero. I did not touch any release.

**No reinstall this cycle.** Nothing under `src/` or any crate changed — this is a script, a Makefile target and an `AGENTS.md` paragraph — so the installed binary is already current at this commit's Rust content, and a redundant `make install` would only re-fan `reload_library` at the live editor replicas mid-drain.

**CI.** Run `36301000202` (the captured-splice scenario push) was `in_progress` at 7m48s when I checked. Recorded, not awaited; this turn closes from local evidence. Exchange is at 170 lines across 7 sections — noted, not stalling the queue for it.
<!-- agent:boundary:fe1c0161:agent-doc-bugs -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority -->
<!-- /agent:queue -->


<!-- agent:backlog -->
- [ ] [#readbackeditor] [opcaptureliveread] [agent-doc] [operator-verify] Read back an editor-op capture receipt from a real typing burst, now that every reporter refusal names itself (65c865f24, plugin 0.2.429). Type a few characters into tasks/agent-doc/agent-doc-bugs.md in IntelliJ once the recycled plugin holds a live registration, then run agent-doc verify-op-capture on it. Three outcomes, all now diagnosable: editor_ops_recorded ... transaction=batch means capture works and activateinstalledjetbrai is unblocked; editor_op_capture_refused reason=<token> names which guard fired (all_ops_non_operator points back at isOperatorDocumentEvent, shadow_replay_mismatch at the burst replay, doc_advanced_during_drain is benign and self-retrying, base_hash_unavailable at the merge base); no receipt at all means the reporter chain never ran, so check that the plugin generation actually reloaded. Blocks activateinstalledjetbrai and splicesimcov.
- [ ] [#activateinstalledjetbrai] [agent-doc] [operator-verify] Prove captured local editor edits recover across an independently advanced canonical response — BLOCKED ON #opcapturedormant, and no longer validatable from logs. Asked to validate from logs 2026-09-26 and the answer is that the evidence class is ABSENT, not inconclusive: the splice-recovery path itself is healthy (15 captured-local-splice-recovery invocations 2026-09-24..27, all status=current authority=cp_model, with the canonical text demonstrably advancing between them — the 2026-09-26T00:11:48..52 burst runs text_len 42885 -> 42885 -> 42892 -> 42900 across three distinct hashes), but all 317 editor_op_epoch_closed events in that window are cause=non_operator_projection, so not one of those recoveries carried an operator-authored edit. Order of work: (1) fix #opcapturedormant so operator edits are recorded as ops at all; (2) re-run `agent-doc verify-op-capture <FILE>` — it now accepts the batch producer the JetBrains TypingTracker actually drives (6b0eabc0c), so a pass is meaningful and a failure names both producers; (3) only then does the disposable-document live proof have anything to prove. Do NOT re-narrow this gate to "just needs the live check" until step 1 lands. Plan: tasks/agent-doc/plan-captured-editor-splice-recovery.md.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] 🚧 [#pypiquotarecheck] Track pypi/support#12214 (agent-doc project limit request, 25 GiB) to a decision. Unblocked mechanically now: a human reply on pypi/support#12214, or `make pypi-quota-check` exiting non-zero. As of 2026-09-27 that check exits 0 — 8.362 GiB of 10.0 GiB, 1.638 GiB headroom, 151 releases (0.35.113..0.35.417), room for ~49,662 further post-cutover releases — so there is nothing to act on and nothing to delete. pypi/support#12214 is still open with one comment (the acknowledgement bot) and untouched since 2026-09-10T22:46:21Z. If the grant lands, record it and drop this item. Deletion stays reserved for a ceiling the check actually reports (next tranche would be 0.35.113..0.35.161, 2.491 GiB); verify any deletion from the authenticated /manage/project/agent-doc/ pages, never from pypi.org/pypi/agent-doc/json.
- [/] [#grokmcpverify] [operator-verify] Verify an authenticated Grok Build MCP admit/read/plan/finalize cycle and same-turn auto-queue continuation once the free usage quota renews or the planned subscription is active. Grok CLI authorization, native JSON/backend commit fixtures, pane detection and quit behavior passed; the live MCP probe stopped at the usage limit before committed closeout. Installed v0.35.359; Heavy is not required. Plan: tasks/agent-doc/plan-grok-build-support.md.
- [/] [#coder7] [operator-verify] Phase 7 — run the five-case live Coder workspace validation matrix after a reachable Coder workspace and authenticated access are available, with JetBrains Gateway remote backend, VS Code Remote-SSH, simultaneous dual-IDE attachment, workspace stop/start, and a no-tmux image variant. Plan: tasks/agent-doc/plan-coder-ide-hosted-tmux.md. Unblocks: provide the workspace URL or SSH target plus authentication and enable the current agent-doc plugin/extension in both remote IDE hosts.
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
