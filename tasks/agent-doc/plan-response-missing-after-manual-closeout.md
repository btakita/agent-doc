# Plan: Response Missing After Manual Closeout

## Problem

The Monster Rod Holders task document recorded the prompt:

```text
Please review stripe orders. Steve claims there were no Stripe orders since we took over the project in Mid April.
spec-test-commit-push
```

The work completed, including commits and pushes, but the assistant response was not written back into `src/boost-client/tasks/monsterrodholders.md`. The visible document ended with the user prompt inside the exchange block, so the session appeared unresolved even though the code/spec/test work was complete.

## Root Cause To Prove

The direct Codex path can complete substantial work, commit/push the site repo and parent repo, and still bypass the agent-doc response patchback/finalize contract. In this case, the parent repo was clean and pushed, but the response body never landed in the session document.

This may overlap with existing bugs:

- `#harnessprompt`: harness/native prompts can be actionable without a durable exchange response.
- `#sidecar-diverge`: successful finalize/commit can still fail to show the response in the editor.
- `#panehang`: panes can finish or idle without a reliable closeout lifecycle.

The distinct failure to preserve is: a successful `spec-test-commit-push` turn must not be allowed to end with only the prompt persisted in the exchange.

## Additional Evidence: Root Task Patchback Miss

On May 10, 2026, `src/session-share/tasks/root.md` recorded:

```text
Fix the test suite. The test suite must pass before completing the turn. Update the agent harness files to enforce this rule.
spec-test-commit-push
```

The work completed and was pushed (`BuildParty 33646bb`, `SessionShare 0223139e`), but the assistant closeout did not land in the document. The user had to ask for manual patchback repair. This is the same prompt-only exchange shape, but against the root session-share task document and after a nested submodule commit plus parent pointer/news commit.

## Additional Evidence: VT Agent Deploy Patchback Miss

On May 10, 2026, `src/session-share/tasks/root.md` recorded:

```text
do [#vt-agent-deploy]. spec-test-news-commit-push
```

The work completed and was pushed (`BuildParty af7b9b7`, `SessionShare ebc806c2`, parent `agent-loop` 8d972190b), but the final response block was not written into the exchange. The user had to request manual repair with: "The response patchback is not written into the document." This is another prompt-only exchange closeout after a nested submodule commit, session-share commit, and parent pointer commit all succeeded.

Implementation note: preserve this as a snapshot-equal regression, not only a diff regression. Once preflight has already committed the prompt into the snapshot/baseline, the post-work guard still has to scan the live `agent:exchange` tail and fail closed if the tail ends with `do [#vt-agent-deploy]...` and no later `### Re:` / `## Assistant` response.

## Additional Evidence: PMax Post-Brand Closeout Patchback Miss

On June 2, 2026, `src/boost-client/tasks/monsterrodholders.md` recorded:

```text
do [#pmaxpost]. #spec-test-commit-push
```

Codex completed the requested work, including the `monsterrodholders-dev`
implementation commit, the `boost-client` task-doc/submodule commit, and pushes
for both repos. The final chat response reported success, but no matching
`### Re: do [#pmaxpost]` response landed in the session document. The document
had the status/review updates and the original prompt, but the exchange still
lacked the actual assistant closeout until a later manual recovery ran
`agent-doc write --commit` with a closed `<!-- patch:exchange -->` block.

The first recovery attempt also exposed two guard details worth preserving in
tests:

1. `agent-doc preflight` initially blocked on an uncommitted parent submodule
   pointer for `src/boost-client`; `agent-doc commit <FILE>` had to finish that
   parent-pointer boundary before response write-back could proceed.
2. A malformed repair payload with an unclosed `<!-- patch:exchange -->` block
   failed closed, and the successful retry needed `--done pmaxpost` because the
   closeout body completed an existing pending item.

Regression coverage should prove that a direct-chat `#spec-test-commit-push`
turn cannot report success after commit/push unless the response heading is
present in the target document HEAD, and that the recovery diagnostic names the
binary-owned `agent-doc write --commit` path with a closed patch block.

## Fix Plan

1. Add a regression fixture for a direct Codex/agent-doc turn that:
   - starts with a prompt already written into an exchange boundary,
   - performs a commit/push-like closeout path,
   - exits without calling finalize or without mutating the exchange body.
2. Add a session-check invariant: if the newest exchange boundary contains a prompt and no assistant response after it, fail closed with a diagnostic naming the document and boundary id.
3. Add a closeout/finalize guard that runs after commit/push operations and before process exit, verifying the response body is present in the document working tree and committed state.
4. If commit/push completed but response patchback failed, provide a recovery command that appends the last assistant response to the target exchange boundary without rerunning the completed site work.
5. Cover the recovery path with a test that preserves submodule discipline: site commit remains unchanged, parent pointer remains unchanged unless the recovered task doc commit requires a parent update.

## Additional Evidence: Claude-skill console-only response + queue-consume divergence

On 2026-06-02, on `tasks/agent-doc/agent-doc-bugs2.md` (Claude Code skill path, not
Codex), the same class surfaced in two compounding shapes within one session:

1. **Console-only response, no persistence command run.** A turn produced a full
   status/hold response in the console and reported the turn complete, but never
   piped the body through `finalize` / `write --commit`. The document was unchanged;
   the response "did not exist." This is the Claude/skill analogue of the Codex
   direct-writeback miss — the agent simply concludes without persisting, so the
   existing Codex Stop-hook `committed_without_response_body` backstop never runs.

