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
queue: start
---

## Status

<!-- agent:status patch=replace -->

completed
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260621-185358.md`*

Compacted content:
❯ - Archived 5 response topic(s): Advance Lender account-message email + deploy; deploy; Message sender timestamps + deploy; 2 more
❯ - Prior summary/context: ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b...

### Re: One floating context-sensitive message widget to unify portal messaging? — opus-4-8

> **Queue prompt:**
>
> Should we just have a context-sensitive message component absolutely positioned in the bottom right corner to unify all messaging in the portal?


> **Queue prompt:** Should we just have a context-sensitive message component absolutely positioned in the bottom right corner to unify all messaging in the portal?

Short answer: **build one unified message component, yes — but make placement a prop, not a single global corner bubble.** The corner launcher is the right default for the self-service portals; the admin operational pages should keep an inline/docked rendering of the *same* component.

**Why this is mostly already done at the layer that matters.** The data is already unified: `app/equityfundingsource/lib/messages_unified.ts` `merge_unified_messages_` folds the three sources — `borrower_messages_to_unified_`, `lender_deal_messages_to_unified_`, `lender_account_messages_to_unified_` — into one `message_T[]` stream (the `msg-unify` series). What's *not* unified is the frontend: there are still separate embedded threads on admin lender-pipeline, deal-detail, borrower application-status, and investor accreditation/requirements. So the real proposal is "collapse the N embedded thread UIs into one component," and the backend makes that cheap.

**Recommendation — one component, two placements:**
- Build a single `<PortalMessages context={...}>` over `merge_unified_messages_`. `context` carries the current scope (deal id / lender id / borrower / "account") so the thread auto-scopes to what the page is about — that's the "context-sensitive" win, independent of where it's drawn.
- **Self-service portals (lender / borrower / investor):** render it as the persistent bottom-right floating launcher. One always-available entry point + a global unread badge is a clear UX upgrade over hunting for an inline thread per page. This is where your instinct is right.
- **Admin operational pages (lender-pipeline, deal-detail):** keep it **inline/docked** next to the record. An admin advancing a lender or working a deal needs the conversation *beside* the row they're acting on; a corner bubble that floats over the table forces context-switching and covers content. Same component, `placement="inline"`.

**Why not a single global corner widget everywhere:**
- It detaches the conversation from the content it's about — on the Requirements page the thread sitting next to the requirements *is* the affordance.
- It covers admin tables/CTAs and is worse on mobile.
- Lower discoverability for first-time lenders mid-questionnaire than an inline "Questions? Message us" panel.

**Net:** unify at the component + data layer (single source of truth, already half-built), and treat "floating corner" as the default *placement* for self-service, with inline docking retained for admin work surfaces. That gets you the unification and the context-sensitivity without regressing the admin operational flows.

This is a sizable frontend surface change, so I'm leaving it as a recommendation rather than implementing it. If you want to proceed, say the word and I'll spec it (a `prds/` plan + one `<PortalMessages>` component with a `placement` prop, migrating the existing embedded threads one page at a time behind tests) and ship it. If you'd rather go all-in on a single global launcher everywhere (including admin), I can do that too — just flagging the admin-page tradeoff above.

Confidence: high on the data-layer facts (verified in `messages_unified.ts`); the placement recommendation is a UX judgment call, hence surfacing the tradeoff rather than auto-deciding.

### Re: PRD the unified component — opus-4-8

> **Queue prompt:**
>
> Prd the unified component


> **Queue prompt:** Prd the unified component

Wrote the PRD for the unified frontend message component: `src/equityfundingsource-dev/prds/portal-messages-component.md` (EFS `47d0cb0`, committed + pushed). PRD only — not implemented.

**What it specs (`#portalmsg`):** one `<PortalMessages context placement>` component over the already-unified `merge_unified_messages_` stream, collapsing the N embedded thread UIs (admin lender-pipeline card, deal-detail, borrower application-status, investor accreditation/requirements, broker) into one. Per the prior recommendation, **placement is a prop, not a single global corner bubble**:
- `context` = `{ party, scope_type, scope_id, party_user_id }` → maps directly to the `message_T` filter the merge layer already produces (the context-sensitive win, independent of where it's drawn).
- `placement="launcher"` = persistent bottom-right floating launcher + global unread badge → default for self-service portals (lender/borrower/investor).
- `placement="inline"` = docked thread beside the record → default for admin operational pages, so an admin isn't fighting a corner bubble covering the table.

**Scope boundary:** this is the *frontend* half. The *data* half (the `messages` table, adapters, admin inbox) stays in `message-unification.md` (`#msg-unify`). Write-path unification is explicitly out of scope — compose keeps POSTing to the existing per-party endpoints until the `#msg-unify3/4/5` cutovers move them onto `messages`.

**Phasing** (one backlog id per phase, each migrated surface verified before the next, legacy thread removed only after its replacement passes tests): #portalmsg1 component foundation (both render modes, shared thread/compose/attachment/unread internals, avatar/name/timestamp row invariant) → #portalmsg2 admin inline cutover → #portalmsg3 self-service launcher (lender/investor) → #portalmsg4 borrower launcher → #portalmsg5 broker (after `#msg-unify5`).

No code/test surface this cycle since it's a planning doc; the `#spec-test-commit-push` preset's deliverable here is the spec, committed + pushed.

### Re: Implement the unified component + always-visible + deploy — opus-4-8

> **Queue prompt:**
>
> Implement src/equityfundingsource-dev/prds/portal-messages-component.md. Make sure the prop component is always visible in the respective UI context. My always visible, make sure the user can click on the messaging no matter where the user scrolls on a dialog, page, or other component that has the messaging prop/context.


> **Queue prompt:** Implement src/equityfundingsource-dev/prds/portal-messages-component.md. Make sure the prop component is always visible in the respective UI context... make sure the user can click on the messaging no matter where the user scrolls on a dialog, page, or other component that has the messaging prop/context.
>
> **Queue prompt:** deploy

Built and shipped the unified `<PortalMessages>` component (#portalmsg1/3). Committed + pushed (EFS `5cc49ab`) and **deployed** (worker version `7926c38a`, live at equityfundingsource.brian-takita.workers.dev). All 1061 tests pass; `build` + `build:worker` green.

**The component** (`lib/ui--browser--efs/portal/shared/portal_messages.tsx`): one component over the per-party message endpoints, with **placement as a prop** — exactly the always-visible behavior you asked for:
- `placement="launcher"` → a `position:fixed` bottom-right FAB **plus a `position:fixed` panel**. The launcher and its open panel are pinned to the viewport, so messaging is reachable and clickable **no matter where the user scrolls** the page. Unread badge on the FAB.
- `placement="inline"` → a card whose composer sits in a `position:sticky` bar (`.pm-inline-compose`), so within a scrolling dialog/page the compose stays reachable (this is the mode for the admin operational pages, queued as #portalmsg2).
- `context = { party, scope_type, scope_id }` resolves endpoint + self-role + copy via the pure, unit-tested `portal_messages_config_`.

**Wired now (self-service launchers):** broker dashboard and investor/lender dashboard now mount `<PortalMessages placement="launcher" ...>`. Deleted the now-orphaned `BrokerMessages` component (fully subsumed). The `LenderAccountMessages` thread stays — it still backs the accreditation/requirements page and the admin pipeline card.

**CSS:** added `.pm-launcher` / `.pm-fab` / `.pm-panel` / `.pm-badge` / `.pm-inline-compose` to `portal.ts`.

**Tests:** new `tests/portal-messages-component.test.ts` (config resolution, the launcher-fixed / inline-sticky always-visible invariant, dashboard wiring); updated the 3 existing tests that asserted the old broker/lender dashboard wiring.

**Scoping note — this is phase 1 + the self-service half of phase 3.** I deliberately did NOT migrate everything in one deploy. Queued the rest as backlog so each live-surface migration is verified on its own:
- `#portalmsg-shell` — mount the launcher at the portal SHELL (not just the dashboard view) so it's always-visible across *every* self-service page, and drop the now-duplicate inline thread on the accreditation page.
- `#portalmsg2` — admin inline cutover (lender-pipeline dialog + deal-detail) using the sticky inline mode.
- `#portalmsg4` — borrower launcher (has Durable Object websocket complexity, needs care).

One honest caveat for live verification: the launcher currently mounts on the dashboard *view*. It's always-visible while you scroll the dashboard; making it persist across all sub-pages of a portal is `#portalmsg-shell`. Worth an eyeball on the deployed lender/broker dashboards to confirm the FAB + panel feel right before I push the shell-level mount.

### Re: #portalmsg-shell — shell-level always-visible launcher — opus-4-8

> **Queue prompt:**
>
> deploy


Done + deployed (#portalmsg-shell). EFS `c3a78d6`, worker version `e11943ce`, live. All 1061 tests pass; `build:worker` green.

You picked "Do #portalmsg-shell next" — so I moved the launcher from the per-view dashboards up to the portal **shells**:
- `InvestorPortalApp` (lender) and `BrokerPortalApp` (broker) now mount `<PortalMessages placement="launcher" ...>` once, as a sibling of the layout. Because it's `position:fixed`, the FAB + panel now render on **every** section of those portals (dashboard, requirements, positions, deals, documents, ...), not just the dashboard — that's the "always-visible no matter where you scroll OR which page" win.
- Removed the launcher from the two dashboards (it would otherwise double-mount).
- **Dropped the now-duplicate inline thread on the accreditation/requirements page** — lender messaging there is covered by the shell launcher.

Tests re-pointed from the dashboards to the shells; the requirements-page test now asserts the shell launcher provides the lender's messaging instead of a per-page thread.

**One bit of debt I deliberately deferred, not hid:** `investor/lender_account_messages.tsx` is now orphaned (the shell launcher fully supersedes it), but deleting it cascades into the `#lender-card-msg-portal` test block. I left the file in place and filed `#portalmsg-shell-cleanup` to delete it + fold its coverage into the unified component tests — deferred so you can eyeball the live shell launcher first.

**Queue note:** you chose only #portalmsg-shell, so I'm **not** auto-draining the remaining `do [#portalmsg2]` (admin inline) and `do [#portalmsg4]` (borrower DO/websocket) heads — those stay queued for your call. Worth eyeballing the deployed lender + broker portals (the FAB should now follow you across every page) before I take the next phase.

### Re: #portalmsg2 — admin lender thread → unified inline component — opus-4-8

Done + deployed (#portalmsg2). EFS `0a49dfa`, worker version `101ce2fd`, live. All 1063 tests pass; `build:worker` green.

Migrated the admin lender-pipeline messaging to the unified component, with the sticky compose you wanted:
- Extended `PortalMessages` with an explicit **admin / EFS-as-sender** config path (`endpoint` + `self_role`/`self_label` + `other_role`/`other_label` overrides) and an `admin_lender_messages_context_(user_id)` helper. Self-service rendering is unchanged (defaults resolve to the old behavior).
- Replaced the bespoke ~110-line admin `LenderAccountMessages` component (2 mount sites: lender-pipeline card + advance-lender dialog) with `<PortalMessages placement="inline" ...>` over the **same** user-scoped admin endpoint (`/api/admin/lenders/:user_id/messages`). Now uses the `position:sticky` `.pm-inline-compose`, so an admin's composer stays reachable while the card dialog scrolls.
- EFS rows render as "self", lender rows as the counterparty — the avatar/name/timestamp row invariant is preserved.

**No admin-table regression:** the change is confined to the card-dialog thread component; the pipeline board/table and the shared deal-interest thread helpers are untouched. Net −130/+87 lines (deleted the duplicated component).

**Scoped honestly — I did NOT do all of #portalmsg2's wording in one shot.** It listed "card dialog AND deal-detail." I migrated the self-contained account-scoped lender thread; the per-deal-interest "Lender communication" thread (`DealInterestWorkflowList`, deal-interest-scoped) and the admin deal-detail borrower+lender threads are more entangled (they share borrower-message helpers and deal-interest state), so I split them into **`#portalmsg2-dealthreads`** rather than risk that refactor in the same cycle.

**Queue:** you approved only #portalmsg2, so I'm **not** auto-draining the remaining `do [#portalmsg4]` (borrower DO/websocket — the riskiest) or `do [#portalmsg-shell-cleanup]`. Those stay queued for your call. Worth eyeballing the deployed admin lender-pipeline card (open a lender, scroll the dialog — the "Message lender" composer should stick to the bottom) before the next phase.

### Re: #portalmsg-shell-cleanup — delete orphaned component — opus-4-8

Done (#portalmsg-shell-cleanup). EFS `500075e`, pushed. All 1061 tests pass; `build:worker` green. **No deploy** — the bundle hash is unchanged (`WGHFDSA7`, same as #portalmsg2), confirming the deletion is runtime-inert (nothing imported it).

Deleted the now-orphaned `lib/ui--browser--efs/portal/investor/lender_account_messages.tsx` (the standalone lender-facing account-thread UI), fully superseded by the unified `PortalMessages` shell launcher (party=lender over `/api/investor/messages`). −187 lines.

Coverage folded, not lost:
- The lender reply round-trip test (`lender_account_message_create_` → `lender_account_messages_R_`, sender_role=lender, lender-scoped) and the investor-route registration test cover the **backend** — untouched, both still pass.
- The shell test now asserts the unified component carries the `/api/investor/messages` endpoint.
- Removed the deleted file from the avatar/shortcut surface lists (the shared `portal_messages.tsx` already covers both).

The **backend** (`lender-account-messages.ts` repo, route handlers, `lender_account_messages_to_unified_` adapter) is deliberately untouched — that data layer still serves both the lender launcher and the admin thread.

**Queue:** two phases remain, both for your call — `do [#portalmsg2-dealthreads]` (admin deal-interest + deal-detail threads, medium risk) and `do [#portalmsg4]` (borrower launcher over the live DO/websocket chat, highest risk). I'm not auto-draining either.

### Re: Stop — leave remaining phases as backlog — opus-4-8

Stopping here per your call. The unified messaging feature's high-value, low-risk work is **complete and live**; the two remaining phases stay as backlog.

**Shipped + deployed this session:**
- PRD `prds/portal-messages-component.md`
- `#portalmsg1/3` — `<PortalMessages>` component (launcher + inline placements) + self-service launchers
- `#portalmsg-shell` — launcher hoisted to the lender/broker portal shells (always-visible on every page)
- `#portalmsg2` — admin lender-pipeline account thread → unified inline (sticky compose)
- `#portalmsg-shell-cleanup` — deleted the orphaned standalone component

Latest worker version `101ce2fd` (cleanup was runtime-inert, no redeploy). 1061 tests green.

**Left as backlog (not started), both larger refactors than the shipped phases:**
- `#portalmsg2-dealthreads` — the deal-interest "Lender communication" + admin deal-detail threads are **controlled** components (render from pre-loaded `interest.lender_messages`/`borrower_messages` with optimistic send handlers), so this needs `PortalMessages` extended to a controlled mode (messages + onSend props) before migrating. Not a quick swap.
- `#portalmsg4` — borrower launcher over the live Durable Object websocket realtime chat path; highest regression risk.

Both remain queued as `do [#id]` mirrors of their backlog items. The in-session queue is paused (qflood guard) and I'm not auto-draining; if `/agent-doc` keeps re-firing on these deferred heads, set `queue: stop` in the frontmatter (or remove the two `do [#...]` lines) to silence it — preflight re-forces `queue: start` while drainable heads exist.

Net for the session: one unified, always-visible messaging component replacing four bespoke thread UIs across the self-service portals and the admin lender card, all behind tests and deployed.

### Re: Email reply-to round-tripping for messaging — opus-4-8

> **Queue prompt:** The message user's email reply-to should go back to the app email (the sender). A CF worker should process the reply, write the message into the database, and send an email to the admin. The admin should be able to reply to the app email, which writes the message into the database, and sends an email to the user. And so on. All emails should be processable by the app via reply-to.

Wrote a grounded PRD for this: `src/equityfundingsource-dev/prds/message-email-roundtrip.md` (EFS `e9175ce`, pushed). PRD only — held off implementing because it touches production email and there's a big finding worth a decision first.

**Key finding — roughly half of this already exists, just gated off.** The borrower ↔ admin chat *already* implements the entire round-trip; it's wired but disabled until inbound routing is configured:
- `message_reply_to_(env, session_id)` — builds a parseable Reply-To via plus-addressing (`base+{token}@domain`).
- `message_inbound_session_id_()` — parses the token back out of an inbound reply's `to`/headers.
- `POST /api/email/inbound` — shared-secret auth → resolve thread → write the message → notify the other side.
- `notify_admin_of_borrower_message_` / `notify_borrower_of_admin_message_` — already set Reply-To + "Reply to this email to add your response to the same thread."
- `email/index.ts` already plumbs `reply_to` through the Cloudflare Email send path.

It's all behind `MESSAGE_EMAIL_INBOUND_ENABLED` / `_SECRET` / `_REPLY_TO` (unset in prod), and it's **borrower-`chat_sessions`-scoped only**.

**So the request = generalize that path to the unified `messages` model for all parties** (lender account + deal-interest, broker, admin), not build from scratch. The PRD's phased plan, filed as backlog:
- `#msgmail1` — unified reply token + addressing (generalize the helpers from borrower `session_id` to a unified thread token; pure plumbing, no behavior change)
- `#msgmail2` — set Reply-To + reply hint on the lender/broker/admin notify emails
- `#msgmail3` — generalize `POST /api/email/inbound` to decode any party's token, authorize the sender as a thread participant, write to `messages`, notify the counterparty
- `#msgmail4` — enable in prod (Cloudflare Email Routing → worker, the secret, the reply-to template) — operator-gated ops, don't enable blind

Security invariants carry forward: fail-closed without the inbound secret, and the inbound `from` must be an authorized participant of the addressed thread (prevents thread injection).

**Recommendation:** I can start `#msgmail1` (token + addressing generalization — additive, well-tested, low risk, the borrower path keeps working through the generalized helpers). `#msgmail4` stays operator-gated since it flips on live inbound email. Say the word and I'll build `#msgmail1` → `#msgmail3` in sequence; the prod enable (`#msgmail4`) waits for you. "deploy" in the queue is moot this cycle — the PRD is a planning doc.

(The two earlier frontend phases — `#portalmsg2-dealthreads`, `#portalmsg4` — remain pinned/deferred per your last call; this email work is an independent transport-layer track.)
<!-- agent:boundary:f1773c94 -->
<!-- /agent:exchange -->
###
<!--
Is there an admin for the borrower portal messages? Can we have an email notification whenever a borrow sends a message? Can we support an email response back into the chat? Can the borrower receive emails whenever an admin responds with a message?
---
After 20:30
- login to TMO. Make sure we are using the correct credentials.
- TMO Sync
-->

<!-- agent:queue preset="#spec-test-commit-push" priority go -->
- The message user's email reply-to should go back to the app email (the sender). A CF worker should process the reply, write the message into the database, and send an email to the admin. The admin should be able to reply to the app email, which writes the message into the database, and sends an email to the user. And so on. All emails should be processable by the app via reply-to.
- :pushpin: do [#portalmsg4]
- :pushpin: do [#portalmsg2-dealthreads]
- deploy
- do [#msgmail4]
- do [#msgmail3]
- do [#msgmail2]
- do [#msgmail1]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#msgmail4] [recommended] Enable email round-trip in prod (ops, operator-gated): configure Cloudflare Email Routing to POST inbound mail to the worker w/ the shared secret; set MESSAGE_EMAIL_INBOUND_ENABLED/_SECRET/_REPLY_TO; verify sending domain (#f4rq). Do not enable blind. Plan: src/equityfundingsource-dev/prds/message-email-roundtrip.md
- [ ] [#msgmail3] [recommended] Unified inbound route: generalize POST /api/email/inbound to decode any party token, authorize the sender as a thread participant, write to messages, notify the counterparty. Mocked-inbound tests across borrower/lender/broker/admin. Plan: src/equityfundingsource-dev/prds/message-email-roundtrip.md
- [ ] [#msgmail2] [recommended] Outbound Reply-To for all parties: set reply_to + reply hint on lender (account + deal-interest), broker, and admin notify emails using the unified token (mirrors the borrower path). Plan: src/equityfundingsource-dev/prds/message-email-roundtrip.md
- [ ] [#msgmail1] Unified reply token + addressing: add the thread token to the unified messages model (reuse thread_id or add reply_token), generalize message_reply_to_ / message_inbound_* from borrower session_id to the unified token, with unit tests. No behavior change. Plan: src/equityfundingsource-dev/prds/message-email-roundtrip.md
- [ ] [#portalmsg2-dealthreads] [recommended] Finish the admin inline cutover: migrate the per-deal-interest "Lender communication" thread (DealInterestWorkflowList in investor_pipeline.tsx, endpoint /api/admin/deal-interests/:id/lender-messages) and the admin deal-detail borrower+lender threads to <PortalMessages placement="inline">. More entangled than the account thread (deal-interest-scoped, shares borrower-message helpers), so split from #portalmsg2. Advances #portalmsg2.
- [ ] [#portalmsg4] [recommended] Borrower launcher: migrate borrower application-status messaging (sessions/loan-scoped + Durable Object websocket path) to <PortalMessages placement="launcher">; needs care around the live DO chat path. Advances #portalmsg4.
- [/] [#cmtsec] [recommended] (UPDATED w/ D1 data 2026-06-20) d6b78994 "Committed amount" section shows a REAL submitted proposal: investor@ (btak.dev+investor@gmail.com) proposal_amount=$100,000 @ 6.5%, submitted 2026-06-19, selected_at=null — NOT a null-amount artifact, so for this deal the block is correct. Latent UI note still valid for OTHER rows: interest_has_proposal_ (admin/deal_detail.tsx:730) renders the block whenever proposal_submitted_at is set, which lib/investor/shared.ts:504 stamps on ANY of amount/rate/points — a rate/points-only proposal would show "Committed amount: -". Optional: gate on proposal_amount != null or relabel "Proposed terms". If the $100k investor@ proposal was unintended/test data, clear that interest row to remove the section.
- [/] [#5q7z-ship] [operator-verify] DEPLOYED 2026-06-19 (worker version 0dd3c983; EFS d8c17b4, pointer 58a2c5fea). Remaining = operator live-verify only: open the admin approve flow on an already-in-TMO loan (deal 7a0c1463 / loan 45ddae53 GQI751) and confirm it no longer returns TMO 400 Duplicate loan number and the Accept-and-Send button reads Already in TMO. Advances #5q7z.
- [/] [#cutover-checklist] [recommended] Write (do not execute) the workers.dev to equityfundingsource.com custom-domain + Google Workspace SSO cutover checklist doc: routing for /, /funded-deals, admin routes; /login/access-callback; rollback steps. Execution stays gated on #m6ch creds. Advances #efs-cutover-sso-plan/#next-steps.
- [/] [#67v2-aa-local] [recommended] Admin pipeline auto-advance local half (advances #67v2-auto-advance): define stage-requirement checks from the TMO-aligned Kanban and enqueue audited status updates to tmo_cache_outbox; replay stays gated on #dzmm. Depends on the #67v2 Kanban stages being built first.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#mcme] [recommended] #msg-read-status Email-open-based read receipts on messages — DEFERRED: gated by the operator on "only if we can detect email reads", which EFS cannot today (sends via Cloudflare Email, no open tracking). Needs a ~3-5 day greenfield subsystem: switch message notifications to Resend with open-tracking, add a signed Resend event webhook (email.opened), an email_events table, and message<->email linking, then surface email_opened_at in the message DTO/UI. In-app portal read cursors already exist for borrower chat (chat_sessions.*_last_read_at) but are not email reads.
- [/] [#msg-unify4] [gated: awaiting dual-write prod soak] Lender cutover — dual-write (best-effort mirror into messages, both lender sources) + backfill (migration 0098) SHIPPED (EFS b5a6a55). Remaining: after soak, switch the inbox to read lender data from messages (dual-read), then drop deal_interest_lender_messages + lender_account_messages. Plan: src/equityfundingsource-dev/prds/message-unification.md.
- [/] [#msg-unify3] [gated: awaiting dual-write prod soak] Borrower cutover — dual-write (best-effort mirror into messages) + backfill (migration 0097) SHIPPED (EFS 411a4da). Remaining: after the dual-write soaks live, switch the unified inbox to read borrower data from messages (dual-read), then drop chat_messages. Plan: src/equityfundingsource-dev/prds/message-unification.md.
- [/] [#lender-card-msg-email] [recommended] Email-to-lender parity for #lender-card-msg account messages (mirror notify_lender_deal_message_): notify the lender by email when an admin posts an account message, like the deal-interest thread does. Rides the verified EFS Resend sending domain gate (#f4rq). Plan: src/equityfundingsource-dev/prds/lender-card-messaging.md.
- [/] [#proprate] Lender proposal persists a DEFAULT interest rate never finalized. investor/deal_detail.tsx:362 pre-fills proposal_rate from deal.monthly_interest when deal.proposal_interest_rate==null; submit (:425-435) sends it whenever an amount is present; :421 requires a finite rate, so a lender cannot commit an amount with rate TBD — the pre-filled default (6.5% on d6b78994) persists as if finalized. Duration NOT affected (no proposal_term_months column). DATA CLEANUP DONE 2026-06-20: nulled proposal_interest_rate on the d6b78994/b8ccaa76 row (kept proposal_amount=100000 + proposal_submitted_at; D1 changes=1). CODE FIX still pending operator go: do NOT pre-fill the rate with the deal default; allow proposal_interest_rate=null ("amount committed, rate TBD"); relax :421 validation + admin LoanTermProposalSummary display.
- [/] [#nthc] [operator-verify] DEPLOYED 2026-06-20 (worker version 17b06176-fc91-44b6-b35d-1cd61beed01b; EFS 8889a4c). Remaining = operator live-verify only: open the Lender Bros group Edit overlay on deal d6b78994, bump lender2 from 0%, confirm the split saves and the "60% confirmed (partial)" badge reconciles.
- [/] [#pmhf] [recommended] Reuse the rebalancing share editor (lib/ui--browser--efs/portal/shared/lending_groups_alloc.ts: rebalance_stakes_/distribute_evenly_/lock pins) in the per-deal split UI (deal_detail allocations) so per-deal % editing gets the same 100%-balanced, lockable inputs.
- [/] [#dealtab500] [operator-verify] Root-cause the deal 7a0c1463 / loan 45ddae53 (GQI751) loan-tab "Internal Error". Static trace independently re-confirmed exhausted: GET /admin/pipeline serves only a shell (no server-side deal processing); every client-fetched endpoint is guarded — admin_loan_R_ is a plain SELECT, admin_deal_R_ LTV/onboarding chain (onboarding_json_/loan_ltv_calc_) is fully try/catch + null guarded, /documents is harmless; all client fetches degrade to error cards, not the worker Internal Error page. Regression guard shipped (efs a39be3d) locking the thin-TMO-loan onboarding shape against throws (gformula deal-economics helpers). REMAINING (operator-gated, cannot be agent-done): reproduce live via `bun run cf:worker:tail` + a browser click on the loan tab to capture the real throwing endpoint + stack, OR inspect loan 45ddae53/deal 7a0c1463 D1 rows once the #cftk token lands. Relates to #5q7z/#214f.
- [/] [#5q7z] [recommended] 'Accept and send to TMO' duplicate-loan bug: for a loan already associated with a TMO record (deal 7a0c1463 / loan 45ddae53 has tmo_loan_number=GQI751, tmo_loan_rec_id=3840D27B..., tmo_loan_application_rec_id=cda67748), the approve flow (admin/applications/index.ts application_tmo_writeback_attempt_ -> tmo_writeback.ts:308 LOS.svc/NewLoan) sends NewLoan with the existing GQI751 -> TMO 400 'Duplicate loan number. Unable to save record.' Fix (2 parts): (1) approve/writeback must detect an existing TMO association (tmo_loan_rec_id / writeback_status='sent') and associate+mark accepted locally instead of calling NewLoan (or call an UpdateLoan endpoint); (2) hide/disable the 'Accept and send to TMO' button in lib/ui--browser--efs/portal/admin/full_applications.tsx:424 when the app/loan is already in TMO. Design Q: on accept of an already-in-TMO loan, associate-only (no TMO call) or also push UpdateLoan? Relates to #dzmm/#37ah/#214f.
- [/] [#214f] [recommended] Gate investor deal publish on acceptance: admin_deal_publish_ (app/equityfundingsource/lib/admin/repo.ts:2054) + POST /api/admin/loans/:id/deal publish a loan to the investor marketplace with NO acceptance check (only 'if (!loan) throw'); any non-Funded loan becomes status='published'. So a deal can be an Available Deal while its application is not accepted (observed: investment_deal 7a0c1463 published from loan 45ddae53 status='submitted', tmo 'Application Received', with NO full_application row). Decide the business rule ('accepted before available') and add the gate before status='published' (and/or surface why a loan is ineligible to publish). Relates to the full_applications<->loans<->investment_deals decoupling.
- [/] [#cftk] RESOLVED 2026-06-20: passage btak/equityfundingsource/cloudflare_api_token is an ACTIVE token WITH D1 access — token verify=active; D1 database list + SELECT query both succeeded against account 1e7e194ff8da4ad9e40ce041aea84f76 / DB 280e1539 (the D1 /query API requires Account D1 Edit, so write scope is confirmed). .env line 7 already references this exact passage path, so #cftk is unblocked with no change needed. D1-gated investigations (#dealtab500, #cmtsec) can now run.
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
