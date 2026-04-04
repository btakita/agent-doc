---
agent_doc_session: bef800bc-950e-4f9e-b861-92c249853397
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
v0.31.5 released. Commit-on-claim, auto-setup, VCS refresh, --diff-only, skill runbooks, .ai/runbooks convention.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
*Compacted. CI workflows idempotent (release.yml --clobber, pypi.yml skip-existing). `make release` added (CI-first: check→tag→push). Prerelease runbook updated. Gitignore fixed (.bin/ .cargo/ separate). All pushed.*
<!-- agent:boundary:930e73cc -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] Release agent-doc v0.31.9
- [x] Add version sync check to `make check`
- [x] Make CI release workflows idempotent
- [x] Add `make release` target (CI-first flow)
- [x] Push agent-doc changes to remote
- [x] Fix gitignore (.bin/ and .cargo/ separate entries)
<!-- /agent:pending -->
