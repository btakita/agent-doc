---
agent_doc_session: 62fe1f41-886c-4f3d-b6fa-48d46106ec27
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
Archived site crawled. Architecture proposed. Awaiting decisions before scaffolding.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
It looks like https://www.equityfundingsource.com/ is down
Please crawl https://web.archive.org/web/20260207050228/https://www.equityfundingsource.com/
Let's try to reconstruct the website.
Create a private repo at btakita/equityfundingsource
Add as a git submodule in agent-loop.

Use rappstack. Create the site architecture.
### Re: Reconstruct equityfundingsource.com (HEAD)

I crawled the Wayback Machine archive. Here's what the site contained:

**Equity Funding Source Inc.** -- California hard money / private lending brokerage, run by Judith Krug (CEO/Broker, 26+ years). Based in Los Angeles and Malibu.

**Archived pages:**
- **Home** -- Hero ("No Stress Hard Money Private Lending"), services (Quick Approvals, Competitive Rates 8.99-12.99%, Secured Financing up to 65% LTV), funded deals preview, investor section
- **About** -- Company overview, "Quick & Easy Funding" bullet list, trust deed investor details, rate of return info, "Meet the Broker" (Judith Krug bio + photo)
- **Funded Deals** -- Gallery of 18 closed business purpose loans with property photos (locations across CA + one in Oahu, HI, amounts $50K-$1.465M)
- **Blog** -- Empty/minimal at time of archive
- **Broker Portal** -- Empty/minimal at time of archive

**Contact info preserved:** (323) 225-3800, (213) 596-5793, contact@equityfundingsource.com, CA DRE #01835370, NMLS #239093

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
<!-- agent:boundary:5284e6aa -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Create private repo btakita/equityfundingsource-dev
- [ ] Add as git submodule in agent-loop/src/
- [ ] Scaffold rappstack project structure
- [ ] Build Home page
- [ ] Build About page
- [ ] Build Funded Deals page
- [ ] Download/source property images
- [ ] Blog page (stub)
- [ ] Broker Portal page (stub)
- [ ] Tailwind styling to match original design
- [ ] Deploy configuration
<!-- /agent:pending -->
