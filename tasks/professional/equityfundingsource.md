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
  '#commit-push': commit all uncommitted files
  '#commit-push-deploy': commit all uncommitted files + deploy
  '#mermaid': Include explanatory mermaid diagram(s).
  '#spec-test-commit-push': update spec + tests. commit + push
  '#spec-test-commit-push-deploy': update spec + tests. commit + push + deploy
queue: start
---

## Status

<!-- agent:status patch=replace -->

EFS deployed Worker 0336450c-126f-49da-9321-1d4c74a6d2c1 on 2026-06-11: full-application validation failures now add red field-level error boxes for missing/invalid fields and those boxes disappear as completed values pass live validation. make check passed (559 tests + build + worker build); remote D1 has no pending migrations; smoke checks returned / 200 and /portal-borrower/new-application 303 unauthenticated redirect. CI workflow check is unavailable because this repo has no workflow named CI. EFS submodule has the intended uncommitted source changes per local no-commit instruction.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260611-174449.md`*

Compacted content:
- Archived 2 response topic(s): validation toasts and deploy; field-level validation boxes and deploy
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f...

### Re: Admin Needs Attention default-open + deploy — gpt-5

> **Queue prompt:**
>
> The admin portal "Needs attention" section should default to open. deploy


Implemented and deployed.

Changed paths:
- `src/equityfundingsource-dev/lib/ui--browser--efs/portal/admin/dashboard.tsx` — the admin `Needs attention` panel now initializes open with `useState(true)`.
- `src/equityfundingsource-dev/tests/admin-dashboard.test.ts` — the existing admin dashboard coverage now asserts the default-open state.

Verification and deploy:
- `bun test tests/admin-dashboard.test.ts` — 25 passed.
- `make check` — 559 passed, plus browser build and Worker build.
- `bun run --cwd app/equityfundingsource cf:d1:migrations:list:remote` — no pending migrations before and after deploy.
- `make deploy` — deployed Worker version `d13af486-6d31-4f1b-a1ed-8caf25f8628c`.
- Live smoke checks: public `/` returned `200`; `/portal-admin` returned `303`, consistent with Cloudflare Access redirect.
- `gh run list --workflow CI --limit 1` could not find a workflow named `CI`; `gh workflow list` returned no workflows for this repo.
- Live smoke checks: public `/` returned `200`; `/portal-admin` returned `303`, consistent with Cloudflare Access redirect.
- `make deploy` — deployed Worker version `5b15492f-ed7a-492d-ac77-17cb19c396a2`.
- `bun run --cwd app/equityfundingsource cf:d1:migrations:list:remote` — no pending migrations before and after deploy.
- `make check` — 559 passed, plus browser build and Worker build.
- `bun test tests/admin-dashboard.test.ts` — 25 passed.
- `src/equityfundingsource-dev/tests/admin-dashboard.test.ts` — coverage now asserts loan/lead attention detail emails and the UI source wiring.
- `src/equityfundingsource-dev/lib/ui--browser--efs/portal/admin/dashboard.tsx` — expanded `Needs attention` detail rows now render the email line with wrapping.
- `src/equityfundingsource-dev/app/equityfundingsource/lib/admin/dashboard/index.ts` — attention detail items now carry an optional `email`, populated from loan `borrower_email` or lead `email`.
- Changed paths: `src/equityfundingsource-dev/app/equityfundingsource/lib/admin/dashboard/index.ts`, `src/equityfundingsource-dev/lib/ui--browser--efs/portal/admin/dashboard.tsx`, and `src/equityfundingsource-dev/tests/admin-dashboard.test.ts`.
- Commands run: `bun test tests/admin-dashboard.test.ts`, `make check`, remote D1 migration list before and after deploy, `make deploy`, live smoke curls, `gh run list --workflow CI --limit 1`, and `gh workflow list`.
- Verification: focused dashboard test passed 25 tests; `make check` passed 559 tests plus browser and Worker builds; D1 had no pending migrations; `/` returned `200`; `/portal-admin` returned `303`.
- Deploy: Worker version `5b15492f-ed7a-492d-ac77-17cb19c396a2`.
- Commit: agent-doc document response commit `f82aad149`; no EFS repo code commit was made because the local repo instruction says not to commit unless explicitly asked.
- `gh run list --workflow CI --limit 1` could not find a workflow named `CI`; `gh workflow list` returned no workflows for this repo.
- Live smoke checks: public `/` returned `200`; `/portal-admin` returned `303`, consistent with Cloudflare Access redirect.
- `make deploy` — deployed Worker version `3106fb97-7023-4def-8722-bc1c18499a06`.
- `bun run --cwd app/equityfundingsource cf:d1:migrations:list:remote` — no pending migrations before and after deploy.
- `make check` — 559 passed, plus browser build and Worker build.
- `bun test tests/admin-pipeline.test.ts` — 37 passed.
❯ - `src/equityfundingsource-dev/tests/admin-pipeline.test.ts` — coverage now asserts the new email payload and UI wiring.
❯ - `src/equityfundingsource-dev/lib/ui--browser--efs/portal/admin/pipeline.tsx` — loan and lead pipeline cards now show the email line, and pipeline card search matches email.
- `src/equityfundingsource-dev/app/equityfundingsource/lib/admin/dashboard/index.ts` — dashboard attention detail card builders stay compatible with the email-carrying payloads.
- `src/equityfundingsource-dev/app/equityfundingsource/lib/admin/loan_management/index.ts` — the admin pipeline endpoint passes borrower/lead email into card builders.
❯ - `src/equityfundingsource-dev/app/equityfundingsource/lib/admin/repo.ts` — pipeline card payload types now carry `borrower_email` for loan cards and `email` for lead cards.

Confidence: high. Escalation: none.

Suggested git commit message:

```txt
fix(admin): open needs attention by default

