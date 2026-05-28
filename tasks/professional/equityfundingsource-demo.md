---
agent_doc_session: 25032905-09d1-468a-9ffd-387fd2ee6580
agent_doc_format: template
agent_doc_write: crdt
agent: codex
claude_model: opus
codex_args: -s danger-full-access
links:
  - ./judith-krug.md
  - ./robert-waldman.md
  - ../personal/robert-ward.md
  - ./bob-remeika.md
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md. DO NOT IMPLEMENT THE FIX.
  '#commit-push': commit all uncommitted files + push in claudescore-plugin, claudescore.com, and session-share
  '#commit-push-deploy': commit all uncommitted files + push in claudescore-plugin, claudescore.com, and session-share. deploy
  '#mermaid': Include explanatory mermaid diagram(s).
  '#spec-test-commit-push': update spec + tests. commit + push
  '#spec-test-commit-push-deploy': update spec + tests. commit + push + deploy
queue_active: false
---

## Status

<!-- agent:status patch=replace -->

Transferred EFS demo context: demo from workers.dev, show /apply + admin application-leads + role portal entry points, and frame next build around TMO LOS intake/status/docs/cache with custom domain, Resend, Google Workspace, TMO access, Dropbox, and legal approvals as demo asks.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
❯ What are #next-steps for the demo with judith-krug?
### Re: Judith demo next steps — gpt-5

For the Judith demo, keep the agenda narrow: show the working EFS portal layer, then use the TMO LOS discussion to explain where the next integration/replacement work starts. Do not make this a broad TMO-servicing replacement pitch; EFS is using TMO LOS/API, so the near-term story is intake, status, documents, cache, and staff-reviewed writeback.

#### Demo Objective

The demo should answer three questions for Judith:

1. **Can she see real progress?** Show the live-ish workers.dev site, `/apply`, admin lead review, and role-specific portal entry points.
2. **Does the workflow match her business?** Borrower applies, broker submits/refers, admin reviews, investors see approved deals, and TMO remains the LOS back office while EFS owns the branded workflow.
3. **What decisions unblock the next phase?** Domain/email/Google/TMO/Dropbox/legal approvals, then broker backend and TMO LOS inventory.

#### Before The Demo

- Use `https://equityfundingsource.brian-takita.workers.dev` as the demo URL unless custom-domain cutover is explicitly approved before the call.
- Smoke-test the public path: home, funded deals, `/apply`, contact, and footer email.
- Smoke-test the admin path: Cloudflare Access login, application lead list, lead detail/update, and staff notification state.
- Prepare one clean sample lead and one or two portal examples. If any route still has fixture/static data, say that directly instead of letting Judith infer it is TMO-backed.
- Have screenshots ready as fallback in case Cloudflare Access, network, or login interrupts the live demo.

#### Demo Flow

1. **Public site:** brand, current navigation, funded-deal/public trust surface, and `/apply`.
2. **Application lead:** submit or show a sample prequalification lead and explain that it lands in D1 for admin review.
3. **Admin review:** show how staff can review status, lead state, and next action without relying on live TMO during page load.
4. **Portal overview:** show borrower, broker, and investor surfaces at a high level, with clear labels for what is production-backed versus planned/TMO-backed.
5. **TMO LOS boundary:** explain that the next replacement layer is TMO Loan Origination: application reference, loan number, statuses, documents/attachments, conditions, funding, and sync cache.
6. **Decision close:** ask for the approvals/access needed to finish the next build increment.

#### Ask Judith / Robert For These Decisions

- Confirm whether the workers.dev demo can remain the temporary production URL until domain cutover is intentionally scheduled.
- Approve an EFS-owned sending domain for Resend so lead notifications stop using testing-mode sender/recipient values.
- Provide Google Workspace admin credentials when they want real Google SSO/calendar/email integration.
- Restore or provide TMO LOS access for inventory: products, custom fields, statuses, condition phases, document templates, screenshots/exports, and sample loans.
- Confirm Dropbox folder structure, naming conventions, duplicate-file policy, and sanitized sample documents.
- Review/approve funded-deal images, copy, lien-position wording, and legal/disclosure language.

