# Compact Exchange

Steps to compact an agent-doc exchange component when it grows too large.

## When to compact

- User explicitly requests "compact exchange"
- Never auto-compact without user approval

## Steps

1. **Read the full exchange content** from the document

2. **Summarize** — preserve:
   - Decisions made (with rationale)
   - Key facts and discoveries
   - Open items and pending work
   - Discard verbose back-and-forth, code snippets already committed, exploratory dead-ends

3. **Run `agent-doc compact <FILE>`**
   - Archives the original content to `.agent-doc/archives/`
   - Replaces exchange content with archive pointer (uses `replace_content()`, not the patch pipeline)
   - Updates snapshot atomically
   - Pass `--message "summary text"` to include a custom summary instead of the default archive pointer

4. **Commit** via `agent-doc commit <FILE>`
