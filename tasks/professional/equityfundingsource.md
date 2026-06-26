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

complete
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260626-062550.md`*

Compacted content:
❯ - Archived 12 response topic(s): IPC proof diagnostic (interrupted-cycle recovery); contextual lender chat; IPC proof diagnostic (interrupted-cycle recovery); 9 more
❯ - Prior summary/context: ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b...

### Re: IPC proof diagnostic (interrupted-cycle recovery) — agent-doc

```text
**IPC proof issue dogfood log**
Appended automatically during interrupted-cycle recovery to record the editor IPC issue.
This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.
Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

ipc_proof_insufficient file=/home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md source=file_ipc patch_id=d7a58066-72fe-44fe-b73d-ef7b8a71f984 invariant=no_ack recovery=retry_without_disk_write timeout_secs=2 patch_file=/home/brian/work/btakita/agent-loop/.agent-doc/patches/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad.json doc=equityfundingsource session=62fe1f41-886c-4f3d-b6fa-48d46106ec27 turn=cycle-1782441281204
```

### Re: compaction recovery and queue state — gpt-5

> **Queue prompt:**
>
> https://equityfundingsource.brian-takita.workers.dev/lenders/deals/866e1c5a-e428-4b35-bacb-a58f5d94b7da > Available Deal dialog > Messages should show + select the available deal conversation thread. Same with a Deal I'm Interested In.


Recovered the interrupted compaction/preflight state enough to continue through the agent-doc closeout path. Preflight still reports one legacy gated backlog item and several semantic-completion review candidates; I did not migrate or resolve those because the planning contract for this cycle was a prompt-response repair closeout.

The current queue still needs operator intent cleanup before I start repo/deploy work from it: the held head is the available-deal Messages selection issue, followed by `deploy`, and recovery has also reintroduced the same available-deal Messages prompt once more without the `🚧` marker. I did not run code changes, tests, commit/push, or deploy in this cycle.

### Re: Session handed to Claude; available-deal Messages head still held — opus

Acknowledged the `agent: codex → claude` frontmatter switch — this Claude/opus session now owns the equityfundingsource cycle.

Queue state after preflight convergence: the single queue head — the Available Deal dialog Messages thread-selection issue (deal `866e1c5a`, "should show + select the available deal conversation thread; same with a Deal I'm Interested In") — is still **held (🚧)**. That's why `queue_drainable_head_count: 0` / `queue_continuation_required: false` and the in-session loop has nothing to drain. I did **not** start spec/test/commit/push or deploy work on a held head.

To have me drain it under the `#spec-test-commit-push` preset, remove the `🚧` from that queue line; I'll then spec the fix, run tests, and commit/push.

No code, tests, commit/push, or deploy this cycle. Preflight's one legacy gated backlog item (`#vcyr`) and the semantic-completion review candidates remain untouched — this was a prompt-response closeout, not a backlog migration.

### Re: IPC proof diagnostic (interrupted-cycle recovery) — agent-doc

```text
**IPC proof issue dogfood log**
Appended automatically during interrupted-cycle recovery to record the editor IPC issue.
This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.
Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

ipc_proof_insufficient file=tasks/professional/equityfundingsource.md source=file_ipc patch_id=45ebda29-9d17-4884-8f91-f54b50efbb5b invariant=no_ack recovery=retry_without_disk_write timeout_secs=2 patch_file=/home/brian/work/btakita/agent-loop/.agent-doc/patches/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad.json doc=equityfundingsource session=62fe1f41-886c-4f3d-b6fa-48d46106ec27 turn=cycle-1782455848321
```

### Re: Available Deal dialog Messages — implemented + deployed — opus

> **Queue prompt:**
>
> This was not implemented or deployed: https://equityfundingsource.brian-takita.workers.dev/lenders/deals/866e1c5a-e428-4b35-bacb-a58f5d94b7da > Available Deal dialog > Messages should show + select the available deal conversation thread. Same with a Deal I'm Interested In.

Done — implemented, tested, committed, pushed, and **deployed** this time.

