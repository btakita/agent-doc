---
description: Draft and publish LinkedIn posts via corky
user-invocable: true
argument-hint: "<command> [args]"
---

# linkedin

Draft and publish LinkedIn posts. Handles formatting constraints, link placement, and scheduling.

## Invocation

```
/linkedin draft <topic>     — draft a LinkedIn post
/linkedin post <file>       — publish a post to LinkedIn
/linkedin schedule <file>   — schedule a post for later
```

## Commands

### `draft` — Draft a Post

Write a LinkedIn post following the formatting and structure guidelines in `runbooks/linkedin-post.md`.

**Steps:**
1. Draft the post body as plain text (LinkedIn does not render markdown)
2. Move all external links to a first-comment draft
3. Save to `mail/drafts/linkedin/[YYYY-MM-DD]-[slug].md`

### `post` — Publish to LinkedIn

```bash
corky linkedin post <file>
```

Publishes the post body and optionally adds the first comment with links.

### `schedule` — Schedule a Post

```bash
corky schedule add --platform linkedin --time "YYYY-MM-DDTHH:MM" <file>
corky schedule list
corky schedule run
```

## Conventions

- **No markdown bold** — `**text**` shows as literal asterisks on LinkedIn
- **No external links in post body** — LinkedIn suppresses reach on posts with outbound URLs
- All links go in a **first comment** posted immediately after
- Use line breaks for readability
- Emojis acceptable for visual breaks but don't overdo it

## Runbooks

When drafting or posting, read and follow the linked runbook:

- `linkedin post` — [runbooks/linkedin-post.md](runbooks/linkedin-post.md)
