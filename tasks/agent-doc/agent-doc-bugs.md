---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: 3d56a2a6-95ce-42c4-b71c-66c102e54fb4
  codex: 01a0cb4d-e033-7a20-b7fb-4d4a3825fa39
agent: codex
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260921-131911.md`*

Compacted content:
- Archived 46 response topic(s): GitHub issues #52 and #53; macOS-only release cadence and editor-authoritative saves; Response; 43 more
- Prior summary/context: prior compacted content: Archived 15 response topic(s): #fixcauseslazily; rename-safe panes and route-layout superses...

### Re: Response

The agent-doc hook rejected this turn before admission. It misparsed `agent-doc session-check …` as an invocation and tried to canonicalize:

`/home/brian/work/btakita/agent-loop/session-check`

That path does not exist, so no cycle contract was created. Per the [agent-doc workflow](/home/brian/work/btakita/agent-loop/.claude/skills/agent-doc/SKILL.md), I must stop without running preflight or modifying the new issue.

Current release state: `v0.35.406` and commit `aa14cbb28` are pushed; GitHub Release/PyPI workflows were still running and are not yet verified complete. The `tasks/api.md` reconciliation defect remains to be fixed in a newly admitted cycle.
❯ Diagnose + unwedge api.md turn. Fix agent-doc causes for this issue:
```
› agent-doc /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/api.md


• Agent-doc admission failed closed: the editor-visible changes are stranded, not deferred, and waiting will not commit
  them. No cycle or retained response capture exists.

  From the pane that owns this session, run:

  agent-doc commit /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/api.md

  If an unwritten response body remains, use agent-doc write --commit … instead. Other panes will fail with pane
  ownership mismatch.

  done 10:19 AM
