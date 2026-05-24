---
agent_doc_session: 62fe1f41-886c-4f3d-b6fa-48d46106ec27
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
codex_args: -s danger-full-access
links:
- ./judith-krug.md
- ./robert-waldman.md
- ../personal/robert-ward.md
- ./bob-remeika.md
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md. DO NOT IMPLEMENT THE FIX.
  '#commit-push': commit all uncommitted files + push in claudescore-plugin, claudescore.com, and session-share
  '#commit-push-deploy': commit all uncommitted files + push in claudescore-plugin, claudescore.com, and session-share. deploy
  '#spec-test-commit-push': update spec + tests. commit + push
  '#spec-test-commit-push-deploy': update spec + tests. commit + push + deploy
---

## Status

<!-- agent:status patch=replace -->
25 pages deployed with form backend (D1 + R2 + email). Document pipeline architecture: Tier 0-3 (WAF -> Workers AI -> Claude -> Ragie).
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260524-130742.md`*

Compacted content:
- Archived 1 response topic(s): Portal brand home link
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f...
❯ Please create a Simulation Mortgage Office API based on our existing knowledge and https://developers.themortgageoffice.com/

❯ We have access to the TMO sandbox using brian.takita@gmail.com for today. Tomorrow we will lose access so we need to simulation to continue work during the week.
### Re: TMO simulator API — gpt-5

Implemented a local Simulation Mortgage Office API for EFS development.

What changed:

- Added `app/equityfundingsource/lib/tmo-sim/routes.ts`, mounted at `/tmo-sim` when `TMO_SIM_ENABLED=1`.
- Added `TMO_API_BASE_URL` support to the shared TMO wrapper and the borrower/admin call paths that previously hard-coded `https://api.themortgageoffice.com`.
- Seeded an in-memory TMO-style sandbox with loan, application, funding, attachment, payment schedule, borrower, product, and lender endpoints.
- Preserved TMO's JSON response envelope: `{ Data, ErrorMessage, ErrorNumber, Status }`.
- Added simulator docs at `docs/TMO/simulator.md` and `.env.example` entries.
- Added tests for the simulator lifecycle and API base URL switching.
- Tightened payment-schedule XML parsing to handle both documented `System.Data.DataTable` rows and the live sandbox's `Table` diffgram rows.

Local simulator setup:

```bash
TMO_SIM_ENABLED=1
TMO_API_BASE_URL=http://127.0.0.1:8787/tmo-sim
TMO_API_TOKEN=TMO
TMO_DATABASE=API Sandbox
```

Example:

```bash
curl http://127.0.0.1:8787/tmo-sim/LOS.svc/GetLoans \
  -H 'Token: TMO' \
  -H 'Database: API Sandbox'
```