#### Recommended Next Build Order After Demo

1. Finish demo hardening and capture Judith's corrections.
2. Inventory actual TMO LOS usage (`#xt4d`) before implementing more TMO writeback assumptions.
3. Finish broker backend so the existing broker UI does not overstate functionality.
4. Build document checklist / upload-token / admin review around real EFS document workflows.
5. Add staff-approved TMO LOS outbox writes and cache refreshes.
6. Only then consider broader replacement pieces such as condition automation or document generation.

Closeout evidence: changed path is this session document only. Commands used included `tsift status`, `tsift status --fix`, `agent-doc --version`, `agent-doc preflight/read/plan`, linked-doc `sed` reads, `tsift --envelope search`, and this `agent-doc finalize`. Verification is source-review only; no code changed and no tests were run. Confidence is high on the demo agenda and medium on exact route/data readiness until the pre-demo smoke-test backlog item is completed. No escalation was used.

### Re: closeout repair — gpt-5

The substantive demo next-steps answer is captured above, and the concrete pre-demo follow-ups are already in the backlog. This note only reopens the binary-owned closeout after the live document buffer normalized status/backlog content after the first commit.

### Re: transferred EFS demo material — gpt-5

Transferred the demo-relevant material from the main EFS document into this demo document as working demo context.

#### Current Demo State

- Demo URL remains `https://equityfundingsource.brian-takita.workers.dev` until custom-domain cutover is intentionally approved.
- `/apply` public prequalification is deployed and writes D1-backed `application_leads` for admin review.
- Admin review is available behind Cloudflare Access on `/login/access-callback` using `ACCESS_TEAM_DOMAIN` / `ACCESS_AUD`.
- Logged-out navigation currently exposes Borrower Login, Apply for a Loan, Broker Portal, and Investor Portal, with Broker Portal placed between Apply and Investor.
- Public site/footer email is `info@equityfundingsource.com`.
- Application lead notifications still use Resend testing-mode values until an EFS-owned sending domain is verified.
- Custom-domain cutover is deferred; do not demo `equityfundingsource.com` as the live app URL unless that changes before the call.

#### Demo Framing To Carry Forward

The TMO story should stay focused on Loan Origination, not loan servicing replacement. The relevant next build surface is: application intake, normalized statuses, borrower/broker documents, condition/checklist workflow, local D1/R2 cache, and staff-reviewed TMO LOS writeback. The broad servicing/fund-accounting parts of TMO should be treated as out of scope unless Judith explicitly expands the project.

#### Demo Asks

- Confirm workers.dev is acceptable as the temporary demo/production URL.
- Approve custom-domain timing separately from the demo.
- Verify an EFS-owned Resend sending domain.
- Provide Google Workspace admin credentials for real Google SSO/calendar/email integration.
- Restore or provide TMO LOS access/screenshots/exports for products, custom fields, statuses, conditions, templates, and sample loans.
- Confirm Dropbox folder structure and sanitized sample document flow.
- Review funded-deal images, copy, lien-position wording, and legal/disclosure language.

Existing demo backlog items already cover the concrete pre-demo work, so I did not add duplicates here.

<!-- agent:boundary:769e351a -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#pctc] Before Judith demo, smoke-test the workers.dev demo path: public home, /apply, borrower login, broker portal entry, investor portal entry, and admin application-leads review.
- [ ] [#m83y] Prepare a short Judith demo script: public site -> Apply lead -> admin lead review -> portal role overview -> TMO LOS cache/replacement boundary -> decisions needed.
- [ ] [#t447] Create or verify demo-safe credentials/data for Judith: Cloudflare Access admin path, borrower/broker/investor examples, realistic application lead, and screenshots as fallback.
- [ ] [#hz0m] Ask Judith/Robert for demo decisions: custom-domain timing, EFS Resend sending domain, Google Workspace admin credentials, TMO LOS access/screenshots, Dropbox folder/sample docs, and funded-deal/legal approval.
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->
