# Plan - harness mismatch guard

## Problem

A document can specify `agent: codex` (or another harness) in its frontmatter,
but a different harness (e.g. Claude Code) can still start a session on it
without warning. There is no preflight guard that compares the document's
declared harness against the active harness environment.

This can lead to the wrong skill instructions being applied, wrong model
attribution, and wrong closeout behavior (e.g. Codex stop-replay guard vs
Claude Code finalize path).

## Classification

This is an agent-doc bug. Preflight should detect the mismatch between the
`agent:` frontmatter field and the `--origin` / active harness, then either
warn or block.

## Proposed Fix

1. **Preflight detection**: during step 0 or step 4, compare `agent:` frontmatter
   value (if present) against the detected or declared harness origin.
2. **Behavior on mismatch**:
   - If `agent:` is set and does not match the active harness, emit a
     `harness_mismatch` warning in the preflight JSON output.
   - Skill reads the warning and prints a console notice: "Document declares
     agent: codex but active harness is claude — responses will use claude
     attribution and closeout."
   - Do not hard-block — the user may intentionally switch harnesses.
3. **Model attribution**: if the document says `agent: codex` but Claude Code
   is running, the response attribution should still reflect the actual model
   (opus-4-6, etc.), not the frontmatter-declared harness.

## Scope

- `agent-doc preflight` — add harness mismatch detection
- Skill layer (Claude Code, Codex, OpenCode) — read and surface the warning
- No changes to finalize or write paths needed

## Edge Cases

- Document has no `agent:` field — no mismatch possible, skip check
- Document has `agent: claude` and Claude Code is active — match, no warning
- User explicitly switches harness mid-session — warn once, don't repeat
