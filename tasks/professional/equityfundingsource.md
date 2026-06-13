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
queue: stop
---

## Status

<!-- agent:status patch=replace -->

committed
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260613-164802.md`*

Compacted content:
- Archived 2 response topic(s): emails, TMO webhooks, funded-deals copy, disclosure; borrower notification emails
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f...

### Re: advance the review items — opus-4-8

Went through all 10. Two genuinely unblocked items became backlog; the rest are either already shipped or waiting on something external. Triage:

**Actionable now — added to backlog:**
- **#tmopoll** (advances #dzmm, and is the keystone for the whole writeback cluster): the TMO tightened-polling sync you chose over webhooks. Phase P1 = a scheduled Worker cron mirroring TMO loan/funding/schedule reads into D1 + a sync-log. Plan: `tasks/professional/equityfundingsource/plan-tmo-polling-sync.md` (committed). This is what unblocks #vd8s, #67v2, #zwws phase 4, and #7a4d.
- **#xcvginbound** (advances #xcvg): the inbound email→chat path — `POST /api/email/inbound` with sender/thread resolution + auth, appending into `chat_messages`. This is the code half and is **not** gated on the sending domain. The outbound borrower notification stays config-gated on #f4rq (the `#emrt` done item covers the flip).

**Already shipped — only a TMO-gated remainder is left (candidates to resolve):**
- **#qakx** — investor document pages are D1/R2-backed (shipped via #qakx-impl).
- **#37ah** — broker portal backend is D1-backed (shipped via #37ah-backend); live TMO sync rides on #tmopoll.
- **#vd8s** — drag-to-confirm-wire shipped; the Active-lane AddLoanFunding sync is now tracked under #tmopoll.
- **#67v2** — Kanban shipped; auto-advance-on-requirements is now tracked under #tmopoll.

**Externally gated — not agent-actionable, here's the unblock:**
- **#jnzs** — Judith's sign-off. Per your call we sync directly rather than emailing her; this needs you to capture her approval.
- **#zwws** — remaining phases gated on TMO writeback (#tmopoll) + the Resend domain (#f4rq); queue one phase at a time.
- **#next-steps** — custom-domain cutover is intentionally deferred; true Workspace SSO is gated on Google admin creds (#m6ch).
- **#dropbox-sync-adapter** — gated on Dropbox access from you/Robert.

Net new actionable work: **#tmopoll** and **#xcvginbound**. Say the word (or queue one) to implement.

### Re: do [#tmopoll] + do [#xcvginbound] — both shipped — opus-4-8

Both done, committed, and pushed. Full EFS suite green (631 pass / 0 fail), build + worker build OK.

**#tmopoll — TMO polling sync (EFS `75ff1ea`).** Important finding: the polling *engine* already existed and is wired — `tmo_sync_all_`/`tmo_sync_if_due_` read TMO loans/fundings/schedules into a D1 cache mirror, recording per-run `tmo_sync_batches` + per-field `tmo_cache_change_logs`, driven by a `scheduled()` handler on an hourly cron with admin-configurable intervals. So I did **not** rebuild it. The real gap was operator *visibility*: the run history and change log were never surfaced. I added read functions (`tmo_sync_batches_recent_R_`, `tmo_cache_change_logs_recent_R_`), a `GET /api/admin/tmo-sync` status endpoint, and a **TMO sync status panel** in admin Settings (last run time/status/counts + recent changes, refreshing after a manual sync). Tests cover the reads.

**#xcvginbound — inbound email→chat (EFS `d8e6320`).** Replaced the 501 scaffold at `POST /api/email/inbound` with a real, authenticated handler. Two-factor sender auth: (1) the configured inbound provider must present a shared secret (`X-Inbound-Secret`; **fails closed when no secret is set**, so unauthenticated mail can never inject), and (2) the `from` address must resolve to an `admin` user. Authenticated replies append as an admin chat message via `borrower_chat_message_create_admin_`, claim thread ownership, and notify the borrower — same path as a portal admin reply. This is the #xcvg *code* half; outbound borrower notification stays config-gated on the verified sending domain (#f4rq).

