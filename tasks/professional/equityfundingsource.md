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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260718-155749.md`*

Compacted content:
- Archived 3 response topic(s): Remove the single TMO lender id; map vestings to TMO lenders; do #lender-account-drop-column, #tmo-lender-vesting-doc, #tmo-getlender-confirm; Launch without the TMO production key
- Prior summary/context: prior compacted content: Archived 4 response topic(s): Reconcile each lender with TMO from the deal interests tab; Pe...

### Re: Production Cloudflare environment — opus-4-8

Done. Production is live and isolated — worker `equityfundingsource-production`, version `d5b42115`, commit `1253f6b6`.

| | Staging (was the only env) | Production (new) |
|---|---|---|
| Worker | `equityfundingsource` | `equityfundingsource-production` |
| URL | `equityfundingsource.brian-takita.workers.dev` | `equityfundingsource-production.brian-takita.workers.dev` |
| D1 | `equityfundingsource` (33 users, 7 loans, 7 deals) | `equityfundingsource-production` (95 tables, 0 rows) |
| R2 | `equityfundingsource-{assets,documents,tmo-cache}` | same, `-production` suffixed |
| TMO | connected | not set — runs split |

Deploy with `bun run deploy:worker:production`; every `cf:` script has a `:production` twin. Staging deploys are unchanged.

I proved the isolation rather than asserting it: submitted the contact form against production, got a 303, confirmed the row landed in the production D1 and that staging's `contact_messages` was untouched, then deleted the test row.

**The trap I designed around.** Wrangler environments do *not* inherit top-level bindings — only `name`, `main`, `account_id`, and `compatibility_*`. Vars, assets, R2, D1, Durable Objects, rate limiters, triggers, and observability all have to be redeclared, and a binding you add to the top level and forget under `[env.production]` deploys cleanly and then throws on the first production request that touches it, which staging can never reproduce. `tests/efs-production-environment.test.ts` diffs the two environments and fails the suite the moment they drift.

Secrets are per-environment too, which is the same trap with worse consequences. Production had no `AUTH_SECRET` until I pushed it — a worker that cannot sign session cookies means nobody can log in. It was also missing `CLOUDFLARE_API_TOKEN`, which gates **all** outbound email, so contact forms, application leads, invites, and login links would have silently failed. Both are set now, and I verified the contact submission end to end after pushing them.

**A correction to what I told you last cycle.** I said TMO was definitively unset in production. That was wrong, and I should have checked rather than inferred it from the absence of a TMO reference in the secrets-push script. Staging has `TMO_API_TOKEN` and `TMO_DATABASE` set, and I confirmed those credentials actually work — `LOS.svc/GetLoans` returns 200 with real loan data. So staging is genuinely connected to a TMO instance; presumably the sandbox, since the production key is what you are waiting on. The split-system work still stands and is exactly right for the new production environment, which has no TMO secrets and therefore takes the local-first path by design. That matches what you asked for: production works without sync.

**I also closed the remaining hole in that work.** Last cycle handled TMO being *unconfigured*, but a TMO that is configured and *unreachable* — expired key, vendor outage, timeout — still 502'd the approval and left the application `failed`. That is a real weekend risk given staging runs on a sandbox key. TMO failures are now classified by whether TMO actually reached a verdict: network errors, 401/403/408/429, any 5xx, and a non-JSON 200 are `unavailable` and degrade to the same local-first path as an unconfigured TMO; 400/404/409/422 and a non-zero `Status`/`ErrorNumber` (duplicate loan number, validation) are `rejected`, stay a loud 502, and are never papered over by quietly creating the loan locally.

**Verification:** 1775 pass / 0 fail, typecheck + build clean. Both environments deployed and smoke-checked (`/`, `/apply`, `/contact`, `/login`, `/admin/login` all 200). Production bundle string-checked for the split-mode markers. Documented in `docs/deploy.md` § "S — Staging and production".

**Two decisions that are yours, not mine.**

Production starts empty. That is deliberate — an environment sharing staging's database is not a production environment — but it means if the 33 users, 7 loans, and 7 deals currently in staging are real business data rather than test data, they need to be carried over at cutover. The export/import recipe is in `docs/deploy.md`; the wrinkle is that documents live in R2, not D1, so `R2_DOCUMENTS` has to be copied separately or uploaded documents will 404 in production even though their rows exist. Queued as `#efs-prod-seed`. Tell me which it is and I will do it.