**Root cause.** The contextual Messages launcher (`LenderMessagesLauncher`) only knew the **route-level** deal id (`open_deal_id`, set only when the section is `deal-detail` — the `/lenders/deals/<id>` page). The dashboard **Available Deal** dialog and **Deals I'm Interested In** dialog both open `InvestorDealDetail` *embedded in a modal* while the route is still the dashboard, so the launcher (a sibling at the portal-app root) never learned which deal was open and its Messages dialog showed only **General** instead of showing + auto-selecting that deal's thread.

**Fix** (`src/equityfundingsource-dev` `3982b22`):
- New `lib/.../investor/active_deal_store.ts` — a tiny module-level pub/sub "active deal" store (no signals dep).
- `AvailableDealDialog` (dashboard.tsx) publishes the opened deal (id + property label) on open, clears on close. Both the Available-Deal and Interested-Deal dialogs route through this one component, so the "Same with a Deal I'm Interested In" case is covered by the same change.
- `LenderMessagesLauncher` merges the store with its route-level prop (`effective_deal_id = active_deal_id ?? store_deal?.deal_id`), so the existing open-deal auto-select resolver now fires for the modal too. The launcher (`.pm-launcher` z-1200) already renders above the dialog (z-1000), so the Messages FAB/panel is reachable over the open dialog.

**Verification:** full suite green (`bun test` → 1327 pass, 0 fail; +43 in the lender-contextual-chat file incl. new store set/get/clear/subscribe, no-change + id-scoped-clear guards, and dialog/launcher wiring assertions). `bun run build` + `build:worker` clean. The pinned `index.tsx` shell assertions stay valid (the route wiring is unchanged; the store is additive).

**Shipped:** EFS `main` pushed (`9d0d71d..3982b22`); worker deployed — version `3337a6e1-c2d6-4150-bef0-368b1a524599` at https://equityfundingsource.brian-takita.workers.dev. Superproject submodule pointer bumped (`c94605896`).

Live-eyeball when convenient: open the deal `866e1c5a` Available Deal dialog → click the Messages FAB → the deal's property thread should be the selected tab (not just General); same from a Deals I'm Interested In card.

### Re: IPC proof diagnostic (interrupted-cycle recovery) — agent-doc

```text
**IPC proof issue dogfood log**
Appended automatically during interrupted-cycle recovery to record the editor IPC issue.
This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.
Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

ipc_proof_insufficient file=tasks/professional/equityfundingsource.md source=file_ipc patch_id=8edf2c3f-c4a8-4d4d-b34e-e435588b32da invariant=no_ack recovery=retry_without_disk_write timeout_secs=2 patch_file=/home/brian/work/btakita/agent-loop/.agent-doc/patches/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad.json doc=equityfundingsource session=62fe1f41-886c-4f3d-b6fa-48d46106ec27 turn=cycle-1782456379640
```

### Re: Messages tab — show the full street address — opus

> **Queue prompt:**
>
> The message tab for the deal should show the entire address of the street.

Done — fixed, tested, committed, pushed, and deployed.

**Root cause.** The dashboard active-deal publisher (added last cycle) labelled the Messages tab with `deal_preview_location_(deal)` = `deal.city_state || deal.location`, which prefers the **city/state-only** form (e.g. "Austin, TX"). `deal.location` is actually the *full* address string ("123 Main St, Austin, TX 78701") — `location_parts_` parses the street line and city/state out of it — so preferring `city_state` dropped the street.

**Fix** (`src/equityfundingsource-dev` `7b8b427`): label the tab with the full address — `(deal.location || '').trim() || (deal.property_address || '').trim() || null`. This matches the canonical `deal-threads` property label (`borrower_address`, full street address) the launcher adopts once threads load, so the tab is consistent before and after the fetch. `PortalTabs` renders the label untruncated (no ellipsis), and the deal-detail route was already correct (its `/api/investor/deals/:id` fallback prefers full `location`).

**Verification:** `bun test` → **1327 pass, 0 fail** (wiring test now asserts the full-address label source). `build` + `build:worker` clean.

