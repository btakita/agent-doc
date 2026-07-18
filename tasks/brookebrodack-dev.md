---
agent_doc_session: d2691213-7160-4943-9751-c954341df959
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
agent_doc_model_tier: high
queue: start
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md. DO NOT IMPLEMENT THE FIX.
  '#commit-push': commit all uncommitted files
  '#commit-push-deploy': commit all uncommitted files + deploy
  '#mermaid': Include explanatory mermaid diagram(s).
  '#spec-test-commit-push': update spec + tests. commit + push
  '#spec-test-commit-push-deploy': update spec + tests. commit + push + deploy
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/5ed9ebdbe4a2edb795318ea94faf66a77813df7180cde01350e66214e980296b-20260718-205427.md`*

Compacted content:
- Archived 8 response topic(s): GPU policy for local model plugins; #teah; a worker pool of Codex / Claude Code / opencode sessions; 5 more
- Prior summary/context: prior compacted content: Archived 15 response topic(s): Lazily for the state layer; local moderation dispatch service...

### Re: queue strike of #gbmig3 — opus-4-8

Acknowledged — the completed `~~do [#gbmig3]~~` head is removed from the queue. That was the only edit this cycle; no repo work was requested and none was performed.

**Queue state:** the binary reports `queue_drainable_head_count: 0` / `queue_continuation_required: false`, so no in-session auto-loop. The remaining heads (`#fhquorum2`, `#fhoneshot`, `#gpupol`, `#gpulease`, `#rak9`, `#p0rw`, `#sbv9`, `#nd8n`, `#k0ye`, `#fhmig`, `#50fc`, `#y457`, `#j0ee`, `#2amm`) are not in-session drainable from here — the freehold items in particular belong to `tasks/software/freehold.md`, which preflight reports as foreign-owned by pane `%27` (also `%3` for equityfundingsource, `%23` for agent-doc-bugs2). Those drain in their own sessions.

**Editor authority wedge (recorded, non-blocking for this response):** preflight computed the backlog→queue mirror (15 active ids) but could not persist it — `editor_attached_model_missing`, `recovery=retry_without_disk_write`. A typed `agent-doc admin reload-lib` delivered cdylib v0.35.6 to 1/1 editor endpoints and unwedged the read path (`agent-doc plan` now resolves), but the mutation path still reports the same state, so the queue text on record is the pre-maintenance text. Binary-owned replica re-registration and retained-intent replay continue asynchronously; `operator_action=none`, so I did not force-disk or repair.

### Re: #fhquorum2 — quorum of 1 — opus-4-8

> **Queue prompt:** do [#fhquorum2]: Do not require a 2nd agent. 1 is sufficient. If APPROVE_QUORUM_MIN = 2, then have 2 agent calls in parallel.

Done. `APPROVE_QUORUM_MIN` is now `1` in `freehold-tribunal/src/lib.rs`, so a single-plugin deployment can approve instead of escalating every entry to a human.

**The anti-capture property moved rather than disappeared.** The old minimum of 2 existed so that owning one plugin could not publish — with the minimum at 1 and no other change, an owned plugin voting `Approve` while its peers were dead would have published, and the existing `a_single_compromised_plugin_cannot_approve` test would have gone red. So `aggregate` now also requires `answered.len() == results.len()`: **every configured plugin must answer**, and all answers must agree. A compromised plugin cannot approve on its own by silencing its peers, because a plugin dropping out of the quorum still withholds approval — the invariant `docs/plugin-isolation.md:166` asks for ("a plugin falling out of the quorum must never be a silent event"). In a single-plugin deployment that one plugin *is* the whole panel; compromising it is compromising the panel, which is the tradeoff you asked for.

**Second half of the directive was already satisfied.** Plugin calls are not sequential — `freehold-host/src/lib.rs:47` fans them out through `futures::future::join_all`, so however many plugins are configured, they run in parallel. No change needed there.

Changes:
- `crates/freehold-tribunal/src/lib.rs` — `APPROVE_QUORUM_MIN: 2 → 1`; added the all-configured-answered condition to `aggregate`; new `escalate_reason` branch `"only N of M configured plugin(s) answered"`; corrected the module and const docs, which still claimed one plugin can never publish.
- `crates/freehold-tribunal/tests/aggregate.rs` — new `a_lone_configured_plugin_can_approve`; kept `a_single_compromised_plugin_cannot_approve` green under the new rule with a comment explaining why it still holds.

Verification: `make check` exit 0, `cargo test --workspace` 37 passed / 0 failed across 12 suites. Committed to the freehold submodule as `d296bd7` (not pushed, and the superproject gitlink is not bumped — concurrent sessions are live in this tree).
<!-- agent:boundary:ede8fbd1 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue go -->
- ~~do [#fhquorum2]: Do not require a 2nd agent. 1 is sufficient. If APPROVE_QUORUM_MIN = 2, then have 2 agent calls in parallel.~~
- do [#fhoneshot]
- do [#gpupol]
- do [#gpulease]
- do [#rak9]
- do [#p0rw]
- do [#sbv9]
- do [#nd8n]
- do [#k0ye]
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
- [ ] [#gbdeploy1] Confirm the re-run Deploy to Cloudflare Pages workflow went green (runs 29659909999 / 29660239368 were still in progress at closeout). The original run FAILED its post-deploy smoke test because the guestbook API was serving an error page — code reading deleted_at shipped before migration 0003 was applied. Prod was verified healthy by hand afterwards (GET /guestbook/api/entries → 200 application/json), so this is a green-signal confirmation, not a suspected outage
- [ ] [#fhoneshot] When adding a CLI-agent-backed verdict plugin, invoke it ONE-SHOT per request (claude -p / codex exec, non-interactive) under the existing PluginSpec budget_ms + OCI sandbox contract. Do NOT pool or reuse sessions: retained conversation state breaks per-entry verdict independence and makes a successful prompt injection persist across future verdicts, which is the exact threat the sandbox exists to contain
- [ ] [#gpupol] Run ONE persistent local-inference server (vllm or ollama, both already installed) as the sole GPU owner, outside the sandbox. Model plugins reach it over the existing egress-proxy path with egress_hosts: ["127.0.0.1:<port>"] — no new mechanism, and /dev/nvidia* never enters the sandbox. Depends on #p0rw (the proxy must exist first). Start with ollama for its idle-unload behaviour; switch to vLLM only if sustained concurrency exceeds ~4-8 requests
- [ ] [#gpulease] [recommended] Host-side GPU admission lease in freehold-host: add a declared gpu field to PluginSpec and hold a semaphore across plugin spawn, so N model plugins cannot stampede the inference server. ~100 lines, no framework. Only worth building once more than one local-model plugin exists concurrently — until then #gpupol alone is sufficient
- [ ] [#rak9] Pre-existing tsconfig error TS5110 in brookebrodack-dev: `module` must be NodeNext when `moduleResolution` is NodeNext. Not caused by the guestbook work, but it means `tsc -p tsconfig.json` never exits clean
- [ ] [#p0rw] Write the allowlisting egress proxy for the plugin egress hostport in src/freehold — model plugins are routed to it by sandbox_argv but it does not exist yet, so a model plugin currently has no working network path (~50-line CONNECT proxy, allowlist generativelanguage.googleapis.com). Confirmed still absent: egress_hosts appears only on PluginSpec and in tests, with no proxy binary or call site
- [ ] [#sbv9] Author a seccomp BPF filter and pass it via bwrap --seccomp <fd> in src/freehold (crates/freehold-host/src/sandbox.rs). Outstanding condition on bwrap being the default sandbox — confirmed still absent (no seccomp reference anywhere in crates/). Until it lands a compromised plugin has no network/filesystem/caps/env but does have the full kernel syscall surface. Needs fd-passing through tokio Command (pre_exec/CommandExt); bwrap has no --seccomp-path variant
- [ ] [#nd8n] Add cgroup limits for the bwrap sandbox path in src/freehold via a transient systemd scope (systemd-run --user --scope -p MemoryMax= -p TasksMax=) — mem_limit_mb and pids caps are applied on the OCI paths only, so the default bwrap profile has no ceiling on a runaway plugin. Confirmed still absent: mem_limit_mb exists on PluginSpec but no systemd-run/cgroup call site
- [ ] [#k0ye] #gbdispatch2: Durable Object + WebSocket transport + auth for the moderation dispatch edge (now freehold-hub/freehold, submodule src/freehold) — the sandbox/aggregation half is done and pushed; the edge half is unwritten
- [ ] [#fhmig] Migrate the freehold-scoped items (#sbv9, #nd8n, #dj8c, #p0rw, #k0ye, #50fc) out of tasks/brookebrodack-dev.md into tasks/software/freehold.md, whose agent:backlog is currently EMPTY. The dispatch repo was renamed + transferred to freehold-hub/freehold (submodule src/freehold); this document should keep only brookebrodack-dev site/guestbook scope. Deferred this cycle: tasks/software/freehold.md is foreign-owned by pane %27
- [ ] [#50fc] Confirm the truncated dispatch requirement — the queued prompt ends mid-sentence at "It will automatically", which leaves the plugin aggregation/automation policy (§6 of tasks/software/plan-brookebrodack-guestbook-dispatch-service.md) unspecified; blocks #k0ye. Operator input required
- [ ] [#y457] Set OPENAI_API_KEY on BOTH surfaces — `bunx wrangler pages secret put OPENAI_API_KEY --project-name=brookebrodack` (inline submit-time judging) and `cd workers/guestbook-moderate && bunx wrangler secret put OPENAI_API_KEY && bunx wrangler deploy` (cron sweeper). Without it entries just stay pending.
- [ ] [#j0ee] Pick the moderation model empirically once the key exists: `OPENAI_API_KEY=... bun scripts/guestbook-model-bench.ts gpt-4o-mini gpt-5-nano gpt-5.4-nano gpt-5.4-mini gpt-5.6-luna` and take the cheapest/fastest with UNSAFE=0, then set OPENAI_MODEL on both surfaces
- [ ] [#2amm] `hermes -z` is broken in hermes-agent 0.18.2 — every one-shot run dies with an httpx ResponseNotRead inside the Gemini provider, so the local primary path always falls through. Try `hermes update` or file upstream
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
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
<!-- /agent:done -->
