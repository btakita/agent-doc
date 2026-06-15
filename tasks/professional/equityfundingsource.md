---
agent_doc_session: 62fe1f41-886c-4f3d-b6fa-48d46106ec27
agent: claude
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
  '#advance-review': Go through the review items. Add actionable backlog items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md. DO NOT IMPLEMENT THE FIX.
  '#commit-push': commit all uncommitted files
  '#commit-push-deploy': commit all uncommitted files + deploy
  '#mermaid': Include explanatory mermaid diagram(s).
  '#spec-test-commit-push': update spec + tests. commit + push
  '#spec-test-commit-push-deploy': update spec + tests. commit + push + deploy
queue: start
---

## Status

<!-- agent:status patch=replace -->

committed
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260615-035459.md`*

Compacted content:
- Archived 4 response topic(s): Move applied deals to a Pending deals section + deploy; Applied deal still in Available deals; deploy; 1 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f...

### Re: do [#cgw7] — wire broker email + in-app notifications — opus-4-8

> **Queue prompt:**
>
> do [#cgw7]


Done. Broker events now generate in-app notifications and (opt-in) email. New module `app/equityfundingsource/lib/broker/notify.ts` with three triggers, wired into `lib/broker/routes.ts`:

- **Deal submitted** (`POST /api/broker/deals`) → in-app + email to all active admins.
- **Deal status change** — both broker self-transitions (`PATCH /api/broker/deals/:id/status`) and admin promotions (`POST /api/admin/broker-deals/:id/status`) → owning broker. Status-specific copy; `docs_needed` is the "documents requested" event.
- **Commission recorded / marked paid** (`POST /api/admin/broker-commissions`, `.../mark-paid`) → owning broker.

Design notes:
- In-app notifications (`notification_create_` → shared `notifications` table, surfaced by the existing `GET /api/broker/notifications`) **always fire**. Email is opt-in behind `BROKER_EMAIL_NOTIFICATIONS_ENABLED` (Cloudflare Email Service or local capture), mirroring the borrower-message notify pattern. `BROKER_NOTIFICATION_FROM` overrides the From (falls back to `EFS_EMAIL_FROM`). Both documented in `.env.example`.
- Side-effects run via `notify_after_` (`ctx.waitUntil`) and swallow errors, so a notification failure never breaks the broker request.

Added `lib/broker/notify.test.ts` (4 tests, all pass). Full suite green: **699 pass / 0 fail**, plus `build` + `build:worker` (typecheck) clean. Committed in the submodule (`2f3337b`) and bumped the superproject pointer (`b06386894`).

One follow-up not in scope here: the broker portal needs a **deal-detail view** at `/portal-broker/deals/:id` for the status-email link to resolve — added as a backlog note if that route doesn't exist yet.

### Re: last EFS call — do the "positions" make sense given EFS doesn't service loans? — opus-4-8

> **Queue prompt:**
>
> Analyze the last EFS call transcript. EFS does not handle payments. Are the positions something that makes sense to keep?


Short answer: **yes, keep the investor "My Positions" / portfolio view — but it's a static record of what an investor funded, not a servicing view.** That's exactly the line the 2026-06-03 call drew, and the code is most of the way there.

**What the call decided (2026-06-03, `docs/calls/2026-06-03 16-58-49`).** Robert/Judith explicitly cut the servicing-dependent sections from the investor portal — *"distribution and statements… we don't need that section at all. Returns… we don't need that at all… because my positions covers that and we don't do statements,"* and Brian confirms *"because we don't service."* What they kept ("My Positions") is, per Judith, a per-loan list of **property address, loan amount, interest rate, and payment** for *"each loan that they have with us."* So "positions" = the investor's holdings/history, which EFS knows because it *arranged* each loan — none of it requires servicing.

**The one nuance — "payment".** EFS doesn't collect or track payments, so the "payment" shown must be the **scheduled interest-only payment** (these are interest-only loans: ≈ loan amount × rate ÷ 12), a figure computed from origination terms — **not** an actual-received / payment-status feed. Shown that way it's fine and is what Judith asked for. Shown as live cash-flow it would overstate what EFS knows.

**Where the code still drifts from that.** P3 of the dashboard rework (`8d40c66`, #zwws) already dropped Avg-yield / YTD-distributions / Next-distribution from the dashboard *citing "EFS does not service loans."* But the underlying **positions API didn't get the same cleanup** — `lib/investor/portfolio/index.ts` (+ `lib/investor/shared.ts`) still compute and return:

- `yield_mix` / `annual_yield` — the call said *no yield tracking*;
- `next_payment_date` / `payment_schedule` (and `loan_payments`) — these read as a live servicing schedule EFS can't authoritatively maintain.

So the *concept* is sound and worth keeping; the *API still leaks servicing-flavored fields* the call already retired on the front end.

**Recommendation.** Keep "My Positions." Finish the no-servicing alignment in the API: drop `yield_mix`/`annual_yield` and `next_payment_date`/`payment_schedule` from the `investor_positions` response, keep `addr` / `amount` / `rate` / `position_type`, and relabel the per-loan `payment` as "scheduled monthly interest (est.)". Queued as **#58j3 / #posnosrv** (in backlog) rather than ripping out API surface inside an analysis turn — it's a small relabel-vs-remove call I'd rather you green-light first. Say the word and I'll do it as a `#spec-test-commit-push`.

