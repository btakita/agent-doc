# granola

Import and interact with Granola meeting notes via shared links.

## Invocation

```
/granola <URL>
/granola import <URL> [--output <path>]
```

- `/granola <URL>` — scrape a Granola shared note and display the content
- `/granola import <URL>` — scrape and save as an agent-doc template document

## How It Works

Granola (granola.ai) is a Mac-only meeting notes app. It captures audio, transcribes, and generates structured summaries with Claude Sonnet. Shared links (`notes.granola.ai/d/<id>`) are read-only web views with no public API.

**Key insight:** Granola has context not visible in the shared document — the full meeting transcript, audio, and Granola's own Claude Sonnet analysis. The shared note is a summary. When importing, we capture the structured summary and make it interactive via agent-doc.

## Scraping Method

Granola embeds full document content in the Next.js RSC (React Server Components) payload. Extract via:

```bash
curl -sL "<URL>" | python3 -c "
import sys, re, html
content = sys.stdin.read()
matches = re.findall(r'self\.__next_f\.push\(\[1,\"(.*?)\"\]\)', content)
full_html = ''
for m in matches:
    decoded = m.encode().decode('unicode_escape')
    if '<h3>' in decoded or '<li>' in decoded:
        full_html += decoded
# Convert HTML to markdown
text = full_html
text = re.sub(r'<h3>(.*?)</h3>', r'### \1', text)
text = re.sub(r'<li>(.*?)</li>', r'- \1', text)
text = re.sub(r'</?ul[^>]*>', '', text)
text = re.sub(r'</?p[^>]*>', '', text)
text = re.sub(r'<strong>(.*?)</strong>', r'**\1**', text)
text = re.sub(r'<[^>]+>', '', text)
text = html.unescape(text)
print(text.strip())
"
```

## Workflow

### 1. Scrape the shared link

Fetch the URL and extract the structured notes from the RSC payload. Parse HTML to markdown.

### 2. Extract metadata

From the RSC payload, extract:
- Document title
- Creator name and workspace
- Created date
- Attendees

### 3. Create agent-doc template (import mode)

Generate a template document with components:

```markdown
---
agent_doc_session: <generated UUID>
agent_doc_format: template
agent_doc_write: crdt
granola_source: <URL>
---

## Meeting Notes

<!-- agent:notes patch=replace -->
<scraped content here>
<!-- /agent:notes -->

## Analysis

<!-- agent:analysis patch=replace -->
*Run /agent-doc to analyze the meeting notes*
<!-- /agent:analysis -->

## Action Items

<!-- agent:action-items patch=replace -->
*Extracted from meeting notes*
<!-- /agent:action-items -->

## Exchange

<!-- agent:exchange patch=append -->
<!-- /agent:exchange -->
```

### 4. Interactive prompting

Once imported as an agent-doc template, you can:
- Ask questions about the meeting inline via the Exchange component
- Have the agent extract action items, decisions, follow-ups
- Cross-reference with other session documents (transcripts, previous meetings)
- Feed into the session analyzer for metrics

## Runbooks

- `import` — [runbooks/import.md](runbooks/import.md)
- `enrich notes` / `enrich from transcript` — [runbooks/enrich-from-transcript.md](runbooks/enrich-from-transcript.md)

### Import a Granola note

1. Run `/granola import <URL> --output <path>`
2. The skill scrapes the shared link and creates a template document
3. Open the document in your editor
4. Use `/agent-doc` to interact with the imported content

### Access Granola's hidden context

Granola's shared notes are summaries. The full transcript is accessible via the transcript link (if shared):
- Check for transcript URL in the scraped content (format: `notes.granola.ai/t/<id>`)
- If available, scrape the transcript separately and include it as a component

## Limitations

- **No Linux client** — Granola is Mac-only. This skill only works with shared links.
- **No API** — All access is via HTML scraping. May break if Granola changes their frontend.
- **Read-only** — Cannot push content back to Granola.
- **Workaround for ongoing access:** Ask the note creator to push to Google Docs (Granola has native integration), then use Google Docs API.
