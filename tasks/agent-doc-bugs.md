---
agent_doc_session: 92fba2ac-9d1f-4a7a-8764-821a8a08ecd0
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

<!-- agent:exchange -->

### Re: Duplicate component tags bug

**Root cause:** `template.rs::parse_patches()` had no code-block awareness. When a response contained component/patch markers inside a fenced code block (e.g., showing examples), it treated them as real patch directives — creating duplicate component tags after CRDT merge.

The component parser (`component.rs::parse()`) already correctly skips markers inside code blocks via `find_code_ranges()`. The template patch parser did not.

**Fix (template.rs):**
1. Opening markers: Added `find_code_ranges()` call + check to skip markers inside code ranges
2. Closing markers: New `find_outside_code()` helper prevents closing markers in code blocks from prematurely terminating real patch blocks

**5 new tests:**
- Markers inside ``` blocks are preserved as content (not parsed as patches)
- Same for ~~~ blocks
- Closing marker insid cdblckdoen'teminrelp
Nralmarkerssill(ersngur)

All 351 passclippy cle.

Shall I releasev0.17.8 with this ?<!-- /agent:exchange -->
,.
