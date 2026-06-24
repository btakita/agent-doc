---
agent_doc_session: 62fe1f41-886c-4f3d-b6fa-48d46106ec27
agent: opencode
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

complete
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260624-210628.md`*

Compacted content:
❯ - Archived 13 response topic(s): Remove Lead Dialog Open Page And Deploy; Lead Dialog Next Step And Deploy; Deploy Queue Residue Cleanup; 10 more
❯ - Prior summary/context: ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b...

### Re: Open Message Conversation Modal And Deploy — gpt-5

> **Queue prompt:**
> https://equityfundingsource.brian-takita.workers.dev/admin/pipeline?dialog=lead&dialog_lead_id=c907d851-8870-49aa-a6fc-66d393868ce1 > Open Message Conversation should open the modal...not the page.
>
> **Queue prompt:**
> deploy

Implemented, pushed, and deployed.

Changed paths in `src/equityfundingsource-dev`: `lib/ui--browser--efs/portal/admin/messages.tsx`, `lib/ui--browser--efs/portal/shared/lead_pipeline_controls.tsx`, `lib/ui--browser--efs/portal/shared/ui.tsx`, `tests/admin-application-lead-detail.test.ts`, `tests/admin-pipeline.test.ts`, `prds/equity-funding-source-lending-portal-prd.md`, and `prds/investor-pipeline-admin.md`.

What changed: `Open message conversation` is now an in-page button that dispatches a shared admin-messages open event instead of navigating to `/admin/messages`. The global `AdminMessagesLauncher` listens for that event, opens the existing Messages modal, switches to the borrower tab, and seeds the borrower search from the lead email so the conversation opens inside the modal while the Deal Pipeline lead dialog remains on the page.

Commits: EFS submodule commit `a2a2213 fix(admin): open lead messages in modal` was pushed to `origin/main`. Parent repo gitlink commit `e87bd7b72 chore(efs): bump lead messages modal submodule` was committed locally only; I did not push parent `main` because the parent repo is dirty with unrelated work.

Verification: focused `bun test tests/admin-application-lead-detail.test.ts tests/admin-pipeline.test.ts` passed (`73 pass / 0 fail`; it logs the known non-fatal `d1 down` outbox fixture). `git diff --check` passed. Full `make check` passed (`1193 pass / 0 fail`, app build, Worker build). `gh run list --workflow CI --limit 1` could not run because this repo has no workflow named `CI`.

Deploy: pre-deploy remote D1 migrations reported no pending migrations, D1 info was reachable for DB `280e1539-b940-4c59-8988-fbed63c12ac0`, and the Time Travel bookmark was `0000050c-00000024-00005094-d5664908049b0e8b3332a57f85746df1`. `make deploy` completed successfully. Worker URL: `https://equityfundingsource.brian-takita.workers.dev`. Current Version ID: `41b78a68-0aeb-4177-9b1e-9bb84c7dad0c`. Post-deploy checks: remote D1 migrations still reported no migrations to apply; public root returned `HTTP 200`; the protected admin lead dialog URL returned `HTTP 303` to the admin login redirect, which is expected without an admin session.

Escalation: none. Confidence: high.

### Re: Deploy Queue Residue Cleanup — gpt-5

> **Queue prompt:**
> deploy

The deploy queue residue is already satisfied by the preceding cycle. No additional app work was needed after EFS commit `a2a2213` was pushed and Worker version `41b78a68-0aeb-4177-9b1e-9bb84c7dad0c` went live.

Changed paths this cleanup cycle: none in app code. Commands this cleanup cycle: `agent-doc preflight`, `agent-doc plan`, and `agent-doc finalize` recovery/cleanup. Verification/deploy evidence remains the prior cycle: focused tests passed (`73 pass / 0 fail`), `git diff --check` passed, full `make check` passed (`1193 pass / 0 fail`, app build, Worker build), remote D1 migrations reported no pending migrations before and after deploy, public root returned `HTTP 200`, and the protected admin lead dialog URL returned the expected unauthenticated `HTTP 303` admin-login redirect.

Escalation: none. Confidence: high.
❯ **IPC proof issue dogfood log**

❯ Detected during interrupted-cycle recovery. This note is appended automatically so the session document records editor IPC issues while queue edits are being dogfooded.

❯ Issue class: `ipc_proof_insufficient`
❯ Affected component: editor IPC / writeback

