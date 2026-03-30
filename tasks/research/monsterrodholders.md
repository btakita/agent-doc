---
agent_doc_session: bec81dd7-71a1-488e-b716-8a0622713142
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
Site crawled. 57 products, 79 pages, 20 blog posts (11 are spam). WordPress/WooCommerce platform with possible SEO compromise.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->

### Assistant
MonsterRodHolders research session initialized. What would you like to research? For example:

- Product line and pricing
- Materials and construction
- Competitor comparison
- Customer reviews and reputation
- Where to buy / availability

Please crawl:
https://monsterrodholders.com/
Look at the site directory structure.
https://monsterrodholders.com/wp-sitemap.xml

### Re: Site crawl (HEAD)

Crawled the homepage and all sitemap XML files. Here's the full directory structure:

#### Site Overview
- **Platform:** WordPress + WooCommerce
- **Company:** Family-owned, Lebanon Junction, KY (since 2007)
- **Focus:** Heavy-duty fishing rod holders for crappie, catfish, and shark fishing

#### Navigation (13 top-level categories)
Rod Holders | Rod Holder Bases | Racks & T-Bars | Rod Transport Accessories | Mounting Hardware | Boat Accessories | HydroWeb Landing Nets | River Anchors | Bank Fishing Products | Kayak Products | Apparel & Decals | Gift Cards | Gear & Guidance (Blog)

#### Sitemap Index (8 sub-sitemaps)
| Sitemap | Count | Content |
|---------|-------|---------|
| `wp-sitemap-posts-product-1.xml` | 57 | All products |
| `wp-sitemap-posts-page-1.xml` | 79 | Static pages |
| `wp-sitemap-posts-post-1.xml` | 20 | Blog posts |
| `wp-sitemap-taxonomies-product_cat-1.xml` | 17 | Product categories |
| `wp-sitemap-taxonomies-category-1.xml` | — | Post categories |
| `wp-sitemap-taxonomies-post_tag-1.xml` | — | Post tags |
| `wp-sitemap-taxonomies-product_tag-1.xml` | — | Product tags |
| `wp-sitemap-taxonomies-product_shipping_class-1.xml` | — | Shipping classes |

#### Products (57 items)
**Rod Holders:**
- Double Action 3345 Rod Holder (without economy base)
- Talon Double Action 5-35
- Anchoring Tournament Series Package Deal (3345)
- 8-inch Rod Holder Extension

**Rod Holder Bases:**
- Economy Base (2-pack, 4-pack)
- Double Duty 55 Aluminum Base
- X-tra Duty Base
- Jon Boat Gunwale Base
- G3 Rail Base
- 7/8" Round Rail Base
- 1" Round Rail Base
- 1" Round Rail Deluxe
- Square Rail Base (Pontoons)
- Track Rail Base (SeaArk Boats)
- SlideTrack 45
- Slide Track 90 (Versa Track)
- Versa Track Slide Track 90 Deluxe
- Kayak Track Base
- Ground Stake Base

**Racks & T-Bars:**
- RMS8 Rod Rack (anchoring/trolling)
- Corner T-Bars
- Pontoon Corner T-Bars
- 1" Round Rail Corner T-Bar
- Pontoon Corner T-Base
- Replacement Corner T-Base
- Replacement Risers for Corner Ts

**HydroWeb Landing Nets:**
- HD80 Heavy Duty Landing Net
- HD150 Heavy Duty Landing Net
- HydroWeb Landing Nets (base model)
- Replacement Netting
- Replacement Net Handles
- Landing Net Caddys

**River Anchors:**
- River Anchor

**Bank Fishing:**
- Bank Master Pro

**Rod Transport:**
- Side Mount Transport Stations
- Flat Mount Transport Stations

**Boat Accessories:**
- Monster Bait Cutting Board
- Bait Cutting Board Mount
- Bait Cutting Board w/ Mount

**Draggin' Master (Bait & Tackle):**
- Weedless Circle Hooks
- Brush Guards
- 3oz In-Line Weighted Rig
- Black Armor Leader Line
- Rattle Float Pegs
- Bite Me Floats
- Drag-On Floats

**Mounting Hardware:**
- Universal Slide Track T-Bolts
- SS Pan Head Machine Screws (1/4-20 x 1-1/4, 1-1/2, 2")
- Hex Nuts for Rod Holders

**Apparel & Other:**
- "My Monsters Are Reel" T-Shirt
- Monster Head Gear / Hats
- Original Monster Logo (decal)
- Gift Card

#### Key Pages
- `/about-us/` — Company story
- `/contact/` — Contact info
- `/warranty/` — Warranty policy
- `/shipping/` — Shipping & returns
- `/pricing-and-inventory/` — Price/stock info
- `/terms-and-conditions/` — Terms
- `/privacy-policy/` — Privacy
- `/certified_resellers/` — Reseller directory
- `/sponsorships/` + `/sponsorship-application/` — Sponsorship program
- `/pro-staff/` — Pro staff page
- `/affiliate-area/` — Affiliate program
- `/gear-guidance/` — Blog/guides landing
- `/bargain-bin-specials/` — Sale items
- `/the-catfish-dude-videos/` — Video content
- `/get-qualified/` — Dealer qualification

#### Blog Posts (20)
Legitimate content (9 posts, 2018–2024):
- New metallic look announcement (2018)
- Catfish Cook Off (2019)
- Pro Tandem PAC (2019)
- Why Anglers Choose Monster Rod Holders (2024)
- Mounting Base Options guide (2024)
- HydroWeb Trophy Catfish Landing Nets (2024)
- Predicting Winter Blue Catfish Movements (2024)
- Rod Holder Models and Uses (2024)
- HD-80 Key Features & Benefits (2024)

**Spam posts (11, all March 2026):** German and Russian gambling/betting spam posts — the site appears to have been compromised by SEO spammers injecting blog posts about sports betting, casino games (Plinko), and gambling apps. This is a security concern.

#### Product Categories (17)
rod-holders | rod-holder-bases | racks-and-t-bars | rod-transport-solutions | mounting-hardware | boat-accessories | hydroweb-landing-nets | anchors | bank-products | kayak-products | apparel-decals | gift-cards | rigging-deals | bait-tackle | bait-tackle/draggin-master | monster-gear | uncategorized

#### Notable Findings
1. **SEO spam compromise** — 11 recent spam blog posts in German/Russian suggest WordPress exploitation
2. **Duplicate pages** — shop-2, cart-2, checkout-2, my-account-2, wishlist-2 (likely theme migration artifacts)
3. **WooCommerce + Zoho integration** — `/woocommerce-to-zoho-inventory-plugin/` page exists
4. **GeoDirectory pages** — gd-archive, gd-details suggest a directory plugin is installed
5. **Niche but complete product line** — 57 products covering rod holders, bases, nets, anchors, tackle, transport, and accessories
<!-- agent:boundary:53a7803f -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- Awaiting user direction on what to research deeper (pricing, competitors, specific products, security assessment, etc.)
<!-- /agent:pending -->
