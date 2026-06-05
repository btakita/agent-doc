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
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md. DO NOT IMPLEMENT THE FIX.
  '#commit-push': commit all uncommitted files + push in claudescore-plugin, claudescore.com, and session-share
  '#commit-push-deploy': commit all uncommitted files + push in claudescore-plugin, claudescore.com, and session-share. deploy
  '#mermaid': Include explanatory mermaid diagram(s).
  '#spec-test-commit-push': update spec + tests. commit + push
  '#spec-test-commit-push-deploy': update spec + tests. commit + push + deploy
queue_active: true
---

## Status

<!-- agent:status patch=replace -->

EFS workers.dev production: `/apply` public prequalification + D1-backed `application_leads` admin review is deployed in Worker version `734df89a-8af6-401e-83e4-fd5215a48e3f` as of 2026-05-28 from EFS commit `0afd62b`; public homepage rate-estimator dollar prefixes use `-translate-y-1/4`, and amount inputs use `pl-6`/24px left padding loaded after the base input rule. Local `bun dev` on port 4200 mounts `/apply`; public site/footer email is `info@equityfundingsource.com`; logged-out navigation exposes Borrower Login, Apply for a Loan, Broker Portal, and Investor Portal with Broker Portal placed between Apply and Investor. Cloudflare Access protects /login/access-callback with ACCESS_TEAM_DOMAIN/AUD. `/portal-admin/users` lets existing admins create admin/borrower/broker/investor accounts without reopening public `/admin/signup`; production D1 has active admin rows for `brian.takita+admin@gmail.com` and `btak.dev+admin@gmail.com`. Application lead notifications still route through Resend testing-mode config `APPLICATION_LEAD_NOTIFICATION_TO=brian.takita@gmail.com` and `APPLICATION_LEAD_NOTIFICATION_FROM=Equity Funding Source <onboarding@resend.dev>` until an EFS-owned sending domain is verified. Custom-domain cutover is deferred; continue using https://equityfundingsource.brian-takita.workers.dev until further notice.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260605-064822.md`*

