---
agent_doc_session: 62fe1f41-886c-4f3d-b6fa-48d46106ec27
resume: 01a02ad0-c273-7640-add0-d1a2d1b232f0
agent: codex
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
queue: go
---

## Status

<!-- agent:status patch=replace -->

complete
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260822-180929.md`*

Compacted content:
- Archived 5 response topic(s): Homepage testimonial copy; Staging and production deployment; Expandable homepage review; 2 more
- Prior summary/context: prior compacted content: Archived 10 response topic(s): Google Reviews social proof; Remove Google Reviews border box...

### Re: Yelp beside Google reviews — gpt-5

> **Queue prompt:**
>
> do [#yelpalongside]


Completed `#yelpalongside`. The testimonial summary card now presents sibling **Google reviews** and **Yelp reviews** links, with responsive side-by-side layout where space permits. The displayed 5.0 aggregate remains explicitly scoped to Google; the Yelp CTA links to EFS's verified Yelp business profile without implying an unverified Yelp aggregate rating.

Updated the homepage PRD and regression coverage for the two profile destinations. Focused homepage/carousel tests passed, and the full `make check` suite completed successfully (exit 0). This repository currently has no GitHub Actions workflows configured to inspect.

Committed and pushed to `equityfundingsource-dev` as `fd648226` (`feat(home): link Yelp beside Google reviews (#yelpalongside)`). I also rebuilt and installed the current `agent-doc` binary/library and matching JetBrains plugin package to resolve the preflight stale-install warning; supervisor recycling was scheduled for the next idle boundary.

### Re: Testimonial auto-navigation — gpt-5

