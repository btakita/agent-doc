# Plan: recover answered turns when the boundary marker is left before the completed response

## Problem

On May 1, 2026, `src/session-share/tasks/claudescore-3.md` ended up in a mixed state:

1. the user prompt `Can we run specific rubrics for fine tuning?` was visibly answered
2. the assistant response text was already present in `agent:exchange`
3. the `<!-- agent:boundary:... -->` marker still sat *before* that answered prompt/response pair

That left route and session-check treating the prompt as still pending. The next `Run Agent Doc` attempt waited for a new cycle ack for `pending prompt_target: Can we run specific rubrics for fine tuning?` and then failed closed, even though the document already showed the completed answer.

## Desired behavior

If a document already contains an answered prompt after the previous boundary, agent-doc should either:

- reposition the boundary to the true end of the completed turn, or
- detect that the prompt is no longer actionable and avoid reopening / waiting on it as pending work.

The user should not need manual document surgery just to clear a stale boundary marker after a response has already landed.

## Interactive prompt?

Default: no.

This bug is a deterministic stale-state problem, not an ambiguous authoring decision. When the document already shows a completed prompt/response pair, agent-doc should repair that state automatically rather than pausing for operator input. An interactive prompt would add friction to a case where the tool should be able to prove a safe repair from the transcript shape alone.

Interactive recovery is only appropriate as a last-resort escape hatch when agent-doc detects multiple plausible repairs and cannot safely choose one, for example:

- two competing unresolved prompts both appear to match one response
- the boundary sits inside a malformed exchange and auto-repair would discard user-visible text
- the document contains concurrent manual edits that make the completion invariant ambiguous

Even in those cases, the primary contract should still be:

1. try deterministic auto-repair first
2. fail closed with a concrete explanation if repair is ambiguous
3. offer an interactive fix path only after the deterministic path is exhausted

## Investigation plan

- [ ] Reconstruct which write paths can land assistant output after the existing boundary without also moving the boundary:
  - normal finalize / patchback
  - sidecar/IPC boundary reposition
  - adopt-current / repair / session-check recovery
  - manual patchback or response replay paths
- [ ] Define the completion invariant precisely: what transcript shape proves that a prompt below the old boundary is already answered and should no longer be considered pending?
- [ ] Decide where to repair the state:
  - write-time: move the boundary when the response is appended
  - recover-time: session-check / repair canonicalizes a stale boundary
  - route-time: pending prompt detection ignores already-answered prompt/response pairs even if the boundary is stale
- [ ] Encode the recovery policy explicitly:
  - deterministic answered-turn repair is the default
  - interactive prompting is reserved for irreducibly ambiguous repair shapes only
- [ ] Add a deterministic repair path for existing docs already in this broken shape so reruns can recover automatically.
- [ ] Add regression coverage for the concrete repro:
  - a document with `agent:boundary` before an answered prompt
  - route must not wait for a new cycle ack on that prompt
  - repair/session-check must move or neutralize the stale boundary
- [ ] Patch the final outcome back into `tasks/agent-doc/agent-doc-bugs2.md` and clear the backlog item only after a real routed session confirms the fix.