```

### Re: api.md admission recovery — gpt-5 · 2026-09-21T11:02-04:00

Fixed and deployed the agent-doc cause. The `api.md` turn is unwedgeed: `session-check` now reports cycle `cycle-1789999472341` committed with `commit_success` and `no_drainable_work`.

The failure was an authority-boundary contradiction. A CRDT retry could prove that canonical state and disk already exactly matched the response target, return an unapplied/converged no-op, and then have the outer serialized write path demand a new visible-delivery receipt. Because a no-op emits no new delivery, that manufactured an ownerless retained refusal and made the next admission call the already-durable response “stranded.”

The invariant is now: an attached write is complete without a new ACK only when the relay reports an unapplied converged no-op and both canonical CRDT state and disk exactly match its target hash. `atomic_write_rebased_through_authority_body` remains the policy owner. Applied writes still require the strict editor receipt and native-save proof; non-durable no-ops remain retained; divergent authority still follows the existing rebase path. This remains a one-shot atomic boundary over the existing CRDT observations, so no parallel reactive owner or imperative refresh loop was introduced.

Regression coverage recreates the released-barrier state with a live replica that still has an undelivered patch, proves no visible receipt exists, and verifies the exact canonical+disk target completes without leaving a pending write.

Verification:

- Focused regression: passed.
- `make check`: passed end-to-end — 9,619 Rust tests, 179 VS Code tests, JetBrains/native cross-editor harness, release/privacy audits, bootstrap tests, and TLA+ model checks.
- Private-name denylist: no matches in the edited public source.
- Latest pre-existing CI was green; CI for this push is currently in progress.

Committed and pushed to `agent-doc` main as `9848fe7e8` (`fix: accept exact durable CRDT no-op`). The tested build and both editor packages were installed, and the `api.md` project controller was marked for two-phase recycle.

Operator action: restart IntelliJ IDEA to activate the newly installed JetBrains package generation; the running IDE still maps the superseded plugin JAR.

No actionable backlog follow-up remains because the diagnosed defect, regression coverage, full verification, push, install, and controller recycle request are complete.

### Re: Response

Confirmed. The old IntelliJ process is gone, and the new process started at 11:33—after the current plugin JAR was installed at 11:01. The updated plugin generation is active.

### Re: GitHub issue #56 — gpt-5 · 2026-09-21T12:23:01-04:00

> **Queue prompt:** Fix https://github.com/btakita/agent-doc/issues/56 + release + publish

Completed and closed issue #56.

- Implementation: fallback release discovery now excludes prereleases and drafts; GitHub requests use `GITHUB_TOKEN` or `GH_TOKEN` when available; exhausted rate limits report reset and authentication guidance.
- Specification and tests: regression coverage is in `src/plugin.rs`; the normative release-resolution contract is now in `SPEC.md`.
- Verification: full local `make check` passed at release commit `aa14cbb28` and again on current main after the spec update. Release CI passed; the spec-only CI run is in progress.
- Delivery: implementation/release commit `aa14cbb28` shipped as published release `v0.35.406`; spec follow-up `511c8a8b6` is pushed to `origin/main`. The installed CLI reports `agent-doc 0.35.406`.
- GitHub issue #56 is closed.

No additional follow-up is needed.

### Re: attached-replica recovery and layout convergence — gpt-5.6 · 2026-09-21T21:41-04:00

> **Queue prompt:**
>
> Run Agent Doc on agent-doc-bugs.md issue: route_queue_activation: failed to resolve current document /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md: editor is still attached for /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md; missing_replica recovery exhausted and disk read authority is refused

Fixed and deployed through agent-doc 0.35.414 (`924bbdbdf`, `d6779f7ad`, `105d6de8a`, `f777a3fbf`, `1b632504f`, `8c19dfd52`, `4749c7ac6`, `ccf73b94d`, and `5eb2d3555`, pushed to `origin/main`).

- Attached editors with a missing controller replica now get a bounded fresh-endpoint recovery: the controller bypasses a stale retained hold only for that repair, seeds the canonical frontier, preserves queued local splices, and rearms cooldown instead of refusing disk authority forever.
- Editor layout now retains controller-owned file-to-pane assignments across temporary model-authority loss, with live-pane and foreign-owner revalidation. Focusing `agent-doc-bugs2.md` no longer evicts or swaps out the existing document pane.
- Captured-response queue projection no longer performs document-actor/editor I/O while holding the shared reactive controller context. It admits that work to an asynchronous controller worker and publishes exact applied/failed receipts back as Sources. Queue completion still waits for the matching applied receipt, while unrelated editor layout can converge independently. This also fixes the reported `dev.md` dispatch failure ending in `projection_effect_in_flight`.
- The remaining retained-transition effects—activation observation, guarded CRDT projection, closeout wake, and materialized-capture settlement—now use a separate asynchronous worker with exact attempted/published frontier fencing. A blocked editor transition can no longer pin the Lazily context used by route/layout dispatch.
- Document-turn authority publication now crosses its own asynchronous boundary instead of writing the state plane from inside the deriving Lazily Effect. A regression blocks the publisher while proving actor transitions and authority reads remain responsive.
- Every remaining durable document-completion edge—retained-write settlement, compact completion, and terminal queue lifecycle publication—now admits exact generation-fenced commands to one controller worker. State-ledger or editor-adapter I/O therefore cannot hold the shared Lazily context. This closes the common `projection_effect_in_flight` cause for both the reported `dev.md` and `fpe.md` Run Agent Doc failures.
- The final controller-wide deadlock was a state-plane lock inversion: a subscriber held its channel condition mutex while acquiring the process reactive scope, while publication held that scope while notifying the channel. Subscribers now observe revision and history first, then take the wait mutex and recheck the revision before sleeping. The deterministic regression blocks the process scope and proves channel publication notification remains responsive.
- The last finalize-only lock inversion was in attached-editor current-text logging: `crdt_current_text` retained the per-document relay mutex while operation logging crossed into the durable turn-attribution ledger. Current-text reads now release the relay before that boundary. The regression acquires the relay at the exact pre-log seam, proving concurrent editor/state callbacks cannot strand the controller.
- Turn attribution is now primed at controller-request, route, and idle-watch admission, before any CRDT relay or state critical section can be entered. Later operation logs are request-local memo hits instead of live-projection reads. This removes the remaining relay/state inversion exposed by the first post-0.35.412 finalize attempt; regressions prove admission precedes domain dispatch and that logging does not revisit its durable source after priming.
- Closeout-owner claim and release now share one serialized durable-append → live-apply ingress with editor, retained-write, compact, and queue state events. The claim no longer retains the live projection mutex while waiting for SQLite, removing the memory↔SQLite inversion exposed by the post-0.35.413 finalize attempt. A deterministic regression holds SQLite's writer lock, proves the claim waits, and simultaneously acquires the projection mutex.

Verification passed through the complete release gate: 9,633 Rust tests, simulations, 179 VS Code tests, JetBrains and VS Code suites, cross-editor native simulation, version/docs/parity audits, Python bootstrap tests, Lean, and all TLA models. Direct ledger evidence confirms the timed-out attempts created no new response capture or editor-op capture, so this preserved response remains the sole closeout payload.

### Re: GitHub issue #56 queue cleanup — gpt-5.6 · 2026-09-21T21:51-04:00

> **Queue prompt:**
>
> The sessiuon c


> **Queue prompt:**
>
> do [#fixhttpsgithub]


`#fixhttpsgithub` was already completed: GitHub issue #56 is closed, release `v0.35.406` was published, and the implementation, specification, tests, push, and installation are recorded in the earlier issue response and completed-work archive. I marked the duplicate stale backlog/queue item done; no additional follow-up is needed.

