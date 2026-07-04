---
name: Markdown Annotations prior art
description: iA Writer's Markdown-Annotations spec for content source coloring in agent-doc editor view
type: reference
---

**Prior art for content-source annotation:** https://github.com/iainc/Markdown-Annotations

The iA Writer editor color-codes annotations in markdown. This is a candidate "standard" for agent-doc's planned editor view feature that colors user vs agent content differently. Discovered during eval-runner session (2026-04-04) while discussing deterministic preprocessing vs LLM interpretation for blockquote attribution.

**How to apply:** When implementing content-source annotation in agent-doc (the editor view coloring feature), evaluate iA's Markdown-Annotations spec as the base format. See also `tasks/recruit/robert-ross.md` and `tasks/recruit/robert-ross-granola.md` for the design reference that prompted this direction.