Admin dashboard now expands the Needs attention panel on first render and the dashboard test covers that default-open state. Verified with focused admin tests, make check, and production deploy d13af486-6d31-4f1b-a1ed-8caf25f8628c.
```

### Re: Pipeline card email + deploy — gpt-5

> **Queue prompt:**
>
> In the pipeline cards, include the email address. deploy


Implemented and deployed.

Changed paths for this queue item:
❯ - `src/equityfundingsource-dev/app/equityfundingsource/lib/admin/repo.ts` — pipeline card payload types now carry `borrower_email` for loan cards and `email` for lead cards.
❯ - `src/equityfundingsource-dev/app/equityfundingsource/lib/admin/loan_management/index.ts` — the admin pipeline endpoint passes borrower/lead email into card builders.
❯ - `src/equityfundingsource-dev/app/equityfundingsource/lib/admin/dashboard/index.ts` — dashboard attention detail card builders stay compatible with the email-carrying payloads.
❯ - `src/equityfundingsource-dev/lib/ui--browser--efs/portal/admin/pipeline.tsx` — loan and lead pipeline cards now show the email line, and pipeline card search matches email.
❯ - `src/equityfundingsource-dev/tests/admin-pipeline.test.ts` — coverage now asserts the new email payload and UI wiring.

Verification and deploy:
- `bun test tests/admin-pipeline.test.ts` — 37 passed.
- `make check` — 559 passed, plus browser build and Worker build.
- `bun run --cwd app/equityfundingsource cf:d1:migrations:list:remote` — no pending migrations before and after deploy.
- `make deploy` — deployed Worker version `3106fb97-7023-4def-8722-bc1c18499a06`.
- Live smoke checks: public `/` returned `200`; `/portal-admin` returned `303`, consistent with Cloudflare Access redirect.
- `gh run list --workflow CI --limit 1` could not find a workflow named `CI`; `gh workflow list` returned no workflows for this repo.

No actionable follow-up/backlog item is needed; the requested change is complete and deployed.

Confidence: high. Escalation: none.

Suggested git commit message:

```txt
fix(admin): show emails on pipeline cards