2. **Persistence attempted but queue-consume diverged under live IPC.** A separate
   `finalize --done <id>` cycle DID commit the `### Re:` body and reap the backlog
   item, but the final queue-consume step failed
   (`queue consume: snapshot queue state diverged from document queue`,
   invariant `live_prompt_drift_after_preflight`) while the user was live-typing new
   queue items (JB IPC listener active; a JB `File Cache Conflict` reverted the queue
   to an older snapshot). The next preflight then hard-blocked on the now-orphaned
   bare `[#id]` queue head (`open backlog item(s) exist only outside live
   agent:backlog`), so **subsequent turns could not reach `finalize` at all** — new
   responses had nowhere to land. `repair --apply-recovery` classified it
   `sidecar_visible_drift`, committed the lossy working-tree side (dropped an answered
   free-text queue line), and did not strike the consumed head; the head had to be
   struck manually.

## Additional Fix Plan (queue-consume / Claude-skill path)

6. Add a Claude/skill-path "response produced but never persisted" detection: a
   completed skill turn whose console response has no matching committed `### Re:`
   in HEAD should be detectable at next-preflight / session-check, mirroring the
   Codex `committed_without_response_body` guard.
7. Make queue-consume divergence head-aware and non-blocking for the next turn: on
   divergence, re-resolve the consumed head against the committed response and strike
   it (`[#id]` -> `~[#id]~`) through the binary-owned commit boundary; never commit a
   lossy working-tree side that drops sibling queue lines.
8. Add an `agent-doc repair` / `finalize` sub-path that strikes a consumed orphaned
   queue head when its referenced backlog item is already in `agent:done` / archived,
   clearing the orphaned-backlog guard without a manual edit, and update that guard's
   message to name the repair command.
9. Make consume/divergence recovery IPC-listener aware: when a live listener is active,
   push convergence (`converge_queue_auto`) before re-running consume; never require a
   raw `git checkout` / sed-edit of a live-typed buffer.

Relates to `#write-closeout-response-shape`, `#adoc-queue-ipc-buffer-divergence`,
`#duplicate-response-after-commit`, and the JB `File Cache Conflict` snapshot-revert
report queued alongside this item.

## Additional Evidence: MRH SERP closeout response loss + stalled queue

On 2026-06-22, `agent-doc /home/brian/work/btakita/agent-loop/src/boost-client/tasks/monsterrodholders.md`
was invoked from an owned Codex pane. The self-invocation guard correctly refused to
reroute and exposed the live queue head `#serpfx-frh-recover`; the agent then ran
that head manually in the pane.

The work completed and was committed:

- `src/boost-client/tasks/monsterrodholders.md` was updated with the 2026-06-22
  "fishing rod holders" GSC/SERP recovery findings, `#serpfx-frh-recover` was
  removed from the active queue, and the next queue head became
  `#convfix-automation-deploy`.
- The boost-client submodule reached commit `77c971c3`; the superproject pointer
  reached `c15d92bbc`.
- Local verification showed a clean boost-client worktree; `make check` was
  unavailable in that repo and `gh run list --workflow CI --limit 1` found no
  workflow named `CI`.

The closeout path did not preserve the assistant response:

1. `agent-doc finalize` / `agent-doc write --commit` initially returned empty-response
   failures.
2. A manually added `### Re:` exchange block caused session-check to complain about
   direct response patchback, so it was removed.
3. `agent-doc compact ... --commit` then committed a clean-looking cycle and archived
   the attempted exchange, but the live exchange tail became empty.
4. Final checks showed `agent-doc session-check` returning ok for
   `cycle-1782150007029` as `committed`, while also reporting
   `queue_continuation_required=true` and next queue prompt
   `#convfix-automation-deploy`.
5. `tsift --envelope session-review ... --next-context` reported
   `live_exchange_tail_total=0`, so the visible document contained no assistant
   closeout response even though the task state advanced.

A follow-up repair attempt reproduced the failure in a tighter form:
`agent-doc write --commit --origin codex-repair tasks/monsterrodholders.md` exited
successfully, but printed `rejecting late fallback patch: cycle ... already committed`,
did not append the response body, and reintroduced the stale queue body as
uncommitted drift. The queue had to be restored manually and committed as
boost-client `3969eaa` with parent pointer `1fcb39a13`. Regression coverage should
treat a zero-exit write with no visible response body as failure, and should verify
that missing-response repair cannot resurrect stale queue snapshots.

This variant differs from a prompt-only exchange: the response was generated in chat,
the document mutations and commits succeeded, and the queue state advanced, but the
response body was later compacted/normalized away. The closeout contract needs a
post-commit invariant that proves the current turn's response body is present in the
visible document or in a declared archive with an in-document summary. If the response
is missing, session-check must fail closed and name a binary-owned recovery command
that writes the missing response without rerunning already-completed side effects.

## Exit Criteria

- A completed direct Codex `agent-doc <FILE>` turn cannot silently exit with a prompt-only exchange.
- A completed Claude/skill `agent-doc <FILE>` turn cannot silently exit with a console-only response and no committed `### Re:` in HEAD.
- The diagnostic distinguishes "work not done" from "work done but response patchback missing."
- The recovery path writes the missing response into the original document boundary and does not duplicate commits or rerun side effects.
- A queue-consume divergence cannot leave an orphaned bare `[#id]` head that hard-blocks the next turn's `finalize`; recovery is a single binary-owned command that strikes the consumed head without fighting a live IPC buffer.
