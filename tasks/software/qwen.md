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
### Re: Enabling web search with Qwen 3.6

Web search availability depends on the specific Qwen 3.6 deployment/platform you're using. Here are the key factors:

**Qwen API / DashScope:**
If you're using Qwen through Alibaba's DashScope API, web search is not a built-in tool capability — it's a separate model feature. The Qwen models themselves don't have native tool-use web search like Claude does. You'd need to:

1. **Implement web search externally** — call a search API (Google Custom Search, Bing, SearXNG, etc.) from your application code, then feed the results into the Qwen prompt as context
2. **Use a framework with tool calling** — Qwen 3.6 supports function calling, so you can define a `web_search` tool that your code implements, and the model will request to use it

**Claude Code context:**
If you're asking about web search *within Claude Code* — the `WebSearch` and `WebFetch` tools are available regardless of which model you're chatting with in your own sessions. But if you're using Qwen directly (outside Claude Code), web search is not something the model has built-in access to.

**Bottom line:** Qwen doesn't have native web search as a built-in capability. You need to wire it up yourself via function calling or external API calls.
❯ I'm using claude code. Why did the claudescore.md session not find WebSearch? Is this due to me using agent-doc?

### Re: Why claudescore.md couldn't use WebSearch (HEAD)

The "Web search was unavailable (model access issue)" in claudescore.md refers to the **agent instance that was working on that document**, not this session. That session was likely running with a model or API configuration that didn't support the `WebSearch` tool.

`WebSearch` is a Claude Code **tool** (not a model capability) — it's one of our built-in tools that calls the web search API on behalf of the agent. For it to work:

1. **The model must support tool use** — some models or API tiers don't support the function calling / tool use protocol that Claude Code uses. If the agent was routed to a model that lacks tool use, `WebSearch` is unavailable.
2. **The API/backend must expose it** — Some Claude Code deployments or proxy configurations may have `WebSearch` disabled or the MCP gateway may not be routing it.
3. **Enterprise/policy restrictions** — If the account or deployment has web search disabled via policy.

This session has `WebSearch` and `WebFetch` available (I can confirm they're in my toolset), so the claudescore issue was an artifact of that particular session's model configuration, not something inherent to agent-doc itself.

If you want to **verify** whether it's available in the current session, I can run a quick web search right now — want me to search for something as a smoke test?
Why is the previous response patch uncommitted?

### Re: Enabling web search with Open WebUI + Qwen (and Claude access) (HEAD)

**Open WebUI web search works with Qwen — it's model-agnostic.**

The key difference from Claude Code: Open WebUI runs web search as a **preprocessing step on its own backend**. It searches, scrapes, and injects results into the prompt before the LLM ever sees it. Qwen only receives a richer prompt — it never needs tool calling.

**To enable in Open WebUI:**
1. Admin Settings → Web Search → enable toggle
2. Pick a search provider (15+ options):
   - **No key needed:** DuckDuckGo, Jina
   - **Self-hosted:** SearXNG
   - **API keys:** Brave Search, Tavily, Google PSE, Kagi, etc.
3. Toggle the web search icon when composing messages

**For Claude (Sonnet/Opus) through Open WebUI:**

Anthropic's API is not OpenAI-compatible, so there's no direct provider config for it in Open WebUI. Two routes:

1. **OpenRouter** (easiest) — Set OpenAI-compatible URL to `https://openrouter.ai/api/v1`, add API key, then models like `anthropic/claude-sonnet-4-6` and `anthropic/claude-opus-4-6` are available
2. **LiteLLM proxy** — Run a local LiteLLM server with your Anthropic API key, point Open WebUI's OpenAI endpoint at it

Either way, web search works automatically with Claude models too — same preprocessing pipeline.
<!-- agent:boundary:931efaad -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
<!-- /agent:pending -->