Net: #vd8s/#67v2/#zwws phase 4/#7a4d now have a live TMO data mirror + visibility behind them; the borrower message round-trip's inbound leg is built and waiting only on inbound-routing config (`MESSAGE_EMAIL_INBOUND_ENABLED` + `MESSAGE_EMAIL_INBOUND_SECRET` + provider webhook).

### Re: Step 7 "Review & Sign" reverts to Step 6 on refresh — opus-4-8

> **Queue prompt:**
>
> On https://equityfundingsource.brian-takita.workers.dev/portal-borrower/new-application?application_id=cda67748-3a99-49b7-95b4-cdfede0fcb7a, the Step 6 Documents is complete by me clicking "Save & Continue" and then the Step 7 is complete by me clicking "Save & Continue", which takes me to Step 7 "Review & Sign". But when I refresh, I go back to Step 6. deploy


Fixed and deployed (EFS `ca63ddf`, worker version `1e9b13fe`).

**Root cause.** The borrower full-application wizard never persisted which step you were on — on load it *reconstructed* the resume step purely from how complete each section's data was (`resume_step_from_sections_`). That heuristic can't tell the **Documents** step (index 5) from **Review & sign** (index 6): both have all field steps complete, and the Documents step has no required-field gate. So its last line returned `signature.esign_consent ? 6 : 5` — i.e. it could only land you on Review & sign *after* you e-signed. Refreshing while sitting on Review & sign (not yet signed) always recomputed to 5 = Step 6 Documents.

**Fix.** Persist the furthest step you actually reached per application (`localStorage`, keyed by application id) and use it via a new pure `resolve_resume_step_` to disambiguate **only** the Documents↔Review tail. The completeness heuristic stays authoritative for the field-entry steps (0–4), so a stale stored value can never push you forward past genuinely incomplete earlier data; only forward progress is recorded.

**Verification.** Added `tests/full-application-resume-step.test.ts` (8 cases: tail disambiguation, no-push-past-incomplete, clamp, heuristic fallback). Full suite green — 639 tests, 0 fail — then `build` + `build:worker` + `deploy:worker` succeeded. Note: this is a per-browser resume (localStorage); a different browser/device still falls back to the completeness heuristic.

### Re: Print button on the borrower new-application page — opus-4-8

> **Queue prompt:**
>
> Please add a Print button to https://equityfundingsource.brian-takita.workers.dev/portal-borrower/new-application?application_id=cda67748-3a99-49b7-95b4-cdfede0fcb7aa


Done — committed in EFS `d7622a7`.

A Print button already existed in the wizard topbar (uncommitted from a prior cycle), but it called `window.print()` directly. Because the new-application form is a step-gated wizard, only the *currently visible* step is in the DOM — so that button would have printed a single step, not the application.

Fixed it to print the **full** application:
- The button now sets a `printing` flag; an effect renders every step into the DOM, then opens the print dialog and resets the flag (on `afterprint`, with a synchronous fallback).
- Each step gate (`{(printing || step === N) && …}`, steps 0–6) now also fires while printing.
- Existing print CSS in `portal.ts` already hides the sidebar/nav, stepper, form-actions, and buttons, so the printout is just the application content.

Verification: full suite green — **642 pass / 0 fail**; `build` + `build:worker` clean. Updated `tests/full-application-print-button.test.ts` to assert the full-application wiring (button → `set_printing(true)`, all 7 step gates render while printing).

Not yet deployed — the change is committed and built but the live worker at `equityfundingsource.brian-takita.workers.dev` still runs the old bundle. Say the word and I'll run `deploy:worker`.

### Re: deploy — opus-4-8

> **Queue prompt:**
>
> deploy


Deployed. The live worker now runs the Print-button bundle (EFS `d7622a7`).