> **Queue prompt:**
>
> do [#testimonialsautonavigate]


Recommendation: **keep the testimonials manually navigated**. There are only two pages of text-heavy reviews, so autoplay adds distraction and can move content while someone is reading or interacting with an expanded review. The current Previous/Next controls keep the reading pace predictable.

This also avoids the extra interaction contract required for accessible autoplay. WAI's [carousel pattern](https://www.w3.org/WAI/ARIA/apg/patterns/carousel/) says an auto-rotating carousel needs a stop/restart control, must stop when keyboard focus enters, and must stop on pointer hover; its [carousel tutorial](https://www.w3.org/WAI/tutorials/carousels/) likewise emphasizes user control over movement.

Completed `#testimonialsautonavigate` by declaring `data-testimonial-rotation="manual"`, documenting the no-autoplay decision and future accessibility requirements in the homepage PRD, and updating regression coverage. Focused homepage/carousel tests passed, and the full `make check` suite completed successfully (exit 0). The EFS repository has no GitHub Actions workflows configured to inspect.

Committed and pushed to `equityfundingsource-dev` as `d5dfef77` (`fix(home): keep testimonials user-controlled (#testimonialsautonavigate)`).

### Re: Yelp full-review formatting — gpt-5

> **Queue prompt:**
>
> Full Yelp review does not format correctly ![img_5.png](img_5.png)


Fixed and pushed in `11a2822f` (`fix(home): make Yelp review embeds responsive`).

The failure was the carousel layout, not the review text: at the medium breakpoint, three cards squeezed Yelp’s injected iframe to 154px, producing the clipped widget and horizontal scrollbar shown in `img_5.png`. Yelp’s official embed uses a script-injected iframe and message-based resizing ([Yelp’s embed architecture](https://engineeringblog.yelp.com/2016/10/embedded-reviews-at-yelp.html)), so the surrounding card must provide a usable responsive width.

The testimonial grid now stays single-column below the wide-desktop breakpoint and temporarily becomes one full-width column whenever a Yelp disclosure is open. The iframe and all of its grid/flex ancestors have explicit shrink and width constraints, while the disclosure keeps a clean keyboard-visible focus ring.

Headless-browser verification at a 780px viewport showed the expanded Yelp iframe increase from 154px to 589px inside a 637px card, with the document width remaining 765px—no page-level horizontal overflow. The focused testimonial tests pass, `make check` exits 0, and `git diff --check` passes. No GitHub Actions workflows are configured for this repository.

### Re: Queue closeout — gpt-5

The completed Yelp formatting response is committed above. Its answered free-text queue head has now been consumed, and no drainable queue work remains.

### Re: Complete Google reviews — gpt-5

> **Queue prompt:**
>
> staging deploy


> **Queue prompt:**
>
> The read full reviews on the Google reviews is not showing the full Google review. Please fix. Based on https://www.google.com/maps/place/Equity+Funding+Source/@34.0945796,-118.2175058,17z/data=!3m1!4b1!4m6!3m5!1s0x80c2c7de7f00feb1:0x6881362d450c158d!8m2!3d34.0945796!4d-118.2175058!16s%2Fg%2F11w9s1jmh7?entry=ttu&g_ep=EgoyMDI2MDgxMi4wIKXMDSoASAFQAw%3D%3D

Fixed and committed in `117a2efc` (`fix(home): show complete Google reviews`).

The defect had two parts: Tim Hayden's **Read full review** disclosure stored only the first sentence, while the California Hard Money Direct and Alfredo Arteaga cards had no full-review disclosure. I verified all three against the linked Google listing and restored their complete listing copy. Each Google card now has a native keyboard-accessible disclosure; Alfredo's long review retains its four paragraph breaks, and the renderer HTML-escapes every paragraph before inserting it.

The homepage PRD now requires complete verified Google disclosure copy, and regression coverage asserts all three full reviews plus the long-review paragraphs. The focused homepage/carousel tests pass, and the full `make check` suite exits 0. This repository has no GitHub Actions workflows configured to inspect.

The requested EFS push follows this required session-document closeout. I also rebuilt and installed `agent-doc` 0.35.260 plus its native library/plugin package to resolve the preflight stale-install warning; route-owned supervisors were marked for idle-boundary recycling.

Suggested commit message (used): `fix(home): show complete Google reviews`

### Re: Staging deployment — gpt-5

> **Queue prompt:**
>
> deploy to staging

Staging deployment is complete.

- Pushed commit `117a2efc` (`fix(home): show complete Google reviews`) to `origin/main`.
- Ran the default staging `make deploy` target successfully.
- Deployed Worker version `301b441e-31de-4f11-87da-b4d6c02a9f3c` to [equityfundingsource.brian-takita.workers.dev](https://equityfundingsource.brian-takita.workers.dev).
- Confirmed the staging D1 database had no pending migrations before or after deployment. The pre-deploy Time Travel bookmark was `000018de-00000000-000050cf-e76dedef1a2f1d198cdcf6330abbf390`.
- Confirmed the live homepage returns HTTP `200`, contains Tim Hayden, California Hard Money Direct, and Alfredo Arteaga, and renders all three full-review disclosure blocks.
- `make check` passed before deployment.
- GitHub Actions inspection returned no workflow runs.
- Preserved the pre-existing `AGENTS.md` modification and untracked `.agent/` directory without changing them.

### Re: Yelp reviews in a modal — gpt-5

> **Queue prompt:**
>
> Full Yelp reviews are still not rendering properly. Should this be a modal instead? ![img_7.png](img_7.png)

Yes. The screenshot confirms that Yelp's cross-origin widget is too wide to expand cleanly inside a carousel card, so I replaced the in-card Yelp disclosure with a responsive native modal.

- **Modal interaction:** **Read full review** opens Yelp's official widget in the browser's modal top layer, leaving the three-card carousel layout unchanged.
- **Accessibility:** Native Escape handling remains available; the close button and backdrop close the dialog, the close control receives initial focus, and closing restores focus to the review trigger.
- **Responsive widget fit:** On narrow screens, the branded Yelp widget keeps a 400px internal layout and scales proportionally to the available modal width. Browser verification at a 390px viewport fit the widget into 350px at 0.875 scale with no horizontal overflow in the modal panel or page.
- **Loading behavior:** Yelp's script remains lazy-loaded on first modal open and is added only once.
- **Content integrity:** Direct reviewer-specific Yelp links remain on every Yelp card. The complete Google-review disclosures are unchanged.
- **Specification and tests:** Updated the homepage testimonial PRD and added coverage for modal opening/closing, focus restoration, lazy script loading, narrow-screen fitting, markup, and styles.
- **Verification:** Focused testimonial/homepage tests passed; full **make check** passed (exit 0). The digest's **d1 down** text is an expected mocked-outage case. **git diff --check** is clean. Browser-level desktop and mobile visual checks passed. No **CI** workflow is configured in this repository.
- **Commit:** **30baab79** — **fix(home): open Yelp reviews in responsive modal**. Push follows the required session-document closeout.
- Preserved the unrelated **AGENTS.md** modification and **.agent/** directory without staging or changing them.

Suggested commit message (used):

~~~txt
fix(home): open Yelp reviews in responsive modal

Move Yelp's official review widgets out of carousel cards and into accessible native dialogs, scale the branded widget for narrow viewports, preserve lazy loading and direct Yelp links, and update the PRD and testimonial coverage.
~~~

### Re: Response

Implemented and pushed the Yelp review modal fix.

- Native accessible modal with Escape, backdrop/close controls, and focus restoration
- Lazy Yelp widget loading
- Responsive mobile scaling with no page or modal-panel overflow
- Focused tests and full `make check` passed
- Browser-verified at desktop and 390px mobile widths
- Commit `30baab79` pushed to `origin/main`
- `agent-doc` cycle committed; `session-check` passed
- Unrelated `AGENTS.md` and `.agent/` changes preserved

Suggested commit message (used): `fix(home): open Yelp reviews in responsive modal`

### Re: Google review modals — gpt-5

> **Queue prompt:**
>
> Google full reviews should also be a modal...instead of expanding the height of the review section.

Implemented and committed as **229add16** (**fix(home): open Google reviews in testimonial modal**).

Google and Yelp testimonials now share the same native, keyboard-accessible modal controller. Each Google card keeps its excerpt in the carousel and opens the complete multi-paragraph review in the modal, so expanding review content no longer changes the testimonial section height. Yelp widget loading remains lazy and Yelp-only. Escape, backdrop, close-button behavior, and focus restoration are preserved.

Verification completed:

- Focused homepage and carousel tests passed.
- Full **make check** completed with exit code 0; the digest matched the suite's expected mocked **d1 down** error path.
- A local headless-browser check confirmed the complete Google review opens, body scrolling locks, horizontal page overflow remains absent, the carousel stays exactly the same height, and closing restores focus.
- No GitHub Actions workflow named **CI** exists in this repository.

Only the six intended implementation, PRD, and test files were committed. The existing **AGENTS.md** and **.agent/** worktree changes were preserved. The code push follows successful agent-doc closeout.

### Re: Side-by-side Google and Yelp ratings — gpt-5

> **Queue prompt:**
>
> Also show 5.0 on Yelp along-side 5.0 on Google ![img_8.png](img_8.png).

Implemented and committed as **6db40390** (**fix(home): show Google and Yelp ratings together**).

The testimonial summary card now presents two distinct rating columns: **5.0 on Google** and **5.0 on Yelp**, each with its own five-star accessibility label. The verified Google and Yelp profile links remain directly beneath the ratings. The layout follows the supplied screenshot on desktop and preserves the side-by-side ratings at 390 px; the narrower viewport stacks only the profile buttons.

Verification completed:

- Focused homepage tests passed.
- Full **make check** completed with exit code 0; the digest matched the suite's expected mocked **d1 down** error path.
- Headless-browser checks at desktop and 390 px confirmed equal rating columns, single-line platform labels, and no horizontal page overflow.
- The public Yelp page blocked automated retrieval, so the operator-supplied screenshot is the approval source for the Yelp **5.0** claim.
- No GitHub Actions workflow named **CI** exists in this repository.

Only the three intended homepage, PRD, and test files were committed. The existing **AGENTS.md** and **.agent/** worktree changes were preserved. The code push follows successful agent-doc closeout.

The session exchange is large; active queue draining continues without compaction as required by the current auto-queue.
<!-- agent:boundary:a61b5b26:equityfundingsource -->
<!-- /agent:exchange -->

<!-- agent:queue preset="#spec-test-commit-push" priority go -->
- ~~Full Yelp reviews are still not rendering properly. Should this be a modal instead? ![img_7.png](img_7.png)~~
- ~~Google full reviews should also be a modal...instead of expanding the height of the review section.~~
- 🚧 📌 Also show 5.0 on Yelp along-side 5.0 on Google ![img_8.png](img_8.png).
- 📌 deploy to staging
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#censiblecorefollow] Rotate the FactSet FTP credential with FactSet, then update passage finegar/FACTSET_FTP_LOGIN and FACTSET_FTP_PASSWORD. Proven 2026-08-02: the 12-char login and 8-char password sitting in censible-core git history (5 commits, d19a6c0f 2022-06-10 through f839e701b 2026-03-14) are SHA-256 identical to what passage serves today, so the credential has never been rotated and the historical copy is live. FTS.Factset.com:21 is reachable and answers. The tracked working-tree file was NOT carrying a secret - it holds passage command substitutions since f839e701b. Rotation supersedes any history rewrite; rewriting is optional cleanup for a private repo and cannot un-disclose anything. Blocked on a FactSet account action.
- [/] [#tmo-sandbox-purge-apply] [operator-verify] Execute the sandbox purge only after explicit operator go. The auto-mirrored queue directive is not approval for permanent `DeleteLoan` operations. Before any approved purge, take the documented snapshot; preserve the existing 16-loan target list, keep-list, dry-run evidence, and warning that attachment bytes and RecIDs are not recoverable. Recipes: `docs/TMO/delete-loan.md`, `docs/TMO/snapshot.md`.
- [/] [#tmo-orphan-apply] [operator-verify] Apply the orphan reconcile plan: TMO_API_TOKEN="$(passage equityfundingsource/tmo/sandbox/api | head -1)" bun ./bin/tmo-orphan-reconcile.ts --remote --apply. Dry run re-verified 2026-07-18 against remote D1: reconcile_cache=12 + tombstone=6, push_funding=0, blocked_no_vesting=0, zero TMO writes (EFS-local only). CORRECTION 2026-07-18: this targets the STAGING database (equityfundingsource, 280e1539-b940-4c59-8988-fbed63c12ac0), NOT production — the earlier 'mutates PRODUCTION D1' note was wrong; --d1 defaults to the staging binding and production (293f0a4d) is untouched. TMO side is the sandbox database (001-8553-000-Equity Funding - Sandbox, sandbox=true). All 6 tombstone rows (TST217, TST254, TST374, TST723, TST762, TST999) verified inert: efs_id NULL, zero rows in loans.tmo_loan_number, zero rows in funds (funds is empty entirely). Operator confirmed 2026-07-18 that staging data is test data. Remaining gate is operator go only.
- [/] [#tmo-reconcile-auto] Automated reconciliation attempt over tmo_unmatched_fundings: for each unmatched TMO funding try RecID -> Account against lender_vestings.tmo_lender_account -> lender email, record confidence plus evidence, auto-bind above a threshold and queue the rest with the candidate ranking pre-computed. Per PRD D4 a bind reconciles EFS to TMO (TMO funding wins) and must record what it overrode. The matcher must be a pure function over pulled fundings + the lender/vesting roster so it is testable without TMO, mirroring vesting_gap_. Phase 3 of src/equityfundingsource-dev/prds/vesting-and-tmo-reconciliation.md.
- [/] [#tmo-orphan-as-fundings] Replace #tmo-orphan-cache's tombstone plan per the operator directive: for each of the 18 orphan tmo_cache_entities 'loan' rows, reconcile against TMO if the loan exists, push the EFS-side funding into TMO via LOS.svc/AddLoanFunding (LOS-embed lender, PRD D1) and adopt the returned RecID if it does not, and tombstone only pure debris with no EFS deal. Gated on the EFS->TMO push gate (#dzmm/#7a4d). Phase 2 of src/equityfundingsource-dev/prds/vesting-and-tmo-reconciliation.md.
- [/] [#tmo-prod-seat-not-provisioned] [operator-verify] The 2026-07-17 20:19 "Welcome to The Mortgage Office" credentials are for the SANDBOX host: the email's own Login link is https://app2.themortgageoffice.com/Login, not app.themortgageoffice.com (which is what the 2026-05-15 welcome email linked). So April's "added you to the production account" has not landed as a production seat yet — app2 still offers exactly one company, Equity Funding - Sandbox. Ask April to confirm which account/serial the new seat was created on and to provision app.themortgageoffice.com access. Credentials stored at passage equityfundingsource/tmo_login_2026_07_17.
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
- [/] [#tmo-clone-apply] [operator-verify] Execute the anonymized live-to-sandbox TMO clone and the staging D1 refresh per prds/tmo-sandbox-clone-and-staging-refresh-plan.md section 8. CLONE SOURCE CONFIRMED 2026-08-02: use tmo-backups/2026-08-01/2026-08-01T17-15-48-000Z.json or later - it is the first schedule-bearing production artifact (14 of 31 loans, 43 schedule rows; the 2026-07-30 and 2026-07-31 artifacts are flat zero). Still blocked on two operator actions: (1) create passage btak/equityfundingsource/anonymize_seed, which must be stable forever because regenerating it re-pseudonymizes everything and breaks the sandbox-to-staging joins; (2) an explicit go for the destructive steps, which delete sandbox loans and wipe-and-load staging D1. Everything else is built and green as of EFS 20d7a416: rehearse with tmo-sandbox-clone.ts --simulate, then --from-r2 dry run, then --from-r2 --apply, then d1-staging-refresh.ts dry run and --apply. Coordinate with #tmo-sandbox-purge-apply, which gates the same purge.
- [/] [#efs-email-subdomain] [operator-verify] Grant `passage:btak/equityfundingsource/production/cloudflare_api_token` Email Sending, Email Routing, and required DNS write access on `equityfundingsource.com`; then enable the `efs.equityfundingsource.com` sending/routing subdomain, route `reply@efs.equityfundingsource.com` plus-addressed replies to the EFS Worker, push and deploy EFS commit `c6d4133e`, and verify login, invite, notification, and inbound-reply delivery end to end. Current evidence: Email Sending list succeeds for the apex, but Wrangler enable and direct POST `/zones/426085c68d8d584075c824ff1d715926/email/sending/subdomains` fail with Cloudflare code 10000; the separate infra token has no production-account access.
<!-- /agent:review -->

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

## Completed / Reaped

<!-- agent:icebox -->
- [ ] [#efs-prod-secrets] [operator-verify] Cutover prerequisite, NOT a live outage — the [env.production] Cloudflare environment is provisioned and deployed but not launched (no DNS, empty D1, no traffic). It has 4 secrets; the live default env has 13. Present in [env.production]: ADMIN_GLOBAL_PASSWORD, AUTH_SECRET, CLOUDFLARE_ACCOUNT_ID, CLOUDFLARE_API_TOKEN. Missing: TMO_API_TOKEN, TMO_DATABASE, AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY, AWS_ENDPOINT_URL_S3, DROPBOX_APP_KEY, DROPBOX_APP_SECRET, DROPBOX_REFRESH_TOKEN, and one more. If DNS were cut over today, every TMO, S3, and Dropbox call would fail closed. Push with 'bun run cf:secrets:push:production'. Secrets are per-account AND per-environment, so do this AFTER #efs-cf-account-custody or it has to be redone. Discovered 2026-07-18.
- [ ] [#efs-cf-account-custody] [operator-verify] Move EFS to its own Cloudflare account. Both environments currently live in Brian's personal account (1e7e194ff8da4ad9e40ce041aea84f76), which also hosts unrelated projects. STEP 1 (Judith, instructions drafted 2026-07-18): sign up at dash.cloudflare.com/sign-up with a SHARED EFS address (becomes billing + recovery contact), verify the email (hard prerequisite — invites and 2FA both fail without it), invite Brian as Super Administrator via Manage Account -> Members -> Invite, enable 2FA, have a company card ready for the R2 checkout. Super Administrator is required, not just Workers Platform Admin, because R2 activation is a purchase and the deploy pipeline needs an account-owned API token. STEP 2 (Brian): D1 databases and R2 buckets cannot move between accounts — create them in the new account, update the ids under [env.production] in wrangler.toml, export+import D1, copy R2, re-push secrets (per-account AND per-environment), then repoint DNS. Sequencing: do this before #efs-prod-seed to migrate the data once instead of twice. Advances #efs-dns-cutover. KEPT OPEN 2026-07-18: not started — this cycle was preempted by the operator's live Jeff Blake / Lucas Huang TMO investigation prompt.
- [ ] [#efs-dns-cutover] [operator-verify] equityfundingsource.com does not resolve: no A/AAAA record, nameservers still ns1/2/3.dreamhost.com, and wrangler.toml declares no route or custom_domain. The worker is live and correct at equityfundingsource.brian-takita.workers.dev. Needs registrar + Cloudflare access: move the zone to Cloudflare (or add a CNAME at the registrar), add the custom domain / route to wrangler.toml, verify /login/access-callback still works, and allow for NS propagation. This is the remaining gate on the weekend launch. Advances #tcb0 / #efs-cutover-sso-plan. KEPT OPEN 2026-07-18: not started — this cycle was preempted by the operator's live Jeff Blake / Lucas Huang TMO investigation prompt.
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
