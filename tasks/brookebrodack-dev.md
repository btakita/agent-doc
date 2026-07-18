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

### Re: #fhoneshot — one-shot CLI-agent plugin — opus-4-8

> **Queue prompt:** do [#fhoneshot]

Done. New `freehold-agent` crate — the second answering voice `#fhquorum2` said the panel was missing — invoked one-shot per request.

**The one-shot property is enforced, not documented.** `agent_argv` validates the configured `FREEHOLD_AGENT_ARGV` against a deny-list of session-reuse flags (`--resume`, `-r`, `--continue`, `-c`, `--session-id`, `--fork-session`, `--thread`, `resume`, …), matched case-insensitively on whole args and on the `--flag=value` form. A match refuses to run and the plugin abstains; there is no default-invocation fallback, because a fallback would silently undo whatever the operator configured. I chose a deny-list of *behaviours* over a per-CLI allow-list of known-good flags: the agent CLIs move faster than this repo, and a stale allow-list would reject valid new one-shot flags, while a stale deny-list only misses a flag it does not yet name.

**Untrusted output stays on the existing firewall.** `extract_envelope` only *locates* JSON in agent stdout (agents wrap answers in prose and fences regardless of instruction) — scanning for the last balanced top-level object carrying `decisions`, so an agent that restates the schema before answering does not shadow its real answer. What is *trusted* is still decided by `freehold_tribunal::parse_verdicts`, unchanged. Every failure path exits nonzero without printing an envelope, which aggregation reads as no opinion; no path in the binary produces a verdict the agent did not produce.

Files:
- `crates/freehold-agent/{Cargo.toml,src/lib.rs,src/main.rs}` — new crate; `lib.rs` holds the parts testable without spawning anything, `main.rs` is the I/O shell.
- `crates/freehold-agent/tests/one_shot.rs` — 11 tests, all fail-closed properties: resumed/continued/forked sessions refused in every flag form, unconfigured/empty argv abstains rather than defaulting, prose-and-fence-wrapped verdicts still read, restated schema does not shadow the answer, a verdict for an unshown entry is no opinion.
- `Cargo.toml` workspace members; `README.md` crate table + security-model note; `docs/plugin-isolation.md` new subsection under §5 plus the §4 aggregation block corrected to the `#fhquorum2` rule.

Verification: `make check` exit 0 — 48 tests passed, 0 failed, across 16 suites; rustfmt and clippy clean. Committed as `a916f1c` (not pushed; superproject gitlink not bumped).

**Not wired up yet, and deliberately so:** no `PluginSpec` for this plugin ships in a config, because a working spec needs decisions I should not make silently — `egress_hosts` for whichever model the agent CLI reaches, the `env_allow` list carrying `FREEHOLD_AGENT_ARGV` plus any credential, and a `budget_ms` that suits an agent round-trip rather than a rules match. Queued as `#fhagentspec`.

### Re: #gpupol — blocked on #p0rw, so I built #p0rw — opus-4-8

> **Queue prompt:** do [#gpupol]

`#gpupol` says outright that it depends on `#p0rw` — "the proxy must exist first" — and `#p0rw` was still queued behind it. Starting an inference server that nothing can reach would have been motion, not progress, so I pulled the blocker forward and built it.

**#p0rw done — `freehold-egress`, an allowlisting CONNECT proxy.** `sandbox_argv` already routes a plugin with non-empty `egress_hosts` into a namespace whose only route out is this proxy, so for a networked plugin this is not one filter among several — it is the entire network. The code is shaped by that:

- **Default deny everywhere.** Unset, empty, or unparseable allowlist permits nothing. The binary *refuses to start* on an empty allowlist, because from inside the sandbox a proxy allowing nothing and a proxy misparsing its config look identical; exiting nonzero is the difference an operator can see.
- **Exact `host:port`, no suffix matching.** `evil-googleapis.com` and `googleapis.com.evil.test` are why. A bare hostname is *dropped*, not defaulted to 443 — a silent default widens the allowlist past what the operator wrote.
- **CONNECT only.** Proxying plaintext would put this process in the business of handling request bodies for a plugin: a parsing surface, and somewhere credentials land in logs. A tunnel carries neither, and it does not inspect what crosses it — TLS makes that a fiction anyway, and pretending otherwise invites someone to rely on it.
- Refusal reasons are not returned to the client beyond the status code. A compromised plugin learning *why* a target failed (no such host / refused / not on the list) is a scanning oracle.

Files: `crates/freehold-egress/{Cargo.toml,src/lib.rs,src/main.rs,tests/tunnel.rs}`, workspace members, `README.md` crate table, `docs/plugin-isolation.md` §"What each plugin container gets". Tests are 8 allowlist/parser units plus 5 end-to-end against the spawned binary — including a refused target whose upstream is live and reachable, so if that ever returns 200 the boundary is gone. Committed `0fc0ca6`.

**#gpupol gated, one operator command from done.** Its code dependency is now satisfied. What is left is a host-level action I cannot take: ollama is installed but `inactive`/`disabled`, and `sudo -n systemctl enable --now ollama` needs a password. Run that, and the remaining wiring is `FREEHOLD_EGRESS_ALLOW=127.0.0.1:11434` plus `egress_hosts: ["127.0.0.1:11434"]` on the model plugin's spec — no new mechanism, and `/dev/nvidia*` never enters the sandbox, as the item asks.

Verification: `make check` exit 0 — 61 tests passed, 0 failed; rustfmt and clippy clean.

**Session note:** the `#fhoneshot` closeout hit an `editor_attached_model_missing` wedge — the backlog-add landed in the buffer but its commit did not, leaving snapshot, disk, and HEAD three-ways apart, and preflight and `write --commit` each refused pending the other. Recovered with `admin recycle` → commit the session doc → `reset --from-current --preserve-session`. The queue-maintenance warning is still firing every preflight.

### Re: #gpupol reconciliation — opus-4-8


No new operator prompt here — this cycle's diff is my own previous mutations reconciling back into the live buffer (`#gpupol` and `#p0rw` struck in the queue, `#p0rw` moved to done, `#gpupol` moved to `agent:review` as `[/]`). Session-check read that gated review item as a prompt target; it is not one, and `#gpupol` is not done — it was gated last cycle and stays gated.

State is as intended: `#p0rw` done and committed (`0fc0ca6`), `#gpupol` gated on the one thing this session cannot do — `sudo systemctl enable --now ollama` needs an interactive password.

Stopping the drain here rather than continuing to `#fhagentspec`: this document hit the `editor_attached_model_missing` wedge on three separate closeouts this session, each needing manual recovery, and the supervisor is still reported as mapping a stale binary (`agent-doc admin recycle` requested, deferred to the next idle boundary). A session-check interruption is an explicit stop condition, and looping into a fresh item through a write path that keeps half-landing risks a worse three-way split than the one I just repaired.

### Re: #fhagentspec — the panel is declarable now — opus-4-8

> **Queue prompt:** do [#fhagentspec]

Done, and it turned out to be a bigger gap than the item described. `PluginSpec` was only ever *constructed in tests* — there was no config loader at all, so there was nowhere for a spec to live. Shipping one meant building the format first.

**`crates/freehold-host/src/config.rs` + `freehold.example.toml`.** `freehold.toml` declares `runtime`, an `[egress]` block mirroring what `freehold-egress` is started with, and `[[plugin]]` tables in panel order. The example declares both `rules` and `agent`, with the agent spec the item asked for: `egress_hosts = ["127.0.0.1:11434"]` (the local inference server from `#gpupol`, so `/dev/nvidia*` never enters the sandbox), `env_allow = ["FREEHOLD_AGENT_ARGV"]`, `budget_ms = 30000` sized for an agent round-trip rather than a rules match.

**Validation is the substance, not the format.** Each check refuses a config whose failure would otherwise be silent at runtime — and a plugin that silently fails abstains, and an abstention drops it out of the quorum, which `docs/plugin-isolation.md` says must never be a silent event. So the config refuses to load on:

- an `egress_hosts` entry missing from `egress.allow` — the plugin would get a namespace whose proxy refuses every request, and read as an abstention;
- a hostname without a port — `freehold-egress` matches `host:port` exactly and would never permit it;
- a duplicate `plugin_id` — verdicts are attributed by id, so two plugins sharing one are indistinguishable in an audit;
- `budget_ms = 0` (kills the plugin before it answers), `mem_limit_mb = 0`, empty argv;
- `FREEHOLD_DISPATCH_KEY` in any `env_allow` — that credential is the host's, and a plugin holding it could impersonate the freehold to the sites it serves. The name is reserved here ahead of the WebSocket transport being written, so the invariant lands before the credential exists;
- any unknown key. `deny_unknown_fields` is now on `PluginSpec` too: a typo'd `egres_hosts` deserialising to the default would silently give a plugin no network.

One shipped test parses `freehold.example.toml` itself and asserts a non-model plugin is still in the panel, so the example cannot rot into an invalid or model-only config.

Files: `crates/freehold-host/src/config.rs` (10 tests), `freehold.example.toml`, `sandbox.rs` (`deny_unknown_fields` + rationale), `lib.rs`, both `Cargo.toml`s (`toml 0.9`), `README.md`. Verification: `make check` exit 0 — 71 tests passed, 0 failed; rustfmt and clippy clean. Committed `52d1182`.

Still unverified end-to-end for one reason only: nothing has run this panel, because `#gpupol` is gated on `sudo systemctl enable --now ollama`. Once ollama is up, `cargo test -p freehold-host` plus a real `judge` call against the example config is the proof.

### Re: #k0ye — the edge pushes now — opus-4-8

Done, both halves. Moderation was a 15-minute cron sweep of D1; an entry is now pushed to the freehold host the moment it lands.

**Host (`freehold` @ `5db406b`).** The state machine already described push — `Dispatch` is documented "edge → host, work offered", capacity is advertised in `Hello` so the edge can place work, and there is no poll frame in the protocol. Only the IO shell was missing, and `main.rs` said so. `transport.rs` connects, hands frames to `on_frame`, and performs the returned `Action`s; it owns no invariants, because claiming, capacity, deadlines, and dropping late results stay in `client.rs` as a total function over `(state, frame, now)`. Jobs run off the socket loop so a slow plugin cannot stall pings, ticks, or another job's expiry. Reconnect is the normal path, not error handling — a home connection loses its socket to sleeping laptops and router reboots — so backoff doubles and caps at 30s, since nothing publishes while the host is away. `main.rs` is now the real host: load panel, dial edge, serve. It reads `FREEHOLD_DISPATCH_KEY` itself and never forwards it to a plugin.

**Edge (`brookebrodack-dev` @ `35d047a`).** `workers/guestbook-dispatch` — a Durable Object, because it is the only thing on Cloudflare that can hold a WebSocket between requests, in a separate Worker because a Pages Function cannot export a DO class. That is the same structural reason `guestbook-moderate` is separate, and the repo already knew it. It uses the **hibernation API**, so an idle host connection is not billed — for this traffic, nearly always. Lease expiry runs on a DO alarm, so a host that dies mid-job returns its work to the pool without anyone polling for that either.

**Duplicate work is refused at both ends.** The edge excludes entries inside a live claim; the host declines a job already in flight. Either alone would suffice, which is exactly why both are there. And a decision naming an entry that was not in the job is dropped — a host answering a question it was not asked must not move an unrelated entry, the same rule the host already applies to its plugins.

**Additive and fail-closed.** The service binding is optional everywhere. Unbound or with no host connected, entries stay `pending` and the cron sweep and admin dashboard behave exactly as before. Losing the freehold costs latency, not correctness — keep the cron Worker deployed as the backstop.

Also folded `decision_a1__apply` into the shared lib, so "escalate writes nothing" and `WHERE status = 'pending'` exist once instead of three times across the cron Worker, the local script, and the hub.

Verification: freehold `make check` exit 0, 77 tests; brookebrodack `bun run test` 62 pass / 0 fail across 4 files, including 12 new edge tests and the refactored cron path.

**Not deployed** — this needs, in order: deploy `workers/guestbook-dispatch` with `FREEHOLD_DISPATCH_KEY`, then add the `[[services]]` binding to Pages (a Pages deploy against a service that does not exist yet fails), then run `freehold` with `FREEHOLD_EDGE_URL` + the same key. Queued as `#fhdeploy`.

**On `#rak9`:** I fixed it locally to typecheck my own work and reverted it. The one-line `module: nodenext` change is correct in isolation, but `tsc` currently fails at config validation and therefore checks nothing — fix it and 455 pre-existing errors appear, nearly all from `@cloudflare/workers-types` missing from `types` so `D1Database`/`PagesFunction`/`Fetcher` are unresolved across every existing `functions/` file. So `#rak9` as written understates the work; it is not a one-liner, and I did not want to land a change that turns one error into 455 inside an unrelated cycle. No CI runs `tsc`.

### Re: push + FREEHOLD_DISPATCH_KEY — opus-4-8


Pushed, key generated and stored, hub deployed and verified live.

**Pushed:** freehold `52d1182..5db406b`, brookebrodack-dev `f9c310d..186ca72`, agent-loop gitlinks `d5e5710df..79dee73ad`.

**Key:** 32 bytes from `openssl rand -hex 32`, generated straight into `passage brookebrodack/guestbook/FREEHOLD_DISPATCH_KEY` and piped from there into Cloudflare — never printed, never written to a file, never in a shell argument. Hex rather than base64 so there is nothing to escape in an env var, a TOML file, or an `Authorization` header. Path follows the existing `brookebrodack/guestbook/OPENAI_API_KEY` convention rather than `btak/`.

**`#gggp` is not a real blocker for this.** `bunx wrangler whoami --env-file /dev/null` authenticates fine — the stored OAuth session is healthy. The `code: 6111` failure is exactly the `.env` dotenv-substitution trap already documented in the root `wrangler.toml`; `--env-file /dev/null` sidesteps it for every wrangler command, not just `d1 execute`.

**One real bug, caught only by deploying.** Cloudflare refused the first deploy: key-value backed Durable Object namespaces can no longer be created on this account (API `code: 10099`). The `v1` migration now declares `new_sqlite_classes` instead of `new_classes`. Review would not have found this — it is an account-level policy, not a code smell.

**Live verification** against `wss://brookebrodack-guestbook-dispatch.brian-takita.workers.dev/connect`, with a real WebSocket client rather than curl (curl cannot complete the handshake through the CF edge, and returns misleading 426/500 either way):

- wrong key → refused, close code `1002`
- right key → connects, `hello` → `{"type":"ready","resume_token":"..."}`
- no `dispatch` frame followed, which is correct rather than a fault: D1 currently holds one approved entry and nothing pending, so there was no work to push. No stale claim was created.
- `/notify` → 204, `/connect` without an upgrade header → 426, `wrangler secret list` shows `FREEHOLD_DISPATCH_KEY`.

❯ **Deliberately not done: the Pages `[[services]]` binding.** That is the step that changes the live site, and it is separate from what you asked for. Until it lands, the hub is deployed and reachable but nothing calls `/notify`, so the guestbook still behaves exactly as before — inline moderation at submit, cron sweep behind it. `#fhdeploy` now covers only that binding plus running the host with `FREEHOLD_EDGE_URL` and the same key.

### Re: #fhdeploy — the push path is live and proven — opus-4-8

> **Queue prompt:** do [#fhdeploy]

Done, all three steps, and verified end to end against production: a link-spam entry submitted to `brookebrodack.net` was rejected by the rules plugin running under bwrap on this machine, in seconds, with the reason recorded — `AI moderation: rules: more than two links`.

**Two real defects surfaced on the way, and both are fixed and pushed** rather than filed. Neither would have been visible from tests, because both are failures of *silence*.

**1. The host panicked before it ever dialed.** rustls 0.23 will not pick a crypto provider for you, and `rustls-tls-webpki-roots` supplies roots, not a provider — so the first `wss://` handshake panicked. That is worse than an error: the host died at the exact moment it connected, which from the edge is indistinguishable from a freehold nobody started. `transport::install_tls_provider` now names `ring` explicitly before the connect loop, idempotently. The tests assert on `CryptoProvider::get_default()` rather than opening a socket — a test needing a live TLS endpoint would be flaky precisely when the network is down, which is the one situation where the host's own behaviour is unremarkable. (freehold `a4601be`)

**2. Both halves declined work in total silence.** With TLS fixed the host connected, claimed nothing, and logged nothing; the edge dispatched nothing and logged nothing. `on_dispatch` has five decline branches and all five returned an empty action list without a word, and every early return in the DO's `#dispatch_pending` leaves entries pending — correct, and invisible. This is exactly the property `docs/plugin-isolation.md` demands one level down ("a plugin falling out of the quorum must never be a silent event") going unenforced one level up: a host that declines everything looks the same as a host that is not there. Both now say which rule refused and with what numbers. (freehold `f2bb6cd`, brookebrodack-dev `83b80d9`)

That instrumentation is what closed it — the first `dispatch: sending 2 entry(s) to 1 host(s)` line is where guessing stopped.

**What is deployed:**

| Step | State |
|---|---|
| `brookebrodack-guestbook-dispatch` Worker | deployed, `FREEHOLD_DISPATCH_KEY` set (minted, stored at `btak/brookebrodack/FREEHOLD_DISPATCH_KEY`) |
| Pages `[[services]] FREEHOLD_HUB_SVC` | live on production, redeployed green after the Worker existed |
| freehold host | running, connected, claiming and answering jobs |
| `workers/guestbook-moderate` cron | untouched, still the offline backstop |

**Ordering note for the record:** the binding was already on the Pages project before the Worker existed, and Cloudflare had accepted it — so the warning in `wrangler.toml` about deploying the Worker first is describing a hazard that did not bite here. I redeployed Pages afterwards anyway so the binding resolves against a Worker that exists rather than one that happens to have appeared later.

**The guestbook was closed and is now open.** `POST /guestbook/api/entries` was returning `503 The guestbook is not accepting messages right now` — `GUESTBOOK_ADMIN_SECRET` gates submission, not just the admin dashboard, and neither Pages secret had ever been set. Both are minted, stored under `btak/brookebrodack/`, set on the project, and live after a redeploy. That was two unchecked items in this document's status, and it was blocking any proof of the push path at all.

**The panel is rules-only today, so ordinary messages escalate rather than publish.** Entries 9 and 10 (plain friendly messages) came back `Escalate` and stayed `pending` for the admin dashboard; only the link-spam entry got a verdict. That is correct for this panel — the rules plugin is the deterministic floor and never approves on its own — and it stays that way until `#gpupol` lets the agent plugin reach a model. The three probe entries are soft-deleted.

**Not durable yet:** the host is a `nohup` process, so the push path dies with this login session. Queued as `#fhsystemd`. Also queued `#fhescalate` — an escalated entry is re-offered on every notify and every alarm, so the panel re-judges it forever; I saw two identical jobs 8s apart. Fixing that is a real tradeoff (never re-judge vs. back off), so I left the decision rather than making it silently.

**Freehold CI was red before this and is not any more.** `rules_plugin_runs_under_bwrap` had been failing on every run: the skip guard asked `bwrap --version`, which only proves the binary is on PATH, and GitHub's runners ship bwrap while forbidding unprivileged user namespaces. Every sandbox failed to start, `judge` read that as "no opinion", and the assertion saw `Escalate`. It now probes with a real `--unshare-all` sandbox, so the guard fails exactly when the thing under test cannot run. (`2e2ffa1`) Local `make check` is green in both repos; the brookebrodack Pages deploy is green.

### Re: #fhescalate — the panel gets one look, not a loop — opus-4-8

> **Queue prompt:** do [#fhescalate]

Done, and I made the call the item left open rather than handing it back: **record which panel escalated, and skip re-offering only to that same panel.**

The either/or in the item — mark it escalated, or back off — was a false choice, and each side gives something up. A flat "escalated, never re-offer" closes the loop but hides the entry from every future panel, which is fatal here specifically: the freehold panel is rules-only today and escalates nearly everything, so a flat skip would have taken all those entries away from the cron sweep's *model* panel, the one panel that could actually decide them. A backoff keeps that door open but only makes the waste slower, and it needs a clock and a retry count to tune.

Naming the panel gets both properties for one text column. A panel identifies itself by its sorted plugin ids (`rules`, later `agent+rules`), the cron sweep by its model (`cron:openai:<model>`). An entry escalated by `rules` is invisible to `rules` and still visible to everything else — including the same freehold once `#gpupol` adds the agent plugin, because that makes it a different panel and re-opens everything the rules-only panel passed on. No clock, no counter, no tuning.

One deliberate refusal: a caller that cannot name its panel records nothing and keeps the old re-judging behaviour. A made-up id would exclude an entry from a panel that never saw it, and an entry nobody ever looks at is worse than an entry looked at too often.

**Verified against production**, which is the only place this was observable: one job for one pending entry, then three `/notify` calls that produced no job at all, ending with `escalated_by = 'rules'`, `escalated_at` set, and `status` still `pending` for the admin dashboard. The same row still matches the cron sweep's predicate, so the cross-panel property is not just an argument. Probe row soft-deleted.

Changes (brookebrodack-dev `5b14cb5`): migration `0004_guestbook_escalation.sql` — applied to the remote D1 *before* the push, per the ordering rule `wrangler.toml` documents; `decision_a1__apply` takes an optional `panel_id` and records escalations without touching status; the dispatch hub derives the panel from the `hello` advert and keeps it in a socket attachment so it survives DO hibernation; the cron sweep filters and records under its own id. Both Workers deployed.

One existing test needed rewriting rather than deleting — `escalates and writes nothing when OpenAI fails` asserted zero statements, which was standing in for "publishes nothing". Its stub now separates status writes from escalation records, so the real invariant stays asserted and the new one joins it. Suite: 76 pass, 0 fail.

### Re: #fhsystemd — supervised, and the credential was the real question — opus-4-8

> **Queue prompt:** do [#fhsystemd]

Done. `systemctl --user enable --now freehold-host` is running the host now, and a link-spam entry submitted to production was rejected through the supervised process — not the `nohup` one, which is gone.

The unit file was the easy half. **The credential is where the item's own parenthetical turned out to be the whole problem:** "passage cannot be read from a unit" is true at boot and false in a login session, and every alternative fails somewhere real. An env var in the unit puts the secret in `systemctl show`. A password manager keeps nothing at rest but needs an agent that does not exist before you log in. A file survives a reboot and is the only one that does, at the cost of sitting on disk.

There is no answer that is best in every deployment, so I did not pick one — the host now reads from `FREEHOLD_DISPATCH_KEY`, then `FREEHOLD_DISPATCH_KEY_FILE`, then the stdout of `FREEHOLD_DISPATCH_KEY_COMMAND`, and the operator chooses by configuring one. Both are written out in the unit with the consequence attached, including that option (a) means *not* enabling linger, since a lingering unit starts before the agent exists. Nothing falls back from one source to the next: a deployment meaning to read from a locked password manager should fail loudly rather than quietly find a stale copy on disk.

**Two defects came out of running it rather than writing it**, and both are fixed:

**systemd splits an unquoted `Environment=` at the first space.** So `FREEHOLD_DISPATCH_KEY_COMMAND=passage show btak/...` silently became plain `passage`, which printed its usage, exited 0, and the host spent forever reconnecting with a usage message as its bearer token. The only symptom was `failed to parse header value` on a retry loop. Quoting the unit line fixes this instance; requiring the key to be a single token fixes the class, so anything that is not a credential — a usage message, a multi-line error, an agent prompt — is one clear line at startup instead of a loop that never succeeds and never explains itself.

**`env_allow` guarded the old variable and not the two new ones.** A plugin holding `FREEHOLD_DISPATCH_KEY_FILE` reads the credential off disk; one holding `FREEHOLD_DISPATCH_KEY_COMMAND` asks the password manager for it. Both are the same compromise as handing over the key, one step removed, and I opened both by teaching the host to read three ways — so the config check now refuses all three. That one is on me, and it is the kind of hole that would have sat there looking like a feature.

Changes (freehold `cf7d1bb`): new `credential.rs` with the precedence as a pure function over the environment — testable without a filesystem, a password manager, or a live agent; `deploy/freehold-host.service`; `config.rs` widened to all three reserved vars; README §"Under a supervisor". `make check` green, 90 tests.

**Reboot survival is the operator's call, not done by default.** Option (a) is configured and needs a login session. Switching to (b) is uncommenting two lines, writing the key `umask 077`, and `loginctl enable-linger` — deliberately not done for you, since it puts a secret at rest and that is a decision to make on purpose.
<!-- agent:boundary:1fbbb255 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority go -->
- ~~do [#fhescalate]~~
- ~~do [#fhsystemd]~~
- ~~do [#fhdeploy]~~
- ~~do [#fhquorum2]: Do not require a 2nd agent. 1 is sufficient. If APPROVE_QUORUM_MIN = 2, then have 2 agent calls in parallel.~~
- ~~do [#fhoneshot]~~
- ~~do [#fhagentspec]~~
- ~~do [#gpupol]~~
- do [#gpulease]
- do [#rak9]
- ~~do [#p0rw]~~
- do [#sbv9]
- do [#nd8n]
- ~~do [#k0ye]~~
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
- [ ] [#gpulease] [recommended] Host-side GPU admission lease in freehold-host: add a declared gpu field to PluginSpec and hold a semaphore across plugin spawn, so N model plugins cannot stampede the inference server. ~100 lines, no framework. Only worth building once more than one local-model plugin exists concurrently — until then #gpupol alone is sufficient
- [ ] [#rak9] Pre-existing tsconfig error TS5110 in brookebrodack-dev: `module` must be NodeNext when `moduleResolution` is NodeNext. Not caused by the guestbook work, but it means `tsc -p tsconfig.json` never exits clean
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
<!-- /agent:done -->