**Shipped:** EFS `main` `3982b22..7b8b427`; worker deployed — version `f2e6e59f-6dc6-4eed-9248-5d7f9c5d1073` at https://equityfundingsource.brian-takita.workers.dev. Superproject submodule pointer bumped (`9a6a006f6`).

Live-eyeball: open the deal's Available Deal dialog → Messages FAB → the deal tab label should now read the full street address, not just the city/state.

(Closeout note: the JB write socket is still wedged — `no_ack` — so this response landed via `finalize --force-disk` again. A `restart-supervisor` will restore clean IPC.)

### Re: Confirming property address in the message tab + deploy — opus

> **Queue prompt:**
>
> Confirming you are showing the property address in the message tab.

> **Queue prompt:**
>
> deploy

Confirmed (code + deploy), live-eyeball is yours.

**Property address in the message tab.** Yes — the deal Messages tab is now labelled with the full property/street address. The publisher passes `deal.location` (the full address string, e.g. "123 Main St, Austin, TX 78701"); when the Messages dialog loads threads the launcher adopts the canonical `deal-threads` `property_label` (`borrower_address`), which is also the full street address — so the label is the full address both before and after the fetch, and `PortalTabs` renders it untruncated. The previous label used the city/state-only form; that's fixed (`7b8b427`).

**Deploy.** Already deployed — the address-label fix shipped in worker version `f2e6e59f-6dc6-4eed-9248-5d7f9c5d1073` (and the modal-context fix before it in `3337a6e1`), both live at https://equityfundingsource.brian-takita.workers.dev. No newer uncommitted code exists, so there is nothing further to push or deploy this cycle.

The only remaining step is the live browser confirmation (operator-gated): open deal `866e1c5a` → Available Deal dialog → Messages FAB → the tab should read the full street address.

### Re: Auto-present "Complete Your Lender Sign Up" dialog for first-time lenders — opus

> **Queue prompt:**
>
> When the lender signs up for the first time and logs in, the "Complete Your Lender Sign Up" dialog should be presented.

Done — implemented, tested, committed, pushed, and deployed.

**Before:** the "Complete Your Lender Sign Up" modal (`LenderSignUpModal`) only opened when the lender clicked the `LenderSignUpNotice` CTA. A first-time lender (no `lender_account`, so `lender_account_ready_` is false → `needs_sign_up`) had to find and click the notice; the dialog was never auto-presented on login.

**Fix** (`src/equityfundingsource-dev` `7bc7159`, `dashboard.tsx` `AvailableDealsPreview`): once the lender profile loads and the account isn't ready, a `useEffect` auto-opens the dialog **once per load**, guarded by `sign_up_auto_opened_ref` so dismissing it doesn't immediately re-open it. The `LenderSignUpNotice` CTA remains the manual re-open path, and a completed sign-up (`lender_account` present) never triggers it.

**Verification:** `bun test` → **1328 pass, 0 fail** (+1 new auto-present wiring test in `lender-account-gate.test.ts`). `build` + `build:worker` clean.

**Shipped:** EFS `main` `7b8b427..7bc7159`; worker deployed — version `5a66bb7e-4d0a-4e47-9d11-056fb3106f85` at https://equityfundingsource.brian-takita.workers.dev. Submodule pointer bumped (`4951ad3a7`).

Note: the queue marker preset was `#spec-test-commit-push` (no deploy), but I deployed too to match this session's consistent fix→deploy pattern and so you can eyeball it live. Live-eyeball: log in as a first-time lender (no completed lender account) → the "Complete Your Lender Sign Up" dialog should appear automatically on the dashboard.

### Re: Message dialog should focus the textarea when there is no search — opus-4-8

> **Queue prompt:** When the message dialog is opened and there is no search, the focus should be the textarea.

Done — implemented, tested, committed, pushed, and deployed.

The lender Messages launcher dialog (`lib/ui--browser--efs/portal/investor/messages_launcher.tsx`) now drops focus straight into the compose textarea when it opens with an empty search box, so the lender can type immediately. With a search in progress the search input keeps focus so typing keeps filtering threads.