- `bun run deploy:worker` — built and uploaded the worker (900 KiB / 209 KiB gzip), 1 new static asset, startup 16 ms.
- New live version: `0236deec-39fe-4699-86d7-eb0e4d78642e` at https://equityfundingsource.brian-takita.workers.dev (hourly cron trigger intact).
- Verified: `GET /` returns HTTP 200.

No source changed this cycle — the Print-button work was already committed and built last cycle, so there was nothing new to spec/test/commit; this turn just shipped it. The full-application Print button (renders all 7 steps into the DOM, then `window.print()`) is now live on the borrower new-application page.
<!-- agent:boundary:303f15f8 -->
<!-- /agent:exchange -->
###
<!--
Is there an admin for the borrower portal messages? Can we have an email notification whenever a borrow sends a message? Can we support an email response back into the chat? Can the borrower receive emails whenever an admin responds with a message?
-->

<!-- agent:queue preset="#spec-test-commit-push" go -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog queue -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#xcvg] [recommended] Borrower message email round-trip: email notification when a borrower sends a secure-portal message, inbound email reply that appends back into the D1 chat, and a borrower-facing email when an admin replies. Builds on the shipped chat_sessions/chat_messages + admin owner. Gated on a verified Resend EFS sending domain (f4rq) and Google Workspace SMTP (m6ch/p40t). (Distinct from the operator/comms emails to Judith + Robert.)
- [/] [#jnzs] [recommended] Await Judith or EFS reviewer approval on funded-deal images, copy, and lien-position wording before invoicing or launch sign-off. The funded-deals page carries no standalone legal/disclosure text; the disclosure needing legal review is the ESIGN/UETA Electronic record and signature disclosure on the borrower full-application/document-uploads flow (full_application.tsx). Funded-deals copy was reframed as real closed deals and shipped.
- [/] [#vd8s] [recommended] Investor Pipeline phase 2 — remaining after stalled-stage highlighting shipped (EFS 055a73c): drag-to-confirm-wire from the Committed lane (needs multi-pending-fund disambiguation) and TMO AddLoanFunding sync state in the Active lane once the writeback gate (#dzmm/#k2xg) clears. Per-investor activity timeline folds into the #vtx8 detail modal. Plan: src/equityfundingsource-dev/prds/investor-pipeline-admin.md
- [/] [#67v2] Admin loan pipeline: TMO-aligned stages + Kanban (drag cards, auto-advance when stage requirements met) synced to dashboard; mirror TMO stages as single source of truth. From 2026-06-03 call. Relates to #dzmm/#k2xg.
- [/] [#zwws] [recommended] Investor dashboard rework — remaining phases are externally gated: 4 vestings/special-instructions (TMO writeback-gated #dzmm/#k2xg/#7a4d); 6 new-deal email + workflow-priority dashboard (#f4rq). Shipped: P1 nav (30037e3), P2 dashboard-composite landing (3e06907), P2b interest-button + distributions/statements cleanup (efs cd4253a), P3 simplified portfolio (8d40c66), P5 accreditation→annual Requirements (efs 1957f64). Plan: src/equityfundingsource-dev/prds/rpzm-investor-dashboard-rework.md. Queue one phase at a time.
- [/] [#next-steps] Remaining EFS workers.dev production state: D1 API token unblocked; migrations 0025/0026 are already applied; Cloudflare Access env vars are deployed for /login/access-callback on workers.dev using the one-time PIN IdP. Custom domain cutover remains intentionally deferred; true Google Workspace SSO remains gated on Google admin credentials.
- [/] [#37ah] [recommended] Broker portal backend: replace static broker demo data with TMO-backed submit-deal, pipeline, commissions, documents, and notifications
- [/] [#dzmm] TMO webhooks assumed UNAVAILABLE per operator (2026-06-13) — TMO docs show no webhook/streaming API. Pursue the tightened-polling sync mitigation instead of push; the six-question TMO support email (#k2xg) is deferred (save for later).
- [/] [#qakx] Investor portal document pages: replace static Documents, Statements, and Accreditation fixture data with D1/R2-backed APIs and authenticated upload/request/download flows.
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
