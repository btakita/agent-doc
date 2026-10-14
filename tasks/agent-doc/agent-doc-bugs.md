---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: ba846c4c-2e01-4e12-9539-096faec207ee
  codex: 01a1359c-17a9-76c3-8aac-bfc30b074aea
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
presets:
  '#actionable-review': Add actionable review items into backlog + queue
  '#agent-doc-causes': Fix agent-doc causes
  '#do-backlog': /goal do backlog items
  '#gh-fix': fix then close
  '#release': release + publish
  '#self': agent-doc-bugs.md
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
---

## Status

<!-- agent:status patch=replace -->
Review pass done. frontendlivedispatch closed on live evidence. Two review items remain, both genuinely gated: an operator decision (jb262dynlauncherrecommen) and an external PyPI reply (pypiquotarecheck).
<!-- /agent:status -->
 

## Exchange

<!-- agent:exchange patch=append subagents -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261012-223708.md`*

Compacted content:
- Archived 59 response topic(s): dispatched GH #240 to a subagent; Clear Session Context on sdk.md lost both presses; Clear Session Context on api.md wedged; 56 more
- Prior summary/context: prior compacted content: Archived 9 response topic(s): what #drivecapturedsplice still needs; release status; v0.35.4...

### Re: #actionable-review — opus-5.5 · 2026-10-12T18:38-04:00

I checked the three gated review items to see which ones I can act on. I did this inline instead of sending it to a subagent, because the work only touches this session document, and dispatched subagents aren't allowed to edit it.

- **frontendlivedispatch: closed as done.** haiven-dev frontend.md is dispatching and closing out normally. It had at least five committed agent-doc(frontend) cycles today (the latest at 17:54). Pane %166 (codex) is idle at a fresh prompt after reporting "closeout committed successfully". There were no recycle_ttl_elapsed_supervisor_alive refusals in ops.log. The specific pass lines (route_dispatch_only_recycle_inflight_ready_after_start, or reason=dispatch_ready_after_start) never fired, because the in-flight recycle wedge they were meant to clear did not come back. That recovery path is unused, not unproven. The semantic-completion match against operatorverifylive was a false hint: that entry is the earlier, pre-precondition version of this same item.
- **jb262dynlauncherrecommen: stays gated.** This is purely an operator decision: keep accurate-warning as the policy, or authorize a restart-free 262 split-mode launcher. Nothing here is agent-executable until you choose.
- **pypiquotarecheck: stays gated.** I rechecked pypi/support#12214: still OPEN, one comment, last updated 2026-09-10. The quota headroom verdict from today's check still holds. It is unblocked only by a human reply on PyPI's side.

No new actionable backlog or queue items came out of this. The two remaining review items are genuinely gated. Side note: frontend.md had a short burst of idle_queue_dispatch_deferred reason=defer_unrendered_frame around 22:30Z. It cleared on its own (3 events), so I didn't file anything.

❯ Run Agent Doc after a Restart Agent during a turn:
```
› agent-doc /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/infra.md


• The agent-doc turn was not admitted because a prior document write is still reconciling. I’ll run only the permitted
  session check to see whether an operator prompt is waiting; I will not modify the document or start a new response.

• Failed (exit 1) agent-doc session-check /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/infra.md
  └ [perf] session_check.operations file=/home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/infra.md total_ms=26…
    [session-check] INTERRUPTED: binary-owned response delivery `1791849117705171624-1-738fdcec2752715c0b4c2a700049ae2…
    + 2 lines (^t to expand)

• Agent-doc could not start because a retained response is awaiting editor projection. The registered editor endpoint
  rejected the delivery receipt, so waiting alone will not resolve it.

  Restart or reload the editor that has tasks/infra.md open. The retained response is durable—do not resend it or
  force-write the file. Once the editor reconnects, re-trigger this request so agent-doc session-check can confirm
  recovery.

  No pending operator prompt was listed.

  Worked for 14s • 8:08 PM
```
---

### Re: GitHub issue #245 — gpt-5 · 2026-10-12T19:48-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/245

Fixed, integrated, installed, pushed, and closed.

- `agent-doc-ops-log-io` now assembles each `ops.log` and `cycles.jsonl` record with its terminating newline and submits the complete buffer through one `write_all` call, preventing concurrent appenders from landing between the record body and newline.
- Added deterministic writer-level regression coverage and updated `specs/08b-single-process-control-plane.md` with the append atomicity contract.
- Integrated on `main` as `b5f41ff4db` and pushed to GitHub.
- Authoritative `make check` passed from a clean build: 10,423 tests passed, 167 skipped; both JetBrains builds and the cross-editor checks passed. `make install` also completed successfully.
- GitHub issue #245 is closed. CI run 38324006988 for `b5f41ff4db` is in progress; local full-suite verification is the closeout proof.

A separate `release + publish` queue item arrived mid-turn and was claimed and dispatched to `subagent:release_publish`; it remains in flight for its own release-readiness handoff.

