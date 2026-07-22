---
agent_doc_session: d2691213-7160-4943-9751-c954341df959
resume: aff3725f-f537-4991-9ede-2d5b54545e7d
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
agent_doc_model_tier: high
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md. DO NOT IMPLEMENT THE FIX.
  '#commit-push': commit all uncommitted files
  '#commit-push-deploy': commit all uncommitted files + deploy
  '#mermaid': Include explanatory mermaid diagram(s).
  '#spec-test-commit-push': update spec + tests. commit + push
  '#spec-test-commit-push-deploy': update spec + tests. commit + push + deploy
queue: go
---

# brookebrodack-dev — Session Summary

## Completed Work

### 1. Submodule Setup
- Added `btakita/brookebrodack-dev` as git submodule in agent-loop (replaced initial `brookebrodack-site` standalone)
- 21 nested submodules initialized

### 2. Dependency Upgrades
All packages upgraded in `app/brookebrodack-site/package.json`:
- elysia 1.0→1.4, drizzle-kit 0.23→0.31, drizzle-orm 0.32→0.45, marked 13→17, tailwindcss 3→4, postcss 8.4→8.5
- Added `@sinclair/typebox ^0.34.48` (Elysia 1.4 requirement)
- Added `better-sqlite3` (drizzle-kit CLI requirement)
- Fixed `drizzle.config.ts`: `driver: 'libsql'` → `dialect: 'sqlite'`

### 3. Environment Setup
- Populated `.env` from Contabo Docker container (`brookebrodack-www-1`)
- Keys: `GOOGLE_API_KEY`, `YOUTUBE_CHANNELID`, `YOUTUBE_RSS_URL`, AWS S3 credentials
- Removed `AUTHORY_YOUTUBE_RSS_URL` (feed was empty, contributed zero data)

### 4. Cloudflare Pages Migration (All 7 Phases)

| Phase | Status | Details |
|-------|--------|---------|
| 1. Static build | Done | `app/brookebrodack-site/static-export.ts` — pre-renders all routes |
| 2. GH Actions CI/CD | Done | `.github/workflows/deploy.yml` — push + cron (6h) + workflow_dispatch |
| 3. Assets | Done | Downloaded from Contabo S3 in CI (no R2 needed) |
| 4. YouTube data | Done | Baked at build time via Google API |
| 5. DNS cutover | Done | CNAME → `brookebrodack.pages.dev` |
| 6. Scheduled rebuilds | Done | Cron every 6h |
| 7. WebSub worker | Done | `workers/websub/` with auto-renewal cron (7 days) |

### 5. Authory Removal
- Deleted `authory_youtube_rss_cache.ts` and all references
- Feed was empty (0 items), only caused unnecessary cache invalidation
- Removed from env, deploy.yml, and GitHub secrets

### 6. Infrastructure State

**Live at:** `https://brookebrodack.net/` (CF Pages, HTTP 200)

**GitHub secrets** (9 total for `btakita/brookebrodack-dev`):
`CLOUDFLARE_API_TOKEN`, `CLOUDFLARE_ACCOUNT_ID`, `GOOGLE_API_KEY`, `YOUTUBE_CHANNELID`, `YOUTUBE_RSS_URL`, `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_ENDPOINT_URL`, `AWS_BUCKET`

**WebSub worker:** `https://brookebrodack-websub.brian-takita.workers.dev`
- Secrets: `GH_TOKEN`, `WEBSUB_SECRET`
- Cron: `0 0 */7 * *` (auto-renewal)
- YouTube subscription active

**DNS records** (brookebrodack.net zone):
- `brookebrodack.net` CNAME → `brookebrodack.pages.dev`
- `www.brookebrodack.net` CNAME → `brookebrodack.pages.dev`

**Contabo decommissioned:** Docker container `brookebrodack-www-1` stopped and removed. `ssh.brookebrodack.net` A record deleted.

### 7. Key Commits
- `4ecb1e6` (domain--server--brookebrodack): `refactor: remove authory youtube rss dependency`
- `bf45fcd` (brookebrodack-dev): `refactor: remove authory dependency, add websub auto-renewal`
- `69ce282` (brookebrodack-dev): `fix: add asset download step to CI workflow`
- `7c4e1af` (brookebrodack-dev): `feat: Cloudflare Pages migration`
- `5f3d6ed` (brookebrodack-site): `feat: add static export script`