Compacted content:
- Archived 1 response topic(s): [#zwws] Investor dashboard #rpzm phase 2b
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f...

### Re: [#zwws] Investor dashboard #rpzm phase 5 — opus-4-8

> **Queue prompt:**
>
> [#zwws]


Shipped phase 5 — the next un-gated phase. Spec + tests updated, committed + pushed (EFS `1957f64`, superproject pointer `cb2a12d28`).

#### Accreditation reframed as **Requirements**
- The nav item and page now read **Requirements**. The page leads with the **annual investor questionnaire** (status card shows an explicit "Annual" cadence and "Renews" date), and the requirements checklist notes the **accredited-investor certification is collected as part of your lender onboarding documents**.
- Kept the section id, `/portal-investor/accreditation` route, and `/api/investor/accreditation` API stable — this is a presentation/workflow reframe, not a plumbing rename, so no links or callers break.

#### Annual cadence enforced server-side
- Added `ACCREDITATION_ANNUAL_TTL_SECONDS` (365 days) and made the admin accreditation-decision route default a verified decision's TTL to it when the reviewer doesn't set one. So `expires_at` and the renewal window (expires − 60 days) now open on a yearly cadence by default instead of never expiring.

#### Verification
- `bun test`: **285 pass / 0 fail** (added annual-cadence + Requirements-framing coverage in the e2e and admin suites).
- `bun run build` + `bun run build:worker`: clean. (Pre-existing project-wide `fetch().then(data: T)` and the unrelated `blog_post_status_T` tsc notes are unchanged — the project gates on `bun run check`, not strict `tsc`.)
- PRD `rpzm-...` updated: phase 5 marked shipped.

**Remaining `#zwws` phases are externally gated:** 4 (vestings/special-instructions — TMO-writeback-gated `#dzmm`/`#k2xg`/`#7a4d`) and 6 (new-deal email + workflow-priority dashboard — `#f4rq`-gated). No un-gated phase remains, so I'm not continuing the loop — queue phase 4 or 6 once its gate clears.

### Re: Can we ungate phases 4 & 6 with the TMO cache? — opus-4-8

Short answer: the TMO **cache** by itself doesn't ungate either phase — but the gates are narrower than "all-or-nothing," and a meaningful chunk of both phases is shippable now without waiting on the external conditions.

#### Why the cache alone doesn't do it
The system runs an explicit `tmo_wins_for_reads_outbox_for_writes` strategy:
- **TMO cache = the read side.** `tmo_cache` (via `tmo-cache.ts`) is a read-through mirror of TMO data in D1. Reading it never writes anything back into TMO.
- **`tmo_cache_outbox` = the write side.** Local mutations are written to EFS D1 optimistically, then an outbox row is enqueued for deferred replay into TMO. Today it's used in exactly one place — `add_loan_funding` (the investor fund-commit in `deal_marketplace`).

So phase 4's gate isn't "we lack a cache" — it's "we haven't confirmed TMO's vesting **write** endpoint + field map" (`#7a4d`), plus the sync-direction decision (webhooks vs polling, `#dzmm`/`#k2xg`).

#### Phase 4 — vestings / special instructions: partially un-gate-able
- **Ship now (EFS-local half):** capture and display vestings + special instructions in EFS D1 and the investor/EFS UI — including multiple vestings per lender and multiple lenders per loan — using the same optimistic-write + `tmo_cache_outbox` pattern that `add_loan_funding` already uses. No TMO confirmation needed for the EFS-owned experience.
- **Still gated:** the actual **push into TMO**. The outbox gives us the transport, but a vesting outbox row needs a real TMO operation + field mapping to replay against (`#7a4d`), and we should settle push-vs-poll first (`#dzmm`/`#k2xg`). Until then the writeback leg stays parked.

#### Phase 6 — new-deal email + workflow-priority dashboard: un-gate-able, but not via TMO
This phase has nothing to do with TMO — its gate is `#f4rq` (the EFS Resend sending domain).
- **Ship now:** the **workflow-priority dashboard** half (urgent action items — wire funds, missing docs — bubbling to the top) has no email dependency at all.
- **Ship now in testing mode:** the new-deal email can be built exactly like P2b's "I'm interested" — wired through Resend, delivering to the account-allowed recipient until `#f4rq` verifies the production domain, at which point real-investor delivery turns on with no code change.

#### Recommendation
Decompose `#zwws` into "local-first" sub-work that ships now (phase 4 EFS-local capture/display; phase 6 dashboard + testing-mode email) and keep only the true TMO-push and production-email legs gated. I've captured this as `#zwws-local-first` `[recommended]`. Want me to queue the phase-4 EFS-local half next (it's the larger, higher-value piece)?

### Re: [#zwws-local-first] Phase 4 EFS-local — vestings + special instructions — opus-4-8

Shipped the phase-4 **EFS-local half** (the larger un-gated piece). Spec + tests updated, committed + pushed (EFS `eb549a8`, superproject `f86c1bee8`).

#### What shipped
- **New "Vestings" portal section** where investors manage **vestings** (legal holding entities — individual / joint / trust / LLC / entity / IRA / other; multiple per investor) and **special instructions / preferences**. EFS is the source of truth, no TMO dependency.
- **D1 schema** (migration `0041`): `investor_vestings` + `investor_special_instructions`.
- **API**: `GET/POST/PATCH/DELETE /api/investor/vestings` and `PUT /api/investor/special-instructions` — investor-auth-guarded, validated, investor-scoped (soft-archive on remove).
- **UI**: nav + page with add/edit/remove and an instructions editor (`vestings.tsx`).

#### Verification
- `bun test`: **292 pass / 0 fail** (new `investor-vestings.test.ts` covers repo/migration/routes/UI; QA fixtures + runbook + smoke script add the vestings step).
- `bun run build` + `build:worker`: clean; new files `tsc`-clean; migration `0041` SQL + upsert validated in sqlite.
- **Migration note:** `0041` is committed but **not yet applied to remote D1** — preset was commit+push, not deploy. It applies on the next `deploy:worker` / `cf:d1:migrate:remote` (same deferred-deploy posture as `#zr5z`).

#### Boundaries (deliberately deferred)
- **TMO push** (writing vestings into TMO) + **multiple-lenders-per-loan** per-lender portion/steps/status stay gated on the TMO writeback field map (`#7a4d`) + sync direction (`#dzmm`/`#k2xg`) — wire via the `tmo_cache_outbox` pattern once confirmed.
- **Phase 6's un-gated half** (workflow-priority dashboard) is captured as `#zwws-phase6-dashboard` `[recommended]` — shippable next without any external gate.

`#zwws-local-first` is marked done (its phase-4-local deliverable). Queue `#zwws-phase6-dashboard` when you want the workflow-priority dashboard next.

### Re: [#67v2] TMO board → admin pipeline workflow — opus-4-8

I can't retrieve the TMO board images through corky, so I'm **not** designing the workflow yet — building the pipeline logic off a guessed board would be wrong. Here's exactly what I hit and what I need.

#### What I tried
- `corky sync refetch "<the Gmail URL>"` → **`Gmail URL target FMfcgzQgMLxHCrjZbfGFBpkwSTTgxGbd not found in any Gmail API account`**. The `#inbox/FMfcg…` id in a Gmail web URL is the **web-client** thread id, not the Gmail **API** id, so refetch can't resolve it.
- `corky sync account brian.takita` (full inbox) → didn't surface the thread, and more importantly:
- corky's thread→Markdown sync renders images as **remote `![](https://…)` links** and does **not** download attachment binaries to disk. User-attached screenshots (which the TMO board is) come through as MIME attachment parts that the sync drops — so even once the thread syncs, there's no viewable image file for me to read.

Net: with the current corky tooling I have no way to actually *see* the board, and I won't fabricate the TMO stages/columns/requirements.

#### What unblocks it (any one)
1. **Save the board images to a path** I can read (e.g. `tasks/professional/assets/tmo-board-*.png`) — simplest + most reliable.
2. Give me the **Gmail API thread id** (or the email **subject + sender/date**) so `corky sync refetch <raw-id>` can pull it.
3. Say the word and I'll use **chromium-bridge** to open the Gmail URL in your authenticated browser and capture the images (heads-up: that surfaces your inbox on-screen — livestream caution).

#### Ready to go the moment I can see it
`#67v2` is a known feature: admin loan pipeline with **TMO-aligned stages + Kanban** (drag cards, auto-advance when stage requirements are met) synced to the dashboard, mirroring TMO stages as the single source of truth. There's prior art to build on — the investor pipeline already defines `Reviewing → Committed → Documents → Funding → Active` and admin loan status exists — but the board screenshots tell me the **real** TMO column names, ordering, and per-stage requirements, which I need to model it correctly.

I've **gated `#67v2`** as blocked-on-input. Drop the board images (or the resolvable thread id) and re-queue it, and I'll produce the workflow design + first implementation slice.
<!-- agent:boundary:efe6a2b4 -->
<!-- /agent:exchange -->
###
<!--
Is there an admin for the borrower portal messages? Can we have an email notification whenever a borrow sends a message? Can we support an email respons back into the chat? Can the borrower receive emails whenever an admin responds with a message?
-->

<!-- agent:queue preset="#spec-test-commit-push" start -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#zwws-phase6-dashboard] [recommended] #zwws phase 6 un-gated half: workflow-priority dashboard — urgent action items (wire funds, missing docs/info) bubble to the top with in-app indicators; available deals lead when nothing is urgent. The new-deal email is the gated half (build in Resend testing mode like P2b; production delivery gated on #f4rq). Plan: src/equityfundingsource-dev/prds/rpzm-investor-dashboard-rework.md.
- [ ] [#vtx8] [bug] Application leads vanish when advanced past intake (e.g. 'Contacted') — finish the admin pipeline so leads aren't dropped from view. From 2026-06-03 call.
- [ ] [#ty2v] Secure wiring-instructions delivery: expiring/self-destructing link or click-to-reveal (bank-style), plus 'verify wiring with the title company directly' reminder; flow title/escrow -> EFS -> lender. From 2026-06-03 call.
- [ ] [#z3kx] GoHighLevel CRM as messaging/notifications path of record (Robert's enterprise GHL); integrate with the site so notifications also surface in-app; avoid SendGrid/extra recurring fees. From 2026-06-03 call. Relates to #xcvg.
- [ ] [#xt4d] Inventory TMO usage once access is restored: screenshots, exports, required fields, statuses, document flows, notifications, reports, integrations, and workflows EFS actually uses.
- [ ] [#0e1p] Draft phased TMO replacement roadmap after the demo: EFS-first MVP, TMO sync/cutover plan, document intelligence/search, borrower/broker/investor/admin portals, and tenant-ready resale packaging.
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
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#67v2] Admin loan pipeline: TMO-aligned stages + Kanban (drag cards, auto-advance when stage requirements met) synced to dashboard; mirror TMO stages as single source of truth. From 2026-06-03 call. Relates to #dzmm/#k2xg.
- [/] [#zwws] [recommended] Investor dashboard rework — remaining phases are externally gated: 4 vestings/special-instructions (TMO writeback-gated #dzmm/#k2xg/#7a4d); 6 new-deal email + workflow-priority dashboard (#f4rq). Shipped: P1 nav (30037e3), P2 dashboard-composite landing (3e06907), P2b interest-button + distributions/statements cleanup (efs cd4253a), P3 simplified portfolio (8d40c66), P5 accreditation→annual Requirements (efs 1957f64). Plan: src/equityfundingsource-dev/prds/rpzm-investor-dashboard-rework.md. Queue one phase at a time.
- [/] [#zr5z] Re-deploy equityfundingsource full apply4 wizard to production. P1–P6 implemented, 24 apply4 tests green, build-plan spec updated, all committed+pushed (efs 071d8fe). Remaining: cf:d1:migrate:remote + deploy:worker + smoke-test against prod — held pending explicit deploy authorization (re-queue with #spec-test-commit-push-deploy or say 'deploy').
- [/] [#cm3e] [recommended] Borrower online payment collection (Stripe) + TMO sync: add ACH/card payment in the borrower portal make-payment section; on Stripe payment_intent.succeeded webhook, verify signature, record the receipt in a new EFS D1 loan_payment_transactions table, reconcile the matching loan_payments schedule row, and push the payment to TMO. GATE: confirm ABS/TMO exposes a borrower-payment/receipt WRITE endpoint (current docs/TMO cover loan/application writes, investor AddLoanFunding, and schedule/funding reads only) — ties into the TMO-writeback gate (dzmm/k2xg). Deferred non-MVP collection path from e245; handle KYC, partial payments, NSF reversals, fees.
- [/] [#xcvg] [recommended] Borrower message email round-trip: add an admin owner for borrower-portal secure messages, email notification when a borrower sends a message, inbound email reply that appends into the D1 chat, and borrower email when an admin replies. Builds on existing chat_sessions/chat_messages. Gated on Resend EFS sending domain (f4rq) + Google Workspace SMTP (m6ch/p40t).
- [/] [#next-steps] Remaining EFS workers.dev production state: D1 API token unblocked; migrations 0025/0026 are already applied; Cloudflare Access env vars are deployed for /login/access-callback on workers.dev using the one-time PIN IdP. Custom domain cutover remains intentionally deferred; true Google Workspace SSO remains gated on Google admin credentials.
- [/] [#37ah] [recommended] Broker portal backend: replace static broker demo data with TMO-backed submit-deal, pipeline, commissions, documents, and notifications
- [/] [#dzmm] TMO webhooks: confirm outbound webhook support (Robert to check with Mortgage Office)
- [/] [#qakx] Investor portal document pages: replace static Documents, Statements, and Accreditation fixture data with D1/R2-backed APIs and authenticated upload/request/download flows.
- [/] [#1v5m] Resolved 2026-05-25: created an EFS-specific Cloudflare API token scoped to account 1e7e194ff8da4ad9e40ce041aea84f76 with D1, Workers Scripts, R2, observability, and read-only user/account metadata permissions; updated passage btak/equityfundingsource/cloudflare_api_token. D1 migration list/apply now succeeds and reports no pending migrations.
- [/] [#jnzs] [recommended] Await Judith or EFS reviewer approval on funded-deal images, copy, lien-position wording, and legal/disclosure language before invoicing or launch sign-off. Technical D1 migration, Worker deploy, and live funded-deals/admin-route smoke tests completed 2026-05-23.
- [/] [#dropbox-sync-adapter] Dropbox sync adapter: after Dropbox access is available, push/pull R2_DOCUMENTS with idempotent retries, backfill, tombstone handling, and per-document sync logs.
<!-- /agent:review -->

## Completed / Reaped

<!-- agent:icebox -->

- [ ] [#e1kb] Point equityfundingsource.com and www.equityfundingsource.com at the Cloudflare Pages project equityfundingsource, then verify / and /funded-deals on the custom domain; if admin routes are expected on the custom domain too, decide whether to route those paths to the Worker or move the dynamic app fully behind the same hostname.
<!-- /agent:icebox -->

<!-- agent:done archive=tasks/professional/equityfundingsource.done.md -->

<!-- completed work archived in tasks/professional/equityfundingsource.done.md -->
<!-- /agent:done -->
