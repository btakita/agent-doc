---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: c47dae65-e36f-40cb-8ebb-b7c9573e3c41
  codex: 01a11229-37c1-78e2-9044-2a7ef441955c
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

Released agent-doc v0.35.460; fixed and closed GH #139–#143; restored this document pane; cleared backlog cruft; captured two release-cycle follow-ups.
<!-- /agent:status -->
 

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261007-061259.md`*

Compacted content:
- Archived 14 response topic(s): GH #148, GH #149, releases, and release speed; release fast path; continuation; 11 more
- Prior summary/context: prior compacted content: Archived 42 response topic(s): do #steerbaselineabsorb; preflight refused on its own repair ...

### Re: release and delegated fixes — gpt-6.1-sol · 2026-10-07T05:06:55-04:00

> **Queue prompt:**
>
> Run Agent Doc on this file; then interrupt the Codex turn; agent-doc should detect if the turn is interrupted. I ran Compact Exchange and got an error. Compact Exchange should work even if a turn is running/interrupted. Run Agent Doc again caused the enqueued Compact Exchange to run.
>
> #gh-fix https://github.com/btakita/agent-doc/issues/166
>
> #gh-fix https://github.com/btakita/agent-doc/issues/169
>
> #gh-fix https://github.com/btakita/agent-doc/issues/170
>
> #gh-fix https://github.com/btakita/agent-doc/issues/171


> **Chat prompt (#chatprompt):** In a subagent: Fix agent-doc background processes thrashing

> **Chat prompt (#chatprompt):** also fix in subagent: api.md session crashed

> **Chat prompt (#chatprompt):** fix in subagent: ~/work/btakita/agent-loop/src/haiven-dev docs/fpe-ser…emporal-plan ⇡802 +1 !7                                 04:02:15
>
> ❯ /home/brian/.cargo/bin/agent-doc start --route-owned --route-owned-reap-policy auto --resume=01a0eaeb-c65f-7241-adde-93f0934ccc02 -- /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/api.md
> Error: Unterminated frontmatter block

> **Queue prompt:** release + publish

Completed the delegated bug batch and published it in two patch releases:

- `v0.35.472` contains the Compact/interruption race fix, background-process thrash fix, and GH #166, #169, #170, and #171. The issues are closed and the release was installed locally.
- `v0.35.473` contains the `api.md` crash fix. Source and tag are pushed at `2b6c74ebb`; the generated JetBrains `0.2.503` install revision is recorded on `main` at `5d3187301`.

The fixes are:

- **Run/interruption + Compact Exchange:** a document advance during drain now refreshes the durable op-capture epoch, so Compact Exchange defers through the active-typing window instead of treating partial input as quiescent. The interrupted/rerun flow no longer strands the queued compact operation.
- **Background-process thrashing:** recycle attempts are monotonic per session, causal recycle markers are validated, and capture-resume occurs only for proven binary staleness. Repaired supervisors/controllers settle instead of repeatedly recycling.
- **GH #166:** fresh-controller focus escalation now requires positive live tmux layout evidence.
- **GH #169:** warm supervisors refresh the incremental liveness journal, distinguish no-route from dead-route state, and use bounded recovery.
- **GH #170:** optional `data: null` controller envelopes preserve the typed outer-success/inner-null result.
- **GH #171:** concurrent `Preparing` successors are protected by exact identity, and failed handoffs retain observable exit/signal evidence.
- **`api.md` crash:** route-owned hot re-exec was parsing malformed editor-authoritative text containing a literal `9---` closing fence even though disk was already valid. Startup now validates the live authority before exec, retries transient invalid authority at a bounded cadence, propagates cold-start failure synchronously, and repairs only a proven one-character-prefixed closing fence after both the YAML and repaired full document validate. The live `api.md` session converged to its already-valid disk content; the document itself was not edited by the worker.

Validation and installation completed:

- Full `make check` passed, including 9,615 Rust tests, editor suites, and formal-model checks.
- `make tmux-ci` passed.
- Post-install editor parity passed: 193 VS Code tests plus the native cross-editor simulation.
- `make install-full` installed CLI/library `0.35.473`, JetBrains `0.2.503`, and VS Code `0.2.80`. Runtime processes were marked for safe idle-boundary handoff; none were forcibly recycled.

The one-time remote observation found CI and the `v0.35.473` Release workflow still in progress; the GitHub release object was not yet available. Per release policy, I did not poll or wait for those asynchronous jobs.
> **Chat prompt (#chatprompt):** also fix in subagent: • Blocked by hook
>   └ agent-doc Stop hook intercepted an unfinished document cycle for /home/brian/work/btakita/agent-loop/src/haiven-
> dev/tasks/sdk.md. [session-check] INTERRUPTED: document still contains completed tracked item(s) after closeout:
> #sdkjira1361. Re-run preflight/repair so the reap is persisted through the snapshot + commit boundary The latest
> assistant text was captured into the pending/capture ledger before auto-close. The hook replayed the response through
> the normal write path. The hook wrote or recovered the response but could not finish the required commit boundary:
> [session-check] INTERRUPTED: free-text agent:queue head(s) "do https://haiven-inc.atlassian.net/browse/HAVN-1489" were
> seen at preflight but have no committed response/echo or explicit deferral proof in the closeout — the prompt may have
> been silently lost
>     [session-check] hint: either respond to the unresolved head(s) and run `agent-doc finalize /home/brian/work/
> btakita/agent-loop/src/haiven-dev/tasks/sdk.md`, or add `<!-- no-free-text-queue-head-guard -->` if the removal was
> intentional (see #lr-queue-patchback-miss). Do not send the final answer yet. If the response is missing from the
> document, run `agent-doc repair /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk.md` first. Then finish the
> commit boundary for this turn and end with `agent-doc session-check /home/brian/work/btakita/agent-loop/src/haiven-dev/
> tasks/sdk.md`.

> **Chat prompt (#chatprompt):** also fix in subagent: • Blocked by hook
>   └ agent-doc Stop hook kept an active `agent:queue auto` moving for /home/brian/work/btakita/agent-loop/src/haiven-
> dev/tasks/sdk.md. The next queue prompt is "do https://haiven-inc.atlassian.net/browse/HAVN-1490". Continue THIS turn
> in-pane: answer that prompt in /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk.md and persist with `agent-
> doc finalize /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk.md` (or `agent-doc write --commit /home/
> brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk.md`). Do NOT run `agent-doc /home/brian/work/btakita/agent-loop/
> src/haiven-dev/tasks/sdk.md` from this pane — that re-invokes the owner pane and hits the recursive-direct-invocation
> deadlock guard, and do not send the final answer yet.

### Re: GH #172, GH #173, SDK Stop-hook recovery, and release — gpt-6.1-sol · 2026-10-07T07:07:25-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/172

Fixed and closed. A top-level `**Deferred:**` response now vetoes automatic queue consumption consistently in both queue-consume layers. Predicate coverage and a real finalize regression prove the head remains queued instead of being silently struck.

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/173

Fixed and closed. Attached mid-turn steering now reads CRDT/editor authority instead of stale disk state, applies authority-aware debounce, and content-hashes explicit sends. This prevents a live queue head from being falsely reported removed and then re-added.

The requested SDK Stop-hook incident was also repaired. Completed tracked residue is now a typed interruption: Stop performs terminal maintenance/reaping before dispatching the next fresh queue head, and repeated-head recovery cannot replay stale assistant text as evidence for a different head. The owner pane supplied the real HAVN-1489 and HAVN-1490 responses; the final SDK `session-check` reports committed cycle `cycle-1791367513679`, no continuation required, and only three operator-verification heads deferred.

During integration, the clean baseline exposed a deterministic delayed-replica preflight failure. The bounded observer loop now waits through the configured registration window while preserving definitive refusal behavior. Focused regressions and the affected package suites pass.

> **Queue prompt:** release + publish

Released `v0.35.474` from commit `397b6a6e54bc493513cb0d01316e19b4ce7943ac`. The tag and `main` are pushed, the installed CLI reports `agent-doc 0.35.474`, the native library and editor packages were installed, and both GitHub issues were closed with release notes. The final version-projected `make check` and release-owned tmux CI gate passed. The one-shot GitHub Actions inspection found CI run 37611054867 in progress for the release commit; the GitHub Release object was not yet present, so it was not polled. The release parity audit records all four changes.

The implementation was isolated in subagent worktrees, reviewed and integrated serially into one release tree. `tsift` bounded navigation and test-digest evidence, while the agent-doc workflow enforced claim ownership, the single response write, and the commit/session-check boundary.

### Re: recovered response

Completed and released `agent-doc v0.35.474` at commit `397b6a6e5`.

- Fixed GitHub issues #172 and #173 and closed both.
- Fixed the SDK Stop-hook replay/queue bug for HAVN-1489/HAVN-1490.
- Fixed delayed editor-replica preflight handling.
- Full `make check` and tmux CI passed.
- CLI, native library, JetBrains, and VS Code integrations installed.
- SDK and primary documents both pass `agent-doc session-check`.
- Release CI was in progress during the single inspection; the GitHub Release object was not yet available.

The agent-doc workflow enforced isolated subagent worktrees and committed closeout; tsift provided bounded navigation and test evidence. See [agent-doc-bugs.md](/home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md) and [sdk.md](/home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk.md).
### Re: recovered response

Yes. The current declaration is correct:

```md
<!-- agent:queue subagents preset="#subagents #spec-test-build-install-commit-push" priority go -->
```

Bare `subagents` now means “dispatch every currently eligible, dependency-ready, unclaimed head,” scaling up to the runtime’s concurrency limit. Use `subagents=N` only when you want a fixed cap.

The `#subagents` entry inside `preset` is redundant—the attribute controls dispatch—but it is harmless. For clearer separation, you could simplify it to:

