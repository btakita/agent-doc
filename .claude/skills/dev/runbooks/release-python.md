# Release: Python

Publication targets: PyPI only

## Steps

1. Run pre-release checklist (see `runbooks/pre-release-checklist.md`)
2. Build: `python -m build` or `make build`
3. Publish: `TWINE_PASSWORD="$(passage pypi/publish_token)" twine upload dist/*`
4. Verify: `pip index versions <package>`
5. Update parent submodule pointer
