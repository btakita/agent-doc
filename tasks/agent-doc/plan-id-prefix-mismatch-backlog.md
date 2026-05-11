# Plan: Backlog item ID prefix mismatch

## Bug
Backlog item `[#2fha]` uses an internal ID instead of the declared bug tag `[#nopatchbackopencode]`. The inline text says `2026-05-11 [#nopatchbackopencode]` but the checklist prefix is `[#2fha]`.

## Root cause
When a bug is captured via `#agent-doc-bug` declaration chain, the skill assigns an internal short ID (`#2fha`) instead of using the declared tag (`#nopatchbackopencode`) as the primary ID. This is related to `#bugchainorder` (declaration chain capture issues).

## Fix
1. When parsing `#agent-doc-bug` declarations that include an explicit `[#tag]` inline, use that tag as the checklist ID prefix instead of generating a new random short ID.
2. Fall back to short ID generation only when no explicit tag is provided.
3. Add a validation step that the `[#id]` prefix matches any `[#tag]` declared in the item text.

## Reproduction
- Backlog item in agent-doc-bugs2.md: `- [ ] [#2fha] 2026-05-11 [#nopatchbackopencode] OpenCode agent-doc turn completed but...`
- Expected: `- [ ] [#nopatchbackopencode] 2026-05-11 OpenCode agent-doc turn completed but...`
