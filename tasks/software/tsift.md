---
agent_doc_session: tsift-v0.1
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#bugs-observed': Please add any bugs from observed behavior to the backlog.,
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

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260619-175702.md`*

Compacted content:
❯ - Archived 2 response topic(s): #kgsameas; #kgconfgate
❯ - Prior summary/context: prior compacted content: Archived 4 response topic(s): #next-steps to fix tsift bugs; #kgentitycollapse; #kgconfrank;...

### Re: release + publish — v0.1.71 is published except the operator-gated crates.io step — opus-4-8

> **Queue prompt:**
>
> :pushpin: release + publish

The release is already done as far as an agent can take it — there is no new agent-doable release work this cycle.

**v0.1.71 state (tsift submodule):**
- HEAD = tag `v0.1.71` = `396bbdc` (`chore: release v0.1.71`); working tree clean, in sync with `origin/main`. Tag is pushed.
- CI green. Installed binary `tsift 0.1.71` (matches the release).

**Release workflow run `27841350649` — 6 of 7 jobs succeeded:**
- ✅ `publish GitHub release` — `v0.1.71` live with all three platform artifacts (`x86_64-unknown-linux-gnu`, `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`) + `.sha256` sidecars.
- ✅ `publish OpenCode npm plugin`.
- ✅ all three cross-target builds + `verify release inputs`.
- ❌ `publish crates.io package` — the **only** failure:
  ```
  Failed to retrieve token from Cargo registry. Status: 400.
  Error: No Trusted Publishing config found for repository `btakita/tsift`.
  ```

**This failure is `#vmax`, not a code/release-config bug.** The OIDC migration is already shipped (`#1td4`, `rust-lang/crates-io-auth-action@v1`); the auth handshake reached crates.io and was rejected only because no Trusted Publisher is registered. That registration is **operator-gated** — it needs a crates.io login + browser 2FA to register the Trusted Publisher for each of the 23 crates (agent cannot perform). Once the operator completes `#vmax` (register publishers → force-move `v0.1.71` to HEAD → re-push to re-trigger), the crates.io job authenticates via OIDC and publishes; no code change is required.

**Not auto-looping the remaining `release + publish` queue items** — they request the same already-completed release, and the sole outstanding step (crates.io) is operator-gated under review `#vmax`. Re-running would be a no-op against a clean tree at the already-tagged HEAD.

### Re: release + publish (duplicate head) — no new work — opus-4-8

> **Queue prompt:**
>
> release + publish

Duplicate of the queue head answered immediately above. The v0.1.71 release is published (GitHub Release + OpenCode npm + all binaries); the only outstanding step is the crates.io publish, which failed solely because no OIDC Trusted Publisher is registered for `btakita/tsift` — the operator-gated `#vmax`. Working tree is clean at the already-tagged `v0.1.71` HEAD, so there is nothing for a second release pass to do. Queue drained; not auto-looping.

### Re: Follow crates.io/docs/trusted-publishing to setup trusted publishing — done + verified — opus-4-8

> **Queue prompt:**
>
> :pushpin: Follow https://crates.io/docs/trusted-publishing to setup trusted publishing.


> **Queue prompt:**
>
> Follow https://crates.io/docs/trusted-publishing to setup trusted publishing.

Done — this completes the operator-gated `#vmax`/`#1td4` that was the sole remaining gap in the v0.1.71 `release + publish`. Crates.io Trusted Publishing is now configured for **all 23 tsift crates** and the previously-failing OIDC publish is live.

**What I did** (via your authenticated crates.io browser session, driven over chromium-bridge against the crates.io tab so your active tab was never hijacked — confirmed logged in as `btakita`):
- Registered a GitHub Actions Trusted Publisher for **all 23 crates** in `release.yml`'s publish list — `repository_owner=btakita`, `repository_name=tsift`, `workflow_filename=release.yml`, no environment — via `POST /api/v1/trusted_publishing/github_configs`. All 23 returned HTTP 200.
- Verified persistence: a follow-up GET per crate returned **23/23** with exactly one config each = `btakita/tsift:release.yml`.
- Gate var `TSIFT_ENABLE_CRATES_PUBLISH` was already `true` (set 2026-05-21), so no change needed.

**Verification (the actual publish):**
- Re-ran the failed `publish crates.io package` job from the v0.1.71 release run (`27841350649 --failed`) rather than churning a new tag/GitHub-release/npm pass.
- The **"Authenticate to crates.io via OIDC trusted publishing" step now succeeds** — this is the exact step that previously failed with `No Trusted Publishing config found for repository btakita/tsift`.
- The publish is live via OIDC: `tsift-core` already moved to `0.1.71` on crates.io; the remaining crates publish in dependency order (each waits for crates.io to index its dependency). The full 23-crate publish is finishing in the background.

No code or workflow change was required — the OIDC migration (`#1td4`, `rust-lang/crates-io-auth-action@v1`) was already shipped; it only needed the per-crate Trusted Publisher registration, which is what this did. Future `v*` releases now publish to crates.io automatically with no token/secret.
<!-- agent:boundary:26a77da7 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#kgconfreqverify] [operator-verify] Verify the confidence-required schema change (#kgconf, payload_schema in packages/tsift-kg/src/ollama.rs) does not reduce extraction yield on the live local model (Qwen3-32B-Q4) via an A/B over a real corpus; if required-confidence cuts entities/relations or induces hallucinated scores, relax to prompt-only + the derived-default fallback. Needs GPU + Ollama (operator-gated).
- [/] [#vmax] [operator-verify] Enable crates.io OIDC publishing for tsift (advances review #1td4; code already shipped in fe4144b/f29f7a2, release.yml crates job uses rust-lang/crates-io-auth-action@v1). Operator-gated steps (need crates.io account + browser 2FA — agent cannot perform): (1) On crates.io register a Trusted Publisher for EACH of the 23 tsift crates — GitHub repo btakita/tsift, workflow release.yml, no environment: tsift-core, tsift -md-ast, tsift-graph, tsift-sqlite, tsift-algorithms, tsift-resolution, tsift-cache, tsift-tokensave, tsift-libsql, tsift-index, tsift-summarize, tsift-quality, tsift-agent-doc, tsift-digest, tsift-search, tsift-status, tsift-session, tsift-memory, tsift-memgraphrag, tsift-surrealdb, tsift-cli, tsift, tsift-sim-world. (2) Enable the gate var: gh variable set TSIFT_ENABLE_CRATES_PUBLISH --body true. (3) Force-move the v0.1.70 tag to HEAD and push to re-trigger; confirm the publish-crates.io job authenticates via OIDC (no CARGO_REGISTRY_TOKEN) and publishes.
- [/] [#1td4] Release infra: crates.io publish job in .github/workflows/release.yml is MIGRATED to OIDC trusted publishing (commit fe4144b, on origin/main f29f7a2; uses rust-lang/crates-io-auth-action@v1, id-token: write, no CARGO_REGISTRY_TOKEN). Code complete. Remaining is operator-gated and tracked as actionable backlog #vmax: register the Trusted Publisher per crate on crates.io (23 crates), set repo var TSIFT_ENABLE_CRATES_PUBLISH=true, then re-tag v0.1.70 to verify OIDC publish. Stays gated until operator completes crates.io registration.
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
