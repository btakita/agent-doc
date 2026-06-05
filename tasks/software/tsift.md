---
agent_doc_session: tsift-v0.1
resume: 019e85d7-f9ee-79f3-b597-4ed352495bbd
agent: claude
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
queue: start
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260605-064424.md`*

Compacted content:
- Archived 6 response topic(s): #cyba; #hxgp, #9tg6, tsift+agent-doc design; hard dependency analysis; 3 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: #0y9e publish opencode-tsift to npm — opus-4-8

Attempted the publish; **npm rejected it (E403)** — blocked on an account/token policy, not the package. Nothing was published.

#### What I did
- `opencode-tsift@0.1.62` is **not yet on npm** (clean first publish — no version conflict). Dry-run pack is healthy: 20 files (`bin/`, `commands/`, `src/`, `package.json`), 6.4 kB.
- Ran `npm publish --access public` from `packages/opencode-tsift`, authenticating token-safe via `passage btak/NPM_TOKEN` (literal `${NPM_TOKEN}` in a temp `.npmrc`, no cleartext, removed after).

#### The blocker
```
npm error 403 Forbidden - PUT https://registry.npmjs.org/opencode-tsift
Two-factor authentication or granular access token with bypass 2fa enabled is required to publish packages.
```
The token at `passage btak/NPM_TOKEN` is a **classic/automation token**; your npm account enforces 2FA on publish, which classic tokens can't satisfy.

#### To unblock — pick one
1. **Granular access token (recommended).** npmjs.com → Access Tokens → *Generate New Token → Granular* → Permissions: **Packages and scopes = Read and write** (all packages, or scope to `opencode-tsift`). Granular tokens bypass 2FA for automation. Store it at `passage btak/NPM_TOKEN` (replacing the classic one), then re-run this item.
2. **One-time OTP.** Give me a current authenticator code and I'll publish immediately with `npm publish --otp=<code>` (valid ~30s).
3. **CI path.** Set GitHub repo secret `NPM_TOKEN` (granular) + variable `TSIFT_ENABLE_NPM_PUBLISH=true`, then re-push tag `v0.1.62` — the `publish OpenCode npm plugin` job in `release.yml` will publish. (Currently neither the secret nor the var exists in GitHub; only the crates.io pair is set.)

Gating `#0y9e` until a publish-capable token or OTP is available. The agent-doc `release + publish` item remains separately gated under the stopped queue.
<!-- agent:boundary:02926f00 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- ~complete the wip~
- NPM_TOKEN secret added to https://github.com/btakita/tsift/settings/secrets/actions
- release + publish
- ~do [#0y9e]~
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog queue go -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#0y9e] I set NPM_TOKEN="$(passage btak/NPM_TOKEN)". Publish to npm.
- [/] [#astnav-npm] Configure NPM_TOKEN + TSIFT_ENABLE_NPM_PUBLISH secrets, publish opencode-tsift to npm (medium)
- [/] [#surreal-batch-bfs] #surreal-batch-bfs-gated: Batch frontier expansion - gated (low)
- [/] [#surreal-fixture] Record SurrealDB samples in graph-db-performance-history.json fixture (medium)
- [/] [#surrealbulk-close] Close #surrealbulk review item: re-run benchmarks, update SPEC.md benchmark table, mark done
- [/] [#rankdefault] Promote ranked_neighborhood into default graph lookup/context-pack/evidence output only after quality gates pass; compare ranked vs unranked output on real backlog/code queries for useful-hit density, duplicate-name precision, handle coverage, latency, and emitted tokens.
- [/] [#surrealbulk] Batch SurrealDB projection load: replace per-row upsert loops with bulk/transactional row load plus one-pass secondary-index rebuild; measure refresh/load separately.
- [/] [#full-e2e-verify] In fresh project: opencode plugin opencode-tsift && opencode then /tsift-status. On success mark #e2e-install done.
- [/] [#npm-publish-verify] After auth configured, tag release and verify publish-opencode-plugin CI job succeeds and opencode-tsift appears on npmjs.org
- [/] [#npm-auth-setup] Configure NPM_TOKEN secret + TSIFT_ENABLE_NPM_PUBLISH=true variable in GitHub repo settings
- [/] [#e2e-install] Verify end-to-end install: fresh opencode project + opencode plugin opencode-tsift + /tsift-status
<!-- /agent:review -->
###
<!--
Can we increase max-hop caps?
Could FalkorDB help with performance?
-->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
