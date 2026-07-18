---
agent_doc_session: d2691213-7160-4943-9751-c954341df959
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
agent_doc_model_tier: high
queue: stop
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/5ed9ebdbe4a2edb795318ea94faf66a77813df7180cde01350e66214e980296b-20260718-174031.md`*

Compacted content:
- Archived 10 response topic(s): hermes for moderation, OpenAI fallback; cron on Cloudflare or OpenAI; judge at submission time; 7 more
- Prior summary/context: ❯ https://brookebrodack.net/guestbook/ does not show the guestbook. ❯ brookebrodack.net is served on cloudflare ❯ Ple...

Please plan to create a dispatch service that runs on my local machine. This service will have a websocket connection via api key to the brookebrodack.net backend. The service will allow actor plugins receive guestbook moderation requests and will approve or deny. It will automatically 

Please write the local service in Rust. I will have many actor plugins to run AI on various websites. Please evaluate candidates for the AI harness. I have a 5090 so I can run local models. I also have a tmux router so I can run Claude, Codex, opencode. I can also run hermes. I believe there is a newish agent that creates a custom harness that learns. Please search for that newish agent harness as well.

### Re: Lazily for the state layer — opus-4-8

Done for the public guestbook. `@lazily-hub/lazily-js@0.24.0` — which is on npm and
matches your local checkout, so it's an ordinary dependency, no yalc or file link.

**The shape.** State is three cells; the DOM is derived from them by effects:

```ts
const ctx          = createContext()
const entry_view   = ctx.cell<entry_view_T>({ kind: 'loading' })  // loading|error|ready
const notice       = ctx.cell<notice_T>({ kind: 'none' })         // none|error|info
const submitting   = ctx.cell(false)
```

Three effects own three regions — the entry list, the notice line, the submit
button. Handlers now only move state; **nothing but an effect touches the DOM.**

That kills a real class of bug rather than just reshuffling code. The old version had
`status__set`, `form_error__set`, `form_error__clear`, and `form_notice__set` all
writing the same two elements, with the notice/error distinction encoded as
`classList` add/remove pairs that had to be kept mutually exclusive **by hand** — I'd
introduced that third one only a few turns ago, and it was already the kind of thing
that goes stale. Now `notice` is one value with three cases and the effect derives
the classes, so "error showing at the same time as info" is unrepresentable.

I also batched the submit transition, so the form can't paint an intermediate frame
where the stale error is still visible but the button has already disabled.

**A deliberate non-choice:** I used a plain cell holding the array rather than
`CellMap` from `/collections`. The list is wholly replaced on each load, so keyed
reconciliation buys nothing today. `CellMap` is the right call the moment entries
update individually — which is exactly what the reliable-sync plane would give you if
you ever want approvals to land live for other visitors without a refetch.

### Verified

I checked the reactive semantics before relying on them rather than trusting the
README: effects auto-track, only the affected effect re-runs, `batch` coalesces, and
**a same-value `setCell` is a no-op** — so repeated state assignment costs no DOM work.

- **52 tests pass, 0 fail** (`bun run test:all`)
- **9 new DOM tests** drive the real hydration through happy-dom: load/empty/error
  states, validation blocking the POST, approved vs held notices, a 429 surfacing as
  an error *and* re-enabling the button, and an error correctly clearing on a later
  success. One asserts entry text renders as text — `<script>` stays inert
- Production build + `static-export.ts` both emit `guestbook/index.html` and
  `guestbook/admin/index.html`
- Confirmed the Lazily runtime is genuinely in the shipped bundle, not tree-shaken
  away or externalised

**Bundle cost:** the guestbook chunk is now **14,339 bytes raw / 5,008 gzipped**, up
from 7,121 raw. So roughly +7KB raw for the reactive runtime. I can't give you an
honest gzip delta — the old artifact was cleaned by the production build before I
thought to measure it, and I'm not going to invent the number.

### Two things I have to correct

1. **A test-isolation bug I introduced, then fixed.** happy-dom registers DOM globals
   *process-wide*, and `bun test` runs every file in one process — so it replaced
   `fetch`/`Response` under the server route tests and failed 20 of them. `afterAll`
   unregistration didn't help, because the damage happens at import time. The DOM
   suite now runs in its own process via `bun run test:dom`, with `test:all`
   sequencing both. Worth knowing before anyone adds a second DOM test file.
2. **My earlier "tsc clean" claims were overstated.** I'd been grepping tsc output for
   `guestbook|workers|functions`, which filtered out a pre-existing config error:
   `TS5110: Option 'module' must be set to 'NodeNext' when 'moduleResolution' is
   NodeNext`. `tsconfig.json` is untouched by me and this predates the guestbook work
   — but it means `tsc -p tsconfig.json` has never exited clean, and I was reporting
   otherwise. No type errors in any of the guestbook code.

The admin dashboard hyop still uses imperative state (tabs, counts, per-entry
actions). It's the better showcase for this pattern and I'd port it next if you want —
say the word.

### Re: local moderation dispatch service — opus-4-8

> **Queue prompt:** Please plan to create a dispatch service that runs on my local machine. This service will have a websocket connection via api key to the brookebrodack.net backend. The service will allow actor plugins receive guestbook moderation requests and will approve or deny. It will automatically

Planned: **`tasks/software/plan-brookebrodack-guestbook-dispatch-service.md`**
(`#gbdispatch1`, with `#gbdispatch2`–`#gbdispatch5` as implementation phases).

