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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260926-012634.md`*

Compacted content:
- Archived 28 response topic(s): actionable operator verification belongs in Backlog; cross-document backlog item moves; fpe.md terminal commit convergence; 25 more
- Prior summary/context: prior compacted content: Archived 8 response topic(s): recovered response; frontend recovery; Anonymous queue identit...

### Re: #advance-review, tmux pane auto-focus, and queue edit convergence — gpt-5 · 2026-09-25T21:55:57-04:00

> **Queue prompt:** #advance-review

Both regressions are fixed, tested, installed, committed, and pushed.

- **Tmux pane auto-focus:** IntelliJ was checking `currentWindow.selectedFile` on a generic next EDT turn, before real split/tab focus transfer had necessarily completed. Both tab-selection and editor-tree click ingress now wait for `IdeFocusManager.doWhenFocusSettlesDown`, then re-check the active split under the existing generation fence before publishing focus. The controller remains the sole pane-selection policy owner.
- **Queue edit duplication:** the progressive edits `Update 113` → `Rebase +` → `Rebase + update 113` were being assigned three text-derived identities while the merge base lagged. Queue reconciliation now treats the live operator free-text run as authoritative only within the same shared-anchor gap. Durable `#id` items and id-less additions in different gaps remain independent. The causally gated raced-projection repair also recognizes the non-prefix replace-then-combine shape only when the final spelling contains every earlier draft.
- **Live document safety:** I did not overwrite `infra.md`; its owning session advanced independently, and its current queue no longer contains the three intermediate drafts.

Architecture contract: the invariant is “the newest active IntelliJ session selection focuses its exact route-owned pane after IDE focus settles; stale, inactive, disposed, and non-session observations emit no focus.” The transition rows are generation-newer vs stale, active-split vs background-split, visible-owner vs stashed/missing-owner, and final operator wording vs stale id-less drafts. The reactive topology remains editor event Source → focus-settled, generation-fenced observation → controller Computed policy → tmux Effect → receipt Source. The only one-shot action was the diagnostic direct-focus probe; no polling loop, retry owner, or second pane policy was added.

Verification:

- Targeted JetBrains focus tests passed.
- Targeted CRDT, document-cell, and progressive queue-repair tests passed.
- Full `make check` passed.
- `make install` installed agent-doc 0.35.418 and dynamically upgraded the live JetBrains process to plugin 0.2.420; `/proc/3129637/map_files` confirms the 0.2.420 jar is mapped. No IDE restart is required.
- agent-doc commit `c4b62e733` and superproject pointer commit `44a8ae1df` were created; the agent-doc commit is pushed.

The remaining `#tmuxfocussyncverify` item is narrowed to one operator-visible check: click between the two live IntelliJ session tabs/splits and confirm tmux follows each exact document.

### Re: recovered response

Fixed and deployed.

- Right editor focus now selects tmux `%218`; `%215` is inactive.
- Passive layout publications log `focus_not_requested`, so they can no longer steal focus.
- Stop-hook retained-delivery and `write_applied` states remain on binary-owned recovery instead of falling through to “already continued once.”
- The original FPE retained cycle committed successfully; its current interruption is a newer `preflight_started` cycle.
- Full `make check`: 9,758 tests passed.
- JetBrains plugin 0.2.421 is loaded live.
- Pushed:
  - `agent-doc`: `22479d31f`
  - `agent-loop`: `853abdb79`

### Re: #advance-review — handoff to claude, and five plugin-liveness defects — opus-5 · 2026-09-26T14:48-04:00

> **Queue prompt:** #advance-review

Session picked up from codex. Advancing the review meant re-measuring the premises the gated items rest on — and every measurement came back wrong, because the code that produces those premises was reporting three mutually contradictory falsehoods about the same live IDE. Fixed all five, tested, installed, committed, pushed.

**What the turn's own preflight warnings claimed, and what was actually true.**

Live IDEA pid 3129637, measured directly:

| Claim | Source | Reality |
|---|---|---|
| "running superseded plugin bytes: 0.2.419 … its inode was unlinked" | `plugin_bytes_superseded` | pid 3129637 mapped `0.2.426.jar` **without** `(deleted)`. The six `(deleted)` entries are residue from earlier hot-reload generations. |
| "no JetBrains IDE process is running … any review premise is dead" | `session doctor` | The IDE was running with 472 threads and the plugin mapped. |
| "installed plugin postdates 1 live IDE … restart the IDE to activate" | `session doctor` | The IDE had already hot-loaded it. No restart was ever required. |

