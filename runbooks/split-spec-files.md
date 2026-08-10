# Split Spec Files

Use this runbook when a spec file has become too large or too internally disconnected to remain one clean source of truth.

## When to split

Split a spec when at least one of these is true:

- The file is well past the project line-budget guidance and keeps growing.
- Independent features are changing in different sections every week.
- Reviewers need to scroll through unrelated rules to validate one change.
- A stable entrypoint file would help preserve external links while moving detailed behavior elsewhere.

Do not split by equal page count. Split by behavior boundary.

## Target shape

- Keep the original numeric/spec identity as a short index file whenever external links already point at it.
- Move normative detail into sibling files named for behavior, not chronology.
- Put the most volatile, most cross-referenced behavior in the focused sibling files.
- Leave shared invariants in the index only when they really span multiple sibling specs.

## Managed instruction surfaces

- Apply the same split rule across agent-doc-managed harness instruction surfaces, not just one harness. If Claude Code, Codex, or a future managed harness installs the same rule, keep the ownership contract aligned across those surfaces.
- Treat user-owned root instruction files such as project `AGENTS.md` or `CLAUDE.md` as opt-in. Auto-split or rewrite them only when they are still clearly agent-doc-managed or still exactly match the generated baseline.
- Prefer wording in terms of managed-versus-custom ownership rather than naming one harness as special.

## Procedure

1. Outline the current file by major headings and mark which sections change together.
2. Choose 2-4 sibling files that each have one clear ownership boundary.
3. Keep the original spec file as the stable index when it is already linked from `SPEC.md`, docs, or instructions.
4. Move or rewrite the detailed sections into the sibling files. Preserve command names and invariant wording that callers already rely on.
5. Add a command/spec map in the index so a reader can find the right sibling file without scanning every spec.
6. Update any top-level catalog that points at the old monolith, such as `SPEC.md`, docs indexes, or instruction surfaces.
7. Record the split in `VERSIONS.md` when the repo tracks doc/spec changes there.
8. Re-run the verification/audit path after the split so path references and instruction budgets stay valid.

## Naming rules

- Prefer names like `07-closeout-commands.md` or `07-session-tmux-commands.md`.
- Reuse the parent number so readers still understand the files as one command-spec family.
- Avoid names that only mirror implementation modules unless the spec boundary is identical to the implementation boundary.

## Guardrails

- Do not turn the old file into a dead link.
- Do not duplicate the same invariant in multiple siblings unless the duplication is explicitly intentional and kept in sync.
- Do not leave hidden behavior in `VERSIONS.md` or instruction prose that never made it into the new sibling specs.
- If the split creates a reusable authoring rule, add a short instruction-surface pointer to this runbook so the next split follows the same pattern.
