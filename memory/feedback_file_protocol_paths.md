---
name: Use file:// protocol for browser-openable paths
description: When saving HTML or other files for the user to open in a browser, use file:/// protocol paths so they can click directly
type: feedback
---

When saving files for the user to open in a browser (e.g., HTML email drafts), always present the path with `file:///` protocol prefix (e.g., `file:///tmp/ripple-reply.html`) instead of just the filesystem path (`/tmp/ripple-reply.html`).

**Why:** The user wants to click the path directly to open it in their browser. Bare filesystem paths aren't clickable in most terminals/editors.

**How to apply:** Any time you save a file intended for browser viewing, present the path as `file:///absolute/path/to/file.html`.
