# Dynamic Context

Use AGENTS.md and SKILL.md files as routing surfaces. They should say when to
load context and which file or command owns it; they should not restate detailed
workflow, schema, history, or example content already stored elsewhere.

## Authoring Rule

- Keep trigger metadata, irreversible safety rules, and one-line handoff rules in
  AGENTS.md or SKILL.md.
- Move procedures to `runbooks/`, domain facts to `references/`, generated state
  to a command or service, and concrete examples to `examples/` or fixtures.
- Make each AGENTS/SKILL entry name the file or command that owns the detail:
  `Use runbooks/persist-closeout.md for closeout`, not an inline copy of the
  closeout procedure.
- Prefer `skill-harness install-dir` / `check-dir` for skills with companion
  resources so installed harnesses receive the same dynamic files as the source.
- Treat generated harness files as mirrors. Edit the canonical source surface and
  regenerate or reinstall instead of hand-maintaining each mirror.

## Dynamic Context Service

Use a local service when context depends on mutable state, queries, or large
corpora. A good service exposes small deterministic commands such as:

```text
context status
context pack --scope <repo|skill|doc> --query <topic> --budget normal
context explain --id <symbol|task|rule>
context diff --since <rev>
```

The service should return bounded markdown or JSON with stable source handles,
timestamps, and commands to expand. AGENTS/SKILL entries then point to the command
instead of carrying a stale digest.

## Storage Model

Use SQLite for local-first context. Keep tables separated by freshness class:

- `source_index`: file path, symbol/rule id, content hash, mtime, summary.
- `session_memory`: task id, decision, outcome, source document, committed rev.
- `runtime_state`: active queue, claims, live supervisor/session facts.
- `context_pack_cache`: query, budget, input hashes, generated pack, expiry.

Do not put policy directly in the database. Store policy in committed files and
store generated summaries with source hashes so stale packs can be invalidated.

## Invalidation

Recompute dynamic packs when any source file hash, git revision, package version,
or config fingerprint used by the pack changes. Generated context should include
the source handles it used so an agent can inspect exact files when needed.
