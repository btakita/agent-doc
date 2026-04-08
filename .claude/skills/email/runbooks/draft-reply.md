# Draft Reply

Create a threaded reply to an existing email conversation.

## Steps

1. **Find the conversation:** Search `mail/conversations/` for the thread
   ```bash
   ls mail/conversations/ | grep -i <keyword>
   ```
2. **Read the conversation** to understand context and extract threading info:
   - **Thread ID** from the YAML frontmatter
   - **Subject** from the frontmatter or first message
   - **Sender email** for the `--to` field
3. **Check contact file:** Read `mail/contacts/<name>/CLAUDE.md` for relationship context, verify any claims about their platform/tools/status
4. **Create draft:**
   ```bash
   corky draft new --to "<email>" --in-reply-to "<thread-id>" "Re: <original subject>"
   ```
5. **Write the email body** to the draft file (markdown format)
6. **Review voice guidelines:** Check `mail/voice.md` for tone and formatting
7. **Push to Gmail Drafts:** `corky draft push <draft-file>` (never `--send` without approval)
8. **Wait for user approval** before sending

## Threading Rules

- Always use `Re: <original subject>` (Gmail breaks threads on subject change)
- Always set `in_reply_to` with the Thread ID from the conversation file
- Never create a standalone email when context implies a reply

## Delivery

See `runbooks/email-send.md` for send workflow (gmail-api vs SMTP).
