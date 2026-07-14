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

complete
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260714-154832.md`*

Compacted content:
- Archived 14 response topic(s): Deal-modal tab-switch resize jank + consistent large size; Deal-modal lender-notify emails (Lender Documents + Funding Package) & remove Wire tab; Deploy the lender-notify email work (#dlne-deploy); 11 more
- Prior summary/context: prior compacted content: Archived 4 response topic(s): Document counts on the Loan Package / Lender Documents / Fundi...
Remove https://equityfundingsource.brian-takita.workers.dev/admin/pipeline?dialog=deal&dialog_deal_id=070770ae-0e0e-4962-9c63-1e08dfc8b0b4&deal_tab=funding-package > Wiring Instructions (Secure Link) since the Funding Package tab will have the wire instructions.
#spec-test-commit-push-deploy

### Re: Remove "Wiring Instructions (Secure Link)" from the deal Funding Package tab — opus-4-8

Done, deployed, and verified live.

**Change.** The Funding Package tab renders `LoanWiringSection`, which bundled the wire-instructions editor *and* the "Wiring Instructions (Secure Link)" issuance card (`LoanWireLinks`). I gated the secure-link card behind a new `show_secure_link` prop (default `true`) and pass `show_secure_link={false}` from `FundingPackageTab`. The standalone admin loan detail keeps the card. Two edits:

- `lib/ui--browser--efs/portal/admin/loan_detail.tsx` — `LoanWiringSection` now takes `show_secure_link` and only renders `LoanWireLinks` when true.
- `lib/ui--browser--efs/portal/admin/deal_detail.tsx` — the Funding Package tab passes `show_secure_link={false}`.

The wire-instructions **editor** (routing/account numbers, upload) stays on the tab; only the expiring-secure-link issuance card is gone, matching "the Funding Package tab will have the wire instructions."

**`#spec-test-commit-push-deploy`.**
- Spec/test: added source-assertion coverage in `tests/wire-instruction-links.test.ts` (secure-link card is gated; Funding Package tab passes `show_secure_link={false}`). `bun run check` green — 1617 pass / 0 fail, typecheck exit 0.
- Commit + push: `25c9f12` on `main`, pushed to `origin`.
- Deploy: `deploy:worker` (frontend-only, no migrations) → version `e1f2fe96`.
- Verify: live served admin bundle `index.browser-5NHK4VEO.js` (2.07 MB) carries `show_secure_link = true` (default), the `&& LoanWireLinks` gate, and `show_secure_link: false` from the tab. Public/admin routes 401, so verified by string-grep of the served bundle.
/goal In https://equityfundingsource.brian-takita.workers.dev/admin/pipeline?dialog=deal&dialog_deal_id=070770ae-0e0e-4962-9c63-1e08dfc8b0b4&deal_tab=documentst > Loan Package tab says 6/13. It should be 13/13 since all 13 of the documents are present. Use the same logic for the Lender Documents + Funding Package tabs.

All complete tabs should be green and incomplete tabs should be red. In the above link:
- Loan Documents should be green
- Lender Documents should be red
- Funding Package should be red
- Closing should be red

### Re: Loan Package "6/13" should be "13/13" + green/red tab completeness — opus-4-8

Fixed, deployed, and verified live.

**Root cause (confirmed against prod D1 for deal `070770ae` / loan `a1f2d74c`).** The deal has 13 document types in the catalogue but only **6 are required** for it — all 6 required types have their document uploaded; the other 7 are non-required (default-off or overridden off) and left empty. The badge denominator counted all 13 types and the numerator only types-with-a-doc, so a *complete* deal read "6/13". The Loan Package is genuinely complete.

**Fix (`#deal-tab-complete-ratio`).** Introduced a **satisfied** count: a category is satisfied when it has a document **or** it is not required for this deal. So this deal reads **13/13** (6 with docs + 7 not-required). A missing *required* doc drops it below full and flags incomplete.

