# agent-doc Hardening Runbook (pure Claude Code session)

**Run this in a PURE Claude Code session, not a managed `/agent-doc` session.**
Open `claude` in `src/agent-doc` (the submodule) and execute phase-by-phase. The work
edits the exact code paths a managed session uses to drive itself (controller lifecycle,
finalize/IPC, route dispatch), so dogfooding agent-doc while rewriting them is what has
been corrupting the session doc. No queue, no finalize boundary, no IPC listener here —
just `git`, `cargo`, `make check`, and SimWorld.

Keep `tasks/agent-doc/agent-doc-bugs2.md` as the **triage record** in a separate managed
session if you like, but do the implementation here and mark items done afterward.

---

## Global rules (apply to every phase)

- **One phase per commit.** Don't bundle unrelated fixes.
- **`make check` must be green before every commit.** Capture `MAKE_CHECK_EXIT=$?` explicitly —
  the RTK wrapper can report exit 0 on FAILED tests; also `grep` output for `result: FAILED`.
- **Live-class behavior gets SimWorld coverage**, not a promise of manual verification.
- **Build + install after source edits** (both binaries + cdylib), every phase that ships code:
  ```
  cargo install --path .          # ~/.cargo/bin (tmux-spawned sessions)
  cargo build --release           # .bin/agent-doc -> target/release
  agent-doc lib-install           # refresh the JB cdylib (mtime hot-reload)
  ```
- **Recycle controllers** after install when the change touches controller/IPC code:
  `pkill -f 'agent-doc controller serve'` (they relaunch clean on next invoke).
- **Bump `VERSIONS.md`** before any publish. Don't publish from this runbook unless a phase says so.
- After each phase: check CI with `gh run list --workflow CI --limit 1` and fix red before moving on.

---

## Phase 0 — Re-baseline the dirty tree ✅ DONE (2026-06-12, commit `2b643683`)

**Outcome (supersedes the stale premise below):** the dirty tree did **not** carry `#fintol` WIP.
It carried a complete, green **Phase 2 `#stuckhandoff2` M4+M5** implementation (182 lines in
`project_controller.rs` + `gc.rs`) — M4 handoff drop-guard (`HandoffDropGuard`) and M5 cross-project
`/proc` sweep (`reap_orphaned_preparing_controllers_all_projects`, called from `gc::run`). `make check`
was green as-is. Per the operator's call, deterministic coverage was added first (M4 abort-sends-shutdown
+ complete-suppresses, M5 `controller_serve_project_root_from_args` parsing, plus an `#[ignore]`d live
cross-project reap sweep that runs in the `make check --ignored` leg), then committed as `2b643683`
(`make check` green, 4337 passed). **M2 (gate-on-Stable) is still outstanding** — Phase 2 now means M2
only. Submodule `main` is ahead of `origin/main` by 3 (unpushed). Not yet built/installed/recycled live.

---

**Original premise (STALE — kept for history):**

**Why:** the submodule working tree currently carries uncommitted `#fintol` work-in-progress —
~311 insertions across `agent-doc-orchestration/src/write.rs`, `src/sim_world.rs`,
`tests/finalize_integration.rs`. Every other phase builds/tests on top of this, so it must be
resolved into a known state with a green `make check` baseline. Prior audit finding: naive
forward-merge wiring (`#fintol2`/`#pcwc`) **breaks 7 shipped tests** because a post-commit
worktree legitimately differs from HEAD via carried-forward uncommitted edits. So do not assume
the WIP is correct.

**Steps:**
1. Inspect the WIP: `git -C src/agent-doc diff` — characterize whether it's a coherent partial
   `#fintol2` (forward-merge-at-gate) or scratch work.
2. `make check` as-is. Record the result.
   - **Green** → commit it as an explicit checkpoint: `agent-doc(#fintol2): WIP forward-merge groundwork`
     (or finish the rung in Phase 4 and commit there). You now have a clean baseline.
   - **Red** (expected if it hits the 7-test carry-forward class) → **decide with the operator**:
     (a) finish `#fintol2` to green now as Phase 4 brought forward, or
     (b) `git stash` the WIP and start the other phases from the last clean commit (`08dfc27b`),
         returning to `#fintol` in Phase 4 from a clean base.
3. Confirm `git -C src/agent-doc status` is clean (committed or stashed) and `make check` is green
   before leaving Phase 0.

**Gate:** do not start Phase 1+ until the tree is clean and `make check` is green.

---

## Phase 1 — Make M1+M3 live (install + recycle)

**Why:** `#stuckhandoff2` M1 (controller self-watchdog) + M3 (orphan proc-scan reaper) are already
committed at `08dfc27b`, but the installed binary predates them, so running controllers still wedge.
This phase ships nothing new — it just makes the existing fix real and stabilizes any future
dogfooding.

