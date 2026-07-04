---
name: Update VERSIONS.md before every release
description: Always update VERSIONS.md in agent-doc submodule before publishing a new version
type: feedback
---

Always update `src/agent-doc/VERSIONS.md` before each agent-doc release. Add an entry for the new version with a summary of changes.

**Why:** User explicitly requested this as a mandatory pre-release step. VERSIONS.md serves as the human-readable changelog.

**How to apply:** In the release process, after committing feature changes and before bumping the version number, add the new version entry to the top of VERSIONS.md (below the header, above the previous version).