### Re: supervisor restart crash — gpt-5.6 · 2026-09-21T23:50-04:00

> **Queue prompt:**
>
> release + publish


> **Queue prompt:**
>
> Fix gh issue #57.


> **Queue prompt:**
>
> Address new comments on gh issue #55.


> **Queue prompt:**
>
> Fix Run Agent Doc on docs.md then switching editor to dev.md resulted in focusing on a tmux pane in the stash tmux window.


> **Queue prompt:**
>
> Run Agent Doc on dev.md with agent:queue not in go mode resulted in a single tmux pane layout.


> **Queue prompt:**
>
> do [#sessioncrashedduring]


> **Queue prompt:** The session crashed during restart?

Yes. The earlier install-triggered restart genuinely killed the supervisor handoff while leaving its preserved Codex child orphaned.

The old supervisor executed `execve_preserve_child`, but the replacement ran ordinary document admission during the controller/editor relay handoff. With no usable current-text witness at that instant, admission failed with `editor is still attached ... missing_replica recovery exhausted and disk read authority is refused`; the replacement exited before `supervisor_reexec_adopted_child`, and the session then ended as `sync_missing_pane`.

Fixed and deployed in agent-doc 0.35.416, commit `6f8228609` (`fix: preserve child across supervisor reentry`), pushed to `origin/main`.

The replacement-supervisor path is now explicitly a transport reentry around an already-running child. If live controller/editor authority is momentarily unavailable during that handoff, it may read disk only as observational launch metadata, without publishing disk replica authority. It preserves the existing actor/session lifecycle, skips ordinary frontmatter repair, registers the transport, and schedules the existing live-model reconciliation. Ordinary new-session admission remains fail-closed.

Regression coverage proves both the no-relay-witness decision and the observational metadata fallback. Verification passed: 9,636 Rust tests under bounded concurrency, doctests, deterministic simulator, clippy, version/privacy/generated-doc audits, 179 VS Code tests, JetBrains tests, the real native cross-editor controller harness, Python bootstrap tests, Lean, and all TLA+ models. Two unrelated default-concurrency flakes were rerun individually and passed; the full bounded matrix was clean.

The full release build, native library, and both editor packages are installed. The CLI reports 0.35.416. During installation the original supervisor PID and exact Codex child PID remained alive; the fleet handoff is marked to occur at the safe post-turn boundary.

No additional backlog follow-up is needed for this crash.
❯ Fix Run Agent Doc on dev.md issue: route found legacy pane-association evidence for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/dev.md, but the normal path will not re-elect ownership from %68.
❯ Inspect the candidate, claim it explicitly if it is authoritative, or kill it before rerouting.
- %68 session=0 window=@0 (agent-doc) cmd=agent-doc sources=session-log,process-tree,supervisor-pid
  ❯ view: tmux capture-pane -pt %68 | tail -n 80
  ❯ assign: agent-doc claim /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/dev.md --pane %68 --force
  ❯ kill: tmux kill-pane -t %68

### Re: Response

Cannot fix this in the current turn: agent-doc refused admission before opening a cycle because an editor write is stranded with no owner.

From the session’s owning pane, run:

```bash
agent-doc commit /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md
```

If an unwritten response body remains, use `agent-doc write --commit …` instead. Then invoke agent-doc again to diagnose and fix the underlying cause.

Per the [agent-doc skill](/home/brian/work/btakita/agent-loop/.codex/skills/agent-doc/SKILL.md), I did not recreate preflight or mutate the document from this unadmitted turn.

### Re: live editor edits lost at commit barrier — gpt-5.6 · 2026-09-22T10:27-04:00

Fixed and deployed in agent-doc 0.35.417.

The `entitlements.md` loss was caused by the commit barrier treating an ordinary intermediate JetBrains save as an out-of-band disk correction while the editor replica was still live. That rebuilt the canonical CRDT epoch from the lagging disk text and rotated its lineage. The editor's next legitimate document-op frames then looked stale, were quarantined, and the canonical rebootstrap replaced the buffer—dropping the queue item, preset, and queue/backlog priority edits.

The barrier now preserves live-editor authority: baseline-to-disk reconciliation runs only when there are no live editor replicas. With an editor attached, the barrier flushes the live CRDT into the consistent cut without rebuilding the epoch or rotating lineage. Explicit controller writes continue through the CRDT publication path. A regression reproduces a lagging intermediate disk save plus an unflushed queue/priority edit and proves both edits survive with unchanged lineage. The existing out-of-band-correction regression now explicitly disconnects the editor before disk reconciliation.

While running the full gate I also fixed a deterministic test-support lifetime race: the isolated controller runtime could drop its final strong reference before its asynchronous queue-completion worker published the terminal receipt. The test adapter now retains the runtime until the projected receipts are durable.

The exact lost `entitlements.md` settings were recovered from IntelliJ Local History and restored: queue preset `create a PR if needed`, queue priority/go, and backlog priority/queue mirroring. The already-completed HAVN-1331 research prompt was not requeued. At the final 0.35.417 commit boundary, the user's newer live `#implementhttpshaiven` queue item and mirrored backlog item were also preserved and committed in `haiven-dev` as `8f9bdff`.

Verification passed:

- focused CRDT commit-barrier regressions: 7/7;
- full `make check`: 9,637 Rust tests, 179 VS Code tests, native cross-editor SimWorld, release/version/docs/parity audits, Python bootstrap tests, Lean, and all TLA+ models;
- local `make tmux-ci`: passed;
- installed CLI reports `agent-doc 0.35.417`; the matching native library and editor packages were installed, and supervisors/controllers were marked for safe-boundary recycle.

Source commit `0be958a30` (`fix: preserve live editor state at commit barriers`) is pushed to `origin/main`. No additional follow-up remains for this defect.

### Re: release 0.35.417 — gpt-5.6 · 2026-09-22T11:03:15-04:00

> **Queue prompt:** release + publish

Released and published `agent-doc` 0.35.417 from the exact tested commit `0be958a30d3c345b5dbcd0ca6e06401232ff5002` (`fix: preserve live editor state at commit barriers`).

- Pushed immutable tag `v0.35.417`; both `origin/main` and the tag resolve to `0be958a30`.
- GitHub Release is public at https://github.com/btakita/agent-doc/releases/tag/v0.35.417 with GNU Linux, musl Linux, ARM64 Linux, Windows, JetBrains 0.2.392, VS Code 0.2.71, and `SHA256SUMS` artifacts.
- PyPI serves `agent-doc==0.35.417` as `agent_doc-0.35.417-py3-none-any.whl`.
- GitHub `CI`, `Release`, and `PyPI` workflows all completed successfully. The PyPI workflow's final verification performed a clean install and confirmed the downloaded native CLI reports `agent-doc 0.35.417`.
- Local verification on the tagged commit had already passed `make check` (9,637 Rust tests plus editor, native SimWorld, Python, Lean, and TLA+ coverage), `make tmux-ci`, focused commit-barrier regressions, and `make install`; the installed CLI also reports `agent-doc 0.35.417`.

Confidence: high. The source worktree is clean and the public tag, release assets, checksum, package registry, and installed version agree.

Operator activation note: the currently running IntelliJ process still maps the superseded unlinked JetBrains plugin jar. Restart IntelliJ or close and reopen the affected editor tab to load the already-installed 0.2.392 package; this does not affect the published release.

### Re: concise IDEA lint failure — gpt-5 · 2026-09-22T13:13:44-04:00

> **Queue prompt:** Make this error message easier for a human to read given a small prompt dialog in Idea:

Implemented and pushed the IDEA-specific presentation fix as agent-doc 0.35.418 / JetBrains plugin 0.2.394 (`3b4a481ec`, `fix: simplify JetBrains lint failure notifications`).

The compact notification now reads like this instead of embedding the full nested controller error:

```text
Agent Doc couldn't compact this document because a document directive is malformed.

api.md, line 449
Attribute `queu0000e` on `agent:backlog` is missing `=value`.

Suggested fix: `queu0000e= `

Fix the directive, then try again.
```

The complete subprocess output remains in the IDEA log for diagnosis. Unknown failures are capped to a bounded notification length, so another verbose backend error cannot fill the small UI surface.

Changed paths:

- `editors/jetbrains/.../TerminalUtil.kt`: action-specific compact failure text, lint diagnostic extraction, and bounded fallback.
- `editors/jetbrains/.../TerminalUtilTest.kt`: exact regression for the reported error plus the generic length bound.
- `specs/14-realtime-workflow.md`: operator-notification contract.
- `VERSIONS.md`, `editors/release-parity.tsv`, package manifests, generated skill copies, and JetBrains `gradle.properties`: 0.35.418 / 0.2.394 version projection.

Verification and delivery:

- Focused `TerminalUtilTest`: passed.
- `make check`: passed twice, including the final post-install metadata; exit 0 and zero detected failures.
- `make install`: installed CLI/library 0.35.418, JetBrains 0.2.394, and VS Code 0.2.71; `agent-doc --version` reports 0.35.418.
- `git diff --check`: passed; source worktree clean after commit.
- Commit `3b4a481ec49ba94f4a247a23a41f774206a0bc04` is on `origin/main`.
- GitHub CI run 35759252186 is in progress; the preceding CI run is green, and local full-suite evidence is complete.

Confidence: high. No escalation is needed. Restart the running IDEA instance to activate JetBrains plugin 0.2.394. The exchange is now 216+ lines; the active queue was drained without interrupting it for compaction, as required.
❯ In api.md, I was typing in agent:queue when a turn started. My queue item was duplicated with a previous version of the item. Fix this category of bug in agent-doc.
```
- Review https://github.com/haiven-dev/haiven-api/pull/544. 
- Review https://github.com/haiven-dev/haiven-api/pull/544. How are the squashed merge SHAs handled?
```

### Re: Response

Implemented and pushed the queue-race fix as `c3879bc81`. Full `make check` passed, and CI is running.

Also:

- Preserved prior uncommitted work in local commit `3f6692b78`; not pushed because that checkout has divergent history.
- #538 need not be rebased onto #544 now. If #544 merges first, rebase #538 onto updated `main`, not the squash-discarded PR head.
- Keep #538 consumer-side; reusable logic should live in a neutral shared-action repository, not `haiven-contracts`.

The response is durably captured in agent-doc’s CRDT authority, but IntelliJ rejected the final native-save request after installation. Please restart IntelliJ once; the controller owns the retained response and should finish the document commit automatically. Do not manually resend or save the response.

### Re: Response

Fixed, committed, pushed, installed, and live-verified.

- `67edf5415` — `fix: retain editor route across layout churn`
  - Passive editor layouts can no longer supersede an in-flight Run Agent Doc route.
  - The newest passive layout is retained and applied after dispatch settles.
  - Newer explicit foreground layouts still supersede correctly.
- Live `Run Agent Doc` for `tasks/software/tsift.md` succeeded on the first attempt with no `superseded_by_newer_layout_state`.
- `make check` passed: 9,642 Rust tests, 179 VS Code tests, cross-editor simulation, parity checks, and formal models.
- Installed controller is active. [CI is running](https://github.com/btakita/agent-doc/actions/runs/35770646184).
- Earlier queue-race fix remains pushed as `c3879bc81`. Existing divergent uncommitted work was preserved in local commit `3f6692b78`.

For haiven-api:

- PR #538 does not currently need rebasing onto #544; their merge is clean.
- If #544 lands first, rebase #538 onto the resulting `main` commit—not directly onto the PR branch.
- I would not move #538 wholesale into `haiven-contracts`: it operates on consumer-repository state such as the contracts gitlink, secrets, and workflows.
- If multiple consumers need it, extract the reusable mechanism into a neutral versioned action/tool and keep a thin consumer-specific wrapper in each repository.

### Re: completed multiline queue cleanup — gpt-5 · 2026-09-22T17:44:34-04:00

> **Queue prompt:** The struck multi-line queue item below was not cleared when `agent:queue` completed.

Fixed and shipped in `agent-doc` commit `4b4fabdc2` (`fix: clear completed multiline queue residue`), now pushed to `main`.

The queue convergence policy now owns an indented Markdown continuation when it begins at or beyond the content column of the preceding completed inline queue row. That continuation drains with the completed row. Unindented, shallow-indented, or otherwise unowned freeform operator text still blocks lossless cleanup.

The invariant and policy owner are explicit: completed-row residue is decided once from the parsed ordered queue entries in `agent-doc-queue/src/queue_convergence.rs`; this is a pure one-shot convergence decision, so no polling or reactive state topology is needed. The preflight integration test proves the completed row plus indented command-error continuation clears the queue body, `auto`, and active queue state together.

Verification passed:

- focused queue-policy and preflight integration tests;
- the complete `make check` gate (9,645-test/editor-parity surface, exit 0);
- `git diff --check`;
- public/private-name denylist scan (no matches);
- local CLI, native library, JetBrains, and VS Code install.

The installed controller was marked for a two-phase recycle. CI run `35783752395` was in progress at the required one-time post-push check. Restart IntelliJ to activate the newly installed plugin package.

### Re: Response

Fixed and pushed to `main` in two commits:

- `9d2fe3ff2` — hermetic checks, Cargo/temp path handling, non-interactive VS Code packaging, safer recycle guidance.
- `b667a885c` — serialized finalize-wake publication and protected retained-settled wakes from generic downgrades.

Verification:

- `make check`: 9,648 Rust tests passed, plus editor, cross-editor, bootstrap, audit, and TLA+ checks.
- `make install`: successful; binary, native library, JetBrains, and VS Code packages installed.
- Idle-boundary recycling scheduled automatically.
- Worktree clean.
- CI snapshot was queued: run `35791291748`; not polled further per repository policy.

### Re: 2026-09-22T19:23:58-04:00 — gpt-5.6

> **Queue prompt:** On tsift.md, I switched agent: claude to agent: codex but agent-doc reverts my change. Now switching the editor to tsift.md does not auto-sync the tmux pane.

Fixed and deployed.

The frontmatter edit was not ultimately reverted: the authoritative document now retains `agent: codex`, and the controller successfully started/focused Codex pane `%117`. The later failure was a lifecycle gap: once that layout-provision pane moved to stash, the bounded orphan policy correctly reaped it, but the low-latency focus-only projection classified the resulting dead/closed actor as a non-structural refusal. Since the editor layout itself had not changed, no full reconciliation followed, so later tab selections had no pane to focus.

The JetBrains adapter now treats every unusable selected-owner receipt—stashed, reaped/dead, missing actor or registry binding, or non-focusable actor—as a generation-fenced request for a forced full-surface reconciliation. Pane creation remains exclusively in the structural layout graph, preserving exact pane cardinality and avoiding the earlier focus-path three-pane growth bug. A stale receipt still cannot override a newer editor selection, and inactive-desktop refusals remain non-structural.

Verification and deployment:

- The focused EditorTabSyncListener suite passed all 49 tests, including the new table-driven missing-owner regression.
- Full `make check` passed.
- JetBrains package generation advanced from 0.2.394 to 0.2.395, with the source digest fence updated.
- Commit `6034ff0b39` (`fix: recover reaped pane on editor selection`) was pushed to `origin/main`.
- `make install` installed the CLI/native library and JetBrains 0.2.395 package; the project controller was marked for two-phase recycle.
- CI run 35797157295 was observed once and is in progress.

Restart IntelliJ to activate plugin 0.2.395; the running IDE still has the superseded 0.2.394 JAR mapped. After restart, selecting `tasks/software/tsift.md` should force structural recovery when its pane has been reaped, then focus the replacement Codex pane.
<!-- agent:boundary:00048a2e:agent-doc-bugs -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- ~~The struck multi-line queue item below was not cleared when `agent:queue` completed.~~
- ~~Make this error message easier for a human to read given a small prompt dialog in Idea:~~
  agent-doc command failed (exit 1): Error: project controller command `compact_document` failed: [lint-gate] INTERRUPTED: 1 blocking lint finding(s) for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/api.md (mode=warn, source=default). Fix the directives below before re-running `agent-doc finalize` / `agent-doc write --commit`, or set `agent_doc_lint_dialect: off` in frontmatter / `[lint] dialect = "off"` in `.agent-doc/config.toml` to temporarily skip this gate. /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/api.md:449:1 error: attribute `queu0000e` on `agent:backlog` is missing `=value` [agent-doc/malformed-attr] hint: try `queu0000e= `
- 🚧 On tsift.md, I switched `agent: claude` to `agent: codex` but agent-doc reverts my change. Now switching the editor to tsift.md does not auto-sync the tmux pane.
- Fix api.md session issue: • Agent-doc accepted the response intent but deferred its terminal commit while the editor replica converges. Per its recovery protocol, I’m checking the session exactly once and will not resend or force-write the response.
<!-- /agent:queue -->


<!-- agent:backlog -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#runsubmitfirstmatch] [operator-verify] — confirm in a live pane that JetBrains "Run Agent Doc" now submits the trigger instead of stranding it in the composer. Fixed in 275ab868e: route_trigger_visible_in_current_draft scans newest-first, so repair_pass_through_stranded_draft no longer reports outcome=cleared over a composer that still holds the trigger. Reproduce the old shape by running Run Agent Doc twice on the same document without clearing, so a consumed copy of the identical trigger sits within 16 non-empty lines above the new draft. NEXT: a human watches one Run Agent Doc dispatch land as a started turn; then resolve. <!-- gate-verify verify="controller_editor_route_settled" disproof="startup_miss" set_at="1789950246" -->
- [/] [#cargovodtrim] [operator-verify] [P2][security] Trim the credential exposure from the livestream VOD once the operator supplies the VOD URL or platform/start-time locator. Known exposed interval: 2026-09-12T02:57:27.170Z through approximately 02:58:14Z.
- [/] [#pypiquotafix] [operator-auth] [P1] AUTHORIZED cleanup: after 0.35.383 thin wheel is live, sign into https://pypi.org/manage/project/agent-doc/releases/ in Chromium tab 0; then permanently delete the 34 oldest existing releases, 0.35.0 through 0.35.112 (all pre-2026-08-02), freeing about 1.75 GiB. Pinned PyPI installs of deleted versions will stop working. Browser is currently logged out; support request pypi/support#12214 remains open.
- [/] [#tmuxfocussyncverify] [operator-verify] Restart IntelliJ to activate JetBrains plugin 0.2.390, then switch between agent-doc-bugs.md and the cross-root session document and confirm the right tmux pane follows each selected document. Automated fix c2f5a95b4 makes selected-document focus a self-healing handoff: a missing or accepting-but-unserving destination controller is recovered by the shared native policy and the exact ordered focus fact is retried once; passive layout observation remains non-launching. Live premise measured 2026-09-21T08:00-04:00: IDEA PID 3483053 still maps agent-doc-jetbrains-0.2.389.jar while 0.2.390 is installed, so restart is the only remaining activation/eyeball step. Plan: tasks/agent-doc/plan-tmux-auto-sync-reactive.md.
- [/] [#grokmcpverify] [operator-verify] Verify an authenticated Grok Build MCP admit/read/plan/finalize cycle and same-turn auto-queue continuation once the free usage quota renews or the planned subscription is active. Grok CLI authorization, native JSON/backend commit fixtures, pane detection and quit behavior passed; the live MCP probe stopped at the usage limit before committed closeout. Installed v0.35.359; Heavy is not required. Plan: tasks/agent-doc/plan-grok-build-support.md.
- [/] [#activateinstalledjetbrai] [operator-verify] One IntelliJ restart activates the staged JetBrains plugin. PREMISE RE-MEASURED 2026-09-20T17:05-04:00: live IDEA is PID 1023909 started 2026-09-20 15:44:32; staged jar written 2026-09-20 16:42:24. The recorded PID 1645748 and the 2026-09-18 16:39:05 write are both wrong, and the PID 306200 asserted earlier in this document does not exist at all. Proof of non-activation is byte identity, not version: /proc/1023909/map_files shows agent-doc-jetbrains-0.2.388.jar (deleted) against on-disk inode 76585058, with the same 0.2.388 version string on both. Remaining splice proof unchanged: captured local edits recover across an independently advanced canonical response in a disposable live editor document. Original retained capture committed as 1b35dcb028 and all automated regressions passed. Plan: tasks/agent-doc/plan-captured-editor-splice-recovery.md.
- [/] [#harnessswitchverify] [operator-verify] Remaining proof: in a disposable live IntelliJ session, make an unsaved codex -> claude -> codex frontmatter switch while observing supervisor receipts; prove one spawn per authoritative switch, deferred restart for non-authoritative evidence, and no refusal or respawn storm. PREMISE RE-MEASURED 2026-09-20T17:05-04:00: live IDEA is PID 1023909 started 2026-09-20 15:44:32, so the previously recorded live PID 1645748 is gone as well as the older 1035669; the staged plugin jar 0.2.388 written 16:42:24 is not activated in that process; installed CLI is 0.35.400. Unblocks: provide the disposable document path and perform the real unsaved editor edit, since persisted CLI writes cannot substitute for this proof. Plan: tasks/agent-doc/plan-stop-gated-queue-head.md.
- [/] [#coder7] [operator-verify] Phase 7 — run the five-case live Coder workspace validation matrix after a reachable Coder workspace and authenticated access are available, with JetBrains Gateway remote backend, VS Code Remote-SSH, simultaneous dual-IDE attachment, workspace stop/start, and a no-tmux image variant. Plan: tasks/agent-doc/plan-coder-ide-hosted-tmux.md. Unblocks: provide the workspace URL or SSH target plus authentication and enable the current agent-doc plugin/extension in both remote IDE hosts.
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