**Steps:**
1. From `src/agent-doc`: `cargo install --path . && cargo build --release && agent-doc lib-install`.
2. `pkill -f 'agent-doc controller serve'` to recycle controllers onto the fresh binary.
3. Verify: `agent-doc --version` from `~/.cargo/bin`, and confirm no `--handoff-state preparing`
   procs survive a deliberate interrupted handoff (M1 self-SIGTERM within the deadline).

**Closeout:** no commit (install only). Note in the triage doc that M1+M3 are now live.

---

## Phase 2 — `#stuckhandoff2` remaining rungs: ~~M2~~ / ~~M4~~ / ~~M5~~ ✅ DONE

> **ALL THREE LANDED + SHIPPED (2026-06-12).** M4+M5 in Phase 0 closeout (`2b643683`); M2 in
> `3e0503ec`. Both pushed to `origin/main` (`08dfc27b..3e0503ec`), built/installed, controllers
> recycled (agent-loop controller PID 4168122 on the new binary, `handoff: stable`).
>
> **M2 scope note:** implemented as a `Stable`-gate on the **dispatch (mutating_command_admission)**
> path in `handle_dispatch` — a non-`Stable` controller refuses dispatch admission, records a
> `controller_not_authoritative` receipt + `dispatch_refused_non_stable_controller` ops-log line.
> Read-only/operator/handoff RPCs stay available. **Deliberately did NOT touch the finalize/commit
> worktree path** (the contested `#pcwc` carry-forward invariant) — the sub-plan flagged that the
> controller IPC handlers don't write the worktree directly, so the dispatch-admission gate is the
> in-scope symptom-killer. The worktree-write half stays with Phase 3 (`#pcwc`).

**Sub-plan:** `tasks/agent-doc/plan-stuck-handoff-hardening.md`
**Touchpoints:** `agent-doc-orchestration/src/project_controller.rs`, `gc.rs`

- **M2 — gate worktree/ipc writes on `Stable`** *(do first; symptom-killer, overlaps `#pcwc`)*: a
  non-`Stable` controller is non-authoritative for worktree writes, so even a still-wedged
  `Preparing` controller can't corrupt the tree between wedge and reap. This directly shrinks the
  `#pcwc` post-commit-drift window, so landing it before Phase 3 reduces that work.
- **M4 — client handoff drop-guard**: RAII guard that sends `retire`/`shutdown` if the handoff is
  dropped before `promote_handoff` — prevents the wedge at the source.
- **M5 — cross-project gc sweep**: gc reaps wedged controllers across all known project roots, not
  just the triggering one.

**SimWorld:** deterministic coverage for (a) `Preparing` controller cannot write the worktree,
(b) dropped handoff triggers the drop-guard, (c) cross-project sweep reaps an orphan in another root.
**Closeout:** `make check` green → commit `agent-doc(#stuckhandoff2): M2 gate-on-Stable + M4 drop-guard + M5 cross-project sweep` → build/install/recycle → CI green.

---

## Phase 3 — `#pcwc` post-commit worktree auto-remediation

**Sub-plan:** `tasks/agent-doc/plan-claudescore-3-post-commit-head-boundary-drift.md` (open the
'auto-repair' checkbox)
**Touchpoints:** `git.rs` (`emit_postcommit_worktree_check` already lands the detection at ~`:3336`)

**Goal:** when `postcommit_worktree_check match=false` but HEAD holds the correct committed response,
**auto-reconcile the worktree to HEAD** (via `show_head`) AND **push the committed content to the
editor buffer** so it stops writing stale content back — instead of warning + leaving a manual
`git checkout HEAD -- FILE`. The worktree reconcile is doable now; the editor-buffer push is the
durable half and overlaps `#rtwbcast`.

**Carry-forward caution:** this fights the same invariant as `#fintol` (a post-commit worktree can
legitimately differ from HEAD via carried-forward edits). Distinguish "HEAD authoritative, listener
corrupted the tree" (auto-reconcile) from "user has new uncommitted content" (do NOT clobber).
**SimWorld:** worktree==HEAD guard + a post-commit stale-buffer-writeback case that auto-reconciles +
a carried-forward-edit case that is preserved.
**Closeout:** `make check` green → commit → build/install/recycle → CI green.

---

## Phase 4 — `#fintol` finalize tolerance (forward-merge concurrent edits)

