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

Answered: `Requested loan number` is the optional subject-property `loan_number`, distinct from canonical TMO loan number; the live application row has it stored as an empty string.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260612-225138.md`*

Compacted content:
- Archived 4 response topic(s): Stripe account email; Stripe account email sent; Remove payment processors; 1 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f...

### Re: Requested loan number — gpt-5

> **Queue prompt:**
>
> What is "Requested loan number" in https://equityfundingsource.brian-takita.workers.dev/portal-admin/loan-applications/a8cf595e-06d0-4f73-b41d-f68e1422e8e3? Why is it blank? 


`Requested loan number` is not the canonical TMO loan number. It is the subject-property form field stored as `application_subject_property.loan_number`, labeled in the admin/detail UI as `Requested loan number`. Migration `0050_full_application_tmo_loan_number.sql` explicitly separates it from `full_applications.tmo_loan_number`, which is the real/canonical TMO loan number after admin/TMO writeback.

For the specific production application `a8cf595e-06d0-4f73-b41d-f68e1422e8e3`, remote D1 currently has:

- `full_applications.status = 'draft'`
- `full_applications.tmo_application_number = null`
- `full_applications.tmo_loan_number = null`
- `full_applications.writeback_status = 'not_started'`
- `application_subject_property.loan_number = ''`
- subject-property data otherwise exists, including `loan_amount = 100000`, `loan_purpose = 'purchase'`, and the Thompson, CT property address
- no `application_subject_property_change_logs` rows for this application

So it is blank because nobody has supplied that optional requested loan number on this application, and the save path persists an omitted `loan_number` / `requested_loan_number` as an empty string. This is not a missing join or rendering bug. If EFS expected that row to show the TMO loan identifier, that is the separate `tmo_loan_number` field, and this draft application has not been submitted or written back to TMO yet.

Related production note: the remote D1 migration list currently stops at `0061_public_content_home.sql` / `0061_application_subject_property_admin_history.sql`; `0062_full_application_admin_change_history.sql` is not applied yet, so the newer all-form `application_change_logs` table is not present in production. That does not cause this blank `Requested loan number`, but it does mean the prior all-form history change is not fully reflected in remote D1 yet.

### Re: Loan application admin editing — gpt-5

> **Queue prompt:**
>
> The admin should be able to edit loan applications such as https://equityfundingsource.brian-takita.workers.dev/portal-admin/loan-applications/a8cf595e-06d0-4f73-b41d-f68e1422e8e3. Ensure we have the change history saved in the database. Make the change history viewable for all forms.


Implemented and shipped the admin loan-application editor/history work.

Changed paths: `src/equityfundingsource-dev` at `a0d5dde` adds `PUT /api/admin/full-applications/:id/sections/:section`, migration `0062_full_application_admin_change_history.sql`, per-section admin editors for broker source, personal, addresses, employment, income, assets, liabilities, REO, subject property, and declarations, plus D1-backed `application_change_logs` history displayed in each form card. Subject-property `PATCH` remains available and now refreshes the new all-section history too. The parent repo records that submodule pointer in `1a93213b2`.

Commands: `bun test tests/admin-subject-property-edit.test.ts`; `make check`; `gh run list --workflow CI --limit 1`; `gh workflow list`; `git commit`/`git push` in `src/equityfundingsource-dev`; parent `git commit`/`git push` for the submodule pointer.

Verification: targeted admin tests passed; full `make check` passed with 565 tests plus browser and worker builds. GitHub Actions check could not find a workflow named `CI`, and `gh workflow list` returned no workflows for this submodule, so local verification is the available proof.

Escalation: no escalated shell permissions were needed. Existing unrelated uncommitted changes in the EFS submodule and parent repo were left untouched. Confidence: high.
<!-- agent:boundary:6152bcc4 -->
<!-- /agent:exchange -->
###
<!--
Is there an admin for the borrower portal messages? Can we have an email notification whenever a borrow sends a message? Can we support an email response back into the chat? Can the borrower receive emails whenever an admin responds with a message?
-->

<!-- agent:queue preset="#spec-test-commit-push" go -->
- I plan to use Claude Code & Codex to use the api. How should I answer "9. Do you plan to use your Google Ads API token with a tool developed by someone else? *". If yes, "Please provide the URL of the tool. *".
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
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
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#vd8s] [recommended] Investor Pipeline phase 2 — remaining after stalled-stage highlighting shipped (EFS 055a73c): drag-to-confirm-wire from the Committed lane (needs multi-pending-fund disambiguation) and TMO AddLoanFunding sync state in the Active lane once the writeback gate (#dzmm/#k2xg) clears. Per-investor activity timeline folds into the #vtx8 detail modal. Plan: src/equityfundingsource-dev/prds/investor-pipeline-admin.md
- [/] [#67v2] Admin loan pipeline: TMO-aligned stages + Kanban (drag cards, auto-advance when stage requirements met) synced to dashboard; mirror TMO stages as single source of truth. From 2026-06-03 call. Relates to #dzmm/#k2xg.
- [/] [#zwws] [recommended] Investor dashboard rework — remaining phases are externally gated: 4 vestings/special-instructions (TMO writeback-gated #dzmm/#k2xg/#7a4d); 6 new-deal email + workflow-priority dashboard (#f4rq). Shipped: P1 nav (30037e3), P2 dashboard-composite landing (3e06907), P2b interest-button + distributions/statements cleanup (efs cd4253a), P3 simplified portfolio (8d40c66), P5 accreditation→annual Requirements (efs 1957f64). Plan: src/equityfundingsource-dev/prds/rpzm-investor-dashboard-rework.md. Queue one phase at a time.
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