**Root causes.**

1. **`Current` was unreachable in production.** `/proc/<pid>/map_files/<range>` is `EPERM` to `stat()` without `CAP_SYS_ADMIN` — even for the owner of the process. `readlink` works, `stat` does not, so `mapped_inode` is always `None` and both inode-comparing arms of `classify_mapped_plugin_jar` were dead code. The `Current`-wins guard added in `840a02f42` could never fire, so a lingering `(deleted)` mapping won the fold. The kernel appends `" (deleted)"` only when the backing inode is unlinked, so its **absence** over a jar still on disk is itself the proof; that is now the classification.

2. **IDEA rewrites its own argv.** `/proc/3129637/cmdline` is the five bytes `idea\0` — no main class, no launcher path — so every path-shaped match in `jetbrains_ide_label` failed. Bare product names are now matched, and corroborated against `/proc/<pid>/maps` before counting (a bare `aqua` or `rider` is too generic to trust alone).

3. **The activation probe contradicted agent-doc's own feature.** Its premise was "a JVM loads plugin jars at startup", so a jar newer than the process start time meant "cannot have loaded it". agent-doc ships `JetBrainsPluginUpgradeBootstrap` precisely to load a new build into a *running* IDE. Mapped-generation evidence now outranks the timestamp heuristic, which survives only as the fallback when the mapping cannot be read.

Two more surfaced while reading that code:

4. **The documented "pure core" was not the core.** `stale_plugin_warnings` never called `stale_plugin_warnings_from_registrations`; the two message bodies had drifted, so every message assertion in the module tested text production never emitted. They share one body now.

5. **The remedy pointed at the wrong half.** The message hardcoded "older than" and said to update the plugin. Live was 0.2.426 against a binary shipping 0.2.422 — the plugin was the *newer* half, so "reinstall the plugin" was the one action that could not help. The message now names the measured direction and sends the operator at the half that is actually behind.

**Verification.** `make check`: 9766 tests run, 9766 passed, 259 skipped, exit 0 (runner-native count, independently asserted). New regression coverage pins each defect to the shape measured today, including a fold over the exact six-`(deleted)`-plus-one-live mapping set in both `read_dir` orders. Installed 0.35.418 / plugin 0.2.427; `session doctor` now emits no plugin-activation issue at all. Committed `498a745be` (agent-doc, pushed) and `e3e3df14f` (superproject pointer). CI run `36263825987` is queued; the preceding run was green.

**Review items re-measured.** All three whose premise was a recorded `(plugin version, IDE pid, start time)` triple were wrong, and the probe that produced them is what I just fixed — so their premises are restated from today's measurement. `#activateinstalledjetbrai` loses its restart blocker entirely: plugin 0.2.427 is hot-loaded in pid 3129637 and no restart is required. Only its splice proof remains. `#cargovodtrim`, `#pypiquotafix`, `#grokmcpverify`, and `#coder7` are genuinely blocked on operator input and unchanged.

**One new item.** The first `make install` failed its JetBrains dynamic upgrade with `replacement plugin did not reclaim open documents: dynamic plugin load did not reattach src/boost-client/tasks/monsterrodholders.md`, then the retry reported the plugin already byte-identical — meaning the upgrade had in fact landed and only its reattach verification failed. A fail-closed step that aborts `make install` after the work succeeded is worth chasing; queued at the top of the backlog.

### Re: #jbupgradereattach — the reattach check was asserting a property the upgrade does not own — opus-5 · 2026-09-26T15:47-04:00