## Follow-up Items
- Replace `GH_TOKEN` in WebSub worker with a long-lived GitHub PAT (`actions:write` scope) if current `gh` CLI token expires
- Add static export to relysjs framework (tracked in `tasks/relysjs.md`)

<!-- agent:status mode=replace -->
- [x] 190 blog posts with AI summaries + Whisper transcripts
- [x] Yalc migration (10 `@rappstack/*` packages)
- [x] Server fixes (graceful errors, 404 for missing slugs)
- [x] SEO: sitemap (194 URLs + video extensions), VideoObject JSON-LD
- [x] Mailchannels DNS records + WebSub email notifications deployed
- [x] 23 automated tests
- [x] CI yalc→npm fallback for GitHub Actions
- [ ] `ANTHROPIC_API_KEY` GitHub secret (optional, for CI post generation)
- [x] Guestbook rebuilt: page + admin dashboard + Pages Functions API on D1
- [x] Hold-for-approval moderation; existing entries approved
- [x] Deploy guards: content-asserting smoke test + guestbook API check
- [x] Moderation switched to local hermes agent, OpenAI fallback (14/14 verified)
- [ ] Set `GUESTBOOK_ADMIN_PASSWORD` / `GUESTBOOK_ADMIN_SECRET` Pages secrets
- [ ] Deploy the guestbook (nothing committed or deployed yet)
- [ ] `hermes -z` is broken upstream (0.18.2), so the primary path cannot run yet
- [ ] `OPENAI_API_KEY` for the moderation fallback — none is scoped to this project
<!-- /agent:status -->

<!-- agent:exchange mode=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/5ed9ebdbe4a2edb795318ea94faf66a77813df7180cde01350e66214e980296b-20260719-070224.md`*

Compacted content:
- Archived 2 response topic(s): do [#tsnextpin]; do [#4s08]
- Prior summary/context: prior compacted content: Archived 1 response topic(s): do [#rappsqlite]

### Re: do [#ciappts] — opus-4-8

> **Queue prompt:**
>
> do [#ciappts]


I did the work this was waiting on, but I did **not** uncomment the steps — and the reason is worth reading, because I nearly shipped a red CI step.

**What the item was waiting for.** `#ciappts` was gated on `#tsbrowser` and `#rappts` clearing. I measured rather than assumed: `typecheck:browser` was at 21 errors and the app type-check at 24, and **zero** of them were in code this repo owns. All of them were inside `node_modules/@rappstack/*`, which ships `.ts` sources, so `skipLibCheck` never applies. The drizzle cluster from the original note was already gone — `#rappdeps` had landed and rappstack is aligned on `drizzle-orm ^0.45.1` throughout.

So the remaining blocker was upstream code, and I fixed it in `/home/brian/work/rappstack/dev`:

- **`@rappstack/ui--any` — `MarkedPlaintextRenderer` (17 errors).** This is the substantive one. marked 13 handed each renderer method its already-rendered child strings; marked 17 hands it the token object and expects the method to render children itself via `this.parser`. Every method still had the 13 signature. That is not only a type error against the base class — none of those methods would have received what they expected at runtime either. I ported all of them: block methods parse their tokens (`blockquote`, `listitem`), inline ones use `parseInline`, and `list`/`table` now build their bodies from `token.items` and `token.header`/`token.rows`. Plaintext output is unchanged. Also dropped the second `parser.parse(tokens, loose)` argument — marked 17's instance method takes one.
- **`domain--server--blog/post/Post__schema.ts` (3).** zod 4 removed `errorMap` in favour of `error`, and dropped the `invalid_date` issue code. Worth noting: that means the custom `'Wrong date format.'` message had **already silently stopped firing** — the handler could never match. Remapped to `invalid_type`.
- **`domain--server/jsonld`, `rdfa`, `domain--server--blog/breadcrumb`, `ui--any/dl` (4).** `request_url__pathname_` returns `string | undefined` and `url__join` takes no undefined segments; asserted the same way the sibling `site__website_` calls already do. In `dl`, `typeof x === 'function'` cannot narrow a union that contains `Function` itself, so the recursion needed the explicit cast the rest of that file already uses.

