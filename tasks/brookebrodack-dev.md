---
agent_doc_session: d2691213-7160-4943-9751-c954341df959
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/5ed9ebdbe4a2edb795318ea94faf66a77813df7180cde01350e66214e980296b-20260719-032943.md`*

Compacted content:
- Archived 8 response topic(s): #rak9; #tsappsub; #tsappclean; 5 more
- Prior summary/context: prior compacted content: Archived 12 response topic(s): queue strike of #gbmig3; #fhquorum2; #fhoneshot; 9 more

### Re: #sweepstale — the guard caught the repo it was written about — opus-4-8

> **Queue prompt:** do [#sweepstale]

Done. Pushed to `ctx-core/dev` as `8fac099f1` + `bd123379b`, gitlinks bumped in agent-loop.

`bin/ctx_core__workspace_submodules__check_stale.sh` fetches every `lib/*`, `tools/*`, `vendor/*` submodule in parallel (191 of them, 7.6s at `--jobs 12`) and classifies each against `origin/HEAD` as equal / ahead / behind / diverged. The sweep now aborts on any behind-or-diverged one. `--rebase` fast-forwards them; `CTX_CORE_ALLOW_STALE_SUBMODULES=1` overrides.

**The first real run refused, and it was right to.** `src/ctx-core-dev` was 190 submodules behind — every one carrying the same `chore(deps): update dependencies to latest` commit, pushed from the *other* ctx-core-dev checkout at `~/work/ctx-core/`. Running the sweep here would have reverted the entire prior sweep. That is the failure mode the item describes, sitting live in the tree, and it is worth naming precisely: the danger is not one stale clone, it is **two clones of the same monorepo**, where the one you happen to be in has no way to know the other already did the work.

After rebasing onto `origin/main` and syncing gitlinks, the checker exits 0.

**On the two things I did not simply follow the item on:**

- **Refuse, not auto-rebase.** The item allowed either. Auto-rebase silently rewrites ~190 checkouts as a side effect of asking for a dependency report, which is a much larger action than the one requested — so `--rebase` is opt-in and re-runs the check afterward rather than trusting its own loop.
- **`--rebase` is fast-forward only.** A diverged submodule is reported for a human, never rewritten. Behind-only is recoverable; diverged means someone has local work, and losing it to a convenience flag is a worse outcome than the revert this guard exists to prevent.

Both fast-forward paths are exercised, not assumed: `merge --ff-only` on a branch checkout (`lib/rebuildjs`, on `main`) and `checkout --detach` on a detached one (`lib/array`) — each run for real and restored.

**One incidental finding.** The first run reported `lib/addtocalendar.com` and `lib/auth0-ui-solid` as unresolvable. They are not uninitialized — they are declared in `.gitmodules` with **no gitlink in the tree at all**, so `git submodule update --init` cannot materialize them and never could. Removed the two stanzas rather than leaving the checker reporting two permanent phantoms.
❯ Please auto-rebase. I'm not working on these submodules.

### Re: auto-rebase by default — opus-4-8

❯ > ❯ Please auto-rebase. I'm not working on these submodules.

❯ Flipped. Pushed as `d53389423`, gitlink bumped.

❯ Fast-forwarding is now the default; `--check-only` reports without changing anything. Your reason is the better one and I should have reached it myself: these 191 submodules are *consumed*, not developed in, so a behind checkout is a sync problem to fix, not a decision worth stopping a dependency sweep to ask about. Refusing by default made the guard cost you a manual step on every run to protect against a case that does not arise in this repo.

❯ **What I kept.** A **diverged** submodule — local commits upstream does not have — is still never rewritten; it is reported and the sweep refuses. That is not the case you were describing. Fast-forwarding is recoverable; silently discarding unpushed local work is not, and the flag that does it should never be the one you get by default.

❯ Verified by rewinding real submodules rather than reasoning about it: `lib/array`, `lib/uuid`, `lib/rebuildjs` reset one commit back, then auto-rebase recovered all three and the re-check passed clean. For the diverged case I put a local-only commit on `lib/uuid` — auto-rebase skipped it, refused, and **the probe commit survived**. All three restored to their exact original SHAs afterward; no gitlink drift.

❯ One implementation note worth recording: the post-fast-forward re-check re-execs as `--after-rebase` rather than `--rebase`, so anything that failed to fast-forward refuses on the second pass instead of recursing into another attempt.

### Re: #schemadts2 — it was never a schema-dts breaking change — opus-4-8

> **Queue prompt:** do [#schemadts2]

Done, and the item's diagnosis was wrong in a way worth stating plainly. Pushed as `4944afb` (+ two submodule commits), gitlink bumped.

**schema-dts 2.0 is not a breaking change for this codebase, and rappstack was never the blocker.** The item recorded "Article, ItemList and VideoObject stop being assignable to Thing" and concluded the jsonld/rdfa helpers in `@rappstack/domain--server` had to be updated first. Neither half holds:

- Compiled `const t: Thing = a` for `Article`, `ItemList`, `ListItem` and `VideoObject` against **both** 1.1.5 and 2.0.0 in isolation. Both clean. The assignability the item says 2.0 removed is intact.
- The real cause was a **version split inside this repo**: `app/brookebrodack-site` pinned `schema-dts ^1.1.5` while `lib/ui--server--brookebrodack` had been bumped to `^2.0.0`. Both resolved simultaneously, so `Thing` in the rappstack helper signature came from one copy and `Article` at the call site from the other. Nominal mismatch — the same shape as `#rappts` (drizzle 0.32 vs 0.45) and `#yalcdrift` (marked 13 vs 17), not an API change.

That is why the errors looked exotic: `TS2321 excessive stack depth comparing 'ItemList' and 'Thing'` is what a cross-copy comparison of a deep recursive union produces, not what a renamed type produces.

**The fix was one line** — align `app/brookebrodack-site` to `^2.0.0`. I bumped rappstack's five `schema-dts` devDependency pins first while testing the item's stated theory, measured that they had no effect on consumer resolution (they are devDeps; consumers never install them), and **reverted them** rather than leaving an unnecessary cross-repo change behind.

Measured: `typecheck:browser` 45 → **36**, which is the exact pre-existing baseline, with **0** schema-dts errors and 0 errors in `ui--server--brookebrodack`. Edge `typecheck` 0. `test` 67 pass, `test:dom` 9 pass, lint clean.

**One incidental effect I did not go looking for.** The reinstall also collapsed two other duplicate resolutions the lockfile was carrying — `marked 13.0.3` alongside `17.0.4` (the `#yalcdrift` split) and `esbuild 0.25.12` alongside `0.28.1`. Single copies of all three now. I have **not** marked `#yalcdrift` done: its specific claim is that a *from-scratch* install re-splits marked, and my install was incremental, so that claim is untested. It needs a `rm -rf node_modules` reinstall to confirm either way.

Check run `29672498182` was still in progress at closeout — recorded, not waited on.

### Re: #ctxredtests — seven independent defects, not one — opus-4-8

> **Queue prompt:** do [#ctxredtests]

All seven exit 0. Pushed as `b254e4739` plus six submodule commits; gitlink bumped. `#rhonouvu` is closed with it.

The item framed these as a category ("red before the sweep"). They are not one problem, and two of them were tests that had **never** passed rather than tests that broke:

- **google** — `(...opts_a)` then `{ ...opts_a }` spreads an *array* into an object, so `opts.GOOGLE_TRACKING_ID` was always `undefined` and the function always threw. All four call sites in the package also passed an object as `missing_argument__throw`'s **message** parameter, which is why the failure printed `[object Object]` instead of naming the key.
- **relysjs** — `test` invoked a `check-dts` that is neither a script nor a dependency, so it exited 1 *after* 76 passing tests. Replaced with the sibling `test:types`/`test:unit` pair.
- **rhonojs** (`#rhonouvu`) — one file imported `bun:test` while the runner is uvu, killing the entire run. Ported it. The test had also **copied** the duck-type predicate instead of importing it, so it was verifying a duplicate of the logic rather than the logic; I extracted `hono__is` from `server/app` so it now exercises production code. 91 pass.
- **rebuildjs-tailwindcss** — two stacked failures: `typescript: next` pulled a TS 6.0.0-dev whose `lib.dom.d.ts` conflicts with bun-types, and `test:unit` ran under node+tsx+esmock while importing Bun-only APIs. No test in the package uses esmock; that harness was vestigial.
- **auth0** — `@sinonjs/fake-timers` 15 ships its own declarations, which win over `@types/`; the bundled name is `Clock`, not `InstalledClock`. Separately, `AUTH0_URL` still pointed at auth0-js **9.30.1** after the sweep moved the dependency to 10.2.0. That test was doing its job. I verified the 10.2.0 CDN bundle returns 200 before repointing rather than assuming the URL existed.
- **search** — same fake-timers rename.
- **preprocess** — `index.d.ts` references `Buffer` with no node types declared, so it failed against its own published declarations.

**One finding that matters more than the seven fixes.** Midway through, `auth0`, `preprocess` and `search` flipped from green to red across nothing but a reinstall. I checked whether my own change caused it — reverted my typescript pin, reinstalled, still red — so it was not mine. The cause is `typescript: next`: auth0's `tsc` resolved **7.1.0-dev.20260717.1**. A suite whose colour depends on when you last ran `bun install` is not a gate no matter how many of its packages are green, so `#tsnextpin` is the load-bearing item here, not a cleanup. I fixed the three properly (the defects are real under any compiler) rather than leaving them attributed to install churn.

Not done: the other ~183 packages still carry `typescript: next`. That is `#tsnextpin`, still queued.

### Re: queue drain paused at an operator-gated head — opus-4-8

> **Queue prompt:** do [#oidcnpm]

Stopping the in-session drain here, and this is a stop rather than a stall.

`#oidcnpm` is the head, and it is **operator-only**: configuring the npm Trusted Publisher needs an authenticated npmjs.com browser session with 2FA. `queue_continuation_required` is `false` for exactly that reason. The binary tried to auto-strike this head back in the session's first preflight (`source=review_gated`), but that maintenance pass failed to persist — `queue_maintenance_not_persisted`, with editor authority stuck in `editor_attached_model_missing` — so the resolved head has been sitting on top of the queue ever since, gating everything below it.

**What is actually left, in one line:** `#rappdeps`, `#tsnextpin`, `#lzeffectvoid`, `#tsappclean`, `#sbv9`, `#nd8n`, `#fhmig`, `#50fc`, `#y457`, `#j0ee`, `#2amm` — agent-drainable, but queued behind the operator-gated head.

Three items closed this session, all pushed to their own remotes: `#sweepstale`, `#schemadts2`, `#ctxredtests` (+`#rhonouvu`).

Two things worth your attention rather than mine:

- **The `agent-loop` superproject has six gitlink commits local and unpushed.** Every submodule change is pushed; only the superproject pointers are held back, since pushing a shared superproject is not something I will do unasked.
- **`#tsnextpin` is load-bearing, not cleanup.** Three ctx-core-dev packages flipped green→red across nothing but a reinstall this session, because `typescript: next` floats (auth0 resolved `7.1.0-dev.20260717.1`). Until that pin lands, the workspace suite's colour depends on when someone last ran `bun install`.

I also left a dangling `preflight_started` cycle by opening a preflight I did not answer; this response closes it.

### Re: #rappdeps — the item pointed at a checkout 17 commits behind — opus-4-8

> **Queue prompt:** do [#rappdeps]

Drizzle is aligned and the cluster it caused is gone. `#rappts` closes with it. Pushed as `d382c3c` (rappstack) + `aa7bcbc` (brookebrodack) and four submodule commits; gitlinks bumped.

**The item named the wrong checkout, and that mattered.** It says "rappstack is at /home/brian/work/rappstack/dev". There are two clones of `rappstack/dev`, and that one is **17 commits behind origin**. The authoritative tree is `src/rappstack-dev`, which is exactly at `origin/main` and is what the `.yalc` snapshots actually came from — that is how I noticed: the yalc copy of `domain--server` pinned `^0.45.1` while the named checkout's source said `^0.32.0`.

The item's "uncommitted working tree to resolve first" is that stale clone's leftovers — a half-finished widening of the `typescript` **peer**Dependency from `^5.5.3` to `*` — already superseded upstream. Running the sweep there would have reverted 17 commits of work. That is precisely the failure `#sweepstale` was written about, and it is worth saying plainly: **rappstack has no equivalent guard**, so the protection I added earlier this session does not cover this repo.

**On the alignment itself.** The split was real but not where `#rappts` recorded it. Four separate pins had to move, in two repos:

- `domain--server--youtube`, `domain--server--auth`, `adapter-drizzle` — `^0.32.0` → `^0.45.1`
- `lib/domain--server--brookebrodack` — also `^0.32.0`, which `#rappts` never named. Bumping only rappstack left the lockfile still resolving two copies; I re-measured and saw 15 errors unchanged before finding this one.

`domain--server--auth` needed a rebase onto an upstream elysia→hono migration; I kept both sides (hono **and** the new drizzle pin) rather than letting either win.

Measured in the consumer: `typecheck:browser` **36 → 21**, youtube cluster **15 → 0**, edge `typecheck` 0, `test` 67 pass, `test:dom` 9 pass, lint clean. Not a pin change taken on faith — 0.32→0.45 is a real bump, so the suite is the evidence.

**Not done, and still queued:** the dependency sweep proper, which is the other half of `#rappdeps`. I amended the item with the correct checkout so the next run does not start from the stale one. I also gated `#oidcnpm`, which has been sitting resolved-but-unstruck at the queue head since the first preflight's queue maintenance failed to persist.

**Session infrastructure note:** the finalize before this one failed with `editor_attached_model_missing` — `reload-lib` reported 0/0 live editor endpoints while Lazily still believed the editor was open. `admin recycle` cold-started a replacement supervisor and cleared it. The stale supervisor (pid 1912833) is still mapping an old binary with its recycle deferred.

### Re: do [#rappdeps] — opus-4-8

The premise in the item turned out to be stale in a useful way: **the drizzle alignment was already committed and pushed upstream** as `d382c3c fix(deps): align drizzle-orm on ^0.45.1 across the server packages` on `rappstack/dev` `origin/main`. `/home/brian/work/rappstack/dev` was simply a stale checkout — 0 commits ahead, 18 behind, no local-only work.

**Uncommitted tree, resolved.** Before touching anything I backed it up to scratchpad (`rappdeps-backup/`: superproject patch, 10 per-submodule patches, submodule head list) and tagged the two divergent submodule commits `backup/rappdeps-pre-sync` in `domain--any--blog` (712ac18) and `domain--server--blog` (cbb0371) so they stay reachable.

What the tree actually contained was superseded, not valuable:
- The 10 `package.json` edits were tool-generated peerDep loosenings (`typescript: "^5.5.3"` → `"*"`). Root pins `typescript: "next"`, which resolves to a 7.1.0-dev build that does not satisfy `^5.5.3`, so bun rewrites them on every install. `origin/main` already carries the same loosening in several packages.
- The two divergent submodule commits ran *backwards* relative to upstream: local 712ac18 does `tags → tag_a1` and `rhonojs → relysjs`, while upstream went the other way (`Rename tag_a1 to tags`, `migrate relysjs to rhonojs`). Keeping them would have re-reverted the upstream direction.

**Sweep result — single drizzle universe confirmed.** Fast-forwarded to `d382c3c`, synced submodules, and reinstalled clean. All four consumers now resolve to one hoisted copy:

| package | pin | resolves |
|---|---|---|
| `domain--server` | `^0.45.1` | 0.45.2 |
| `domain--server--auth` | `^0.45.1` | 0.45.2 |
| `domain--server--youtube` | `^0.45.1` | 0.45.2 |
| `persontric-adapter-drizzle` | `^0.45.1` | 0.45.2 |

That took two passes worth flagging. The first `bun install` aborted partway through linking when `better-sqlite3` failed its native build against Node v26 (its C++ does not match Node 26's V8 API — pre-existing, unrelated to this change). The abort left `.bun/drizzle-orm@0.32.2` in the store with all four packages still symlinked to it, so `bun pm ls` reported 0.45.2 while the on-disk links were the old universe — exactly the split we were trying to kill. `bun install --force` did not prune it; only a full `node_modules` wipe and reinstall did.

**Two things I deliberately did not do.**

The 8 dangling gitlinks are filed as `#rappgitlink` rather than fixed — pushing to 8 separate repos is outward-facing, and `dev-bin` needs a genuine merge decision rather than a push, since the gitlink commit and its `origin/main` have each got content the other lacks.

I also left `rappstack/dev` uncommitted on purpose. `git status` shows `M bun.lockb` plus `M` on those same 8 submodules — but those 8 are *stuck at older commits* because their checkout failed, so committing them would regress `origin/main`'s gitlinks. The lockfile is likewise derived from a tree with 8 stale submodules. Both should land only after `#rappgitlink` is resolved.

Note that `#rappts`'s 15-error youtube cluster lives in **brookebrodack-dev**, which was already on `^0.45.1`. This cycle fixed the upstream half it consumes; whether that clears the cluster needs a typecheck over there. I could not verify it from this side — `tsc` in `domain--server--youtube` fails first on an unrelated `tsconfig.json` error (`TS5110: 'module' must be set to 'NodeNext' when 'moduleResolution' is 'NodeNext'`) against the old `typescript@5.5.0-dev` that the root `next` pin resolved to.
❯ 🚧 please commit + push

### Re: please commit + push — opus-4-8

> **Queue prompt:**
>
> do [#rappgitlink]


Done, and `#rappgitlink` is fixed at the root rather than worked around. `rappstack/dev` is now at `6201622`, pushed, with a fresh recursive clone verified working.

**What the 8 submodules actually had.** Each carried exactly one unpushed commit — `chore: bump version to 0.1.0 for npm publishing` — while its own remote had moved on with dependency updates and a `typescript` peerDep loosening. A batch npm-publish bump was committed into `rappstack/dev`'s gitlinks but the submodule commits themselves were never pushed, so the superproject referenced objects nobody could fetch.

None were a simple push; all 8 were diverged (1 ahead, 1–5 behind). I rebased each bump onto its remote `main`, which produced the intended union — version bump on top of the newer deps.

Six rebased clean. Two conflicted (`snippet--rappstack`, `ui--any--blog`) because both sides edited the same `package.json` region. I resolved both by taking **upstream's** file and applying only the version bump, since upstream's dependency decisions were strictly newer — `snippet--rappstack` upstream had already replaced `workspace:*` with `^0.1.1` ranges (vs the local commit's `^0.1.0`) plus a `relysjs → rhonojs` migration. Every result was verified fast-forward, clean tree, valid JSON before anything left the machine. Prior HEADs are tagged `backup/pre-rappgitlink` in each submodule.

**7 of 8 pushed. One can't be.** `btakita/snippet--rappstack` is **archived on GitHub — read-only**, so its commit is unpublishable by any means. I pointed that gitlink at the existing remote `main` (`94e07aa`) and dropped the local bump, which is what makes a clone work today. That needs a real decision from you, so it is filed as `#rappsnippetarchive` — unarchive and re-push, move the package to a new remote, or leave it pinned.

**Verification.** After committing the gitlinks I ran a full `git clone --recurse-submodules` from GitHub into a temp dir. Every submodule checked out, including all 8 that previously failed. That was the actual broken behavior, so this is the proof that matters — `submodule update` in the existing checkout would have kept resetting to the old recorded gitlink and told me nothing.

I then regenerated `bun.lockb` against the corrected submodules and confirmed the drizzle result held: no split store entries, all four consumers on one hoisted **0.45.2**. That lockfile is now committed — last cycle I withheld it precisely because it was derived from a tree with 8 stale submodules, and that objection no longer applies.

Both checkouts are synced to `6201622` with clean trees. `rappstack/dev` shows only an untracked `.idea/`, which I left alone. I did not touch the `agent-loop` superproject — it has ~30 modified paths from concurrent sessions, and none of this turn's work belongs there.

Two caveats carried forward unchanged. `better-sqlite3` still fails its native build against Node v26, so installs need `--ignore-scripts`; and whether this clears `#rappts`'s 15-error youtube cluster still needs a typecheck in brookebrodack-dev, which I can't run from this side.
rappstack uses bun.

### Re: rappstack uses bun — opus-4-8

> **Queue prompt:** 📌 rappstack uses bun.

Noted and verified — everything I ran last cycle was bun. `bun install`, `bun install --force`, `bun install --ignore-scripts`, and `bun pm ls` for the resolution check; the committed lockfile is `bun.lockb`. No npm, pnpm, or yarn command touched the repo.

I confirmed the tree agrees rather than taking my own word for it: `bun.lockb` is the only lockfile in `rappstack/dev` or any of its submodules — no stray `package-lock.json`, `pnpm-lock.yaml`, or `yarn.lock` — and the workspace uses `workspace:*` protocol deps throughout.

One phrase of mine is worth disambiguating, since it reads like a contradiction. The submodule commits I rebased and pushed were titled *"bump version to 0.1.0 for npm publishing"*, and `#rappsnippetarchive` repeats that wording. That is the upstream commit author's phrasing and refers to the **npm registry** — the place bun publishes *to* — not the npm CLI. `bun publish` targets the same registry. Nothing in that work implies switching package managers.

The pin does sharpen one open item, so I filed it as `#rappsqlite`. `better-sqlite3` fails its node-gyp native build against Node v26, which is what made `bun install` abort partway through linking and leave stale package symlinks behind — the exact mechanism that silently recreated the drizzle 0.32.2/0.45.2 split universe I spent last cycle chasing. `--ignore-scripts` is a workaround, not a fix, and it will bite again on any clean install. The real fix stays inside bun: a `better-sqlite3` release with Node 26 prebuilds, a pinned toolchain Node for installs, or dropping it if it is only a transitive dev dependency.

Exchange is at 183 lines across 8 response sections; queue is active so I am not stalling to compact.
<!-- agent:boundary:3290799a -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority go -->
- do [#rappsqlite]
- ~~📌 rappstack uses bun.~~
- do [#rappsnippetarchive]
- do [#tsnextpin]
- do [#lzeffectvoid]
- do [#tsappclean]
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
- [ ] [#rappsqlite] better-sqlite3 fails its node-gyp native build against Node v26 in rappstack/dev (v8::Global ctor arity mismatch in better_sqlite3.o), so every 'bun install' aborts mid-link and leaves stale package symlinks — that is what silently reproduced the drizzle 0.32.2/0.45.2 split universe. Workaround today is 'bun install --ignore-scripts'. Fix properly: bump better-sqlite3 to a release with Node 26 prebuilds, pin the toolchain Node version for installs, or drop it if only a transitive dev dep. Stays in bun (see pinned note: rappstack uses bun)
- [ ] [#rappsnippetarchive] btakita/snippet--rappstack is ARCHIVED on GitHub (read-only), so its unpushed 'bump version to 0.1.0' commit a61f06b can never be published. rappstack/dev's gitlink now points at the existing remote main 94e07aa and the local bump is dropped (preserved as tag backup/pre-rappgitlink in agent-loop/src/rappstack-dev/lib/snippet--rappstack). Decide: unarchive the repo and re-push the bump, move the package to a new remote, or leave it pinned at 94e07aa. Note its remote main also carries a relysjs->rhonojs migration the local branch never had
- [ ] [#tsnextpin] 186 of 190 ctx-core-dev packages carry `typescript: next` in devDependencies. That is what silently pulled a TS 7.1 dev build into rebuildjs and broke its type-check on bun-types (TS2309), which in turn blocked prepublishOnly. Pin to a stable range repo-wide as rebuildjs now is (^7.0.2), and add skipLibCheck where a dependency .d.ts is the failing surface. KEPT OPEN: not started — sequence after #pushctxcore so it does not stack a second uncommitted sweep on top of the unpushed dependency sweep
- [ ] [#lzeffectvoid] lazily-js types EffectRun as `() => (() => void) | null | undefined`, so a block-bodied effect arrow (inferred `void`) will not type-check and every cleanup-free effect must write `return undefined`. Widen to `| void` in src/lazily-js and mirror to the other bindings; then drop the three explicit returns in lib/ui--browser--brookebrodack/guestbook/hyop/guestbook__hyop.ts. KEPT OPEN: not blocked, just not started — the 2026-07-18 esbuild/dependency-sweep prompt took that cycle, and this is a published-package type change across the binding family rather than a tail-end edit
- [ ] [#ciappts] Add the app/brookebrodack-site typecheck step to .github/workflows/check.yml. Left out because it is red on 46 errors this package does not own (#tsbrowser + #rappts). Add once both clear
- [ ] [#yalcdrift] CONFIRMED, with a measurement: a fresh bun install re-resolves marked 13.0.3 alongside 17.0.4 and the two ctx/*.ts Marked errors (Marked<string, string> vs Marked) come back, taking the app type-check to 62 with 2 in its own sources. Verified this is resolution churn and NOT caused by any version bump — reverted both package.json files to HEAD, reinstalled, measured 62/2 identically. Root cause: one yalc'd @rappstack package pins marked ^13.0.2 while two pin ^17.0.4. Fix belongs in rappstack alongside the drizzle alignment (#rappdeps)
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
<!-- /agent:done -->
