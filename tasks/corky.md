---
session: 584ef929-37ee-47f6-b3d8-d4266ad31ee9
resume: null
agent: null
model: null
branch: null
---

## User

Let's enable corky to send attachments.

The attachment may be a screenshot that is in the Claude session memory.

## Assistant

**Design: attachment support for corky drafts**

### Draft format

Attachments listed in YAML frontmatter:
```yaml
---
to: alice@example.com
subject: Meeting notes
attachments:
  - /tmp/screenshot-2026-02-27.png
  - ~/Documents/report.pdf
---

Here are the meeting notes...
```

### CLI

```
corky draft --to alice --subject "Notes" --attach screenshot.png --attach doc.pdf
corky send draft.md    # sends with attachments as MIME multipart/mixed
```

### Implementation

1. **`DraftConfig` update** — add `attachments: Vec<PathBuf>` to frontmatter struct
2. **MIME encoding** — switch from plain text to `multipart/mixed` when attachments present. Use `lettre`'s `SinglePart::binary()` + `MultiPart::mixed()` for proper MIME encoding (corky already uses `lettre` for SMTP)
3. **Content-type detection** — `mime_guess` crate for auto-detecting attachment types (image/png, application/pdf, etc.)
4. **File validation** — check files exist and are readable at send time, not draft time (files might move)

### Screenshot from Claude

Claude Code writes screenshots to `/tmp/` as `.png` files. The user passes the path:
```
corky draft --to alice --attach /tmp/claude-screenshot-abc123.png
```

Corky doesn't need Claude awareness — it just attaches whatever file path is given. The Claude agent (or user) provides the path.

### Phases

1. **Phase 1:** `--attach` flag on `corky draft` + attachment sending via `corky send`
2. **Phase 2:** Inline image embedding (for HTML emails, images displayed in body)
3. **Phase 3:** Clipboard attachment (`--attach clipboard` grabs current clipboard image)

Want me to implement Phase 1?

## User

Implement Phase 1
