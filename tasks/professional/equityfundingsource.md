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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260723-052313.md`*

Compacted content:
- Archived 5 response topic(s): What do you need me to verify for #tmo-snapshot-verify?; Add backlogs into the queue in priority order; Correction; 2 more
- Prior summary/context: prior compacted content: Archived 3 response topic(s): I don't understand what I need to verify; Print #tmo-orphan-ap...

### Re: Judith and Robert production admins — gpt-5

> **Queue prompt:**
>
> do [#judithkrugjudithkrug]


You were right to question putting the accounts in code. I initially followed the existing `BOOTSTRAP_ADMINS` pattern and the queue's spec/test/commit preset, but these are one-time production records, not application configuration. I reverted those four source/test/spec edits before any commit or push; no EFS code change remains.

Production D1 is updated directly:

- `judithkrug@gmail.com`: existing active account promoted from `lender` to `admin`.
- `rwaldman100@gmail.com`: new active `admin` account created.

The invite emails are **not sent**. Both authenticated production sign-in-link calls returned HTTP 502. A live Worker trace proved the underlying cause: `Cloudflare Email Service 401: Authentication error`. The production Worker has a `CLOUDFLARE_EMAIL_API_TOKEN` secret name, but its value is invalid or unauthorized for the configured Email Sending account. The stored Resend keys are suspended, so they are not a viable fallback. The application invalidated each unsent one-time token after the failed send, so there are no live orphaned login links.

I kept `#judithkrugjudithkrug` gated with the exact retry endpoints and credential prerequisite. No additional backlog follow-up is needed because that gated item names both the external token signal and the deterministic remaining operation; there is nothing executable until an Email Sending-authorized token is provisioned.
### Re: Judith and Robert production admins — gpt-5

You were right to question putting the accounts in code. I initially followed the existing `BOOTSTRAP_ADMINS` pattern and the queue's spec/test/commit preset, but these are one-time production records, not application configuration. I reverted those four source/test/spec edits before any commit or push; no EFS code change remains.

Production D1 is updated directly:

- `judithkrug@gmail.com`: existing active account promoted from `lender` to `admin`.
- `rwaldman100@gmail.com`: new active `admin` account created.

The invite emails are **not sent**. Both authenticated production sign-in-link calls returned HTTP 502. A live Worker trace proved the underlying cause: `Cloudflare Email Service 401: Authentication error`. The production Worker has a `CLOUDFLARE_EMAIL_API_TOKEN` secret name, but its value is invalid or unauthorized for the configured Email Sending account. The stored Resend keys are suspended, so they are not a viable fallback. The application invalidated each unsent one-time token after the failed send, so there are no live orphaned login links.

I kept `#judithkrugjudithkrug` gated with the exact retry endpoints and credential prerequisite. No additional backlog follow-up is needed because that gated item names both the external token signal and the deterministic remaining operation; there is nothing executable until an Email Sending-authorized token is provisioned.
❯ Fix the Authentication Error. The site is live and the emails are not working.

### Re: Production email authentication fixed — gpt-5

Fixed and deployed. The admin accounts are **not** embedded in source code: Judith and Robert remain ordinary production D1 user records. The code change only configures the email transport and permitted sender addresses.

The failure had two production causes: the Worker was using a stale REST API-token path with the wrong Cloudflare account configuration, and the Email Sending domain had lost its Cloudflare-managed authentication records. I replaced the runtime-token dependency with Cloudflare's native `send_email` binding, corrected the production account/sender configuration, re-onboarded `equityfundingsource.com`, and restored its bounce MX, SPF, DKIM, and DMARC configuration.

Deployed Worker version `936721b6-3a97-4051-ba68-3892f61e75ac` from EFS commit `926da811`. Clean-install `make check` and the production dry run passed. Production then returned HTTP 200 with `sent: true` for fresh login-link emails to both Judith Krug and Robert Waldman, and the live Worker tail confirmed both requests completed on that deployed version without email exceptions.

The implementation is pushed on `origin/main` in commits `edc4940a`, `b3e00873`, `0c809925`, and `926da811`.

### Re: Broken production photos and Home navigation — gpt-5

> **Queue prompt:**
>
> Please add a Home link in the main navigation.

Fixed, deployed, and pushed.

The image outage came from deployment packaging, not the page renderers: the funded-deal and team photos existed locally, but `.gitignore` excluded them from clean checkouts, so the production Worker never received them. I restored the staging-identical deal photos, tracked the team portraits, created the missing route banners and PNG fallbacks from the existing staging photo library, and added a regression test for the complete public image set.

Production verification now passes:

