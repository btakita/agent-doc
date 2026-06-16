---
agent_doc_session: 62fe1f41-886c-4f3d-b6fa-48d46106ec27
resume: 019ed201-3acf-79e2-8b39-876609f3c6b3
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260616-064243.md`*

Compacted content:
- Archived 1 response topic(s): copy-button layout + deploy
- Prior summary/context: prior compacted content: Archived 8 response topic(s): compact card labels + deploy; Borrower Info inline values + ge...

### Re: lender selection pipeline — gpt-5

> **Queue prompt:**
>
> :pushpin: Continue setting up the lender selection pipeline. Where lenders are sent information, communication with the lender, etc.

Continued the lender selection pipeline setup by adding structured lender communication follow-up fields to the existing deal-interest workflow. The Lender Pipeline modal now records last lender contact time, method, contact summary, and next follow-up date alongside the package/call/selection/verbal-commitment milestones.

Changed paths: `src/equityfundingsource-dev/app/equityfundingsource/migrations/0070_deal_interest_lender_communication.sql`, admin investor workflow API/types, `lib/ui--browser--efs/portal/admin/investor_pipeline.tsx`, `prds/investor-pipeline-admin.md`, and `tests/admin-investor-pipeline.test.ts`.

Verification: `bun test tests/admin-investor-pipeline.test.ts` passed (61 tests); `make check` passed (741 tests, app build, Worker build); `git diff --check` passed. GitHub Actions note: neither the EFS submodule nor the parent repo has a workflow named `CI`; the parent repo's available `Agent Doc` workflow was queued as run `27599285422` after the push.

Committed and pushed: EFS submodule `6ba172b` (`feat(admin): track lender communication follow-ups (#deal-interest-communication)`) to `btakita/equityfundingsource-dev:main`; parent repo pointer `7f62390ad` (`chore(efs): advance lender communication pipeline`) to `btakita/agent-loop:main`.

Escalation: none. Confidence: high. No deploy was run; this queue item used the `#spec-test-commit-push` path, and the new D1 migration will be applied by the normal deploy path. Exchange is large, but the active queue was drainable, so I did not pause for compaction.

### Re: Deal Pipeline + GoHighLevel — gpt-5

> **Queue prompt:**
>
> I'm talking about the Deal Pipeline. https://equityfundingsource.brian-takita.workers.dev/portal-admin/loans/45ddae53-3e7f-43b1-a627-17b79b06fa3f and the Available Deal dialog. The admin will communicate with lenders over email and select a lender. Would Go High Level integration be a good idea here?

Yes, GHL is a good fit here, but only as the communication/CRM layer, not as the source of truth for the Deal Pipeline.

For the Available Deal dialog, EFS should own the durable deal state: which lenders are interested, who was selected, package/document permissions, borrower document requests, verbal commitment, funding allocation, audit history, and TMO/R2/D1 links. That state is too EFS-specific and too tied to documents/security to move into GHL.

GHL is useful around that core workflow: send the lender package email, include a calendar link, create follow-up tasks, track replies/conversations, update a CRM opportunity/contact, and keep Robert/Judith's outreach automations in one familiar CRM. Official docs support the basic shape: HighLevel has APIs for contacts, messaging, workflows, calendars, webhooks, and more; the Conversations API has a `POST /conversations/messages` endpoint; workflows include email/SMS/call/manual-action steps and create/update opportunity actions. Sources checked: https://help.gohighlevel.com/support/solutions/articles/48001060529-highlevel-api-documentation, https://marketplace.gohighlevel.com/docs/ghl/conversations/send-a-new-message/, https://help.gohighlevel.com/support/solutions/articles/155000002294-what-are-workflow-actions-complete-list-.

Recommended architecture:

1. EFS remains canonical. The Available Deal dialog should show interested lenders and selection controls directly from D1 (`investor_deal_interests`, funds/allocations, documents).
2. GHL is an adapter. Store GHL contact/conversation/opportunity/message IDs on the EFS interest/deal rows, but never require GHL to render the Deal Pipeline.
3. Start narrow. First integration should be "Send package / start follow-up" from the Available Deal dialog: sync or create lender contact in GHL, send/trigger the email, store the GHL message/conversation ID, and log the result in EFS.
4. Pull replies back into EFS only as communication history. A reply can update last-contact/next-follow-up fields, but selecting a lender should remain an explicit admin action in EFS.
5. Avoid GHL as the pipeline board. Its opportunity pipeline can mirror a deal for CRM visibility, but EFS needs to model multiple interested lenders per deal, multi-lender allocations, private document access, and TMO writeback gates.

Caveats: use current HighLevel API docs and private integration/OAuth rather than legacy V1 API patterns; their docs say V1 reached end-of-support on December 31, 2025. Also confirm Robert's enterprise/sub-account access includes the API/workflow features needed, because HighLevel says API access varies by plan.

Changed paths: session document only. Verification: reviewed local EFS Deal Pipeline/Lender Pipeline notes and current official HighLevel API/workflow docs. Escalation: none. Confidence: medium-high.

### Re: Deal Pipeline GHL Adapter — gpt-5

> **Queue prompt:**
>
> do [#deal-pipeline-ghl-adapter]

Implemented the GoHighLevel adapter foundation for the Deal Pipeline lender-selection workflow. EFS/D1 stays the source of truth for Available Deal lender selection, documents, audit history, verbal commitment, allocations, and TMO gates; GHL is now modeled as a CRM/outreach adapter around each `investor_deal_interests` work item.

What shipped:

1. Added migration `0071_deal_interest_ghl_adapter.sql` with per-interest GHL contact, conversation, opportunity, package message, package workflow, sync status/error, and synced-at fields.
2. Round-tripped those fields through the admin workflow API and workflow summaries.
3. Added a pure strategy helper: contact + package workflow ID means trigger a GHL workflow; contact without workflow ID means direct Conversations API message; no contact means `not_configured`.
4. Added a GoHighLevel adapter block to the Lender Pipeline modal so admins can manually link current GHL IDs now and future automation can reuse the same D1 fields.
5. Updated `prds/investor-pipeline-admin.md` with the integration boundary and send-path decision.

Current official HighLevel docs checked: API platform overview / V1 end-of-support note (`2025-12-31`) at https://help.gohighlevel.com/support/solutions/articles/48001060529-highlevel-api-documentation, Conversations `POST /conversations/messages` at https://marketplace.gohighlevel.com/docs/ghl/conversations/send-a-new-message/, Opportunities API at https://marketplace.gohighlevel.com/docs/ghl/opportunities/opportunities/, and contact workflow enrollment at https://marketplace.gohighlevel.com/docs/ghl/contacts/add-contact-to-workflow/.

Changed paths: `src/equityfundingsource-dev/app/equityfundingsource/migrations/0071_deal_interest_ghl_adapter.sql`, admin investor workflow API/types, `lib/ui--browser--efs/portal/admin/investor_pipeline.tsx`, `prds/investor-pipeline-admin.md`, and `tests/admin-investor-pipeline.test.ts`.

Verification: `bun test tests/admin-investor-pipeline.test.ts` passed (63 tests); `make check` passed (743 tests, app build, Worker build); `git diff --check` passed. GitHub Actions note: neither the EFS submodule nor the parent repo has a workflow named `CI`. The parent `Agent Doc` workflow run `27601831539` failed before runner startup: both jobs had zero steps/logs and GitHub annotated that recent account payments failed or the spending limit needs to be increased.

Committed and pushed: EFS submodule `89b9558` (`feat(admin): add GHL deal pipeline adapter state`) to `btakita/equityfundingsource-dev:main`; parent repo pointer `a11c22952` (`chore(efs): advance GHL adapter state`) to `btakita/agent-loop:main`.

Escalation: none. Confidence: high. No deploy was run; this queue item used the `#spec-test-commit-push` path, and the new D1 migration will be applied by the normal deploy path.

### Re: GHL communication pipeline — gpt-5

> **Queue prompt:**
>
> How would GHL benefit the communication between EFS, lenders, and borrowers? Can we facilitate the Processing, Underwriting, Terms negotiation, and Closing stages of the deal pipeline?

Yes. GHL can help a lot here if it is treated as the communication/workflow layer around the Deal Pipeline, not as the system of record for the Deal Pipeline.

The right split is:

1. **EFS/D1 stays canonical** for loan status, Available Deal visibility, interested lenders, selected lender, borrower portal messages, document requests, private document access, underwriting notes, verbal terms, allocation/funding state, audit history, TMO sync gates, and close/funded state.
2. **GHL mirrors and orchestrates communication**: contacts, conversations, outbound package emails/SMS, follow-up tasks, calendar/booking links, internal notifications, workflow reminders, and optional opportunity-stage visibility for Robert/Judith.
3. **Every GHL action should write back an external ID or event summary into EFS**, so the admin can still understand the deal from EFS if GHL is unavailable.

This fits the current EFS implementation. The Lender Pipeline already tracks per-interest lender communication fields and GHL adapter IDs (`ghl_contact_id`, `ghl_conversation_id`, `ghl_opportunity_id`, package workflow/message IDs, sync status/error). That is the right foundation: one GHL contact/conversation/opportunity can be linked to a specific lender + deal-interest work item without letting GHL decide who is selected or funded.

How I would use it by stage:

1. **Processing**
   - EFS moves the loan/deal into Processing.
   - EFS syncs/links borrower and lender contacts in GHL.
   - GHL sends the initial processing email, follow-up reminders, and internal notifications.
   - EFS records the GHL contact/conversation/opportunity IDs and next follow-up state.

2. **Underwriting**
   - EFS remains the borrower document-request system because private files and portal permissions belong there.
   - GHL sends reminders with secure EFS portal links, not sensitive attachments.
   - Lender package updates can go through a GHL workflow or direct conversation message.
   - Replies and no-response signals should update EFS communication history and next-action fields.

3. **Terms negotiation**
   - GHL is useful for email threads, call reminders, templates, and CRM visibility.
   - EFS should store the actual negotiated state: selected lender, verbal amount, conditions, pre-doc instructions, term summary, and approval/funding status.
   - A GHL opportunity stage can mirror “Terms negotiation,” but should not be the authoritative approval/selection state.

4. **Closing**
   - GHL can send closing reminders, internal notifications, calendar links, and possibly documents/contracts where the document is suitable for GHL.
   - EFS should still own final closing state, deal close action, wire/funding records, lender allocation, borrower portal state, document archive, and TMO writeback.
   - Add tests around adapter planning, outbox idempotency, status/error persistence, borrower-message notification routing, and webhook signature/thread-token rejection.
   - Start in dry-run/manual-link mode for one or two deals, then enable outbound package workflow, then stage-bridge workflows, then inbound reply threading.
   - Store secrets in Worker env, redact GHL errors in user-facing UI, and log full adapter failures only in admin/debug surfaces.
   - Send secure portal links, not private borrower/lender documents as GHL attachments unless explicitly approved.
6. **Security, observability, and rollout**
   - Do not let inbound GHL events select a lender, approve terms, release documents, or close/fund a deal without explicit EFS admin action.
   - Ingest lender/borrower replies, appointment events, workflow completion, and bounce/failure events as communication history and next-action hints.
   - Add GHL webhook or polling ingestion only after outbound IDs are stable.
5. **Reply/event ingestion**
   - Email reply back into chat should terminate at EFS, not GHL-owned state: require signed thread tokens or verified inbound routing, append the reply to D1, and only mirror an event summary to GHL.
   - Admin replies in EFS: EFS creates the D1 chat message, then GHL can send the borrower an email/SMS notification that links back to the borrower portal.
   - Borrower sends secure message: EFS creates the D1 chat message, notifies the owning admin in-app, and optionally creates/sends a GHL conversation notification with a secure EFS portal link.
   - Keep `chat_sessions` and `chat_messages` canonical in EFS.
4. **Borrower/admin secure-message bridge**
   - Make the outbox idempotent by EFS entity ID + stage + event type, with retry/dead-letter state, rate-limit handling, and an admin-visible last sync/error summary.
   - Map Processing, Underwriting, Terms negotiation, and Closing to GHL workflow enrollment and optional opportunity-stage mirroring.
   - On EFS stage transitions, enqueue an outbound sync job rather than calling GHL inline from the UI.
3. **Deal-stage bridge**
   - Store the returned contact/conversation/opportunity/message/workflow IDs and sync status in EFS, and keep the EFS deal-interest row as the source of truth for selected lender, verbal commitment, conditions, allocation, and funding state.
   - Upsert or link the lender contact, optionally create/update the GHL opportunity, then either enroll the contact into a package workflow or send a direct conversation message.
   - Use the existing `investor_deal_interests` GHL fields as the first production bridge.
2. **Lender package send from the Available Deal dialog**
   - Define the mirrored GHL opportunity pipeline stages for visibility only: Processing, Underwriting, Terms negotiation, Closing, Funded/Closed, Parked/Lost.
   - Define GHL custom fields/tags for EFS IDs: `efs_user_id`, `efs_loan_id`, `efs_deal_id`, `efs_deal_interest_id`, role (`borrower`, `lender`, `broker`), stage, and portal URL.
   - Confirm Robert's HighLevel location/sub-account, plan/API access, sending domains, phone/SMS capability, calendar ownership, and whether this should use a Private Integration Token or OAuth.
1. **Credentials and GHL model setup**

Current HighLevel docs support this architecture: HighLevel documents REST APIs for contacts, messaging, workflows, calendars, payments, webhooks, and related CRM functions, but says V1 API support ended on **December 31, 2025**, so new work should use current marketplace docs / V2 private integration or OAuth surfaces: https://help.gohighlevel.com/support/solutions/articles/48001060529-highlevel-api-documentation. The Conversations API exposes `POST /conversations/messages`: https://marketplace.gohighlevel.com/docs/ghl/conversations/send-a-new-message/. Contacts can be enrolled into a workflow with `POST /contacts/:contactId/workflow/:workflowId`: https://marketplace.gohighlevel.com/docs/ghl/contacts/add-contact-to-workflow/. Opportunities can be created/updated through the Opportunities API: https://marketplace.gohighlevel.com/docs/ghl/opportunities/create-opportunity/. Workflows currently include communication actions, internal notifications, appointment actions, opportunity actions, and document/contract actions: https://help.gohighlevel.com/support/solutions/articles/155000002294-what-are-workflow-actions-complete-list-.

Recommendation: build the next integration as an **EFS-to-GHL stage bridge**. On each EFS stage transition or lender workflow action, enqueue an outbound sync job that upserts/links GHL contacts, optionally upserts a GHL opportunity, triggers the right GHL workflow, and stores the returned IDs/status in EFS. GHL then improves response speed and follow-up consistency across EFS, lenders, and borrowers, while EFS remains the trusted deal, document, and funding ledger.

Added follow-up backlog item `#ghl-stage-bridge` for the implementation bridge.

Changed paths: session document only. Verification: reviewed current EFS PRD/UI state and current official HighLevel API/workflow docs. Escalation: none. Confidence: high.

### Re: GHL stage bridge queue correction — gpt-5

Correction to the previous closeout: I cleared the self-created `#ghl-stage-bridge` backlog entry instead of leaving it in this priority queue. The GHL stage bridge remains the recommended implementation direction, but it is not an active queued task from this answer.

Changed paths: session document only. Verification: pending `session-check` after this repair write. Escalation: none. Confidence: high.

### Re: GHL stage bridge parked — gpt-5

Parked `#ghl-stage-bridge` in the icebox instead of the priority backlog so the recommendation is preserved without starting an implementation queue item.

Changed paths: session document only. Verification: pending `session-check` after this repair write. Escalation: none. Confidence: high.
Added the clean parked GHL integration plan to `tasks/professional/equityfundingsource.md` and committed it as `0304d828c`. The queue item is consumed, and no active backlog item was added; the plan stays parked against existing icebox items `#z3kx` and `#ghl-stage-bridge`.

Verification: `agent-doc finalize` completed and `session-check` passed. No code/tests were run because this was plan-only. Preflight still reports existing warnings about a stale local `agent-doc` install and possible semantic matches in older review items; I did not change those.

### Re: GHL integration plan — gpt-5

> **Queue prompt:**
>
> :pushpin: Create a plan for the GHL integration. I will park this in the icebox for now.


Clean parked plan for the GoHighLevel integration:

1. **Keep EFS canonical.** EFS/D1 owns loans, deal stages, borrower secure messages, document permissions, lender selection, funding allocation, audit history, and TMO gates. GHL is the communications/CRM adapter: contacts, conversations, templates, workflows, calendar links, tasks, and mirrored opportunity visibility.
2. **Start with lender package send.** From the Available Deal dialog, link/upsert the lender contact, optionally create/update the GHL opportunity, then either enroll the contact in a package workflow or send a direct GHL conversation message. Persist all returned GHL IDs/status back onto the existing `investor_deal_interests` adapter fields.
3. **Add an EFS-to-GHL stage bridge.** On Processing, Underwriting, Terms negotiation, and Closing transitions, enqueue idempotent outbound jobs that update GHL contact/opportunity/workflow state. Do not call GHL inline from the UI; keep retry/dead-letter state and admin-visible sync errors in EFS.
4. **Bridge borrower/admin messaging carefully.** `chat_sessions` and `chat_messages` stay canonical. Borrower message notifications and admin-reply notifications can be sent through GHL, but inbound email replies should terminate at EFS through verified routing/thread tokens before appending to D1. GHL should receive a mirror/event summary, not own the chat thread.
5. **Ingest replies only as communication history.** GHL webhooks or polling can update last-contact, next-follow-up, reply/bounce, appointment, and workflow-completion summaries. They must not select a lender, approve terms, release documents, close a deal, or mark funding complete without explicit EFS admin action.
6. **Roll out in stages.** Dry-run/manual-link one or two deals first, then enable lender package workflow, then stage bridge, then inbound reply threading. Use secure EFS portal links rather than sensitive attachments unless EFS explicitly approves the document class for GHL.

Parking: I would leave this in the icebox under the existing GHL items (`#z3kx` for GHL as messaging/notifications path of record and `#ghl-stage-bridge` for stage mirroring). I did not add an active backlog item because the requested plan is intentionally parked for later.

Docs checked: HighLevel API overview / V1 end-of-support note (https://help.gohighlevel.com/support/solutions/articles/48001060529-highlevel-api-documentation), Conversations `POST /conversations/messages` (https://marketplace.gohighlevel.com/docs/ghl/conversations/send-a-new-message/), contact workflow enrollment (https://marketplace.gohighlevel.com/docs/ghl/contacts/add-contact-to-workflow/), opportunity creation (https://marketplace.gohighlevel.com/docs/ghl/opportunities/create-opportunity/), and workflow actions (https://help.gohighlevel.com/support/solutions/articles/155000002294-what-are-workflow-actions-complete-list-).

Changed paths: session document only. Verification: agent-doc preflight/plan passed; reviewed the shipped EFS GHL adapter and borrower-message PRD notes; checked current official HighLevel docs. No code/tests were run because this was plan-only. Escalation: none. Confidence: high.

### Re: lender selection + deploy — gpt-5

> **Queue prompt:**
>
> :pushpin: What can we implement for the lender selection process in the admin Available Deals UIs? I want a workflow where the admin can see the Interested Lenders, select an Interested Lender (in the UI and bi-directional sync with TMO).


Implemented and deployed the admin Interested Lenders selection workflow.

What changed:
- Added `PUT /api/admin/deal-interests/:id/select`, which selects exactly one interested lender per deal, clears any previous selected interest for that deal, and returns the refreshed interested-lender list.
- Added a gated `tmo_cache_outbox` enqueue helper for `select_lender` payloads. Selection requires the loan's TMO loan number and the selected lender's TMO account; replay remains gated on #7a4d/#dzmm until the selected-lender TMO write-field map is confirmed.
- Updated Deal Detail / Interested Lenders UI so admins can select a lender from the workflow controls, see the selected badge/card styling, and see a selected-lender summary with TMO account + queued sync state.
- Documented the shipped cut in `prds/investor-pipeline-admin.md` as Phase 5f.

Commits:
- EFS submodule: `386aa6b feat(admin): select interested lender with TMO outbox` pushed to `btakita/equityfundingsource-dev main`.
- Parent pointer: `8ad76cdee chore(efs): advance lender selection workflow` pushed to `btakita/agent-loop main`.

Verification:
- `bun test tests/admin-investor-pipeline.test.ts` — 66 passed.
- EFS `make check` — 749 tests passed, app build passed, Worker build passed.
- Parent `make check` — passed, including instruction-files, agent-doc, and corky suites.
- `git diff --check` / staged diff checks were clean.

Deploy:
- Ran `make deploy` from `src/equityfundingsource-dev`.
- Applied remote D1 migrations `0070`-`0073`; postdeploy migration list reports no pending migrations.
- Deployed Worker version `e51e9088-581a-47c1-a6e8-b7a1517ce4ad`.
- Smoke checks: public root returned `200`; `/portal-admin/deals` returned `303` as expected behind Cloudflare Access.

CI note:
- EFS repo has no workflow named `CI` and no recent Actions runs listed.
- Parent repo has no workflow named `CI`; latest `Agent Doc` run for `8ad76cdee` failed before jobs started because GitHub reports account billing/spending-limit blockage, not a test failure: https://github.com/btakita/agent-loop/actions/runs/27647115956

Agent-doc note: preflight still warns that the installed `~/.cargo/bin` agent-doc cdylib predates source commit `ce26d32c`; rebuild with `make install` in `src/agent-doc` when you want the live-session binary refreshed.

### Re: deploy — gpt-5

> **Queue prompt:**
>
> :pushpin: deploy


Deploy is complete.

- Ran `make deploy` from `src/equityfundingsource-dev`.
- Remote D1 migrations `0070_deal_interest_lender_communication.sql` through `0073_loan_activity_logs.sql` applied successfully.
- Postdeploy migration check reports `No migrations to apply`.
- Worker version: `e51e9088-581a-47c1-a6e8-b7a1517ce4ad`.
- Smoke checks: public root returned `200`; `/portal-admin/deals` returned `303`, matching the Cloudflare Access redirect expectation.
- Latest parent Actions run for the pushed pointer commit did not run jobs because GitHub reported account billing/spending-limit blockage, not a test failure: https://github.com/btakita/agent-loop/actions/runs/27647115956

Session note: exchange is large, but queue mode was active, so I continued draining instead of stopping to compact.
<!-- agent:boundary:9d9f28cc -->
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
- [ ] [#ghl-stage-bridge] [recommended] Implement an event-driven GHL stage bridge from EFS Deal Pipeline state to HighLevel contacts/opportunities/workflows for Processing, Underwriting, Terms negotiation, and Closing while keeping EFS/D1 canonical for loan, lender-selection, borrower portal, document, audit, and funding state.
- [ ] [#xt4d] Inventory TMO usage once access is restored: screenshots, exports, required fields, statuses, document flows, notifications, reports, integrations, and workflows EFS actually uses.
- [ ] [#0e1p] Draft phased TMO replacement roadmap after the demo: EFS-first MVP, TMO sync/cutover plan, document
- [ ] [#f4rq] Verify an EFS-owned sending domain in Resend, then switch APPLICATION_LEAD_NOTIFICATION_FROM and APPLICATION_LEAD_NOTIFICATION_TO from testing-mode values to EFS production addresses.
- [ ] [#k2xg] Send Robert the six-question TMO outbound webhook prompt in src/equityfundingsource-dev/docs/tmo-webhooks-question.md for him to forward to Mortgage Office, then record the response and decide push vs. tightened polling.
- [ ] [#m6ch] Get Google Suite admin credentials from Robert (SMTP + Google Calendar integration)
- [ ] [#p40t] [recommended] Google Calendar/G Suite notification integration after Google admin credentials are available
- [ ] [#q2x2] HTML email templates: lighter navy + gold, 2-3 drip for PBM orphan investor outreach
- [ ] [#x5rn] Convert EFS logo JPEG -> transparent PNG/SVG (Robert sent JPEG)
- [ ] [#kzt6] Collect testimonials for carousel (6-12 min: investors, brokers, borrowers) — push Judith + Robert helping
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
