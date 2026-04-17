---
agent_doc_session: tsift-v0.1
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
Project created. Repo: https://github.com/btakita/tsift. Submodule: src/tsift. Rust CLI skeleton pushed.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->

<!-- agent:boundary:ca439f2d -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Wire Sift as search backend (shell-out to `sift` binary, parse output)
- [ ] Batch edit command: accept multiple file edits in one invocation
- [ ] Agent model routing skill: haiku for search, sonnet for edits, opus for architecture
- [ ] SQL introspection: connect to SQLite/Postgres, expose schema + query
- [ ] Claude Code hook integration: rewrite commands through tsift like rtk does
- [ ] CLAUDE.md with project conventions
- [ ] CI: GitHub Actions for cargo test + clippy
- [ ] Release workflow: crates.io + GitHub Releases with prebuilt binaries
<!-- /agent:pending -->
