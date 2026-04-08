---
description: Draft and send emails with HTML formatting via corky or browser paste
user-invocable: true
argument-hint: "<command> [args]"
---

# email

Draft, format, and send emails from agent-doc sessions. Handles HTML formatting, Gmail threading, and multiple delivery paths.

## Invocation

```
/email draft <recipient> <subject>   — draft a new email or reply
/email send <draft-file>             — send a draft via corky or browser
/email html <draft-file>             — save HTML version for browser paste
```

## Commands

### `draft` — Create or Reply to Email

Draft a new email or reply to an existing thread.

**Steps:**
1. If replying, search `mail/conversations/` for the thread
2. Extract thread ID and subject for threading
3. Create draft via `corky draft new --to <recipient> --in-reply-to <thread-id> "<subject>"`
4. Write email content to the draft file
5. Set status to `approved`

**Reply threading:**
- Always search `mail/conversations/` for existing threads with the recipient
- Extract Thread ID from the conversation file
- Use `--in-reply-to` with the thread ID
- Preserve the original subject with `Re:` prefix (Gmail threading requires matching subjects)

### `send` — Send a Draft

**IMPORTANT: Never send automatically.** See `runbooks/email-send.md` for full workflow.

### `html` — Generate HTML for Browser Paste

See `runbooks/browser-paste.md` for the full workflow.

## Conventions

- **Never send without approval** — always push to Gmail Drafts first, wait for user to say "send"
- Always use `file:///` protocol for browser-openable file paths
- Always search for existing threads before creating standalone emails
- Gmail threading requires matching subjects — use `Re: <original subject>`
- HTML emails use inline CSS (Gmail strips `<style>` tags)
- Draft content comes from agent-doc session context — read correspondence, resume, and contact files as needed
- Verify facts against `mail/contacts/<name>/CLAUDE.md` before sending
- Attachment paths must be absolute (tilde `~` not expanded for attachments in corky)

## Runbooks

When executing these operations, read and follow the linked runbook:

- `send email` — [runbooks/email-send.md](runbooks/email-send.md)
- `review inbox` — [runbooks/review-inbox.md](runbooks/review-inbox.md)
- `draft reply` — [runbooks/draft-reply.md](runbooks/draft-reply.md)
- `enrich contact` — [runbooks/enrich-contact.md](runbooks/enrich-contact.md)
- `gmail config` — [runbooks/gmail-config.md](runbooks/gmail-config.md)
- `browser paste` — [runbooks/browser-paste.md](runbooks/browser-paste.md)
