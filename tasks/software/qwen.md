---
agent_doc_session: 33dc0f11-3f6a-4bb8-a581-7966933e0742
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
❯ In src/session-share/tasks/claudescore.md, "Web search was unavailable (model access issue)" was mentioned.
❯ I'm using Qwen 3.6. How can I enable web search?
### Re: Enabling web search with Qwen 3.6 (HEAD)

Web search availability depends on the specific Qwen 3.6 deployment/platform you're using. Here are the key factors:

**Qwen API / DashScope:**
If you're using Qwen through Alibaba's DashScope API, web search is not a built-in tool capability — it's a separate model feature. The Qwen models themselves don't have native tool-use web search like Claude does. You'd need to:

1. **Implement web search externally** — call a search API (Google Custom Search, Bing, SearXNG, etc.) from your application code, then feed the results into the Qwen prompt as context
2. **Use a framework with tool calling** — Qwen 3.6 supports function calling, so you can define a `web_search` tool that your code implements, and the model will request to use it

**Claude Code context:**
If you're asking about web search *within Claude Code* — the `WebSearch` and `WebFetch` tools are available regardless of which model you're chatting with in your own sessions. But if you're using Qwen directly (outside Claude Code), web search is not something the model has built-in access to.

**Bottom line:** Qwen doesn't have native web search as a built-in capability. You need to wire it up yourself via function calling or external API calls.
<!-- agent:boundary:57d23d4f -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
<!-- /agent:pending -->
