---
agent_doc_session: 39ffe113-30d6-4186-8eee-e50634d306c3
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
❯ Please look at https://www.skills.google/
❯ Can we install the google skills to interact with google docs?
Would corky benefit from the google skills?
### Re: skills.google (HEAD)

[skills.google](https://www.skills.google/) is Google's **cloud learning platform** — Vertex AI, Gemini, certifications, hands-on labs. It is not a plugin or MCP system; it offers no installable integrations.

### Re: Can we install google skills to interact with Google Docs? (HEAD)

Not from skills.google — it's a training site, not an integration hub.

The correct path for Google Docs integration is the **Google Workspace MCP** or the Google Drive/Docs REST API. This session already has Gmail and Calendar MCPs (`mcp__claude_ai_Gmail__authenticate`, `mcp__claude_ai_Google_Calendar__authenticate`) — Google Docs/Drive would follow the same pattern (an MCP server, not a skill file).

To add Google Docs support you'd need to install or configure a Workspace MCP that covers Drive/Docs, then authenticate it similarly to Gmail.

### Re: Would corky benefit from the google skills? (HEAD)

Corky handles correspondence: email (Gmail API), LinkedIn, YouTube. A Google Drive/Docs MCP would let corky read drafts or content directly from Google Docs — useful if you author correspondence or content in Docs before sending.

Current corky workflow: local files → Gmail/LinkedIn/YouTube APIs. If drafts live in Google Docs rather than local files, the integration would be valuable. If not, the benefit is minimal.
<!-- agent:boundary:4ee26922 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
<!-- /agent:pending -->