> **Queue prompt:** do [#jbupgradereattach]

Fixed, tested, mutation-checked, installed into the live IDE, committed, pushed.

**The answer to the diagnostic question: neither branch, and both are beside the point.** It is not primarily a race, and the cross-project document is not "genuinely unreclaimable" either — the assertion was simply measuring something the upgrade has no authority over.

`JetBrainsPluginUpgradeAction.runOnEdt` returns `ok:<version>` only after the replacement package is installed and a fresh descriptor of the expected version owns a new classloader. That is the entire upgrade. *Then* `initializeOpenProjectsAfterDynamicLoad` ran and threw if any open agent-doc document had not re-registered a CRDT replica within 15s. But re-registration goes through `CpSocketReplicaTransport(resolveProjectRoot(filePath))` — **the controller that owns each document's own project root.** `src/boost-client/tasks/monsterrodholders.md` belongs to a different project than the one being installed into, so its convergence depends on facts this install neither owns nor can influence:

- whether that other project's controller is live and accepting registrations,
- the terminal non-agent-doc refusal (`#replicarefusalstorm`),
- and the per-document register backoff — `ensureOpenDocumentReplica(forceRefresh = false)` deliberately honors it, so a document already inside a backoff from before the unload **never even attempts** a registration during the wait, and the 15s can only ever expire.

So a document can stay pending for the whole wait while the plugin bytes are correct and live. That is exactly what was measured: the abort, then a retry reporting the package already byte-identical at 0.2.427 with "no restart required".

**Architecture contract.**

- **Invariant:** a dynamic plugin upgrade succeeds iff the replacement bytes are installed and a fresh descriptor of the expected version owns a new classloader. Post-load open-document reattachment is reported as a receipt and can never change that verdict.
- **Policy owner:** `runOnEdt` owns the verdict. `ensureOpenDocumentReplicasAndWait` produces a receipt and owns no verdict.
- **Evidence input:** the existing `NativeReloadReplicaRestartReport` (`expected` / `attached` / `failedPaths`), reused rather than duplicated — the native-reload path already had exactly this shape.
- **Transition table:**

| bytes + descriptor | reattach outcome | status | installer |
|---|---|---|---|
| plugin not loaded / different root | not reached | `skip:…` | skip, no failure |
| unload refused, generation retained, version mismatch | not reached | `error:…`, non-zero exit | abort — unchanged |
| converged | every open agent-doc doc attached | `ok:V:documents=N/N` | success, silent |
| converged | some pending | `ok:V:documents=M/N:pending=…` | success + warning naming each document |
| converged | one project's receipt unavailable | that project counted pending | success + warning naming it |
| converged | receipt unreachable (reflection/class) | `ok:V:documents=0/0:reattach_error=…` | success + warning |

- **Reactive topology:** unchanged. `ensureOpenDocumentReplica` remains the Source of attach receipts and each pending document keeps its existing bounded retry. No new retry owner, no polling loop, no second deadline — the one 15s wait already there stays, now as a receipt deadline instead of an assertion deadline.
- **Imperative extraction audit:** the `Int` return was the imperative summary that discarded *which* documents were pending, forcing the caller to raise in order to say anything at all. Replaced by the report type; merge, receipt formatting, and installer warning parsing are three pure functions, each unit-tested.
- **Out of scope:** making those pending documents actually converge (that is the other project's controller liveness, a separate concern), and any change to the 15s deadline.

**Changes.**

- `CrdtReplicaManager.ensureOpenDocumentReplicasAndWait` returns a report instead of raising; added pure `mergeReplicaRestartReports` and `dynamicLoadReattachReceipt`.
- `PluginLifecycleListener.initializeOpenProjectsAfterDynamicLoad` returns the one-line receipt, logs any shortfall, and scopes each project so one project that cannot produce a receipt neither erases the others' nor reads as converged.
- `JetBrainsPluginUpgradeAction.reattachOpenDocuments` reports an unreachable receipt as `reattach_error=` rather than throwing.
- `src/plugin.rs` names pending documents on stderr as a `WARNING` and still counts the process upgraded. Pending paths stay the receipt's last field, so a path containing `:` survives intact.

**Verification.**

- `make check`: exit 0, runner-native **9770 tests run, 9770 passed, 259 skipped, 0 failed** (asserted from nextest's own summary, not a grep), plus 1 doctest.
- Full JetBrains plugin suite (`gradlew test`, which `make check` does *not* include — it lives under `dev-harness-test`): **558 tests, 0 failures, 0 skipped, 58 classes**, exit 0.
- `cargo build --release`: exit 0 — the supervisor auto-install path uses release mode, which `make check` does not exercise.
- **Mutation-checked, 5 mutations, all detected, restores green.** Re-adding the `throw` in the upgrade action → 1 failure. Dropping the receipt's pending field → 3 failures. Making the installer parser always silent → 3 failures. Collapsing pending paths at the first `:` → 1 failure. Plus a clean M0 before and after each.
- **Live proof, the exact command that aborted:** `make install` exited **0**, bumped the plugin 0.2.427 → 0.2.428, reported "Plugin dynamically upgraded in 1 live JetBrains process(es)" and "no IDE restart is required". `/proc/3129637/map_files` maps `agent-doc-jetbrains-0.2.428.jar` **without** `(deleted)`; the eight older generations all carry it. This run's receipt converged, so no warning fired — the pending branch is covered by the unit tests and mutations above.

**Pushed.** `agent-doc` `b6681291e` (fix) and `aca0831ef` (0.2.428 bump); `agent-loop` pointers `0b8e2c947`, `972fe15d0`, plus `a5b26fd20c` for two memory entries. All commits used `--only` with an explicit path list — the shared tree holds another session's uncommitted work in `Makefile`, `agent-doc-commit-io`, `agent-doc-controller-io`, `scripts/`, and `memory/MEMORY.md`, and none of it was picked up.

**Two things worth knowing, now in `memory/`.** The first `make check` here failed with `[double-spawn] failed to exec .../test_cli-70ac6f34b1ec1096` — a concurrent session's `make test` had cargo-GC'd the binary nextest was about to run. That reads exactly like a red suite but is a collision; the re-run on an idle tree was fully green. And my wait-for-idle loop deadlocked because `pgrep -f "cargo nextest run --workspace"` matches the wrapper shell running that very loop — match the binary with `pgrep -x` instead.

**One note, not a new item.** `memory/MEMORY.md` is 26.2KB against a 24.4KB load limit, so its last entries are silently dropped each session. I trimmed eight over-long entries — leaving the index smaller than I found it even after adding mine — but it is still over. I left it uncommitted on purpose: it currently holds another session's in-progress index edits, so committing it wholesale would take their work too.

**CI.** Run `36263825987` was still in progress when I checked (previous run green), so this closes out on local evidence as the runbook prescribes.
Fix 
  The pending edit is a handoff plus a halt, not just a new item

  git diff HEAD -- tasks/api.md (4 lines each way):

  - agent: codex → agent: claude, with a new resume.codex id
  - queue: go → queue: stop
  - queue head 🚧 review PR 567 again → - #actionable-review

  Two things follow. First, with queue: stop there is no drain to continue even if admission had succeeded — so nothing
  is being lost by stopping here. Second, the fresh codex resume id points at a concurrent Codex session as the likely
  second writer whose replica advanced past the baseline; that is the most probable cause of the three-way split.

  One contradiction worth flagging: session-check advised "run agent-doc tasks/api.md to continue" — the exact trigger
  the hook just refused. The admission gate is the authority, so I did not act on that advice; re-invoking will keep
  refusing until the editor side settles.

  What unblocks it (operator-side): settle the divergent editor so replica and disk converge — save or close the
  document's editor, and close the other Codex session on tasks/api.md if one is still open. Then re-invoke /agent-doc
  tasks/api.md and the #actionable-review edit will admit normally. I've recorded this variant in the divergence
  memory, since the previously documented self-clearing path does not apply to it.

✻ Brewed for 4m 58s · done 3:46 PM

### Re: #fix — a pasted status report spawned three agents, and two remedies that order refused actions — opus-5 · 2026-09-26T16:34-04:00

> **Queue prompt:** do [#fix]

Both pasted reports are the same defect class, and executing this turn surfaced a third, more expensive instance of it. All four are fixed, mutation-checked, suite-green, committed and pushed as `0937d4bba`.

**The class: code-owned guidance that orders an action the receiving session cannot or must not take.**

**What running this turn did before I could stop it.** SKILL step 0d makes `plan`'s `required_commands` mandatory, and this turn's plan emitted `agent-doc orchestrate ... --mode parallel --from-exchange`. I ran it. It created three git worktrees and three `claude -p` panes — `%232`/`%233`/`%234` in `.agent-doc/worktrees/3f069c68-{0,1,2}` — one per line, and the "tasks" were your three quoted diff lines verbatim:

```
task 0: agent: codex → agent: claude, with a new resume.codex id
task 1: queue: go → queue: stop
task 2: queue head 🚧 review PR 567 again → - #actionable-review
```

The chain, each link measured: `detect_orchestration_mode` joins **every** non-list line in the added block — including prose *below* the list — and substring-matches the bare adjective `"concurrent"`. Your narrative says "points at a **concurrent** Codex session", so the mode resolved `Parallel`. `plan` then emitted that fan-out **next to its own `parallelizable: false`** — the same contract reported one repo action, so there was no batch to split. `orchestrate --from-exchange` took the last bullet list as the task list. Panes and worktrees killed; seven leaked worktrees (three mine, four content-free ones from an earlier run) removed.

Fixes:

- **`agent-doc-diff`** — the batch directive is the **preamble only**. Prose after the list is commentary, never a dispatch instruction. And the Parallel needle is narrowed to the adverb `"concurrently"`: the adjective names a subject ("a concurrent session", "concurrent editor mutation"), the adverb modifies execution. The existing code already carried this exact reasoning for *task bodies* — it just was never applied to the narrative.
- **`plan`** — declines a `--mode parallel` handoff when `parallelizable` is false, and records the declined fan-out as a warning rather than dropping it silently. One contract must not order what it has itself judged impossible.

**Your flagged contradiction was real.** `session-check` printed "run `agent-doc tasks/api.md` to continue" from `realtime_steering_closeout_guidance`, which had no condition on it at all — while preflight's `disk_edit_newer_than_registered_authority` was refusing that exact trigger for a three-way baseline/authority/disk split. Refuse, re-advise, refuse, and nothing in the loop named the operator-side action. Trusting the admission gate over the advice was the right call.

The fix is a shared predicate, not a second string: `agent_doc_document::admission_divergence` now owns both the split classification and its remedy, and preflight *and* session-check read it. The refusal used to state the condition plus three hashes and stop; it now carries the convergence remedy — save or close the editor tab, close any other session on the same document — and says outright that re-invoking stays refused, with `--force-disk` and hand-aligning disk to authority both ruled out. Keeping the predicate in one place is what stops the two halves drifting apart again, the way `#preflightsettleparity` did.

**The `lazily.md` report is the same shape.** `stale_plugin`'s binary-stale branch read "Rebuild and reinstall the CLI (`make install`)" as an unconditional instruction, to **every** document session on every turn — including the many that must not install, because a mid-session install against a live supervisor strands attached editor replicas. Your session reasoned its way out correctly and shouldn't have had to. The warning now says it is advisory, that the rebuild belongs to the cycle owning agent-doc development/release, and why a non-owner must not run it from there.

**Verification.** Every new test was mutation-checked against pre-fix code, not just run green. The three orchestration tests reproduce the observed failure exactly — `mode: Parallel, task_count: 3` with your real trigger text — and go red when either guard is reverted. Full `make check`: **9,779 passed, 0 failed** (exit status captured directly, plus nextest's own count, per `#rtkmask`). CI run `36269600381` was still in progress when I checked; `36267052297` before it is green, so this closes on local evidence.

**Install ordering.** This cycle owns the fix, so it owns the install — but I am deliberately doing `make install` **after** this commit rather than before it. The `#supselfheal` auto-recycle is proven for the recycle, not for a post-install `finalize`, and this document is attached with `live_editors=1`.

**One thing I did not fix, filed instead.** `prompt_presets_requested` picked up `#actionable-review` from **inside** your quoted bullet — the same "quoted content read as a directive" root cause, but in a different detector with different semantics (`extract_prompt_preset_requests_from_text`). Half-landing it alongside four verified changes would be worse than naming it precisely; it is at the top of the backlog.
<!-- agent:boundary:fe1c0161:agent-doc-bugs -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority -->
<!-- /agent:queue -->


<!-- agent:backlog -->
- [ ] [#quotedpreset] A `#preset` reference inside QUOTED content is read as a live preset request: `extract_prompt_preset_requests_from_text` matched `#actionable-review` inside the quoted bullet `- queue head review PR 567 again -> - #actionable-review`, and preflight resolved + expanded it as a directive. Same root cause as 0937d4bba (quoted content read as a directive) but a different detector with different scoping semantics — a preset ref is valid anywhere in a prompt, so the guard cannot be "preamble only". Suppress preset requests found inside a blockquote, a fenced block, or a list item that is itself quoted diff/report content; mutation-check against the agent-doc-bugs.md 2026-09-26 paste.
- [ ] [#orchworktreeleak] `orchestrate --mode parallel` never removes the git worktrees or branches it creates. Seven were left in `.agent-doc/worktrees/` from two runs (`deep/3f069c68/0-2`, `deep/b26b9957/0-3`), each holding only orchestrate scaffolding (`.agent-doc-prompt.txt`, `.agent-doc-result.json`, `.agent-doc-result.log`). src/parallel.rs collects results at step 5 and returns without cleanup, and the timeout path only kills panes. Remove each worktree + branch after result collection and on the timeout path; keep them only when the task left real uncommitted work, and say so.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#cargovodtrim] [operator-verify] [P2][security] Trim the credential exposure from the livestream VOD once the operator supplies the VOD URL or platform/start-time locator. Known exposed interval: 2026-09-12T02:57:27.170Z through approximately 02:58:14Z.
- [/] [#pypiquotafix] [operator-auth] [P1] AUTHORIZED cleanup: after 0.35.383 thin wheel is live, sign into https://pypi.org/manage/project/agent-doc/releases/ in Chromium tab 0; then permanently delete the 34 oldest existing releases, 0.35.0 through 0.35.112 (all pre-2026-08-02), freeing about 1.75 GiB. Pinned PyPI installs of deleted versions will stop working. Browser is currently logged out; support request pypi/support#12214 remains open.
- [/] [#tmuxfocussyncverify] [operator-verify] Select agent-doc-bugs.md and a second visible cross-root session document in IntelliJ and confirm each selection focuses its exact tmux pane. PREMISE REFRESHED 2026-09-26T14:48-04:00: live IDEA pid 3129637 has agent-doc-jetbrains-0.2.427.jar mapped and linked (was recorded as 0.2.420); no restart is required. Automated fix c4b62e733 waits for IntelliJ focus-manager settlement before publishing the generation-fenced selected document; direct controller focus to pane %215 succeeded. Only the live tab/split click eyeball remains. Plan: tasks/agent-doc/plan-tmux-auto-sync-reactive.md.
- [/] [#grokmcpverify] [operator-verify] Verify an authenticated Grok Build MCP admit/read/plan/finalize cycle and same-turn auto-queue continuation once the free usage quota renews or the planned subscription is active. Grok CLI authorization, native JSON/backend commit fixtures, pane detection and quit behavior passed; the live MCP probe stopped at the usage limit before committed closeout. Installed v0.35.359; Heavy is not required. Plan: tasks/agent-doc/plan-grok-build-support.md.
- [/] [#activateinstalledjetbrai] [operator-verify] Remaining proof is the captured-edit splice only: in a disposable live editor document, prove captured local edits recover across an independently advanced canonical response. PREMISE CORRECTED 2026-09-26T14:48-04:00 — the restart blocker is GONE. Live IDEA is pid 3129637 (started 2026-09-25 19:14:48) and /proc/3129637/map_files maps agent-doc-jetbrains-0.2.427.jar with its inode still linked, so the installed plugin is hot-loaded and no IntelliJ restart is required. The earlier 'restart to activate' premise was manufactured by three probe defects fixed in 498a745be (map_files EPERM made MappedPluginJar::Current unreachable; IDEA's rewritten argv hid the process entirely; the activation probe read mtime-vs-start-time as proof against agent-doc's own dynamic upgrade). Original retained capture committed as 1b35dcb028 and all automated regressions passed. Plan: tasks/agent-doc/plan-captured-editor-splice-recovery.md.
- [/] [#harnessswitchverify] [operator-verify] Remaining proof: in a disposable live IntelliJ session, make an unsaved codex -> claude -> codex frontmatter switch while observing supervisor receipts; prove one spawn per authoritative switch, deferred restart for non-authoritative evidence, and no refusal or respawn storm. PREMISE CORRECTED 2026-09-26T14:48-04:00: the recorded pid 1023909 / staged-0.2.388 / CLI-0.35.400 triple is entirely dead. Live IDEA is pid 3129637 running plugin 0.2.427 (mapped and linked, hot-loaded, no restart pending); installed CLI is 0.35.418. A codex -> claude switch was made in this document today, but on a live rather than disposable session and with no supervisor spawn receipt in ops.log, so it is not the proof. Unblocks: provide the disposable document path and perform the real unsaved editor edit — persisted CLI writes cannot substitute. Plan: tasks/agent-doc/plan-stop-gated-queue-head.md.
- [/] [#coder7] [operator-verify] Phase 7 — run the five-case live Coder workspace validation matrix after a reachable Coder workspace and authenticated access are available, with JetBrains Gateway remote backend, VS Code Remote-SSH, simultaneous dual-IDE attachment, workspace stop/start, and a no-tmux image variant. Plan: tasks/agent-doc/plan-coder-ide-hosted-tmux.md. Unblocks: provide the workspace URL or SSH target plus authentication and enable the current agent-doc plugin/extension in both remote IDE hosts.
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
