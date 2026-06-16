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
  '#advance-review': Go through the review items. Add actionable backlog items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md. DO NOT IMPLEMENT THE FIX.
  '#commit-push': commit all uncommitted files
  '#commit-push-deploy': commit all uncommitted files + deploy
  '#mermaid': Include explanatory mermaid diagram(s).
  '#spec-test-commit-push': update spec + tests. commit + push
  '#spec-test-commit-push-deploy': update spec + tests. commit + push + deploy
queue: stop
---

## Status

<!-- agent:status patch=replace -->

committed
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260616-003823.md`*

Compacted content:
- Archived 1 response topic(s): #investor-pipeline-resequence and borrower portal messages
- Prior summary/context: prior compacted content: Archived 2 response topic(s): compact archive summary / no-op closeout; #gq5c follow-up next...

### Re: Remove redundant "Published deals" + deploy — gpt-5

> **Queue prompt:**
>
> Remove redundant "Published deals" in https://equityfundingsource.brian-takita.workers.dev/portal-admin/pipeline


Removed the redundant `Published deals` cross-cut board lane from `/portal-admin/pipeline`. The board now renders only the real pipeline phase columns, while the per-loan `Published` badge and publish/close deal actions remain intact.

Changed paths:
- `src/equityfundingsource-dev/lib/ui--browser--efs/portal/admin/pipeline.tsx`
- `src/equityfundingsource-dev/tests/admin-pipeline.test.ts`
- `src/equityfundingsource-dev` parent gitlink

Commits:
- EFS app: `5d19911 fix(admin): remove redundant published deals column`
- Parent gitlink: `a2f34ece4 chore(efs): update deployed app submodule`

Deploy:
- Rebasing onto `origin/main` brought in pending migration `0069_final_loan_docs_signature.sql`; inspected it before deploy. It only adds final-document signature/notary columns and an index to `loans`.
- `make deploy` applied that migration and deployed Worker version `6c5106e4-63f7-4b8e-8306-8322846d9212`.

Verification:
- `bun test tests/admin-pipeline.test.ts` passed: 53 tests.
- `make check` in `src/equityfundingsource-dev` passed after rebase: 736 tests, app build, Worker build.
- `bun run --cwd app/equityfundingsource cf:d1:migrations:list:remote` after deploy: no migrations to apply.
- Live smoke: `/` returned 200; `/portal-admin/pipeline` returned 303 to the admin login, as expected for the protected route.
- Downloaded the deployed browser asset `/index.browser-VRGDIGYI.js`; it contains no `Published deals` column string or removed lane identifiers.
- GitHub Actions check: this submodule has no `.github/workflows`, and `gh run list --workflow CI --limit 1` reports no workflow named `CI`.
- Top-level `make check` is not EFS-specific and failed in existing `src/agent-doc` tests: one tmux isolation failure (`duplicate session: test`) and one IPC long-path timeout. The EFS app suite and deploy proof are green.

Confidence: high for the EFS fix and deploy. Escalation: none.

### Re: deploy — gpt-5

> **Queue prompt:**
>
> deploy


Deploy is complete. No additional code changes were needed in this queue pass because the prior pass already deployed the EFS Worker after removing the redundant `Published deals` lane.

Deploy proof:
- `make deploy` completed successfully.
- Remote D1 migration `0069_final_loan_docs_signature.sql` was applied successfully.
- Worker version deployed: `6c5106e4-63f7-4b8e-8306-8322846d9212`.
- Production URL: `https://equityfundingsource.brian-takita.workers.dev`.

Verification:
- `bun run --cwd app/equityfundingsource cf:d1:migrations:list:remote`: no migrations to apply.
- Live `/`: HTTP 200.
- Live `/portal-admin/pipeline`: HTTP 303 to admin login, expected for the protected route.
- Deployed browser asset `/index.browser-VRGDIGYI.js`: no `Published deals` column string or removed lane identifiers.
- EFS app checks were green before deploy: focused admin pipeline test passed; full `make check` passed with 736 tests plus app and Worker builds.

Commits/push:
- EFS app `main` pushed at `5d19911`.
- Parent repo `main` pushed through session commit `3dc937ce1`.

Confidence: high for deploy completion. Escalation: none.

### Re: Compact Available Deals / Loan TMO Details — gpt-5