On custody: both environments are in your personal Cloudflare account, which also hosts unrelated projects. D1 databases and R2 buckets cannot move between accounts, so when EFS takes custody the production resources have to be recreated there and the data migrated again. If custody is likely to land before launch, it is cheaper to do the seeding once, after the move, rather than twice. Queued as `#efs-cf-account-custody`.

`#efs-dns-cutover` is unchanged and still the gate on the public launch — `equityfundingsource.com` has no A record and its nameservers are still DreamHost. When it cuts over, bind the domain under `[env.production]` only so staging stays on workers.dev; the exact TOML block is in the deploy doc.

### Re: Brief Cloudflare account instructions for Judith — opus-4-8

Here is a message you can send her as-is. It is five steps, about five minutes, and needs no technical knowledge. Swap in your preferred email on step 3.

---

**Setting up the EFS Cloudflare account**

1. Go to **dash.cloudflare.com/sign-up**. Enter an email and password, then click **Create Account**. No credit card is asked for.

   Please use a **shared EFS address** (something like `admin@equityfundingsource.com` or a distribution list), not your personal email. This address becomes the account's billing and recovery contact, so it needs to outlive any one person's mailbox.

2. **Check that inbox and click the verification link.** Cloudflare sends it right after signup. Nothing else on this list works until the email is verified.

3. Invite Brian: **Manage Account → Members → Invite**. Enter `brian.takita@gmail.com`, choose the role **Super Administrator**, then **Continue to summary → Invite**.

4. Turn on two-factor: **My Profile** (top-right menu) → **Authentication** → enable an authenticator app, and save the backup codes somewhere safe. This account will hold the live website, so it is worth the extra minute.

5. Have a **company card ready** — not needed today, but Cloudflare asks for one when the file-storage service is switched on, which is Brian's next step.

That is everything. Do not change any domain or DNS settings — Brian will handle that part.

---

Three notes for you, not her.

**Step 2 is a hard blocker, not a formality.** Cloudflare will not let her send the invite in step 3, or enable 2FA in step 4, until the email is verified. If she does steps 1 → 3 back to back it will just fail, so it is worth her clicking the link before moving on.

**Super Administrator is the right role for you, not a shortcut.** The narrower "Workers Platform Admin" role covers Workers, D1, R2, and Durable Objects, but it cannot purchase subscriptions or create account-owned API tokens — and you need both: R2 requires a checkout step, and the deploy pipeline authenticates with an account-owned token. Anything less means going back to her for each of those.

**One thing I could not confirm.** Cloudflare's own docs do not state whether activating R2 requires a card on file even for the free 10 GB tier. Community reports consistently say yes, and the billing docs describe card preauthorization, so I told her to have one ready rather than have that surface as a surprise mid-setup. Everything else above is from the official docs — signup takes no payment method, D1 is free-plan included, and SQLite-backed Durable Objects (what EFS uses) have been on the free Workers plan since April 2025, so the platform itself costs nothing to stand up.

Once she has done this and you have accepted the invite, `#efs-cf-account-custody` is unblocked: create the D1 database and R2 buckets in the new account, update the ids under `[env.production]`, migrate the data, re-push the secrets, and then point DNS at it. Doing custody before `#efs-prod-seed` means moving the data once instead of twice.
❯ 🚧 Will the $5/month account be sufficient for EFS? Please write the CF instructions in a markdown file.

### Re: Is $5/month enough, and the CF instructions as a file — opus-4-8

Both done — commit `7194c8f8`. Judith's guide is `src/equityfundingsource-dev/docs/cloudflare-account-setup.md`; the cost analysis is `docs/deploy.md` § "C — What this costs".

**Yes, $5/month covers EFS — with one caveat that is worth two minutes of your attention.**

I measured the live staging environment against the included allowances rather than guessing at the scale:

| Resource | Actual | Included in $5 | Headroom |
|---|---|---|---|
| D1 storage | 2.55 MB | 5 GB | ~2,000× |
| D1 rows read | 1.54 M/day | 25 B/month | ~540× |
| D1 rows written | 2,372/day | 50 M/month | ~700× |
| R2 storage (3 buckets) | ~17 MB | 10 GB | ~600× |

