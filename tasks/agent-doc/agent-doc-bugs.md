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
<!-- agent:boundary:fe1c0161:agent-doc-bugs -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- ⏭️ #advance-review
<!-- /agent:queue -->


<!-- agent:backlog -->
- [ ] [#jbupgradereattach] make install aborts on a JetBrains dynamic upgrade whose reattach verification fails even though the upgrade landed. Observed 2026-09-26: first install exited 2 with 'replacement plugin did not reclaim open documents: dynamic plugin load did not reattach src/boost-client/tasks/monsterrodholders.md', and the immediate retry reported the plugin already byte-identical at 0.2.427 and 'no restart required' — so the replacement bytes were in place and only the open-document reattach assertion failed. Determine whether the reattach check races the document reopen or whether a cross-project document genuinely cannot be reclaimed, and stop aborting the whole install when the plugin bytes converged.
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