> **Queue prompt:**
>
> The https://equityfundingsource.brian-takita.workers.dev/portal-admin/pipeline Available Deals dialog & https://equityfundingsource.brian-takita.workers.dev/portal-admin/loans/45ddae53-3e7f-43b1-a627-17b79b06fa3f page should have the data more condensed. Not a full-width label + field layout.

Condensed the shared TMO loan data presentation used by both the Deal Pipeline Available Deals dialog and `/portal-admin/loans/:id`. The TMO loan summary, borrower, collateral, and encumbrance sections now render as a compact inline key/value grid instead of using portal-wide `.detail-row` rows.

Changed paths:
- `src/equityfundingsource-dev/lib/ui--browser--efs/portal/shared/tmo_loan_details.tsx`
- `src/equityfundingsource-dev/tests/tmo-loan-details-density.test.ts`
- `src/equityfundingsource-dev/prds/67v2-admin-loan-pipeline-kanban.md`
- `src/equityfundingsource-dev` parent gitlink

Commits/push:
- EFS app: `e70a770 fix(admin): compact TMO loan detail rows (#pipeline-dialog-density)` pushed to `origin/main`.
- Parent repo: `c7b62733c chore(efs): update compact TMO detail layout submodule` pushed to `origin/main`.

Verification:
- `bun test tests/tmo-loan-details-density.test.ts tests/boolean-checkbox-fields.test.ts` passed.
- `make check` in `src/equityfundingsource-dev` passed: 739 tests, app build, and Worker build. The digest runner flagged a `d1 down` signature, but the expanded log shows `739 pass / 0 fail`; this was a log-filter false positive.
- Top-level `make check` passed.
- `gh run list --workflow CI --limit 1` in both the EFS submodule and parent repo reported no workflow named `CI`.

No D1 migration was needed. Confidence: high. Escalation: none.

### Re: deploy — gpt-5

> **Queue prompt:**
>
> deploy

Deployed the current EFS app from clean submodule commit `e70a770 fix(admin): compact TMO loan detail rows (#pipeline-dialog-density)`.

Changed paths:
- None this cycle; deploy used the already-committed EFS app state.

Commands:
- `make deploy` in `src/equityfundingsource-dev`
- `bun run --cwd app/equityfundingsource cf:d1:migrations:list:remote`
- Live smoke checks with `curl` for `/` and `/portal-admin/pipeline`
- Downloaded deployed asset `/index.browser-EEVRGIOE.js` and checked it for the compact TMO detail implementation
- `gh run list --workflow CI --limit 1` in both the EFS submodule and parent repo

Deploy proof:
- Worker URL: `https://equityfundingsource.brian-takita.workers.dev`
- Worker version: `28ae8153-0e47-43e0-a102-8bc2656b7e16`
- Remote D1 migrations: no migrations to apply
- Deployed browser asset: `/index.browser-EEVRGIOE.js`

Verification:
- `/` returned HTTP 200.
- `/portal-admin/pipeline` returned HTTP 303 to `/login?role=admin&next=%2Fportal-admin%2Fpipeline`, expected for the protected admin route.
- The downloaded deployed browser bundle includes `TMO_DETAIL_GRID_STYLE` / `compact_pairs_`, confirming the compact TMO loan detail code is in the deployed asset.
- `src/equityfundingsource-dev` remained clean after deploy.
- GitHub Actions check: both the EFS submodule and parent repo report no workflow named `CI`.

I did not rerun the full app suite in this pass because there were no code changes since the prior green `make check`; `make deploy` rebuilt the app and Worker before upload. Preflight reported high exchange accretion, but the active queue was drainable, so I continued the deploy cycle.

Confidence: high. Escalation: none.
<!-- agent:boundary:66de993b -->
<!-- /agent:exchange -->
###
<!--
Is there an admin for the borrower portal messages? Can we have an email notification whenever a borrow sends a message? Can we support an email response back into the chat? Can the borrower receive emails whenever an admin responds with a message?
-->

