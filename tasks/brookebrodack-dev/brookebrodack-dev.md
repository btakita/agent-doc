---
agent_doc_session: d2691213-7160-4943-9751-c954341df959
resume:
  claude: 1f6671b6-0e2b-41e7-9565-25805afd7ee1
  codex: 01a14b3a-64c6-7473-a2b0-33b31c9610c7
agent: codex
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

### Re: fan-out queue batch — gpt-5 · 2026-10-17T15:21-04:00

Seven queue items are complete:

- `#nm8q`: pinned all 18 TypeScript declarations in `rappstack/dev` to `^7.0.2`, regenerated `bun.lockb`, installed the result, and verified the real compiler is `Version 7.0.2`. Frozen-lock install and every root/submodule `git diff --check` pass. The old compiler crash is gone. The first real type-check now reaches two pre-existing TS7 config errors (`module`/`moduleResolution` pairing and removed `downlevelIteration`), so I added a separate appended follow-up for the 30-tsconfig migration instead of silently expanding this pin task.
- `#gbdeploy1`: the two originally named runs (`29659909999`, `29660239368`) did finish red at the Pages deploy step, but replacement run `29660441139` succeeded on the same SHA as the second run, including deployment and smoke test. Both production guestbook API endpoints currently return HTTP 200 JSON.
- `#mc4f`: fast-forward pushed all 187 recoverable ctx-core TypeScript-sweep gitlink targets to `main`; 187/187 remote heads exactly match, with zero failures. Superproject sweep commit `61bee1918` was already published and is an ancestor of current `origin/main`. The unavailable historical `lib/solid-start` target is no longer referenced.
- `#lzeffectvoid`: integrated the `EffectRun` `| void` widening in `src/lazily-js` and removed the three obsolete `return undefined` statements in the guestbook effect code. The clean worker worktree passed `make check`, the focused consumer, and the full guestbook type-check. In the shared target checkout, the changed declaration passes focused Prettier and diff checks; its broader `make check` is currently stopped by a pre-existing untracked `tests/core-artifact-consumer/index.mjs` formatting failure, which I did not rewrite.
- `#sbv9` and `#nd8n`: integrated both freehold hardening patches together. Bwrap now receives a classic-BPF seccomp denylist through an inherited FD and runs inside a quiet transient user systemd scope with memory/task limits. The combined implementation passes freehold `make check`, the live sandbox suite, and latest main CI is green.
- `#y457`: set encrypted `OPENAI_API_KEY` bindings on both the `brookebrodack` Pages project and `brookebrodack-guestbook-moderate`, then deployed and verified worker version `c1a039e7-a2b1-4442-84c9-969f7a45d306` with its 15-minute schedule.

The repository edits for `#nm8q`, `#lzeffectvoid`, `#sbv9`, and `#nd8n` remain uncommitted and unpushed because those queue items did not authorize commit/push.

Five worker handbacks require your decision. The packets below are copied verbatim.

```text
[agent-doc] subagent operator request (#waypostrequest)
kind: question
blocking: true
request: Complete npm web login at https://www.npmjs.com/login?next=/login/cli/27269c4c-95bd-482b-b6fb-8ddfb5e12a00 and confirm when it succeeds.
context: `npm whoami` returned E401 in the isolated publish worktree. `npm login --auth-type=web` is running in PTY session 7955 and waiting at “Press ENTER to open in the browser”; publication of @rappstack/ui--any, @rappstack/domain--server, and @rappstack/domain--server--blog cannot proceed until this npm account authentication (including npm 2FA if prompted) completes. The login URL is scoped to this active CLI login flow.
choices:
- Complete web login: Visit the URL, sign in and finish npm 2FA, then tell the coordinator it is complete so this worker can resume the same session.
- Cancel publishing: Stop the login session and leave all three packages unpublished.
```