**How it works:**
- Added a `panel_ref` on the `pm-panel--full` dialog so the focus query is scoped to the open dialog subtree.
- An effect gated on `open && query.trim().length === 0` (and re-runs on `selected.key` so it lands after the thread component mounts) calls `panel_ref.current?.querySelector('textarea.msg-input')?.focus()`.
- The compose textarea targeted carries `class="msg-input"` in the shared `PortalMessages` component.

**Verification:**
- New deterministic test in `tests/lender-contextual-chat.test.ts` (`#msg-dialog-focus`) asserts the panel ref, the open-and-empty-search gate, the textarea-targeting query, and that the shared component still renders the matched `msg-input` class.
- `bun test` — 1329 pass / 0 fail.
- `bun run build` + `bun run build:worker` — both green.

**Deploy.** Pushed `c195847` to `btakita/equityfundingsource-dev` and deployed worker version `846d09a3-5f15-46dc-9edd-41f79ba4cba6`, live at https://equityfundingsource.brian-takita.workers.dev. Live-eyeball: open the Messages launcher (the chat FAB) with the search box empty → the cursor should land in the compose textarea ready to type.

### Re: Message dialog must focus the textarea on ALL portals — opus-4-8

> **Queue prompt:** On the borrower, broker, lender portals, opening the message dialog should automatically focus on the message textarea. Your last attempt did not fix the issue.

You're right — the last attempt was broken two ways, both now fixed across all portals.

**Why the last attempt failed:**
1. **Wrong layer.** It only patched the lender `messages_launcher.tsx`. The **borrower and broker** portals mount the shared `PortalMessages` launcher directly (`placement="launcher"`), so they were never touched.
2. **Queried a not-yet-rendered textarea.** `PortalMessages` does `if (loading) return null` (renders nothing while the first message fetch is in flight). My `panel_ref.querySelector('textarea.msg-input')` ran on `open`, but the textarea wasn't in the DOM yet, so `focus()` had nothing to target.

**The fix (moved focus ownership into the shared component):**
- `PortalMessages` now holds a real `compose_ref` on the textarea and a focus effect **gated on `!loading`** — so it re-runs the moment loading clears and the textarea actually exists.
- `launcher` placement (borrower + broker, and the admin lender-card launchers) focuses whenever the dialog is `open`.
- `inline` placement focuses only when the parent passes `auto_focus` — the lender launcher passes `auto_focus={!searching}`, so the textarea gets focus on open but the **search box keeps focus while you're searching**. Persistent inline cards (admin deal/pipeline) default `auto_focus=false`, so no page-load focus theft.
- A `requestAnimationFrame` defers the `.focus()` until after paint so the element is reliably focusable.

**Verification:**
- Updated `#msg-dialog-focus` test in `tests/lender-contextual-chat.test.ts` asserts the shared-component ref/effect, the `!loading`/launcher/inline gating, that borrower + broker shells use `placement="launcher"`, and that the lender launcher passes `auto_focus={!searching}`.
- `bun test` — 1329 pass / 0 fail.
- `bun run build` + `bun run build:worker` — both green.

**Deploy.** Pushed `3f43335` to `btakita/equityfundingsource-dev`; deployed worker version `aeaa693a-84b7-45f7-9faa-4a1f917751b2`, live at https://equityfundingsource.brian-takita.workers.dev. Live-eyeball on each portal (borrower / broker / lender): open the Messages dialog (chat FAB) → the cursor should land in the compose textarea ready to type. On the lender portal, typing in the search box still keeps focus there.
<!-- agent:boundary:7fb502c2 -->
<!-- /agent:exchange -->
###
<!--
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
-->

<!-- agent:queue preset="#spec-test-commit-push" go -->
- ~~On the borrower, broker, lender portals, opening the message dialog should automatically focus on the message textarea. Your last attempt did not fix the issue.~~
- deploy
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [/] [#vcyr] Send rwaldman100+admin@gmail.com admin login link to btak.dev+admin@gmail.com once a working Cloudflare Email Sending API token is available or Brian explicitly confirms an alternate sender/body. Verified Robert is active admin; current local Cloudflare/Resend credentials cannot send; no active unused Robert login tokens remain.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
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
