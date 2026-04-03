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

3. **Archive the original** to `.agent-doc/archives/<hash>-<timestamp>.md`
   - `agent-doc compact` handles this if available
   - Otherwise: `cp <FILE> .agent-doc/archives/<hash>-$(date +%Y%m%d-%H%M%S).md`

4. **Replace exchange content using the Edit tool**
   - **IMPORTANT:** Use the Edit tool, NOT `agent-doc write`
   - `agent-doc write` with `patch=append` appends — compaction requires full replacement
   - Replace the content between `<!-- agent:exchange -->` and `<!-- /agent:exchange -->`

5. **Add archive pointer** at the top of the new exchange:
   ```
   *Compacted. Content archived to `.agent-doc/archives/<filename>`*
   ```

6. **Commit** via `agent-doc commit <FILE>`
