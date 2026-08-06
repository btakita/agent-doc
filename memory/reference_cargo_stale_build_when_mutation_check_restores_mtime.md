---
name: reference_cargo_stale_build_when_mutation_check_restores_mtime
description: "A mutation-check script that restores files by moving a backup over them keeps the OLD mtime, so cargo reuses the mutated artifact and the next run lies"
metadata: 
  node_type: memory
  type: reference
  originSessionId: a5a9e782-82c8-4457-962f-9adae81c9c8a
  modified: 2026-07-26T18:56:55.064Z
---

Mutation-checking a Rust gate by patching a source file, running the test, then
restoring from a backup **silently poisons every later run** if the restore
preserves the backup's mtime. `shutil.move(path + ".bak", path)` and `cp -p` both
do. Cargo decides freshness by mtime, sees a file *older* than the built
artifact, and keeps the **mutated** rlib — so the clean tree reports failures
that no longer exist in the source, and `git diff` shows nothing wrong.

Symptom that gave it away: `queue_family_conformance` went from 20 passed to 2
failed with `invalidates.is_full` after a mutation sweep, while
`git status src/` was clean and the mutated line was visibly back in the file.

Fix: `touch` the sources after every restore and before every run.

```python
shutil.move(path + ".bak", path)
os.utime(path, None)          # <- without this the next build is stale
```

This also invalidates the sweep's own verdicts if any probe happened to write
with an old mtime — re-run the whole sweep after adding the touch. One probe in
that first sweep reported UNCAUGHT and was actually caught once rebuilds were
forced.

Related to [[reference_cargo_check_intercepted_returns_stale_digest]] — same
class of failure (cargo confidently returning a result for code that is not on
disk), different cause.
