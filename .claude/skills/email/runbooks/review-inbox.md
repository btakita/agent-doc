# Review Inbox

Sync and triage recent emails across all accounts.

## Steps

1. **Sync latest:** `corky sync` (incremental by default, tracks IMAP UIDs)
2. **List recent conversations:**
   ```bash
   ls -lt mail/conversations/ | head -20
   ```
3. **Check mailbox-routed conversations** (if configured):
   ```bash
   ls -lt mail/mailboxes/*/conversations/ | head -20
   ```
4. **Read new/unread messages:** Open recent conversation files, scan for new messages since last review
5. **Check contact context:** For each sender, check `mail/contacts/<name>/CLAUDE.md` for relationship context
6. **Triage:** Categorize messages as:
   - Needs reply (draft a response)
   - Needs action (add to pending)
   - Informational (no action)
7. **Report findings** to the user with a summary of new messages and recommended actions

## Notes

- Conversations are markdown files with YAML frontmatter containing Thread ID, labels, and metadata
- File mtime reflects the last message date
- Slug-based filenames are immutable (identity tracked by Thread ID inside the file)
- Multi-account threads accumulate labels from all accounts
