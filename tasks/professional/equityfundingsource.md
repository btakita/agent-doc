---
agent_doc_session: 62fe1f41-886c-4f3d-b6fa-48d46106ec27
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
Blog: will use *--blog pattern. Image recovery in progress. Ready to scaffold on approval.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
It looks like https://www.equityfundingsource.com/ is down
Please crawl https://web.archive.org/web/20260207050228/https://www.equityfundingsource.com/
Let's try to reconstruct the website.
Create a private repo at btakita/equityfundingsource
Add as a git submodule in agent-loop.

Use rappstack. Create the site architecture.
### Re: Reconstruct equityfundingsource.com

I crawled the Wayback Machine archive. Here's what the site contained:

**Equity Funding Source Inc.** -- California hard money / private lending brokerage, run by Judith Krug (CEO/Broker, 26+ years). Based in Los Angeles and Malibu.

**Archived pages:**
- **Home** -- Hero ("No Stress Hard Money Private Lending"), services (Quick Approvals, Competitive Rates 8.99-12.99%, Secured Financing up to 65% LTV), funded deals preview, investor section
- **About** -- Company overview, "Quick & Easy Funding" bullet list, trust deed investor details, rate of return info, "Meet the Broker" (Judith Krug bio + photo)
- **Funded Deals** -- Gallery of 18 closed business purpose loans with property photos (locations across CA + one in Oahu, HI, amounts $50K-$1.465M)
- **Blog** -- Empty/minimal at time of archive: Use *--blog repos for blog functionality. See briantakita.me-dev
- **Broker Portal** -- Empty/minimal at time of archive

**Contact info preserved:** (323) 225-3800, (213) 596-5793, contact@equityfundingsource.com, CA DRE #01835370, NMLS #239093

I don't see css layout correctly on archive.org. Some images are missing. Can we recover?

I will contact robert-waldman for missing assets.

**Proposed rappstack architecture:**
```
equityfundingsource-dev/
├── app/equityfundingsource/     # Hono + rhonojs SSR app
├── lib/
│   ├── domain--server--efs/     # Domain logic (deals data, contact, meta)
│   ├── ui--server--efs/         # Server UI (pages, components)
│   └── ui--browser--efs/        # Client hydration (mobile menu, animations)
├── content/
│   ├── deals/                   # Funded deals (YAML/MD per deal)
│   └── blog/                    # Blog posts (markdown + frontmatter)
├── public/                      # Static assets (property images, logos)
└── package.json                 # Bun workspace
```

**Questions before scaffolding:**
1. Create repo + scaffold now, or review architecture first?
2. Property images -- download from Wayback Machine or use placeholders?
3. Google Forms links for applications -- keep external or build custom?
4. Blog + Broker Portal -- include as stubs or skip?
### Re: Blog, images, and next steps

**Blog architecture:** Agreed. I'll use the `*--blog` pattern from briantakita.me-dev -- `domain--any--blog`, `domain--server--blog`, `ui--server--blog`, `ui--browser--blog` as submodule dependencies. Blog will be a separate concern wired into the site.

**Image recovery:** Running a background check against the Wayback Machine `im_` URLs now. Archive.org often preserves images even when CSS/JS breaks. I'll report which are recoverable. For anything missing, robert-waldman can provide originals, or we use placeholders until then.

**CSS layout on archive.org:** The broken layout is expected -- Elementor generates dynamic CSS that Wayback Machine doesn't always capture correctly. We're rebuilding with Tailwind anyway, so we just need the content and images, not the original CSS.

**Proceeding with scaffolding:**
1. Create private repo `btakita/equityfundingsource-dev` (the `-dev` suffix per rappstack convention)
2. Add as submodule at `src/equityfundingsource-dev`
3. Scaffold rappstack structure with blog submodule deps
4. Google Forms links kept as-is (external)
5. Broker Portal as stub page

Waiting for your go-ahead to create the repo and start scaffolding.
<!-- agent:boundary:2c878719 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Create private repo btakita/equityfundingsource-dev
- [ ] Add as git submodule in agent-loop/src/
- [ ] Scaffold rappstack project structure (with blog deps)
- [ ] Recover images from Wayback Machine
- [ ] Contact robert-waldman for missing assets (user action)
- [ ] Build Home page
- [ ] Build About page
- [ ] Build Funded Deals page
- [ ] Blog setup (briantakita.me-dev pattern)
- [ ] Broker Portal page (stub)
- [ ] Tailwind styling
- [ ] Deploy configuration
<!-- /agent:pending -->