```md
<!-- agent:queue subagents preset="#spec-test-build-install-commit-push" priority go -->
```
### Re: subagent scaling, queue recovery, editor layout fixes, and v0.35.475 — gpt-6.1-sol · 2026-10-07T14:59:33-04:00

> **Chat prompt (#chatprompt):** cont

> **Chat prompt (#chatprompt):** subagent fix: › agent-doc /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/devops.md
>
>
> • Preflight refused this turn because the document’s agent:done archive points to tasks/devops.md instead of a .done.md
>   file. I won’t start a response or edit the document; I’m running the permitted session check now to surface any
>   pending operator prompt verbatim.
>
> • Failed (exit 1) agent-doc session-check /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/devops.md
>   └ [perf] session_check.operations file=/home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/devops.md total_ms=3…
>     [session-check] INTERRUPTED: cycle `cycle-1791316137364` is `committed` (commit_success), but the active harness s…
>     + 2 lines (ctrl+t to expand)
>
> • Preflight admission failed; no response cycle was started and the document was not edited.
>
>   Reason: agent:done archive=tasks/devops.md must point to a .done.md file.
>
>   session-check reported this pending operator prompt verbatim:
>
>   > agent-doc /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/devops.md
>
>   Worked for 21s • 2:44 AM

> **Chat prompt (#chatprompt):** never mind...user error

> **Chat prompt (#chatprompt):** However, I fixed the archive, then Run Agent Doc...and the prompt did not submit. I manually	submitted the prompt.

> **Chat prompt (#chatprompt):** subagent: Allow the operator to delete backlog items: › agent-doc /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/fpe.md
>
>
> • The turn is still refused for the same missing backlog item, #setuppersonalslack. I’ll run the permitted session
>   check once more and report its exact result without modifying the document.
>
> • Failed (exit 1) agent-doc session-check /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/fpe.md
>   └ [perf] session_check.operations file=/home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/fpe.md total_ms=4381…
>     [session-check] INTERRUPTED: open backlog item(s) from recent history are completely absent from the project: #set…
>     + 2 lines (ctrl+t to expand)
>
> • Agent-doc remains blocked because #setuppersonalslack is absent from the live backlog. Session-check found no
>   additional pending operator prompt.
>
>   Restore, relocate, or mark that item done before retrying. No files were changed.
>
>   Worked for 37s • 12:23 PM

> **Chat prompt (#chatprompt):** also fix in subagent: auto-sync to agent-doc-bugs.md cause a 1 pane layout. Sync Tmux Pane did recover the 2 pane layout.

> **Chat prompt (#chatprompt):** also fix Run Agent Doc on agent-doc-bugs.md while this turn was running: dispatch-only codex reopen refused to inject into pane %130 for /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md because the pane still shows queued draft in composer; restore an idle prompt and retry
> the route-owned host supervisor (pid 870535) serving this document is mapping a STALE agent-doc binary while a newer build is installed, so it can keep producing File Cache Conflict / IPC-drift dialogs (#fcc0/#ipcdrift). Refresh it without discarding the live turn: `agent-doc admin recycle` (recycles at the next idle boundary) or `agent-doc session restart-supervisor <FILE>` (refuses busy panes).

> **Chat prompt (#chatprompt):** It should have been "- I'm skeptical about the reported 63.2% Not Flagged rate."

> **Chat prompt (#chatprompt):** Can we use more subagents?

> **Chat prompt (#chatprompt):** Is the current usage of the `subagents` `agent:queue` attribute correct?

> **Chat prompt (#chatprompt):** Is the `#subagents` preset redundant now?

> **Chat prompt (#chatprompt):** In a subagent: fix: • Blocked by hook
>   └ agent-doc Stop hook intercepted an unfinished document cycle for /home/brian/work/btakita/agent-loop/tasks/agent-
> doc/agent-doc-bugs.md. [session-check] INTERRUPTED: cycle `cycle-1791393792054` is still `preflight_started`
> (preflight_started) — cycle started but no write/commit followed. The latest assistant text was captured into the
> pending/capture ledger before auto-close. The hook could not finish the required commit boundary: [lint-gate]
> INTERRUPTED: 1 blocking lint finding(s) for /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md
> (mode=warn, source=default). Fix the directives below before re-running `agent-doc finalize` / `agent-doc write
> --commit`, or set `agent_doc_lint_dialect: off` in frontmatter / `[lint] dialect = "off"` in `.agent-doc/config.toml` to
> temporarily skip this gate. /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md:174:1 error: attribute
> `subagents` on `agent:queue` is missing `=value` [agent-doc/malformed-attr]   hint: try `subagents=<value>` . The
> retained capture or projection is already durable and the same intent commits itself once delivery converges — this is a
> deferral, not a lost response. Run `agent-doc session-check /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-
> doc-bugs.md` once to observe the terminal state; do NOT re-send the response, force disk, or `admin recycle`, which
> disturb the capture being awaited. Do not reach for `admin reload-lib` on your own either: `admin reload-lib` is the
> sanctioned recovery only when `agent-doc session-check` reports that the editor holding the document is not serving its
> replica, and then its remedy governs. Do not send the final answer yet. If the response is missing from the document,
> run `agent-doc repair /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md` first. Then finish the
> commit boundary for this turn and end with `agent-doc session-check /home/brian/work/btakita/agent-loop/tasks/agent-doc/
> agent-doc-bugs.md`.

> **Queue prompt:** ⏭️ do [#subagentsattributescale]

Implemented. Bare `subagents` now means all eligible, dependency-ready, unclaimed queue heads, while `subagents=N` provides a fixed cap. The active Codex environment permits four agents total, including the root, so it can run three worker agents concurrently; additional eligible work dispatches as slots become free. `fan-out` remains the bare alias.

The declaration now in this document is correct:

```md
<!-- agent:queue subagents preset="#spec-test-build-install-commit-push" priority go -->
```

The `#subagents` preset is redundant now because the typed queue attribute owns scheduling. Removing it from the preset list was correct. The earlier recovered response stated the intended runtime semantics but exposed a real lint mismatch: the installed dialect validator rejected bare `subagents` even though the queue parser accepted it. The lint adapter now shares the queue-domain interpretation, so bare `subagents` and `fan-out` pass while malformed bare attributes such as `preset` still fail.

> **Queue prompt:** The last 2 responses in sdk.md have been unescaped.

Fixed. Strict closeout/replay now commits the canonical escaped response through the private index and recognizes the safe pending repair after restart, without overwriting newer editor authority or a concurrent disk cut. This also repairs the retained Stop-hook cycle that was blocked by the false bare-attribute lint error.

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/174

Fixed on `main` in `85ecc680e` and closed. Half-rekeyed rename identities are tolerated and repaired by matching current document identity, while genuine copy conflicts retain a typed terminal refusal instead of retrying 132 times.

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/175

Fixed on `main` in `d1cc2491b` and closed. Remote Dev capture now separates visible structural evidence from selected focus evidence, preventing hidden tabs from widening a two-pane or one-pane tmux layout.

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/176

Fixed on `main` in `7bdd2a0fc` and `32fa4b3a8`, then closed. Plugin packaging now enforces a tracked-plus-untracked source digest fence, and the JetBrains generation advanced to `0.2.504`, preventing changed sources from shipping under an already-released plugin version.

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/177

Fixed on `main` in `b013de126` and closed. JetBrains evaluates Remote Dev snapshot/authority before backend-window readiness, so tab-switch capture no longer waits for `wins=1`; deferrals now include an INFO reason.

The other reported defects are fixed on `main` as well:

- Run Agent Doc submit verification now keeps the pass-through window open long enough to distinguish a real empty composer from a delayed submission (`f3ff1b4fa`).
- Explicit operator backlog deletion is accepted when proven by editor operations or an idle cut with committed history; unexplained disappearance still blocks (`a8a3ec148`, `b72ab679e`).
- Safe passive auto-sync preserves the current layout when any requested column is stale, preventing the observed two-pane-to-one-pane collapse; an explicit full sync may still realize the valid remainder (`3601a7b03`).
- Queued same-route reopen is coalesced after supervisor refresh, foreign drafts still refuse injection, and an obsolete stale-supervisor warning is suppressed once the recycle settles (`03db286e6`). No forced recycle disturbed this live turn.

The corrected document wording is exactly:

- I'm skeptical about the reported 63.2% Not Flagged rate.

> **Queue prompt:** release + publish

Published `agent-doc v0.35.475` from `f374e3f7d`: GitHub Release assets, PyPI, and release CI are green, and the release was installed locally. The subsequent fixes above are pushed to public `main` through `3158f3c80`; they are installed locally but intentionally are not represented as a second tag or release.

Verification and installation are complete:

- The integrated `make check` passed with no failures (`tart-6ef16a123b`), including the cross-editor simulation.
- `make install-full` installed CLI/native `0.35.475`, JetBrains `0.2.504`, and VS Code `0.2.80`. The active document endpoint deferred native reload to the safe idle boundary, preserving this turn.
- Book and Pages checks for `3158f3c80` are green; the main CI run was still in progress at the required one-shot observation.
- All queue claims were released before this response.
❯ Completed and committed the agent-doc cycle.

- Bare `subagents` usage is correct; `#subagents` preset is redundant.
- Current capacity is three worker subagents plus the root agent.
- Published and installed `v0.35.475`.
- Pushed subsequent fixes through `3158f3c80`.
- Closed GitHub issues #174–#177.
- Full `make check` passed; full install succeeded.
- Preserved the corrected sentence: “I'm skeptical about the reported 63.2% Not Flagged rate.”
- Terminal `session-check` passed for cycle `cycle-1791395961104`; supervisor continuation requires no operator action.

❯ The check emitted only a non-blocking coined-ID warning from verbatim historical prompt text.

### Re: late selected preset closeout and GH #177 — gpt-6.1-sol · 2026-10-07T15:58:03-04:00

> **Chat prompt (#chatprompt):** subagent fix: • Blocked by hook
>   └ agent-doc Stop hook opened a fresh cycle for post-commit prompt work in /home/brian/work/btakita/agent-loop/tasks/
> agent-doc/agent-doc-bugs.md, then retained the response under that cycle but could not finish closeout. The latest
> assistant text was captured into the pending/capture ledger before auto-close. The hook could not finish the required
> commit boundary: [finalize] pre-write gate: selected prompt-preset queue head lacks response evidence: #gh-fix https://
> github.com/btakita/agent-doc/issues/177. Address every resolved preset expansion in the response; a literal `#preset`
> queue-head quote is optional. No response has been captured.. The retained capture or projection is already durable and
> the same intent commits itself once delivery converges — this is a deferral, not a lost response. Run `agent-doc
> session-check /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md` once to observe the terminal state;
> do NOT re-send the response, force disk, or `admin recycle`, which disturb the capture being awaited. Do not reach for
> `admin reload-lib` on your own either: `admin reload-lib` is the sanctioned recovery only when `agent-doc session-check`
> reports that the editor holding the document is not serving its replica, and then its remedy governs. Do not recapture
> the response or rerun finalize/write. Run `agent-doc session-check /home/brian/work/btakita/agent-loop/tasks/agent-doc/
> agent-doc-bugs.md` and let the binary-owned keyed retry finish.
>
> • The agent-doc Stop hook retained the response and explicitly forbids recapture or another write. I’m running the one
>   permitted session-check so the binary-owned keyed retry can finish the commit boundary.
>
> • Failed (exit 1) agent-doc session-check /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md
>   └ [perf] session_check.operations file=/home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md total_ms…
>     [session-check] INTERRUPTED: cycle `cycle-1791399707092` is still `response_captured` (response_captured) — respons…
>     + 2 lines (ctrl+t to expand)

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/177

Fixed and closed. The underlying Remote Dev tab-capture defect remains fixed on public `main` in `b013de126`: JetBrains evaluates remote snapshot and authority before backend-window readiness, so tab switches no longer wait for `wins=1`, and capture deferrals log an INFO reason.

The subagent also fixed the closeout defect that reopened this already-answered head. Root cause: #177 entered the live queue after the pre-turn baseline with the selected `🚧` marker. The response contained the exact queue prompt and answer, but the baseline fence rejected every late head, leaving #177 active; the next Stop hook then opened a fresh cycle whose short terminal summary lacked current-cycle preset evidence.

The corrected invariant is now implemented on public `main`:

- `7aa890a00` atomically consumes a baseline-late head only when it is already selected/in-progress **and** the current response contains exact answer evidence.
- A marker alone, preset-expansion prose alone, or a concurrent unselected operator draft remains insufficient, so newly typed work is still protected.
- `6f8c29fa7` aligns the cross-crate queue-IO contract test.
- `8d8b7480a` synchronizes the updated respond runbook across all bundled harness surfaces.

Verification and delivery are complete:

- The final `make check` passed with no failures (`tart-dab74fac69`), including 9,641 tests, closeout/session simulations, editor parity, and TLA+.
- The commits are pushed through `8d8b7480a` and installed locally as CLI/native `0.35.475`; the live editor endpoint deferred its native handoff until this safe cycle boundary.
- Book and Pages checks are green; CI was still in progress at the required one-shot observation.

### Re: release and publish status — gpt-6.1-sol · 2026-10-07T16:02:18-04:00

> **Chat prompt (#chatprompt):** have we released + published?

Yes for `v0.35.475`: it was published as a non-draft, non-prerelease GitHub Release on 2026-10-07 with the native archives, editor packages, and checksum assets, and PyPI currently reports `agent-doc 0.35.475` uploaded moments later.

However, that release tag resolves to `f374e3f7d`. The subsequent fixes—including GH #174–#177, the layout/reopen/backlog changes, bare-subagent lint/replay repair, and the late selected preset closeout fix—are pushed to public `main` through `8d8b7480a` and installed locally, but have **not** been published in a newer tagged release or PyPI version.
<!-- agent:boundary:d2a9ba1d -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built
<!-- agent:queue subagents preset="#spec-test-build-install-commit-push" priority go -->
<!-- /agent:queue -->


<!-- agent:backlog priority -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#drivecapturedsplice] [operator-verify] Captured-splice stale-replica live proof (plan: tasks/agent-doc/plan-captured-editor-splice-recovery.md). Recipe and regression test shipped in v0.35.466; live proof remains gated because the corrected run reached owner pane %134 but stopped before the retained-delivery marker, edit two, and verifier completion. Rerun the recipe through verify-captured-splice-recovery and require it to pass.
- [/] [#qsubattr5] [agent-doc] Queue subagents attribute, phase 5 (plan: tasks/agent-doc/plan-queue-subagents-attribute.md). SETUP COMPLETE: tasks/software/tsift.md now has subagents=3 on branch qsubattr5-dogfood commit de5a8aa7c0d5e879547d4e321d159b1eff840073; gated pending the next real operator-created multi-head batch proof from distinct queue claims and ops.log that every eligible head dispatches exactly once and none drain inline.
- [/] [#steergatemodelrerun] [agent-doc] Rerun the steergatenamo model evaluation on real pause points. Gate: at least 200 labelled steering_gate_log rows with a text_tail (66 as of 2026-10-06); src/agent-doc/scripts/steergate-model-eval --dataset currently exits insufficient_data. Then integrate turnsense locally (ONNX via ort, lazily downloaded, opt-in) only if rules+perceptron+turnsense beats rules+perceptron on held-out log-loss; if close but short, fine-tune on the logged rows. Proxy results and decision: tasks/agent-doc/eval-steergatenamo.md
- [/] [#pypiquotarecheck] Track pypi/support#12214 (agent-doc project limit request, 25 GiB) to a decision. Unblocked mechanically by a human reply on pypi/support#12214, or make pypi-quota-check reporting a ceiling in reach. Rechecked 2026-10-06: 8.363 GiB of 10.0 GiB, 1.637 GiB headroom, 198 releases through 0.35.465, post-cutover mean 35.3 KiB, approximately 48,621 further releases fit; VERDICT headroom fine. The support issue remains open with only the github-actions bot comment and has not changed since 2026-09-10. Nothing to act on or delete. If the grant lands, record it and drop this item; deletion stays reserved for a ceiling the authenticated check reports.
- [/] [#grokmcpverify] [operator-verify] Verify an authenticated Grok Build MCP admit/read/plan/finalize cycle and same-turn auto-queue continuation. NEW BLOCKER 2026-09-30: grok 1.0.41 is now SIGNED OUT ('grok -p' -> 'Error: Not signed in'); operator runs 'grok login --device-code' (or sets XAI_API_KEY), and the free usage quota or the planned subscription must be active. Grok CLI authorization, native JSON/backend commit fixtures, pane detection and quit behavior passed earlier; the last live MCP probe stopped at the usage limit before committed closeout. Heavy is not required. Plan: tasks/agent-doc/plan-grok-build-support.md.
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#ghrecvpolicy] [agent-doc] Recipient-side [receive] auto_create_components policy for multi-user workspaces (GH #40, https://github.com/btakita/agent-doc/issues/40). Parked until multi-user workspace support exists.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
