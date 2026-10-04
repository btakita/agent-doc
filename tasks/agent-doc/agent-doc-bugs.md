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
  '#subagents': 'run the remaining items in subagents'
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261004-070500.md`*

Compacted content:
- Archived 15 response topic(s): release + publish; release cleanup; queue dispatch; 12 more
- Prior summary/context: prior compacted content: Archived 33 response topic(s): GH #106, GH #107, and release v0.35.441; GH #108: JetBrains u...

### Re: do #steerbaselineabsorb — opus-5-5 · 2026-10-04T03:10-04:00

> **Queue prompt:** do [#steerbaselineabsorb]

I claimed this item and handed it to a background subagent (worktree `agent-doc-wt/steerbaselineabsorb`, branched from v0.35.450 `478ff964e`). It is fixing both symptoms:
1. **Steering absorbed into a commit:** steering that is still pending at closeout, but whose text is already in the committed baseline, will be saved as an unrecorded prompt (the same mechanism as `chat_prompts`). The next contract then carries it with `no_changes: false`.
2. **Answered steering keeps coming back:** an item is marked consumed once a later committed response answers it, so the hook stops resurfacing it.

It has two regression tests to add, one for each symptom, and must pass `make check` before committing on branch `steerbaselineabsorb`. When it reports back, I'll merge the branch, do one `make install`, release the claim, and close the item.

### Re: preflight refused on its own repair (haiven-dev sdk.md) — opus-5-5 · 2026-10-04T03:20-04:00

> **Chat prompt (#chatprompt):** Also fix in subagent:
> ❯ /agent-doc /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk.md
> ● Bash(agent-doc session-check tasks/sdk.md 2>&1 | grep -v '^\[perf\]' | tail -25)
>   ⎿ [session-check] ok — cycle `cycle-1791097819297` is `committed` (commit_success); repaired committed historical committed_capture snapshot drift
>      queue_continuation_required=true next_queue_prompt="Add backlog items for the demo work. Create an ADR PR in …
> ● agent-doc refused to start this turn, so I haven't written anything to tasks/sdk.md. Its own repair step left the file slightly different from what it expected.
>   I ran session-check afterwards. It fixed the mismatch and confirms my last reply (the demo table) is committed.
>   It also found your new queue item: "Add backlog items for the demo work. Create an ADR PR in the haiven-sdk repo." The loop restarts in about 90 seconds and will take that one first.

I filed this as `#preflightrepairdrift` and handed it to a second subagent in its own worktree (`agent-doc-wt/preflightrepairdrift`, branched from v0.35.450). It works separately from the `#steerbaselineabsorb` subagent.

The bug: preflight ran a repair step (fixing committed-capture snapshot drift), and then its own check refused the turn because the document no longer matched what it expected. session-check later made the same repair and reported ok, so the drift was harmless and the turn should have been admitted.

The subagent will:
1. Find the exact refusal reason in haiven-dev's `ops.log` (around 07:14Z).
2. Have preflight absorb its own repair before the admission check. Drift that would lose operator text still fails closed.
3. Add a regression test for this case and pass `make check`.

When both branches are in, I'll merge them serially and run one `make install`.

### Re: queue: GH #127, #128, #129 + release — opus-5-5 · 2026-10-04T03:52-04:00

> **Queue prompt:** do https://github.com/btakita/agent-doc/issues/127
> **Queue prompt:** do https://github.com/btakita/agent-doc/issues/128
> **Queue prompt:** do https://github.com/btakita/agent-doc/issues/129
> **Queue prompt:** release + publish

I claimed each head. Every subagent has its own worktree based on integration commit `b63deb809`, so it builds on both fixes that are not yet released:
- **GH #127** (`agent-doc-wt/gh127b`): `cross_document_sweep` uses 23–42s of the 85s admission deadline committing other documents. The fix takes it off the admission-critical path.
- **GH #128** (`agent-doc-wt/gh128b`): a stale-binary recycle reports `old_version == new_version`, then asks to restart again. The fix makes it report the version it actually re-exec'd into and stops the duplicate request.
- **GH #129** (`agent-doc-wt/gh129b`): the `session-check` repair hint asks for the very document write that the admission-deadline refusal forbids. The fix makes the two consistent and checks why the queue head was dropped.

**Both fixes are done and merged** on the integration branch (`b63deb809`), and the full `make check` is running on the merged code:
- **`#steerbaselineabsorb`** (`bc6abda76`): steering absorbed into a closeout commit now carries into the next contract with `no_changes: false`, and answered items stop resurfacing. Passed on its branch: 10822 tests.
- **`#preflightrepairdrift`** (`1c4caeded`): the haiven-dev refusal was the template-normalization repair. It ran while the operator was typing and correctly rolled forward over the new text, but the settle step only accepted the pre-typing target. Settle now accepts "the repair plus newer edits" and keeps the typed text as pending input. Passed on its branch: 10826 tests.

**release + publish** stays with me. Once the three GH branches land, I'll merge them in one at a time, run `make check` and one `make install`, cut the release, close the issues, and close all five items.

