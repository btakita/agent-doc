---
name: reference_lazily_coverage_json_hot_multisession_file
description: "lazily-spec/coverage.json is a hot multi-session file; committing it swallows other sessions' pending binding flips"
metadata: 
  node_type: memory
  type: reference
  originSessionId: 73e3ddac-f51f-4f72-b922-49c60d4f4707
---

`src/lazily-spec/coverage.json` is the single-source feature matrix; every binding
README + `docs/coverage.md` regenerates from it via `node scripts/sync-coverage.mjs`
(guard: `--check`). When several agent sessions each implement the same feature row
in their own binding (e.g. #lzzcpy zero-copy transport landing in go/py/dart at once),
they ALL flip their cell in this one shared working-tree file simultaneously.

**Hazard:** `git add coverage.json && commit` captures the WHOLE working-tree state —
swallowing every other session's uncommitted flip (and their compact↔pretty reformat)
into your single commit. The repo convention is one commit per binding
("docs(coverage): mark <Lang> ✅ for <row> (#tag)"), so a bundled commit clobbers that.

**How to apply:** After implementing your binding's feature, your cell is present in the
shared coverage.json + synced READMEs (coverage-check passes) — that's enough for your
binding's own repo/release. Do NOT commit the shared spec coverage files if
`git diff HEAD coverage.json` shows flips for bindings other than yours. Either land a
Go-only/your-binding-only commit surgically against HEAD (isolate your one-cell change,
regenerate derived, restore the others' working-tree flips), or leave it for the coverage
coordination to carry. Your binding's OWN committed README already reflects your ✅ cell;
other bindings' cells drifting in your working tree is cosmetic churn, not your commit.

Related: [[feedback_concurrent_sessions_force_push_shared_superproject]],
[[feedback_session_document_scope_discipline]]. Release convention:
[[reference_lazily_release_targets]] (go → GitHub tag `v*` = module publish).