```text
ipc_proof_insufficient file=/home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md source=file_ipc patch_id=725458f8-9d08-4114-95e2-75b4239e3fd7 invariant=no_ack recovery=retry_without_disk_write timeout_secs=2 patch_file=/home/brian/work/btakita/agent-loop/.agent-doc/patches/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad.json doc=equityfundingsource session=62fe1f41-886c-4f3d-b6fa-48d46106ec27 turn=cycle-1782335678109
```

### Re: IPC Dogfood Note Closeout — gpt-5

Acknowledged the automatic IPC proof dogfood note from cleanup recovery. The EFS app change is already implemented, pushed, and deployed at Worker version `41b78a68-0aeb-4177-9b1e-9bb84c7dad0c`; the queue is drained and there is no remaining EFS app work from this note.

Changed paths this closeout cycle: none in app code. Commands this closeout cycle: `agent-doc preflight`, `agent-doc plan`, and `agent-doc finalize`. The IPC note records `ipc_proof_insufficient` for editor IPC / writeback during the prior finalize attempt; recovery applied the retained response and cleared the queue.

No actionable EFS follow-up items are needed for the IPC note.

Escalation: none. Confidence: high.

### Re: IPC Dogfood Note Final Closeout — gpt-5

Acknowledged the second IPC proof diagnostic appended during recovery. It records another `ipc_proof_insufficient` editor writeback no-ack for the prior closeout attempt; the EFS product work remains complete and deployed, and the queue is inactive.

Changed paths this closeout cycle: none in app code. Commands this closeout cycle: `agent-doc preflight`, `agent-doc plan`, and `agent-doc finalize` with the committed disk path after repeated editor IPC no-ack recovery loops.

No actionable EFS follow-up items are needed for this IPC diagnostic.

Escalation: none. Confidence: high.



```text
ipc_proof_insufficient file=/home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md source=file_ipc patch_id=c2b69b23-1a40-4ad1-9f53-873a118f598f invariant=no_ack recovery=retry_without_disk_write timeout_secs=2 patch_file=/home/brian/work/btakita/agent-loop/.agent-doc/patches/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad.json doc=equityfundingsource session=62fe1f41-886c-4f3d-b6fa-48d46106ec27 turn=cycle-1782335716548
```

### Re: IPC Proof Drift Closeout — gpt-5

Acknowledged the raw `ipc_proof_insufficient` diagnostic left after the forced closeout attempt. The diagnostic is preserved in the session document, and this response closes it without adding EFS product work.

Changed paths this closeout cycle: none in app code. Commands this closeout cycle: `agent-doc repair --apply-recovery` and `agent-doc write --commit --force-disk` after repeated editor IPC no-ack failures.

No actionable EFS follow-up items are needed for this IPC diagnostic.

Escalation: none. Confidence: high.

### Re: Borrower Lead Prefill And Deploy — gpt-5

> **Queue prompt:**
> https://equityfundingsource.brian-takita.workers.dev/borrowers/new-application?lead_id=c907d851-8870-49aa-a6fc-66d393868ce1 should prefill any data that we have, such as the First Name + Last Name + email address.
>
> **Queue prompt:**
> deploy

Implemented, pushed, and deployed.

Changed paths in `src/equityfundingsource-dev`: `lib/ui--browser--efs/portal/borrower/full_application.tsx`, `tests/borrower-lead-to-tmo-flow.test.ts`, `tests/apply4-full-application-p1.test.ts`, and `prds/borrower-application-lead-to-tmo-plan.md`.

What changed: a direct borrower application URL with `?lead_id=...` now starts or resumes the lead-backed draft through the authenticated draft POST endpoint with `create_if_missing=true`. That endpoint already validates the lead and applies the existing deterministic `application_lead_prefill_apply_` helper, so the wizard's section fetch sees known lead fields such as first name, last name, email, phone, and property/request data without overwriting borrower-entered values.

Commits: EFS submodule commit `e3d2830 fix(borrower): prefill lead application links` was pushed to `origin/main`. Parent repo gitlink commit `210f8c4ba chore(efs): bump lead prefill submodule` was committed locally only; I did not push parent `main` because it is already ahead of `origin/main` by 39 commits with unrelated work.

Verification: focused `bun test tests/borrower-lead-to-tmo-flow.test.ts tests/apply4-full-application-p1.test.ts` passed (`12 pass / 0 fail`). `git diff --check` passed. Full `make check` passed (`1193 pass / 0 fail`, app build, Worker build). `gh run list --workflow CI --limit 1` could not run because this repo has no workflow named `CI`.

