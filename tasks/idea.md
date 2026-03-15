---
agent_doc_session: a1344531-cfa3-4c62-99ee-f18fb9231782
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

How do I exclude directories from idea file search?

<!-- agent:exchange -->
### Re: Exclude directories from IDEA file search

Several approaches, from most to least permanent:

**1. Mark as Excluded (permanent, affects indexing)**
- Right-click directory in Project panel > **Mark Directory as** > **Excluded**
- Or: **File > Project Structure** (`Ctrl+Alt+Shift+S`) > select module > click directory > **Excluded** button
- Stored in `.iml` file as `<excludeFolder url="..." />`

**2. Custom Search Scope (per-search, reusable)**
- In **Find in Files** (`Ctrl+Shift+F`) > **Scope** dropdown > **Custom**
- Define in **Settings > Appearance & Behavior > Scopes**
- Pattern syntax: `!file:node_modules//*&&!file:.yalc//*&&!file:dist//*`

**3. File type / pattern filter**
- In Find in Files, use the **File mask** field to include/exclude by pattern

**For this workspace**, good candidates to exclude:
- `node_modules/`, `.yalc/`, `target/`, `dist/`, `build/`
- Submodule build outputs (e.g., `src/*/target/`, `src/*/node_modules/`)
<!-- /agent:exchange -->
