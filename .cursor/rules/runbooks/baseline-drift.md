# Baseline Drift

Use this runbook when an `agent-doc` cycle was committed, then the user made a
separate manual commit to the same session document before the next preflight.

## Recovery Contract

`capture::validate_replay` may refresh the cycle baseline automatically when it
can prove the manual commit is benign:

- the captured baseline still represents the prior committed document state,
- the live document drift is outside the captured response body and outside the
  component scope the active cycle owned, and
- the refresh would not commit or hide a fresh prompt target.

When the only response-body difference is the known normalized-response shape
for a user-cleaned response, the binary may adopt that normalized response and
refresh the baseline. This preserves the active cycle instead of forcing the
operator through a destructive reset.

Any other response-body overlap remains fail-closed. The error should include
the non-destructive recovery command:

```bash
agent-doc reset --from-current --preserve-session <FILE>
```

## Operator Path

1. Re-run `agent-doc preflight <FILE>`.
2. If preflight auto-refreshes, continue the normal cycle and persist through
   `agent-doc finalize <FILE> --baseline-file <preflight.baseline_file>`.
3. If preflight refuses because drift overlaps owned response content, inspect
   the document, then run the printed `agent-doc reset --from-current
   --preserve-session <FILE>` command only when the current visible markdown is
   the state to keep. The reset preserves the captured response payload and
   cycle while rebasing that active capture's replay hashes to the explicitly
   accepted visible document.
4. Re-run preflight after the preserve-session reset and continue normally.

## What Not To Do

- Do not use plain `agent-doc reset --from-current` for this case unless the
  user explicitly accepts losing session continuity and snapshot history.
- Do not manually patch the response into the document or repair with a bare
  `git commit`; the next response still needs to cross `finalize` or
  `write --commit`.
- Do not auto-refresh when the drift changes the captured response body in a
  way that is not the normalized-response adoption case.

## See Also

- `runbooks/commit.md` — binary-owned closeout ordering.
- `runbooks/jb-cache-conflict.md` — related IPC cancel recovery path.
- `specs/07-core-commands.md` — `reset --from-current --preserve-session`.
- `specs/12-deterministic-simulation.md` — SimWorld coverage for this class.
