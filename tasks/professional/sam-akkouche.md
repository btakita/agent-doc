---
agent_doc_session: 3af2ebc5-cc8e-487f-b92c-f5bf3f7e9e00
agent_doc_format: template
agent_doc_write: crdt
agent: claude
claude_model: opus
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the acklog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/59616156e6ea8847d9ce915f0b61a469a80f3ec14e99ef4e63be8fb1fa5e6dcd-20260601-024705.md`*

Compacted content:
- Archived 8 response topic(s): Create a corky contact for sam-akkouche; Create a resume for the sam-akkouche call (tomorrow 11am); More agentic summary (focus Dreamable + agent-doc, mention PresenceAI); 5 more
- Prior summary/context: ❯ Create a corky contact for sam-akkouche. https://www.linkedin.com/in/sam-akkouche/
❯ Don't mention WordPress-vs-Shopify. Mention mitigating bot attack with fraudulent orders using CloudFlare Turnstile, WordPress security, Paypal. Please also look at my commit history to find more details about what is relevant for high-end work.
❯ > Identified and documented an admin-account security compromise with a remediation path, evaluated WordPress-vs-Shopify migration,

### Re: Drop WP-vs-Shopify; add bot/fraud mitigation (Turnstile, WordPress security, PayPal) — opus-4-8

Done — rewrote the MonsterRodHolders Experience entry in `resume/data.toml` and rebuilt the PDF. I mined the monsterrodholders work record (the `bot-attack-mitigation` and `card-testing-bot-attack` correspondence) for accurate, high-end detail. New entry:

> **MonsterRodHolders.com** — Rescued a failing WooCommerce storefront (57-SKU catalog) and grew MRR 50% in one month through SEO and Google Ads. Diagnosed a **card-testing bot attack** (170+ fraudulent PayPal orders in two weeks, threatening merchant-account standing) from order-database analysis and mitigated it — deployed **Cloudflare Turnstile** on checkout to block headless bots, removed the low-cost product the bots used to validate stolen cards, and drove **PayPal fraud controls** (3D Secure, AVS/velocity filters). Hardened **WordPress security** after an admin-account compromise, configured Cloudflare CDN for performance and DDoS protection, and restored analytics tracking. WordPress, WooCommerce, Cloudflare Turnstile, PayPal, SEO, Google Ads, Cloudflare CDN, Google Analytics.

**Changes:**- **Removed** "evaluated WordPress-vs-Shopify migration."
- **Added** the bot/fraud mitigation with **Cloudflare Turnstile**, **WordPress security** hardening, and **PayPal** fraud controls.
- Sourced from your commit/correspondence history — the high-end signal here is *diagnosing* a card-testing attack from order-DB analysis (170+ fraudulent orders, real-customer silent-decline risk) and *collapsing the attack economics* (removing the $1 stolen-card validation product), not just installing a CAPTCHA.

**Verification:** clean rebuild; Shopify reference gone (0), Turnstile present, **Dreamable leak = 0**.

Note: this entry is now detailed (4 sentences) — appropriate for an Experience bullet, but if you want it tighter for one-page density, say so and I'll compress the fraud/security clause.

Still uncommitted across `resume/` + `mail/` — no `#commit-push` invoked. (Exchange remains at `block`-level accretion — compaction offer still stands.)
Should I mention real-time Voice Agent with On Demand asset generation? Will this break my agreement with Dreamable?

### Re: Mention real-time Voice Agent + on-demand asset generation? Does it break the Dreamable agreement? — opus-4-8

Short answer: **describe it as a generic capability, not as their product — and don't put it on the resume as something you "built for the client" without checking your actual NDA.** I can't read your agreement, so treat this as risk-reduction guidance, not a legal ruling.

**Why the specific phrasing is risky:**
"Real-time Voice Agent with on-demand asset generation" is a **product-architecture description** — it tells a reader *what Dreamable's product does*, even with the name redacted. The standing confidentiality rule for that engagement is **skills-only: never the name and never the product architecture**. A distinctive product concept can identify the company (or its IP) as surely as the name, so this crosses the line the rule is drawing.

**The safe vs. risky distinction:**
- ✅ **Safe (capability/skill framing):** "real-time voice AI," "streaming inference and generative media pipelines," "real-time multimodal agents" — listed as *your expertise*, tied to the tech (LiveKit, WebRTC, streaming inference, CopilotKit). This is what your resume already does ("real-time AI inference and streaming pipelines").
- ⚠️ **Risky (product framing):** "At [Confidential AI Startup], built a real-time Voice Agent that generates assets on demand" — that's narrating their specific product, which is exactly what the agreement protects.

**My recommendation:**
1. **Keep the resume capability-level.** If you want more voice/generative signal, I can add a generic skill like "real-time voice agents" or "on-demand/generative media pipelines" to the Confidential AI Startup line — framed as transferable expertise, not their product.
2. **Verify against the actual NDA** before going more specific. If your agreement only restricts the *name*, you have more latitude; if it restricts product/IP details (most do), keep it generic. When in doubt, generic wins — and you can always describe the specifics verbally to Sam under his agency's own confidentiality.

Want me to add a generic "real-time voice / generative media" capability phrase to the Confidential AI Startup clause? I'll keep it skills-only and re-verify the Dreamable leak check stays 0.

(Noted your edit merging the "Changes:" line in the prior response — left as-is.) Still uncommitted across `resume/` + `mail/`; exchange still at `block`-level accretion.
❯ Use the safe description.

### Re: Use the safe description — opus-4-8

Done — added the capability-level voice/generative phrasing to the Confidential AI Startup line in `resume/data.toml` and rebuilt the PDF. The entry now reads:

> **Confidential AI Startup** — Real-time AI systems engineering (Apr–May 2026): built **real-time voice AI and generative media pipelines** — low-latency streaming inference and agent-harness analysis tooling — and delivered investor-facing demos under tight timelines. Rust, Python, TypeScript, LiveKit, WebRTC, CopilotKit.

**Kept it skills-only:** "real-time voice AI and generative media pipelines" reads as transferable expertise (the kind many AI teams build), not as a description of their specific product. Added `voice AI` and `generative media` to the entry's tags too.

**Confidentiality verification:**
- `dreamable` leak = **0**
- "voice agent" (their product phrasing) = **0**
- "on-demand asset" (their feature phrasing) = **0**
- Name still rendered only as "Confidential AI Startup"

So you get the voice/generative signal for Sam without narrating Dreamable's product. As noted before, if you ever want to go more specific, check it against the actual NDA first — verbal detail to Sam under his agency's confidentiality is the safer channel.

Still uncommitted across `resume/` + `mail/` — no `#commit-push`. Exchange remains at `block`-level accretion (compaction offer stands).
<!-- agent:boundary:2e5e5f68 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done -->
- 2026-05-31 [#j52j] [recommended] Sync MonsterRodHolders growth story into resume/data.toml Experience accomplishment (add SEO + Google Ads + 50% MRR result; optional (2026) recency tag) so it backs up the Career Highlight bullet
<!-- /agent:done -->
