# Plan — validate the JetBrains routed-cycle acknowledgment fix for stale raw prompt/response tails

## Problem

The latest JetBrains `Run Agent Doc` failure for `src/session-share/tasks/buildparty-investor-demo/repo.md` did not stall on a busy pane. Route recovered the authoritative pane, resent the reopen, and then waited 30 seconds for a new cycle ack that should never have been required:

- the changed exchange tail sat below a stale `agent:boundary`
- that tail already contained the user prompt
- it also already contained plain assistant completion prose (`I updated ...`) and follow-up bullets
- because there was no formal `### Re:` block, route/session-check still classified the tail as `pending prompt_target`

That left JetBrains with a false startup-miss even though the visible work in the document was already answered.

## Goal

Prove that stale-boundary tails with clear raw assistant completion prose no longer trigger routed cycle-ack waiting or `pending prompt_target` closeout noise.

## Validation steps

- [ ] Re-run the JetBrains `Run Agent Doc` repro against `src/session-share/tasks/buildparty-investor-demo/repo.md`.
- [ ] Confirm route no longer logs `waiting up to 30s for a new cycle ack for pending prompt_target: I renamed the repo ...` for that stale-tail shape.
- [ ] Verify `agent-doc preflight` / `agent-doc plan` on the target doc no longer classify the raw answered tail as actionable prompt-bearing work.
- [ ] If JetBrains still fails closed, capture the next blocker and whether it is a real dispatch/startup failure instead of stale-tail classification.

## Exit criteria

- The routed reopen for `buildparty-investor-demo/repo.md` no longer records a startup-miss solely because the answered tail used raw completion prose instead of a formal response heading.
- Any remaining JetBrains reroute failure is narrowed to a genuine dispatch/readiness problem with preserved stage diagnostics.
