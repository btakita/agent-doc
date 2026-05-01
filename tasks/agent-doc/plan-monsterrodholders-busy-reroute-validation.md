## Goal

Validate the live `src/boost-client/tasks/monsterrodholders.md` reroute after the same-document busy-pane fallback stops fail-closing on the missed idle-prompt heuristic.

## Checks

- Re-run the exact `Run Agent Doc` / `agent-doc route tasks/monsterrodholders.md` shape against the live pane that previously failed with `is still not showing an idle codex prompt`.
- Confirm route now sends the bare `agent-doc <FILE>` reopen after the no-op scoped-fix path when the supervisor is healthy and the pane still authoritatively owns the document.
- Confirm the reroute still fails closed with a startup-miss if the reopen is accepted but no new document cycle starts.

## Exit Criteria

- The original `monsterrodholders.md` repro no longer stops at the idle-prompt gate.
- Ops logs and user-facing route output show either a successful new cycle ack or a startup-miss proof, not the old busy-pane false negative.
