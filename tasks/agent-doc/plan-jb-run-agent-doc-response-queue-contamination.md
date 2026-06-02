# Plan: JB Run Agent Doc can enqueue assistant response prose

**Bug ID:** `#jb-run-agent-doc-response-queue-contamination`
**Status:** planned
**Observed:** 2026-05-30 in `src/boost-client/tasks/monsterrodholders.md`

## Problem

After the malformed compact output and uncommitted repair, JetBrains `Run Agent
Doc` added part of an assistant response body to `agent:queue auto`.

The live dirty diff in `src/boost-client/tasks/monsterrodholders.md` shows:

```markdown
<!-- agent:queue auto -->
preset #spec-test-commit-push-deploy
- do [#nbsearch]
- do [#bidstrat]
- do [#pdpconv]
- Yes. I drove the already-authenticated Google Ads browser session with `chromium-bridge` ...
<!-- /agent:queue -->
```

The final line is not a user prompt or queue directive. It is prose copied from
the `### Re: #gads106demote` response body.

## Desired Behavior

Route and queue synthesis must preserve provenance:

- only user-authored prompts, explicit `do [#id]` directives, or backlog-derived
  queue entries may enter `agent:queue`
- assistant response bodies under `### Re:` are never queue candidates
- free-text queue entries created by route must carry source metadata or pass a
  strict prompt-origin check
- malformed exchange/repair state must fail closed before queue regeneration

## Plan

1. Add a fixture using the current `monsterrodholders.md` dirty diff:
   malformed compact history, corrected visible exchange, active backlog, and a
   response-body sentence that must not become a queue line.
2. Audit `route.rs` pending-dispatch enqueue logic and queue auto regeneration
   for any path that extracts text from exchange response bodies.
3. Add an exchange parser helper that splits user prompt regions from assistant
   `### Re:` regions before queue synthesis.
4. Require queue additions to identify a source:
   - explicit user exchange prompt
   - backlog/review item id
   - existing queue line preserved from visible user edit
5. If the source cannot be proven, skip the queue mutation and emit a diagnostic
   naming the rejected candidate text.
6. Add session-check coverage that rejects assistant-response prose inside
   `agent:queue auto` unless it was already an explicit user-authored queue line.

## Likely Files

- `src/agent-doc/src/route.rs`
- `src/agent-doc/src/queue.rs`
- `src/agent-doc/src/preflight.rs`
- `src/agent-doc/src/template.rs`
- JetBrains `Run Agent Doc` route/action tests

## Verification

- Regression: `Run Agent Doc` on the malformed/repair mixed fixture queues only
  `#nbsearch`, `#bidstrat`, and `#pdpconv`; the `Yes. I drove...` sentence is
  rejected.
- Regression: normal backlog-to-auto-queue regeneration still works.
- Live JetBrains replay on `monsterrodholders.md` does not add response prose to
  the queue.
