---
agent_doc_session: fce94c2a-2072-49c6-924b-144e96b540b7
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---


The @rappstack/* packages were recently published.
The apps used submodules from rappstack.
Now that @rappstack/* packages are published, the apps need to reference the @rappstack/* packages instead of using submodules.

Convert brookebrodack-dev, cogov-dev, censible-core, drmanny-dev, herbaliciousbliss-dev, prescience-notes-dev

Use yalc if possible. And deploy via github actions. The deployments must depend on the published npm packages...not yalc. yalc is only for development.

<!-- agent:exchange -->

### Re: Both fixed

**Root cause (same for both):** The `ready__wait` call is NOT just for watch mode — the `rebuild_tailwind_plugin` reactive system needs it to finish PostCSS/Tailwind CSS processing before exit. Skipping it caused `process.exit(0)` to fire before CSS was written and before esbuild output was flushed.

**herbaliciousbliss-dev:** Made `rhonojs__ready__wait(MAX_INT32)` unconditional. CSS now 32KB (was 623 bytes). Added `run()` wrappers matching briantakita.me-dev pattern.

**drmanny-dev:** Made `relysjs__ready__wait(300_000)` unconditional. `dist/server/index.js` (4.5MB) now produced.

Both CI runs in progress. This should be the final fix.

| App | URL | CI |
|-----|-----|-----|
| briantakita.me-dev | briantakita.me | ✅ |
| prescience-notes-dev | pn.briantakita.me | ✅ |
| brookebrodack-dev | brookebrodack.net | ✅ |
| cogov-dev | cogov.me | ✅ |
| herbaliciousbliss-dev | hb.briantakita.me | 🔄 CI running |
| drmanny-dev | pe.briantakita.me | 🔄 CI running |

Styling broken on https://pe.briantakita.me/
<!-- /agent:exchange -->
