---
name: reference_corky_linkedin_publish_format
description: "corky linkedin publish file format — images:/first_comment: frontmatter, dry-run proof, no silent truncation"
metadata: 
  node_type: memory
  type: reference
  originSessionId: 8b4740e4-a582-4b5d-a38d-d760a78a0977
---

`corky linkedin publish <file>` posts from `mail/social/<date>-...-linkedin.md` (the file WITH YAML frontmatter). `mail/drafts/linkedin/<date>-...md` are raw body drafts only — NOT what corky publishes.

Frontmatter the publish path reads (corky-social/src/social/{publish,linkedin,draft}.rs):
- `images:` — list of paths **resolved relative to the draft file's own dir** (`draft_dir.join(path)`); uploaded via LinkedIn initializeUpload→PUT→image-URN and attached.
- `first_comment: |` — block scalar; auto-posted via `create_comment` right after the share succeeds (no separate `corky linkedin comment` step needed).
- `status:` must be `ready` (or `scheduled_at` set) to actually publish; `draft` blocks publish but `--dry-run` works regardless.
- Body: `MAX_BODY_LENGTH = 3000`; corky **bails (errors), never silently truncates** if over.

Verify before publishing with `corky linkedin publish <file> --dry-run` — it uploads the image (proving it works), prints body+comment char counts and the image URN, creates no post. A bare frontmatter file (no `images:`/`first_comment:`) posts body-only — silently no image, no comment. Fixed the 2026-06-26 Cell Family teaser this way: added both fields, dry-run showed image uploaded + body 1553 / comment 349 chars.

Light-theme diagram render for teasers: extract the blog SVG, substitute the `var(--color-*)` CSS vars with `config/blog_site.ts` `color_scheme_vars.light` values (accent `0,108,172`, card `230,230,230`, fill `251,254,251`, text-base `40,39,40`), wrap with a bg rect + padding, `rsvg-convert -z 2`. See [[feedback_check_git_before_deferring_repinned_clean_session]].

**Truncated-post bug (FIXED corky e147ef9, 2026-06-26):** LinkedIn's Posts API `commentary` is "little Text Format" — the reserved chars `\ | { } @ [ ] ( ) < > # * _ ~` must EACH be backslash-escaped even when not a mention/hashtag, or LinkedIn silently drops the body from the first unescaped one onward. corky's `strip_linkedin_markdown` (only stripped backticks + `**`) escaped none → teaser truncated at the first `~` (`~1,000`/`~11µs`); the earlier architecture post truncated before `(Slot, Cell, Effect)`. Fix: `escape_little_text()` in `crates/corky-social/src/social/linkedin.rs`, applied in BOTH `create_post_at` AND the `update_post_at` PARTIAL_UPDATE reconcile (the reconcile re-truncates a good create otherwise). Length check stays on pre-escape text. The v2 `socialActions` comment endpoint is legacy plaintext `message.text` (annotations in `attributes`) — NOT escaped, so first-comment parens publish fine. Grammar: LinkedIn little-text-format docs, the `Text` production.