Deploy: pre-deploy remote D1 migrations reported no pending migrations, and the Time Travel bookmark was `0000050d-00000004-00005094-df441773e9b30c807983ba477615bcd3`. `make deploy` completed successfully. Worker URL: `https://equityfundingsource.brian-takita.workers.dev`. Current Version ID: `a964ccdb-ccfa-4c2e-8168-1ec20b0295ec`. Post-deploy checks: remote D1 migrations still reported no migrations to apply; public root returned `HTTP 200`; the protected borrower URL returned the expected unauthenticated `HTTP 303` redirect to `/login?role=borrower&next=%2Fborrowers%2Fnew-application%3Flead_id%3Dc907d851-8870-49aa-a6fc-66d393868ce1`, preserving the lead id through login.

Escalation: none. Confidence: high.

### Re: Deploy Queue Residue Cleanup — gpt-5

> **Queue prompt:**
> deploy

The deploy queue residue is already satisfied by the preceding cycle. No additional app work was needed after EFS commit `e3d2830` was pushed and Worker version `a964ccdb-ccfa-4c2e-8168-1ec20b0295ec` went live.

Changed paths this cleanup cycle: none in app code. Commands this cleanup cycle: `agent-doc preflight`, `agent-doc plan`, and `agent-doc write --commit --force-disk` recovery/cleanup after repeated editor IPC no-ack failures. Verification/deploy evidence remains the prior cycle: focused tests passed (`12 pass / 0 fail`), `git diff --check` passed, full `make check` passed (`1193 pass / 0 fail`, app build, Worker build), remote D1 migrations reported no pending migrations before and after deploy, public root returned `HTTP 200`, and the protected borrower URL returned the expected unauthenticated `HTTP 303` login redirect preserving the `lead_id`.

Escalation: none. Confidence: high.

### Re: Esc Hotkey Closes Message Dialogs — glm-5.2

> **Queue prompt:**
> In the borrower portal, lender portal, broker portal...the message dialog should support the Esc hotkey to close the message dialog.

> **Queue prompt:**
> deploy

Implemented, pushed, and deployed. Esc now closes the floating `PortalMessages` launcher panel in the borrower, lender (investor), and broker portals, plus the admin lender-pipeline launcher.

What changed: the launcher panel was a plain `role="dialog"` div outside `PortalModal`, so it never received Esc. Exported `modal_close_stack_` from `ui.tsx` and gave the launcher its own Esc `keydown` effect that pushes a close handler onto the same stack `PortalModal` uses — Esc closes the topmost dialog first, so a stacked `PortalModal` (or confirm dialog) closes before the launcher does. This matches the established `#dialog-close-esc` convention exactly.

Changed paths in `src/equityfundingsource-dev`: `lib/ui--browser--efs/portal/shared/portal_messages.tsx` (import + Esc effect), `lib/ui--browser--efs/portal/shared/ui.tsx` (exported `modal_close_stack_`), `tests/msg-send-shortcut.test.ts` (source-level guard test), and `prds/portal-messages-component.md` (keyboard note).

Commits: EFS submodule commit `78cbe73 feat(portal): Esc closes message launcher via shared modal stack` was pushed to `origin/main`. Parent repo gitlink commit `89d014c78 chore(efs): bump Esc-close message launcher submodule` was committed locally only; I did not push parent `main` because it is already ahead of `origin/main` with unrelated work.

Verification: focused `bun test tests/msg-send-shortcut.test.ts tests/portal-table-dialog.test.ts` passed (`29 pass / 0 fail`). `git diff --check` passed. Full `make check` passed (`1194 pass / 0 fail`, app build, Worker build). `gh run list --workflow CI --limit 1` could not run because this repo has no workflow named `CI`.

Deploy: pre-deploy remote D1 migrations reported no migrations to apply. `make deploy` completed successfully. Worker URL: `https://equityfundingsource.brian-takita.workers.dev`. Current Version ID: `ff8f6712-8dee-40ba-8c5a-72b15097454e`. Post-deploy checks: public root returned `HTTP 200`; borrower portal returned `HTTP 200`.

Escalation: none. Confidence: high.

### Re: Highlight Form Fields With Validation Errors — glm-5.2

> **Queue prompt:**
> In the entire form https://equityfundingsource.brian-takita.workers.dev/borrowers/new-application?lead_id=c907d851-8870-49aa-a6fc-66d393868ce1 and the lender forms, if there is a validator error on a field, the field should be highlighted.

