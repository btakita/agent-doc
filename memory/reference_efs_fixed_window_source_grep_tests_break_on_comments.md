---
name: reference_efs_fixed_window_source_grep_tests_break_on_comments
description: "EFS guard tests slice a FIXED byte count from a function's start to grep its body — adding a leading comment silently pushes the guarded call out of the window. Slice to the end of the function instead."
metadata:
  node_type: memory
  type: reference
---

Hit twice in two cycles (2026-07-30, EFS `653fe3fe` and `0a87c46d`).

`tests/admin-pipeline.test.ts` and `tests/admin-investor-pipeline.test.ts` guard
behaviour by grepping **source text**:

```ts
const idx = src.indexOf('export async function foo_')
const block = src.slice(idx, idx + 800)      // ← fixed byte count
expect(block).toContain("WHEN ? = 'Funded' THEN 'funded'")
```

Add an explanatory comment above the guarded code and the call slides past the
window. Observed windows: `800` (`#closed-step-decouple` in `admin-pipeline`),
`1800` and `900` (accreditation endpoints in `admin-investor-pipeline`).

**It fails loudly, which is the only mercy** — a red `toContain` during a normal
edit. The dangerous variant is the negative assertion (`.not.toContain(...)`),
which silently starts passing for the wrong reason once the window no longer
reaches the code it is meant to forbid.

Fix, applied at both sites:

```ts
const end = src.indexOf('\n}', idx)
expect(end).toBeGreaterThan(idx)
const block = src.slice(idx, end)
```

Related trap in the same repo: `tests/efs-production-environment.test.ts` once
matched a table name in prose because it used bare `indexOf('[observability.logs]')`
instead of a line-anchored match. Same family — text-matching a source file without
anchoring to its structure. Prefer `tsift edit-intents` / AST matching over both
([[feedback_use_ast_not_regex_for_source_edits]]).

See also [[reference_efs_repo_has_no_ci_workflow]]: local `make check` is the only
gate, so a guard that quietly stops guarding is never caught downstream.
