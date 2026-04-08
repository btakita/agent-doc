# Enrich Notes from Granola Transcript

Extract hidden transcript context from Granola's shared note UI and add it to the agent-doc template.

## When to use

After importing a Granola note via `/granola import`, the agent-doc template contains only the structured summary. Granola holds the full meeting transcript and generates contextual summaries on text selection. Use this runbook to pull in that hidden context.

## Steps

### 1. Open the Granola shared link in a browser

Navigate to the `granola_source` URL from the document's frontmatter.

### 2. Select key claims or topics

In the Granola web UI, select text passages that are:
- Important decisions or agreements
- Claims that need verification or context
- Topics where the summary feels incomplete

When you select text, Granola shows a **Transcript Summary** popup with:
- The relevant transcript excerpt (verbatim)
- Granola's contextual summary of that section

### 3. Copy transcript summaries

For each selected passage:
1. Read the Transcript Summary popup
2. Copy the relevant excerpt (both the transcript quote and Granola's summary)

### 4. Add to the agent-doc exchange

Paste the transcript context into the document's exchange component as enrichment:

```markdown
#### Transcript context: <topic>

> <transcript excerpt from Granola popup>

Granola summary: <Granola's contextual summary>
```

### 5. Run /agent-doc

The agent can now use the transcript context alongside the structured notes for deeper analysis, action item extraction, or follow-up planning.

## Tips

- Focus on passages where the summary is ambiguous or where exact wording matters
- Transcript excerpts are especially valuable for compensation discussions, technical decisions, and action item assignments
- You can paste multiple transcript excerpts in one exchange entry