- Badge is now the completeness ratio `"<satisfied>/<total>"` (same shape as the Closing tab), replacing `"<uploaded> · <with>/<total>"`.
- The count pill renders **green** when `satisfied === total`, **red** otherwise. Applied to all four tabs — Loan Package, Lender Documents, Funding Package, and Closing (colored from its step count).
- Same satisfied logic for Lender Documents (all 5 sections needed) and Funding Package (all loan types must be flagged). Summary line now reads "<n> documents uploaded · <x> of <y> requirements met".

**Live states match your spec** (queried prod): Loan Package **13/13 green**; Lender Documents **0/5 red** (no lender docs uploaded); Funding Package **red** (2 of 6 loan types flagged into the package); Closing **red** (deal still `published`, not closed).

**Verification.** `bun run check` green — 1620 pass / 0 fail, typecheck+build exit 0 (added `#deal-tab-complete-ratio` pure-helper + UI-wiring tests). Committed `2dfa985`, pushed to `origin/main`, deployed worker version `0eed9586`. Live bundle `index.browser-6TJ3TG6I.js` carries `satisfied_categories`, the ratio badge, and the green/red pill colors (verified by string-grep; admin routes 401).

One interpretation note: I took "13/13" literally — the denominator stays the full 13-type catalogue and non-required types count as satisfied. If you'd rather the ratio only count *required* types (so it'd read 6/6), say the word and I'll switch it.
In https://equityfundingsource.brian-takita.workers.dev/lenders/deals/070770ae-0e0e-4962-9c63-1e08dfc8b0b4?tab=lender-documents, please remove the re-render jank after a file is uploaded.

### Re: Remove re-render jank after uploading a file on the lender Lender Documents tab — opus-4-8

Fixed, deployed, and verified live.

**Root cause.** The shared `DealLenderDocuments` component (used by both the admin and the lender-facing `/lenders/deals/:id?tab=lender-documents` tab) had `upload_` and `remove_` call `load_()` on success. `load_()` sets `loading = true`, which swaps the entire tab body for a "Loading lender documents…" skeleton and **remounts every section row** — so each upload flashed the whole list. That refetch was unnecessary: the upload endpoint already returns the created document.

**Fix (`#lender-docs-upload-jank`).** Update the list in place instead of a full reload:
- Upload → `set_docs(prev => [...prev.filter(d => d.id !== uploaded.id), uploaded])` using the returned row.
- Delete → `set_docs(prev => prev.filter(d => d.id !== id))`.

The initial-mount load still shows the skeleton (correct); only the post-mutation flash is gone. The row now updates in place with no remount. The lender tab passes no `on_change`, so there's no parent-side reload either.

**Verification.** `bun run check` green — 1621 pass / 0 fail, typecheck+build exit 0 (added a source-assertion test that neither `upload_` nor `remove_` calls `load_()`). Committed `9b1e397`, pushed to `origin/main`, deployed worker `08545067`. Live served bundle `index.browser-X5TUB275.js` md5 matches the locally-built asset exactly, so the fix is live.

In https://equityfundingsource.brian-takita.workers.dev/lenders/deals/070770ae-0e0e-4962-9c63-1e08dfc8b0b4?tab=lender-documents, provide a 1 sentence description of each upload item for the lender.
#spec-test-commit-push-deploy

### Re: 1-sentence description of each lender Lender Documents upload item — opus-4-8

Done, deployed, and verified live.

The five built-in Lender Documents rows already carried a `hint`, but it was worded from EFS's internal perspective and only rendered while a row was empty. Rewrote each into a clear one-sentence description addressed to the lender, and made it stay visible whether or not a file is uploaded (`#lender-docs-item-descriptions`):

- **Term sheet / commitment letter** — "Your signed term sheet or commitment letter stating the rate, amount, and terms you are offering for this deal."
- **Deal-specific proof of funds** — "Recent evidence — such as a bank or brokerage statement — that the capital to fund this specific deal is available."
- **Underwriting conditions** — "The conditions your underwriting requires to be satisfied before or at closing."
- **Counter-signed items** — "Any deal documents you have signed and are returning, such as a counter-signed agreement or addendum."
- **Funding authorization** — "Your written authorization to release and wire the funds for this deal."

