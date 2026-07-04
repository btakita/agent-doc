---
name: Testing before upstream release
description: Always install locally and let user test before publishing to crates.io/PyPI/GitHub Releases
type: feedback
---

Do NOT publish agent-doc (or other tools) to crates.io, PyPI, or GitHub Releases until the user has confirmed the feature works in a live session.

**Why:** Released versions are harder to retract, and bugs in published releases affect downstream users. The manual testing gate ensures the feature works end-to-end before it's public.

**How to apply:** After `make check` and `cargo install --path .`, always pause and say "Installed locally. Please test the feature before I publish upstream." Only proceed to `cargo publish` / `gh release create` after explicit user confirmation.
