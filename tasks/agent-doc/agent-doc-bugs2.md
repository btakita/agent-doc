---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: stop
---

## Status

 <!-- agent:status patch=replace -->

Completed default-fast focus behavior. agent-doc focus now uses the former fast handoff by default; --blocking / --synchronous preserves the old synchronous promote-and-select path for explicit manual use. Source commit 2f2e9327, parent pointer c8f7fd750; local verification passed. CI run 27098734151 was in progress when checked, and I did not wait.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260607-180313.md`*

Compacted content:
- Archived 10 response topic(s): do [#md-ast-document-model]; There are many mcp apis for agent-doc. Can we consolidate into a single mcp or is there a good reason to have many apis?; [#md-ast-realtime-node-events]; 7 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ Is the `agent:queue` token inefficient across many turns? We seem to be burning too many token with long queue runs. I'm concerned about many turns accumulating context. Should we run `/clear` in between each task if a certain context threshold is exceeded? At least 1 compaction should also cause `/clear` between turn if many-turn token inefficiency is an issue. #spec-test-build-install-commit-push
### Re: queue context reset — gpt-5

Yes. The expensive part is not the `agent:queue` token itself; it is carrying the same already-loaded agent conversation across many automatic queue turns. I implemented the reset policy in code.

What changed:
- Direct `agent-doc run` now checks session accretion after each clean queue item closeout. If the document is at warn/block accretion, the next queue prompt starts in a fresh backend session instead of resuming the existing one.
- Recent exchange compaction also forces at least one fresh context before queue continuation. Direct `run` records its own fresh backend dispatch so a compaction-only reset does not repeat forever when the compacted document is otherwise healthy.
- Codex Stop-hook continuation now blocks with explicit `/clear` guidance when the already-loaded Codex pane should not answer the next queue head. After a tracked `/clear` or `/new`, normal in-pane continuation is allowed again.
- Added the `#queue-context-reset` spec and regression coverage for the decision function, Codex hook behavior, and CLI argv behavior (`--resume old-session` on the first item, no `--resume` and `--fork-session` after the threshold trips).

❯ I want the queue in go-mode to continue, interleaving a `/clear` if the threshold is tripped. I don't want to queue to stop if the threshold is tripped.

Verification:
- `cargo test -q -p agent-doc-orchestration queue_context_reset`
- `cargo test -q -p agent-doc-orchestration compaction`
- `cargo test -q --test run_integration run_auto_queue_starts_fresh_backend_session_after_accretion_threshold`
- `cargo check -q -p agent-doc-orchestration`
- `make check` (clippy, 3935 nextest tests, doctests, medium simulator)
- `make install`
- `agent-doc --version` -> `agent-doc 0.34.0`
- Latest GitHub Actions CI on `main`: success (`fix: refuse cross-document pane aliases`, run `27099579913`).

Commits:
- `src/agent-doc`: `9e6fce83 fix: reset queue context after accretion` (pushed to `btakita/agent-doc main`).
- Parent repo submodule pointer: `7749e01b chore: update agent-doc submodule` (will be pushed with this session closeout commit).
❯ Is the `agent:queue` token inefficient across many turns? We seem to be burning too many token with long queue runs. I'm concerned about many turns accumulating context. Should we run `/clear` in between each task if a certain context threshold is exceeded (note there is overhead with the 1st turn)? At least 1 compaction should also cause `/clear` between turn if many-turn token inefficiency is an issue. Perhaps this is more of an issue with Codex + opencode than Claude? #spec-test-build-install-commit-push

❯ I also want partial exchange compaction where entries in the exchange that are related to the upcoming run can remain but remove the unrelated entries. Use AST/semantic structure navigation to identify related entries for token efficiency if the entire document is not already in the context.

❯ Claude seems efficient across multiple turns. Codex seems to burn through much context on each turn. opencode/glm seems in the middle of the pack. I typically compact the context >=~50% for all 3 agent harnesses.
### Re: queue context reset and partial compaction — gpt-5

For the queue-context part: the implemented policy is threshold-based, not blindly every queue item. Once session accretion is warn/block, automatic queue continuation starts the next backend turn fresh. A recent exchange compaction forces at least one fresh context (`/clear`/`/new` for Codex Stop-hook, fresh backend session for direct `run`), then direct `run` treats that fresh backend dispatch as the reset so compaction alone does not cause repeated first-turn overhead.