<!-- agent:queue preset="#spec-test-commit-push" priority go -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#efs-cutover-sso-plan] Workers.dev to custom-domain/SSO closeout: write and execute a cutover checklist for equityfundingsource.com/www routing, /login/access-callback, rollback, and Google Workspace SSO after #m6ch credentials arrive. Advances #next-steps.
- [/] [#zwws-vestings-local] [recommended] Investor dashboard phase 4 local half: capture/display vestings and special instructions in EFS D1/UI with tmo_cache_outbox enqueue; leave TMO push gated on #dzmm/#7a4d. Advances #zwws.
- [/] [#67v2-auto-advance] Admin pipeline auto-advance: define stage requirement checks from the TMO-aligned Kanban, run them against D1/tmo_cache, and enqueue audited status updates; keep replay gated by #dzmm. Advances #67v2.
- [/] [#jnzs-approval-followup] [recommended] Send/track funded-deals approval packet: attach current screenshots/copy/lien-position language plus the full_application ESIGN/UETA disclosure excerpt, record Judith/EFS reviewer decision, and convert requested edits into implementation backlog. Advances #jnzs.
- [/] [#xcvg-prod-email] [recommended] Borrower secure-message email production pass: after #f4rq/#m6ch/#p40t or #z3kx path-of-record is decided, enable borrower/admin outbound notifications, verify inbound reply threading, and add production env/runbook checks. Advances #xcvg.
- [/] [#dzmm-writeback-replay] EFS->TMO writeback replay: once #7a4d field map and #k2xg support answers arrive, replay tmo_cache_outbox with idempotency keys, conflict logging, retry/dead-letter state, and tests across broker/admin/investor writebacks. Advances #dzmm/#37ah/#67v2/#zwws.
- [/] [#xcvg] [recommended] Borrower message email round-trip: email notification when a borrower sends a secure-portal message, inbound email reply that appends back into the D1 chat, and a borrower-facing email when an admin replies. Builds on the shipped chat_sessions/chat_messages + admin owner. Gated on a verified Resend EFS sending domain (f4rq) and Google Workspace SMTP (m6ch/p40t). (Distinct from the operator/comms emails to Judith + Robert.)
- [/] [#jnzs] [recommended] Await Judith or EFS reviewer approval on funded-deal images, copy, and lien-position wording before invoicing or launch sign-off. The funded-deals page carries no standalone legal/disclosure text; the disclosure needing legal review is the ESIGN/UETA Electronic record and signature disclosure on the borrower full-application/document-uploads flow (full_application.tsx). Funded-deals copy was reframed as real closed deals and shipped.
- [/] [#67v2] Admin loan pipeline: TMO-aligned stages + Kanban (drag cards, auto-advance when stage requirements met) synced to dashboard; mirror TMO stages as single source of truth. From 2026-06-03 call. Relates to #dzmm/#k2xg.
- [/] [#zwws] [recommended] Investor dashboard rework — remaining phases are externally gated: 4 vestings/special-instructions (TMO writeback-gated #dzmm/#k2xg/#7a4d); 6 new-deal email + workflow-priority dashboard (#f4rq). Shipped: P1 nav (30037e3), P2 dashboard-composite landing (3e06907), P2b interest-button + distributions/statements cleanup (efs cd4253a), P3 simplified portfolio (8d40c66), P5 accreditation→annual Requirements (efs 1957f64). Plan: src/equityfundingsource-dev/prds/rpzm-investor-dashboard-rework.md. Queue one phase at a time.
- [/] [#next-steps] Remaining EFS workers.dev production state: D1 API token unblocked; migrations 0025/0026 are already applied; Cloudflare Access env vars are deployed for /login/access-callback on workers.dev using the one-time PIN IdP. Custom domain cutover remains intentionally deferred; true Google Workspace SSO remains gated on Google admin credentials.
- [/] [#dzmm] TMO webhooks UNAVAILABLE per operator (2026-06-13). Read-side tightened polling is shipped (scheduled tmo-sync with configurable interval + due-gating + tmo_cache read mirror). Remaining gated: EFS to TMO writeback (tmo_cache_outbox replay) needs the #7a4d write field map + #k2xg support answers before it can replay.
- [/] [#dropbox-sync-adapter] Dropbox sync adapter: #dropbox-sync-contract shipped the shared documents Dropbox metadata, document_sync_logs idempotency schema, pure sync planner, and mocked adapter tests. Remaining: after Dropbox access is available, wire the real credential-backed worker to push/pull R2_DOCUMENTS with retries, backfill, tombstone replay, and per-document sync logs.
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
