# Browser Paste Workflow

Fallback for sending HTML-formatted emails when `corky draft push` is unavailable.

## Generate HTML

1. Read the draft markdown file
2. Convert to styled HTML with Gmail-compatible inline CSS:
   - `font-family: Arial, sans-serif; font-size: 14px; color: #333; line-height: 1.6`
   - Headings: `color: #1a1a1a; border-bottom: 1px solid #ddd`
   - Subheadings: `color: #444`
3. Save to `/tmp/<slug>.html`
4. Print `file:///tmp/<slug>.html` for the user to open

## Paste into Gmail

1. Open the `file:///tmp/<slug>.html` URL in browser
2. Select all (Ctrl+A)
3. Copy (Ctrl+C)
4. Paste into Gmail compose window (Ctrl+V)
5. Always use `file:///` protocol paths (not bare `/tmp/...`)
