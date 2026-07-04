---
name: No auto-send emails
description: Never send emails directly without explicit user approval — always push to Gmail Drafts first
type: feedback
---

Never send emails directly via `corky draft push --send` without the user explicitly saying "send". Default to pushing to Gmail Drafts (`corky draft push` without `--send`) so the user can review first.

**Why:** User corrected after multiple emails were sent directly without review. This aligns with corky's CLAUDE.md safety rule: "Never send email directly. Always save as a Gmail draft for review first."

**How to apply:** When drafting emails for recruiter submissions or any correspondence:
1. Create the draft and show contents in the exchange
2. Push to Gmail Drafts (no `--send` flag)
3. Wait for explicit "send" instruction before using `--send`
