# Code-Enforced Directives

Behavioral invariants must be enforced by the binary, not by agent instructions or machine-local memory. This ensures correctness on any cloned repo, regardless of which agent or environment runs it.

## Principle

When a behavioral rule exists in the SPEC:
1. **Implement it in the binary** (correct by construction)
2. **Add tests** (regression impossible)
3. **Document in the SPEC** (intent for future development)
4. **Update module harness** (spec comments in source code)

Machine-local memory (MEMORY.md, feedback files) supplements but **cannot replace** code enforcement. Memory is for agent behavior patterns; code is for system invariants.

## When This Applies

Before writing a SPEC invariant, ask: "Can the binary enforce this without agent cooperation?"

| Invariant type | Enforcement | Example |
|---------------|-------------|---------|
| Pane lifecycle rules | Binary code | Binding invariant: claim provisions new pane when occupied |
| File format rules | Binary code | Auto-scaffold empty files in sync |
| Comment stripping | Binary code | strip_comments in diff pipeline |
| Response routing | Skill (SKILL.md) | Diff-type routing (requires LLM judgment) |
| Quality processes | Runbooks + memory | Precommit testing discipline |

**Rule of thumb:** If it can be a `#[test]`, it belongs in the binary.

## Anti-Pattern: "Correctly Refused"

Never characterize broken binary behavior as "correct" because agent instructions say so. If the binary errors when the SPEC says it should provision, the binary is wrong — fix the binary, don't rationalize the error.

Verify against the SPEC before characterizing any error as expected behavior.

## Anti-Pattern: Manual Intervention

Never manually fix a problem that the software/process should handle. If a file wasn't committed, don't `git add && git commit` it manually — fix the pipeline so it commits automatically. If a UUID wasn't assigned, don't assign it manually — fix `ensure_session_uuid` or the entrypoint that should have called it.

Manual fixes mask systemic issues. The user expects the software to work. Every manual intervention is a bug report.

## Checklist for New Invariants

- [ ] Invariant defined in SPEC.md
- [ ] Implemented in binary code (not just agent instructions)
- [ ] Test proving the invariant holds
- [ ] Test proving violation is caught (negative test)
- [ ] Module harness spec updated (doc comment in source)
- [ ] No dependency on machine-local memory for correctness
