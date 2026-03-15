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

### Re: herbaliciousbliss CSS + drmanny lockfile

**herbaliciousbliss CSS:** Only 623 bytes — Tailwind v4 classes aren't generated. `@import "tailwindcss"` + `@source` directives produce nearly empty output with `esbuild.build()`. The `rebuild_tailwind_plugin` PostCSS processing may behave differently in one-shot mode. Investigating.

**drmanny-dev:** Workspace packages resolved to stale `rebuildjs@0.70.46`. Regenerated lockfile, all on 0.70.47 now. CI running.

| App | URL | CI |
|-----|-----|-----|
| briantakita.me-dev | briantakita.me | ✅ |
| prescience-notes-dev | pn.briantakita.me | ✅ |
| brookebrodack-dev | brookebrodack.net | ✅ |
| cogov-dev | cogov.me | ✅ |
| herbaliciousbliss-dev | hb.briantakita.me | ⚠️ CSS broken |
| drmanny-dev | pe.briantakita.me | 🔄 CI running |

Ok...fix hb.briantakita.me. What are you waiting for?
<!-- /agent:exchange -->
