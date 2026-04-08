# Email Send Workflow

**IMPORTANT: Never send automatically.** Always push to Gmail Drafts first and wait for explicit user approval.

## Steps

1. Push to Gmail Drafts: `corky draft push <file>` (no `--send` flag)
2. Show the user what was pushed (recipient, subject, body preview)
3. Wait for explicit "send" instruction
4. Only then: `corky draft push <file> --send` (requires status = `approved`, `review`, or `scheduled`)

## Delivery by Account Type

### gmail-api accounts (e.g. btak.dev)

- `corky draft push <file>` -> Gmail API `drafts.create` (Drafts folder)
- `corky draft push <file> --send` -> Gmail API `messages.send` (sends directly)
- Uses OAuth2 with `gmail.compose` scope (auto-refreshes after first browser consent)
- No SMTP password needed

### SMTP accounts (e.g. personal, proton-dev)

- `corky draft push <file>` -> IMAP APPEND to Drafts folder
- `corky draft push <file> --send` -> SMTP send
- Requires password or password_cmd in .corky.toml
