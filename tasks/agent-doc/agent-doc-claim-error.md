---
agent_doc_session: bef800bc-950e-4f9e-b861-92c249853397
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
Sync auto-start bug fixed. Unclaimed files no longer spawn sessions on tab navigation.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/4674cbdd322fce519fe66718682225f86b088adb9a6e63aa6f5df754c59a4ef1-20260402-030850.md`*

The JB editor is very laggy with typing. Please look in the logs.
### Re: Editor lag — Tailwind CSS LSP is the culprit

The IDEA log shows the **Tailwind CSS language service** sending `textDocument/didChange` for every single keystroke, with the **entire file content** (~180KB) in each message. Every character you type triggers a 180KB JSON payload to the Tailwind CSS LSP.

This is not an agent-doc issue — it's the Tailwind CSS IntelliJ plugin processing `.md` files it shouldn't care about.

**Fix options:**
1. **Disable Tailwind CSS for this project** (Settings → Languages & Frameworks → Tailwind CSS → uncheck Enable)
2. **Exclude .md files** from Tailwind CSS processing (if the plugin supports file type filtering)
3. **Disable the Tailwind CSS plugin entirely** (Settings → Plugins → search "Tailwind" → disable)

Option 1 is the least disruptive — agent-doc documents don't use Tailwind CSS.
Option 1
### Re: Tailwind CSS disabled

Go to Settings → Languages & Frameworks → Tailwind CSS → uncheck "Enable Tailwind CSS support for this project". This stops the LSP from processing every keystroke in `.md` files.

If the lag persists after unchecking, restart the IDE to fully stop the Tailwind CSS language server process.

Why is the file exclude not working?

```
{
  "inspectPort": null,
  "emmetCompletions": false,
  "classAttributes": ["class", "className", "ngClass", "class:list"],
  "classFunctions": [],
  "codeActions": true,
  "codeLens": true,
  "hovers": true,
  "suggestions": true,
  "validate": true,
  "colorDecorators": true,
  "rootFontSize": 16,
  "lint": {
    "cssConflict": "warning",
    "invalidApply": "error",
    "invalidScreen": "error",
    "invalidVariant": "error",
    "invalidConfigPath": "error",
    "invalidTailwindDirective": "error",
    "invalidSourceDirective": "error",
    "recommendedVariantOrder": "warning",
    "usedBlocklistedClass": "warning"
  },
  "showPixelEquivalents": true,
  "includeLanguages": {
    "ftl": "html",
    "jinja": "html",
    "jinja2": "html",
    "smarty": "html",
    "tmpl": "gohtml",
    "cshtml": "html",
    "vbhtml": "html",
    "razor": "html"
  },
  "files": {
    "exclude": [
      "**/.git/**",
      "**/.hg/**",
      "**/.svn/**",
      "**/node_modules/**",
      "**/.yarn/**",
      "**/.venv/**",
      "**/venv/**",
      "**/.next/**",
      "**/.parcel-cache/**",
      "**/.svelte-kit/**",
      "**/.turbo/**",
      "**/__pycache__/**",
			"**/*.md"
    ]
  },
  "experimental": {
    "configFile": null,
    "classRegex": []
  }
}
```
### Re: Tailwind CSS file exclude not working

The `files.exclude` config in the Tailwind CSS settings is for the **VS Code extension**, not the JetBrains plugin. The JetBrains Tailwind CSS plugin uses its own settings and doesn't read this JSON config format.

For JetBrains, the Tailwind CSS LSP is bundled and controlled by:
- Settings → Languages & Frameworks → Tailwind CSS → **Enable** checkbox (per-project)

The `files.exclude` JSON you pasted won't take effect in IntelliJ. The JetBrains plugin doesn't support per-file-type exclusion — it's all-or-nothing via the Enable checkbox.

**Recommended action:** Uncheck "Enable Tailwind CSS support" for this project. The agent-loop workspace doesn't need Tailwind CSS processing. The submodules that do use Tailwind (prescience-notes-dev, briantakita.me-dev) have their own IDEA project contexts.

If you already unchecked it and the lag persists, restart the IDE — the Tailwind CSS LSP process may still be running from the previous session.

Disabled the plugin. Still getting lag when editing ragie-web-doc.md. Why?
### Re: Lag on ragie-web-doc.md — PatchWatcher is the culprit (HEAD)

Tailwind CSS is no longer firing (confirmed in logs). The lag is now from the **PatchWatcher** in the agent-doc plugin:

```
PatchWatcher - Patch produced no changes for ragie-web-doc.md
ProgressManagerImpl - checkCancelled is invoked while write-action is pending
```

The PatchWatcher polls `.agent-doc/patches/` for patch files every few seconds. When it finds one (or checks for one), it acquires a **write-action** on the Document API to apply it. Even when the patch produces no changes, the write-action blocks the EDT (Event Dispatch Thread), causing typing lag.

This is happening every ~5 seconds — `ragie-web-doc.md` has a session running that's producing patch files that result in no-ops.

**Root cause:** The PatchWatcher shouldn't acquire a write-action for no-op patches. It should check if the patch would produce changes BEFORE locking the document.

This is a separate bug — should I investigate and fix the PatchWatcher?
<!-- agent:boundary:ed1bda9f -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] All implementation, tests, audit docs, and release complete
- [x] Published: crates.io ✓, GitHub Release ✓
- [x] CI fixes complete
- [x] Tailwind CSS disabled (was causing lag)
- [ ] Fix PatchWatcher: skip write-action for no-op patches (causes EDT lag)
- [ ] Evaluate utility of `/btw` (harness preemption) for agent-doc sync
<!-- /agent:pending -->
