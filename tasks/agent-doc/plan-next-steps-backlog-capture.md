# Plan: #next-steps preset backlog capture failure

## Bug
Previous agent-doc turn for `agent-doc-bugs2.md` acknowledged `#next-steps` but did not add any backlog items. The skill's finalize step did not run `--pending-add` for the follow-up items identified in the response.

## Root cause
The `#next-steps` preset generates follow-up recommendations in the response body, but the skill does not enforce that those recommendations are captured into `agent:backlog` before finalize. The pending capture rule requires concrete follow-up work to be added in the same cycle, but there is no guard verifying that `#next-steps` output was persisted.

## Fix
1. Add a `next_steps_capture_guard` to session-check / finalize that detects when the `#next-steps` preset was requested but no new backlog items were added in the same cycle.
2. Fail closed (or warn + add `<!-- no-pending-capture -->`) when the guard detects uncaptured next-steps output.
3. The guard should be similar to `pending_capture_guard` but scoped to the `#next-steps` preset specifically.

## Reproduction
- Session: agent-doc-bugs2.md, generation ~143-145
- Prompt: `#next-steps`
- Response identified follow-up items but finalize did not run `--pending-add` for any of them
- User had to manually add `#rspcmt7` and request patchback in a subsequent turn
