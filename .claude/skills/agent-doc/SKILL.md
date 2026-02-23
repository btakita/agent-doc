# agent-doc run

Interactive document session — respond to user edits in a markdown document.

## Invocation

```
/agent-doc <FILE>
```

Arguments: `FILE` — path to the session document (e.g., `plan.md`)

## Core Principles

- **Document is the UI** — the user's edits ARE the prompt; respond in the document AND the console
- **Preserve user edits** — never overwrite; read the file fresh before writing
- **Show progress** — stream your response in the console so the user sees real-time feedback
- **Maintain session** — the document is a living conversation, not a one-shot

## Workflow

### 1. Read the document and snapshot

- Read `<FILE>` to get current content
- Read the snapshot at `.agent-doc/snapshots/<hash>.md` where `<hash>` is SHA256 of the canonical file path
  - If no snapshot exists, treat this as the first run (entire document is new)

### 2. Compute the diff

- Compare snapshot (previous state) against current content
- The diff represents what the user changed since the last run
- If no diff (content unchanged), tell the user nothing changed and stop

### 3. Respond

- Address the user's changes naturally in the console (this gives real-time streaming feedback)
- Respond to:
  - New `## User` blocks
  - Inline annotations (blockquotes, comments, edits to previous responses)
  - Structural changes (deletions, reorganization)
- Your console response IS the document response — they should be the same content

### 4. Write back to the document

After responding, update the document file:

1. **Re-read the file** (user may have edited during your response)
2. Append your response as:
   ```
   ## Assistant

   <your response>

   ## User

   ```
3. Use the Edit tool to append (not Write — preserves user edits made during response)
4. Update the snapshot to match the new document state

### 5. Git integration (optional)

If the document is in a git repo:
- Before responding: `git add -f <FILE> && git commit -m "agent-doc: <timestamp>" --no-verify`
- After writing response: do NOT commit (leave as uncommitted for diff gutters)

## Document Format

Session documents use YAML frontmatter:

```yaml
---
session: <uuid or null>
agent: <name or null>
model: <model or null>
branch: <branch or null>
---
```

The body alternates `## User` and `## Assistant` blocks. Inline annotations (blockquotes, comments) within any block are valid prompts.

## Snapshot Storage

- Location: `.agent-doc/snapshots/` relative to CWD
- Filename: `sha256(canonical_path) + ".md"`
- Contains the full document content after the last run

## Success Criteria

- User sees streaming response in the Claude console
- Document is updated with the response (user can see it in their editor)
- User edits made during response are preserved (not overwritten)
- Snapshot is updated for the next run's diff computation
