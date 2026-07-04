---
name: feedback_askuserquestion_no_option_confirm_actual_choice
description: "When AskUserQuestion returns no-option-selected plus a note, confirm the actual choice — don't default to your recommendation"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: b561f3f0-1842-4e6e-b164-347c645b3fef
---

On the EFS email-sender decision (2026-06-22), AskUserQuestion returned "(no option selected)" with the note "If this is only going to touch the existing EFS app email, then it's ok. This is a test/staging app only." I treated that conditional note as approval for my recommended option (A=Resend) and built + cut over the whole Resend path. The operator had actually selected the **Cloudflare** option (B — CF Email Service on efs.briantakita.me) and wanted to **test before cutting over**. I had to revert the Resend cutover and restore prod.

**Why:** A "no option selected" + free-text note is NOT an endorsement of your top recommendation. The note often qualifies a choice the operator made out-of-band (verbally / in their head), not the option you ranked first.

**How to apply:** When AskUserQuestion comes back without a concrete option but with a note, do NOT proceed on your recommended default. Re-confirm the actual selection in one line ("Did you mean the Cloudflare path or Resend?") before building anything irreversible. Also: when the operator says "test before cutting over," never flip production config (FROM vars / deploy) until a non-prod test of the new path passes. See [[feedback_testing_before_release]] and [[feedback_explain_decisions_plainly_not_jargon]].