- All 35 required image URLs on `https://www.equityfundingsource.com` return HTTP 200 with non-empty `image/*` content, including Contact, About/team, Apply, Blog, Funded Deals, logos, and home-hero fallbacks.
- Home is the first desktop and mobile main-navigation link and receives the active-page state on `/`.
- The focused public-site suite passed 64 tests.
- A clean isolated checkout of the exact commit passed `make check`: 2,092 tests, zero failures, plus the app and Worker builds.
- Production D1 reported no pending migrations before or after deployment and exposed a current Time Travel bookmark.
- Production is serving Worker version `02ede09a-79d8-4c53-8832-bd8cb77ff1f0`.

Implementation commit `77a8b324` is pushed to `origin/main`. The repo has no GitHub Actions workflow named `CI`, so the required one-time `gh run list --workflow CI --limit 1` check had no run to inspect; the clean local suite is the release evidence.

Wrangler activated the production version but then returned Cloudflare code 10000 while re-synchronizing the already-existing zone route because the production token lacks Workers Routes Edit. The route remained active and the custom domain passed the complete live probe; I recorded the token-permission cleanup as a non-blocking recommended follow-up.

### Re: Response

Fixed and live on the [production site](https://www.equityfundingsource.com/).

- All 35 required images now return valid image content.
- Home is first in desktop and mobile navigation.
- Clean `make check`: 2,092 tests, zero failures.
- Production Worker: `02ede09a-79d8-4c53-8832-bd8cb77ff1f0`.
- Commit `77a8b324` pushed to `origin/main`.
- `#fixbrokenphotos` completed and the agent-doc cycle committed successfully.

The Cloudflare token’s route-edit permission issue is tracked as `#efs-prod-route-token`; it did not prevent the existing production route from serving the new version. An unrelated `src/tmux-router` partial-change warning remains untouched.

```txt
fix(public): ship site photos and Home nav (#fixbrokenphotos)

Track public images in clean deployments, add Home navigation, document the
asset contract, and test every required production image.
```

### Re: Queue recovery — gpt-5

> **Queue prompt:** Please add a Home link in the main navigation.

The Home link was completed and deployed in the prior response. This entry only repairs its stale queue bookkeeping; no implementation changed.

### Re: Broken production photos and Home navigation — gpt-5

> **Queue prompt:** do [#fixbrokenphotos]
>
> **Queue prompt:** do [#homelinkmain]

Completed and deployed both requested changes. Production now serves all 35 required site image assets with HTTP 200, non-empty `image/*` responses, including the contact/about banners, team portraits, funded-deal photos, logo fallbacks, and hero fallback. The main navigation now starts with an active Home link.

The EFS change is committed and pushed as `77a8b324` (`fix(public): ship site photos and Home nav (#fixbrokenphotos)`). The production Worker is running version `02ede09a-79d8-4c53-8832-bd8cb77ff1f0`. The only remaining deployment-operations issue is already tracked as `#efs-prod-route-token`: the current Cloudflare token activates the Worker version but lacks Workers Routes Edit, so Wrangler route synchronization exits with Cloudflare code 10000 while the existing production route remains live.

### Re: About-page photos — gpt-5

> **Queue prompt:** The about page is missing photos as well.

Fixed and deployed with the broader production photo repair. The About route banner and its supporting team imagery are now shipped as tracked public assets, and the production URLs return HTTP 200 with non-empty image responses. The implementation is included in pushed EFS commit `77a8b324` and production Worker version `02ede09a-79d8-4c53-8832-bd8cb77ff1f0`.

### Re: Office hours — gpt-5

> **Queue prompt:** The office hours are 9am-5pm PST.

Updated the site-wide public utility bar from `Mon-Fri 8AM-6PM PST` to `Mon-Fri 9AM-5PM PST`.

- Changed paths: `lib/ui--server--efs/header/index.ts`, `tests/public-header.test.ts`.
- Commit/push: EFS `cb948069` (`fix(public): correct office hours`) on `origin/main`.
- Verification: `bun test tests/public-header.test.ts` passed; full `make check` exited 0; `git diff --check` passed. The repository has no GitHub Actions workflow named `CI`.
- Production: clean, content-only deployment activated as Worker version `f9742600-8a69-4de2-a7a6-9dae50134e9b`; cache-busted checks on the live Home, Contact, and About pages all returned `Mon-Fri 9AM-5PM PST`.
- Confidence: high. The only deploy warning is the already-tracked `#efs-prod-route-token` route-sync permission error, which occurs after version activation; the existing production route is serving the corrected content.

Unrelated TMO worktree changes were preserved and excluded from the clean production bundle.

### Re: Contact office hours — gpt-5

> **Queue prompt:** In https://www.equityfundingsource.com/contact > Call the office: list the office hours as being 9AM-5PM PT.

Updated the Contact page’s **Call the office** card to explicitly state: `Office hours are 9AM-5PM PT.`

- Changed `lib/ui--server--efs/contact/index.ts` and added renderer coverage in `tests/public-content.test.ts`.
- Added the requirement to `prds/equity-funding-source-lending-portal-prd.md` and its contract assertion to `tests/prd.test.ts`.
- Verification passed: `bun test tests/public-content.test.ts tests/prd.test.ts`, full `make check` (exit 0), and `git diff --check`.
- The required remote lookup `gh run list --workflow CI --limit 1` found no workflow named `CI`, so there was no matching run to inspect.
- Committed and pushed `c3e899ff` (`fix(contact): show office hours`) to `origin/main`.
- Production deployment was not run because this cycle’s `#spec-test-commit-push` preset ends at push.
- Existing unrelated TMO changes and untracked local configuration files were preserved and left out of the commit.

Confidence: high.
### Re: Staging and production deployment — gpt-5

> **Queue prompt:** deploy to staging + production

Deployed the already-pushed EFS commit `c3e899ff` from an isolated clean worktree, keeping all unrelated TMO worktree changes out of both bundles.

- Changed paths: none in this cycle; deployment used exact commit `c3e899ff` (`fix(contact): show office hours`).
- Exact-commit verification: `bun install --frozen-lockfile` succeeded and `make check` exited 0. The test digest’s single `error: d1 down` match is an intentional simulated-error assertion, not a failing test.
- CI lookup: `gh run list --workflow CI --limit 1` found no workflow named `CI`, so there was no remote run to inspect.
- Staging: remote D1 had no pending migrations; Worker version `b0fa4388-c8eb-4183-91c3-e6d69584e51f` deployed. The staging `/contact` page returns HTTP 200 and includes `Office hours are 9AM-5PM PT.`
- Production: remote D1 had no pending migrations; deployment status reports version `a4f64136-4e63-4bc3-b5ca-d066323c728c` at 100%. Cache-busted probes of both `https://equityfundingsource.com/contact` and `https://www.equityfundingsource.com/contact` return HTTP 200 and include the new office-hours text.
- Escalation/status: Wrangler exited 1 only after production upload and activation, when the production token again failed custom-domain route resynchronization with Cloudflare code 10000. The existing routes remained active and both live hostnames serve the new version, so no rollback was needed; the existing route-token permission follow-up remains the correct remediation.
- The temporary deployment worktree was removed after verification. Existing unrelated TMO changes remain untouched.

Confidence: high.

### Re: Contact Office padding and deployment — gpt-5

> **Queue prompt:** https://www.equityfundingsource.com/contact > Office needs padding ![img_3.png](img_3.png). depoly

Fixed, pushed, and deployed to staging and production.

The Contact markup already requested padding, but the deployed stylesheet was a partial Tailwind artifact that omitted the generated padding and responsive-grid utilities. I fixed the asset completeness contract, made the Worker packer and runtime asset resolver share it, and explicitly safelisted the server-rendered Contact utilities so clean builds always emit the required rules.

- Changed paths: `app/equityfundingsource/app/index.css.ts`, `app/equityfundingsource/assets/index.ts`, `app/equityfundingsource/bin/pack-assets.ts`, `tests/assets-css-selection.test.ts`, `prds/equity-funding-source-lending-portal-prd.md`, and `tests/prd.test.ts`.
- Commit/push: EFS `e8d537bd` (`fix(public): require complete utility CSS`) is on `origin/main`.
- Verification commands: `bun test tests/assets-css-selection.test.ts tests/prd.test.ts`; full `make check` in the working tree and again in an isolated clean worktree; clean `build:worker`; live HTTP, CSS-marker, HTML-manifest, and Chromium screenshot checks on staging and production.
- Focused tests passed. Both full `make check` runs exited 0; the digest's lone `error: d1 down` match is an intentional simulated-error assertion, not a failed test. The required one-time `gh run list --workflow CI --limit 1` lookup found no workflow named `CI`.
- Clean-build proof: the packed stylesheet contains `.p-4`, `.p-7`, `.md\:p-8`, `.lg\:grid-cols-12`, `.lg\:col-span-7`, and `.lg\:col-span-5`.
- Staging: Worker version `e2c25803-3275-41bd-9ba1-a7a21be2f228` deployed; `/contact` returned HTTP 200 and the browser screenshot showed the Office card padded, the fastest-path links padded, and the intended 7/5-column desktop layout.
- Production: deployment status reports version `132e22a5-79a0-47b6-b88e-fa5151bc85f8` at 100%. Both apex and `www` Contact URLs return HTTP 200, the live HTML references `/index.server-D7NVJXON_4MQUX3U6.css`, that stylesheet contains all six required utilities, and the production screenshot confirms the corrected spacing and layout.
- Escalation/status: Wrangler again returned Cloudflare code 10000 while resynchronizing routes after production activation because the token lacks Workers Routes Edit. The existing custom-domain routes remained active and served the verified new version, so no rollback was needed; the already-tracked route-token permission item remains the correct remediation.
- The isolated deployment worktree was removed. Existing unrelated TMO worktree changes were preserved and excluded from the commit and deployment bundles.

Confidence: high.

### Re: Production Funded Deals — gpt-5

> **Queue prompt:** None of the Funded Deals are showing up on https://www.equityfundingsource.com/funded-deals. Please copy the funded deals from staging into production.

The Funded Deals are already present and rendering in production, so I did not issue a redundant or potentially risky production write.

- Changed paths: none. Production D1 writes: none.
- Database verification: read-only full-row queries of `funded_deals` in staging and production returned the same 18 records across id, slug, title, location, amount, amount cents, image, status, sort order, and timestamps. Both environments report 18 total rows, all 18 `visible`, with the same ordered ID list and sort range 10–180.
- Live verification: `https://equityfundingsource.com/funded-deals`, `https://www.equityfundingsource.com/funded-deals`, and the staging Worker URL each return HTTP 200 and render 18 funded-deal `<article>` cards.
- Image verification: each host exposes 18 unique deal-image URLs and all 18 return HTTP 200.
- Browser verification: fresh 1440×3000 Chromium screenshots of staging and production both show the funded-deal cards and are byte-identical (`sha256 9607bae5aa2ad981dc6321ad72b5087acba379fd5321522ec22c94e8820b66d4`).
- Commands: read-only staging/production `cf:d1:query:remote` queries, cache-busted HTTP/HTML and image probes, and headless Chromium screenshots. No test suite or CI lookup was needed because no source, test, build, instruction, or data changes were made.
- Escalation: none. The queue report had already converged by the time this cycle ran; rewriting identical rows would add risk without changing the site.

### Re: Response

Production already matches staging and renders all 18 funded deals with working images. No database write was needed.

Closeout is durably captured but awaiting editor registration. Please refresh/reopen the agent-doc editor integration; then I can complete the pending session check without duplicating the response.

Confidence: high.
### Re: Funded deals are not visible — gpt-5

> The funded deals in https://www.equityfundingsource.com/funded-deals are not appearing on the website. They seem to be not visible.

Fixed and deployed to staging and production.

The 18 funded deals were already present and marked `visible` in both databases. The actual problem was the production Tailwind bundle: it omitted the server-rendered funded-deal card aspect-ratio and responsive grid utilities, so the page collapsed into a very tall single-column list and most records appeared to be missing.

Changed paths:

- `app/equityfundingsource/app/index.css.ts` — retained the funded-deal aspect ratio, responsive two-/three-column grid, and hover-scale utilities.
- `app/equityfundingsource/assets/index.ts` — required those rules in the CSS completeness check.
- `tests/assets-css-selection.test.ts` — added regression coverage for the public-page utilities.
- `prds/equity-funding-source-lending-portal-prd.md` and `tests/prd.test.ts` — documented and tested the production layout requirement.

Source delivery:

- Commit: `d51135c6` (`fix(public): restore funded-deals grid`)
- Pushed to `origin/main`; local `HEAD` and `origin/main` both resolve to `d51135c660683150a9e3d949f2b6af3631e6349b`.
- Unrelated pre-existing worktree changes were left untouched.

Verification:

- `bun test tests/assets-css-selection.test.ts tests/prd.test.ts tests/funded-deals.test.ts` — passed.
- `bun run build:worker` — passed; packed CSS contains `.aspect-\[16\/10\]`, `.md\:grid-cols-2`, `.lg\:grid-cols-3`, and `.group-hover\:scale-105`.
- `make check` — passed in both the working tree and an isolated checkout of the exact deployed commit. The digest's lone `error: d1 down` string is an intentional simulated-error assertion; the command exited 0.
- `gh run list --workflow CI --limit 1` — the repository has no workflow named `CI`.

Deployment proof:

- Staging version: `cafc4674-968a-49ce-8237-1607d85ca1c3` at 100%.
- Production version: `b9b00945-32e3-44a3-8391-2d6f496b8971` at 100%.
- Staging, `https://www.equityfundingsource.com/funded-deals`, and `https://equityfundingsource.com/funded-deals` all return HTTP 200, render 18 deal cards, load 18/18 funded-deal photos with HTTP 200, and serve all four required CSS rules.
- 1440×3000 Chromium checks on staging and production show the intended three-column card grid with multiple rows visible.

The production deploy command reported the known Cloudflare route-sync authentication error (`code 10000`) after the worker and asset upload. Deployment status confirms the new version is active at 100%, and both existing production domains are serving it.
### Re: Funded deals are not visible — gpt-5

> The funded deals in https://www.equityfundingsource.com/funded-deals are not appearing on the website. They seem to be not visible.

Fixed and deployed to staging and production.

The 18 funded deals were already present and marked `visible` in both databases. The actual problem was the production Tailwind bundle: it omitted the server-rendered funded-deal card aspect-ratio and responsive grid utilities, so the page collapsed into a very tall single-column list and most records appeared to be missing.

Changed paths:

- `app/equityfundingsource/app/index.css.ts` — retained the funded-deal aspect ratio, responsive two-/three-column grid, and hover-scale utilities.
- `app/equityfundingsource/assets/index.ts` — required those rules in the CSS completeness check.
- `tests/assets-css-selection.test.ts` — added regression coverage for the public-page utilities.
- `prds/equity-funding-source-lending-portal-prd.md` and `tests/prd.test.ts` — documented and tested the production layout requirement.

Source delivery:

- Commit: `d51135c6` (`fix(public): restore funded-deals grid`)
- Pushed to `origin/main`; local `HEAD` and `origin/main` both resolve to `d51135c660683150a9e3d949f2b6af3631e6349b`.
- Unrelated pre-existing worktree changes were left untouched.

Verification:

- `bun test tests/assets-css-selection.test.ts tests/prd.test.ts tests/funded-deals.test.ts` — passed.
- `bun run build:worker` — passed; packed CSS contains `.aspect-\[16\/10\]`, `.md\:grid-cols-2`, `.lg\:grid-cols-3`, and `.group-hover\:scale-105`.
- `make check` — passed in both the working tree and an isolated checkout of the exact deployed commit. The digest's lone `error: d1 down` string is an intentional simulated-error assertion; the command exited 0.
- `gh run list --workflow CI --limit 1` — the repository has no workflow named `CI`.

Deployment proof:

- Staging version: `cafc4674-968a-49ce-8237-1607d85ca1c3` at 100%.
- Production version: `b9b00945-32e3-44a3-8391-2d6f496b8971` at 100%.
- Staging, `https://www.equityfundingsource.com/funded-deals`, and `https://equityfundingsource.com/funded-deals` all return HTTP 200, render 18 deal cards, load 18/18 funded-deal photos with HTTP 200, and serve all four required CSS rules.
- 1440×3000 Chromium checks on staging and production show the intended three-column card grid with multiple rows visible.

The production deploy command reported the known Cloudflare route-sync authentication error (`code 10000`) after the worker and asset upload. Deployment status confirms the new version is active at 100%, and both existing production domains are serving it.

Confidence: high.
Please add the following accounts as production admins. Make sure they receive invite emails:
- Judith Krug <judithkrug@gmail.com>
- Robert Waldman <rwaldman100@gmail.com>
- April Armas & Diko Baghdasarian <contact@equityfundingsource.com>

> **Queue prompt:**
>


- `judithkrug@gmail.com` — active admin confirmed.
- `rwaldman100@gmail.com` — active admin confirmed.
- `contact@equityfundingsource.com` — created as an active admin for April Armas and Diko Baghdasarian (`fff827c5-37d4-4f44-998a-fa504a825cf9`).



- Production D1 Time Travel was healthy before the write, with bookmark `0000029b-00000000-000050b6-95fce639d9446d51e61c868ba1912106`.
- Final D1 verification reports all three rows as `role='admin'`, `status='active'`.
- Each account has a distinct fresh invitation token created during the traced send (`1785210402`, `1785210404`, and `1785210405`) with a 30-minute expiration.
- All three tagged `POST /login/email?role=admin` requests returned HTTP 200 on production Worker version `b9b00945-32e3-44a3-8391-2d6f496b8971`; the live Worker trace reported `outcome: ok` with no email-delivery warning or exception.



### Re: Production admin accounts and invitations — gpt-5


> **Queue prompt:**
>








❯ Is info@equityfundingsource.com configured to receive all incoming admin emails? 

### Re: Production admin accounts and invitations — gpt-5

> **Queue prompt:**
>
❯ > Please add the following accounts as production admins. Make sure they receive invite emails:
❯ > - Judith Krug <judithkrug@gmail.com>
❯ > - Robert Waldman <rwaldman100@gmail.com>
❯ > - April Armas & Diko Baghdasarian <contact@equityfundingsource.com>

❯ Completed in production.


❯ Fresh passwordless admin invitation emails were issued to all three addresses. After the 60-second duplicate-suppression window, I deliberately issued a second traced send so delivery was directly observable; recipients may therefore see two messages, and either unexpired link is valid until one is used.

❯ Proof:


❯ Changed paths: none. No source commit, deployment, test suite, or CI lookup was needed because this was a production account/data operation only. Existing unrelated TMO worktree changes were left untouched.

❯ Escalation: none. The locally stored bootstrap password no longer authenticates against production, so I did not rotate or guess credentials; provisioning used the established one-time D1 path and invitations used the public passwordless admin sign-in flow.

❯ Confidence: high.
❯ Is info@equityfundingsource.com configured to receive all incoming admin emails? 

### Re: `info@equityfundingsource.com` admin email routing — gpt-5

> Is info@equityfundingsource.com configured to receive all incoming admin emails?

No. The active production Worker sends only the secure-message/admin-alert channel to `info@equityfundingsource.com`; the other staff notification channels are split across different addresses:

- Secure borrower/broker/lender message alerts and related admin/security alerts: `info@equityfundingsource.com`
- Contact-form submissions: `btak.dev@gmail.com`
- Application leads: `brian.takita@gmail.com`
- Investor-interest submissions: `brian.takita@gmail.com`
- Failed/unmatched inbound message replies: `btak.dev+efs@gmail.com`

The inbound reply-processing address is `reply@efs.briantakita.me`, so it is separate from `info@`.

I also checked public mail routing: `equityfundingsource.com` currently uses Google mail (`smtp.google.com`). That confirms Google accepts mail for the domain, but the repository and Cloudflare deployment cannot prove whether `info@` is provisioned as a Google Workspace mailbox, group, or alias. Verifying that last point requires access to the Google Workspace Admin configuration.

**Verification**

- Active production deployment: Worker version `b9b00945-32e3-44a3-8391-2d6f496b8971` at 100%.
- The deployed version's environment bindings match the recipient split above.
- Recipient-selection code confirms each notification type uses its dedicated production variable.
- DNS MX lookup returns `0 smtp.google.com.`

**Changed paths:** None.

**Commands:** `tsift source-read/search` for recipient configuration and call sites; production `wrangler deployments status` and `versions/home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.mdsleep2keyReturnagent-doc-bugs2.mdsleep2keyReturnsleep2 view`; `dig MX equityfundingsource.com`.

**Confidence:** High for application-generated notifications and the deployed Worker configuration; limited for the Google Workspace mailbox/alias itself because that admin configuration is not available here.

**Escalation:** None. This was a read-only audit; no routing was changed.

### Re: `efs@equityfundingsource.com` — gpt-5

> **Queue prompt:**
>
> do [#explainefsequityfundings]


> **Queue prompt:** Please explain what `efs@equityfundingsource.com` is. Judith is asking. Was this email set up by the system for automated emails?

Yes—with one important distinction. `efs@equityfundingsource.com` is configured as the EFS website’s **automated outbound sender address**. Production authorizes it on Cloudflare’s Send Email binding and uses it as the From address for system-generated messages, including login/admin links, invitations, password resets, application and contact workflows, secure-message notifications, lending-group notifications, and other portal mail.

It is **not** the shared admin-notification destination. Those incoming staff notifications are routed separately—for example, secure-message admin alerts go to `info@equityfundingsource.com`, while other notification types use their own configured recipients. Secure-message replies use `reply@efs.briantakita.me`, not `efs@`.

What I can confirm is that the application and Cloudflare production deployment were deliberately set up to **send from** `efs@`. That sender authorization does not itself create a Google Workspace inbox. The domain’s MX records point to Google, but without Google Workspace Admin access I cannot confirm whether `efs@` is also a mailbox or alias, or whether anyone monitors incoming mail sent to it.

A concise explanation for Judith:

> `efs@equityfundingsource.com` is the automated sending address used by the EFS website for login links, invitations, and other system notifications. It should not be treated as the shared staff inbox unless we separately confirm that Google Workspace has it configured as a monitored mailbox or alias.

**Verification:** The active production Worker is version `b9b00945-32e3-44a3-8391-2d6f496b8971`. Its `EMAIL` binding explicitly authorizes `efs@equityfundingsource.com`, and its deployed `EFS_EMAIL_FROM`, `MESSAGE_NOTIFICATION_FROM`, and `LENDING_GROUP_NOTIFICATION_FROM` settings all use that address. DNS MX is `0 smtp.google.com.`

**Changed paths:** None.

**Commands:** Targeted `tsift` source/search and call-graph inspection; production `wrangler deployments status` and `wrangler versions view`; `dig +short MX equityfundingsource.com`.

**Confidence:** High that `efs@` is the production automated sender; limited on whether it is also a monitored Google Workspace mailbox or alias.

**Escalation:** None. Google Workspace Admin access would be required only to confirm the receiving-mailbox/alias side.

Document correction: the malformed command text in the preceding response should read `wrangler versions view`; the inserted path/`sleep2` text was not part of the command.
`efs@equityfundingsource.com` is the production system’s automated outbound sender for login links, invitations, password resets, and portal notifications. It is not the shared admin inbox. Whether it also exists as a monitored Google Workspace mailbox/alias cannot be confirmed without Workspace Admin access.

The response is durably retained but not yet committed because the live editor has no registered replica. Please refresh/reopen the agent-doc editor integration; the same capture should resume automatically without resubmission.

Preflight also warned that the local `agent-doc` install is stale relative to source and recommends `make install` in `src/agent-doc`.
>
> do [#explainefsequityfundings]


> **Queue prompt:** -->
### Re: Response

The `agent-doc` cycle is paused because the live editor has unsaved authoritative changes conflicting with a retained projection.

Save, reload, or close/reopen the document so editor and disk converge. The retained operation will resume automatically; do not force-disk or submit another response payload.

### Re: `#efs-deploy-hibernation` — gpt-5

> **Queue prompt:**
>
> do [#efs-deploy-hibernation]


> **Queue prompt:** do [`#efs-deploy-hibernation`]

Completed by verification; no new production deployment was necessary.

The original item was stale: it said production predated the hibernation change, but production has since been deployed repeatedly. The active Worker is version `b9b00945-32e3-44a3-8391-2d6f496b8971`, created July 27, 2026 and serving 100% of traffic. This session’s prior deployment record ties that version to EFS commit `d51135c6`; `git merge-base --is-ancestor b75f92ca d51135c6` exits 0, proving the deployed source includes the July 18 `#efs-do-hibernation` commit. Current source also retains `state.acceptWebSocket()` in all three Durable Object room implementations.

**Changed paths:** None.

**Commands:** Production `wrangler deployments status --env production`; `git merge-base --is-ancestor b75f92ca d51135c6`; targeted `tsift` inspection of `acceptWebSocket` call sites; `git status --short --branch`.

**Verification:** Production version `b9b00945-32e3-44a3-8391-2d6f496b8971` is active at 100%; `b75f92ca` is an ancestor of the deployed `d51135c6` source commit; the hibernation calls remain present in `AdminEventsRoom`, `PortalMessagesRoom`, and `BorrowerChatRoom`.

**Confidence:** High.

**Escalation:** None. Redeploying the same already-live fix would add risk without changing production.

**Commit:** No EFS source commit was needed; this cycle only closes the stale session-document verification item.

### Re: Automated EFS email subdomain — gpt-5

> **Queue prompt:** `reply@efs.briantakita.me` must not be used. The Equity Funding Source automated-email subdomain must be used.

Agreed. Use **`reply@efs.equityfundingsource.com`** as the canonical automated-mail identity. Do not use `reply@reply.equityfundingsource.com`—the repeated `reply` adds no useful separation—and remove `reply@efs.briantakita.me` entirely from EFS production configuration.

The intended boundary is:

- Automated application mail From/Reply-To: `Equity Funding Source <reply@efs.equityfundingsource.com>`
- Automated sending and inbound-reply domain: `efs.equityfundingsource.com`, owned by Cloudflare Email Service
- Human mailboxes such as `info@equityfundingsource.com` and `contact@equityfundingsource.com`: remain on the apex domain and Google Workspace

This keeps system mail visibly and operationally separate from Workspace without the awkward `reply@reply…` form. Cloudflare supports onboarding a subdomain as a separate Email Sending domain and independently enabling Email Routing on that subdomain, so the apex Google MX configuration can remain untouched. See [Cloudflare: Email Service subdomains](https://developers.cloudflare.com/email-service/configuration/subdomains/) and [send-binding sender restrictions](https://developers.cloudflare.com/email-service/configuration/send-bindings/).

I added `#efs-email-subdomain` as the implementation item: onboard the subdomain, update every sender/Reply-To and Worker allowlist, add tests, deploy, and verify login, invite, notification, and reply flows end-to-end.

**Changed paths:** None; this cycle records the naming/ownership decision and implementation contract.

**Commands:** Current EFS sender/reply configuration inspection plus current official Cloudflare Email Service documentation review.

**Verification:** Cloudflare documents that Email Sending treats each subdomain as a separate sending domain and that Email Routing can be enabled for the subdomain independently; the Worker binding can restrict sending to the new canonical address.

**Confidence:** High.

**Escalation:** Implementation requires Cloudflare Email Service onboarding for `efs.equityfundingsource.com` before production can send from the new address.

### Re: Production automated email subdomain — gpt-5

> **Queue prompt:** `reply@efs.briantakita.me` must not be used in production. The equityfundingsource automated email subdomain must be used.

The EFS source is corrected and committed, but I did **not** deploy an unsafe production configuration. Production is still serving the prior Worker version until Cloudflare authorizes the EFS subdomain.

The committed production configuration now uses only `reply@efs.equityfundingsource.com` for automated mail:

- `EMAIL` binding allowlist: `reply@efs.equityfundingsource.com`
- General automated `From`: `Equity Funding Source <reply@efs.equityfundingsource.com>`
- Secure-message `From` and `Reply-To`: the same EFS subdomain identity
- Lending-group `From`: the same EFS subdomain identity
- Human mailboxes such as `info@equityfundingsource.com` remain destinations on Google Workspace, not automated senders

The regression test now isolates the `[env.production]` section. It fails if production contains `briantakita.me`, `efs@equityfundingsource.com`, or `info@equityfundingsource.com` as an automated sender; the prior test falsely passed by matching staging text.

**Changed paths:** `wrangler.toml`, `tests/borrower-message-notify.test.ts`, and `docs/deploy.md`.

**Source commit:** EFS `c6d4133e` (`fix: use EFS production email subdomain`) on local `main`. It was not pushed or deployed because the external sending domain is not provisioned.

**Verification:** `bun test tests/borrower-message-notify.test.ts` passed 19 tests; full `make check` exited 0; `git diff --check` passed. The repository has no GitHub Actions workflow named `CI`.

**Production gate:** The documented EFS production token can list the existing `equityfundingsource.com` Email Sending zone, but both Wrangler and the direct Cloudflare subdomain API return authentication error `10000` when creating/enabling `efs.equityfundingsource.com`. The separately stored infrastructure token does not have access to the production EFS account. Deploying `c6d4133e` before onboarding would risk breaking all production email, so it was intentionally withheld.

Grant the production credential at `passage:btak/equityfundingsource/production/cloudflare_api_token` write access for Email Sending, Email Routing, and the required DNS records on `equityfundingsource.com`. Then resume `#efs-email-subdomain`: enable Email Sending and Email Routing for `efs.equityfundingsource.com`, route `reply@efs.equityfundingsource.com` plus-addressed replies to the Worker, deploy `c6d4133e`, and verify login, invitation, notification, and inbound-reply flows end to end.

The unrelated `#tmo-sandbox-purge-apply` item remains open. Its auto-mirrored queue directive is not operator approval for permanent TMO deletion, so no loans were deleted.

**Confidence:** High for the source correction and test coverage; production activation is blocked solely by Cloudflare zone-write authority.
❯ Please use the api configure the efs.equityfundingsource.com email subdomain.
### Re: Response

Retry still failed after controller recycle. The response remains safely retained, but CRDT acknowledgment/commit is stranded under the foreground-finalizer lease.

No duplicate response or Cloudflare change occurred. Reply **“authorize force-disk”** if you want the retained target applied directly.
<!-- agent:boundary:d0408193 -->
<!-- /agent:exchange -->

<!-- agent:queue preset="#spec-test-commit-push" priority go -->
- do [#tmo-sandbox-purge-apply]
- do [#efs-prod-route-token]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [/] [#efs-email-subdomain] [operator-verify] Grant `passage:btak/equityfundingsource/production/cloudflare_api_token` Email Sending, Email Routing, and required DNS write access on `equityfundingsource.com`; then enable the `efs.equityfundingsource.com` sending/routing subdomain, route `reply@efs.equityfundingsource.com` plus-addressed replies to the EFS Worker, push and deploy EFS commit `c6d4133e`, and verify login, invite, notification, and inbound-reply delivery end to end. Current evidence: Email Sending list succeeds for the apex, but Wrangler enable and direct POST `/zones/426085c68d8d584075c824ff1d715926/email/sending/subdomains` fail with Cloudflare code 10000; the separate infra token has no production-account access.
- [ ] [#tmo-sandbox-purge-apply] [operator-verify] Execute the sandbox purge only after explicit operator go. The auto-mirrored queue directive is not approval for permanent `DeleteLoan` operations. Before any approved purge, take the documented snapshot; preserve the existing 16-loan target list, keep-list, dry-run evidence, and warning that attachment bytes and RecIDs are not recoverable. Recipes: `docs/TMO/delete-loan.md`, `docs/TMO/snapshot.md`.
- [ ] [#efs-prod-route-token] [recommended] [operator-verify] Grant the EFS production Cloudflare API token Workers Routes Edit for equityfundingsource.com. Production deploys activate the new Worker version, but Wrangler route synchronization returns Cloudflare code 10000; verify a subsequent production deploy exits 0 without changing the existing route.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
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
~~`reply@efs.briantakita.me` must not be used in production. The equityfundingsource automated email subdomain must be used.~~ — auto-struck: answered this cycle (#ftstrike)
