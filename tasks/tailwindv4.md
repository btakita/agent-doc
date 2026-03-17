---
agent_doc_session: a90ef1a3-df78-4fda-bb96-679789416f14
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

## Overview
@rebuildjs/tailwindcss needs to migrate from Tailwind v3 API to v4. The plugin at src/ctx-core-dev/lib/rebuildjs-tailwindcss/ calls tailwind({...config}) which no longer exists in v4.

## Scope
- Source: src/ctx-core-dev/lib/rebuildjs-tailwindcss/
- Current version: 0.10.60 (locked to Tailwind v3)
- Target: Tailwind v4 (@tailwindcss/postcss)

## Migration Steps
- [ ] Replace `tailwind()` PostCSS plugin call with `@tailwindcss/postcss`
- [ ] Update config handling — v4 uses CSS-based `@theme` directives
- [ ] Update package.json dep from `tailwindcss: ^3.4.18` to `^4.x`
- [ ] Fix type imports (`Config` type changed in v4)
- [ ] Update test fixtures for v4 CSS output format
- [ ] Test with briantakita.me build
- [ ] Publish updated @rebuildjs/tailwindcss to npm

## Key Changes in Tailwind v4
- No JavaScript-based API for main tailwind processor
- Must use @tailwindcss/postcss as PostCSS plugin
- Configuration via CSS @theme directives or @config directive
- safelist, corePlugins, separator options removed

## Overview
@rebuildjs/tailwindcss needs to migrate from Tailwind v3 API to v4. The plugin at src/ctx-core-dev/lib/rebuildjs-tailwindcss/ calls tailwind({...config}) which no longer exists in v4.

## Scope
- Source: src/ctx-core-dev/lib/rebuildjs-tailwindcss/
- Current version: 0.10.60 (locked to Tailwind v3)
- Target: Tailwind v4 (@tailwindcss/postcss)

## Migration Steps
- [ ] Replace `tailwind()` PostCSS plugin call with `@tailwindcss/postcss`
- [ ] Update config handling — v4 uses CSS-based `@theme` directives
- [ ] Update package.json dep from `tailwindcss: ^3.4.18` to `^4.x`
- [ ] Fix type imports (`Config` type changed in v4)
- [ ] Update test fixtures for v4 CSS output format
- [ ] Test with briantakita.me build
- [ ] Publish updated @rebuildjs/tailwindcss to npm

## Key Changes in Tailwind v4
- No JavaScript-based API for main tailwind processor
- Must use @tailwindcss/postcss as PostCSS plugin
- Configuration via CSS @theme directives or @config directive
- safelist, corePlugins, separator options removed

## Exchange
<!-- agent:exchange -->
### Compacted Exchange History

**Library migration (@rebuildjs/tailwindcss):**
- Swapped `tailwindcss` → `@tailwindcss/postcss`, removed JS config API
- Fixed reactive build loop (use `__persist` instead of `__update`)
- Transform `@tailwind` → `@import "tailwindcss"` before PostCSS
- All 4 tests pass. Published `@rebuildjs/tailwindcss@0.11.0` then `@0.11.1` (Oxide engine file-write fix)

**briantakita.me consumer migration:**
- Deleted `tailwind.config.ts` (triggered v3 compat mode)
- Moved all config to CSS: `@theme`, `@utility` (15 skin classes), `@plugin "@tailwindcss/typography"`
- Removed dead `tailwindcss_config` from `build.ts`

**v4 breaking changes fixed (5 total):**
1. `outline-2` now sets `outline-style: solid` → use `outline-hidden` as default
2. `@source` directives needed for content scanning outside CSS project root
3. Typography plugin outputs to `@layer utilities` → prose overrides must be unlayered
4. Preflight no longer resets heading/p/pre margins → add explicit `margin: 0` reset
5. Unlayered prose overrides beat `:where()` first/last-child rules → add explicit margin rules

**YouTube videos:** All 3 dogfooding videos set to public via corky. Corky defaults already correct (`visibility: public` at every stage since v0.15.1).

**Deployed:** Committed + pushed. GitHub Actions run `23092709527` → Cloudflare Pages.

**Remaining:**
- [ ] Add smoke test (build exits 0)
- [ ] Add CSS assertion test (verify skin utilities, prose styles)
- [ ] Add E2E test with lightpanda headless browser
- [ ] Fix deprecated `selection:bg-opacity-70` → `selection:bg-skin-accent/70` in templates
<!-- /agent:exchange -->
