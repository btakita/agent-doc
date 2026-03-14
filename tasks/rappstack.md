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

### Re: CI fix results

**All 4 fixes applied. 4/6 apps now deploying successfully:**

| App | URL | CI | Fix Applied | Remaining Issue |
|-----|-----|-----|------------|-----------------|
| briantakita.me-dev | briantakita.me | ✅ DEPLOYED | — | — |
| prescience-notes-dev | pn.briantakita.me | ✅ DEPLOYED | — | — |
| brookebrodack-dev | brookebrodack.net | ✅ DEPLOYED | Plugin v0.0.3 + skip ready wait | — |
| cogov-dev | cogov.me | ✅ DEPLOYED | Removed plugin (R2 files unhashed), published plugin v0.0.4 with CSS url-token fix | — |
| drmanny-dev | pe.briantakita.me | ❌ FAIL | Added `marked-highlight` dep | `@btakita/domain--server--peakexistence/jsonld` — missing exports map entry |
| herbaliciousbliss-dev | hb.briantakita.me | ❌ FAIL | Skipped ready wait in CI | `@btakita/ui--browser--herbaliciousbliss/layout` — missing exports map entry + `tailwindcss` not resolved |

**drmanny-dev and herbaliciousbliss-dev** have pre-existing module resolution issues — packages missing `exports` map entries for subpath imports. These aren't related to the rappstack migration or Tailwind v4 upgrade.

Want me to fix the remaining 2?

Yes
<!-- /agent:exchange -->