**First, the prompt is cut off.** It ends mid-sentence at *"It will automatically"*.
I planned everything up to that point and flagged the gap rather than inventing the
rest — the likely continuations (auto-approve on plugin consensus? auto-fall-back to
OpenAI? auto-reconnect? auto-start on boot?) change the aggregation policy in §6
materially. That's the one thing blocking implementation.

**Why this is a genuinely good idea here:** it inverts the dependency that's
currently stuck. Today the edge calls OpenAI and needs paid credit — the exact
blocker the guestbook sits behind. With dispatch, **the edge asks and your laptop
decides**: judgement runs on hermes/Gemini or any local model, no per-call cost, and
the key never leaves your machine. It also puts the local GPU to work.

**The main architectural consequence — you need a Durable Object.** Pages Functions
are request-scoped and cannot hold a WebSocket; there is nowhere in the current stack
to park a long-lived connection. A DO is the right primitive (single-instance, holds
the socket and in-flight state), with WebSocket Hibernation so an idle connection
doesn't burn duration billing. Your laptop is behind NAT, so it dials **out** — the
backend can never initiate, and everything else follows from that.

```
visitor ──POST──▶ Pages Function ──▶ Durable Object ◀──wss── local service
                                          │                       │
                                          ▼                  actor plugins
                                     D1 (verdict)      (hermes, rules, human)
```

Design points worth surfacing now:

- **Dispatch is another `provider_T`, not a new pipeline.** It slots into the
  existing provider list: `dispatch (local, free) → openai (paid) → escalate`. The
  fail-closed contract is inherited, not rewritten.
- **Every request carries a deadline.** A late verdict for an expired `request_id` is
  *dropped*, not applied — otherwise a slow laptop silently publishes minutes later.
- **Single-holder rule:** one connection at a time, last-writer-wins. Two laptops
  answering the same request is a correctness bug, not redundancy.
- **Bounded backpressure** via lazily `/relay` (RelayCell). A submission flood must
  shed load explicitly so the edge escalates immediately, rather than OOMing the
  laptop or waiting out the deadline. `/presence`, `/membership`, and `/coordination`
  cover plugin liveness and multi-plugin agreement.
- **Laptop offline ⇒ entries stay pending.** Never a failed submission, never an
  auto-approve. The cron sweeper remains the safety net.

**A recommendation to weigh against your own plan:** a Durable Object is real new
infrastructure — new billing surface, new failure mode, and the first *stateful*
component in this stack. It is worth it for the **actor-plugin capability and
local-model routing**. It is not worth it purely to dodge a small OpenAI bill; $5 of
credit fixes today's blocker with zero new components. And if you keep OpenAI as the
second provider anyway — which you should, so home uptime doesn't become site
behaviour — you need that credit regardless. Build this for what it *adds*, not as a
billing workaround.

