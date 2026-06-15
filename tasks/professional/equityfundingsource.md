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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260615-022613.md`*

Compacted content:
- Archived 6 response topic(s): Published deal tag + Published deals column (portal-admin/pipeline); Available deal dialog on the investor dashboard; Lender Account Request gates Available deals; 3 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f...

### Re: Dashboard application notice needs a button to the form — opus-4-8

> **Queue prompt:**
>
> "Lender account application — incomplete" in https://equityfundingsource.brian-takita.workers.dev/portal-investor/dashboard should have a button to link to the form tho complete the application. deploy


> **Queue prompt:**
>
> "Lender account application — incomplete" in https://equityfundingsource.brian-takita.workers.dev/portal-investor/dashboard should have a button to link to the form to complete the application. deploy

Done — shipped, tested, committed, pushed, and deployed.

**What changed:** the shared `LenderApplicationNotice` now takes an optional `complete_href`. When provided it renders a status-aware CTA button — **Complete application** (not started), **Update application** (denied), or **View application** (pending) — linking to the target. The investor **dashboard** passes `complete_href="/portal-investor/available-deals"` on both notice placements (deals present and the no-deals case), so the "Lender account application — incomplete" banner now has a button to the page where selecting a deal surfaces the application form. The notice on the Available deals page and deal detail stays button-less since the form is already on the page (`#deal-first-apply-cta`).

**Verification:** `bun run check` green — **692 tests pass, 0 fail**, browser + worker builds clean. Extended `tests/lender-account-gate.test.ts` (asserts the `complete_href`/`investor-lender-application-cta` button and the dashboard wiring) and updated `docs/current_progress.md`.