```text
[agent-doc] subagent operator request (#waypostrequest)
kind: question
blocking: true
request: Close `tasks/brookebrodack-dev.md` in the live editor after the current coordinator cycle (and keep `tasks/software/freehold.md` closed), then tell the coordinator to retry the already-authorized six-item migration once the concurrently dispatched `#sbv9`, `#nd8n`, and `#50fc` results/claims have been reconciled.
context: Read-only ownership evidence now shows the former target owner `%27` is gone (`actor: missing`, `registry: missing`, `live_pane: closed-clean`), so foreign ownership is no longer the blocker. The source is still live on `%25` and the focused-extract runbook forbids `--apply` while either document is editor-attached; this subagent is also expressly forbidden to mutate either session document. The exact six-ID preview currently fails because `#p0rw` and `#k0ye` are archived in `agent:done`, not selectable tracked work. A clean preview for the other four produced operation `81cdbfc60caf`: two linked exchange nodes, backlog items `#sbv9/#nd8n/#50fc`, and review item `#dj8c`, all with `source_action: remove_on_apply`. Safe retry sequence after detachment/claim reconciliation: reopen `#p0rw` and `#k0ye` from source `agent:done` into open backlog (without adding source queue heads), rerun `agent-doc extract ... --ids sbv9,nd8n,dj8c,p0rw,k0ye,50fc`, verify the preview, then rerun it with `--apply`. No semantic reconfirmation is needed; the original imperative already authorizes the move.
choices:
- Detach and retry: Close the source tab after this cycle; the coordinator performs the safe reopen/preview/apply sequence when the related worker claims are settled.
- Defer migration: Keep the source attached and leave `#fhmig` open for a later detached cycle.
```

```text
[agent-doc] subagent operator request (#waypostrequest)
kind: question
blocking: true
request: What should the truncated sentence “It will automatically …” require for plugin aggregation and downstream automation?
context: I inspected the current plan and every recoverable committed copy of the original prompt (commits 94c6d54b59, b1b4ebcff5, af00c1e5f7, and 9c1c253795). Each ends at exactly “It will automatically”; no missing continuation is present. The current contracts already require null/crash/timeout/unparseable output to count as no opinion, never approval; unknown/disagreement to escalate; dispatch escalation to fall through to OpenAI when credentialed and otherwise leave the entry pending. The unresolved choice blocks #k0ye. The plan’s recommended fail-closed policy is the safest fit but cannot be silently substituted for the operator’s missing sentence.
choices:
- Fail-closed consensus (recommended): Run every configured plugin; approve only if every plugin returns approve, reject if any plugin rejects, and escalate on abstention, error, timeout, or disagreement; escalation falls through to OpenAI when available, otherwise remains pending.
- Strict majority: Run every configured plugin; a strict approve/reject majority decides, while ties, no quorum, errors, and timeouts escalate; this tolerates one dissent/failure but weakens the fail-closed posture.
- First decisive answer: Use the first valid approve/reject returned by a configured plugin and escalate only if none decides before the deadline; this minimizes latency and compute but lets one plugin decide alone.
- Supply the intended continuation: Provide the missing words or a different explicit policy, including whether “automatically” meant aggregation, OpenAI fallback, reconnect, or startup rather than moderation semantics.
```

```text
[agent-doc] subagent operator request (#waypostrequest)
kind: question
blocking: true
request: Choose how to restore Hermes inference credentials so I can rerun the required one-shot verification.
context: Hermes is already v0.21.5 (before/after unchanged). `hermes update --check` cannot update this installation because `/opt/hermes-agent` has no `.git` and the install method is reported as `unknown`; `hermes update --plan` also shows a live systemd gateway that a real update could restart, so I did not run the mutating updater. The original v0.18.2 httpx ResponseNotRead crash no longer occurs, but `hermes -z` still exits 2: the default route returns Gemini HTTP 404, and an explicit `-m gemini-2.5-flash` reveals `API key not valid`. The active key comes from the exported shell `GOOGLE_API_KEY`; removing it process-locally leaves no configured provider and exits 1. Current Hermes provides specific legacy-key rotation guidance, so an upstream issue is not justified by this evidence. A valid credential/provider choice is required for a passing exact one-shot check.
choices:
- Rotate Google key: Replace the exported `GOOGLE_API_KEY` with a valid Google AI Studio Gemini key, restart the Hermes gateway/shell environment as needed, then rerun the exact one-shot check.
- Switch provider: Run `hermes model` and authenticate/select another available provider, then rerun the exact one-shot check.
- Stop with diagnosis: Make no credential or service changes; close this queue item with the update limitation and failed verification evidence.
```

```text
[agent-doc] subagent operator request (#waypostrequest)
kind: question
blocking: true
request: Please add credits to the OpenAI project for `brookebrodack/guestbook/OPENAI_API_KEY`, then tell me to retry #j0ee; should I wait for that, or stop this item without selecting/deploying a model?
context: I ran the exact authorized benchmark candidates from isolated worktree `/tmp/brookebrodack-j0ee.QlLuxr`, feeding the approved Passage credential directly without printing or persisting it. All five candidates (`gpt-4o-mini`, `gpt-5-nano`, `gpt-5.4-nano`, `gpt-5.4-mini`, `gpt-5.6-luna`) returned HTTP 429 `You have no credits remaining`. Therefore there is no measured UNSAFE/latency evidence, no defensible cheapest/fastest winner, and I have not set `OPENAI_MODEL` or deployed either surface.
choices:
- Add credits and retry: preserve the empirical selection requirement; I will rerun the same five candidates, select from UNSAFE=0 results, set both surfaces, deploy where needed, and verify.
- Stop blocked: leave `OPENAI_MODEL` unchanged on both surfaces and close #j0ee as blocked on OpenAI billing.
```

Please answer these five requests. The npm login is the only time-sensitive one because its CLI session is already polling.

### Re: #rjrh — gpt-5 · 2026-10-17T15:23-04:00

`#rjrh` remains blocked on the active npm web-login handoff. The worker is still holding its polling CLI session open; no completion or expiry output has arrived, so publishing the three packages or enabling the downstream workflow steps would be unsafe to claim.

Complete npm login and 2FA at:

https://www.npmjs.com/login?next=/login/cli/27269c4c-95bd-482b-b6fb-8ddfb5e12a00

Then tell me to resume `#rjrh`. If you do not want to authenticate, tell me to cancel the waiting login and leave the publication item open. The existing `#rjrh` TODO remains the operator handoff.

### Re: #rappstackdevtypescript — gpt-5 · 2026-10-17

Completed the TypeScript 7 config migration in `/home/brian/work/rappstack/dev`, without committing or pushing.

The live tree contained 40 affected configs rather than the backlog’s estimated 30 because ten nested adapter test projects use the same compiler settings. All 40 now pair `module: "NodeNext"` with the existing NodeNext module resolution and omit the removed `downlevelIteration` option. The edits cover the root/generated configs, all 23 package configs, and 15 nested adapter test configs.

Verification with TypeScript 7.0.2:

- `tsc --showConfig`: 40/40 passed.
- Full `tsc -p <config> --pretty false`: 10 passed, 30 failed on existing source/type debt.
- Zero TS5110, TS5102, or unknown-compiler-option diagnostics remain.
- Passing packages: `dev-bin`, `domain--any--blog`, `domain--browser--blog`, `domain--server`, `domain--server--youtube`, `ui--any`, `ui--any--blog`, `ui--browser`, `ui--server`, and `ui--server--blog`.
- Root and every submodule `git diff --check` passed.

The existing TypeScript `^7.0.2` package declarations and regenerated `bun.lockb` remain intact. The main checkout also matches the isolated worker’s 40 config files byte-for-byte.

The npm publication worker produced a newer blocker after the prior login handoff: all three current manifest versions are already present on npm, and the attempted login ended with `E401`. I updated `#rjrh` and its TODO accordingly. Its exact handoff is:

```text
[agent-doc] subagent operator request (#waypostrequest)
kind: question
blocking: true
request: Choose release versions and have the coordinator prepare version-bumped package commits, then provide an authenticated npm session so this worker can publish them.
context: Registry inspection shows the exact local manifest versions cannot be published: @rappstack/ui--any is local 0.1.0 and registry latest 0.1.0; @rappstack/domain--server is local 0.1.2 and registry latest 0.1.2; @rappstack/domain--server--blog is local 0.1.1 while registry latest is 0.1.4. npm forbids overwriting published versions. This worker’s coordinator rules explicitly prohibit version bumps and releases, so it cannot prepare the required publishable commits. Separately, `npm whoami` returned E401. The earlier web-login flow did not complete and fell back to a credential prompt, which has been canceled safely. No packages were published and no package files were changed.
choices:
- Prepare patch releases: Coordinator creates/provides commits for ui--any 0.1.1, domain--server 0.1.3, and domain--server--blog 0.1.5; operator completes npm authentication/2FA when prompted; this worker then resumes publication in dependency order.
- Specify different versions: Operator supplies three unused semver versions; coordinator prepares/provides those commits; operator completes npm authentication/2FA; this worker resumes publication.
- Cancel publishing: Leave all three fixes unpublished and keep the brookebrodack-dev type-check steps blocked.
```

### Re: #fhmig — gpt-5 · 2026-10-17

No operator choice has been supplied, so I did not mutate either session document or claim the migration complete. I gated `#fhmig` on the existing operator handoff.

The former target owner is gone, but the source is still editor-attached. A safe retry also requires reopening archived `#p0rw` and `#k0ye` before rerunning the six-ID extract; the verified four-item preview is operation `81cdbfc60caf`.

Choose one:

- Detach and retry: close this source tab and keep `tasks/software/freehold.md` closed; I will reopen the two archived items, preview all six IDs, verify it, and apply the extraction.
- Defer migration: keep the source attached and leave the gated item for a later cycle.

### Re: remaining operator gates — gpt-5 · 2026-10-17

No operator decisions or billing change arrived for the remaining queue heads, so I gated them rather than claiming completion:

- `#50fc` — choose the missing aggregation policy. Recommended: fail-closed consensus (all configured plugins run; unanimous approval required; any rejection rejects; abstention/error/timeout/disagreement escalates). Alternatives are strict majority, first decisive answer, or your intended continuation after “It will automatically …”.
- `#j0ee` — all five benchmark candidates returned OpenAI 429 “no credits remaining.” Choose “add credits and retry” to preserve empirical model selection, or “stop blocked” and leave `OPENAI_MODEL` unchanged.
- `#2amm` — Hermes is already 0.21.5 and the old response-read crash is gone, but the active Google key is invalid and this installation has no known updater. Choose “rotate Google key,” “switch provider,” or “stop with diagnosis.”

Their existing TODOs remain the operator handoffs. No credentials, services, model settings, or moderation policy were changed.
<!-- agent:boundary:5d323eb2 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue subagents priority go -->
- ~~do [#fhmig]~~
- ~~do [#50fc]~~
- ~~do [#j0ee]~~
- ~~do [#2amm]~~
- ~~do [#rappstackdevtypescript]~~
<!-- /agent:queue -->

## Notes

<!-- agent:notes -->
<!-- /agent:notes -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#rjrh] Publish @rappstack/ui--any, @rappstack/domain--server, and @rappstack/domain--server--blog to npm from /home/brian/work/rappstack/dev. BLOCKED: the current local manifest versions are already published (ui--any 0.1.0, domain--server 0.1.2, domain--server--blog 0.1.1 while registry latest is 0.1.4), so publishable version bumps are required in addition to npm authentication/2FA. Suggested patch releases are 0.1.1, 0.1.3, and 0.1.5; after publication, uncomment the two type-check steps in brookebrodack-dev .github/workflows/check.yml
<!-- /agent:backlog -->

## Todo

<!-- agent:todo -->
- [ ] [#rjrh] Choose release versions, prepare version-bumped package commits, and complete npm authentication/2FA for [#rjrh]
- [ ] [#fhmig] Choose whether to detach the source and retry or defer [#fhmig]
- [ ] [#50fc] Choose the plugin aggregation policy for [#50fc]
- [ ] [#2amm] Choose a Hermes credential recovery path for [#2amm]
- [ ] [#j0ee] Add OpenAI credits and retry, or stop blocked, for [#j0ee]
<!-- /agent:todo -->

## Review

<!-- agent:review -->
- [/operator] [#2amm] Restore Hermes one-shot inference. Hermes is already 0.21.5 and the old ResponseNotRead crash is gone, but /opt/hermes-agent has no git/update method and the active GOOGLE_API_KEY is invalid; GATED on operator choosing key rotation, another provider, or stopping with the diagnosis
- [/operator] [#j0ee] Pick the moderation model empirically after OpenAI billing is restored. All five authorized candidates currently return HTTP 429 no credits, so no UNSAFE/latency winner can be selected and OPENAI_MODEL remains unchanged on both surfaces; GATED on operator adding credits or choosing to stop blocked
- [/operator] [#50fc] Choose the plugin aggregation policy for the truncated requirement ending at 'It will automatically'. The missing continuation is unrecoverable from every committed copy and blocks #k0ye; GATED on operator selection among fail-closed consensus, strict majority, first decisive answer, or supplied intended wording
- [/operator] [#fhmig] Migrate the freehold-scoped items (#sbv9, #nd8n, #dj8c, #p0rw, #k0ye, #50fc) into tasks/software/freehold.md. GATED on operator choice: the former target owner is gone, but the source document remains editor-attached; #p0rw and #k0ye must be reopened from done before rerunning the six-ID extract. The verified four-item preview operation was 81cdbfc60caf
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

<!-- agent:done archive="tasks/brookebrodack-dev/brookebrodack-dev.done.md" -->

<!-- completed work archived in tasks/brookebrodack-dev/brookebrodack-dev.done.md -->
<!-- /agent:done -->