**Result against the local yalc store:** `typecheck`, `typecheck:browser`, and the app type-check are all green. Lint green. 67 tests + 9 dom tests pass. (The one `routes.test.ts` failure I hit first was the documented missing-`build` prerequisite, not a regression — it passes after `bun run build`, exactly as the workflow's own comment says.)

**Why the steps are still commented out.** The `Resolve yalc dependencies to npm` step rewrites `file:.yalc/@rappstack/*` to `"^0.1.0"`, so **CI resolves the published packages, not my local fixes**. I didn't want to assert that from reasoning, so I reproduced it: copied the tracked tree to a scratch dir, applied the same `sed`, installed, and re-ran — **31 errors**. Gating now would have produced a red step on day one, which is the precise failure the original comment warned against.

So the blocker has gone from "39 errors across two root causes" to one action: publish those three packages to npm. That needs 2FA, which is yours. I filed it, and gated `#ciappts` behind it.

**One more thing I found:** rappstack has the same disease `#tsnextpin` just cured in ctx-core-dev — 13 packages pin `typescript: "*"`, 2 pin `"next"`, and the installed compiler is a **5.5.0-dev from April 2024** that hard-crashes `tsc` outright (`TypeError: Cannot read properties of undefined (reading 'flags')`). That is why these errors were only ever visible from downstream repos: rappstack cannot currently type-check itself. Filed separately.

**Commits:** rappstack `7cde3f8`, plus two sibling package commits and superproject `9ddb68d`; brookebrodack-dev `86eef0f`. Nothing pushed.

### Re: #yalcdrift — opus-4-8

Closing this one, because the rappstack work above resolved it and the recorded diagnosis is now out of date. `session-check` flagged it as still open, so here is the measurement rather than an assertion.

**What the item said:** a fresh `bun install` re-resolves marked 13.0.3 alongside 17.0.4, the two `ctx/*.ts` `Marked<string, string>` vs `Marked` errors come back, and the app type-check goes to 62 errors with 2 in its own sources. Root cause recorded as "one yalc'd @rappstack package pins marked ^13.0.2 while two pin ^17.0.4".

**What is true now.** I copied the tracked tree to a scratch dir, deleted `bun.lock` entirely, and re-installed from scratch:

- The dual resolution **persists** — `marked@13.0.3` and `marked@17.0.6` both land.
- The app type-check is nonetheless **green**, exit 0. The two `ctx/*.ts` Marked errors are gone.

So the symptom this item existed to track is fixed, and the stated root cause is no longer accurate. All three rappstack packages that ship marked (`ui--any`, `ui--server`, `dev-bin`) are on `^17.0.4` — the `#rappdeps` sweep aligned them. The `^13.0.2` edge that remains comes from `@btakita/dev-bin@0.1.0` on npm, which is a separate published package and only a bin-tool dependency; it no longer reaches the type program. brookebrodack's own `bun.lock` also still carries a stale metadata entry for the yalc'd `@rappstack/ui--any` claiming `^13.0.2`, which is why the split looked like a rappstack problem — the local yalc source says `^17.0.4`.

Two marked majors coexisting is untidy, but it is now inert rather than a type-check blocker, and it is not something this repo can pin away. Not filing a follow-up for it.
<!-- agent:boundary:5d323eb2 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority go -->
- ⏭️ do [#rjrh]
- 🚧 do [#nm8q]
- ~~do [#tsappclean]~~
- ~~do [#ciappts]~~
- ~~do [#yalcdrift]~~
- do [#gbdeploy1]
- do [#mc4f]
- ⏭️ do [#lzeffectvoid]
- do [#sbv9]
- do [#nd8n]
- do [#fhmig]
- do [#50fc]
- do [#y457]
- do [#j0ee]
- do [#2amm]
<!-- /agent:queue -->

## Notes

<!-- agent:notes -->
<!-- /agent:notes -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#rjrh] Publish @rappstack/ui--any, @rappstack/domain--server, and @rappstack/domain--server--blog to npm from /home/brian/work/rappstack/dev (fixes committed at 9ddb68d, currently only in the local yalc store). Needs npm 2FA. This is the sole remaining blocker on #ciappts — once published, uncomment the two type-check steps in brookebrodack-dev .github/workflows/check.yml
- [ ] 🚧 [#nm8q] rappstack/dev has the same typescript dist-tag problem #tsnextpin fixed in ctx-core-dev: 13 packages pin `typescript: "*"` and 2 pin `"next"`, and the installed compiler is a 5.5.0-dev from April 2024 that hard-crashes tsc (TypeError: Cannot read properties of undefined reading 'flags' in getCheckFlags). Pin the repo to ^7.0.2 so rappstack can type-check itself instead of relying on downstream repos to find its errors
- [ ] [#mc4f] Push the ctx-core-dev typescript pin sweep: 190 per-package submodule commits + superproject are committed LOCALLY and unpushed. Same ~190-public-repo motion as #pushctxcore, so it needs operator go before pushing each submodule and the superproject
- [ ] [#lzeffectvoid] lazily-js types EffectRun as `() => (() => void) | null | undefined`, so a block-bodied effect arrow (inferred `void`) will not type-check and every cleanup-free effect must write `return undefined`. Widen to `| void` in src/lazily-js and mirror to the other bindings; then drop the three explicit returns in lib/ui--browser--brookebrodack/guestbook/hyop/guestbook__hyop.ts. KEPT OPEN: not blocked, just not started — the 2026-07-18 esbuild/dependency-sweep prompt took that cycle, and this is a published-package type change across the binding family rather than a tail-end edit
- [ ] [#gbdeploy1] Confirm the re-run Deploy to Cloudflare Pages workflow went green (runs 29659909999 / 29660239368 were still in progress at closeout). The original run FAILED its post-deploy smoke test because the guestbook API was serving an error page — code reading deleted_at shipped before migration 0003 was applied. Prod was verified healthy by hand afterwards (GET /guestbook/api/entries → 200 application/json), so this is a green-signal confirmation, not a suspected outage
- [ ] [#sbv9] Author a seccomp BPF filter and pass it via bwrap --seccomp <fd> in src/freehold (crates/freehold-host/src/sandbox.rs). Outstanding condition on bwrap being the default sandbox — confirmed still absent (no seccomp reference anywhere in crates/). Until it lands a compromised plugin has no network/filesystem/caps/env but does have the full kernel syscall surface. Needs fd-passing through tokio Command (pre_exec/CommandExt); bwrap has no --seccomp-path variant
- [ ] [#nd8n] Add cgroup limits for the bwrap sandbox path in src/freehold via a transient systemd scope (systemd-run --user --scope -p MemoryMax= -p TasksMax=) — mem_limit_mb and pids caps are applied on the OCI paths only, so the default bwrap profile has no ceiling on a runaway plugin. Confirmed still absent: mem_limit_mb exists on PluginSpec but no systemd-run/cgroup call site
- [ ] [#fhmig] Migrate the freehold-scoped items (#sbv9, #nd8n, #dj8c, #p0rw, #k0ye, #50fc) out of tasks/brookebrodack-dev.md into tasks/software/freehold.md, whose agent:backlog is currently EMPTY. The dispatch repo was renamed + transferred to freehold-hub/freehold (submodule src/freehold); this document should keep only brookebrodack-dev site/guestbook scope. Deferred this cycle: tasks/software/freehold.md is foreign-owned by pane %27
- [ ] [#50fc] Confirm the truncated dispatch requirement — the queued prompt ends mid-sentence at "It will automatically", which leaves the plugin aggregation/automation policy (§6 of tasks/software/plan-brookebrodack-guestbook-dispatch-service.md) unspecified; blocks #k0ye. Operator input required
- [ ] [#y457] Set OPENAI_API_KEY on BOTH surfaces — `bunx wrangler pages secret put OPENAI_API_KEY --project-name=brookebrodack` (inline submit-time judging) and `cd workers/guestbook-moderate && bunx wrangler secret put OPENAI_API_KEY && bunx wrangler deploy` (cron sweeper). Without it entries just stay pending.
- [ ] [#j0ee] Pick the moderation model empirically once the key exists: `OPENAI_API_KEY=... bun scripts/guestbook-model-bench.ts gpt-4o-mini gpt-5-nano gpt-5.4-nano gpt-5.4-mini gpt-5.6-luna` and take the cheapest/fastest with UNSAFE=0, then set OPENAI_MODEL on both surfaces
- [ ] [#2amm] `hermes -z` is broken in hermes-agent 0.18.2 — every one-shot run dies with an httpx ResponseNotRead inside the Gemini provider, so the local primary path always falls through. Try `hermes update` or file upstream
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#ciappts] Add the app/brookebrodack-site typecheck step to .github/workflows/check.yml. Left out because it is red on 46 errors this package does not own (#tsbrowser + #rappts). Add once both clear
- [/] [#oidcnpm] Configure the npm Trusted Publisher on the rebuildjs package at npmjs.com (org rebuildjs, repo rebuildjs, workflow release.yml). Workflow is committed (bfe2707); this browser step needs 2FA ONCE and then no release needs it again. npm retires 2FA-bypass tokens for account ops early Aug 2026 and for direct publishing ~Jan 2027. Repeat for rhonojs/relysjs/ctx-core once their release workflows land; lazily-js's E404 publish failure was this same missing step. KEPT OPEN: operator-only — requires an authenticated npmjs.com browser session with 2FA
- [/] [#gpupol] Run ONE persistent local-inference server (vllm or ollama, both already installed) as the sole GPU owner, outside the sandbox. Model plugins reach it over the existing egress-proxy path with egress_hosts: ["127.0.0.1:<port>"] — no new mechanism, and /dev/nvidia* never enters the sandbox. Depends on #p0rw (the proxy must exist first). Start with ollama for its idle-unload behaviour; switch to vLLM only if sustained concurrency exceeds ~4-8 requests
- [/] [#cy58] Add credit/billing to the OpenAI project holding brookebrodack/guestbook/OPENAI_API_KEY — the key authenticates and is scoped correctly, but every chat-completions call returns 429 `insufficient_quota`, so moderation cannot run
- [/] [#dj8c] Verify Podman specifically end-to-end in src/freehold. OCI path IS verified: crates/freehold-host/tests/sandbox_live.rs round-trips a real verdict through Docker, which uses the identical sandbox_argv construction (branches differ only in binary name). BLOCKED: podman not installed; installing needs an interactive sudo password. The test skips per-runtime and covers Podman automatically once installed — then run: cargo test -p freehold-host --test sandbox_live -- --nocapture
<!-- /agent:review -->

## Icebox

<!-- agent:icebox -->
- [ ] [#53z1] Distilled embedding+MLP classifier as a reject-or-abstain plugin (never approves). Value is failure-mode diversity, not speed: an MLP over embeddings has no instruction-following channel, so it is structurally far harder to prompt-inject than an LLM — a second non-injectable voter alongside plugin-rules. GATED ON DATA: needs low-thousands of human-labelled entries; a personal guestbook will not produce that for a long time. Revisit once /guestbook/admin has accumulated real review history
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done -->
- 2026-07-18 [#64r6] Duplicate of #sbv9 (closeout-retry artifact) — seccomp BPF filter for bwrap
- 2026-07-18 [#j262] Duplicate of #nd8n (closeout-retry artifact) — cgroup limits for the bwrap sandbox path
- 2026-07-18 [#wqr2] Design done + scaffold shipped: local moderation dispatch service — github.com/btakita/brookebrodack-dispatch (Rust workspace, submodule at src/brookebrodack-dispatch). Plan tasks/software/plan-brookebrodack-guestbook-dispatch-service.md (#gbdispatch1 DONE). Remaining phases #gbdispatch2 (DO + wire + auth), #gbdispatch3 (plugin host), #gbdispatch4 (edge integration), #gbdispatch5 (tests)
- 2026-07-18 [#acnd] Commit + deploy the guestbook — the whole feature (functions/, scripts/, workers/, wrangler.toml, d1/ + 4 submodules) is still uncommitted, so prod /guestbook is still the fallback home page
- 2026-07-18 [#teah] UNANSWERED PROMPT — evaluate AI harness candidates for the dispatch actor plugins (5090 for local models; tmux-router drives Claude/Codex/opencode; hermes available) AND search for the 'newish agent that creates a custom harness that learns'. Asked in the exchange and never addressed; the turn went to the repo/sandbox work. Needs real web research — do NOT answer from recall, the agent-name recall is exactly what would be hallucinated
- 2026-07-18 [#gggp] Re-auth wrangler (`bunx wrangler login`) — the stored OAuth token has the right scopes and has not expired, but the API rejects it with "Invalid format for Authorization header [code: 6111]", blocking secret-put and deploy
- 2026-07-18 [#gbmig3] Apply d1/migrations/0003_guestbook_soft_delete.sql to the remote D1: bunx wrangler d1 execute brookebrodack --remote --file d1/migrations/0003_guestbook_soft_delete.sql (migrations are applied by hand — wrangler.toml sets no migrations_dir). Code is committed and pushed (24bec1a) but the column does not exist remotely yet, so guestbook read paths will error against prod until this runs. BLOCKED on #gggp
- 2026-07-18 [#fhquorum2] Approval is currently UNREACHABLE in freehold: APPROVE_QUORUM_MIN = 2 (freehold-tribunal/src/lib.rs) but freehold-rules is the only plugin binary in the workspace, so at most one plugin can ever answer and every entry escalates to a human. A second answering verdict plugin is required before auto-approve can fire at all — and a third if you want approval to survive the local model being down (freehold-rules abstains unless it matches a deterministic spam rule, so it cannot be relied on as one of the two)
- 2026-07-18 [#fhoneshot] When adding a CLI-agent-backed verdict plugin, invoke it ONE-SHOT per request (claude -p / codex exec, non-interactive) under the existing PluginSpec budget_ms + OCI sandbox contract. Do NOT pool or reuse sessions: retained conversation state breaks per-entry verdict independence and makes a successful prompt injection persist across future verdicts, which is the exact threat the sandbox exists to contain
- 2026-07-18 [#p0rw] Write the allowlisting egress proxy for the plugin egress hostport in src/freehold — model plugins are routed to it by sandbox_argv but it does not exist yet, so a model plugin currently has no working network path (~50-line CONNECT proxy, allowlist generativelanguage.googleapis.com). Confirmed still absent: egress_hosts appears only on PluginSpec and in tests, with no proxy binary or call site
- 2026-07-18 [#fhagentspec] Ship a PluginSpec for freehold-agent so it actually joins the panel: egress_hosts for whichever model the configured agent CLI reaches, env_allow carrying FREEHOLD_AGENT_ARGV plus any credential the CLI needs, and a budget_ms sized for an agent round-trip rather than a rules match. The crate and its one-shot enforcement landed in a916f1c; nothing dispatches to it until a spec exists
- 2026-07-18 [#k0ye] #gbdispatch2: Durable Object + WebSocket transport + auth for the moderation dispatch edge (now freehold-hub/freehold, submodule src/freehold) — the sandbox/aggregation half is done and pushed; the edge half is unwritten
- 2026-07-18 [#fhdeploy] Deploy the freehold push path in order: (1) cd workers/guestbook-dispatch && bunx wrangler secret put FREEHOLD_DISPATCH_KEY && bunx wrangler deploy; (2) add the [[services]] FREEHOLD_HUB_SVC binding to the Pages project and deploy (a Pages deploy against a service that does not exist yet fails); (3) run freehold with FREEHOLD_EDGE_URL + the same key. Keep workers/guestbook-moderate deployed as the offline backstop
- 2026-07-18 [#fhescalate] an escalated entry stays 'pending' and is re-dispatched on every notify and every alarm, so the panel re-judges it forever. Decide between marking it escalated (edge stops offering it; a later, better panel never gets a second look) and a backoff on re-offer, then implement. Seen live: two identical jobs for the same two entries 8s apart
- 2026-07-18 [#fhsystemd] supervise the freehold host instead of running it under nohup — a systemd user unit plus a credential source it can read without an interactive gpg unlock (passage cannot be read from a unit). Until this lands the push path dies with the login session, and the 15-minute cron in workers/guestbook-moderate is the only backstop
- 2026-07-18 [#gpulease] [recommended] Host-side GPU admission lease in freehold-host: add a declared gpu field to PluginSpec and hold a semaphore across plugin spawn, so N model plugins cannot stampede the inference server. ~100 lines, no framework. Only worth building once more than one local-model plugin exists concurrently — until then #gpupol alone is sufficient
- 2026-07-18 [#rak9] Pre-existing tsconfig error TS5110 in brookebrodack-dev: `module` must be NodeNext when `moduleResolution` is NodeNext. Not caused by the guestbook work, but it means `tsc -p tsconfig.json` never exits clean
- 2026-07-18 [#tsappsub] app/brookebrodack-site/tsconfig.json carries the SAME TS5110 defect just fixed in the root (module esnext + moduleResolution nodenext), so the submodule's own type-check has also never run. It is a separate repo (brookebrodack-site) — fix it there, not from here
- 2026-07-18 [#tsappclean] the app/brookebrodack-site type-check now runs and reports 267 errors — the honest state of a package that had never been checked, not a regression. Dominated by build.ts (esbuild used but not a direct dependency, implicit anys in plugin callbacks) and app/index.ts (a hono ErrorHandler returning void where it must return Response). Separate repo (brookebrodack-site); do it there. Pairs with #tsbrowser for the lib/ half of the parent repo
- 2026-07-18 [#tsbrowser] type-check the browser half: lib/** and test/guestbook-hyop.test.ts are not covered by tsconfig.json, because @cloudflare/workers-types replaces Response with the Workers one and browser code fails against it on every element. A separate DOM-lib config measures 56 errors today — mostly in lib/domain--server--brookebrodack (sig/ctx generic mismatches) plus a missing image-module declaration in the app submodule. Needs the config split plus those fixes, or an explicit decision that lib/ is checked by its own workspace package instead
- 2026-07-18 [#bbdeps] Dependency sweep for brookebrodack-dev and its lib/** submodules — the third project in the graph the operator named. Do after #rappdeps so the @rappstack versions it resolves are already aligned
- 2026-07-18 [#eslintflat] eslint and @typescript-eslint are pinned at 9 / 7 in brookebrodack-dev and nothing runs them (no lint script, no CI step). eslint 10 dropped .eslintrc.json support, which is the config this repo has. Either delete the three dead devDeps or migrate to eslint.config.js and add a lint step to check.yml — do not bump without picking one
- 2026-07-18 [#chkci] Confirm Check run 29671085390 went green. Progress so far: run 29668807479 failed all 67 app tests on the missing gitignored asset (fixed, 36c60bb); run 29670229183 then failed 20 on ConnectionRefused because routes.test.ts spawns the production start.ts with no build and no migrated db (fixed, 6762d07 adds Initialize database + Build). If this run is still red the next cause is new
- 2026-07-18 [#ciappts2] Now that Check is green and gates lint + edge typecheck, revisit adding the two remaining type-check steps (bun run typecheck:browser, and app/brookebrodack-site typecheck). Both are still red ONLY on @rappstack sources — 15 drizzle-split errors plus the marked 13/17 split. Gate them the same cycle #rappdeps lands, not before
- 2026-07-18 [#pushctxcore] Push the ctx-core-dev dependency sweep: 188 per-package submodule commits + superproject 31ad9151a are committed LOCALLY and unpushed on purpose (~190 public repos in one motion). Working tree is clean. Needs operator go, then push each submodule and the superproject
- 2026-07-18 [#sweepstale] The monorepo dependency sweep computes updates against whatever the submodule checkout happens to be, so on a stale checkout it silently REVERTS newer upstream work rather than updating it. Seen concretely: lib/rebuildjs (8 behind, sweep based on 0.70.45) would have reintroduced a removed elysia dep, dropped bun-types, and reverted typescript ^7.0.2 back to next; lib/rebuildjs-tailwindcss (4 behind) would have dropped @tailwindcss/postcss. Make ctx_core__monorepo_pnpm__dependencies__update.sh fetch and refuse to run (or auto-rebase) when any workspace submodule is behind its upstream
- 2026-07-18 [#schemadts2] schema-dts 2.0 is a real breaking change for this codebase, not a routine bump: Article, ItemList and VideoObject stop being assignable to Thing, plus TS2321 excessive stack depth in brookers__doc_html.ts. Measured 8 new errors in lib/ui--server--brookebrodack, so it was reverted to ^1.1.5. Revisit when the jsonld/rdfa helpers in @rappstack/domain--server are updated for schema-dts 2 typing. KEPT OPEN: blocked on rappstack — the helpers that produce these Thing-typed values live there, so this cannot be fixed from brookebrodack-dev
- 2026-07-19 [#rhonouvu] rhonojs test:unit is red at HEAD and unrelated to dependencies: server/app/hono_duck_type.test.ts calls describe() (bun's runner) while test:unit runs uvu, so the file dies with 'Cannot use describe outside of the test runner'. Either port the file to uvu or move rhonojs to bun test
- 2026-07-19 [#ctxredtests] 7 ctx-core-dev packages have test scripts that were already red before the dependency sweep: auth0, google, preprocess, rebuildjs-tailwindcss, relysjs, rhonojs (#rhonouvu), search. relysjs is a missing check-dts script. Triage and fix so the workspace suite is a usable gate. KEPT OPEN: not started — each is an independent pre-existing failure, and none block the sweep that is already committed
- 2026-07-19 [#rappts] 24 of the remaining type errors are in yalc/npm-linked @rappstack/* SOURCES, not this repo (they ship .ts, so skipLibCheck does not apply): 17 in ui--any/md/MarkedPlaintextRenderer.ts (written against marked 13's Renderer signature, now resolving marked 17), 3 in domain--server--blog/post/Post__schema.ts (zod errorMap), plus dl/, rdfa/, jsonld/, breadcrumb/. SEPARATELY, 15 errors in lib/domain--server--brookebrodack/youtube/ have a single rappstack root cause: @rappstack/domain--server--youtube pins drizzle-orm ^0.32.0 while @rappstack/domain--server pins ^0.45.1, so youtube_video_tbl is a 0.32 table type and drizzle_db_(ctx) is a 0.45 db handle and every query between them is a nominal mismatch. Bumping this repo's pin cannot fix it. Align both packages on one drizzle major and republish; that unblocks #ciappts and gating typecheck:browser
- 2026-07-19 [#rappdeps] rappstack is at /home/brian/work/rappstack/dev (github rappstack/dev). Run the same dependency sweep, and ALIGN DRIZZLE: 4 packages pin drizzle-orm ^0.32.0 while domain--server pins ^0.45.1, which is the whole 15-error youtube cluster in brookebrodack-dev (#rappts) — a table type and a db handle from two different drizzle universes. Also has an uncommitted working tree (bun.lockb, lib/dev-bin, lib/domain--any--blog) to resolve first
- 2026-07-19 [#rappgitlink] rappstack/dev origin/main references 8 submodule commits never pushed to their own remotes, so a fresh clone cannot check them out: lib/dev-bin (2ec18b4), domain--browser, domain--browser--blog, snippet--rappstack, ui--any (e8e383a), ui--any--blog, ui--browser, ui--browser--semantic. Commits exist locally in agent-loop/src/rappstack-dev/lib/*. dev-bin additionally needs a real merge, not just a push: gitlink 2ec18b4 has version 0.1.0 (npm publish bump) while its origin/main 248ac1c has newer deps (marked ^17, ora ^9, clipboardy ^5, ctx-core ^7.2.32) plus typescript peerDep loosened to '*' — neither side is a superset, common ancestor e10283e. Until resolved, rappstack/dev bun.lockb + 8 gitlinks must stay uncommitted (committing would regress origin/main)
- 2026-07-19 [#rappsnippetarchive] btakita/snippet--rappstack is ARCHIVED on GitHub (read-only), so its unpushed 'bump version to 0.1.0' commit a61f06b can never be published. rappstack/dev's gitlink now points at the existing remote main 94e07aa and the local bump is dropped (preserved as tag backup/pre-rappgitlink in agent-loop/src/rappstack-dev/lib/snippet--rappstack). Decide: unarchive the repo and re-push the bump, move the package to a new remote, or leave it pinned at 94e07aa. Note its remote main also carries a relysjs->rhonojs migration the local branch never had
- 2026-07-19 [#rappsqlite] better-sqlite3 fails its node-gyp native build against Node v26 in rappstack/dev (v8::Global ctor arity mismatch in better_sqlite3.o), so every 'bun install' aborts mid-link and leaves stale package symlinks — that is what silently reproduced the drizzle 0.32.2/0.45.2 split universe. Workaround today is 'bun install --ignore-scripts'. Fix properly: bump better-sqlite3 to a release with Node 26 prebuilds, pin the toolchain Node version for installs, or drop it if only a transitive dev dep. Stays in bun (see pinned note: rappstack uses bun)
- 2026-07-19 [#tsnextpin] 186 of 190 ctx-core-dev packages carry `typescript: next` in devDependencies. That is what silently pulled a TS 7.1 dev build into rebuildjs and broke its type-check on bun-types (TS2309), which in turn blocked prepublishOnly. Pin to a stable range repo-wide as rebuildjs now is (^7.0.2), and add skipLibCheck where a dependency .d.ts is the failing surface. KEPT OPEN: not started — sequence after #pushctxcore so it does not stack a second uncommitted sweep on top of the unpushed dependency sweep
- 2026-07-19 [#4s08] lib/solid-start type-check fails on solid-start@0.3.11's node_modules .tsx sources (TS2305 for Route/useRoutes/Navigate against @solidjs/router 0.8.4). skipLibCheck cannot cover .tsx, and solid-start is dead upstream (superseded by @solidjs/start). Decide: migrate to @solidjs/start, pin a compatible @solidjs/router, or retire lib/solid-start to dormant/
- 2026-07-19 [#yalcdrift] CONFIRMED, with a measurement: a fresh bun install re-resolves marked 13.0.3 alongside 17.0.4 and the two ctx/*.ts Marked errors (Marked<string, string> vs Marked) come back, taking the app type-check to 62 with 2 in its own sources. Verified this is resolution churn and NOT caused by any version bump — reverted both package.json files to HEAD, reinstalled, measured 62/2 identically. Root cause: one yalc'd @rappstack package pins marked ^13.0.2 while two pin ^17.0.4. Fix belongs in rappstack alongside the drizzle alignment (#rappdeps)
<!-- /agent:done -->