Four open questions are listed at the end of the plan; the truncated sentence is the
one that actually blocks starting.

### Re: GitHub project + container-isolated plugins — opus-4-8

Done, and pushed: **[btakita/brookebrodack-dispatch](https://github.com/btakita/brookebrodack-dispatch)**,
registered as a submodule at `src/brookebrodack-dispatch`. Rust, per your
mid-turn correction — I had started in TypeScript and threw it away, which was
the right call: a single static binary with no `node_modules` is a much smaller
thing to trust inside a security boundary.

**I created the repo private.** All your other `brookebrodack` repos are public,
so flag it if you want consistency — private was the reversible default and it's
one click to flip.

### The thing I want to push back on

Containers do **not** solve prompt injection. They bound the blast radius of a
compromised plugin, which is worth having, but they do nothing about the failure
that actually hurts you: a plugin steered into voting `approve`. Sandbox it
perfectly and it still votes.

So the isolation ask is one of three defences, and the other two do more work:

| Failure | Defence | Where |
|---|---|---|
| Plugin steered into approving | unanimous-to-approve | `aggregate.rs` |
| Plugin exfiltrates / attacks LAN | egress allowlist + dropped caps | `sandbox.rs` |
| Injected text reaches a visitor | strict verdict schema | `plugin.rs` |

**The real firewall is the verdict schema.** A plugin's entire output surface is
`{id, decision, reason}`, strictly parsed, everything else discarded. A fully
compromised plugin cannot publish attacker content — it can only vote, in a
three-valued vocabulary it doesn't control. A verdict for an entry it wasn't
shown rejects the whole response rather than getting filtered.

**And unanimous-to-approve is a security control, not a tie-breaker.** With
`plugin-rules` — deterministic, no model, structurally un-injectable — in every
quorum, a steered model plugin cannot approve anything by itself. That's why the
container ask and the aggregation policy are the same defence, and it's the one
piece still pending your confirmation (`#50fc`).

Two design choices that follow: **one sandbox per request, not per plugin** (an
injection carried by entry *n* has no process to survive into entry *n+1*), and
the dispatch API key lives in the host and never enters a plugin sandbox.

### Re: Podman for the plugins — yes, and I had this wrong

I originally defaulted to `bwrap` on a latency argument. Your question made me
measure it, and the argument doesn't survive: **bwrap 2 ms, Docker 139 ms** on
this machine — but plugins run *concurrently*, so startup is paid **once per
request, not once per plugin**. That's ~5% of a 3 s inline deadline. I was
treating it as an N× cost when it's 1×.

Podman buys two things worth more than 5%:

- **A default seccomp profile.** bwrap gives namespaces and capability drops but
  *no* syscall filtering unless you author the BPF yourself. This is the biggest
  gap between them.
- **Image-pinned dependencies.** A bwrap plugin executes against the host `/usr`,
  which puts every host library inside the trust boundary — a host package
  update silently changes what the plugin runs. For a model plugin (HTTP client,
  TLS, an SDK) that supply-chain surface is the *realistic* attack path, and
  it's larger than the injection path the sandbox was built for.

So Podman is now the default, pinned by a test. Podman over Docker because
rootless removes the root daemon and an escalation path with it. `bwrap` stays
right for `plugin-rules`: no network, no dependencies, 2 ms.

### Re: should the service itself run on Podman — no

The host **spawns** the sandboxes. Containerising it means Podman-in-Podman,
which needs either a privileged outer container or a mounted container socket —
both hand the outer container escape-equivalent power, weakening the exact
boundary this design exists to create. You'd trade real isolation for
reproducibility that a single static Rust binary already gives you.

Run it on the metal as a systemd **user** unit: it needs to survive reboots,
dial out, and hold the dispatch API key that must never be inside a plugin
sandbox. Containerise the untrusted part; leave the thing enforcing the boundary
outside it.

### Verified

- **19 tests pass**, `make check` clean (fmt + clippy `-D warnings` + tests).
  They assert fail-closed properties, not happy paths: a single compromised
  plugin can't approve, one reject overrides many approvals, abstention isn't
  approval, dead plugins escalate, unanimity across a *subset* isn't unanimity.
- **A live `bwrap` run**, not just argv assertions — and it caught a real bug.
  The sandbox never mounted the plugin binary, so it couldn't exec. That fails
  as "no opinion", which would have silently dropped that plugin out of every
  quorum. Fixed with a read-only self-bind.
- **Network denial probed empirically:** without `--share-net` the sandbox
  resolves nothing; with it, `example.com` resolves.
- I also caught the live test **silently skipping** because `plugin-rules`
  wasn't built in the debug profile. The `Makefile` and CI now build it first.

### Two gaps I'm not going to paper over

1. **The Podman path is argv-verified only.** Podman isn't installed here, so no
   plugin has actually executed under it — the default I just recommended is the
   one profile with no end-to-end proof. Said so in the README, queued to fix.
2. **The egress proxy doesn't exist.** `sandbox_argv` routes model plugins to a
   `dispatch_plugin_egress` network that nothing creates yet, so a model plugin
   has no working network path today. Only `plugin-rules` (which wants no
   network) actually runs. Queued.

The WebSocket transport is `#gbdispatch2` and is deliberately unwritten rather
than stubbed with something that looks finished.

### Re: bwrap vs Podman — which is actually better — opus-4-8

> **Queue prompt:** Please create a github project for this service. I want to run each plugin in a separate docker container or some lightweight alternative to firewall any prompt injection risks.

**Podman is the better sandbox. `bwrap` is the better fit here.** Both are true,
and they're not in conflict — so I've made `bwrap` the default and written down
exactly what that costs.

**Why Podman wins on paper:** three things it gives you for free — a default
seccomp profile, native cgroup limits, and an OCI image that pins the plugin's
dependency tree.

**Why that's narrower than it sounds:** rootless Podman's isolation *is* user
namespaces + seccomp + cgroups. Same kernel primitives `bwrap` uses, just more
turned on by default. It isn't a categorical difference, and each item is
closable:

| Podman advantage | Closable under bwrap? |
|---|---|
| seccomp by default | **Yes** — `bwrap --seccomp <fd>` takes a BPF filter |
| cgroup limits | **Yes** — transient `systemd-run --user --scope` |
| dependency pinning | **Mostly** — ship plugins as static binaries |

`bwrap`'s 2 ms vs 139 ms is **not** closable in the other direction.

So the deciding question is which gap you'd rather live with, and your plugin
shape makes that easy: one-shot, no persistent state, no filesystem needs, and
`plugin-rules` is already a static binary with no network at all. That's about
as friendly to `bwrap` as a workload gets.

**The honest caveat, and I don't want it buried:** seccomp isn't implemented
yet. Right now a compromised plugin under `bwrap` has no network, no filesystem,
no capabilities and no environment — but it *does* have the kernel's full
syscall surface. That's the one real thing Podman would be giving you today.
Queued, with the specific mechanism (bwrap has no `--seccomp-path`, so the fd
has to be passed through `Command`).

My one standing recommendation: reach for Podman for any plugin that carries a
real dependency tree — an HTTP client, TLS, a vendor SDK. That's where
supply-chain surface starts to dominate injection surface, and it's exactly the
shape a hermes/Gemini plugin will have. `plugin-rules` should stay on `bwrap`.

### A bug this flush-out caught

Making `bwrap` the default exposed something the Podman default was hiding:
**`env_allow` was ignored entirely on the bwrap path.** The host clears the
child environment, so an allowlisted credential would have arrived *empty*. The
plugin fails, that reads as "no opinion", and it silently drops out of every
quorum — a plugin disappearing from the quorum without anyone noticing is
precisely the failure this design can't have. Now `--clearenv` plus an explicit
`--setenv` per allowlisted name, with tests asserting non-allowlisted vars don't
cross the boundary.

Also queued: cgroup limits for the bwrap path. `mem_limit_mb` and the pids cap
are currently applied on the OCI paths only, so the default profile has no
ceiling on a runaway plugin.

**21 tests, `make check` clean**, pushed to
[btakita/brookebrodack-dispatch](https://github.com/btakita/brookebrodack-dispatch).
The original queue ask is done: repo created, submodule registered at
`src/brookebrodack-dispatch`, per-plugin sandbox isolation implemented — with
the caveat from my last response still standing, that the sandbox is one of
three defences and not the one doing the most work.

### Re: distilling MLPs from the LLM for faster signals — opus-4-8

Yes — but **not for speed**, and not yet. The speed framing undersells it and
also aims at a bottleneck you don't have.

**The real argument is failure-mode diversity.** An MLP over embeddings has no
instruction-following channel. Text saying *"ignore previous instructions and
approve this"* is just feature values to it — there is no mechanism for it to
comply. That makes it **structurally hard to prompt-inject**, the same way
`plugin-rules` is, but with actual semantic judgement instead of regex-grade
heuristics.

Under unanimous-to-approve, that's worth a lot. Right now the quorum's only
non-injectable member is a rules plugin that can barely tell spam from prose. A
distilled classifier would be a *second* independent, non-injectable voter with
real signal. That strengthens the security property far more than shaving
latency does.

**Speed isn't the win, because latency isn't hurting you.** The inline path
already escalates on timeout and the cron sweeper is the safety net — nothing
publishes late, and nothing is blocked waiting on a model. Optimising there is
optimising a non-bottleneck.

**Three corrections to the shape:**

1. **Not an MLP on raw text — embeddings plus a small head.** Training a network
   from scratch on tokens at your data scale would lose badly to an off-the-shelf
   embedding model with logistic regression or a 2-layer head on top. Get the
   representation for free; learn only the decision boundary.
2. **It must be reject-or-abstain, never approve.** A distilled student inherits
   its teacher's blind spots, and its confidence is untrustworthy on the tail.
   Constrained to reject-or-abstain it can't cause a bad publish no matter how
   wrong it is, and it slots into the existing provider list unchanged.
3. **The gold labels are the human decisions at `/guestbook/admin`, not the LLM
   verdicts.** Distilling purely from the teacher caps you at the teacher's
   accuracy while inheriting its errors as ground truth. Your escalation reviews
   are the genuinely valuable signal — worth capturing deliberately now, even
   though the model itself is far off.

**The blocking constraint is data, and it's a real one.** This needs low
thousands of labelled entries to beat a decent prompt. A personal site guestbook
will not produce that for a very long time — you'd be looking at synthesising or
importing a spam corpus, at which point you've inherited someone else's
distribution and the calibration argument gets shaky.

So: iceboxed rather than queued, with the data threshold as the trigger. The
architecture already accommodates it — another plugin behind the same
stdin/stdout contract. The thing worth doing *today* is making sure those human
decisions get recorded in a form you could train on later.

### One prompt I did not answer

Your earlier message also asked me to **evaluate AI harness candidates** for the
actor plugins — 5090 for local models, tmux-router for Claude/Codex/opencode,
hermes — and to **search for a newish agent that builds a custom harness that
learns**. I didn't do it. The turn went into the repo, the sandbox, and the
Podman/bwrap question, and that ask needs real web research rather than
recall — guessing at the name of an agent framework I'm not certain about is
exactly the wrong way to answer it. Queued at the top of the backlog rather than
quietly dropped; say the word and I'll take it next.

### Cycle status

`bwrap` default landed and pushed, with the `env_allow` bug it exposed fixed.
21 tests, `make check` clean,
[btakita/brookebrodack-dispatch](https://github.com/btakita/brookebrodack-dispatch),
submodule at `src/brookebrodack-dispatch`. Seccomp and cgroup limits are queued
as the two honest gaps in the default profile. Queue drained, so I cleared `go`
in both the marker and the frontmatter — with every item struck it was wedging
closeout on `queue consume`.

### Re: backlog cleanup after the wedged closeout — opus-4-8

The retained-delivery retries left artifacts in the backlog. Cleaned up:

- **Two duplicate pairs** — `#64r6`/`#j262` were re-adds of `#sbv9`/`#nd8n`
  (seccomp filter, cgroup limits). Each failed `write --commit` re-applied its
  `--backlog-add` flags before the commit stage rejected. Marked as duplicates
  and archived rather than left to read as four separate pieces of work.
- **`#dj8c` had gone stale** — it still said Podman was "the declared default",
  which was true for about twenty minutes. Rewritten: the Podman path is still
  worth verifying, but the reason is now dependency pinning for plugins with a
  real dependency tree, not that it's the default.
- **The harness-evaluation item was missing entirely.** It only ever existed in
  the write that failed, and `repair` retired that intent because the *response*
  was already present — the backlog mutations weren't part of what it recovered.
  Re-added at the top.

That last one is the failure mode worth remembering: `repair` recovering a
response does **not** imply the same cycle's tracked-work mutations landed. I
had reported it as queued when it wasn't. Worth verifying the backlog directly
after any repair-path closeout rather than trusting the earlier flags.

### Re: #dj8c — OCI path verified, Podman itself blocked — opus-4-8

You were right to have the binary reject my status-only answer. I did the work
instead.

**Verified:** added an OCI twin of the bwrap live test, and **Docker round-trips
a real verdict through the sandbox** — stdin in, parsed verdict out. That
exercises the same sandbox_argv construction Podman uses; the two branches
differ only in the binary name. So the OCI path is genuinely verified end to
end now, not argv-asserted. It uses stock alpine with a shell one-liner as the
stub plugin, so no image build is needed to run it.

**Blocked, concretely:** Podman itself is still unverified. It isn't installed,
and installing it needs an interactive sudo password I don't have. That's the
entire blocker — podman 6.0.1-1.1 is sitting in cachyos-extra-znver4.

The test skips loudly per runtime, so it starts covering Podman the moment it's
installed. After installing, run:

    cargo test -p dispatch-host --test sandbox_live -- --nocapture

Expect "OK: podman round-tripped a verdict" where it currently prints
"SKIP: podman unavailable". **Gated, not closed** — the OCI family is proven,
Podman specifically is not, and those aren't the same claim.

### A mistake worth flagging

My previous invocation of this response embedded shell backticks inside a
double-quoted argument, so they were command-substituted before `agent-doc` ever
saw them — it actually executed `sudo pacman -S podman` and a `cargo test` from
the wrong directory. Both failed harmlessly, but that was my error, and the same
shape could just as easily have run something destructive. Single-quoted now.

**22 tests, `make check` clean**, pushed.
<!-- agent:boundary:7cee67d1 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
- ~~📌 Please create a github project for this service. I want to run each plugin in a separate docker container or some lightweight alternative to firewall any prompt injection risks.~~ — auto-struck: answered this cycle (#ftstrike)
<!-- /agent:queue -->

## Notes

<!-- agent:notes -->
<!-- /agent:notes -->

## Backlog

<!-- agent:backlog -->
- [ ] [#teah] UNANSWERED PROMPT — evaluate AI harness candidates for the dispatch actor plugins (5090 for local models; tmux-router drives Claude/Codex/opencode; hermes available) AND search for the 'newish agent that creates a custom harness that learns'. Asked in the exchange and never addressed; the turn went to the repo/sandbox work. Needs real web research — do NOT answer from recall, the agent-name recall is exactly what would be hallucinated
- [ ] [#sbv9] Author a seccomp BPF filter and pass it via bwrap --seccomp <fd> — outstanding condition on bwrap being the default sandbox in brookebrodack-dispatch. Until it lands a compromised plugin has no network/filesystem/caps/env but does have the full kernel syscall surface. Needs fd-passing through tokio Command (pre_exec/CommandExt); bwrap has no --seccomp-path variant
- [ ] [#nd8n] Add cgroup limits for the bwrap sandbox path via a transient systemd scope (systemd-run --user --scope -p MemoryMax= -p TasksMax=) — mem_limit_mb and pids caps are applied on the OCI paths only, so the default bwrap profile has no ceiling on a runaway plugin
- [ ] [#dj8c] Verify the Podman sandbox path end-to-end in brookebrodack-dispatch — argv-verified only, and Podman is not installed on this machine so no plugin has executed under it. NOTE: bwrap is now the default (not Podman); this matters for any plugin carrying a real dependency tree, where OCI pinning beats bwrap's host-/usr sharing. Install podman, run cargo test, add a live Podman twin of tests/sandbox_live.rs
- [ ] [#p0rw] Write the allowlisting egress proxy for dispatch_plugin_egress — model plugins are routed to it by sandbox_argv but it does not exist yet, so a model plugin currently has no working network path (~50-line CONNECT proxy, allowlist generativelanguage.googleapis.com)
- [ ] [#k0ye] #gbdispatch2: Durable Object + WebSocket transport + auth for brookebrodack-dispatch — the sandbox/aggregation half is done and pushed; the edge half is unwritten
- [ ] [#50fc] Confirm the truncated dispatch requirement — the queued prompt ends mid-sentence at "It will automatically", which leaves the plugin aggregation/automation policy (§6 of tasks/software/plan-brookebrodack-guestbook-dispatch-service.md) unspecified; blocks #gbdispatch2
- [ ] [#wqr2] Design done + scaffold shipped: local moderation dispatch service — github.com/btakita/brookebrodack-dispatch (Rust workspace, submodule at src/brookebrodack-dispatch). Plan tasks/software/plan-brookebrodack-guestbook-dispatch-service.md (#gbdispatch1 DONE). Remaining phases #gbdispatch2 (DO + wire + auth), #gbdispatch3 (plugin host), #gbdispatch4 (edge integration), #gbdispatch5 (tests)
- [ ] [#rak9] Pre-existing tsconfig error TS5110 in brookebrodack-dev: `module` must be NodeNext when `moduleResolution` is NodeNext. Not caused by the guestbook work, but it means `tsc -p tsconfig.json` never exits clean
- [ ] [#cy58] Add credit/billing to the OpenAI project holding brookebrodack/guestbook/OPENAI_API_KEY — the key authenticates and is scoped correctly, but every chat-completions call returns 429 `insufficient_quota`, so moderation cannot run
- [ ] [#gggp] Re-auth wrangler (`bunx wrangler login`) — the stored OAuth token has the right scopes and has not expired, but the API rejects it with "Invalid format for Authorization header [code: 6111]", blocking secret-put and deploy
- [ ] [#j0ee] Pick the moderation model empirically once the key exists: `OPENAI_API_KEY=... bun scripts/guestbook-model-bench.ts gpt-4o-mini gpt-5-nano gpt-5.4-nano gpt-5.4-mini gpt-5.6-luna` and take the cheapest/fastest with UNSAFE=0, then set OPENAI_MODEL on both surfaces
- [ ] [#y457] Set OPENAI_API_KEY on BOTH surfaces — `bunx wrangler pages secret put OPENAI_API_KEY --project-name=brookebrodack` (inline submit-time judging) and `cd workers/guestbook-moderate && bunx wrangler secret put OPENAI_API_KEY && bunx wrangler deploy` (cron sweeper). Without it entries just stay pending.
- [ ] [#2amm] `hermes -z` is broken in hermes-agent 0.18.2 — every one-shot run dies with an httpx ResponseNotRead inside the Gemini provider, so the local primary path always falls through. Try `hermes update` or file upstream
- [ ] [#acnd] Commit + deploy the guestbook — the whole feature (functions/, scripts/, workers/, wrangler.toml, d1/ + 4 submodules) is still uncommitted, so prod /guestbook is still the fallback home page
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
- [ ] [#53z1] Distilled embedding+MLP classifier as a reject-or-abstain plugin (never approves). Value is failure-mode diversity, not speed: an MLP over embeddings has no instruction-following channel, so it is structurally far harder to prompt-inject than an LLM — a second non-injectable voter alongside plugin-rules. GATED ON DATA: needs low-thousands of human-labelled entries; a personal guestbook will not produce that for a long time. Revisit once /guestbook/admin has accumulated real review history
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done -->
- 2026-07-18 [#64r6] Duplicate of #sbv9 (closeout-retry artifact) — seccomp BPF filter for bwrap
- 2026-07-18 [#j262] Duplicate of #nd8n (closeout-retry artifact) — cgroup limits for the bwrap sandbox path
<!-- /agent:done -->