The shared `DealLenderDocuments` core also backs the admin "on behalf" view, so the clearer descriptions show there too.

**`#spec-test-commit-push-deploy`.** `bun run check` green — 1622 pass / 0 fail, typecheck+build exit 0 (added a source-assertion test pinning the five sentences and the always-visible render). Committed `60555ce`, pushed to `origin/main`, deployed worker `35bbfeda`. Live asset `index.browser-2XLDMV6S.js` md5 matches the locally-built bundle and contains the descriptions.
In https://equityfundingsource.brian-takita.workers.dev/admin/pipeline?dialog=deal&dialog_deal_id=070770ae-0e0e-4962-9c63-1e08dfc8b0b4&deal_tab=lender-documents, the UI should update via when the lender uploads documents. There should also be alerts. Please ensure reliable realtime behavior with lender uploads + state changes so the admin can see the changes without having to reload the page. Do we need extra infrastructure or architecture to ensure reliable delivery?

### Re: Reliable realtime for lender uploads + state changes on the admin deal modal — opus-4-8

**Do we need extra infrastructure? No.** EFS already has the full reliable-realtime stack; the gap was just that lender document uploads weren't wired into it and the deal modal wasn't subscribed. Both are now fixed, deployed, and live.

**The existing stack (reused, `#7djh` / `#admin-alerts`):**
- `AdminEventsRoom` — a Cloudflare **Durable Object** that holds admin WebSocket connections and fans out events. This is the coordination layer a stateless Worker needs to push cross-session (a lender in one session → an admin in another).
- `use_admin_events_` — the client hook: WebSocket push for immediacy, **auto-reconnect**, and a **polling heartbeat as the reliable-delivery fallback** (60s while the socket is live, 20s when it's down) so a missed push still reconciles.
- `#admin-alerts` feed — persists a counted alert row + pushes a desktop notification.

**What I wired in (`#lender-docs-realtime`):**
- Lender upload → `admin_alert_emit_('lender_documents_uploaded', deal_id, actor)` — persists a counted alert deep-linked to the deal's Lender Documents tab, and pushes it. Lender delete → a refresh-only event.
- `AdminDealDetail` now subscribes via `use_admin_events_` and **quietly re-lists** the Lender Documents tab (+ refreshes the doc badges) via a `reload_signal` — no page reload, no loading-skeleton flash.

**Reliability model.** Push = instant; if a push is dropped (socket blip, DO restart), the 20–60s polling heartbeat reconciles, and the persisted alert row survives regardless. Worst case the admin sees the change within ~60s; normally it's immediate. That's sufficient for "admin sees lender uploads without reloading."

**If you ever need stronger (zero-miss, ordered) guarantees** — e.g. an audit-grade activity stream — the one addition would be per-connection sequence numbers + replay-on-reconnect inside the DO (it's currently a stateless in-memory fan-out). Not needed for this use case, and I did not add it. The single shared DO instance is also fine at EFS's admin headcount; it'd only need sharding at much larger scale.

**Shipped.** `bun run check` green — 1624 pass / 0 fail, typecheck+build exit 0 (added backend-emit + frontend-subscription source-assertion tests; updated the alert-feed set test). Committed `e5bc945`, pushed to `origin/main`, deployed worker `26ddb96d`. Verified live: worker bundle carries the `lender_documents_*` events; browser bundle md5 matches the built asset.
<!-- agent:boundary:73e5dd5f -->
<!-- /agent:exchange -->

<!-- agent:queue preset="#spec-test-commit-push" -->
<!-- /agent:queue -->

## Notes

<!-- agent:notes -->
- Admin loan delete: /api/admin/loans/:id
- Admin bulk loan delete: /api/admin/loans/bulk-delete
- Admin TMO loan delete: /api/admin/tmo-loans/:loan_number
- Admin TMO bulk delete: /api/admin/tmo-loans/bulk-delete
- Borrower self-delete: /api/borrower/loans/:id
- Admin user delete: /api/admin/users/:id, because loans.user_id cascades from users, then investment_deals.loan_id
  cascades from loans
---
Is there an admin for the borrower portal messages? Can we have an email notification whenever a borrow sends a message? Can we support an email response back into the chat? Can the borrower receive emails whenever an admin responds with a message?
---
After 20:30
- login to TMO. Make sure we are using the correct credentials.
- TMO Syncs
<!-- /agent:notes -->

## Backlog

<!-- agent:backlog priority queue -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#mam7] Fractionalized loans (future): support up to ~10 lenders/loan with siloed per-lender info in the shared loan view. [followups: Fractionalized loans]
- [/] [#jzqj] Bidirectional TMO conditions sync (TMO as source of truth for conditions) + LOS lender-ID / deep-link into TMO pages. (gated on the Tue TMO screen-share + API access) [followups: TMO]
- [/] [#1067] Accreditation/suitability questionnaire: full form in the lender portal, income/net-worth options only, not a hard funding gate, admin-fillable, editable PDF + investigate DocuSign. Needs the DRE form + a DocuSign account. [followups: Accreditation]
- [/] [#nyc5] [P0] Available-deals upload: API endpoint + API key to upload investor offers to the marketplace (replaces manual YAMM) + a manual add-deal form (same template, admin-created). Needs Brian's offer-format spec. [followups: Available-deals]
- [/] [#tcb0] [P0] Go-live: publish the EFS site this week — disable/hide not-yet-functional portals, keep apply-for-loan + contact inquiries working; verify apply->TMO end-to-end. (needs operator go on which portals to hide) [followups: Go-live]
- [/] [#04v8] [recommended] Validate TMO cross-service lender visibility in a real TMO sandbox/tenant before relying on automated LOS funding writes: create or link a lender through `LSS.svc`, confirm `GetLender`, then attempt `LOS.svc` `AddLoanFunding`/`UpdateLoanFunding` on a test loan or get written TMO support confirmation that LOS funding resolves the LSS lender/vendor master `Account`.
- [/] [#apply-invite-fail-ux] Public `/apply` invite-failure UX: when the borrower invite email fails or is skipped after lead persistence, stop telling the borrower to check their inbox; show a received/follow-up message, keep the failed invite status visible to admins, and add a regression test.
- [/] [#email-bg-outbox] Durable email delivery observability/retry: record lifecycle, secure-message, broker, lending-group, investor-interest, login-link, invite, reminder, and feedback email results in D1 with status/error/retry/dead-letter instead of relying on console-only warnings for background notifications.
- [/] [#email-prod-ready] Email production-readiness hardening: provision a dedicated `CLOUDFLARE_EMAIL_API_TOKEN` secret with Email Sending scope, verify `BROKER_EMAIL_NOTIFICATIONS_ENABLED` if broker email is expected, add an admin/CI diagnostic that fails when production delivery would fall back to local capture, and document that `MESSAGE_EMAIL_INBOUND_SECRET` is only needed for the HTTP parsed-inbound route.
- [/] [#mcme] [recommended] #msg-read-status Email-open-based read receipts — DEFERRED: Cloudflare Email Sending has no open-tracking webhook, and transactional mail should stay on Cloudflare Email Sending. If email-open receipts become mandatory, design a Cloudflare-compatible tracking-pixel/event subsystem or accept portal-only read cursors. In-app portal read cursors already exist for borrower chat (`chat_sessions.*_last_read_at`) but are not email reads.
- [/] [#msg-unify4] [gated: awaiting dual-write prod soak] Lender cutover — dual-write (best-effort mirror into messages, both lender sources) + backfill (migration 0098) SHIPPED (EFS b5a6a55). Remaining: after soak, switch the inbox to read lender data from messages (dual-read), then drop deal_interest_lender_messages + lender_account_messages. Plan: src/equityfundingsource-dev/prds/message-unification.md.
- [/] [#msg-unify3] [gated: awaiting dual-write prod soak] Borrower cutover — dual-write (best-effort mirror into messages) + backfill (migration 0097) SHIPPED (EFS 411a4da). Remaining: after the dual-write soaks live, switch the unified inbox to read borrower data from messages (dual-read), then drop chat_messages. Plan: src/equityfundingsource-dev/prds/message-unification.md.
- [/] [#lender-card-msg-email] [recommended] Email-to-lender parity for #lender-card-msg account messages (mirror notify_lender_deal_message_): notify the lender by email when an admin posts an account message, like the deal-interest thread does. Rides the Cloudflare Email Sending / verified EFS sender gate (#f4rq). Plan: src/equityfundingsource-dev/prds/lender-card-messaging.md.
- [/] [#proprate] Lender proposal persists a DEFAULT interest rate never finalized. investor/deal_detail.tsx:362 pre-fills proposal_rate from deal.monthly_interest when deal.proposal_interest_rate==null; submit (:425-435) sends it whenever an amount is present; :421 requires a finite rate, so a lender cannot commit an amount with rate TBD — the pre-filled default (6.5% on d6b78994) persists as if finalized. Duration NOT affected (no proposal_term_months column). DATA CLEANUP DONE 2026-06-20: nulled proposal_interest_rate on the d6b78994/b8ccaa76 row (kept proposal_amount=100000 + proposal_submitted_at; D1 changes=1). CODE FIX still pending operator go: do NOT pre-fill the rate with the deal default; allow proposal_interest_rate=null ("amount committed, rate TBD"); relax :421 validation + admin LoanTermProposalSummary display.
- [/] [#nthc] [operator-verify] DEPLOYED 2026-06-20 (worker version 17b06176-fc91-44b6-b35d-1cd61beed01b; EFS 8889a4c). Remaining = operator live-verify only: open the Lender Bros group Edit overlay on deal d6b78994, bump lender2 from 0%, confirm the split saves and the "60% confirmed (partial)" badge reconciles.
- [/] [#pmhf] [recommended] Reuse the rebalancing share editor (lib/ui--browser--efs/portal/shared/lending_groups_alloc.ts: rebalance_stakes_/distribute_evenly_/lock pins) in the per-deal split UI (deal_detail allocations) so per-deal % editing gets the same 100%-balanced, lockable inputs.
- [/] [#dealtab500] [operator-verify] Root-cause the deal 7a0c1463 / loan 45ddae53 (GQI751) loan-tab "Internal Error". Static trace independently re-confirmed exhausted: GET /admin/pipeline serves only a shell (no server-side deal processing); every client-fetched endpoint is guarded — admin_loan_R_ is a plain SELECT, admin_deal_R_ LTV/onboarding chain (onboarding_json_/loan_ltv_calc_) is fully try/catch + null guarded, /documents is harmless; all client fetches degrade to error cards, not the worker Internal Error page. Regression guard shipped (efs a39be3d) locking the thin-TMO-loan onboarding shape against throws (gformula deal-economics helpers). REMAINING (operator-gated, cannot be agent-done): reproduce live via `bun run cf:worker:tail` + a browser click on the loan tab to capture the real throwing endpoint + stack, OR inspect loan 45ddae53/deal 7a0c1463 D1 rows once the #cftk token lands. Relates to #5q7z/#214f.
- [/] [#5q7z] [recommended] 'Accept and send to TMO' duplicate-loan bug: for a loan already associated with a TMO record (deal 7a0c1463 / loan 45ddae53 has tmo_loan_number=GQI751, tmo_loan_rec_id=3840D27B..., tmo_loan_application_rec_id=cda67748), the approve flow (admin/applications/index.ts application_tmo_writeback_attempt_ -> tmo_writeback.ts:308 LOS.svc/NewLoan) sends NewLoan with the existing GQI751 -> TMO 400 'Duplicate loan number. Unable to save record.' Fix (2 parts): (1) approve/writeback must detect an existing TMO association (tmo_loan_rec_id / writeback_status='sent') and associate+mark accepted locally instead of calling NewLoan (or call an UpdateLoan endpoint); (2) hide/disable the 'Accept and send to TMO' button in lib/ui--browser--efs/portal/admin/full_applications.tsx:424 when the app/loan is already in TMO. Design Q: on accept of an already-in-TMO loan, associate-only (no TMO call) or also push UpdateLoan? Relates to #dzmm/#37ah/#214f.
- [/] [#214f] [recommended] Gate investor deal publish on acceptance: admin_deal_publish_ (app/equityfundingsource/lib/admin/repo.ts:2054) + POST /api/admin/loans/:id/deal publish a loan to the investor marketplace with NO acceptance check (only 'if (!loan) throw'); any non-Funded loan becomes status='published'. So a deal can be an Available Deal while its application is not accepted (observed: investment_deal 7a0c1463 published from loan 45ddae53 status='submitted', tmo 'Application Received', with NO full_application row). Decide the business rule ('accepted before available') and add the gate before status='published' (and/or surface why a loan is ineligible to publish). Relates to the full_applications<->loans<->investment_deals decoupling.
- [/] [#5eq8] Closing docs step (redesign): rebuild the closing-documents/notarization flow as its own step (replaces the removed Final Loan Documents section). Closing docs are wet-ink signed AND notarized on PAPER (not e-signed); EFS records a signed-status flag (+ final_docs_* D1 columns preserved) and that signed status is a BI-DIRECTIONAL sync with TMO (EFS <-> TMO), not an in-app e-signature flow. Re-introduces a notarize control and SHOULD re-gate allocation wire status (Sent/Confirmed) on notarization — the UI gate + server 409 were removed in the 2026-06-18 AvailableDeal restructure so funding is not hard-blocked in the interim.
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
- [/] [#next-steps] Remaining EFS workers.dev production state: D1 API token unblocked (Cloudflare API token at `passage btak/d1/CLOUDFLARE_API_TOKEN`); migrations 0025/0026 are already applied; Cloudflare Access env vars are deployed for /login/access-callback on workers.dev using the one-time PIN IdP. Custom domain cutover remains intentionally deferred; true Google Workspace SSO remains gated on Google admin credentials.
- [/] [#dzmm] TMO webhooks UNAVAILABLE per operator (2026-06-13). Read-side tightened polling is shipped (scheduled tmo-sync with configurable interval + due-gating + tmo_cache read mirror). Remaining gated: EFS to TMO writeback (tmo_cache_outbox replay) needs the #7a4d write field map + #k2xg support answers before it can replay.
- [/] [#dropbox-sync-adapter] Dropbox sync adapter: #dropbox-sync-contract shipped the shared documents Dropbox metadata, document_sync_logs idempotency schema, pure sync planner, and mocked adapter tests. Remaining: after Dropbox access is available, wire the real credential-backed worker to push/pull R2_DOCUMENTS with retries, backfill, tombstone replay, and per-document sync logs.
<!-- /agent:review -->

## Completed / Reaped

<!-- agent:icebox -->
- [ ] [#vcyr] Send rwaldman100+admin@gmail.com admin login link to btak.dev+admin@gmail.com once a working Cloudflare Email Sending API token is available or Brian explicitly confirms an alternate sender/body. Verified Robert is active admin; current local Cloudflare/Resend credentials cannot send. (Deferred from backlog 2026-07-03; operator replaced the line — restore if unintended.)
- [ ] [#cutover-checklist] [recommended] Write (do not execute) the workers.dev to equityfundingsource.com custom-domain + Google Workspace SSO cutover checklist doc: routing for /, /funded-deals, admin routes; /login/access-callback; rollback steps. Execution stays gated on #m6ch creds. Advances #efs-cutover-sso-plan/#next-steps.
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
