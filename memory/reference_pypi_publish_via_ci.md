---
name: agent-doc PyPI publishing via CI (not local maturin)
description: PyPI releases for agent-doc come from .github/workflows/pypi.yml on release:published, NOT local maturin publish
type: reference
originSessionId: faf95615-ccc7-4883-80f2-7ffc0c379bb9
---
PyPI publishing for agent-doc happens via `.github/workflows/pypi.yml`, which fires on `release: published`. The workflow uses `PyO3/maturin-action@v1` with `manylinux: auto` inside a manylinux container, producing PyPI-compatible wheels for Linux/macOS/Windows.

**Local `maturin publish` cannot produce manylinux wheels on Arch/CachyOS** — the system libc is too new, so wheels get tagged `linux_x86_64` which PyPI rejects. This is **expected** and not a bug. Skip local `maturin publish` entirely; after `gh release create v<version>`, the CI workflow handles PyPI.

**Verify PyPI publish:** `pip index versions agent-doc` after workflow completes, or `gh run list --workflow=pypi.yml`.

**Corrects prior memory:** earlier "PyPI publish blocked by unsupported platform tag" observation (e.g., 2026-04-18 #18985) referred to the LOCAL maturin attempt, not to overall release status. v0.33.1, v0.33.2, v0.33.3 all reached PyPI via CI.
