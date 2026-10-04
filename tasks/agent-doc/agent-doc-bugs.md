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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261004-034126.md`*

Compacted content:
- Archived 33 response topic(s): GH #106, GH #107, and release v0.35.441; GH #108: JetBrains upgrade reporting; release + publish (v0.35.442); 30 more
- Prior summary/context: prior compacted content: Archived 4 response topic(s): GH #99, #100, #101 and release; GH #102; GH #104–105, queue fi...

### Re: release + publish — opus-5.5 · 2026-10-04T00:03-04:00

> **Queue prompt:** release + publish

**v0.35.447 is tagged and pushed; installed locally.**

- A subagent prepared the release in a clean worktree (`~/work/btakita/agent-doc-worktrees/release-0.35.447`, branched from `1064bf1f8`), because the shared `src/agent-doc` checkout holds another session's uncommitted `VERSIONS.md` and `agent-doc-start-runtime-io/src/lib.rs` edits.
- The version was projected with `scripts/agent-doc-dev release-version 0.35.447` across 154 files: every crate `Cargo.toml`, `Cargo.lock`, `pyproject.toml`, the skill headers and `editors/release-parity.tsv`.
- `VERSIONS.md`: `## Unreleased` became `## 0.35.447`. The subagent added entries for the 7 user-facing commits that section was missing (GH #120, #122, #124 ask 5, and others).
- `make check` passed with exit 0: nextest ran 10755 tests, all passed, 262 skipped; clippy, version-sync, editor-parity, lean and tla were green.
- Commit `63b514852` (`chore: release v0.35.447`) was pushed to `origin/main` as a fast-forward from `1064bf1f8`, and tag `v0.35.447` was pushed.
- `make install-full` ran from the worktree (exit 0). `agent-doc --version` reports `0.35.447`; the cdylib and the JetBrains and VS Code plugins were reinstalled.
- CI started from the tag push: Release, PyPI and CI were in progress when last checked, and Book was green. I did not wait for them to finish.

**Needs attention:**
- The shared `src/agent-doc` checkout is still on `1064bf1f8`. I did not fast-forward it, because the other session's uncommitted `VERSIONS.md` edit would collide. That edit looks damaged: it deletes the whole `## 0.35.446` section and appends a few lines at the end of the file. It needs a rebase onto `origin/main` before that session commits.
- To clean up afterwards: remove the worktree and branch `release-0.35.447`, plus the `~/work/btakita/agent-doc-worktrees/tmux-router` symlink the subagent created so the worktree could build.

### Re: release cleanup — opus-5.5 · 2026-10-04T00:12-04:00

> **Queue prompt:** Do the cleanup that you suggested.

The release cleanup is finished:

- **Worktree:** `~/work/btakita/agent-doc-worktrees/release-0.35.447` was removed. Before removing it, the subagent checked that it had no uncommitted changes and that its HEAD, `63b514852`, is already on `origin/main`.
- **Branch:** `release-0.35.447` was deleted. Plain `-d` refused because the shared checkout's local `main` is still at `1064bf1f8`, so the subagent used `-D`. That loses nothing, since the commit is on `origin/main`.
- **Symlink and folder:** the `tmux-router` symlink was removed as a link, without following it. The now-empty `agent-doc-worktrees/` directory was removed too.
- **Untouched:** the remote tag `v0.35.447`, `origin/main`, and the shared `src/agent-doc` checkout.

The shared checkout still needs the other session to rebase its uncommitted edits onto `origin/main`, as noted in my previous response.

### Re: queue dispatch — #gh-fix GH #125, #apimdrevert — opus-5-5 · 2026-10-04T00:32-04:00

Both queue heads were claimed and handed to background subagents (`#subagents` preset):

- **#gh-fix GH #125** (chat-originated prompts never get recorded in the session document): the subagent works in its own worktree, `~/work/btakita/agent-doc-wt-ghfix125` on branch `ghfix125`, which sits outside agent-loop. It owns the SKILL/runbook rule and, if tractable, a preflight detection signal, plus tests and `make check`. I'll integrate its work, install, close #125 and release in the next cycle.
- **[#apimdrevert] "how can I verify?"**: a read-only subagent is putting together a concrete procedure. It covers JetBrains Local History on `api.md` around 2026-09-29 00:40/00:49, the git history of `src/haiven-dev`, and a grep of `idea.1.log`. The answer will land here next cycle.

### Re: [#apimdrevert] how to verify — opus-5-5 · 2026-10-04T00:36-04:00

> **Queue prompt:** [#apimdrevert]: how can I verify?

> **Operator note:** how can I verify?

**Use JetBrains Local History first; it may be the only record left.** The `idea*.log` files have already rotated past 2026-09-29: the oldest, `idea.12.log`, starts on 10-02. Local History is still being written to `~/.cache/JetBrains/IntelliJIdea2026.1/LocalHistory/`. Its default retention is about 5 working days, so 09-29 may be close to being purged.

1. **IDE:** right-click `src/haiven-dev/tasks/api.md` and choose Local History › Show History. Compare these revisions:
   - **00:34** (git commit `1b432940`) against **00:40:02**. This shows the ~209 characters removed by the splice (34346 → 34137 chars).
   - **00:40:02** against **00:40:03**. If the second change puts back exactly those characters, it was an undo/redo flip, not a loss.
   - Just before **00:49:49** against just after it. Codex's change should touch only the queue stop and strike-through.
   - Your newest revision with typing before 00:49 against **02:12**. Any text of yours missing here is the real loss.
2. **Git:** `c9273a0b` is not a git object. It is agent-doc's content hash, stored in `src/haiven-dev/.agent-doc/state.db`. The commit range to check is `1b432940` (00:34:29) to `4f901866` (02:12:48):
   ```
   cd ~/work/btakita/agent-loop/src/haiven-dev
   git log --format='%h %ci' --since='2026-09-28 23:00' --until='2026-09-29 03:00' -- tasks/api.md
   git diff 1b432940 4f901866 -- tasks/api.md   # expect only queue stop/strike + later turns
   git show 1b432940:tasks/api.md > /tmp/api-0034.md
   ```
   - Missing text that is in `1b432940` but not in `4f901866` was dropped between 00:34 and 02:12.
   - Missing text that is in no commit at all was typed and lost before the next commit, and only Local History can show it.
3. **Next time:** right after an incident, run `grep -n '<date> <HH:MM>' ~/.cache/JetBrains/IntelliJIdea2026.1/log/idea*.log | grep -iE 'splice|api.md|reload|projection'` and copy the log aside. The logs rotate in about 2 days.

**Report back:**
- The lost snippet, verbatim.
- The last Local History timestamp where it is present and the first where it is gone.
- The label Local History shows on that boundary revision: an external change, or an edit made in the IDE.

If Local History no longer reaches 09-29, say so and I'll close `#apimdrevert` as unverifiable. It stays in the backlog, gated on your answer.
<!-- agent:boundary:d3a1b952 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#subagents #spec-test-build-install-commit-push" priority go -->
- #gh-fix https://github.com/btakita/agent-doc/issues/125
- ~~[#apimdrevert]: how can I verify?~~ — auto-struck: answered this cycle (#ftstrike)
<!-- /agent:queue -->


<!-- agent:backlog priority -->
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