**Sub-plan:** `tasks/agent-doc/plan-finalize-tolerance.md`
**Touchpoints:** `agent-doc-orchestration/src/write.rs` (gate already auto-absorbs the benign
superset case `#exch-intermix` at ~`:7049`/`:7119` but FAILS CLOSED on a new exchange PromptTarget /
queue `do[#id]` the response doesn't own), `src/sim_world.rs`, `tests/finalize_integration.rs`

**Goal:** finalize should tolerate concurrent UNRELATED user edits (typing a new prompt, editing
elsewhere, compaction) by **forward-MERGING** them with the agent response instead of rejecting with
`live_prompt_drift_after_preflight` / exit-75. Add a `response_target_disjoint_from_user_edit`
primitive:
- **disjoint** (edit doesn't touch the response's CRDT target span) → CRDT-merge response+edit, commit
  the union, carry the new prompt to next cycle.
- **collision** (edit touches the response's target span) → keep fail-closed.

**Rungs:** `#fintol1` (conflict-scope primitive) / `#fintol2` (forward-merge at gate) / `#fintol3`
(SimWorld coverage). If Phase 0 carried `#fintol2` WIP forward, finish it here.
**HIGH RISK — the carry-forward invariant.** This is the class that breaks 7 shipped tests when wired
naively. Re-baseline green first (Phase 0), wire the disjoint/collision split, then make the 7 tests
green by construction (they encode legitimate post-commit divergence — the primitive must classify
them as disjoint/benign, not as drift).
**Closeout:** `make check` green (incl. the previously-broken 7) → commit → build/install/recycle → CI green.

---

## Phase 5 — `#qflood` controller dispatch flooding

**Sub-plan:** `tasks/agent-doc/plan-queue-dispatch-flood.md`
**Touchpoints:** controller dispatch / route path

**Goal:** a go-mode queue must not re-submit `agent-doc <FILE>` triggers into a running harness
session during finalize/queue-run (repro'd cross-harness on lazily-rs Codex; operator had to manually
delete prompts from the Claude Code queue). The desired behavior the operator stated: **"type while
the queue continues — it should not stop, and it should not flood."** Pausing the queue (the current
mitigation) over-blocks legitimate operator JB dispatch, so the real fix is dispatch backpressure /
dedupe, not a hard stop:
- suppress re-dispatch of a head whose cycle is already in-flight in the target session;
- don't re-append an un-completable head into a tight loop;
- let operator-driven dispatch through while auto-drain holds.

**SimWorld:** a go-queue with an in-flight head does not re-dispatch into the session; operator
dispatch is not blocked by auto-drain backpressure.
**Closeout:** `make check` green → commit → build/install/recycle → CI green.

---

## Phase 6 — `#efscodexsubmit` / `#2v7b` Codex JB submit re-submit fallback

**Sub-plan:** `tasks/agent-doc/plan-jb-codex-submit.md`
**Touchpoints:** `route.rs::send_command_unchecked`

**Goal:** `JB Run Agent Doc` on Codex (repro'd on `equityfundingsource.md`) enters the routed prompt
but does not submit it — the merged `text+\r` payload isn't treated as submit by the Codex TUI;
`send_command_unchecked` detects `prompt_not_submitted` (`TriggerStillVisible`) but has no re-submit
fallback. Fix: a Codex-scoped one-shot Enter re-submit + `route_submit_resubmit` proof logging +
SimWorld coverage.
**Live verify (operator-drive, after ship):** check `ops.log` for
`route_submit_resubmit action=enter_key result=accepted`.
**Closeout:** `make check` green → commit → build/install/recycle → CI green.

---

## Optional / blocked (do not start without an explicit decision)

- **`#snbc2`** (`plan-snapshot-baseline-consolidation.md`, Revised slice 2): OPTIONAL cosmetic —
  collapse `baselines/<hash>.md` into a snapshot-store variant (one file, not two) WITHOUT changing
  merge-base semantics. Moderate risk, touches the closeout path; gate behind NEW SimWorld coverage
  for the prompt-bearing finalize-merge class first. Skip unless the file churn is actually biting.
- **`#rtwbcast`** (`plan-realtime-editor-watcher.md`, Rung 4): multi-editor CRDT broadcast — NOT
  pure-Rust; blocked on an editor-plugin protocol decision (Option A: per-editor channel + `editor_id`
  ABI) **and** a two-live-IDE verify. Needs an operator design decision before coding. The committed
  editor-buffer push from Phase 3 is the first half.
- **`#w42v`** (`plan-jb-compact-crdt.md`): JB Compact Exchange CRDT path — the full-content editor-IPC
  path exists but is HARD-DISABLED by design. Re-enabling NEEDS OPERATOR DIRECTION before any code.

---

## Suggested order & dependencies

```
Phase 0 (re-baseline)  ──►  Phase 1 (install M1+M3)
   │
   ├─► Phase 2 (M2/M4/M5)  ──►  Phase 3 (#pcwc auto-repair)   [M2 shrinks #pcwc first]
   ├─► Phase 4 (#fintol)   [HIGH RISK; needs the green re-baseline]
   ├─► Phase 5 (#qflood)   [mostly independent]
   └─► Phase 6 (#efscodexsubmit)  [independent; route.rs only]
```

`#pcwc` (Phase 3) and `#fintol` (Phase 4) both fight the carry-forward invariant — keep their
disjoint/collision classification consistent (ideally share the `response_target_disjoint_from_user_edit`
primitive across both).