External reference checked: [TMO API docs](https://developers.themortgageoffice.com/) and the public API docs/runtime at [api.absws.com](https://api.absws.com/). I also sampled live sandbox `GET GetLoans`, `GET GetProducts`, and `GET GetLoan/1007` with the documented sandbox headers; no live write calls were made.

Changed paths:

- `src/equityfundingsource-dev/.env.example`
- `src/equityfundingsource-dev/app/equityfundingsource/lib/admin/routes.ts`
- `src/equityfundingsource-dev/app/equityfundingsource/lib/loan_applications/routes.ts`
- `src/equityfundingsource-dev/app/equityfundingsource/lib/tmo-api/index.ts`
- `src/equityfundingsource-dev/app/equityfundingsource/lib/tmo-sim/routes.ts`
- `src/equityfundingsource-dev/app/equityfundingsource/worker.ts`
- `src/equityfundingsource-dev/docs/TMO/simulator.md`
- `src/equityfundingsource-dev/tests/tmo-api.test.ts`
- `src/equityfundingsource-dev/tests/tmo-sim.test.ts`

Verification:

- `bun test tests/tmo-api.test.ts tests/tmo-sim.test.ts` passed.
- `make check` passed: 43 tests, app build, and Worker build.
- `gh run list --workflow CI --limit 1` could not run because this repo has no workflow named `CI`; `gh workflow list` returned no workflows.
- `bunx tsc --noEmit` was attempted, but the current install is missing the configured `bun-types` type definition package, so that typecheck cannot run in this checkout as-is.
- No `tmux-ci` target or `.github` workflow exists in `src/equityfundingsource-dev`; local `make check` is the available project verification path.

Notes:

- Simulator state is in memory and resets when the Worker restarts.
- I left the pre-existing untracked `src/equityfundingsource-dev/tmp/` directory untouched.
<!-- agent:boundary:6d642bb6 -->
<!-- /agent:exchange -->
###

## Pending / Not Built

<!-- agent:pending -->
- [ ] [#qr72] [recommended] By May 27, 2026, send Lucas a written bridge plan: $500 now from personal funds if available, ask for the remaining $500 to be paid after EFS payment clears or by an agreed near-term date, and invoice EFS immediately for $5,000 with a smaller immediate continuation retainer accepted if Judith pushes back.
- [ ] [#s62t] [recommended] Prepare EFS invoice with truthful hours/value; Lucas cost is $4,150 (83h @ $50), so floor is 42h/$4,200 and recommended ask is 50-60h/$5,000-$6,000 with any concession as an explicit courtesy credit.
- [ ] [#37ah] [recommended] Broker portal backend: replace static broker demo data with TMO-backed submit-deal, pipeline, commissions, documents, and notifications
- [ ] [#7a4d] [recommended] OCR/TMO field extraction: custom/fillable docs -> reviewed extracted fields -> TMO update/create flow
- [ ] [#p40t] [recommended] Google Calendar/G Suite notification integration after Google admin credentials are available
- [ ] [#dvmr] [recommended] Corky: add long mono-call mixed-speaker recovery (separate diarization confidence, tighter bad-span detection, forced re-diarize on flagged windows)
- [ ] [#m6ch] Get Google Suite admin credentials from Robert (SMTP + Google Calendar integration)
- [ ] [#x5rn] Convert EFS logo JPEG → transparent PNG/SVG (Robert sent JPEG)
- [ ] [#q2x2] HTML email templates: lighter navy + gold, 2-3 drip for PBM orphan investor outreach
- [ ] [#kzt6] Collect testimonials for carousel (6–12 min: investors, brokers, borrowers) — push Judith + Robert helping
- [ ] [#k9zj] Graph/vector database for Judith's document search dashboard (cross-doc search, any doc type)
- [ ] [#dzmm] TMO webhooks: confirm outbound webhook support (Robert to check with Mortgage Office)
- [ ] [#4vb5] Online presence cleanup (after Judith approves)
- [ ] [#a3gc] Consolidate domains under equityfundingsource.com
- [ ] [#zhyg] Take down/redirect Weebly and WordPress sites
- [ ] [#vd6p] Update directory listings (Yelp, Manta, hard money directories)
- [ ] [#tfee] Consolidate Facebook pages
- [ ] [#1w4n] Optimize LinkedIn profile
- [ ] [#kxsx] Form submission pipeline
- [ ] [#njsx] Tier 0: Cloudflare WAF + upload limits
- [ ] [#x0px] Tier 1: Workers AI pre-screening
- [ ] [#xgqg] Tier 2: Claude API document review
- [ ] [#kf1p] Tier 3: Ragie Parse + Index for approved docs
- [ ] [#yh4b] Admin dashboard (Cloudflare Access auth)
- [ ] [#ft47] Token-based document upload flow
- [ ] [#4ktv] Discuss Ragie content validation with Bob Remeika (scheduled call)
<!-- /agent:pending -->

## Review

<!-- agent:review -->
- [/] [#jnzs] [recommended] Await Judith or EFS reviewer approval on funded-deal images, copy, lien-position wording, and legal/disclosure language before invoicing or launch sign-off. Technical D1 migration, Worker deploy, and live funded-deals/admin-route smoke tests completed 2026-05-23.
<!-- /agent:review -->