Pipeline card payloads now carry borrower and lead email addresses, and the Deal Pipeline cards render that email line while card search also matches it. Verified with focused admin pipeline tests, make check, and production deploy 3106fb97-7023-4def-8722-bc1c18499a06.
```

### Re: Needs Attention item emails + deploy — gpt-5

> **Queue prompt:**
>
> The needs attention items should have the email address. deploy

Implemented and deployed.

Changed paths for this queue item:
- `src/equityfundingsource-dev/app/equityfundingsource/lib/admin/dashboard/index.ts` — attention detail items now carry an optional `email`, populated from loan `borrower_email` or lead `email`.
- `src/equityfundingsource-dev/lib/ui--browser--efs/portal/admin/dashboard.tsx` — expanded `Needs attention` detail rows now render the email line with wrapping.
- `src/equityfundingsource-dev/tests/admin-dashboard.test.ts` — coverage asserts loan/lead attention detail emails and the UI source wiring.

Verification and deploy:
- `bun test tests/admin-dashboard.test.ts` — 25 passed.
- `make check` — 559 passed, plus browser build and Worker build.
- `bun run --cwd app/equityfundingsource cf:d1:migrations:list:remote` — no pending migrations before and after deploy.
- `make deploy` — deployed Worker version `5b15492f-ed7a-492d-ac77-17cb19c396a2`.
- Live smoke checks: public `/` returned `200`; `/portal-admin` returned `303`, consistent with Cloudflare Access redirect.
- `gh run list --workflow CI --limit 1` could not find a workflow named `CI`; `gh workflow list` returned no workflows for this repo.

No repo code commit was made; the EFS submodule remains dirty with this change plus prior intended session changes.

Confidence: high. Escalation: none.

Suggested git commit message:

```txt
fix(admin): show emails in needs attention items

Admin dashboard Needs Attention detail items now carry borrower and lead email addresses through the backend payload and render them in the expanded item list with wrapping. Covered by admin dashboard tests, verified with make check, and deployed to Worker version 5b15492f-ed7a-492d-ac77-17cb19c396a2.
```
<!-- agent:boundary:423f4a61 -->
<!-- /agent:exchange -->
###
<!--
Is there an admin for the borrower portal messages? Can we have an email notification whenever a borrow sends a message? Can we support an email respons back into the chat? Can the borrower receive emails whenever an admin responds with a message?
-->

<!-- agent:queue preset="#spec-test-commit-push" go -->
- I'm making a new loan application https://equityfundingsource.brian-takita.workers.dev/portal-borrower/new-application but it does not show up in the admin portal Deal Pipeline. Should this be an application lead?
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#efspgo] [recommended] EFS online-payment go-live config (after Robert provides Stripe account): set Cloudflare secrets STRIPE_SECRET_KEY + STRIPE_WEBHOOK_SECRET + var STRIPE_PAYMENTS_ENABLED=true (wrangler secret put / wrangler.toml), point a Stripe webhook at /api/stripe/webhook, then in admin Platform Settings fill the borrower payment wire/ACH + lender ACH values and flip the per-method enable toggles. Deploy with bun run deploy:worker. Capability is BUILT + admin-configurable (EFS c790ed3); this is config + deploy only.
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
intelligence/search, borrower/broker/investor/admin portals, and tenant-ready resale packaging.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#vd8s] [recommended] Investor Pipeline phase 2 — remaining after stalled-stage highlighting shipped (EFS 055a73c): drag-to-confirm-wire from the Committed lane (needs multi-pending-fund disambiguation) and TMO AddLoanFunding sync state in the Active lane once the writeback gate (#dzmm/#k2xg) clears. Per-investor activity timeline folds into the #vtx8 detail modal. Plan: src/equityfundingsource-dev/prds/investor-pipeline-admin.md
- [/] [#67v2] Admin loan pipeline: TMO-aligned stages + Kanban (drag cards, auto-advance when stage requirements met) synced to dashboard; mirror TMO stages as single source of truth. From 2026-06-03 call. Relates to #dzmm/#k2xg.
- [/] [#zwws] [recommended] Investor dashboard rework — remaining phases are externally gated: 4 vestings/special-instructions (TMO writeback-gated #dzmm/#k2xg/#7a4d); 6 new-deal email + workflow-priority dashboard (#f4rq). Shipped: P1 nav (30037e3), P2 dashboard-composite landing (3e06907), P2b interest-button + distributions/statements cleanup (efs cd4253a), P3 simplified portfolio (8d40c66), P5 accreditation→annual Requirements (efs 1957f64). Plan: src/equityfundingsource-dev/prds/rpzm-investor-dashboard-rework.md. Queue one phase at a time.
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
- [ ] [#z3kx] GoHighLevel CRM as messaging/notifications path of record (Robert's enterprise GHL); integrate with the site so notifications also surface in-app; avoid SendGrid/extra recurring fees. From 2026-06-03 call. Relates to #xcvg.
<!-- /agent:icebox -->

<!-- agent:done archive=tasks/professional/equityfundingsource.done.md -->

<!-- completed work archived in tasks/professional/equityfundingsource.done.md -->
<!-- /agent:done -->