> **Chat prompt (#chatprompt):** Can we unwedge infra.md or do you still need diagnostics?

> **Chat prompt (#chatprompt):** unwedge infra.md

### Re: Restart Agent retained projection and infra.md recovery — gpt-5 · 2026-10-12T20:45-04:00

> **Queue prompt:**
>
> do [#runrestartduring]


Fixed, installed, and unwedge completed. No more diagnostics are needed for this failure.

The root cause was a receipt-classification bug in the JetBrains native-save path. After Restart Agent, normal projection drift (`visible_length_mismatch`, hash mismatch, replica mismatch, or post-save CAS drift) returned the same callback as a genuine editor-endpoint refusal. Rust therefore recorded `DeliveryRejected`, degraded/unregistered the healthy endpoint, and told the operator to restart the editor even though the CRDT/editor path was live.

The fix introduces a distinct callback code 4 and terminal receipt `{"status":"deferred","reason":"projection_changed"}`. Rust maps that to retryable `ProjectionUnconverged`, preserves endpoint ownership, and retries from current authority. Current main's existing callback code 3 remains the reasonless generic deferral for bounded work still running, so the two cases are not conflated.

Implementation and proof:

- Integrated and pushed to `agent-doc` main as `9efcd2579ddc673feb95092bee53a9248330b4be`; the superproject pointer is committed as `ee0054a29b`.
- Updated the realtime spec, IPC protocol/IO, FFI mapping, JetBrains manager/watcher, and Rust/Kotlin regression tests.
- JetBrains package generations advanced to classic `0.2.563` and 262 `0.2.564`.
- Authoritative `make check` passed: 10,426 tests passed, 167 skipped; VS Code, both JetBrains builds, cross-editor simulation, Python bootstrap, docs/audits, and TLA gates passed.
- `make install` completed. The running IntelliJ plugin dynamically upgraded; installed binary/native library and VS Code extension were refreshed.
- GitHub CI run 38328716259 is in progress; local full-suite verification is the closeout proof.

For `infra.md`, read-only diagnostics confirmed the later report was the same durable retained intent and logged the exact `visible_length_mismatch` covered by this fix. After installation, the stale response was confirmed already committed in HEAD. The pane then remained in a stale ready/busy auto-queue state, so the guarded clear was queued and the requested `session interrupt-clear` cleared that empty/stuck turn without resending or force-writing the retained response. A fresh cycle is now running on the four pending infra directives.

The standalone `---` added to the exchange is only a Markdown separator and required no repository or session action.

> **Queue prompt:** Run Agent Doc on infra.md issue
> ```
> › agent-doc /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/infra.md
>
> • Agent-doc admission is still blocked by a retained document write. I’ll run only the permitted session check and
>   report any queued operator prompt or recovery instruction without modifying the document.
>
> • Failed (exit 1) agent-doc session-check /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/infra.md
>   └ [perf] run_context.read label=head_content elapsed_ms=36
>     [perf] session_check.operations file=/home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/infra.md total_ms=35…
>     [session-check] INTERRUPTED: binary-owned response delivery `1791849117705171624-1-738fdcec2752715c0b4c2a700049ae2…
>     + 2 lines (^t to expand)
>
> • Agent-doc remains blocked because the editor endpoint is rejecting the retained write. Restart or reload the editor
>   containing tasks/infra.md, then re-trigger this request.
>
>   The response remains durable—do not resend or force-write it. No pending operator prompt was listed.
>
>   Worked for 13s • 8:24 PM
> ```

This was the same `visible_length_mismatch` incident, not a second defect. The installed typed-deferral fix covers it, and `infra.md` has resumed on a fresh cycle after the guarded interrupt-clear.
> **Chat prompt (#chatprompt):** did you release + publish?

### Re: release status — gpt-5 · 2026-10-12T20:54:44-04:00
No. I pushed fix commit `9efcd2579ddc673feb95092bee53a9248330b4be` to `main` and installed it locally, but I did not cut a new version/tag or publish release artifacts containing it. The latest published release is `v0.35.495` (published 2026-10-12T21:19:58Z), whose release commit predates the fix; no release tag contains the fix commit. The `release + publish` queue item therefore remains pending, and its existing worker claim belongs to an ended session.
> **Chat prompt (#chatprompt):** released?

> **Chat prompt (#chatprompt):** published?

### Re: release and publication status — gpt-5 · 2026-10-12T22:03:10-04:00
No. The stale-projection fix is present on `origin/main` at `9efcd2579ddc673feb95092bee53a9248330b4be`, but there is no newer tag and no published release containing it. GitHub still reports `v0.35.495` as the latest published release, and that tag predates the fix. The previous release-worker claim has ended; `release + publish` remains queued and incomplete.
> **Queue prompt:** do [#runsdkissue]

> **Queue prompt:** release + publish

### Re: sdk.md recovery and v0.35.496 release — gpt-5 · 2026-10-12T22:35:07-04:00

`sdk.md` recovered without repair. `agent-doc session-check` confirmed cycle `cycle-1791856800306` committed successfully, the retained response is visible, the document queue is drained, and session commit `2524f90f409185bb5fc6d22921496f6f995414b4` is clean.

Released `v0.35.496` from commit `28a28c26f4774b31f076a1e3b8822e017f116d9c` and pushed both `main` and the tag. The final version-projected gates passed: 10,426 Rust tests, 167 skipped; 196 VS Code tests; classic and exact-262 JetBrains builds/tests; cross-editor simulations; Python/bootstrap; docs/audits; TLA; and the separate tmux integration suite. `make install-full` succeeded and installed binary/native library `0.35.496`, JetBrains `0.2.565` / exact-262 `0.2.566`, and the VS Code extension.

Publication is dispatched but not yet externally complete. Release workflow run `38336856391` is in progress: https://github.com/btakita/agent-doc/actions/runs/38336856391. The GitHub release and PyPI `0.35.496` endpoint were not present at the one required post-push observation. PyPI is intentionally dispatched by the Release workflow only after all release assets and checksums exist; no further local action is pending.
> **Chat prompt (#chatprompt):** published?

### Re: v0.35.496 publication status — gpt-5 · 2026-10-12T22:50:20-04:00
No, not yet. Tag `v0.35.496` is pushed, but GitHub Release workflow run `38336856391` remains in progress: https://github.com/btakita/agent-doc/actions/runs/38336856391. GitHub does not yet expose a `v0.35.496` release, PyPI still returns 404 for `agent-doc==0.35.496`, and the downstream PyPI workflow has therefore not started.
> **Chat prompt (#chatprompt):** published yet?

### Re: v0.35.496 publication complete — gpt-5 · 2026-10-12T23:04:54-04:00
Yes. GitHub Release `v0.35.496` was published at 2026-10-13T03:01:52Z with all six platform archives, `SHA256SUMS`, both JetBrains packages and update feeds, the VS Code package, and editor-package checksums: https://github.com/btakita/agent-doc/releases/tag/v0.35.496. Release workflow run `38336856391` succeeded. PyPI workflow run `38339749606` also succeeded, and PyPI now serves `agent_doc-0.35.496-py3-none-any.whl` (uploaded 2026-10-13T03:02:42Z).
> **Chat prompt (#chatprompt):** Have you released the plugins?

> **Chat prompt (#chatprompt):** and published the plugins?

### Re: plugin release and publication channels — gpt-5 · 2026-10-12T23:48:13-04:00
Yes through the project release channel, but not through the official IDE marketplaces. GitHub Release `v0.35.496` publishes the VS Code `0.2.81` VSIX, JetBrains classic `0.2.565` ZIP, exact-262 `0.2.566` ZIP, both JetBrains update-feed XML files, and `EDITOR-PACKAGES.sha256`: https://github.com/btakita/agent-doc/releases/tag/v0.35.496. These are the artifacts used by `agent-doc plugin install` and the project update feeds.

The current release workflow has no marketplace-publish job. `vsce show btakita.agent-doc` reports the extension is not found in the official VS Code Marketplace, the JetBrains Marketplace lookup for `com.github.btakita.agent-doc` returns not found, and Open VSX still exposes the older `0.2.1`. Therefore the latest plugin packages are released and downloadable, but they have not been published to the official JetBrains/VS Code marketplaces.
> **Chat prompt (#chatprompt):** Idea is non-responsive

### Re: IntelliJ non-responsive diagnosis — gpt-5 · 2026-10-13T00:29-04:00

IntelliJ is alive but caught in a sustained JavaScript/Node workspace-indexing storm. The live process (PID `3511101`) is using roughly 180–205% CPU and 14.4 GiB resident memory. Fresh JetBrains performance dumps show repeated 10–19 second UI freezes on the AWT event-dispatch thread in `NodeModulesEntityUtil` → workspace-model `VirtualFileIndex` → `Object2ObjectOpenCustomHashMap.find` while IDEA associates hundreds of `node_modules` roots under the generated worktree forest. The log reached about 950,000 included files and more than 500 JavaScript libraries and was still growing.

The dumps do not place agent-doc on the blocked UI path. Its authority-stream threads are waiting in socket reads, while the UI thread is busy inside IntelliJ's JavaScript workspace-model update. The recent plugin upgrade may have coincided with the rescan, but the captured freeze itself is an IntelliJ indexing/write-action problem rather than agent-doc response delivery.

A restart is the practical immediate recovery. I have not killed or restarted IDEA because that could discard unsaved editor state. After restart, exclude the large generated `worktrees/haiven-dev/haiven-frontend/rebase-open-prs-*` tree (or the broader generated worktree directory) from this IDEA project's content roots so the Node plugin does not rebuild libraries for every checkout. Diagnostics are preserved under `~/.cache/JetBrains/IntelliJIdea2026.1/log/threadDumps-freeze-20261013-0027*`.
> **Chat prompt (#chatprompt):** Should I restart Idea now?

> **Chat prompt (#chatprompt):** subagent: • Blocked by hook
>   └ agent-doc Stop hook intercepted an unfinished document cycle for /home/brian/work/btakita/agent-loop/src/haiven-
> dev/tasks/sdk.md. [session-check] INTERRUPTED: free-text agent:queue head(s) "#rebase-conflicts: all my open PRs." were
> seen at preflight but have no committed response/echo or explicit deferral proof in the closeout — the prompt may have
> been silently lost
>     [session-check] hint: either respond to the unresolved head(s) and run `agent-doc finalize /home/brian/work/
> btakita/agent-loop/src/haiven-dev/tasks/sdk.md`, or add `<!-- no-free-text-queue-head-guard -->` if the removal was
> intentional (see #lr-queue-patchback-miss) The latest assistant text was captured into the pending/capture ledger
> before auto-close. The hook could not finish the required commit boundary: closeout operation is already in progress
> for cycle cycle-1791866878706 by captured-finalize-resume-1069718-1791859483080680892-t8 pid=1069718
> role=captured_finalize_resume until=1791867179 [closeout_owner_retry_at=1791867179]. The retained capture or projection
> is already durable and this exact retained intent commits itself once delivery converges — this is a deferral, not a
> lost response. Wait for its controller-owned terminal state edge; do not use the document's current-cycle status as
> proof because queue continuation can advance to a different cycle. Do NOT re-send the response, force disk, or `admin
> recycle`, which disturb the capture being awaited. Do not reach for `admin reload-lib` on your own either: `admin
> reload-lib` is the sanctioned recovery only when `agent-doc session-check` reports that the editor holding the document
> is not serving its replica, and then its remedy governs. Do not send the final answer yet. If the response is missing
> from the document, run `agent-doc repair /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk.md` first. Then
> finish the commit boundary for this turn and end with `agent-doc session-check /home/brian/work/btakita/agent-loop/src/
> haiven-dev/tasks/sdk.md`.
>
> • The stop hook caught a commit-boundary race after the queue was consumed. I’ll wait for the retained finalize
>   controller to reach its terminal state, verify the response is present, repair only if it is missing, then run the
>   required session check before ending.

> **Chat prompt (#chatprompt):** fix the agent-doc causes of that error in a subagent

> **Chat prompt (#chatprompt):** Can I restart Idea now?

### Re: issues #246 and #247, Stop closeout handoff, and v0.35.497 — gpt-5 · 2026-10-13T02:14-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/246

Fixed and closed. A Claude `History n/m` prompt-history recall is now recognized as harness-owned residue and cleared before route injection, while genuinely typed drafts remain protected. The worker’s focused regressions and `make check` passed; its commit was integrated as `8e4cfc65f1`.

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/247

Fixed and closed. Releases now build and select three disjoint JetBrains packages, including a `263..263.*` package, so the plugin-side fixes from #241 and #242 can reach IntelliJ IDEA 2026.3. Classic, exact-262, exact-263, feed-generation, installer-selection, focused Rust/Gradle, `make check-fast`, and `make check` verification passed. The fix was integrated as `436fbd7612`.

The pasted `sdk.md` Stop-hook error was also fixed in the requested isolated subagent. The queue provenance guard and consume ordering were correct; the Codex Stop adapter failed to recognize the standard `closeout_owner_retry_at=` durable-owner receipt and therefore appended stale manual-repair guidance during a legitimate captured-finalize handoff. The adapter now defers to the proven retained owner without weakening the fail-closed queue guard. Focused tests, `make check-fast`, and `make check` passed; the integrated commit is `d5476b7e89`.

> **Queue prompt:** release + publish

Release `v0.35.497` was committed as `8010520d4c`, fast-forwarded to `origin/main`, tagged, and handed off for publication. The corrected authoritative `make check` passed with exit 0 and zero failure signatures. `make release` then completed with `tag_publish_handoff=complete` and `local_install_full=complete`; the installed CLI reports `agent-doc 0.35.497`, and the native library plus editor artifacts were installed once. At the required single observation, the GitHub Release workflow and main CI were still in progress; Book and Pages were green. No async wait is required by the closeout contract.

For IDEA: once this response commits and `session-check` passes, restarting is safe. The currently running local IDE is IntelliJ IDEA 2026.1, and plugin `0.2.569` was dynamically upgraded there, so it does not require a restart. The new build-263 package is `0.2.571`; no live/local 2026.3 installation was present during the installer pass, so a future 2026.3 Remote Dev instance must install/update that package and then restart once to load it. Restarting a 2026.3 instance before installing `0.2.571` would only reload the old plugin.

The unrelated live `sdk.md` retained closeout was not touched or replayed.

Session-accretion warning: the exchange is large, but the active queue was drained without interrupting it for compaction.
> **Chat prompt (#chatprompt):** have you published the release?

### Re: v0.35.497 publication status — gpt-5 · 2026-10-13T02:23-04:00

Not fully yet.

> **Queue prompt:** release + publish

**Deferred:** `origin/main` and tag `v0.35.497` are pushed at `8010520d4cdd0632f3e9c0db47e64659864e6564`, and `make release` completed its tag/publish handoff and local install. However, the [GitHub Release workflow](https://github.com/btakita/agent-doc/actions/runs/38354378905) is still in progress, `gh release view v0.35.497` currently reports `release not found`, and no v0.35.497 PyPI workflow is visible yet. Book and Pages have succeeded; main CI is still in progress.

I did not dispatch duplicate workers for the replayed steering: issue #247 is already fixed and closed, and the release command already ran exactly once.
Blocked by hook error in sdk.md:
```
• Blocked by hook
  └ agent-doc Stop hook retained the exact closeout for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk.md
under its durable keyed retry. The latest assistant text was captured into the pending/capture ledger before auto-
close. The hook could not finish the required commit boundary: closeout operation is already in progress for cycle
cycle-1791902466852 by captured-finalize-resume-1069718-1791902470491047078-t1 pid=1069718
role=captured_finalize_resume until=1791902771 [closeout_owner_retry_at=1791902771]. The retained capture or projection
is already durable and this exact retained intent commits itself once delivery converges — this is a deferral, not a
lost response. Wait for its controller-owned terminal state edge; do not use the document's current-cycle status as
proof because queue continuation can advance to a different cycle. Do NOT re-send the response, force disk, or `admin
recycle`, which disturb the capture being awaited. Do not reach for `admin reload-lib` on your own either: `admin
reload-lib` is the sanctioned recovery only when `agent-doc session-check` reports that the editor holding the document
is not serving its replica, and then its remedy governs. The controller owns delivery, the editor-native save receipt,
and the terminal commit. Do not reopen or recapture the cycle, rerun finalize/write/repair, force disk, recycle the
supervisor, or resend the response. Wait for the existing controller state edge unless it explicitly reports
`needs_operator`; do not send the final answer yet.
```

### Re: Idle CPU thrash — gpt-5 · 2026-10-13T10:43-04:00

> **Queue prompt:** agent-doc was thrashing the CPU while idle for several hours.

Fixed, verified, installed, and pushed to `main`.

The active regression was in JetBrains `TmuxPaneFocusSync`: it polled controller focus state every 500 ms whenever Markdown was open, including background and inactive project states. One idle IDE generated roughly 8–11 fresh controller RPC connections per second and sustained 31–38% controller CPU. Polling now requires all three demand signals: an open Markdown document, an active project surface, and a selected active embedded agent-doc terminal. Inactive states park the poller and clear its prior observation so activation resumes immediately and correctly.

The change adds a pure demand predicate, terminal-host focus evidence, four-case Kotlin regression coverage, and the architecture contract in `specs/07-session-tmux-commands.md`. It landed as `f540206695`, with package-generation commit `b788d3471e`; `origin/main` now points to `b788d3471e530cb111853b22489321dabfffc361`.

Verification completed successfully: focused Kotlin tests, full JetBrains tests/build, classic/262/263 package checks, the authoritative `make check`, and the coordinator-owned `make install`. Installed packages are IntelliJ IDEA `0.2.572` and Rider 262 `0.2.574`; the installed CLI remains `0.35.497`. The latest pre-push main CI run was green.

An obsolete deleted-image `.460` supervisor observed during diagnosis can retain its older polling behavior until that old process exits, but current source and the newly installed editor packages contain the fix.

### Re: `sdk.md` retained closeout — gpt-5 · 2026-10-13T10:43-04:00

The retained closeout completed through its existing controller-owned retry. A read-only `session-check` exited 0 and reported cycle `cycle-1791902466852` as `committed (commit_success)`, with `queue_continuation_required=false`, `ui_outcome=no_drainable_work`, and `next_action=no_agent_action`. No response was resent and no repair, force-write, recycle, or reload was attempted. No operator action is needed.

The exchange is large (over the session-accretion warning threshold); because the queue is active and auto-compaction is not enabled, this cycle continued without compacting.

### Re: GitHub issue #248 — gpt-5 · 2026-10-13T12:29-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/248

Fixed, pushed, integrated, and closed. JetBrains 263 restart upgrades supplied a ZIP rooted at `agent-doc-jetbrains-263/`, while the Java restart stager accepts the canonical `agent-doc-jetbrains/` root. The CLI now rewrites variant entries into a temporary canonical-root archive while preserving ZIP metadata, and the UI no longer reports staging success before launcher acceptance.

The fix landed on `main` as `82dbcf626a` (worker commit `887601495a`). Its targeted regression, `cargo build`, and full `make check` all passed; issue #248 is closed with evidence.

### Re: Remaining idle CPU thrash — gpt-5 · 2026-10-13T12:29-04:00

> **Queue prompt:** agent-doc is still thrashing while idle.

The remaining current-generation CPU source was SQLite convergence startup, not the already-fixed JetBrains focus poller. Every state-database open searched for `document_version = 0`, but the existing composite index could not seek that predicate and scanned live ledgers containing 88,439 and 161,383 events despite zero matching rows. A sparse partial index now makes the guard an indexed `SEARCH`; deterministic EXPLAIN coverage rejects a regression to `SCAN`.

The fix landed on `main` as `6d13b2f776` (worker commit `ba0b321672`). The pre-fix test failed as expected, then the focused test, all 108 SQLite tests, and full `make check` passed. Obsolete deleted-image processes may retain old behavior until they exit, but current installed code contains both idle-thrash fixes.

### Re: Release and publish — gpt-5 · 2026-10-13T12:29-04:00

> **Queue prompt:** release + publish

Version `0.35.498` was projected, committed as `b9c80b963c`, tagged, and pushed to `main`. The canonical release driver reported `tag_publish_handoff=complete` and `local_install_full=complete`. Local `make check` and `make tmux-ci` passed on the exact projected bytes; the installed CLI is `0.35.498`, with IntelliJ `0.2.575`, Rider 262 `0.2.576`, and VS Code `0.2.81`.

GitHub's asynchronous Release workflow remains active: plugins and five platform artifact jobs are green, while Intel Darwin is still building; main CI is also in progress. The GitHub Release object and PyPI upload therefore are not terminal yet. Homebrew automation cannot run because `HOMEBREW_TAP_TOKEN` is absent, although the current GitHub account has admin access to the tap. Follow-up `#publish498verify` records terminal publication verification and the manual tap update once `SHA256SUMS` is available.

The exchange remains above the session-accretion warning threshold; the active queue was drained without compacting because auto-compaction is not enabled.
❯ #subagent lazily.md issue:
```
• Blocked by hook
  └ agent-doc Stop hook retained the exact closeout for /home/brian/work/btakita/agent-loop/tasks/software/lazily.md
under its durable keyed retry. The latest assistant text was captured into the pending/capture ledger before auto-close.
The hook could not finish the required commit boundary: closeout operation is already in progress for cycle cycle-
1791914660270 by captured-finalize-resume-53582-1791914732564333154-t1 pid=53582 role=captured_finalize_resume
until=1791915032 [closeout_owner_retry_at=1791915032]. The retained capture or projection is already durable and this
exact retained intent commits itself once delivery converges — this is a deferral, not a lost response. Wait for its
controller-owned terminal state edge; do not use the document's current-cycle status as proof because queue continuation
can advance to a different cycle. Do NOT re-send the response, force disk, or `admin recycle`, which disturb the capture
being awaited. Do not reach for `admin reload-lib` on your own either: `admin reload-lib` is the sanctioned recovery
only when `agent-doc session-check` reports that the editor holding the document is not serving its replica, and then
its remedy governs. The controller owns delivery, the editor-native save receipt, and the terminal commit. Do not reopen
or recapture the cycle, rerun finalize/write/repair, force disk, recycle the supervisor, or resend the response. Wait
for the existing controller state edge unless it explicitly reports `needs_operator`; do not send the final answer yet.
```

### Re: lazily.md retained closeout — gpt-5 · 2026-10-13T14:08:27-04:00

The retained closeout for `/home/brian/work/btakita/agent-loop/tasks/software/lazily.md` converged successfully. Cycle `cycle-1791914660270` is terminal as `committed (commit_success)`; commit `01e55ca8bac15409d36f3cffe392e5cfcda60e71` contains the recovered response and queue additions.

Verification:
- `agent-doc session-check /home/brian/work/btakita/agent-loop/tasks/software/lazily.md` exited 0 and reported `queue_continuation_required=false`, `ui_outcome=no_drainable_work`, and `next_action=no_agent_action`.
- Terminal proof recorded `did_commit=true`, `agreement=file_snapshot_head`, and `response_materialized_in_head=true`.
- The captured-finalize owner released with `foreground_closeout_finished`.
- No `needs_operator` state or sanctioned `admin reload-lib` remedy was reported.

Changed paths: none in this verification cycle. No response was resent and no finalize/write/repair, force-disk, recycle, or reload operation was run. Confidence is high; escalation is not required.

Concurrent queue work: GitHub issue #249 was claimed and dispatched to a fresh isolated subagent. The previously claimed idle-thrash investigation remains active and is tracing full-status polling and bounded diagnostic counts.

### Re: GitHub #249 — gpt-5 · 2026-10-13T16:50:23-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/249

Preset expansions honored:

- `#gh-fix`: “fix then close”
- `#spec-test-build-install-commit-push`: “update spec + tests. build + install for local testing. commit + push”

Completed. GitHub issue #249 is closed as `COMPLETED`, and the JetBrains 263 modular-source fix is integrated on `origin/main` as `9489effed33a3d7f5f293e3435d3f837b0f93e89`. The original isolated worker commit is `250d5ae05772d0acb903061a2e1bef5022ee7e7a`.

Changed paths: `.github/workflows/release.yml`, `Makefile`, `docs/reference/jetbrains-distribution.md`, the JetBrains 262 modular build metadata/resources, `editors/jetbrains-263/gradle.properties`, and `scripts/check_plugin_versions.py` (11 files total).

Verification:

- Focused plugin-version self-test passed.
- `make jetbrains-263-check` and `make jetbrains-262-check` passed.
- `git diff --check` and Python byte-compilation passed.
- Coordinator `make check` passed at the current integration head: 10,438 Rust tests passed, 196 VS Code tests passed, both JetBrains modular builds succeeded, bootstrap tests passed, and the TLA receipt remained valid.
- Fresh recheck confirmed the issue is closed, the evidence comment is intact, the integration commit is reachable from `origin/main`, and the worker worktree is clean.

Confidence: high. Escalation: none.

The exchange is above the session-accretion warning threshold; queue auto-clear remains active, so compaction is deferred unless explicitly opted in.

### Re: release + publish — gpt-5 · 2026-10-13T16:51:10-04:00

> **Chat prompt (#chatprompt):** subagent fix: • Blocked by hook
>   └ Queue continuation, not an error: Codex labels it `Blocked by hook`. Next `agent:queue auto` prompt for /home/brian/
> work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md: "release + publish". Continue THIS turn in-pane via the
> configured `agent-doc` MCP server (`agent_doc_admit` → `agent_doc_plan` → `agent_doc_finalize` →
> `agent_doc_session_check`), or `agent-doc finalize /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-
> bugs.md` if the MCP tools are unavailable. Do NOT run `agent-doc /home/brian/work/btakita/agent-loop/tasks/agent-doc/
> agent-doc-bugs.md` (recursive self-invocation) and do not send the final answer yet.

> **Queue prompt:** release + publish

Preset expansion honored:

- `#spec-test-build-install-commit-push`: “update spec + tests. build + install for local testing. commit + push”

Completed and freshly re-verified; no duplicate release or publication was triggered.

Release `v0.35.498` remains public at commit `b9c80b963c19c98786f0d8219fca91b647e58e32`. It contains all 15 expected assets: five native archives, the VS Code package, three JetBrains ZIPs, three JetBrains update feeds, `SHA256SUMS`, and `EDITOR-PACKAGES.sha256`.

Publication verification:

- Main CI run `38426442068`: completed successfully; the check job passed.
- Release run `38426442396`: completed successfully; all 10 jobs passed.
- PyPI run `38431402471`: build, publish, and clean-install verification all passed.
- PyPI `agent-doc==0.35.498` remains live with `agent_doc-0.35.498-py3-none-any.whl`.
- The release tag still resolves to the expected commit.

Changed paths: none in this verification cycle; the release changes are already committed at `b9c80b963c19c98786f0d8219fca91b647e58e32`. Commands/evidence included fresh GitHub release, workflow/job, asset-manifest, and PyPI package checks. The detached verification checkout is clean.

The subagent-facing “Blocked by hook” behavior is captured as queue item `[from:coordinator key=subagent-blocked-hook-20261013] Fix subagent reporting queue continuation as Blocked by hook.` for isolated implementation after this release closeout.

Confidence: high. Escalation: none.

> **Subagent completed:** `subagent:subagent_completion_feedback` returned work for queue item “In sdk.md, I have a long list of subagents. When a subagent completes, there's no feedback. agent-doc should immediately write to the `agent:exchange` when the subagent completes.”. Coordinator integration and closeout are pending.
<!-- agent-doc-subagent-completion key=d688a0f356b1a9c1c7560b64d2957a5af2a066e4ada273984b68b7296d4c213a -->

### Re: Subagent completed — agent-doc: `subagent:dev_tmux_feedback` returned work for queue item “What is wrong with dev.md? I cannot open the tmux pane. There needs to be feedback in the editor. `> 4 | : resume:` was fixed but still not opening and has the error message saying `> 4 | : resume:` is broken.”; coordinator integration and closeout are pending.
<!-- agent-doc-subagent-completion key=0ad17090bf998c7ddbe0b08e5cf2354bbbe1d51305084533da3a305e12cdfb50 -->

### Re: Subagent completed — agent-doc: `subagent:steering_delivery_feedback` returned work for queue item “Error in this session: [route] explicit send for /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md: delivered 1 pending steering item(s) to the owning turn (sent=explicit); its next tool call, its turn-boundary report, or the idle wake surfaces them. No second trigger was injected: the open closeout still owns the pane (blocker: [session-check] IN PROGRESS: cycle `cycle-1791930737701` remains `preflight_started` (preflight_started) while owning pane `%192` has a fresh active-turn lease — the response turn is still running, so no response capture or terminal commit exists yet. This is not abandoned closeout evidence. Do not recover or abandon this cycle, rerun finalize/write, or restart/clear the owner from another pane. Re-run `agent-doc session-check /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md` only after the owning turn reaches its terminal boundary.). steering_delivered=1 sent=explicit ui_outcome_contract=ui-outcome-v1 ui_outcome=queued_behind_owner ui_outcome_class=ok next_action=wait_for_owner_turn_to_drain”; coordinator integration and closeout are pending.
<!-- agent-doc-subagent-completion key=15425ea8539c2321243f2fd81889fab7376872c30fe5fc0b9aed444d4266000c -->

### Re: Subagent completed — agent-doc: `subagent:subagent_completion_feedback` returned work for queue item “In sdk.md, I have a long list of subagents. When a subagent completes, there's no feedback. agent-doc should immediately write to the `agent:exchange` when the subagent completes.”; coordinator integration and closeout are pending.
<!-- agent-doc-subagent-completion key=7680f31d88e549d062fb58307569aa82370c0135e8f06a360e5b9acb899640e6 -->

### Re: Subagent completed — agent-doc: `subagent:this_doc_pane_visibility` returned work for queue item “Now this document's tmux pane is not visible.”; coordinator integration and closeout are pending.
<!-- agent-doc-subagent-completion key=47c22e67b3236b9d85d78a52c0cbfa076c200cadb743a95d9d7f32ef3c88e2c5 -->

### Re: Subagent completed — agent-doc: `subagent:this_doc_pane_recurrence` returned work for queue item “This document's tmux pane was visible for a bit...now it is not visible. Please fix in agent-doc + this session.”; coordinator integration and closeout are pending.
<!-- agent-doc-subagent-completion key=5594d9bd2705023c994fc1b67a3fc18a319c1606389c62b2e192105494a30b3e -->

### Re: Subagent completed — agent-doc: `subagent:midturn_recycle_recovery` returned work for queue item “See ad.md: "A recycle can only happen at a turn boundary": Make recycle occur during a turn so the session can immediately recover.”; coordinator integration and closeout are pending.
<!-- agent-doc-subagent-completion key=67ec9dcfa805a1e1ceb88f6b38e4dc64246de8d1c193aa153292fc9507afd914 -->
> **Chat prompt (#chatprompt):** cont

### Re: integrated agent-doc fixes and live verification — gpt-5 · 2026-10-13T20:30:08-04:00

> **Queue prompt:**
>
> do [#likewaysharing]
>
> do [#publish498verify]


Completed, pushed, installed, and live-verified on `main` at `e61d247c3a510c23d6c8a3c4d31ea10798384a2a`. No additional release was created; the published release remains `v0.35.498`.

**Project-wide presets**

The recommended and implemented model is one canonical `.agent-doc/project.md`. Its YAML frontmatter supplies live, read-time `presets` / `prompt_presets` defaults to every existing session document under the nearest `.agent-doc` project root. Values are never copied or synchronized into documents. Document-local exact-key declarations win, including null, in-progress, or malformed local declarations that intentionally mask inheritance. Only presets inherit; the project document body and unrelated frontmatter are inert. Missing project files preserve local-only behavior, while an unreadable or malformed present project file fails closed with an actionable error.

> **Queue prompt:** Another thought on the project `presets`. We have `.agent-doc/project.md`, which has the project frontmatter `presets` which are not copied onto project docs. But the project docs fallback to `.agent-doc/project.md` and can override presets set in `.agent-doc/project.md`.

That is now the implemented contract: `.agent-doc/project.md` is a dynamic fallback, and each project document can override individual preset keys without materializing the inherited values.

> **Queue prompt:** Another alternative is to use yaml, which has a similar format to frontmatter. So `.agent-doc/project.yaml` would have a `presets` key which is an key/value object.

I rejected `.agent-doc/project.yaml`. It would duplicate configuration discovery and migration surfaces while losing the useful Markdown body. `.agent-doc/project.md` already provides the same YAML shape in frontmatter and remains the single source of truth.

**GitHub issue #250 and formal validation**

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/250: Use formal logic to validate this class of error does not happen anymore. This has been a reoccurring issue in Coder environments.

Preset expansions honored:

- `#gh-fix`: “fix then close”
- `#spec-test-build-install-commit-push`: “update spec + tests. build + install for local testing. commit + push”

Issue #250 is fixed and closed. Remote layout correction now preserves the pre-guess base columns through repeated lagging selection guesses. The TLA+ model proves `CorrectionUsesPreGuessBase`, `ProvisionalPreservesAncestry`, and `ProvisionalClearsAfterCorrection`; a deliberately broken configuration violates the core invariant, and a reachability configuration proves the correction path is non-vacuous. The production refinement exhaustively checked 576 traces, including 288 repeated-lagging-selection traces. Evidence is recorded at https://github.com/btakita/agent-doc/issues/250#issuecomment-6155866522.

**Release publication**

`v0.35.498` is fully published at `b9c80b963c19c98786f0d8219fca91b647e58e32`: GitHub Release has all 15 expected assets, its release workflow and main CI succeeded, and PyPI serves `agent-doc==0.35.498`. The Homebrew tap was manually updated at `fea81d6`; `HOMEBREW_TAP_TOKEN` is still absent, so restoring automated tap publication remains an external secret/configuration escalation rather than something I can truthfully claim was restored. I did not create a duplicate tag or publication.

**Idle CPU/read thrash**

> **Queue prompt:** [from:coordinator key=residual-idle-20261013] agent-doc is still thrashing while idle.

Resolved. The remaining causes were SQLite convergence scans plus stale renamed-document supervisor lifecycle handling. Recycle fanout now requires positive checkpoint/marker evidence, renamed-document reexec can adopt exactly one same-project current-generation lease for the unchanged supervisor PID, and missing/ambiguous/closed/foreign targets remain fail-closed.

The recovered `observability.md` actor preserved its original session and pane. Two raw-event-stable 20-second samples measured 2.897% CPU / 0.072 MB/s and 4.597% CPU / 3.615 MB/s, down 68–80% CPU and 86.8–99.7% read throughput from the prior clean 14.4% / 27.4 MB/s baseline. No deleted-image agent-doc process remains in the affected `haiven-dev` project.

**Subagent Stop-hook continuation**

> **Queue prompt:** [from:coordinator key=subagent-blocked-hook-20261013] Fix subagent reporting queue continuation as Blocked by hook.

Fixed. A clean-session continuation now requires both the coordinator session and the exact admitted turn identity. A foreign background collaboration turn sharing the coordinator session returns normal continuation instead of inheriting the queue owner’s `Blocked by hook`; open-cycle drift and refused-admission behavior remain fail-closed.

**dev.md stale frontmatter and editor feedback**

> **Queue prompt:** What is wrong with dev.md? I cannot open the tmux pane. There needs to be feedback in the editor. `> 4 | : resume:` was fixed but still not opening and has the error message saying `> 4 | : resume:` is broken.

The affected file was `tasks/dev/dev.md`. Its valid correction was trapped behind 92 pending local editor splices while controller canonical still held the malformed generation; the prior pane had exited, so restart also reported `missing_actor`. The JetBrains replica path now reconstructs the exact captured pre-edit buffer, rebases only captured operator edits through controller canonical, and never consumes newer pending edits. Pane-open and document-command failures appear in a bounded editor-top status strip, and an absent reported source line is explicitly identified as stale controller frontmatter.

**Explicit steering delivery feedback**

> **Queue prompt:** Error in this session: [route] explicit send for /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md: delivered 1 pending steering item(s) to the owning turn (sent=explicit); its next tool call, its turn-boundary report, or the idle wake surfaces them. No second trigger was injected: the open closeout still owns the pane (blocker: [session-check] IN PROGRESS: cycle `cycle-1791930737701` remains `preflight_started` (preflight_started) while owning pane `%192` has a fresh active-turn lease — the response turn is still running, so no response capture or terminal commit exists yet. This is not abandoned closeout evidence. Do not recover or abandon this cycle, rerun finalize/write, or restart/clear the owner from another pane. Re-run `agent-doc session-check /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md` only after the owning turn reaches its terminal boundary.). steering_delivered=1 sent=explicit ui_outcome_contract=ui-outcome-v1 ui_outcome=queued_behind_owner ui_outcome_class=ok next_action=wait_for_owner_turn_to_drain

Fixed. Explicit steering delivered to the current owner now emits typed `ui_outcome=steering_delivered_to_owner`, concise informational success, and no route-blocked projection. JetBrains and VS Code also recover legacy `Error:` wrappers as success while preserving the no-second-trigger invariant.

**Immediate subagent completion feedback**

> **Queue prompt:** In sdk.md, I have a long list of subagents. When a subagent completes, there's no feedback. agent-doc should immediately write to the `agent:exchange` when the subagent completes.

Implemented `agent-doc queue report-complete`. The coordinator immediately writes a deduplicated, N−1-compatible inert `### Re: Subagent completed` artifact through document authority while retaining the claim for integration. Cross-process invocations serialize per document and recompute under authority; attached editors receive a durable retained intent and the CLI returns immediately, while detached documents use CAS. Ordinary `queue release` no longer claims completion.

Live validation against this active session exposed and then fixed the first implementation’s unbounded convergence wait. The final six completion reports returned in 0.65–0.80 seconds each; the four-process regression completes in 0.33 seconds with every distinct notice exactly once. Refresh-stable dedupe, mixed-prompt classification, strict timeout, and old-controller compatibility are covered.

**This document’s pane visibility**

> **Queue prompt:** Now this document's tmux pane is not visible.

> **Queue prompt:** This document's tmux pane was visible for a bit...now it is not visible. Please fix in agent-doc + this session.

The pane was healthy but off-screen in tmux session `0`: `%192` sat in inactive stash window `@1` while the client viewed `@2`. A later editor-focus generation selected `dev` and restashed it. Successful routes now report typed `unattached_session`, `inactive_window`, or `hidden_by_zoom` visibility warnings with exact attach/reveal commands instead of silently succeeding.

The recurrence had a second cause: explicit focus on stale `%192` was rejected merely because visible peer `%199` had a live turn, even though `%192` also had fresh active-turn proof. Layout admission now permits the explicitly focused active pane to replace/stash the peer without interrupting either turn; inactive stale panes and overdue/widening cases remain excluded. The live session was safely restored with the normal blocking focus path. `%192` is currently the visible active pane, while peer turns remain alive.

**Mid-turn recycle recovery**

> **Queue prompt:** See ad.md: "A recycle can only happen at a turn boundary": Make recycle occur during a turn so the session can immediately recover.

Implemented an adopt-only mid-turn supervisor generation handoff. The supervisor locks child reap, all supervisor IPC effects, and PTY I/O; waits for any current effect and complete PTY transaction; verifies an empty terminal-filter boundary; then execs only its own image. The exact child PID, PTY, session, and open-cycle ownership survive without kill, replay, or duplicate dispatch. Failed exec resumes the old image, and a lost adopt-only child uses spawn-without-redispatch.

The final install occurred while this already-running supervisor was on the pre-fix image, so that image cannot retroactively execute the new path during this same turn. It remains healthy with the same Codex child and will adopt the installed image at this turn boundary; subsequent stale generations can recycle during an open turn under the new fenced handoff.

**Changed paths and verification**

The combined changes span project-preset resolution/specs, remote-layout TLA/refinement tests, Codex Stop-hook identity, controller/supervisor recycle and start-runtime I/O, queue completion feedback, tmux visibility/layout admission, JetBrains/VS Code feedback, and all three JetBrains package-generation manifests.

Coordinator verification on the exact pushed bytes:

- `make check`: exit 0; 10,466 Rust tests passed, 167 skipped; VS Code, cross-editor, JetBrains classic/262/263, Python bootstrap, docs/audits, and all 135 TLA checks passed.
- `make tmux-ci`: exit 0; the seven long queue-clear tests and all route/layout/cross-editor checks passed.
- `make install`: exit 0; binary/native library installed, IntelliJ plugin dynamically upgraded, Rider package installed for next start, and VS Code extension installed.
- Completion-feedback live smoke: six reports accepted in under one second each while the active turn remained intact.
- GitHub CI run `38490233533` for `e61d247c3a` is still in progress; local authoritative gates are the closeout proof.

Confidence: high. Escalation: only the missing `HOMEBREW_TAP_TOKEN` secret remains external. The exchange remains above the session-accretion warning threshold; the active queue was completed without interrupting it for compaction.
> **Chat prompt (#chatprompt):** cont

### Re: integrated agent-doc fixes and live verification — gpt-5 · 2026-10-13T20:30:08-04:00

Completed, pushed, installed, and live-verified on `main` at `e61d247c3a510c23d6c8a3c4d31ea10798384a2a`. No additional release was created; the published release remains `v0.35.498`.

**Project-wide presets**

The recommended and implemented model is one canonical `.agent-doc/project.md`. Its YAML frontmatter supplies live, read-time `presets` / `prompt_presets` defaults to every existing session document under the nearest `.agent-doc` project root. Values are never copied or synchronized into documents. Document-local exact-key declarations win, including null, in-progress, or malformed local declarations that intentionally mask inheritance. Only presets inherit; the project document body and unrelated frontmatter are inert. Missing project files preserve local-only behavior, while an unreadable or malformed present project file fails closed with an actionable error.

> **Queue prompt:** Another thought on the project `presets`. We have `.agent-doc/project.md`, which has the project frontmatter `presets` which are not copied onto project docs. But the project docs fallback to `.agent-doc/project.md` and can override presets set in `.agent-doc/project.md`.

That is now the implemented contract: `.agent-doc/project.md` is a dynamic fallback, and each project document can override individual preset keys without materializing the inherited values.

> **Queue prompt:** Another alternative is to use yaml, which has a similar format to frontmatter. So `.agent-doc/project.yaml` would have a `presets` key which is an key/value object.

I rejected `.agent-doc/project.yaml`. It would duplicate configuration discovery and migration surfaces while losing the useful Markdown body. `.agent-doc/project.md` already provides the same YAML shape in frontmatter and remains the single source of truth.

**GitHub issue #250 and formal validation**

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/250: Use formal logic to validate this class of error does not happen anymore. This has been a reoccurring issue in Coder environments.

Preset expansions honored:

- `#gh-fix`: “fix then close”
- `#spec-test-build-install-commit-push`: “update spec + tests. build + install for local testing. commit + push”

Issue #250 is fixed and closed. Remote layout correction now preserves the pre-guess base columns through repeated lagging selection guesses. The TLA+ model proves `CorrectionUsesPreGuessBase`, `ProvisionalPreservesAncestry`, and `ProvisionalClearsAfterCorrection`; a deliberately broken configuration violates the core invariant, and a reachability configuration proves the correction path is non-vacuous. The production refinement exhaustively checked 576 traces, including 288 repeated-lagging-selection traces. Evidence is recorded at https://github.com/btakita/agent-doc/issues/250#issuecomment-6155866522.

**Release publication**

`v0.35.498` is fully published at `b9c80b963c19c98786f0d8219fca91b647e58e32`: GitHub Release has all 15 expected assets, its release workflow and main CI succeeded, and PyPI serves `agent-doc==0.35.498`. The Homebrew tap was manually updated at `fea81d6`; `HOMEBREW_TAP_TOKEN` is still absent, so restoring automated tap publication remains an external secret/configuration escalation rather than something I can truthfully claim was restored. I did not create a duplicate tag or publication.

**Idle CPU/read thrash**

> **Queue prompt:** [from:coordinator key=residual-idle-20261013] agent-doc is still thrashing while idle.

Resolved. The remaining causes were SQLite convergence scans plus stale renamed-document supervisor lifecycle handling. Recycle fanout now requires positive checkpoint/marker evidence, renamed-document reexec can adopt exactly one same-project current-generation lease for the unchanged supervisor PID, and missing/ambiguous/closed/foreign targets remain fail-closed.

The recovered `observability.md` actor preserved its original session and pane. Two raw-event-stable 20-second samples measured 2.897% CPU / 0.072 MB/s and 4.597% CPU / 3.615 MB/s, down 68–80% CPU and 86.8–99.7% read throughput from the prior clean 14.4% / 27.4 MB/s baseline. No deleted-image agent-doc process remains in the affected `haiven-dev` project.

**Subagent Stop-hook continuation**

> **Queue prompt:** [from:coordinator key=subagent-blocked-hook-20261013] Fix subagent reporting queue continuation as Blocked by hook.

Fixed. A clean-session continuation now requires both the coordinator session and the exact admitted turn identity. A foreign background collaboration turn sharing the coordinator session returns normal continuation instead of inheriting the queue owner’s `Blocked by hook`; open-cycle drift and refused-admission behavior remain fail-closed.

**dev.md stale frontmatter and editor feedback**

> **Queue prompt:** What is wrong with dev.md? I cannot open the tmux pane. There needs to be feedback in the editor. `> 4 | : resume:` was fixed but still not opening and has the error message saying `> 4 | : resume:` is broken.

The affected file was `tasks/dev/dev.md`. Its valid correction was trapped behind 92 pending local editor splices while controller canonical still held the malformed generation; the prior pane had exited, so restart also reported `missing_actor`. The JetBrains replica path now reconstructs the exact captured pre-edit buffer, rebases only captured operator edits through controller canonical, and never consumes newer pending edits. Pane-open and document-command failures appear in a bounded editor-top status strip, and an absent reported source line is explicitly identified as stale controller frontmatter.

**Explicit steering delivery feedback**

> **Queue prompt:** Error in this session: [route] explicit send for /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md: delivered 1 pending steering item(s) to the owning turn (sent=explicit); its next tool call, its turn-boundary report, or the idle wake surfaces them. No second trigger was injected: the open closeout still owns the pane (blocker: [session-check] IN PROGRESS: cycle `cycle-1791930737701` remains `preflight_started` (preflight_started) while owning pane `%192` has a fresh active-turn lease — the response turn is still running, so no response capture or terminal commit exists yet. This is not abandoned closeout evidence. Do not recover or abandon this cycle, rerun finalize/write, or restart/clear the owner from another pane. Re-run `agent-doc session-check /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md` only after the owning turn reaches its terminal boundary.). steering_delivered=1 sent=explicit ui_outcome_contract=ui-outcome-v1 ui_outcome=queued_behind_owner ui_outcome_class=ok next_action=wait_for_owner_turn_to_drain

Fixed. Explicit steering delivered to the current owner now emits typed `ui_outcome=steering_delivered_to_owner`, concise informational success, and no route-blocked projection. JetBrains and VS Code also recover legacy `Error:` wrappers as success while preserving the no-second-trigger invariant.

**Immediate subagent completion feedback**

> **Queue prompt:** In sdk.md, I have a long list of subagents. When a subagent completes, there's no feedback. agent-doc should immediately write to the `agent:exchange` when the subagent completes.

Implemented `agent-doc queue report-complete`. The coordinator immediately writes a deduplicated, N−1-compatible inert `### Re: Subagent completed` artifact through document authority while retaining the claim for integration. Cross-process invocations serialize per document and recompute under authority; attached editors receive a durable retained intent and the CLI returns immediately, while detached documents use CAS. Ordinary `queue release` no longer claims completion.

Live validation against this active session exposed and then fixed the first implementation’s unbounded convergence wait. The final six completion reports returned in 0.65–0.80 seconds each; the four-process regression completes in 0.33 seconds with every distinct notice exactly once. Refresh-stable dedupe, mixed-prompt classification, strict timeout, and old-controller compatibility are covered.

**This document’s pane visibility**

> **Queue prompt:** Now this document's tmux pane is not visible.

> **Queue prompt:** This document's tmux pane was visible for a bit...now it is not visible. Please fix in agent-doc + this session.

The pane was healthy but off-screen in tmux session `0`: `%192` sat in inactive stash window `@1` while the client viewed `@2`. A later editor-focus generation selected `dev` and restashed it. Successful routes now report typed `unattached_session`, `inactive_window`, or `hidden_by_zoom` visibility warnings with exact attach/reveal commands instead of silently succeeding.

The recurrence had a second cause: explicit focus on stale `%192` was rejected merely because visible peer `%199` had a live turn, even though `%192` also had fresh active-turn proof. Layout admission now permits the explicitly focused active pane to replace/stash the peer without interrupting either turn; inactive stale panes and overdue/widening cases remain excluded. The live session was safely restored with the normal blocking focus path. `%192` is currently the visible active pane, while peer turns remain alive.

**Mid-turn recycle recovery**

> **Queue prompt:** See ad.md: "A recycle can only happen at a turn boundary": Make recycle occur during a turn so the session can immediately recover.

Implemented an adopt-only mid-turn supervisor generation handoff. The supervisor locks child reap, all supervisor IPC effects, and PTY I/O; waits for any current effect and complete PTY transaction; verifies an empty terminal-filter boundary; then execs only its own image. The exact child PID, PTY, session, and open-cycle ownership survive without kill, replay, or duplicate dispatch. Failed exec resumes the old image, and a lost adopt-only child uses spawn-without-redispatch.

The final install occurred while this already-running supervisor was on the pre-fix image, so that image cannot retroactively execute the new path during this same turn. It remains healthy with the same Codex child and will adopt the installed image at this turn boundary; subsequent stale generations can recycle during an open turn under the new fenced handoff.

**Changed paths and verification**

The combined changes span project-preset resolution/specs, remote-layout TLA/refinement tests, Codex Stop-hook identity, controller/supervisor recycle and start-runtime I/O, queue completion feedback, tmux visibility/layout admission, JetBrains/VS Code feedback, and all three JetBrains package-generation manifests.

Coordinator verification on the exact pushed bytes:

- `make check`: exit 0; 10,466 Rust tests passed, 167 skipped; VS Code, cross-editor, JetBrains classic/262/263, Python bootstrap, docs/audits, and all 135 TLA checks passed.
- `make tmux-ci`: exit 0; the seven long queue-clear tests and all route/layout/cross-editor checks passed.
- `make install`: exit 0; binary/native library installed, IntelliJ plugin dynamically upgraded, Rider package installed for next start, and VS Code extension installed.
- Completion-feedback live smoke: six reports accepted in under one second each while the active turn remained intact.
- GitHub CI run `38490233533` for `e61d247c3a` is still in progress; local authoritative gates are the closeout proof.

Confidence: high. Escalation: only the missing `HOMEBREW_TAP_TOKEN` secret remains external. The exchange remains above the session-accretion warning threshold; the active queue was completed without interrupting it for compaction.
<!-- agent:boundary:5be64bbf -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built
<!-- agent:queue subagents preset="#spec-test-build-install-commit-push" priority go -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#jb262dynlauncherrecommen] [recommended] [operator-verify] [agent-doc] Operator decision: jb262dynupgrade (153c72916) chose the accurate-warning option (a 262 package reports declined_by=modular_package and falls back to file replacement + restart). Either keep that as the supported policy (then close this), or authorize a real restart-free launcher for the exact-262 split-mode plugin that swaps backend + frontend (the frontend may run in a separate JetBrains Client process), proven against a live 262 IDE.
- [/] [#pypiquotarecheck] Track pypi/support#12214 (agent-doc project limit request, 25 GiB) to a decision. Unblocked mechanically by a human reply on pypi/support#12214, or make pypi-quota-check reporting a ceiling in reach. Rechecked 2026-10-12: pre-cutover 131 releases 8.361 GiB (0.35.113..0.35.343), post-cutover 91 releases 3.19 MiB (mean 35.9 KiB), about 47,722 further releases fit; VERDICT headroom fine. The support issue is still open with one comment, unchanged since 2026-09-10. Nothing to act on or delete. If the grant lands, record it and drop this item; deletion stays reserved for a ceiling the authenticated check reports.
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#ghrecvpolicy] [agent-doc] Recipient-side [receive] auto_create_components policy for multi-user workspaces (GH #40, https://github.com/btakita/agent-doc/issues/40). Parked until multi-user workspace support exists.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
