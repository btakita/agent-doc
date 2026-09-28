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
queue: go
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

### Re: agent-doc causes of the admission refusal — opus-5 · 2026-09-27T04:13-04:00

> **Queue prompt:** re [#readbackeditor]: Add to the logs what you need to verify. Then verify. Automate testing yourself.

Your chat messages redirected this turn to the refusal I'd just reported, so that is what I fixed. `#readbackeditor` is untouched this cycle; I rewrote its backlog text to carry your instruction (automate it, stop asking an operator to type) — see the bottom.

**Two agent-doc causes, both in the shared `admission_divergence` classifier. Fixed, tested, mutation-checked, committed as `8eb8eca5b`.**

#### What actually happened

`ops.log` at 07:13:25Z, in order:

```
commit_document_via_controller_error ... live editor commit projection is not durable yet
controller_commit_projection_pending ... decision=NativeSaveRequired disk_projection_ready=false
commit_staging          snap_len=31112  file_len=31220
document_baseline_checkpoint generation=1792 len=31112
commit_blocked_unproved_head_current_component_drift ... verdict=unanswered_edit_pending
crdt_current_text       len=31220 hash=af296d601d98   live_editors=1
queue_maintenance_deferred ... baseline_hash=d317f989cd3d authority_hash=af296d601d98 disk_hash=b6c8f269b535
```

There was no second writer. The commit was **blocked**, so disk kept the older revision — and the baseline was still checkpointed, at 31112 bytes, against a 31220-byte authority. That 108-byte delta is transient agent-doc markers.

#### Cause 1 — `#admissionmarkersplit`: the planes were compared as raw bytes

The merge baseline is checkpointed in the **durable** (marker-stripped) domain; the editor authority carries the transport markers agent-doc writes into the live buffer and strips on the durable path — `<!-- agent:boundary:… -->`, the ` (HEAD)` heading suffix, `❯ 🚧`, per-cycle guard comments, the managed pipeline frontmatter block. Comparing raw bytes let agent-doc's own transport state *be* the third revision.

This is worse than a stall because it is **permanent**: the remedy printed is "save or close the editor tab", and a save cannot converge planes agent-doc re-splits on the next write. Two refusals 2h35m apart, same shape.

Every other durable comparison in the workspace already normalizes first — `authoritative_current_monotonically_extends_capture_baseline` says so in its own doc comment ("so transport-only state does not manufacture a conflict"), as does `agent_doc_git_io::transient_cleanup`. This module was the outlier.

#### Cause 2 — `#admissionancestorsplit`: "no unchanged branch" ≠ "a writer would be dropped"

Under the blocked commit above, the baseline advances to the live snapshot, disk keeps the older commit, and the operator keeps typing. Three distinct revisions — but the live buffer **preserves every durable line already on disk**. Disk is an ancestor, not a rival. Admission can proceed on the authority and the next commit converges disk; that is the same monotonic proof that already gates rebasing a retained response onto concurrent operator steering.

#### Cause 3 — the refusal could not name its own class

Every case printed the same three raw hashes, so diagnosing one meant reconstructing byte lengths out of surrounding `commit_staging` / `document_baseline_checkpoint` lines (which is exactly what I had to do above). That is the "add to the logs what you need to verify" part:

```
admission_divergence file=… reason=<token> action=<token>
  baseline_hash= authority_hash= disk_hash=
  durable_baseline_hash= durable_authority_hash= durable_disk_hash=
  baseline_len= authority_len= disk_len=
```

Four classifications, each naming itself; refusals also print the durable hashes inline, and `session_check_steering_admission_refused` carries the same `reason=`.

| reason | meaning | action |
|---|---|---|
| `verbatim_branch_unchanged` | a plane matches another byte-for-byte | proceed / fast-forward — **not logged**, it is the every-preflight case |
| `transient_markers_only` | differ only by transport markers; a durable branch is unchanged | same, decided on durable content |
| `authority_subsumes_disk` | three durable revisions, live buffer contains all of disk | proceed on authority, adopt nothing |
| `unmergeable_three_way_split` | disk carries content the authority never received | refuse + operator remedy |

The `reason=`/`action=` pair is rendered by `AdmissionAssessment::diagnostic_fields` in the crate that owns the decision, not spelled in the IO shell — that is what the FlowCore hot-path gate is asking for (it caught my first draft at 15 tokens against a budget of 11).

#### Why this is safe

`assess()` runs the **verbatim** equality tests first and unchanged, so it is a strict widening: an observation that resolved before resolves identically, and only observations that previously *refused* can change verdict. Adopting disk still requires an unchanged durable authority branch; `authority_subsumes_disk` adopts **nothing** (adopting an ancestor would drop your steering). Disk content the authority never received still refuses.

#### Proof

- 11 classifier unit tests + 4 preflight IO-shell tests (marker-only, ancestor, genuine refusal, and *silence* on a converged observation).
- **Mutation check** — reverted `assess` to the raw/no-subsumption behaviour; both new IO tests went red with the exact refusal text from the live wedge. Restored from a scratch copy with an mtime refresh (not `git checkout`), then re-confirmed green.
- `admission_subsumption_matches_the_capture_monotonic_rule` in `agent-doc-capture-io` pins the inlined subsumption predicate to `agent_doc_workflow::capture::current_monotonically_extends_baseline`. The classifier can't import it — `workflow → turn → document` would close a cycle — so the copy is deliberate and now cannot drift.
- `make check`: **exit 0, 9857/9857 passed**, 259 skipped, zero `result: FAILED`. Latest CI run is green.
- Spec: `specs/14-realtime-workflow.md` § "Admission Divergence Across Baseline, Authority, And Disk"; `SPEC.md` indexes it.

#### Ordering note

`make install` runs **after** this closeout, not before — a mid-session cdylib reload strands an attached document's Lazily replica and only an editor-tab reopen clears it. Push follows.

#### Not fixed, deliberately

The blocked commit itself (`NativeSaveRequired` + `disk_projection_ready=false` leaving disk behind while the baseline advances) is untouched. The baseline checkpoint arguably should not advance past a commit that never landed — that would restore the strict common-ancestor invariant instead of tolerating its violation. I fixed the classifier because that is where the refusal is decided and where a wrong answer costs an operator their session; the checkpoint ordering is a separate, wider change across ~30 call sites. Filed at the top of the backlog.

### Re: do [#baselinecommitblocked] — opus-5 · 2026-09-27T05:18-04:00

> **Queue prompt:**
>
> do [#baselinecommitblocked]


Fixed in `42db8999e`, pushed. `checkpoint_document_baseline` now gates on content identity.

**What the trace actually shows.** Your report's premise is half right. The blocked commit did emit `document_baseline_checkpoint generation=1791` and then `generation=1792` — but both wrote `len=31112`, which is the *unchanged HEAD* content, not the live snapshot (31220). The baseline never advanced to a revision that never landed; it re-asserted the revision it already held and burned a generation each time. And since `admission_divergence` classifies by content hash rather than generation, that churn did not itself produce the three-plane split — the planes here were baseline == disk with the authority ahead, which is `authority_subsumes_disk`. What the churn *did* produce is a ledger that misreports the baseline as having moved, and exactly the diagnostic dead end the `admission_divergence` doc comment describes: identical content, moving generation, three planes no longer reconstructible from surrounding `commit_staging` lines.

**Where it came from.** `repair_committed_historical_snapshot_drift` converges the baseline to HEAD via `snapshot_repair … reason=committed_capture basis=head_local_drift`. That first convergence is correct. But while the commit stays blocked and you keep typing, disk never equals HEAD, so the branch qualifies again on the next pass, re-checkpoints the same HEAD content, and the generation walks. Every blocked retry paid for it.

**The fix.** The baseline is content-addressed, so content identity is the gate, and it lives in the one function that owns the fact:

- `checkpoint_document_baseline_with_effects` compares the incoming content hash against the projected baseline. On a match it appends no fact, leaves the generation alone, and logs `document_baseline_checkpoint_unchanged … generation=<unchanged>`.
- The write-only crash-state sidecar is still rewritten on the no-op path — it can be missing on disk even when the typed fact is current.
- I did not gate the ~30 call sites you suggested. Each would have to re-derive a "proven durable projection" answer this function already holds, and there are 882 call sites in total across the workspace, so per-site gating would have covered a third of them. One gate at the owner covers all of them.

**Coverage, and proof it isn't vacuous.** My first attempt at the regression test passed against a disabled gate, so I threw it out: the existing typed-component-drift fixture blocks the commit without ever reaching the re-checkpoint. The replacement drives the real loop — a committed capture materialized in HEAD, operator drift on disk, repair run repeatedly. The first pass converges to HEAD; the three after it must hold the generation. A companion test pins that a genuinely newer revision still advances it, so the gate is content identity and not "checkpointing is now a no-op". Both redden when the gate is disabled (`blocked-commit retry 0 re-asserted the same baseline content`, `identical content is the same revision`).

`load_document_baseline_generation` is new in `agent-doc-snapshot-io` so a test can assert on the projected generation directly instead of parsing ops.log. `SPEC.md` and `specs/14-realtime-workflow.md` both carried the "baseline checkpointed from the live snapshot" phrasing that this trace disproves; both now state the durably-projected invariant instead.

**Verification.** `make check`: 9859 tests run, 9859 passed, 0 failed, exit 0. `cargo build --release` green — the supervisor auto-install path builds release, which nextest does not, so a release-only break would stall promotion at the last good commit. CI run `36308915309` on `42db8999e` is still in progress; the prior run on `8eb8eca5b` was green. Installing + recycling after this closeout rather than before it, so the cdylib reload cannot strand this document's replica mid-write.

### Re: do [#readbackeditor] — opus-5 · 2026-09-27T06:06-04:00

> **Queue prompt:**
>
> do [#readbackeditor]


Automated in `f39cba8f0`, pushed. `verify-op-capture` no longer needs a human IntelliJ burst.

**What was still inferred.** `65c865f24` made a *dormant* ledger diagnosable by writing a refusal receipt for every silent early return. But a ledger that **does** record still proved the four facts only by the *absence* of a refusal — and a reporter whose `documentChanged` listener never ran writes neither a producer marker nor a refusal. Reading "no refusal" as "all four held" is the same absence-is-evidence inversion `#idlerevisionreactive` names, one layer up.

**So the success path states them.** `agent_doc_log_editor_op_capture_proof` writes `editor_op_capture_proof epoch_generation=<n|unknown> operator_ops=<n> non_operator_ops=<n> shadow_replay=agreed|disagreed merge_base=<h12|unavailable>`, and the reporter calls it immediately **before** handing the burst to the record FFI. That ordering is the point: a proof receipt with no producer marker beside it is its own diagnosis — the reporter got all the way there and the FFI still wrote nothing, which is neither a dormant chain nor a named refusal. `verify-op-capture` counts the receipts and, on a missing producer marker, names the latest receipt's fields beside the refusal reasons.

One naming correction. Your item asked for "live registration generation", and there is no such thing in the plugin — `EditorRegistration` carries a timestamp, not a generation, and a live-editor count needs a controller round trip per burst, exactly the load `#idlerevisionreactive` forbids on a hot path. What genuinely bounds a captured burst is the **op-capture epoch**: every remote/agent projection closes it first, so a burst is replayable only while its path is still at the epoch it was captured in. That is what the receipt reports, resolved from the plugin's own `remoteEditorEffectGenerations`. An absent epoch reads `unknown`, never a plausible-looking `0`.

**The synthetic test found a second live defect.** `--expect-cafe-demo` is documented for live JB/VS Code plugin tests, yet it read byte evidence only off the one-op producer's per-op fields — so the batch path the JetBrains `TypingTracker` actually drives could never satisfy it. That is the same false negative `#opcaptureverifybatchproducer` removed from plain mode, surviving in cafe-demo mode because cafe-demo had its own reader. Fixed by giving `EditorOp`'s own crate the byte summary, rendering both the batch producer and the merge consumer through it, and letting cafe-demo read the contract off either.

**Automation, and why the old tests could not have caught this.** Every other test in that module hands the verifier ops-log text the test itself wrote — that proves the parser and nothing about the producers, which is precisely how a verifier demanding a marker the shipped plugin never writes went unnoticed. The new test drives the proof FFI, the batch record FFI, and the merge consumer against a temporary project, then verifies the `ops.log` *those* producers wrote, cafe-demo byte contract included. A Kotlin contract test pins the reporter half: the epoch accessor exists and returns `-1` when absent, the classification split reaches the proof instead of being recomputed there, the proof precedes the record call, and an older cdylib without the ABI does not break capture. It reddens when the proof call is moved after the record call — I checked.

The four refusal tokens from `65c865f24` are untouched.

**Verification.** `make check`: 9862 tests run, 9862 passed, 0 failed, exit 0. Gradle: 564 tests, 0 failures, 0 skipped — read from the JUnit XML rather than the wrapper output, which hides skipped task lines and would let a skipped test task read as a passing one. `cargo build --release` green. JB plugin `0.2.429` → `0.2.430`. CI `36311454957` is in progress; the previous commit's run (`36308915309`, the baseline fix) went green.

This unblocks `#activateinstalledjetbrai` and `#splicesimcov`. Installing and recycling after closeout again, so the cdylib reload cannot strand this document's replica mid-write. Exchange is at 320 lines / 10 response sections; noting it rather than stalling the drain.

### Re: Fix Run Agent Doc on monsterrodholders.md — opus-5 · 2026-09-27T17:44-04:00

Fixed. Committed `e7c4c8e92`, pushed, installed.

**What the IDE was actually reporting.** `idea.log` carried the verbatim failure:

```
[route] FAILED (exit 1): route_queue_activation: failed to resolve current document
  .../monsterrodholders.md: editor is still attached; missing_replica recovery
  exhausted and disk read authority is refused
```

**The bug is one string.** `resolve_editor_unavailable_disk_read_fallback` interpolates the observation family into one message, and `reason` is either `missing_replica` or `sync_pending`. Route's retry predicate (`agent-doc-route-io/src/queue_dispatch.rs`) matched `"sync_pending recovery exhausted"` **literally**, so the entire missing-replica family — the one a live JetBrains session actually hits — got no bounded retry at all and route failed the dispatch on the first refusal. The three existing tests all used the `sync_pending` spelling, so nothing caught it. Mutation-checked: the new `missing_replica` test fails against the old predicate.

**Widening the predicate alone would not have fixed it,** and this is the part worth keeping. Every missing-replica recovery latch in `agent-doc-document-realtime-io` is keyed on the editor replica **liveness witness** — `should_pause_editor_replica_self_heal` and `claim_terminal_missing_replica_rebuild` both refuse a second attempt while it is unchanged. So an immediate retry skips the re-registration loop *and* the terminal rebuild and bails on the identical refusal: a provable no-op that spends one of only two attempts. The fix adds `wait_for_editor_replica_liveness_change` and gates the retry on a registration that actually landed; an unchanged witness fails closed at once. That is `#ensurewindowsize` seen from the caller — a large document on a busy IDE registers *after* the 400ms missing-replica ensure window has closed. Your IDE was indexing a 2MB `mrh-export.xml` at the time.

`<reason>_resolve_retry` now records `witness_changed=` and `decision=retry|fail_closed`.

### Re: do [#activateinstalledjetbrai]: automate the verification — opus-5 · 2026-09-27T17:44-04:00

Automated. Committed `0cd957382`, pushed, installed: `agent-doc verify-captured-splice-recovery <FILE>`.

**First, a correction to this item's own reasoning.** It concluded, from a window where all 317 `editor_op_epoch_closed` events said `cause=non_operator_projection`, that no recovery carried an operator edit. That inference was never available: `agent_doc_clear_editor_op_epoch` is the only producer of that event and writes exactly one cause, so `non_operator_projection` is the field's **only possible value**. It records "an epoch was cleared ahead of a non-operator mutation", never "this epoch carried operator ops". A single-valued field discriminates nothing, and the verifier now refuses it explicitly in its failure text so the next reader cannot repeat it.

The receipt that *does* discriminate is `editor_op_capture_proof`, whose `operator_ops=` is the classification split — which `#opcapturedormant` added last cycle.

**The gate, as a receipt predicate.** A pass needs three receipts for one document, in order: a splice-recovery `status=current` read; an `editor_op_capture_proof` with `operator_ops >= 1` and `shadow_replay=agreed`; then a later splice-recovery `status=current` read with a **different** `text_hash`. Ordering is the property — a capture entirely before or after every advance establishes nothing — and a disagreeing shadow replay discards the burst, so those ops never reached the merge.

Each failure names its own missing link, because these are genuinely different diagnoses: no splice recovery ran; recoveries ran but never resolved `status=current`; the canonical text never advanced; no proof classified an operator op; proofs exist but disagreed. Zero proofs is an *unobserved reporter chain*, not a proof that classified everything as non-operator.

Checked against the live logs, where it fails with the right answer:

```
35 captured-local-splice-recovery receipt(s) for doc=monsterrodholders, but none resolved
`status=current` (observed: editor_attached_model_missing) — the recovery never saw canonical
text, so the editor replica, not op capture, is the blocker
```

The live half is `scripts/xdotool-live-verify.sh captured-splice`: real keystrokes into a throwaway scratch doc under the harness's focus guards, a response cycle **between** two operator edits so the canonical text advances on its own, then the binary decides. Driving only the typing is exactly why earlier sweeps found recoveries with nothing to recover across. I have not run it — it injects keystrokes into your live desktop, so that is your call to make, not mine.

### Re: lazily.md, devops.md, monsterrodholders.md — one crash behind all of them — opus-5 · 2026-09-27T17:44-04:00

Root-caused and fixed. Committed `2c4278b29`, pushed, installed.

**The crash.** `common_prefix_len` walks raw bytes, so two sides diverging inside a multi-byte character yield a prefix that is not a char boundary — and `merge_inner` sliced at it. The character it fires on is **`❯`, agent-doc's own prompt marker**, which is why it was deterministic on document content rather than random. Mutation-verified: without the floor,

```
panicked at agent-doc-merge/src/crdt.rs:376:30:
end byte index 21 is not a char boundary; it is inside '❯' (bytes 19..22 of string)
```

That fix and its two tests were written by the concurrent lazily.md session, which left them uncommitted and asked whether this session should land them. It should — this session owns that code — so they are committed with attribution in the message. They were inside my green `make check`.

**Then five refusal sites reported the consequences as verdicts.** A controller that panics mid-request closes the connection without answering, and the client says `project controller closed connection without a response`. Preflight admission surfaced that *transport fact* as a refusal **with a reason** — so the agent, correctly trusting a reasoned refusal, stopped. Same shape in `session-check`'s INTERRUPTED on devops.md and monsterrodholders.md: "the controller owns the generation-fenced editor save, do not resubmit or force disk" is true only while something durable **holds** the write, and the holder it named was the controller that had just crashed. A dead holder fires no state edge, so two sessions obeyed and stranded their writes. `AGENTS.md` already states that rule (`#percellconverge`) — the predicate just never asked whether the holder was alive.

Admission now re-asks once on a transport drop, using `agent-doc-controller-io`'s **existing** predicate rather than a second spelling of the message (it already retries that error for `coordination_claim`, `actor_binding`, and `dispatch`; admission was the one boundary where not retrying costs a whole turn). Only a transport drop retries, and only with enough budget left to finish — an unnamed overrun is worse than a named refusal.

Be clear about what that retry does and does not buy: against a *recycled* controller it recovers the turn; against a *deterministic panic* it fails the same way, which is correct. The crash fix is the remedy, and it is now installed. The supervisor recycles at the next idle boundary, after which devops.md and monsterrodholders.md should admit normally — their editor buffers are still authoritative and nothing was patched or forced.

### Re: Use formal logic to fix this entire class of error — opus-5 · 2026-09-27T17:44-04:00

Done: `formal/tla/TransientRefusalLatch.tla`, wired into `make check` with five configs. All seven TLA non-vacuity obligations pass.

`EditorReplicaStrand` already models one wedge of this shape, but it is about what the **responder latched**. Everything in this turn is about what the **caller does with a refusal it did not author** — a different property, and the one that made four agents stop this afternoon.

**The class.** A caller-visible refusal must be classified, and each class has exactly one correct response:

- **AUTHORED** — the responder ruled. Re-asking is forbidden; retrying a verdict turns a fail-closed guard into a retry storm.
- **TRANSIENT** — the cause can clear. The refusal must name a witness that clearing advances, and the caller must **wait** for it rather than fail closed.
- **CRASH** — the responder died before answering, so no verdict exists; a replacement is the crash's witness.

A transient refusal whose caller fails closed while the witness could still advance is a latch.

**The sharp statement is `OnlyAuthoredVerdictsStop`:** work is handed back to the operator *only* because the responder ruled. Every instance this turn violates it. It also replaces TLC's deadlock check, which cannot serve here — stopping on a verdict is a legitimate dead end, so deadlock-checking failed the positive run for the one behaviour that is right. Naming *which* dead ends are legitimate is strictly better than forbidding all of them.

**Three knobs, one must-violate config each**, because they are independent fixes for one class and a single wedge would let two go vacuous the moment the third landed:

| knob FALSE | the instance it is |
|---|---|
| `ClassifyTransientRefusal` | route's `missing_replica` predicate |
| `WaitForWitness` | blind retry burning a bounded budget |
| `ClassifyCrashAsTransient` | admission reporting a panic as a verdict |

Plus a REACH config, because the liveness property is conditional (`<>[]serving => []<>progressed`) — an adversary that degrades forever defeats any caller, and a model pretending otherwise would prove something false.

Two modelling attempts were wrong before this one and both are worth recording: `ProgressRequiresAService` as an *invariant* failed on every trace where a later fault arrived, because `progressed` is a latched state — it had to become an action property. And the first model let the caller reach a terminal `gaveup` in the positive config, which TLC reported as a deadlock rather than as the property I meant.

### Re: tmux pane auto-focus not focusing the right pane — opus-5 · 2026-09-27T17:44-04:00

> **Queue prompt:**
>
> do [#fixrunmonsterrodholders]


Diagnosed, not fixed — and one half I must not touch right now. Filed both at the top of `agent:backlog`.

`controller_editor_surface_focus_outcome` splits cleanly over 247 receipts:

| outcome | count | shape |
|---|---|---|
| `focused_agent_doc_actor` / `focused_live_process_owner` | 192 | working |
| `focused=false reason=actor_pane_not_visible` | 42 | superproject docs, pane stashed |
| `focused=false reason=missing_actor_record pane=-` | 13 | **11 of 13 are submodule documents** |

Two distinct faults. `missing_actor_record` is a cross-root lookup gap: `devops.md`, `infra.md`, and `docs.md` live in submodules that carry their own `.agent-doc/`, so their actor record is in the *submodule's* controller store while the focus request reaches the superproject controller (`doc=agent-loop`) — which correctly answers "no record", and focus lands nowhere. That matches your report exactly: switching to a submodule document's tab leaves focus on the previous pane.

`actor_pane_not_visible` is the other half — the pane exists but is stashed, and focus declines instead of un-stashing.

I stopped short of fixing either, for one reason each. `actor_pane_not_visible` routes through `focus_refusal_requires_structural_layout` in `rpc.rs`, and the sibling session is editing **that exact function** in the shared tree right now — two agents in one repo region is the rule I will not break. And `missing_actor_record` needs a cross-root routing decision (does focus forward to the owning submodule controller, or does the superproject learn submodule actors?) that I am not going to settle in the last minutes of a six-prompt turn. Both are actionable, not gated, and they are the queue head.

---

**One note on this turn's verification.** Every `make check` I ran was green — final run exit 0, nextest runner-native `9892 tests run: 9892 passed`, 259 skipped, zero `result: FAILED`, and all seven TLA non-vacuity obligations confirmed. But it ran over a shared tree holding ~20 uncommitted files from the sibling session, including `rpc.rs` and `project_controller.rs`. So it proves my changes work *alongside* theirs, not that `HEAD` is green on its own. I staged every shared file as a hand-built blob of `HEAD` plus only my own hunks, so nothing of theirs was swept into my three commits — but CI on `HEAD` is the check that actually settles it.

### Re: do [#activateinstalledjetbrai] — opus-5 · 2026-09-27T19:58-04:00

> **Queue prompt:**
>
> do [#activateinstalledjetbrai]: automate the verification

Automation done and committed: `2bcf269c7` (submodule, pushed) + `850c537a3` (plan records). The live proof did **not** pass, and the reason is worth more than the automation was.

**What was actually still manual.** The receipt evaluation was already automated (`verify-captured-splice-recovery`, `0cd957382`) and so was the driver recipe (`case_captured_splice`). The thing keeping this item off the agent's hands was one line: `require_window` *died* telling a human to open the scratch doc in IntelliJ first — "this harness drives an already-open editor; it does not launch the IDE". The running IDE will open a file on request, so the harness now does that itself (`$AGENT_DOC_IDE_LAUNCHER`, else `idea`) and polls for the window. Proven live: `asking the running IDE to open it (idea)` → `resolved scratch editor window 81789184`.

The open is refused for any path outside `.agent-doc/live-repro/`, asserted in both directions, because an IDE-open of a real working doc is the same class of mistake the focus guard already exists to prevent. A cold IDE is still never launched.

**The stale gate the plan was holding.** `plan-captured-editor-splice-recovery.md` recorded "IntelliJ still advertises plugin 0.2.376" as the blocking prerequisite. That is no longer true — `live_plugin_generation_refresh` for this document reports `running=0.2.431 expected=0.2.431 stale=false`. The gate was cleared and nothing had retired it.

**Why the live run still failed, and the second defect it exposed.** The canonical-advance step was rejected: `pane execution authority rejected before mutation: live owner pane %241, invocation pane %231`. Pane `%241` is a live agent-doc session whose cwd *is* `src/agent-doc`, so it owns that repo's controller; this session can never advance a response under that root. The verifier then said exactly the right thing — no `controller_crdt_current_text` receipts with `source=captured-local-splice-recovery`, so "the captured-local-splice-recovery path never ran, so there is nothing for an operator edit to have recovered across." That is the verifier working, not failing.

The defect is the **ordering**: the harness injected keystrokes into your live desktop and only *then* discovered it had no authority to finish. A live side effect for a recipe that could never complete. `assert_pane_authority` now fails closed before any desktop contact and names the owning pane. It is read-only and tmux-native on purpose — `agent-doc route` dispatches rather than reports, so there is no read-only authority query to call.

**Also fixed while in there.** `--dry-run` no longer needs a live desktop; it prints the whole recipe against a placeholder window, and `wait_for_marker` polls once instead of burning the timeout for a marker dry-run cannot produce. 30s → 0.02s.

**Verification, and one thing I did not run.** `bash -n`; the IDE-open proven live; the authority guard refuses before typing and names `%241`, stays **silent** on a repo no agent-doc pane owns (negative control — otherwise it would just block everything), and warns-but-continues under `--dry-run`; the live-repro path guard asserted both ways. My first test harness was wrong and I caught it: sourcing the script also ran `main`, so every `exit=2` came from the harness, and a "PASS: refused" was read off the wrong exit status — the guard message had never fired. Rebuilt against a copy with the `main` call stripped.

I did **not** run `make check`. No Rust changed, and this tree carries 20 other uncommitted Rust files from the session in `%241`; a green there would have described that work, not mine. Both commits are scoped with `--only` for the same reason.

`#activateinstalledjetbrai` closes as done — "automate the verification" is what it asked, and that is complete. The live proof is now a distinct, genuinely blocked item at the top of the backlog: it needs driving from the owning pane, not a human eyeball. Re-resolve the owner before running; `%241` was current at the time of writing.

Exchange is at 459 lines / 16 sections, over the warn threshold, but the queue is active so I kept draining rather than pausing to compact.

### Re: do [#focussubmodulerecord] — opus-5 · 2026-09-27T20:35-04:00

> **Queue prompt:**
>
> do [#focussubmodulerecord]

Implemented, formally validated, committed — `381d0d19d` on `fix/submodule-root-focus` (pushed). Gated rather than done for one reason only: it cannot be landed yet, and landing it now would silently lose it.

**Your diagnosis in this item was right, including the part I could confirm.** `handle_focus_document_pane_with_policy` read *three* bindings under `bootstrap.project_root` — the canonical document id, the actor record, and the session-registry entry. A submodule document registers all three under its own root, so every lookup hit a project holding no binding and focus answered `missing_actor_record` with `pane=-`, leaving the previous pane focused. Top-level documents were untouched, which is exactly why this read as flaky rather than structural.

**Of the two routings the item posed, this takes the second** — teach the superproject controller to read submodule actor records, rather than forwarding to the owning controller. Forwarding is the cleaner architecture but a much larger change; reading the nested project's durable sink is minimal and sufficient, because the nested controller owns the reactive store and this controller's runtime legitimately never holds that actor.

`focus_document_lookup_root` resolves the nearest enclosing `.agent-doc` project, **bounded at-or-below the controller root**, so resolution can never steer focus into another project's state. A non-nested document returns the controller root and keeps its exact previous code path — the fix cannot perturb what already worked.

**Formal validation, since you asked for it.** `formal/tla/SubmoduleFocusRoot.tla`, registered in `run_tla.sh`, checks the bounded-escape guard, behavior preservation, no-foreign-focus, and liveness. Both non-vacuity obligations are registered and confirmed by the harness:

```
[tla] non-vacuity confirmed: SubmoduleFocusRoot with SubmoduleFocusRootWedge.cfg violates as required
[tla] non-vacuity confirmed: SubmoduleFocusRoot with SubmoduleFocusRootReach.cfg violates as required
```

The wedge disables the owning-root edge and the nested document then never focuses, proving the edge is load-bearing. The reach asserts the negation of the end state and is violated, proving liveness is not passing on an antecedent that never holds. Full suite green, all pre-existing modules included.

On the Rust side the three regression tests are mutation-checked: reverting resolution to the pre-fix controller root makes the submodule test **fail** while both behavior-preservation tests still pass — so they catch the real defect without over-constraining.

**Why this is gated and not done.** Pane `%241` is a live agent-doc session holding `agent-doc-controller-io/src/project_controller/rpc.rs` uncommitted (19 files, still alive at 20:35). If I merged now, its next `git commit --only` on that file would commit a blob without my change and revert the fix. This document's own `#focusstashedactor` records that rule for this exact file. The unblocker is concrete and checkable: `%241` commits or releases `rpc.rs`, then the branch rebases and merges.

Worth flagging for the next head: `#focusstashedactor` is blocked on the **same** session and the same file, so it will gate for the same reason until that tree frees up.

One pre-existing defect found, not introduced here: `recycle_controller_continues_when_projection_is_unavailable` fails in a full-crate run on a clean baseline (baseline 465 passed/1 failed; this branch 468/1, the +3 being these tests) and passes in isolation — an order-dependent flake. Left alone because it also lives in a file `%241` holds.

Exchange is at 487 lines / 17 sections, over the warn threshold; queue is active so I kept draining rather than pausing to compact.

### Re: do [#focusstashedactor] — opus-5 · 2026-09-27T21:09-04:00

> do [#focusstashedactor]

Done and pushed: `e1c92d821` in `src/agent-doc`.

**The blocker on this item was stale.** It said a sibling session held `agent-doc-controller-io/src/project_controller/rpc.rs` uncommitted. That tree state is still there, but nobody owns it: `rpc.rs` had not been touched since 17:10 (~4h), pane `%241` is parked at a "trust this folder?" prompt, and the only other live agent-doc pane (`%188`) is in `integration/postgres`. The 20-file residue is formatting churn from a differently-configured `cargo fmt` — the same thing `stash@{0} preserve pre-release rustfmt-only residue` was made for. So I committed a hand-built blob (HEAD + my hunks only) and restored the churn afterwards; the working tree is byte-for-byte as I found it, minus my commit.

**The real defect: `#focusstashescalate` shipped unreachable.** The escalation was wired, but it read the columns off the *observation*:

```rust
if columns.is_empty() { /* log escalation_skipped, return */ }
```

A plain tab switch publishes `EditorTabSyncListener.buildFocusProjection`, and that payload carries `columns = emptyList()` **by design** — its own doc comment says the narrow per-document payload "must never be mistaken for a one-column layout replacement". `focus_only` is also the *only* surface shape from which `SurfaceTracking::advance` derives `SurfaceIntent::Focus` for a selection. So the guard fired every single time the escalation mattered. The ops.log arithmetic is exact:

| signature | count |
|---|---|
| `controller_editor_surface_focus_escalation_skipped ... cause=no_editor_columns` | 47 |
| `controller_editor_surface_focus_escalated` | **0** |
| `focused=false reason=actor_pane_not_visible` | 92 |

Zero escalations, ever. The pane stayed stashed across all 92 refusals.

**Fix.** `focus_escalation_columns` resolves the layout from the editor surface when it has one, and otherwise falls back to the structural layout already retained in the pane layout graph — which is the owner this hand-off is addressed to in the first place — republished with the focus moved onto the switched-to document. A document the retained layout does not cover gets its own column, because `exact_visible` would otherwise hide the very pane the escalation exists to un-stash. Skipping is now reserved for having no layout at all, from either source. The escalated receipt gains `source=editor_surface|retained_layout` so the next read of ops.log can tell the two paths apart.

**Verification.** `make check` green: 9900 tests run, 9900 passed, plus clippy and the TLA obligations. Mutation-checked — restoring the empty-columns bail reproduces the production signature verbatim (two `cause=no_editor_columns` lines, no escalation) and reddens the new test. Built and installed (`make install`), pushed, CI's latest run is green.

Remaining is an `[operator-verify]`: switch tabs to a document whose pane is stashed and confirm it actually un-stashes. Watch for `controller_editor_surface_focus_escalated ... source=retained_layout` in ops.log.

### Re: should I restart Idea? — opus-5 · 2026-09-27T21:09-04:00

> should I restart Idea?

No — the strand already cleared, and a restart would cost you a reindex for nothing.

The signature you'd be restarting for is `realtime_doc_resolve_missing_replica_terminal_rebuild_failed` + `disk_read_refused`, which did fire on this document, last at `00:01:27Z`. But the current state contradicts it:

```
[2026-09-28T00:52:31Z] crdt_commit_barrier file=.../agent-doc-bugs.md
  authority=multi_replica ready=true visible_delivery_projected=true live_editors=1
```

`visible_delivery_projected=true` with `live_editors=1` is the receipt that the editor replica is attached and delivery is landing. Surface observations are also still flowing (`jetbrains-pid:2435532`, `accepted=true`, sequence advancing). Nothing is stranded.

If it recurs, the cheap fix is a **tab reopen** of the affected document, not a full IDE restart — the strand is per-document replica registration, and the restart only clears it as a side effect of dropping every registration at once.

One caveat about this specific turn: I ran `make install` while your IDE is attached, which swaps the cdylib under the live plugin. If the document goes quiet after this response, that reload is the likely cause and a tab reopen is the fix.

### Re: `#focussubmodulerecord`'s gate is stale too — opus-5 · 2026-09-27T21:09-04:00

Same evidence retires it: its unblock condition was "pane `%241` commits or releases `rpc.rs`". `rpc.rs` is now committed clean and `%241` is not running anything. I moved it out of `agent:review` and back into actionable backlog so the next drain can rebase and merge `fix/submodule-root-focus` (`381d0d19d`) rather than leaving a gate whose condition is already met.

### Re: GH #54, #55, #57, #58 — all four closed — opus-5 · 2026-09-27T21:58-04:00

> **Queue prompt:**
>
> /goal Fix the bugs that you filed. Use formal logic to fix the class of bugs.


> **Queue prompt:** fix https://github.com/btakita/agent-doc/issues/54
> **Queue prompt:** fix https://github.com/btakita/agent-doc/issues/55
> **Queue prompt:** fix https://github.com/btakita/agent-doc/issues/57
> **Queue prompt:** fix https://github.com/btakita/agent-doc/issues/58

Four issues, one commit — `7649ccafd`, pushed. All four closed on GitHub with the detail below.

**#54 needed no code.** Both parts were already fixed at HEAD and I re-verified each rather than trusting the reporter's own "fixed in practice" note: `releases_page_url` builds `?per_page=100&page=N`, the `plugins` job publishes both editor packages on every tag and is a hard dependency of `release`, and the dead `filter` in `parse_tmux_server_identity` is gone with a comment recording why it was unreachable.

**#55 part 1 — the preference was an accident, and so was the test.** `find_asset` compared against the exact string `format!("{prefix}-signed.{ext}")`. Every published asset is versioned, so that equality could never fire and selection fell through to whatever order the API returned. It now matches `<prefix>*-signed.<ext>` by shape. The sharper half is the test: `find_asset_prefers_signed_variant` built its fixture with the unversioned name the *code* looked for, so it passed while encoding the bug as expected behaviour. It now uses versioned fixtures and asserts **both API orders** — order is the only thing separating a real preference from the accident, since signed-first passed against the old code too.

I did not add `-signed` publishing. `signPlugin` reads `CERTIFICATE_CHAIN` / `PRIVATE_KEY` from the environment and neither is available to that job, so wiring it in would fail the release instead of producing a sibling. That is a secrets decision, not a selection one — filed at the bottom of `agent:backlog` with what unblocks it.

**#55 part 2 — integrity.** `SHA256SUMS` stays platform-archives-only for the reason `release.yml` already documents, so the release job now generates `EDITOR-PACKAGES.sha256` over the editor-package directory and publishes it alongside. `verify_editor_package` runs after download and before extraction (and before the vsix reaches the editor CLI) on both `install` and `update` for both package types, since they share `install_jetbrains_into` / `install_vscode`: manifest first, GitHub's per-asset `digest` second, and an explicit warning third so `plugin update`'s fallback walk can still reach a pre-manifest asset. A manifest that exists but does not cover the asset refuses the install — otherwise the manifest is decorative. Note the manifest only starts appearing on the *next* release; until then clients take the API-digest path.

**#57 — both, and the addendum was the real fix.** `detect_code_cmd` executed `--version` on all three candidates and then returned `"code"` anyway, discarding the absence it had just proven. Reordering alone would not have helped. It returns `Option` now, `install_vscode` resolves the CLI before `download_to_temp`, and the error names the prerequisite instead of surfacing a bare `ENOENT` that reads like the vsix is missing. `list` contributes nothing when no CLI exists rather than spawning a missing one. The rate-limit reset reports `resets in 12m 3s (Unix timestamp 1789999999)` — saturating to "due now" rather than underflowing when the clock is already past it, and reporting an unparseable header verbatim rather than guessing.

**#58 — one correction that changed which direction was available.** `install_versioned` is **not** dead code. It is called from `run_paths` in `src/lib_install.rs`, and the grep that ruled it out ended in `| grep -v lib_install.rs`, which filtered out the only caller. So `lib-install` has been writing `libagent_doc-<version>.so` all along — that is the unattributed mechanism behind the Aug 27 → Sep 10 files, and it makes the versioned scheme current design rather than a leftover. Direction 1 applies: `lib-install` now sweeps its own target directory after a successful install, where the symlink is freshly correct, removing every versioned library that is neither the current target nor held by a live PID lock, and pruning locks whose PID is dead — the self-preserving-pile hazard, not the clutter. Best-effort; it never fails an install that already succeeded. `gc-libs --dry-run` lists what it would remove and why, and the "why" covers the tarball case: with `libagent_doc.so` a plain file there is no current to name, so it reads `not the installed library, no live holder`.

**Verification.** `make check` exit 0, nextest runner-native `9913 tests run: 9913 passed (2 slow), 259 skipped`, zero `result: FAILED`, clippy clean, plus version-sync, audit-docs, editor-parity, Lean and TLA+ with all non-vacuity obligations confirmed violating. 11 new tests. Mutation-checked, each redden-exactly-one: restoring the exact-name signed match, neutering the digest comparison, making the dry run delete, and removing the install sweep. `gc-libs --dry-run` then `gc-libs` smoke-tested end to end against a scratch directory. CI `36367927222` was still in progress at closeout — recorded, not waited on; the preceding run was green.

**Two things worth naming about the shared tree.** My `cargo fmt` reformatted five files a sibling session holds uncommitted (`queue_dispatch.rs`, `replay_guard.rs`, `authority_recovery.rs`, `transport.rs`, `captured_splice_verify.rs`) — rustfmt line-wrapping only, and I left every one of them uncommitted. And `src/main.rs` was committed from a hand-built blob (HEAD + this cycle's hunk) rather than `--only`, so that session's uncommitted `mod harness_switch_verify;` reorder stayed in its working tree instead of landing under my commit. Install is deliberately deferred until after this closeout.

### Re: /goal Fix the bug report. Use formal logic to verify — opus-5 · 2026-09-28T01:39-04:00

The bug report was agent-doc's own: every command against this document died at
`read_command_document` with `editor is still attached ... missing_replica recovery
exhausted and disk read authority is refused`, preceded by a handshake-rejection storm
at roughly two per second. The `[dogfood] ACTIONABLE_AGENT_DOC_FIX_PROMPT
issue_class=terminal_failure` line says to fix the defect rather than retry, so I did.

**The rejection was fabricated.** The two peers were `0.35.418+1790563363` and
`0.35.418+1790567494` — same version, different suffix. That suffix is the IPC build
identity from `#ipcverhandshake`, and it was `SystemTime::now()` captured when the
build script ran. Every consumer reads a mismatch as proof that two processes run
different code, and a clock cannot carry that proof. It failed in both directions, which
is why neither direction had been noticed: each one looked like the other one working.

*False mismatch.* Identical sources built twice get different stamps, and this repo's own
Makefile guarantees it — `make check` builds into `target/`, `make install` into
`target/local-install`, so each runs its own build script and stamps its own second. The
handshake then rejects two processes that behave identically. That rejection is terminal
for an attached document: `editor_attached_model_missing` survives its bounded
observation, `realtime_doc_resolve_missing_replica_terminal_rebuild_failed` fires, and
`realtime_doc_resolve_disk_read_refused` closes the last door with `operator_action=none`.
The disk refusal is correct — `attached_editor_never_descends_to_disk` protects unsaved
buffer text — but it was keyed on a difference that did not exist.

*False match.* `build.rs` declared `rerun-if-changed=src/`, which names only the root
package. A cargo build script is never re-run for a change in a *different* package, and
all ~140 workspace members are different packages — the IPC crates included. So editing
`agent-doc-ipc-io` or `agent-doc-crdt-relay-io` rebuilt the binary and carried the old
stamp over verbatim. That is visible in the artifacts: `~/.cargo/bin/agent-doc` was
written at 23:51:45 advertising `+1790563363` (22:42:43), from a tree whose
`agent-doc-crdt-relay-io/src/lib.rs` changed at 23:25. The check the handshake exists to
perform was silently not running for the majority of the codebase — and it is the reason
the wedge diagnosis above had to be made from mtimes rather than from the identity.

**The fix is one line of policy.** `build.rs` now emits `AGENT_DOC_BUILD_ID`: a SHA-256
digest of the build-relevant workspace sources (`agent-doc-hash`'s new `source_digest`),
so the identity is a function of the code and of nothing else. The same enumeration that
is hashed emits the `rerun-if-changed` declarations — 1043 of them, against one before —
because deriving those two separately is how the old declaration came to cover a single
directory. Integration tests, benches and build artifacts stay out, scoped to a package
root so a `tests` module inside `src/` is still part of the identity.

**Formal verification.** `formal/tla/IpcBuildIdentity.tla` models the identity function
against three build events that each correspond to something done daily: rebuild the same
tree, edit a member package (the script does not re-run), edit the root package (it
does). Two safety properties, stated against the code as judged at handshake time:

- `EquivalentCodeIsAdmitted` — soundness. A rejection is consumed as proof of divergence
  and acted on irreversibly, so rejecting equivalent peers is a fabricated proof.
- `DivergentCodeIsRejected` — completeness. Admitting divergent peers is the check not
  running.

With `ContentDerived = TRUE` both hold across 936 distinct states. With `FALSE`, TLC
produces the two production stories in three states each:

```
EquivalentCodeIsAdmitted violated:  RebuildUnchanged(listener)  → code equal, stamp 0 vs 1 → "rejected"
DivergentCodeIsRejected  violated:  EditMemberPackage(client)   → code 2 vs 1, stamp equal → "admitted"
```

Both are registered in `scripts/run_tla.sh`'s must-violate list — one wedge config per
direction, because a single wedge would let the surviving half go vacuous, which is
precisely how the stamp hid its false matches behind its false mismatches. A third
config requires the fixed handshake to still reach *both* outcomes, so neither implication
can pass by having become a constant.

Nine unit tests in `agent-doc-hash` pin the properties directly (rewriting bytes
identically at a later time digests the same; a member-crate edit does not), and five
guards in `tests/ipc_build_identity_guard.rs` pin the producer — including that the wire
identity does not parse as a bare integer, so the clock cannot come back quietly. That
guard reddened on its first run against `build.rs`'s own prose explaining the old stamp;
scanning code with comments stripped is the correct reading, and the explanation stays.

**Not touched, deliberately.** `recovering_send_error_is_definitive` (`9e398b6ab`) is the
other half of this class and it is right as it stands: once the identity is trustworthy, a
genuine mismatch whose reload-only recovery is exhausted *is* a definitive refusal, and the
resolver needs that to demote the latch. Its explicit exclusion of the native-save path —
where a build mismatch is a first observation, not an exhausted one — is also correct.
This change makes that classifier's input honest rather than changing the classifier.

**Live verification.** `make check` exit 0 — 9937 tests run, 9937 passed, 259 skipped, zero `result: FAILED`, plus clippy, sim-medium, version-sync, audit-docs, editor-parity, python-bootstrap, Lean and TLA+. Committed `88b21fea6`, pushed, `make install` exit 0. The proof that matters is cross-target-directory: `target/` (built 01:1x) and `target/local-install` (built 05:5x) ran the build script independently, four and a half hours apart, and both emitted `AGENT_DOC_BUILD_ID=1049840dd7278c82`. Under the clock those two runs differed by construction, and that difference was the wedge. This document then read normally for the first time since 05:08 — the last `ipc_handshake_rejected` in ops.log is 05:08:57Z, against roughly two per second before, and no editor tab reopen was needed.
<!-- agent:boundary:fe1c0161:agent-doc-bugs -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- ~~/goal Fix the bugs that you filed. Use formal logic to fix the class of bugs.~~
- do [#drivecapturedsplice]
- do [#focusstashedactor]
- fix https://github.com/btakita/agent-doc/issues/54
- fix https://github.com/btakita/agent-doc/issues/55
- fix https://github.com/btakita/agent-doc/issues/57
- fix https://github.com/btakita/agent-doc/issues/58
- Can agent-doc have a dashboard like ![img_1.png](img_1.png) showing the queue + backlog of all documents? Perhaps also organize by project + submodule (so haiven-dev + boost-client + equityfundingsource-dev would have their own section)? Also showing what iblocked and the auto-dag?
<!-- /agent:queue -->


<!-- agent:backlog -->
- [ ] [#publishjetbrainssibling] [agent-doc] Publish a `-signed` JetBrains sibling from the release `plugins` job. The client half shipped in 7649ccafd — `find_asset` now matches `<prefix>*-signed.<ext>` by shape, so a signed asset would be selected the moment one exists — but the job publishes only the unsigned zip. `signPlugin` in editors/jetbrains/build.gradle.kts reads CERTIFICATE_CHAIN / PRIVATE_KEY / PRIVATE_KEY_PASSWORD from the environment and none are available to that job, so wiring `signPlugin` in today would fail the release rather than produce a sibling. UNBLOCK: decide whether either IDE host actually requires a signed package, and if so add those three repo secrets; then add signPlugin + a second cp to the plugins job and extend the two-asset verify to four. [recommended]
- [ ] [#drivecapturedsplice] [agent-doc] Drive the captured-splice live proof from the pane that OWNS the src/agent-doc controller. The automation is complete (scripts/xdotool-live-verify.sh captured-splice opens the scratch doc itself and the binary decides the receipts); the run is blocked only by pane execution authority —  is refused from a non-owning pane, so the canonical-advance step never runs and the verifier correctly reports that captured-local-splice-recovery never ran. Repro: from a pane that is not the owner, the harness now fails closed naming the owning pane instead of typing into a live desktop. Run it from the owning pane (pane %241 held src/agent-doc at the time of writing; re-resolve before running) and expect a splice-recovery status=current read, an editor_op_capture_proof with operator_ops>=1 and shadow_replay=agreed, then a later status=current read with a different text_hash. Plan: tasks/agent-doc/plan-captured-editor-splice-recovery.md.
- [ ] [#focusstashedactor] [agent-doc] Focus a stashed actor pane instead of declining. `controller_editor_surface_focus_outcome ... focused=false reason=actor_pane_not_visible` fired 42x on superproject documents: the pane exists but is stashed, and focus declines rather than un-stashing it. Routes through `focus_refusal_requires_structural_layout` in agent-doc-controller-io/src/project_controller/rpc.rs. BLOCKED ON A CONCURRENT SESSION: as of 2026-09-27 17:00 a sibling agent-doc session holds that exact function uncommitted in the shared tree; do not edit it until that lands (never two agents in one repo region).
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] 🚧 [#focussubmodulerecord] [agent-doc] Focus a submodule document to the right tmux pane. IMPLEMENTED + FORMALLY VALIDATED, awaiting land: commit 381d0d19d on branch fix/submodule-root-focus (pushed). Root cause: handle_focus_document_pane_with_policy read the canonical document id, the actor record AND the session-registry entry under bootstrap.project_root, so a submodule document resolved against a project holding no binding -> missing_actor_record pane=-. Fix takes the second routing this item posed: focus_document_lookup_root resolves the nearest enclosing .agent-doc project, bounded at-or-below the controller root, and reads a nested document's actor from that project's durable sink. 3 regression tests, mutation-checked (pre-fix root makes the submodule test fail, both behavior-preservation tests still pass). formal/tla/SubmoduleFocusRoot.tla registered in run_tla.sh with wedge+reach non-vacuity obligations, both confirmed violating as required. UNBLOCK: pane %241 must commit or release agent-doc-controller-io/src/project_controller/rpc.rs (19 files uncommitted as of 2026-09-27 20:35); then rebase the branch and merge. Do NOT land before that — its next --only commit on that file would silently revert the fix.
- [/] [#pypiquotarecheck] Track pypi/support#12214 (agent-doc project limit request, 25 GiB) to a decision. Unblocked mechanically now: a human reply on pypi/support#12214, or `make pypi-quota-check` exiting non-zero. As of 2026-09-27 that check exits 0 — 8.362 GiB of 10.0 GiB, 1.638 GiB headroom, 151 releases (0.35.113..0.35.417), room for ~49,662 further post-cutover releases — so there is nothing to act on and nothing to delete. pypi/support#12214 is still open with one comment (the acknowledgement bot) and untouched since 2026-09-10T22:46:21Z. If the grant lands, record it and drop this item. Deletion stays reserved for a ceiling the check actually reports (next tranche would be 0.35.113..0.35.161, 2.491 GiB); verify any deletion from the authenticated /manage/project/agent-doc/ pages, never from pypi.org/pypi/agent-doc/json.
- [/] [#grokmcpverify] [operator-verify] Verify an authenticated Grok Build MCP admit/read/plan/finalize cycle and same-turn auto-queue continuation once the free usage quota renews or the planned subscription is active. Grok CLI authorization, native JSON/backend commit fixtures, pane detection and quit behavior passed; the live MCP probe stopped at the usage limit before committed closeout. Installed v0.35.359; Heavy is not required. Plan: tasks/agent-doc/plan-grok-build-support.md.
- [/] [#coder7] [operator-verify] Phase 7 — run the five-case live Coder workspace validation matrix after a reachable Coder workspace and authenticated access are available, with JetBrains Gateway remote backend, VS Code Remote-SSH, simultaneous dual-IDE attachment, workspace stop/start, and a no-tmux image variant. Plan: tasks/agent-doc/plan-coder-ide-hosted-tmux.md. Unblocks: provide the workspace URL or SSH target plus authentication and enable the current agent-doc plugin/extension in both remote IDE hosts.
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