### Re: v0.35.451 released — #steerbaselineabsorb, #preflightrepairdrift, GH #127/#128/#129 — opus-5-5 · 2026-10-04T05:05-04:00

> **Queue prompt:** do [#steerbaselineabsorb]
> **Queue prompt:** do [#preflightrepairdrift]

All five fixes are integrated, released as **v0.35.451**, and installed (`agent-doc --version` → `0.35.451`).

**Integration.** I merged the five subagent branches serially onto one integration branch and added one follow-up commit (below). Full `make check` on the combined tree exited 0: **10859 tests run, 10859 passed**, 262 skipped. Release commit `db150df8c` and tag `v0.35.451` are pushed; the CI Release and PyPI workflows are running. I closed GH #127, #128 and #129 with a pointer to the release, and committed the superproject gitlink as `5575f53cf9`.

**What shipped:**
- **`#steerbaselineabsorb`:** when steering is still pending at closeout and its text is already in the committed baseline, it is saved in a per-document ledger. The next contract carries it as `absorbed_steering_prompts` / a `prompt_target` with `no_changes: false`. Once it is answered, no steering surface shows it again.
- **`#preflightrepairdrift`:** the haiven-dev refusal came from the template-normalization repair, which landed on top of text the operator typed after the repair was computed. Settle now accepts a result proven to be "the repair plus concurrent edits", and still fails closed when the repair is missing or the texts disagree. The typed text stays a pending diff.
- **GH #127:** `cross_document_sweep` now runs on its own budget: 8s by default and at most a quarter of the admission time left (`AGENT_DOC_PREFLIGHT_SWEEP_BUDGET_SECS`). Documents it doesn't reach are deferred to the next preflight through a cursor file. The hook raises its installed timeout to fit the budget.
- **GH #128:** the recycle itself was correct; only the report was wrong. The completion row now comes from the replacement's own status, and same-image recycles log as failures. A repeat restart for the same pid and generation within 30s is suppressed.
- **GH #129:** two new commands, `agent-doc backlog <FILE> requeue` and `keep-unqueued`, repair a dropped head without an admitted cycle. The deadline refusal now forbids only a response write. A queue head that leads with an open backlog id is no longer treated as free text, which was the likely way the head got dropped.
  - My follow-up commit fixes a second hint the subagent spotted: the no-response active-head hint now names `agent-doc backlog <FILE> done <id>`, which is allowed after a refused admission.

**Notes:**
- **Another session's uncommitted work in `src/agent-doc`:** a `lib.rs` edit (`#clearrestartjoinstall`) is still uncommitted there; I preserved it unchanged when fast-forwarding main. Its `VERSIONS.md` edit conflicted with the release entry. That edit looked garbled (it deleted the 0.35.449/450 entries), so I kept the release version. The original is in `stash@{0}` in the agent-doc checkout.
- **Queue items struck before the work was done:** my dispatch response quoted the free-text heads (GH #127/#128/#129, release + publish) as `> **Queue prompt:**`. That caused `#ftstrike` to strike them while they were only dispatched. The work is now done, so nothing was lost, but quoting a head that is only dispatched should not strike it.
- **Verify on the next session:** GH #128's 30s suppression means a newer client briefly connects to the older controller instead of retrying the handoff.

### Re: queue: About Agent Doc action, Dashboard — opus-5-5 · 2026-10-04T13:45-04:00

**`#editoractionmenu`: About Agent Doc.** I claimed it and handed it to a subagent working in its own worktree (`agent-doc-wt/editoractionmenu`, based on v0.35.451). The task: add an "About Agent Doc" action to each plugin's Agent Doc menu (JetBrains, VS Code, and Zed if it has a menu surface). It shows:
- the plugin version;
- the agent-doc binary version actually in use;
- the native library (cdylib) version and build id;
- a warning when the plugin and binary versions don't match.

It can add a CLI command such as `agent-doc version --json` if that helps. It will update the parity tables, bump the plugin versions, and run `make check` plus the plugin tests. I'll integrate the branch, install, and release.

**`#gvqv`: a half-typed fragment.** This was the start of your Dashboard request: preflight made backlog item `#gvqv` from `Add a ``` before you finished typing. I'm not dispatching it. The full line is still being written below it ("Add a `Dashboard` editor prompt + action to show the Agent Doc dashboard. Can we also support a dashboard md file that ?"). I'll dispatch the Dashboard request once that line settles, and close `#gvqv` into it.

I filed the fragment as a bug, `#halftypedcoin`: turning a free-text queue line into a backlog item should wait while the line is still being typed, or while it ends inside an unclosed code span.

**Install note.** The `stale_install` warning came up because I built v0.35.451 from a clean worktree, so the builds in `src/agent-doc`'s own target directory are older than its source. The binary actually in use is v0.35.451: `.bin/agent-doc` runs the newer `~/.cargo/bin/agent-doc`. The warning will clear when `src/agent-doc` is built again.

### Re: dispatched: Dashboard (#gvqv), #halftypedcoin; release queued — opus-5-5 · 2026-10-04T13:55-04:00

Three subagents are running, each in its own worktree based on v0.35.451:
- **`#editoractionmenu` (About Agent Doc):** `agent-doc-wt/editoractionmenu`.
- **`#gvqv` (Dashboard):** `agent-doc-wt/dashboardaction`. I saw you rewrite the `#gvqv` stub into the full request, so the free-text Dashboard head and `#gvqv` now go to this one subagent. It will:
  - add a "Dashboard" action, menu item and command-palette entry to the editor plugins;
  - combine `agent-doc board` (where the work is) with controller and supervisor liveness;
  - generate a live-updating markdown file under `.agent-doc/`, written atomically and kept fresh by the binary, that is never scanned as a session document and that the action opens;
  - add a matching `agent-doc dashboard` command.
- **`#halftypedcoin`:** `agent-doc-wt/halftypedcoin`. Turning a free-text queue line into a backlog item will wait until the operator stops typing, will never act on an obviously unfinished line like `Add a ```, and will fold later typing into the item it created.

**release + publish** stays with me. Once all three branches report back, I'll merge them one at a time (the two plugin branches will need their version bumps and parity tables reconciled), run `make check` and one `make install`, release v0.35.452, and close the items.
<!-- agent:boundary:da65c2e1 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#subagents #spec-test-build-install-commit-push" priority go -->
- do [#editoractionmenu]
- do [#gvqv]
- do [#halftypedcoin]
- do [#gvqv]
- release + publish
<!-- /agent:queue -->


<!-- agent:backlog priority -->
- [ ] [#halftypedcoin] [agent-doc] A half-typed free-text queue line was coined into a backlog id before the operator finished typing. Seen 2026-10-04: the operator typed '- Add a ``' (opening the Dashboard request), and preflight converted it into backlog item gvqv ('Add a ``') and queue head 'do [gvqv]'. The full line ('Add a `Dashboard` editor prompt + action ...') was still being typed underneath. Fix: free-text-to-id conversion must respect the steering typing gate (hold a line that is still changing, or one that ends inside an unclosed code span or mid-word), and a coined item whose source line is later extended should be merged into the extended line, not left as a stub.
- [ ] [#editoractionmenu] Add an `About Agent Doc` Editor action + menu item to show which version of Agent Doc + Plugin is running?
- [ ] [#gvqv] Add a `Dashboard` editor prompt + action to show the Agent Doc dashboard. Can we also support a dashboard md file that live updates?
- [ ] [#ghbrew] [agent-doc] Homebrew formula for agent-doc (GH #31, https://github.com/btakita/agent-doc/issues/31). The tap repo ~/work/btakita/homebrew-tap holds only chromium-bridge.rb today. Add Formula/agent-doc.rb pulling the per-platform GitHub Release archives (binary + cdylib together, GH #52) with SHA256SUMS, and add a release step that bumps the formula on each tag. Close #31 when brew install btakita/tap/agent-doc works.
- [ ] [#ghjbfolding] [agent-doc][jetbrains] JetBrains plugin UX phases 5-7 (GH #19, https://github.com/btakita/agent-doc/issues/19): fold agent:* component regions, add gutter icons on component markers, and a structure view of components and
- [ ] [#ghmultitmux] [agent-doc] One editor managing panes across several tmux sessions (GH #17, https://github.com/btakita/agent-doc/issues/17), e.g. a session per topic or project: an allowed-sessions list in config, with routing and sync choosing among them. Close #17 when shipped.
- [/] [#apimdrevert] [agent-doc][operator-verify] src/haiven-dev/tasks/api.md 'reverted by queue updates' (2026-09-29): forensics found NO lost operator text — disk/controller/editor converged at c9273a0b (HEAD 1b432940 + Codex 00:49:49 queue stop+strike, applied as a 2038-byte CRDT delta against the converged base). Only editor-side anomaly: idea.1.log 00:40:02
6local-splice-batch 3 splices 34346->34137 chars, then 00:40:03 exact inverse back to 25509a1e (undo/redo or plugin-internal flip; no VFS reload/projection logged). Blocked on operator: which text was lost + when (JetBrains Local History on api.md). Then trace the dropping path and fix with regression coverage.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#qsubattr5] [agent-doc] Queue subagents attribute, phase 5 (plan: tasks/agent-doc/plan-queue-subagents-attribute.md): dogfood in tasks/software/tsift.md (replace per-line #subagent: prefixes with subagents=3); verify via queue claims + ops.log that each head is dispatched exactly once and none drain inline.
- [/] [#steergatemodelrerun] [agent-doc] Rerun the steergatenamo model evaluation on real pause points. Gate: at least 200 labelled steering_gate_log rows with a text_tail (0 as of 2026-10-03). Then run src/agent-doc/scripts/steergate-model-eval --dataset from the project root, which exits insufficient_data until the floor is met. Integrate turnsense locally (ONNX via ort, lazily downloaded, opt-in) only if rules+perceptron+turnsense beats rules+perceptron on held-out log-loss; if close but short, fine-tune on the logged rows. Proxy results and decision: tasks/agent-doc/eval-steergatenamo.md
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