**Heads-up — remote had diverged:** `origin/main` had been force-updated with upstream work (#rpzm removable interest-icon toggle, admin application-status history) and my prior-cycle `9a54501` was no longer on it. I rebased my deal-first-apply + CTA commits onto the new `origin/main`, re-ran the full suite green (the interest-toggle merged cleanly into `available_deals.tsx`), and pushed — so deal-first-apply is back on `origin/main`.

**Shipped:**
- `equityfundingsource-dev` `9f16e7c` → pushed to `origin/main` (rebased).
- Superproject pointer bump `db0d83a1a`.
- Deployed to Cloudflare: `https://equityfundingsource.brian-takita.workers.dev` (Version `8bd4dabe`; no new D1 migrations).

### Re: Unify the "Lender account application — incomplete" notice + give every one a complete button — opus-4-8

> **Queue prompt:**
>
> All "Lender account application — incomplete" components across all pages should be the same component...and have a button to complete the application. deploy

Done — unified, spec + tests updated, committed, pushed, and deployed.

**Already shared, now uniform.** The banner was already one component (`LenderApplicationNotice`) on all four placements, but only the dashboard rendered a button. It now **always** renders the status-aware CTA button (**Complete** / **Update** / **View application**) everywhere:

- **Dashboard** (form not on page) → button links to `/portal-investor/available-deals`, where selecting a deal surfaces the form. (Unchanged.)
- **Available deals** (featured deal) and **Deal detail** → the application form (`LenderAccountGate`) is rendered inline, so the button now **scrolls to it** via a new `#investor-lender-application-form` anchor (added `scroll-margin-top` so it lands cleanly below the topbar).
- **Available deals empty state** (no published deals) → previously had a button-less notice and no form at all. It now surfaces the application form inline too, so the button always has a real target.

**Verification:** `bun run check` green — **692 tests pass, 0 fail**, browser + worker builds clean. Extended `tests/lender-account-gate.test.ts` to assert the shared form anchor/href, that every notice carries `complete_href`, and that no placement renders button-less. Updated `docs/current_progress.md`.

**Shipped:**
- `equityfundingsource-dev` `7ad8695` → pushed to `origin/main` (rebased onto latest).
- Superproject pointer bump `5a6d3f1fd`.
- Deployed to Cloudflare: `https://equityfundingsource.brian-takita.workers.dev` (Version `5d08d919`; no new D1 migrations).
- Superproject pointer bump `5a6d3f1fd`.
- `equityfundingsource-dev` `7ad8695` → pushed to `origin/main` (rebased onto latest).
- **Available deals empty state** (no published deals) → previously had a button-less notice and no form at all. It now surfaces the application form inline too, so the button always has a real target.
- **Available deals** (featured deal) and **Deal detail** → the application form (`LenderAccountGate`) is rendered inline, so the button now **scrolls to it** via a new `#investor-lender-application-form` anchor (added `scroll-margin-top` so it lands cleanly below the topbar).
- **Dashboard** (form not on page) → button links to `/portal-investor/available-deals`, where selecting a deal surfaces the form. (Unchanged.)

### Re: Remove "Lender account application — incomplete" notice from the Available deal dialog + deal page — opus-4-8

> **Queue prompt:**
>
> Remove "Lender account application — incomplete" component from the Available deal dialog in https://equityfundingsource.brian-takita.workers.dev/portal-investor/dashboard and in https://equityfundingsource.brian-takita.workers.dev/portal-investor/deals/7a0c1463-27bb-4d7c-82f7-2281b3c008d3. deploy


Done and deployed.

The "Lender account application — incomplete" banner is the `LenderApplicationNotice` component. Both targets — the dashboard **Available-deal dialog** and the standalone **`/portal-investor/deals/<id>`** page — render the same `InvestorDealDetail` (the dialog embeds it), so the notice lived in one place: `deal_detail.tsx`. I removed it from the no-lender-account branch there; the `LenderAccountGate` below it already carries its own incomplete / awaiting-review status copy ("Complete your lender account application" + explanatory text), so the separate banner was redundant.

**Changes** (`src/equityfundingsource-dev`, committed `5d8ba77`, pushed to origin/main):
- `lib/ui--browser--efs/portal/investor/deal_detail.tsx` — dropped `<LenderApplicationNotice …>` and its now-unused imports (`LenderApplicationNotice`, `LENDER_APPLICATION_FORM_HREF`).
- `tests/lender-account-gate.test.ts` — updated the deal-detail assertions to expect the notice is absent while the gate stays.

The dashboard's own Available-deals **card list** still keeps its notice (that's not the dialog, so it's out of scope for this request).

**Verification:** full suite green (692 pass / 0 fail), `build` + `build:worker` clean. Deployed to Cloudflare twice — once for the change, then rebuilt + redeployed from current `main` after a rebase pulled in concurrent `#deal-first-apply-cta` work on the same files, so the live worker (version `852a23bb`) matches HEAD.

### Re: Client-side model for the lender-application UI transitions — opus-4-8

> **Queue prompt:**
>
> :pushpin: When I filled out the Lender Application in the Available deal in https://equityfundingsource.brian-takita.workers.dev/portal-investor/dashboard: The UI should update. Complete application notification should transition into the waiting for approval notification. Also the Available deal that was added card state should change. Do we have a client side model of these sort of transitions?


Short answer: **partly.** There is a real client-side model of these transitions, but it is **per-component, not shared** — and on the **dashboard** (where you hit this) the dialog's copy of the state never reaches the dashboard's notice or cards, so they don't update until a full reload.

**What the model is.** The state object is `InvestorLenderProfile` (`lender_account.tsx`), and the transition is driven entirely by its `lender_request_status: 'pending' | 'done' | 'denied' | null` field plus `lender_account` (the TMO account number). Pure functions derive the UI from it:
- `LenderApplicationNotice` heading: `null` → "incomplete", `'pending'` → "awaiting EFS review", `'denied'` → "denied".
- `lender_account_ready_(lender)` = `Boolean(lender.lender_account)` — gates funding (stays `false` while only the *application* is pending; the account number only lands after EFS approves in TMO).
- On submit, `LenderAccountGate`'s `on_change` is called with `{...lender, lender_request_status: 'pending'}` — an **optimistic** local update, no refetch.

So the transition you describe ("Complete application" → "waiting for approval") is modeled and reactive **— on the standalone `/portal-investor/available-deals` page.** There, `AvailableDeals` holds **one** `lender` state and passes it to *both* the notice (`lender={lender}`) and the gate (`on_change={set_lender}`), so submitting flips the notice immediately.

**Why the dashboard doesn't update.** There is **no shared store** (no context/signal) — three components each keep their own `useState` copy of the profile, each loaded once from `/api/investor/deals`:

| Component | State | Updated on submit? |
|---|---|---|
| `available_deals.tsx · AvailableDeals` | `lender` | ✅ notice + gate share it |
| `dashboard.tsx · AvailableDealsPreview` | `lender` (notice + deal cards) | ❌ never updated after mount |
| `deal_detail.tsx · InvestorDealDetail` (the dialog) | `investor`, `on_change={set_investor}` | ✅ but only its **own** copy |

On the dashboard the deal dialog renders `InvestorDealDetail` (embedded). When you submit there, the dialog's `investor` flips to `pending` and its gate switches to the "request pending" panel — but nothing lifts that change back up to `AvailableDealsPreview`, and **closing the dialog doesn't refetch**. So the dashboard's outer `LenderApplicationNotice` and the deal cards keep their stale state.

**On "the deal card state should change."** The lender application is **account-level**, not per-deal. The only per-deal card states today are `interested` and `investor_fund_status` (`pending`/`active`) — there's no "applied" state tied to a card. So even with the notice fixed, no card changes from applying unless we *add* a new card-level indicator (a UX decision, not just a wiring fix).

**The fix** (cheapest first):
1. **Lift state up** — `InvestorDealDetail` takes an optional `on_lender_change`; the dialog threads it; `AvailableDealsPreview` passes `set_lender`. The dialog's `on_change` then updates both copies. Smallest change; fixes the notice.
2. **Refetch on dialog close** — re-run the `/api/investor/deals` fetch when the dialog closes. Simpler but a round-trip + flash.
3. **Shared store** (preact signal/context for one `InvestorLenderProfile`) — most robust, fixes every future divergence; biggest change.

I recommend **Option 1** for the immediate notice transition (Option 3 later if more cross-component lender-derived state appears). I did **not** implement it this turn since the prompt was a "do we have…?" question and the card-state piece needs a UX decision — queued as actionable backlog item `#9e7b` with a plan at `tasks/professional/efs-investor-lender-state-lift.md`. Say `do #9e7b` and I'll wire Option 1, update tests, and ship it.

<!-- no-queue-removal-guard -->

### Re: do #9e7b — lift lender-application state across the dashboard ↔ deal-dialog boundary — opus-4-8

Done. Implemented **Option 1 (lift state up)** from `tasks/professional/efs-investor-lender-state-lift.md`.

**What changed (EFS submodule `70195ec`, superproject pointer `af640fcdb`):**

- `deal_detail.tsx::InvestorDealDetail` now accepts an optional `on_lender_change?: (next: InvestorLenderProfile) => void`. The inline `LenderAccountGate`'s `on_change` calls both `set_investor(next)` (its own copy) **and** `on_lender_change?.(next)` (the lift), so a parent holding its own lender copy reacts immediately.
- `dashboard.tsx`: `AvailableDealsPreview` passes its own `set_lender` → `AvailableDealDialog` (new `on_lender_change` prop) → `InvestorDealDetail`. Submitting the application in the in-dashboard Available-deal dialog now flips `AvailableDealsPreview`'s `lender` to `pending`, so the dashboard's outer `LenderApplicationNotice` transitions **incomplete → "awaiting EFS review" without a page reload**.

Cards still don't change — as the plan notes, there is no per-deal card-level lender state; the notice is the thing that transitions (account-global `lender_request_status`).

**Tests:** updated the source-assertion tests in `lender-account-gate.test.ts` and `investor-portal-e2e.test.ts` to match the new wiring, plus added two new assertions covering the lifted callback threading. Full suite green: **694 pass / 0 fail**; `bun run build` and `bun run build:worker` both clean.
<!-- agent:boundary:6d46acfc -->
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
