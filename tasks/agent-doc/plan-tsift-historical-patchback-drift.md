# Plan — fix historical patchback drift that hides captured responses behind later fail-closed commits

**Pending:** `#hpbd`

**Observed in:** `tasks/software/tsift.md`

## Problem

`tsift.md` preserved the `#code-review` response in durable capture state, but a later
document closeout still left the visible exchange with `❯ #code-review` and no response.
That forced a manual user follow-up to ask for the response to be inserted.

The preserved evidence shows this was not a missing model response:

- `.agent-doc/captures/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c/cycle-1777427142246.json`
  contains the full `### Re: code review — gpt-5` response body for `tasks/software/tsift.md`
- the later `tsift.md` compact/commit attempt reported:
  `refusing to auto-adopt committed historical response patchback ... HEAD contains an out-of-band typed_component_drift mutation ...`
- `tasks/software/tsift.md` itself records the specific historical marker:
  `### Re: code review — gpt-5` with a bare prompt target missing the expected `❯ ` prefix

So the user-visible miss was caused by historical document drift plus strict closeout behavior,
not by the model failing to generate a response.

## Root Cause To Prove

The likely chain is:

1. an earlier response patchback landed in `HEAD` out of band, with malformed exchange shape
   (`### Re:` marker present, but prompt-target pairing drifted)
2. snapshot and `HEAD` then diverged in a way that `detect_bypassed_response_write_between()`
   and `has_non_exchange_component_drift()` classify as a committed historical patchback
3. a later legitimate closeout for `tsift.md` correctly failed closed in `git::commit()` rather
   than silently absorbing that history
4. the durable capture still contained the real response, but the document surface did not make
   that state obvious enough, so the user had to ask for the patchback manually

The code paths involved are in:

- `src/agent-doc/src/session_check.rs`
- `src/agent-doc/src/git.rs`
- `src/agent-doc/src/repair.rs`

## Desired Behavior

When a document already contains historical malformed patchback state, `agent-doc` should do one
of these things deterministically:

1. auto-repair the snapshot/working state when it can prove the historical drift is exchange-only
   and the captured response can be replayed safely
2. fail closed with a precise, user-visible diagnostic that says the response exists in durable
   capture but historical patchback drift blocked adoption
3. provide an explicit one-step repair path that replays the captured response through the normal
   write/commit boundary without making the user rediscover the missing response by hand

It should not leave the session looking like “the patchback never happened” when the response was
already captured.

## Plan

- [ ] Reconstruct the exact `tsift.md` chain across snapshot, `HEAD`, working tree, durable
  capture, and cycle state:
  - the commit that introduced the malformed historical `### Re:` marker
  - the later compact/commit cycle that failed closed
  - the manual repair cycle that finally wrote the response into the document
- [ ] Audit the historical-drift classifiers in `git.rs`:
  - `classify_committed_historical_agent_doc_mutation`
  - `has_non_exchange_component_drift`
  - `repair_committed_historical_snapshot_drift`
  - `detect_bypassed_response_write_between`
- [ ] Define the safe-repair envelope for historical response patchbacks:
  - exchange-only malformed prompt/response pairing
  - durable capture already marked committed
  - current document still missing the captured response
- [ ] Decide whether the fix belongs in:
  - stricter automatic repair before closeout
  - clearer fail-closed diagnostics with durable-capture provenance
  - or a dedicated replay path that `preflight`/`repair` can trigger safely
- [ ] Add regression coverage for the `tsift.md` class:
  - durable capture contains response
  - `HEAD` contains historical malformed response marker
  - later closeout currently fails closed
  - repaired flow either auto-recovers or emits a precise guided error

## Success Criteria

- a captured response cannot remain effectively hidden behind historical patchback drift without
  a precise diagnostic
- `agent-doc` either repairs this state safely or points to the exact durable capture/replay path
- the `tsift.md` repro no longer requires the user to ask manually for a previously captured
  response to be inserted