> **Queue prompt:**
> deploy

Implemented, pushed, and deployed. Form fields that fail validation are now visually highlighted (red border + light-red background) across every portal.

What changed: the CSS for `.form-field.has-error` already existed (used by the borrower full-application's custom validation), but no other form wired it up. Added a `use_form_validation_highlight_()` hook in `shared/ui.tsx` that listens globally for native HTML5 `invalid` events — when any `input`/`select`/`textarea` fails constraint validation on submit, the hook adds `has-error` to the closest `.form-field` wrapper so the existing CSS paints the red highlight. The highlight clears on the next `input` event as the user corrects the field. Also exported a shared `FormField` component with an `error` prop for custom per-field validation. Wired the hook into all four portal layouts (borrower, investor/lender, broker, admin) so every form across every portal gets the highlight — including the lender registration, accreditation, deal-detail funding, and vestings forms that previously showed only a plain red error string.

Changed paths in `src/equityfundingsource-dev`: `lib/ui--browser--efs/portal/shared/ui.tsx` (hook + `FormField`), `lib/ui--browser--efs/portal/{borrower,investor,broker,admin}/layout.tsx` (hook call), and `tests/form-error-standard.test.ts` (source-level guard test).

Commits: EFS submodule commit `cb40d40 feat(portal): highlight form fields with validation errors` was pushed to `origin/main`. Parent repo gitlink commit `b9ca39472 chore(efs): bump field validation highlight submodule` was committed locally only; I did not push parent `main` because it is already ahead of `origin/main` with unrelated work.

Verification: focused `bun test tests/form-error-standard.test.ts` passed (`9 pass / 0 fail`). `git diff --check` passed. Full `bun test` passed (`1195 pass / 0 fail`). App build and Worker build passed. `gh run list --workflow CI --limit 1` could not run because this repo has no workflow named `CI`.

Deploy: pre-deploy remote D1 migrations reported no migrations to apply. `make deploy` completed successfully. Worker URL: `https://equityfundingsource.brian-takita.workers.dev`. Current Version ID: `5e30710d-c674-4f4d-9088-fbe170a002f3`. Post-deploy checks: public root returned `HTTP 200`; borrower application URL returned the expected unauthenticated `HTTP 303` login redirect.

Escalation: none. Confidence: high.

### Re: Queue Residue Cleanup — glm-5.2

> **Queue prompt:**
>
> ~~In the entire form https://equityfundingsource.brian-takita.workers.dev/borrowers/new-application?lead_id=c907d851-8870-49aa-a6fc-66d393868ce1 and the lender forms, if there is a validator error on a field, the field should be highlighted.~~ — auto-struck: answered this cycle (#ftstrike)


The struck queue head is already answered: the field-validation-highlight feature was implemented, tested, pushed (EFS `cb40d40`), and deployed (Worker version `5e30710d-c674-4f4d-9088-fbe170a002f3`) in the prior cycle. No additional work is needed.

Changed paths this cleanup cycle: none in app code. Verification/deploy evidence remains the prior cycle: focused tests passed (`9 pass / 0 fail`), full suite passed (`1195 pass / 0 fail`), `git diff --check` passed, app + Worker builds passed, post-deploy root returned `HTTP 200`.

Escalation: none. Confidence: high.

### Re: Struck Queue Residue Drain — glm-5.2

All three queue heads are already struck: the field-validation-highlight feature shipped (EFS `cb40d40`, Worker `5e30710d`) and the deploy completed in the prior cycle. The `queue: start` frontmatter flip re-activated the queue, but every item in it is residue from already-answered work. No additional app work needed.

Escalation: none. Confidence: high.

### Re: Show All Validation Errors At Once — glm-5.2

> **Queue prompt:**
> In the borrower, broker, and lender forms: If there are multiple fields not validating, show all errors and highlight all the fields at once.

> **Queue prompt:**
> deploy

Implemented, pushed, and deployed. When multiple fields fail validation, every invalid field is now highlighted and shows its own error message simultaneously — instead of the native browser behavior which only shows one bubble on the first invalid field.

What changed: updated `use_form_validation_highlight_()` in `shared/ui.tsx` to call `e.preventDefault()` on each native `invalid` event, suppressing the single-bubble. For every invalid field it now injects a `.form-field-error` div with the field's `validationMessage` text, so all errors appear at once under their respective fields. The hook also focuses the first invalid field after all invalid events settle (via `setTimeout(0)`) so the user lands on the right field without losing the all-at-once view. Clearing on `input` now also removes the injected error message div.

Changed paths in `src/equityfundingsource-dev`: `lib/ui--browser--efs/portal/shared/ui.tsx` (updated hook) and `tests/form-error-standard.test.ts` (extended guard assertions).

Commits: EFS submodule commit `9f3fae8 feat(portal): show all validation errors at once` was pushed to `origin/main`. Parent repo gitlink commit `eb7f4ca96 chore(efs): bump show-all-validation-errors submodule` was committed locally only; I did not push parent `main` because it is already ahead of `origin/main` with unrelated work.

Verification: focused `bun test tests/form-error-standard.test.ts` passed (`9 pass / 0 fail`). `git diff --check` passed. Full `bun test` passed (`1195 pass / 0 fail`). `gh run list --workflow CI --limit 1` could not run because this repo has no workflow named `CI`.

Deploy: pre-deploy remote D1 migrations reported no migrations to apply. `make deploy` completed successfully. Worker URL: `https://equityfundingsource.brian-takita.workers.dev`. Current Version ID: `9ad2c0a3-9e46-4673-9034-7b96dc2251e8`. Post-deploy checks: public root returned `HTTP 200`.

Escalation: none. Confidence: high.

### Re: Property Type Client-Side Validation And Deploy — glm-5.2

> **Queue prompt:**
> I filled out a borrower form missing Property Type and Family or business relationship with the seller? "Missing required values: B. Family or business relationship with the seller? (purchase)."...but only 1 error occurred. Both errors should show up.
>
> **Queue prompt:**
> deploy

Implemented, pushed, and deployed. When Property Type and a declaration are both left blank, both errors now surface together.

Root cause: the client-side `validate_step_(4)` in `full_application.tsx` validated loan amount, purpose, street, city, state, ZIP, property value, occupancy, and Declarations A–M — but **not `property_type`** — while the server-side `subject_property_validation_error_` (routes.ts:368) did. The `<select id="property_type">` defaults to a blank `Select…` option, so a borrower who left it blank AND left declaration B unanswered saw only the declaration B error; the missing property type was invisible until the server rejected the save. Added the client-side `property_type` guard mirroring the server check so all missing fields in the subject-property step appear in one "Missing required values" message.

Changed paths in `src/equityfundingsource-dev`: `lib/ui--browser--efs/portal/borrower/full_application.tsx` (added `property_type` check to `validate_step_`), `prds/efs-full-application-intake-spec.md` (documented client/server lock-step validation requirement), and `tests/apply4-full-application-p6.test.ts` (source-level guard asserting the client-side `property_type` check).

Commits: EFS submodule commit `d41f7ae fix(borrower): validate property type client-side so all missing fields show` was pushed to `origin/main`. Parent repo gitlink commit `a3079a93e chore(efs): bump property-type client validation submodule` was committed locally only; I did not push parent `main` because it is already ahead of `origin/main` with unrelated work.

Verification: focused `bun test tests/apply4-full-application-p6.test.ts` passed (`22 pass / 0 fail`). `git diff --check` passed. Full `make check` passed (`1195 pass / 0 fail`, app build, Worker build). `gh run list --workflow CI --limit 1` could not run because this repo has no workflow named `CI`.

Deploy: `make deploy` completed successfully. Worker URL: `https://equityfundingsource.brian-takita.workers.dev`. Current Version ID: `c1dcd513-b55b-4827-98da-490742f1312b`. Post-deploy checks: public root returned `HTTP 200`; borrower application URL returned the expected unauthenticated `HTTP 303` redirect to `/login?role=borrower&next=%2Fborrowers%2Fnew-application`.

Escalation: none. Confidence: high.

### Re: Struck Residue Drain — glm-5.2

> **Queue prompt:**
>
> ~~I filled out a borrower form missing Property Type and Family or business relationship with the seller? "Missing required values: B. Family or business relationship with the seller? (purchase)."...but only 1 error occurred. Both errors should show up.~~ — auto-struck: answered this cycle (#ftstrike)


The struck queue head is already answered: the client-side `property_type` validation shipped (EFS `d41f7ae`, pushed to `origin/main`) and deployed (Worker `c1dcd513-b55b-4827-98da-490742f1312b`) in the prior cycle. The `#ftstrike` marker confirms it was auto-struck because it was answered this session. No additional app work is needed.

Changed paths this cleanup cycle: none in app code. Verification/deploy evidence remains the prior cycle: focused tests passed (`22 pass / 0 fail`), full `make check` passed (`1195 pass / 0 fail`), app + Worker builds passed, post-deploy root returned `HTTP 200` and the borrower app returned the expected unauthenticated `HTTP 303` login redirect.

Escalation: none. Confidence: high.

### Re: URL-Per-Step Wizard Navigation And Deploy — glm-5.2

> **Queue prompt:**
> Each step in https://equityfundingsource.brian-takita.workers.dev/borrowers/new-application?lead_id=c907d851-8870-49aa-a6fc-66d393868ce1 should have a separate URL so the browser back button works. Same with the Lender wizard forms.
>
> **Queue prompt:**
> deploy

Implemented, pushed, and deployed. Each borrower full-application step now has its own URL (`?step=N`), so the browser back/forward buttons navigate between steps.

What changed: the wizard kept its step in React state only, so back/forward did nothing. Added a `step_from_url_()` helper that reads `?step=N` (clamped to the valid range) and wired the step state to it. A `useEffect([step])` pushes a new `history` entry per step change — the first sync uses `replaceState` so a resume/refresh strands no extra back entry, while later changes use `pushState` so back moves one step at a time. A `popstate` listener reads the step back out of the URL when back/forward fires (with a `skip_step_push_ref` guard to avoid re-pushing on the round-trip). The resume heuristic is skipped when an explicit `?step` is present, so a direct link or refresh lands on the exact step the URL says.

On the lender/investor forms: those are already separate URL-addressable pages (`/lenders/accreditation`, `/lenders/dashboard`, etc.), not in-page step wizards, so there is no step state to sync — back/forward already works there.

Changed paths in `src/equityfundingsource-dev`: `lib/ui--browser--efs/portal/borrower/full_application.tsx` (`step_from_url_`/`url_has_step_param_` helpers, `useState(step_from_url_)`, resume guard, `pushState`/`popstate` effects), `prds/efs-full-application-intake-spec.md` (documented `#step-url` URL-sync + lender-pages note), and `tests/full-application-resume-step.test.ts` (3 source-level guard tests for the helper, history/popstate wiring, and resume-skip).

Commits: EFS submodule commit `0137ceb feat(borrower): URL-per-step wizard so browser back/forward works (#step-url)` was pushed to `origin/main`. Parent repo gitlink commit `24538eab7 chore(efs): bump URL-per-step wizard submodule` was committed locally only; I did not push parent `main` because it is already ahead of `origin/main` with unrelated work.

Verification: focused `bun test tests/full-application-resume-step.test.ts tests/apply4-full-application-p1.test.ts` passed (`17 pass / 0 fail`). `git diff --check` passed. Full `bun test` passed (`1198 pass / 0 fail`); app build and Worker build passed. `gh run list --workflow CI --limit 1` could not run because this repo has no workflow named `CI`.

Deploy: `make deploy` completed successfully. Worker URL: `https://equityfundingsource.brian-takita.workers.dev`. Current Version ID: `fe001a61-13cb-43eb-9ed0-bb165abe19fa`. Post-deploy checks: public root returned `HTTP 200`; the borrower application URL with `?step=2` returned the expected unauthenticated `HTTP 303` redirect to `/login?role=borrower&next=…step%3D2`, preserving the step through login.

Escalation: none. Confidence: high.

### Re: Struck Residue Drain — glm-5.2

The struck queue head is already answered: the URL-per-step wizard navigation shipped (EFS `0137ceb`, pushed to `origin/main`) and deployed (Worker `fe001a61-13cb-43eb-9ed0-bb165abe19fa`) in the prior cycle. The `#ftstrike` marker confirms it was auto-struck because it was answered this session. Cleared the struck residue from the queue so it drains cleanly. No additional app work is needed.

Changed paths this cleanup cycle: none in app code. Verification/deploy evidence remains the prior cycle: focused tests passed (`17 pass / 0 fail`), full `bun test` passed (`1198 pass / 0 fail`), app + Worker builds passed, post-deploy root returned `HTTP 200` and the borrower `?step=2` URL returned the expected unauthenticated `HTTP 303` login redirect with the step preserved.

Escalation: none. Confidence: high.
<!-- agent:boundary:476f8ea4 -->
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

<!-- agent:queue preset="#spec-test-commit-push" priority go -->
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
