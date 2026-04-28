---
name: Cycle Steps (mandatory write-back)
description: Enforce the five-step agent-doc cycle. Prevents silent-skip bug where Claude answers in console only and never pipes to `agent-doc write`.
---

# Agent-Doc Cycle Steps

Every `/agent-doc <FILE>` invocation MUST execute all five steps. Skipping step 3 is the silent-skip bug: the user sees no response land in their editor, and `ops.log` shows a `preflight_diff_start` with no paired `ipc_write_attempt`.

## Steps

1. **Preflight.** Run `agent-doc preflight <FILE>`. Capture the JSON. If `no_changes: true`, stop. Note `pending_reordered` — if `true`, do not reorder pending this cycle.
2. **Gate checks.** Handle `slash_commands`, `builtin_commands`, `env` exports, `pending_callbacks`, and the model tier gate. Any hard-stop condition (tier mismatch, callback-only cycle) ends the cycle here.
3. **Compose response.** Write the response as patch blocks (template mode) or plain text (append mode). The console text and the document write MUST be the same content — do not answer in console only.
4. **Decide pending mutations.** If the document has an `agent:pending` component, decide which of `--pending-add` / `--pending-done` / `--pending-edit` / `--pending-clear` / `--pending-reorder` flags apply this cycle. Never include a `<!-- patch:pending -->` block — the binary rejects it. Zero mutations is fine (the flags default to no-op). See SKILL.md §1b.
5. **Write-back (MANDATORY).** Pipe the response through `agent-doc write <FILE> --baseline-file <preflight.baseline_file> --stream --origin skill <pending flags...>`. If step 1 reported a non-empty `diff`, step 5 is non-optional. Skipping it means the user's question is silently lost.
6. **Commit.** Run `agent-doc commit <FILE>` immediately after step 5.

## Self-check before ending the turn

Before emitting your final user-facing message, verify:

- [ ] Preflight ran and returned a diff (or `no_changes`).
- [ ] If `pending_reordered: true` was reported, I did NOT pass `--pending-reorder` this cycle.
- [ ] If diff was non-empty: did I run `agent-doc write`? If no, DO IT NOW before responding.
- [ ] If the response contains a `<!-- patch:pending -->` block: REMOVE IT and convert to `--pending-*` flags. The binary will error otherwise.
- [ ] If write ran: did I run `agent-doc commit`? If no, DO IT NOW.

If you catch yourself about to end the turn after writing a long response in the console but without an `agent-doc write` tool call in the same turn, STOP and execute step 4. The response only exists in the document if `agent-doc write` ran.

## Failure modes this runbook prevents

- **Console-only answer.** Claude addresses the user's edits in prose but forgets step 4. Fingerprint in `ops.log`: `preflight_diff_start` with no following `write_origin` for that doc.
- **Tier-gate false stop.** A tier mismatch prints "Run /model <x>" and Claude treats the advisory as a hard stop even when `required_tier` is not set. Only stop when `required_tier` is set AND the current tier is lower.
- **Runbook detour.** Claude reads a referenced runbook (e.g. `compact-exchange.md`) and follows it instead of the normal cycle, then forgets to write the user's original question back. Runbooks handle specific sub-operations; they do not replace steps 4 and 5 unless they explicitly say so.