Two to three orders of magnitude of headroom on everything. Static asset requests, R2 egress, the rate limiters, cron, and the custom domain are all free and unmetered.

**The free plan is not an option**, and not for a reason that goes away with low traffic. Cloudflare Email Sending is simply unavailable on Free — EFS sends application-lead, contact, invite, secure-message, and login-link mail, so that alone decides it. Separately, Free caps CPU at 10 ms per request, which is a poor fit for server-rendering pages (`/` is ~73 KB of HTML). Worth noting too: D1 rows-read is already at 31% of the *free* daily cap with essentially no real users on the system.

**The caveat.** Durable Object duration bills wall-clock time the object is *active*, not CPU time. All three of your WebSocket room classes — `BorrowerChatRoom`, `AdminEventsRoom`, `PortalMessagesRoom` — call `server.accept()`, the non-hibernating API, and none sets an idle alarm (`lib/messages/realtime.ts:35`, `lib/borrower/messages/realtime.ts:76`, `lib/admin/events/realtime.ts:50`). So an open browser tab pins its room and accrues billing for as long as the socket is held, whether or not anyone is typing.

The consequence is that this line item scales with **concurrent open tabs**, not with business volume — the wrong axis. A single always-on room is about 328,500 GB-s against the 400,000 GB-s included, so one of them eats 82% of the monthly allowance by itself. Ten rooms open eight hours a day, twenty-two days a month, lands around $10 rather than $5.

That is not a launch blocker and not a large number, but the fix is cheap and removes the scaling problem entirely: switch the three classes to the WebSocket Hibernation API (`state.acceptWebSocket()` plus `webSocketMessage`/`webSocketClose` handlers). Cloudflare does not bill duration while a hibernatable socket sits idle. Queued as `#efs-do-hibernation` — say the word and I will do it.

So: **$5/month today, $5–10 in a busy month until hibernation lands, and $5 flat after.** Watch email volume as you grow — 3,000/month included, then $0.35 per 1,000, and per-event notifications creep up faster than you would expect. Mail to verified destination addresses in your own account is free and does not count against that quota.

One more thing for launch week: new accounts start with a conservative, undocumented daily email quota that scales with sending reputation. If day one needs volume, ask support to raise it *before* the cutover rather than discovering the ceiling live.

**On the instructions file** — I kept Judith's copy deliberately short and free of anything she does not need. Five steps, no jargon, an explicit "do not touch DNS," and a note that she will not be asked for a card during her part. The one thing I made unmissable is that email verification has to happen before she invites you: Cloudflare blocks both the member invite and 2FA until the address is verified, so running the steps back to back without clicking the link just fails confusingly.
<!-- agent:boundary:5e77cac6 -->
<!-- /agent:exchange -->

<!-- agent:queue preset="#spec-test-commit-push" go -->
<!-- /agent:queue -->

## Notes

<!-- agent:notes -->
How can I instruct Judith to create a Cloudflare account for EFS? She does not have much time today so please make brief instructions for her to follow.
---
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

