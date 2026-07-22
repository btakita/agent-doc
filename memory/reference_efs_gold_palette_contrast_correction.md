---
name: reference_efs_gold_palette_contrast_correction
description: EFS gold accent changed
metadata: 
  node_type: memory
  type: reference
  originSessionId: 72efa082-3e64-49a1-b17a-8297847172ce
---

EFS brand gold was the pale `#C9A227` (≈2.4:1 on white — failed contrast, e.g. the home rate-estimator slider thumb). Corrected 2026-07-10 to a high-contrast gold, split by the surface it sits on:

- `--color-efs-gold: #A67C00` — primary accent on **light** surfaces (slider thumb, buttons, badges, borders). Clears 3:1 on white (3.82), the pale bg (3.47), the slider track (3.08) AND navy (3.48).
- `--color-efs-gold-light: #E8BB2D` — kept; gold text/marks on **navy only** (hero, stats, sidebar brand). ~1.9:1 on white, so never place on light.
- `--color-efs-gold-dark: #7E5D0A` — deepened from #9B7A14; small gold **text** on light (eyebrows, step labels, hover). 6.07:1 on white — AA.
- Standardized overlay tokens added: `--color-efs-gold-tint`/`-tint-hi`/`-border`/`-ring` (rgba(166,124,0,…)). Use these instead of ad-hoc low-alpha rgba; focus rings use `--color-efs-gold-ring` (0.45) so they stay visible on white.
- Slider RAIL fix (follow-up): the unfilled track used `--color-efs-border` #E5E7EB (~1.2:1 on white — nearly invisible, the "slider background too low contrast" complaint was about the rail, not the thumb). Added `--color-efs-track: #7E8794` (3.6:1 on white), track height 6→7px, thumb margin-top -6→-5.5px. Track rules live in the FULL stylesheet only (`.efs-range::-webkit-slider-runnable-track`/`::-moz-range-track` in index.css.ts); critical CSS has only the `.efs-range` base. Deployed worker version b3f491cf.
- How-it-works watermark numerals (01/02/03) in `lib/ui--server--efs/home/index.ts` used `--color-efs-gold` at `opacity:0.15` (~1.1:1 — invisible). Dropped the opacity so they render at full `--color-efs-gold` (#A67C00, 3.82:1). Deployed 5aeb682c. NOTE: live home step TEXT (titles/bodies) is DB-backed (public_content), but the numeral span + its styling is in the home template code, so template edits take effect.
- DEPLOY-VERIFY GOTCHA: the prod home HTML is **edge-cached** at `equityfundingsource.brian-takita.workers.dev`. Right after a deploy the default URL may still serve the OLD HTML; verify the origin worker with a cache-buster query (`/?v=<ts>`) — it refreshes within seconds. CSS assets are content-hashed so they don't have this lag.

Tokens are defined **twice** (keep in sync): `@theme` in `app/equityfundingsource/app/index.css.ts` (full stylesheet) AND `:root` in `lib/ui--server--efs/styles/index.ts` (inline critical CSS). Repo also had ~95 hardcoded `#C9A227` literals + rgba(201,162,39,…) overlays swept to the new values across lib/ and app/.

Interaction rule: gold hovers **deepen** to gold-dark (slider thumb, `.efs-btn-gold`, `.efs-hero-btn-gold`, `.efs-mobile-nav-cta`, portal `.btn-g`) — never lighten to pale gold-light.

Added `:root{color-scheme:light}` (both CSS sources) so browser/OS auto-dark/force-dark can't invert the fixed light palette. Note: a Chromium launched with force-dark still inverts even with this set — for truthful screenshots run headless with `--disable-features=WebContentsForceDark,ForceWebContentsDarkMode,AutoDarkMode` (chromium-bridge's shared instance has force-dark on).

New live color styleguide at **`/styleguide`** — module `lib/ui--server--efs/styleguide/index.ts` (export `./styleguide` in that package.json). Shows the full palette, per-swatch contrast ratios, gold-by-surface rules, transparency overlays, and live slider/button/badge samples.

GOTCHA: page routes must be registered in **BOTH** `app/equityfundingsource/app/index.server.ts` (dev `bun run start` entry) **AND** `app/equityfundingsource/worker.ts` (the Cloudflare deploy entry). Adding only to index.server.ts works in dev but 404s in prod. Mirror the `/about` block in each. Deployed 2026-07-10 (worker version ddf5034d); prod = `equityfundingsource.brian-takita.workers.dev` (custom domain equityfundingsource.com may be unresolvable from a sandbox — verify on the workers.dev URL via string-grep).

CSS lives inside a JS template literal (`export default () => \`...\``) — never use backticks in CSS comments there; it terminates the template and the esmcss build fails with an opaque error. See [[reference_efs_verify_deploy_via_string_grep]] and [[feedback_concurrent_session_clobbers_efs_worker_deploy]] for deploy hazards.
