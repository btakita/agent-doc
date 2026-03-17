---
agent_doc_session: b503b48b-1422-4a94-84c1-20fd3ca007d7
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---
<!-- agent:status mode=replace -->
- [x] SVG logo created (two variants)
- [ ] Review: with eyes vs without eyes
- [ ] Pick final version
<!-- /agent:status -->

<!-- agent:exchange mode=append -->
In agent-doc, create a svg logo.
The logo should be a markdown logo wearing a fedora.
Like a markdown file that is an agent.

Created SVG logo at `src/agent-doc/logo.svg`.

**Design:**
- White document page with folded corner (classic file icon shape)
- Brown fedora hat with gold band on top — gives it the "agent" look
- Markdown symbol (M + down arrow) in the document body
- Two cartoon eyes between the hat and the markdown symbol — personality/agency
- Drop shadow for depth, gradient fills on hat and document

Open `src/agent-doc/logo.svg` in a browser to preview. Let me know if you want adjustments — hat style, colors, eye expression, markdown symbol size, etc.

The brim of the hat should be in front of the document.

Let's see how it looks with an without the eyes.

### Updated: brim in front + two variants

**Fixed layering:** Hat crown and band render behind the document, but the brim now renders **in front** of the document (drawn after the document in SVG paint order). This gives the correct depth effect — the document tucks under the hat brim.

**Two files:**
- `src/agent-doc/logo.svg` — with eyes (agent personality)
- `src/agent-doc/logo-no-eyes.svg` — without eyes (cleaner/more professional)

The no-eyes version centers the markdown symbol slightly higher since there's more vertical space. Open both in a browser to compare.

Better without eyes.
Make the hat look more like a fedora.
4Something a secret agent would wear.
<!-- /agent:exchange -->
