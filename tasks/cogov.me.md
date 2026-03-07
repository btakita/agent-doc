---
agent_doc_session: 159f6a0e-f91f-4485-87e2-7e85474bb00d
tmux_session: '0'
---

When a user visits https://cogov.me/protocol.love, the download document comes up with a file name protocol.love.I think this should be a webpage.
## Assistant

**Bug found and understood.**

`https://cogov.me/protocol.love` returns `content-type: application/octet-stream` (binary download) instead of `text/html`.

**Root cause:** The static export function `static_export__file_path_()` in `relysjs/server/export/index.js:152` treats any route with a non-`.html` extension as a "raw file":

```js
const ext = extname(route)
if (ext && ext !== '.html') {
    return join(out_dir, route)  // saves as bare file, no index.html
}
```

The route `/protocol.love` has extension `.love`, so it's saved as `dist/browser/protocol.love` instead of `dist/browser/protocol.love/index.html`. CF Pages uses the extension to determine Content-Type, and `.love` is unknown → `application/octet-stream` → download.

**Fix options:**

1. **Quick fix (cogov-dev):** Add a `_headers` file to the static output:
   ```
   /protocol.love
     Content-Type: text/html; charset=utf-8
   ```

2. **Proper fix (relysjs):** Change `static_export__file_path_()` to use an allowlist of known raw-file extensions instead of treating everything non-`.html` as raw:
   ```js
   const raw_extensions = new Set(['.txt', '.xml', '.json', '.svg', '.ico', '.webmanifest'])
   if (ext && raw_extensions.has(ext)) {
       return join(out_dir, route)
   }
   ```

Option 2 is the correct long-term fix — it prevents this class of bug for any future route with a dot in the path. Option 1 is a quick workaround.

Which approach would you prefer? Or both (quick fix now, proper fix in relysjs)?

## User

The /protocol.love route should be inferred that it is an html document somewhere. The SSR behvarior was HTML. So we should infer HTML for the static rendering.
