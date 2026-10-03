---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: c47dae65-e36f-40cb-8ebb-b7c9573e3c41
  codex: 01a0fd09-a13c-75b1-aa00-0e20ce0bd034
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#actionable-review': Add actionable review items into backlog + queue
  '#agent-doc-causes': Fix agent-doc causes
  '#do-backlog': /goal do backlog items
  '#gh-fix': fix then closerelease
  '#release': release + publish
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261003-000203.md`*

Compacted content:
- Archived 4 response topic(s): GH #99, #100, #101 and release; GH #102; GH #104–105, queue fixes, retained delivery, tmux sync, and v0.35.440; 1 more
- Prior summary/context: prior compacted content: Archived 5 response topic(s): #gh-fix issue 95; #gh-fix issue 96; #gh-fix issue 97; 2 more

### Re: GH #106, GH #107, and release v0.35.441 — opus-5.5 · 2026-10-02T20:45-04:00

> **Queue prompt:** do https://github.com/btakita/agent-doc/issues/106

> **Queue prompt:** do https://github.com/btakita/agent-doc/issues/107

> **Queue prompt:** release + publish

**GH #106: the layout escalation no longer feeds on itself** (`0bbc15868`). When you switch tabs and the target pane is stashed, the escalation used to take the last published layout, add the document as a new column, and publish that as the next layout. Each switch then read the previous switch's output, which is why the columns climbed 1→2→3→4 within one second. Now the saved layout is republished unchanged, and only when it already contains the document. Otherwise the escalation stops and logs `cause=document_outside_retained_layout`. Because nothing is ever appended, the column count can't exceed what the editor reported, and the order can't depend on which document asked first (the issue's asks 1–4).
- New regression test `focus_only_escalations_never_grow_the_retained_layout`: three uncovered documents each try to escalate against a one-column layout, and the layout stays at one column every time. The existing retained-layout test is updated to match. Spec updated in `specs/07-session-tmux-commands.md`.
- Not covered by this change: asks 5 (log when one projection undoes another) and 6 (clean up the stale-supervisor panes piling up in stash). Both repeat GH #105 asks that v0.35.440 addressed on the accounting side. The escalation was the part that kept making them worse.

**GH #107: `agent-doc upgrade` now updates editor plugins too** (`f0797d87d`). The one-shot upgrade runs the same plugin update as `--auto`. It does this after a binary upgrade and also when the binary is already current, so re-running it repairs a workspace an earlier binary-only upgrade left out of step. If the plugin step fails, the command exits non-zero and says the release is not fully installed. If the binary upgrade itself fails, plugins are left alone so they never get ahead of the binary. Two unit tests added; spec updated in `specs/07-core-commands.md`.

**Release v0.35.441**
- `make check`: 10437 tests passed, all other gates green.
- `audit-docs` first failed because the superproject's Grok, OpenCode, Codex and Cursor skill copies still carried an older version. I regenerated them with `skill install --all` and the audit passed.
- Version bumped with `release-version`; VERSIONS.md entry and editor-parity row added (no plugin change; plugin stays 0.2.480).
- `make release` re-ran the full suite, tagged and pushed `v0.35.441`, and ran `install-full`. `agent-doc --version` now reports 0.35.441.
- GH #106 and #107 are closed by their commits.
- The Release, PyPI and CI workflows were still running when I checked. The Book workflow passed.

### Re: GH #108: JetBrains upgrade reporting — opus-5.5 · 2026-10-02T21:33-04:00

> **Queue prompt:** do https://github.com/btakita/agent-doc/issues/108

Fixed in `cf4d7eb1d` (agent-doc) and pushed. GH #108 is closed. The guard itself is unchanged; it still refuses as designed. The fixes are to how its decision is reported:

1. **The message now names agent-doc, not the IDE.** The upgrader's reason now starts with `agent-doc declined the restart-free upgrade: this JetBrains build retires plugin classloaders asynchronously (AwaitClassloaderUnloadAsyncPostReconfiguration), so there is no safe synchronous swap point; …`. The launcher (`src/plugin.rs`) also recognises the 0.35.435–0.35.441 wording, so a launcher from an older package still blames agent-doc. "The IDE refused" now appears only when the platform actually refused.
2. **A staged install no longer prints a false success line.** It used to print `Plugin installed (v<old>)`. It now prints `Plugin v0.2.480 staged (not installed) for the next IDE start; <dir> still holds v0.2.479, which stays loaded until the IDE restarts.`
3. **It says once that the loss is permanent.** A one-time NOTE states that restart-free upgrade is permanently unavailable on that build, and that the workflow is upgrade + restart, not retrying.
4. **Decisions are written to `ops.log`.** Each attempt logs a `plugin_dynamic_upgrade outcome=hot_upgraded|staged_for_restart|restart_required declined_by=agent-doc|ide|upgrader_failure` line with the version, target, and a reason that includes the pid. This ran live during `make install`: `plugin_dynamic_upgrade outcome=hot_upgraded processes=1 version=0.2.481`.
5. **The stale marker is now removed.** `retire_satisfied_restart_marker` (agent-doc-fs) deletes the staged-restart marker once an installed jar at `staged_version` is at least as new as the marker, which proves the IDE start applied it. This runs on every preflight staged check. A same-version jar older than the marker does not count, and a marker without a staged version is never touched.
6. Ask 6 (wait for the async restart notification instead of refusing) is **not done**. The report itself marked it as unestablished.

I also fixed two remedy messages in preflight and plugin_jar that still said the IDE "refused". JetBrains plugin bumped to 0.2.481.

**Verification:**
- New Rust regression tests: attribution (current and legacy wording), the staged headline plus its `ops.log` fields, and marker retirement (all three cases).
- Updated Java test `asynchronousClassloaderRetirementStagesBeforeTouchingTheLiveGeneration`.
- `make check` exit 0: nextest ran 10440 tests, all passed.
- JetBrains gradle tests, run fresh: 14 passed, 0 failed.
- `make install` hot-upgraded the live IDE.
- CI for `cf4d7eb1d` was still in progress when last checked.
<!-- agent:boundary:d3a1b952 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- ~~do https://github.com/btakita/agent-doc/issues/108~~
- release + publish
<!-- /agent:queue -->


<!-- agent:backlog priority -->
- [ ] [#ghbrew] [agent-doc] Homebrew formula for agent-doc (GH #31, https://github.com/btakita/agent-doc/issues/31). The tap repo ~/work/btakita/homebrew-tap holds only chromium-bridge.rb today. Add Formula/agent-doc.rb pulling the per-platform GitHub Release archives (binary + cdylib together, GH #52) with SHA256SUMS, and add a release step that bumps the formula on each tag. Close #31 when brew install btakita/tap/agent-doc works.
- [ ] [#ghjbfolding] [agent-doc][jetbrains] JetBrains plugin UX phases 5-7 (GH #19, https://github.com/btakita/agent-doc/issues/19): fold agent:* component regions, add gutter icons on component markers, and a structure view of components and
- [ ] [#ghmultitmux] [agent-doc] One editor managing panes across several tmux sessions (GH #17, https://github.com/btakita/agent-doc/issues/17), e.g. a session per topic or project: an allowed-sessions list in config, with routing and sync choosing among them. Close #17 when shipped.
- [ ] [#preflightdeadline] [agent-doc] Make a preflight admission overrun impossible by construction (GH #78 follow-up). Today every wait inside preflight has its own local bound (3s settle waits, per-call controller timeouts), but nothing caps their SUM against the hook's 90s budget. The #78 session-check profile ran resolve_current_document_content 13x on a 518-byte document. Fix: (1) carry an admission deadline (the budget minus a margin) on the per-run PreflightProgress handle, and clamp every bounded wait in preflight to the time remaining; (2) check the deadline at each progress::enter phase boundary, so preflight returns a typed, named, retryable refusal at a step boundary instead of being abandoned mid-step; (3) once the first live overrun names its phase (now logged as preflight_admission_overrun), move that work off the admission path, e.g. to the supervisor's idle boundary. Code: agent-doc-preflight-command-io/src/progress.rs, src/preflight_hook.rs, agent-doc-preflight-io/src/debounce.rs.
- [/] [#apimdrevert] [agent-doc][operator-verify] src/haiven-dev/tasks/api.md 'reverted by queue updates' (2026-09-29): forensics found NO lost operator text — disk/controller/editor converged at c9273a0b (HEAD 1b432940 + Codex 00:49:49 queue stop+strike, applied as a 2038-byte CRDT delta against the converged base). Only editor-side anomaly: idea.1.log 00:40:02
6local-splice-batch 3 splices 34346->34137 chars, then 00:40:03 exact inverse back to 25509a1e (undo/redo or plugin-internal flip; no VFS reload/projection logged). Blocked on operator: which text was lost + when (JetBrains Local History on api.md). Then trace the dropping path and fix with regression coverage.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#drivecapturedsplice] [operator-verify] Captured-splice live proof (#activateinstalledjetbrai). LIVE RUN 2026-09-30 20:33Z (operator go-ahead): after harness fixes 01b1e30b7, the recipe drove IDEA end to end — edit one captured (editor_op_capture_proof operator_ops=37 shadow_replay=agreed, 20:33:01Z), canonical advanced independently via write --commit in owner pane %93 (20:33:02Z, editor acknowledged the target), edit two captured (operator_ops=36 agreed, 20:33:12Z); the final text holds every operator edit plus the advance. No text lost. The verifier still FAILS: its proving triple needs two source=captured-local-splice-recovery reads, and the plugin only takes that path (CrdtReplicaManager rebootstrapCanonicalAndForwardCapturedLocalEdit) when its replica is STALE against the canonical text. Healthy delivery reached the editor before edit two, so the recovery never had to run. DECISION NEEDED: (a) accept the healthy-path trail as the proof, extending verify-captured-splice-recovery with a capture/advance/capture triple, or (b) require the stale-replica race, making the recipe type edit two while the advance is still in flight. Aside: the scratch doc is under gitignored tmp/, so every advance ends commit_refused_ignored_path (write lands on disk, commit refused). Plan: tasks/agent-doc/plan-captured-editor-splice-recovery.md.
- [/] [#pypiquotarecheck] Track pypi/support#12214 (agent-doc project limit request, 25 GiB) to a decision. Unblocked mechanically by a human reply on pypi/support#12214, or 'make pypi-quota-check' reporting a ceiling in reach. Rechecked 2026-09-30 16:25 EDT: 8.362 GiB of 10.0 GiB, 1.638 GiB headroom, 158 releases (0.35.113..0.35.424), post-cutover mean 34.7 KiB, ~49,485 further releases fit; VERDICT headroom fine. Nothing to act on or delete. pypi/support#12214 still open, 1 comment (github-actions bot), untouched since 2026-09-10T22:46:21Z. If the grant lands, record it and drop this item. Deletion stays reserved for a ceiling the check reports (next tranche 0.35.113..0.35.161, 2.491 GiB); verify any deletion from the authenticated /manage/project/agent-doc/ pages, never from pypi.org/pypi/agent-doc/json.
- [/] [#grokmcpverify] [operator-verify] Verify an authenticated Grok Build MCP admit/read/plan/finalize cycle and same-turn auto-queue continuation. NEW BLOCKER 2026-09-30: grok 1.0.41 is now SIGNED OUT ('grok -p' -> 'Error: Not signed in'); operator runs 'grok login --device-code' (or sets XAI_API_KEY), and the free usage quota or the planned subscription must be active. Grok CLI authorization, native JSON/backend commit fixtures, pane detection and quit behavior passed earlier; the last live MCP probe stopped at the usage limit before committed closeout. Heavy is not required. Plan: tasks/agent-doc/plan-grok-build-support.md.
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#ghrecvpolicy] [agent-doc] Recipient-side [receive] auto_create_components policy for multi-user workspaces (GH #40, https://github.com/btakita/agent-doc/issues/40). Parked until multi-user workspace support exists.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
