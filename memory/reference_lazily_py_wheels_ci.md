---
name: lazily-py wheels CI publish (mypyc + cibuildwheel)
description: lazily-py publishes mypyc-compiled wheels to PyPI via .github/workflows/wheels.yml on tag, using PYPI_PUBLISH_TOKEN (passage btak/PYPI_PUBLISH_TOKEN)
metadata:
  type: reference
---

lazily-py ships **mypyc-compiled wheels for every platform** via CI, not local
builds. Local `python -m build` on this host only produces a `linux_x86_64` wheel
(PyPI rejects `linux_x86_64`; needs `auditwheel repair` → manylinux) and can't
build mac/windows — so CI is the publish path.

## Wheels workflow: `.github/workflows/wheels.yml` (src/lazily-py)

- **Trigger:** tag push `v*` (+ `workflow_dispatch` for artifact-only runs, and
  PRs touching wheels.yml/pyproject/setup.py).
- **Build:** `cibuildwheel` matrix — `ubuntu-latest` (manylinux+musllinux x86_64),
  `ubuntu-24.04-arm` (manylinux+musllinux aarch64), `windows-latest` (win_amd64),
  `macos-14` (cross-compiles **both** arm64 + x86_64 via `CIBW_ARCHS_MACOS` — do
  NOT use `macos-13`, those Intel runners are deprecated and stall ~40 min).
  Builds cp312/313/314 (`[tool.cibuildwheel] build` in pyproject.toml). Each
  wheel smoke-tests `from lazily import slot, cell, signal, effect, batch`.
- **Publish:** `pypa/gh-action-pypi-publish@release/v1` with
  `password: ${{ secrets.PYPI_PUBLISH_TOKEN }}` (token auth, **not** OIDC).
  Gated `if: startsWith(github.ref, 'refs/tags/v')`.
- **mypyc build:** `setup.py` runs `mypycify` over the 5 core modules as one
  compilation unit; public classes use `@mypyc_attr(allow_interpreted_subclasses=True)`
  (the `Py_TPFLAGS_BASETYPE` equivalent) so `class HttpClient(Slot[...])` keeps
  working. `build-system.requires` includes `mypy[mypyc]>=2.3`. Pure-Python
  fallback if a target's toolchain is unavailable.

## The token

Repo secret `PYPI_PUBLISH_TOKEN` = passage `btak/PYPI_PUBLISH_TOKEN` (mirrored at
`pypi/publish_token`; token is `pypi-…`, `__token__` auth). **Same key Claude/Codex
use** for `twine upload`. Set/refresh it non-interactively:

```bash
gh secret set PYPI_PUBLISH_TOKEN --repo lazily-hub/lazily-py \
  --body "$(passage show btak/PYPI_PUBLISH_TOKEN)"
```

(OIDC trusted publishing is NOT configured for lazily-py on pypi.org; agent-doc
uses OIDC, but lazily-py uses the token. Do not switch to OIDC without first
adding the trusted publisher on pypi.org.)

## Release flow (hands-off as of v0.31.2+)

1. Bump `pyproject.toml` version + `src/lazily/__init__.py __version__` + CHANGELOG.
2. `make check` green.
3. `git push origin main` → `git tag -a vX.Y.Z -m vX.Y.Z` → `git push origin vX.Y.Z`.
4. `wheels.yml` builds the matrix and publishes automatically. Verify:
   `curl -s https://pypi.org/pypi/lazily/<ver>/json | grep filename` (expect
   ~21 wheels + 1 sdist).

**History:** v0.31.0 (2026-07-17) shipped only cp312-linux + sdist (manual `uv
publish` + `auditwheel repair`). v0.31.1 added the full matrix (21 wheels + sdist)
via CI artifacts downloaded + `uv publish` locally. **v0.31.2 (2026-07-17) =
first fully CI-published release — verified end-to-end:** `git tag v0.31.2` →
`wheels.yml` built the 21-wheel matrix + sdist and the `publish` job uploaded all
to PyPI via `PYPI_PUBLISH_TOKEN` in 54s, no manual step. Latest = 0.31.2.

Relates to [[reference_lazily_release_targets]] (py row), [[reference_pypi_publish_via_ci]]
(agent-doc OIDC), [[reference_cargo_publish_token_passage]] (parallel cargo flow).
