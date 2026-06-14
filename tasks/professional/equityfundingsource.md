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
queue: start
agent_doc_pipeline:
  run_id: cycle-1781420754344
  step: write_applied
---

## Status

<!-- agent:status patch=replace -->

committed
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260614-065755.md`*

Compacted content:
- Archived 11 response topic(s): Use proper label names on the Declarations summary; deploy; "Drop or choose a replacement file" → "Drop file or click to select file"; 8 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f...

### Re: Wide Deal Pipeline kanban — UX options to cut horizontal friction — opus-4-8

> **Queue prompt:**
>
> The Deal Pipeline kanban board is very wide...which can add friction to the admin workflow. What are some UX options to mitigate this issue? Discuss...


The root cause is one column per TMO stage. With ~8–12 stages, the board is wider than any screen, so admins horizontal-scroll to find work and lose the at-a-glance overview that makes a kanban worth having. Options, cheapest-to-richest:

**1. Collapse the many stages into a few macro-phases (recommended default).**
Group the detailed TMO stages under 4–5 phase columns — e.g. *Intake → Underwriting → Approval → Closing → Funded* — and keep the precise stage as a chip/label on the card (and a sub-row if you want). Drag between phases still works; the exact stage shows on the card and in the detail drawer. This is the single biggest width win because column count drops by ~half or more, and it matches how an admin actually triages ("where is this deal, roughly?") before drilling in.

**2. Collapsible / condensed columns.**
Let admins collapse any stage to a thin rail showing just the name + count badge, expanding on click. Auto-collapse empty stages. Pairs well with #1 — collapse the phases you're not working today. Low effort, no data model change.

**3. List/table view toggle.**
Offer a dense, sortable table (group or sort by stage) as an alternate view. Kanban is great for visual flow; a table is better for bulk triage and handles many stages with zero horizontal sprawl. A view toggle (Board / List) lets each admin pick. This is the most reliable "never scroll sideways again" answer.

**4. Saved filters / focused views.**
"My deals", "active only", single-stage, by-broker, by-date. Most width pain is from rendering deals nobody is touching right now; a default "active" filter shrinks the working board immediately. Cheap, high payoff, composes with everything above.

**5. Card density control.**
A compact card mode (one line: borrower + amount + stage chip) narrows every column, so more fit before scroll kicks in. Quick CSS-level change.

**6. Master–detail stage rail (bigger rework).**
Replace the horizontal columns with a vertical left rail of stages + counts; clicking a stage shows its deals in the main pane. Eliminates horizontal width entirely. More build, but the most scalable if the stage list keeps growing.

**Recommended path:** ship **#1 (phase grouping) + #4 (default "active" filter) + #3 (List toggle)** together — phase grouping fixes the structural width, the active filter trims the working set, and the List view is the escape hatch for power triage. #2 and #5 are low-cost polish to layer on after. This stays inside the existing #67v2 kanban work rather than a rewrite.

This is a discussion, so I haven't changed code. If you want, say the word and I'll queue the chosen combination as an implementation item against #67v2.

### Re: Deal Pipeline horizontal width improvements — gpt-5

> **Queue prompt:**
>
> Implement the recommended path for the Deal Pipeline horizontal width improvements.


Implemented the recommended path for the Deal Pipeline width issue in `src/equityfundingsource-dev`.

Changed paths:
- `lib/ui--browser--efs/portal/admin/pipeline.tsx`
- `tests/admin-pipeline.test.ts`
- `tests/application-status-ui.test.ts`
- `tests/portal-table-dialog.test.ts`
- `prds/investor-pipeline-admin.md`
- `prds/equity-funding-source-lending-portal-prd.md`

What changed:
- Deal Pipeline now defaults to an **Active** scope that hides terminal/parked cards.
- Board columns are grouped into macro phase lanes: **Intake**, **Processing**, **Underwriting**, with **Funded / Closed** and **Parked** available under **All**.
- Cards keep the precise raw status visible as a stage chip, so phase grouping does not hide TMO/application state.
- Added a `?view=table` dense triage view over the same filtered pipeline cards.
- Drag/drop still delegates through the shared loan-status writer; phase drops use the lane's primary target status.
- Updated PRD notes and static tests that previously asserted the board was Kanban-only.

Verification:
- `bun test tests/admin-pipeline.test.ts` — pass, 50 tests.
- `bun test tests/portal-table-dialog.test.ts tests/application-status-ui.test.ts` — pass, 23 tests.
- `bun run check` — pass, 674 tests + app build + worker build.
- `make check` — pass, same EFS full check path.
- `git diff --check` — pass.

Commits/pushes:
- EFS submodule: `0dd1f15` (`admin pipeline: add compact triage views`) pushed to `btakita/equityfundingsource-dev main`.
- Superproject pointer: `99579ecfa` (`efs: update deal pipeline triage views`) pushed to `btakita/agent-loop main`.

GitHub Actions:
- No workflow named `CI` exists in the EFS submodule or superproject.
- The superproject's only workflow, `Agent Doc`, ran for `99579ecfa` and failed before any runner started: both jobs had `runner_id: 0`, no steps/logs, and the annotation says recent account payments failed or the spending limit needs to be increased. That is an external CI-start/billing blocker, not a code/test failure.

No new actionable follow-up item was created by this cycle; the next queued item already covers the optional #2 + #5 polish.

Note: preflight also warned the installed `agent-doc` binary is stale relative to source; I did not rebuild it because this cycle was scoped to EFS pipeline work.
<!-- agent:boundary:1590f0ae -->
<!-- /agent:exchange -->
###
<!--
Is there an admin for the borrower portal messages? Can we have an email notification whenever a borrow sends a message? Can we support an email response back into the chat? Can the borrower receive emails whenever an admin responds with a message?
-->

<!-- agent:queue preset="#spec-test-commit-push" go -->
- ~~Implement the recommended path for the Deal Pipeline horizontal width improvements.~~
- Add #2 + #5 polish for the Deal Pipeline horizontal width improvements
- deploy
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
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