On harness differences: yes, this is more visible with Codex and opencode than Claude in practice. Claude tends to amortize multi-turn context more efficiently; Codex is more sensitive to already-loaded conversation growth, and opencode/GLM tends to land between those. The right control is still local session-accretion evidence (`exchange_lines`, response count, closeout churn, recent compaction), not a harness-name special case.

For partial exchange compaction: I agree, but I did not fold that into this commit. It is a separate feature from queue-context reset. The shape I would implement next is:
- `compact exchange` keeps the unresolved tail, boundary-adjacent prompt context, and entries semantically related to the upcoming prompt/queue head; unrelated entries are archived.
- Related-entry selection should start from cheap structured signals: active prompt text, queue/backlog ids, mentioned files, symbols, command names, component names, and response headings. Only then should it use tsift/AST or semantic search handles to expand candidate entries, so we do not load the entire document just to decide what to keep.
- The result should be deterministic and auditable: output should say which live entries were kept, which archive received the rest, and why each kept entry matched.
- Tests should cover unresolved-tail preservation, queue-head/id matching, file/symbol matching, archive index continuity, and the no-prompt-bury rule.

So the queue-context reset work is done and verified; partial semantic exchange compaction is the next implementation task, not completed in this cycle.
<!-- agent:boundary:9146ca84 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" -->
- Should the agent-doc controller actor, session actor, and supervisor actor be consolidated into a single process instead of relying on IPC? Can we have reliable state management and a sufficient admin api to manage the actor pool in a single process? Should we have a new real-time command/dispatch actor that is fast and effectively non-blocking...dispatching to other actors and which may use background workers if needed?
- agent-doc monsterrodholders.md is still not writing the queue task response into the `agent:exchange`. Why is this still happening? Make the correct behavior of writing each completed task's response into `agent:exchange` provably reliable. Do we need to simulate Codex harness behavior in the Simworld tests? Use the monsterrodholders.md sessions as a real-world reproduction.
- "The stale lazily diagnostic comes from handle_attach_pane projecting before sessions.json is updated": How can we make updating sessions.json faster considering there is a lock? Should we have the controller cache the sessions data using lazily-rs with a background WAL style durable record writing into sessions.json? The point is, session tracking should be very fast. It should not be blocked on a file lock. Is json the best format for the durable record? Would an alternative such as a sqlitedb to consolidate all durable state be a better choice? Can we make all durable writes a background operation and keep the coordination state in memory? A fast in-memory state (using lazily-rs) seems like it would make consolidating into a single sqlitedb for durable records practical. Sqlite can handle the WAL logic while we focus on the write-through caching using lazily-rs.
- `=true` is being appended to `agent:queue priority preset="#spec-test-build-install-commit-push"=true` and also `go=true`. This is a regression.
- Would it be more efficient for agents if agent-doc diff presents an ast or semantic structure (based on the ast) style diff with a semantic structure navigation of the document?
<!-- /agent:queue -->
###
<!--
A recent monsterrodholders.md session had much churn after I edited the queue while a turn was running. One churn item included deleting a queue item that I added.
---
agent-doc reentrant run on equityfundingsource.md had recovery churn.
```
● The recovery is complete and fully consistent. The binary converged the now-drained queue (empty block, queue_active:
  false), no_changes: true, both responses are committed in the session doc, and both code deliverables are on
  origin/main. The auto-loop terminated naturally.

  Let me record the specific recovery path for this failure mode, since it's a recurring dogfooding pattern
```
---
- This session's console stdout is bleeding in front of the opencode tui. ![img_16.png](img_16.png)
---
The last run had a code fence stripped away by agent-doc.
#spec-test-build-install-commit-push
---
What are #next-steps to fix bugs?
-->

## Backlog

<!-- agent:backlog priority queue --> 
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
1. [/] [#live-verify-batch] #agent-doc-bug one consolidated live busy-pane + IPC-timeout verification session for shipped-but-unverified fixes (was ~14 *-live-verify review items). Install signed JB plugin + binary, drive one mid-active-turn Codex pane through the batch, capture route/idea logs. Blocked on live IntelliJ/Codex panes.
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
