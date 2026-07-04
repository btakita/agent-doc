---
name: Recursive submodule commits
description: When committing all submodules, commit nested submodules recursively (inside-out), not just top-level
type: feedback
---

When asked to "commit all submodules", commit recursively — check for dirty nested submodules inside each submodule and commit inside-out.

**Why:** User expects all dirty state cleaned up, including nested submodules like `src/briantakita.me-dev/app/briantakita.me`. Skipping nested submodules leaves visible `m` (modified) markers.

**How to apply:** Before committing a submodule, run `git -C <submodule> submodule status` to detect dirty nested submodules. Commit those first, then the parent, then update the outer pointer.
