---
plan: finalize-overwrites-prior-response
status: backlog
---

## Bug

A subsequent `agent-doc finalize` cycle replaces the prior response in the exchange instead of appending after it. The previous turn's response heading and body disappear from HEAD; only the new response is committed. The user notices the loss when their renderer/scrollback shows the new response but the prior one is gone from the document on disk.

Distinct from `#rhdr-blank` (which keeps both responses but jams the new heading onto the prior paragraph's last line). This bug *removes* the prior response entirely.

## Concrete repro

`/home/brian/work/btakita/agent-loop/src/boost-client/tasks/monsterrodholders.md` between commits:

- After commit `7928306` (16:27:01): exchange contained `### Re: Add brian.takita / monsterrodholder74 as manager — already have access — opus-4-7` (the body walks through the Access and security users page and reports "user already has access").
- After commit `a87ce95` (16:47:15, from `cat /tmp/response12.md | agent-doc finalize … --stream`): exchange in HEAD contained only `### Re: Link MCC — need one more detail — opus-4-7`. The prior "Re: Add brian.takita…" response was no longer present in HEAD.
- User restored the missing response by editing the working tree; plain `git commit` at `768f002` reinstated both responses in the correct order.

Note the finalize log for that cycle ended with `Error: [session-check] INTERRUPTED: found likely direct response patchback without agent-doc cycle: ### Re: Add brian.takita / monsterrodholder74 as manager …`. The interrupt detected the prior response as a "direct patchback" but did not prevent the commit, and recovery via `agent-doc reset --from-current` blessed the wrong (truncated) state.

## Root cause hypothesis

The `<!-- patch:exchange -->` apply path likely rewrites the entire exchange component from a stale snapshot, then appends the new block. When the snapshot lags behind HEAD by one cycle (because the previous cycle's commit didn't refresh the snapshot in the path the next finalize reads), the previous response is dropped.

Likely interaction with `#rhdr-blank` (no separator between responses) and `#response-patchback-uncommitted-regression` (existing plan). The `session-check` "direct patchback" warning detected the divergence but the recovery path treated the divergent content as the suspicious patchback rather than as recoverable prior content.

## Proposed fix (do NOT implement here)

1. Make the exchange-append path **always read from HEAD** for the existing content (not from a possibly-stale snapshot), then validate that the snapshot matches HEAD before applying patches; if it diverges, reload from HEAD instead of overwriting.
2. Change `session-check`'s "direct patchback without agent-doc cycle" recovery: when a prior response heading is present in the working tree but missing from HEAD, prefer keeping the working-tree content (the source of truth the user is staring at) and re-running the commit, instead of treating that content as a foreign patchback to delete.
3. Tighten the `agent-doc reset --from-current` semantics so it never blesses a working tree that strictly subtracts response sections from HEAD without a corresponding user delete intent.
4. Regression test: simulate two back-to-back finalize cycles where the second cycle's snapshot lags by one; assert HEAD after the second cycle still contains both response headings (in correct order).

## Acceptance

- After two back-to-back `cat | agent-doc finalize … --stream` cycles, both `### Re: <topic1>` and `### Re: <topic2>` are present in HEAD with the topic-2 heading appearing after topic-1's body.
- `session-check` no longer flags a prior-cycle response as a "direct patchback" once the cycle is committed.
- Existing exchange-append regression suite still passes.

## Related

- [[plan-response-heading-missing-blank-line]] (#rhdr-blank)
- [[plan-response-patchback-uncommitted-regression]]
- [[plan-direct-chat-response-patchback-required]]
- [[plan-late-fallback-patch-redirties-committed-doc]]
