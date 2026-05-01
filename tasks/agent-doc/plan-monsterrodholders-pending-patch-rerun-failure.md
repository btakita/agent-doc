# Plan — fix rerun failure from legacy backlog/pending patch output

## Problem

On May 1, 2026, rerunning `agent-doc` for `src/boost-client/tasks/monsterrodholders.md` failed after handoff to Codex with:

- `warning: <!-- patch:backlog --> is deprecated`
- `Error: ERR: replace:pending block forbidden`

The document itself is valid. The failure happened because the generated response payload attempted to mutate tracked backlog/pending state with legacy full-replace patch syntax instead of the current granular `--pending-*` contract.

## Repro Shape

1. Start from a template-mode session doc with an `<!-- agent:backlog -->` component.
2. Submit a prompt that asks for diagnosis plus a backlog update, such as `Why did this happen? #agent-doc-bug`.
3. Let the routed Codex cycle generate the response.
4. Observe that the write/finalize layer rejects the generated payload before capture/write if it contains `patch:backlog` and/or `replace:pending`.

## Why This Is A Bug

- The current skill and installed runbook already instruct agents to use granular pending flags, not full-replace backlog/pending patches.
- The binary has a compatibility path for a narrow accidental `replace:pending` shape, but mixed or unsupported shapes still fail closed.
- That means a valid planning/reporting prompt can still die at persistence time because the model emits a stale mutation surface.

## Investigation Targets

1. Capture the exact payload produced by the failing cycle before rejection so the source shape is visible in logs or captures.
2. Verify whether the routed Codex prompt assembly is omitting, truncating, or diluting the pending-ops instructions.
3. Verify whether the `#agent-doc-bug` plan/backlog path adds any extra prompting that nudges the model toward explicit patch blocks.
4. Inspect write/finalize normalization to see whether a safe single `patch:backlog` list replacement can be translated into granular pending ops, or whether better diagnostics are the right fix.

## Recommended Fix

1. Improve diagnostics first:
   - record the rejected patch names and whether the response mixed backlog/pending replace blocks
   - preserve enough failing payload metadata to debug without relying on inference from stderr alone
2. Harden the prompting path:
   - make the pending/backlog mutation rule more explicit in the routed Codex write instructions for plan/backlog cycles
   - add a targeted regression for `#agent-doc-bug`-style prompts that ask for both explanation and backlog capture
3. Consider a narrow normalization backstop:
   - if the payload contains exactly one safe list-shaped backlog replacement with no prose edits inside the backlog component, translate it to granular pending mutations before capture
   - keep failing closed on mixed-content or ambiguous shapes

## Files Likely To Change

- `src/agent-doc/src/write.rs`
- `src/agent-doc/src/finalize.rs`
- `src/agent-doc/src/skill.rs`
- `src/agent-doc/runbooks/pending-ops.md`
- `src/agent-doc/tests/pending_integration.rs`
- `src/agent-doc/tests/finalize_integration.rs`

## Exit Criteria

- A rerun of the same `monsterrodholders.md` repro no longer dies on legacy backlog/pending patch syntax.
- The failing shape is either normalized safely or rejected with enough structured diagnostics to identify the source immediately.
- Regression coverage proves the `#agent-doc-bug` plan/backlog flow does not emit forbidden backlog/pending patch mutations.
