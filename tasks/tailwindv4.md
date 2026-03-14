---
agent_doc_session: a90ef1a3-df78-4fda-bb96-679789416f14
tmux_session: '0'
agent_doc_mode: stream
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

## Exchange
<!-- agent:exchange -->
Go
<!-- /agent:exchange -->
