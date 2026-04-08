# Granola Import Runbook

Import a Granola shared note into an agent-doc template document.

## Steps

### 1. Scrape the shared link

```bash
curl -sL "<URL>" | python3 -c "
import sys, re, html, json

content = sys.stdin.read()

# Extract RSC payload
matches = re.findall(r'self\.__next_f\.push\(\[1,\"(.*?)\"\]\)', content)
full_html = ''
metadata = {}

for m in matches:
    decoded = m.encode().decode('unicode_escape')
    # Extract HTML content
    if '<h3>' in decoded or '<li>' in decoded or '<ul>' in decoded:
        full_html += decoded
    # Extract metadata (title, creator, date)
    if 'og:title' in decoded:
        title_match = re.search(r'content\":\"([^\"]+)\"', decoded)
        if title_match:
            metadata['title'] = title_match.group(1)

# Convert HTML to markdown
text = full_html
text = re.sub(r'<h3>(.*?)</h3>', r'### \1', text)
text = re.sub(r'<h2>(.*?)</h2>', r'## \1', text)
text = re.sub(r'<li>(.*?)</li>', r'- \1', text)
text = re.sub(r'</?ul[^>]*>', '', text)
text = re.sub(r'</?p[^>]*>', '', text)
text = re.sub(r'<strong>(.*?)</strong>', r'**\1**', text)
text = re.sub(r'<em>(.*?)</em>', r'*\1*', text)
text = re.sub(r'<br\s*/?>', '\n', text)
text = re.sub(r'<[^>]+>', '', text)
text = html.unescape(text)
text = re.sub(r'\n{3,}', '\n\n', text)

print(json.dumps({'content': text.strip(), 'metadata': metadata}))
"
```

### 2. Check for transcript link

Look for transcript URL in the scraped content:
- Format: `notes.granola.ai/t/<id>`
- If found, scrape it separately using the same method

### 3. Create the template document

Write a template document with the scraped content:

```markdown
---
agent_doc_session: <generated UUID>
agent_doc_format: template
agent_doc_write: crdt
granola_source: <URL>
---

## Meeting: <title>

<!-- agent:notes patch=replace -->
<scraped markdown content>
<!-- /agent:notes -->

## Analysis

<!-- agent:analysis patch=replace -->
<!-- /agent:analysis -->

## Action Items

<!-- agent:action-items patch=replace -->
<!-- /agent:action-items -->

## Exchange

<!-- agent:exchange patch=append -->
<!-- /agent:exchange -->

## Pending

<!-- agent:pending patch=replace -->
<!-- /agent:pending -->
```

### 4. Auto-populate action items

Parse the scraped content for "Next Steps" sections and populate the action-items component.

### 5. Commit

Run `agent-doc commit <output-path>` to create the initial snapshot.
