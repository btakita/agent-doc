---
agent_doc_session: tsift-v0.1
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: stop
---

## Status

<!-- agent:status patch=replace -->

complete
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260610-233419.md`*

Compacted content:
- Archived 7 response topic(s): queue duplicate cleanup; codebase-memory-mcp evaluation; graph-db snapshot artifact prototype; 4 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: install/trust UX comparison — gpt-5

Completed `#cbmtrust` as a design comparison only; no tsift source or Rust architecture changed. Upstream codebase-memory-mcp has a stronger single-command trust story today: the README advertises platform archives, static binaries, SHA-256 checksums, setup scripts, AUR install, and agent-mediated install; its current `install.sh` detects OS/arch, refuses non-HTTPS download URLs except localhost, downloads a latest-release archive, verifies `checksums.txt`, installs the binary, and then runs `codebase-memory-mcp install -y` unless `--skip-config` is passed. Its security policy also claims release-time SLSA provenance, Sigstore/cosign signatures, SBOMs, SHA-256 checksums, VirusTotal blocking scans, OpenSSF Scorecard, native AV checks, CodeQL, fuzzing, and other release gates. The multi-agent UX matters because v0.5.0 release notes describe auto-configuring multiple coding agents, and the later security-hardening issue says `install --plan` / a machine-readable install receipt was added so config/hook writes can be inspected before mutation. Sources checked: https://github.com/DeusData/codebase-memory-mcp, https://raw.githubusercontent.com/DeusData/codebase-memory-mcp/main/install.sh, https://raw.githubusercontent.com/DeusData/codebase-memory-mcp/main/SECURITY.md, https://github.com/DeusData/codebase-memory-mcp/releases, https://github.com/DeusData/codebase-memory-mcp/issues/397, and https://github.com/DeusData/codebase-memory-mcp/issues/388.

tsift already has the right architectural base but a thinner trust wrapper. Its release workflow is tag-driven, checks that all crate versions match the `vX.Y.Z` tag, runs the full CI/package checks, publishes GitHub release archives for Linux x86_64, macOS arm64, and Windows x86_64, attaches per-asset `.sha256` files, publishes crates.io packages, and publishes the OpenCode npm plugin through OIDC trusted publishing. The local `scripts/install.sh` verifies the downloaded archive against the matching `.sha256` before copying `tsift` into `~/.local/bin`, but it only covers Linux x86_64 and macOS arm64, does not expose attestation/SBOM/scan verification, and does not configure agents. Agent integration is intentionally separate: `tsift init` writes managed `AGENTS.md`/`CLAUDE.md` instruction sections, can install a Codex `UserPromptSubmit` hook, and can install OpenCode commands. Those config writes are already idempotent and marker-aware: existing managed blocks become `already present` or `updated`, duplicates are removed for Codex hooks, and unmanaged OpenCode command files fail closed instead of being clobbered. Local sources checked: `src/tsift/scripts/install.sh`, `src/tsift/specs/release-integration.md`, `src/tsift/.github/workflows/release.yml`, `src/tsift/packages/tsift-index/src/init.rs`, and `src/tsift/packages/opencode-tsift/src/install.js`.

Recommendation: copy the trust UX, not the C/static-binary architecture. Keep tsift's Rust workspace, Cargo install path, SQLite graph store, and split crates. Add a thin trust layer in three pieces: first, extend releases with an aggregate `checksums.txt`, GitHub build provenance attestations, an SBOM, and documented `gh attestation verify` / checksum verification steps; add scan gates only where they are deterministic enough not to create noisy release blocks. Second, add a JSON plan/receipt mode to the existing init path, e.g. `tsift init --plan --codex --opencode [--workspace]`, returning planned files, hook events, command sources, created/updated/already-present/conflict actions, redacted paths where appropriate, `writes_started=false`, and the next apply command. Third, let `scripts/install.sh` remain a binary installer by default but optionally chain to that plan/apply flow, for example `--agent-config codex,opencode --plan` and `--agent-config codex,opencode --yes`, so agents can verify binary provenance and config mutations in one flow without moving config-writing logic out of Rust.

Priority order for tsift: `init --plan`/receipt first, because it directly addresses sensitive agent config upserts and can reuse the current idempotent init code; release attestations/SBOM second, because they strengthen binary trust without changing runtime behavior; installer chaining third, because it is mainly UX once the binary and init plan surfaces are trustworthy. Verification: no build/test run was needed because this cycle made no source, spec, or workflow edits. Commands/proof: `agent-doc preflight`, `tsift status`, `tsift --envelope source-read`, `tsift --envelope symbol-read`, `tsift --envelope search`, local release/install file reads, `git status`, and upstream primary-source review. Escalation: none.
<!-- agent:boundary:ebc86f60 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- What are #next-steps to fix bugs in tsift?
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#g5kw] What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
- [ ] [#4z0w] Can we increase max-hop caps?
- [ ] [#cg52] Could FalkorDB help with performance?
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