We have not cut over the DNS today. The current CF environment will be staging. Please setup another CF environment for production. At some point, we will have EFS have custody of the CF account, possibly today. But I want to have a production environment ready to go in case it is needed.
<!-- /agent:notes -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#efs-do-hibernation] Switch the three Durable Object room classes to the WebSocket Hibernation API. BorrowerChatRoom, AdminEventsRoom, and PortalMessagesRoom all call the non-hibernating server.accept() with no idle alarm (app/equityfundingsource/lib/messages/realtime.ts:35, lib/borrower/messages/realtime.ts:76, lib/admin/events/realtime.ts:50), so an open browser tab pins its DO active and accrues GB-s duration billing whether or not anyone is typing — cost scales with concurrent open tabs instead of business volume. A single always-on room is ~328,500 GB-s of the 400,000 GB-s included in Workers Paid (~82%); ten rooms open 8h/day x 22 days is ~792,000 GB-s (~+$4.90/mo). Fix: state.acceptWebSocket() plus webSocketMessage/webSocketClose handlers; Cloudflare does not bill duration while a hibernatable socket is idle. Cost detail: src/equityfundingsource-dev/docs/deploy.md 'C — What this costs'.
- [ ] [#efs-prod-seed] [operator-verify] Decide whether the production Cloudflare environment should be seeded from staging before launch. Production D1 (equityfundingsource-production) is schema-complete and empty; staging holds 33 users / 7 loans / 7 deals. If that is real business data it must be exported+imported once, immediately before cutover (a second import collides on primary keys), and R2_DOCUMENTS must be copied separately or uploaded documents 404 in production despite their D1 rows existing. Recipe: src/equityfundingsource-dev/docs/deploy.md 'Seeding production from staging'. Blocked on the operator confirming real-vs-test data. See #efs-cf-account-custody — if custody moves first, seed once after the move.
- [ ] [#efs-cf-account-custody] [operator-verify] Move EFS to its own Cloudflare account. Both environments currently live in Brian's personal account (1e7e194ff8da4ad9e40ce041aea84f76), which also hosts unrelated projects. STEP 1 (Judith, instructions drafted 2026-07-18): sign up at dash.cloudflare.com/sign-up with a SHARED EFS address (becomes billing + recovery contact), verify the email (hard prerequisite — invites and 2FA both fail without it), invite Brian as Super Administrator via Manage Account -> Members -> Invite, enable 2FA, have a company card ready for the R2 checkout. Super Administrator is required, not just Workers Platform Admin, because R2 activation is a purchase and the deploy pipeline needs an account-owned API token. STEP 2 (Brian): D1 databases and R2 buckets cannot move between accounts — create them in the new account, update the ids under [env.production] in wrangler.toml, export+import D1, copy R2, re-push secrets (per-account AND per-environment), then repoint DNS. Sequencing: do this before #efs-prod-seed to migrate the data once instead of twice. Advances #efs-dns-cutover.
- [ ] [#efs-prod-secret-parity] Production is missing secrets staging has and .dev.vars cannot supply (values are only in Cloudflare and cannot be read back out): DROPBOX_APP_KEY / DROPBOX_APP_SECRET / DROPBOX_REFRESH_TOKEN gate document sync. AUTH_SECRET, ADMIN_GLOBAL_PASSWORD, CLOUDFLARE_API_TOKEN, and CLOUDFLARE_ACCOUNT_ID are already set (2026-07-18); TMO_API_TOKEN / TMO_DATABASE are intentionally unset so production runs split (#tmo-split-launch). AWS_*/S3_BUCKET_NAME are local bin/assets-*.sh only and are NOT needed in the worker. Compare with 'bun run cf:secrets:list' vs 'bun run cf:secrets:list:production'.
- [ ] [#efs-dns-cutover] [operator-verify] equityfundingsource.com does not resolve: no A/AAAA record, nameservers still ns1/2/3.dreamhost.com, and wrangler.toml declares no route or custom_domain. The worker is live and correct at equityfundingsource.brian-takita.workers.dev. Needs registrar + Cloudflare access: move the zone to Cloudflare (or add a CNAME at the registrar), add the custom domain / route to wrangler.toml, verify /login/access-callback still works, and allow for NS propagation. This is the remaining gate on the weekend launch. Advances #tcb0 / #efs-cutover-sso-plan.
- [ ] [#tmo-getlender-verify] [operator-verify] Confirm the #tmo-getlender-confirm LSS path against real TMO data: nothing in production is currently in_lss && !in_los, so GET /LSS.svc/GetLender/:Account has never returned a record in prod — the confirmed_no_funding state is covered by tests and degrades safely on failure, but its success path is unobserved. Needs an operator to name a TMO lender account created but never funded; map a vesting to it, open the reconcile modal, and confirm it reads 'In TMO · no funding yet' rather than 'Unconfirmed in TMO'. Spec: src/equityfundingsource-dev/docs/TMO/get-lender.md.
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
- [/] [#dzmm-writeback-replay] EFS->TMO writeback replay: once #7a4d field map and #k2xg support answers arrive, replay tmo_cache_outbox with idempotency keys, conflict logging, retry/dead-letter state, and tests across broker/admin/investor writebacks. Now also covers the #tmo-split-launch 'create_loan_application' rows — applications approved while EFS ran split carry the full LOS.svc/NewLoan body plus the EFS-assigned loan number and sit at writeback_status='gated' until replayed. Advances #dzmm/#37ah/#67v2/#zwws.
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