(Note: "positions" also surfaces as **lien positions** — "first, second, and other junior lien positions" — but that's loan-structure copy, unrelated to payments/servicing, and is fine as-is.)

### Re: deploy — opus-4-8

> **Queue prompt:**
>
> :round_pushpin: deploy

Deployed to production. Live at `https://equityfundingsource.brian-takita.workers.dev` — **Version `9d8f4e6c-1dfc-4546-b280-6545dc4adc41`**.

**One snag, fixed before deploy.** `build:worker` first failed `pack:worker` with *"no complete CSS bundle found"* — the `dist/browser--dev` directory held a stale 21.7K CSS that had the EFS component layer but was missing the Tailwind utilities (`/*! tailwindcss`, `md:grid-cols-3`), plus a leftover 0-byte sibling. This is the racy-dist case the `pack-assets.ts` validation (commit `4969aef`) is meant to catch. Moved the stale `browser--dev`/`worker-assets` aside and clean-rebuilt — the bundle then validated clean (Tailwind layer present). No source change was needed; it was purely stale build state.

**Verification before/after deploy:**
- `bun run test` → **699 pass / 0 fail** (4460 assertions, 74 files).
- `build` + `build:worker` clean after the rebuild.
- Remote D1 migrations: **none to apply** (0025/0026 already live).
- Smoke test: `/` → 200, `/funded-deals` → 200, deployed CSS bundle `index.server-…6ZMAQWOB.css` → 200, 47KB, contains the Tailwind layer (so the homepage is fully styled, not the partially-styled failure mode).

**Push state:** the EFS submodule was 2 commits ahead of origin (the `#cgw7` broker-notification work: `2f3337b` + `0ebaff0`). Pushed `origin/main 60a07f5..0ebaff0`. Superproject pointer already in sync at `0ebaff0`.

Next queue head is `[#58j3]` (align the investor positions API with the no-servicing model).
<!-- agent:boundary:cc1c2b43 -->
<!-- /agent:exchange -->
###
<!--
Is there an admin for the borrower portal messages? Can we have an email notification whenever a borrow sends a message? Can we support an email response back into the chat? Can the borrower receive emails whenever an admin responds with a message?
-->

<!-- agent:queue preset="#spec-test-commit-push" priority go -->
- [#58j3]
- :round_pushpin: deploy
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#58j3] #posnosrv [recommended] Align investor "My Positions"/portfolio API with the no-servicing model: drop yield_mix/annual_yield and next_payment_date/payment_schedule from the investor_positions response (lib/investor/portfolio/index.ts + shared.ts still compute them even though the 2026-06-03 call + #zwws P3 8d40c66 already removed them from the dashboard); keep addr/amount/rate/position_type and relabel the per-loan "payment" as the scheduled interest-only payment (est., loan amount x rate / 12) so the portal never implies EFS tracks received payments.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#xcvg] [recommended] Borrower message email round-trip: email notification when a borrower sends a secure-portal message, inbound email reply that appends back into the D1 chat, and a borrower-facing email when an admin replies. Builds on the shipped chat_sessions/chat_messages + admin owner. Gated on a verified Resend EFS sending domain (f4rq) and Google Workspace SMTP (m6ch/p40t). (Distinct from the operator/comms emails to Judith + Robert.)
- [/] [#jnzs] [recommended] Await Judith or EFS reviewer approval on funded-deal images, copy, and lien-position wording before invoicing or launch sign-off. The funded-deals page carries no standalone legal/disclosure text; the disclosure needing legal review is the ESIGN/UETA Electronic record and signature disclosure on the borrower full-application/document-uploads flow (full_application.tsx). Funded-deals copy was reframed as real closed deals and shipped.
- [/] [#67v2] Admin loan pipeline: TMO-aligned stages + Kanban (drag cards, auto-advance when stage requirements met) synced to dashboard; mirror TMO stages as single source of truth. From 2026-06-03 call. Relates to #dzmm/#k2xg.
- [/] [#zwws] [recommended] Investor dashboard rework — remaining phases are externally gated: 4 vestings/special-instructions (TMO writeback-gated #dzmm/#k2xg/#7a4d); 6 new-deal email + workflow-priority dashboard (#f4rq). Shipped: P1 nav (30037e3), P2 dashboard-composite landing (3e06907), P2b interest-button + distributions/statements cleanup (efs cd4253a), P3 simplified portfolio (8d40c66), P5 accreditation→annual Requirements (efs 1957f64). Plan: src/equityfundingsource-dev/prds/rpzm-investor-dashboard-rework.md. Queue one phase at a time.
- [/] [#next-steps] Remaining EFS workers.dev production state: D1 API token unblocked; migrations 0025/0026 are already applied; Cloudflare Access env vars are deployed for /login/access-callback on workers.dev using the one-time PIN IdP. Custom domain cutover remains intentionally deferred; true Google Workspace SSO remains gated on Google admin credentials.
- [/] [#37ah] [recommended] Broker portal backend — D1-backed submit-deal, pipeline, commissions, documents, and notifications shipped (#37ah-backend, 2026-06-13). Remaining: TMO writeback sync, gated on #dzmm (TMO write field map #7a4d / six-question support email #k2xg).
- [/] [#dzmm] TMO webhooks UNAVAILABLE per operator (2026-06-13). Read-side tightened polling is shipped (scheduled tmo-sync with configurable interval + due-gating + tmo_cache read mirror). Remaining gated: EFS to TMO writeback (tmo_cache_outbox replay) needs the #7a4d write field map + #k2xg support answers before it can replay.
- [/] [#dropbox-sync-adapter] Dropbox sync adapter: after Dropbox access is available, push/pull R2_DOCUMENTS with idempotent retries, backfill, tombstone handling, and per-document sync logs.
<!-- /agent:review -->

## Completed / Reaped

<!-- agent:icebox -->
- [ ] [#xt4d] Inventory TMO usage once access is restored: screenshots, exports, required fields, statuses, document flows, notifications, reports, integrations, and workflows EFS actually uses.
- [ ] [#0e1p] Draft phased TMO replacement roadmap after the demo: EFS-first MVP, TMO sync/cutover plan, document
- [ ] [#f4rq] Verify an EFS-owned sending domain in Resend, then switch APPLICATION_LEAD_NOTIFICATION_FROM and APPLICATION_LEAD_NOTIFICATION_TO from testing-mode values to EFS production addresses.
- [ ] [#k2xg] Send Robert the six-question TMO outbound webhook prompt in src/equityfundingsource-dev/docs/tmo-webhooks-question.md for him to forward to Mortgage Office, then record the response and decide push vs. tightened polling.
- [ ] [#m6ch] Get Google Suite admin credentials from Robert (SMTP + Google Calendar integration)
- [ ] [#p40t] [recommended] Google Calendar/G Suite notification integration after Google admin credentials are available
- [ ] [#q2x2] HTML email templates: lighter navy + gold, 2-3 drip for PBM orphan investor outreach
- [ ] [#x5rn] Convert EFS logo JPEG → transparent PNG/SVG (Robert sent JPEG)
- [ ] [#kzt6] Collect testimonials for carousel (6–12 min: investors, brokers, borrowers) — push Judith + Robert helping
- [ ] [#kxsx] Form submission pipeline
- [ ] [#4vb5] Online presence cleanup (after Judith approves)
- [ ] [#a3gc] Consolidate domains under equityfundingsource.com
- [ ] [#zhyg] Take down/redirect Weebly and WordPress sites
- [ ] [#vd6p] Update directory listings (Yelp, Manta, hard money directories)
- [ ] [#tfee] Consolidate Facebook pages
- [ ] [#1w4n] Optimize LinkedIn profile
- [ ] [#7fx4] OCR ingestion schema/state machine: add provider-neutral document_processing tables for documents, versions, OCR runs, pages/chunks, extracted fields, entity mentions/edges, review decisions, provider cost, and retry/error state.
- [ ] [#d1h2] OCR provider adapter: process private R2 documents through pluggable parsers starting with Ragie Parse or Workers AI/Claude where appropriate; keep original R2 object as source of truth and persist source page/bounding-box evidence.
- [ ] [#f5kq] OCR admin review workflow: show extracted fields with source evidence, confidence, and diffable approval/rejection before any TMO borrower, loan, lender, or loan-application writeback.
- [ ] [#ntcr] Search/index spike: keep D1/R2 canonical and evaluate Ragie index plus Cloudflare Vectorize/SQL edge tables before adopting Neo4j, FalkorDB, Convex, or another separate graph database.
- [ ] [#7a4d] [recommended] OCR/TMO field extraction: custom/fillable docs -> reviewed extracted fields -> TMO update/create flow
- [ ] [#k9zj] Graph/vector database for Judith's document search dashboard (cross-doc search, any doc type)
- [ ] [#x0px] Tier 1: Workers AI pre-screening
- [ ] [#xgqg] Tier 2: Claude API document review
- [ ] [#kf1p] Tier 3: Ragie Parse + Index for approved docs
- [ ] [#4ktv] Discuss Ragie content validation with Bob Remeika (scheduled call)
- [ ] [#dvmr] [recommended] Corky: add long mono-call mixed-speaker recovery (separate diarization confidence, tighter bad-span detection, forced re-diarize on flagged windows)
- [ ] [#e1kb] Point equityfundingsource.com and www.equityfundingsource.com at the Cloudflare Pages project equityfundingsource, then verify / and /funded-deals on the custom domain; if admin routes are expected on the custom domain too, decide whether to route those paths to the Worker or move the dynamic app fully behind the same hostname.
- [ ] [#z3kx] GoHighLevel CRM as messaging/notifications path of record (Robert's enterprise GHL); integrate with the site so notifications also surface in-app; avoid SendGrid/extra recurring fees. From 2026-06-03 call. Relates to #xcvg.
<!-- /agent:icebox -->

<!-- agent:done archive=tasks/professional/equityfundingsource.done.md -->

<!-- completed work archived in tasks/professional/equityfundingsource.done.md -->
<!-- /agent:done -->
