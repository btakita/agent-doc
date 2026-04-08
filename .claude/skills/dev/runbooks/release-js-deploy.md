# Release: JS/TS Apps

Publication targets: Docker registry + VPS deploy (no npm publish)

## Steps

1. Run pre-release checklist (see `runbooks/pre-release-checklist.md`)
2. `make docker-build && make docker-push`
3. `make deploy`
4. Verify: health check endpoint or `make status`
