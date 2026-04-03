---
agent_doc_session: bacfe7f2-6288-479b-8d56-d8b2cb9a27ca
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
Full dealer list extracted via chromium-bridge. 1,458 locations (1,205 Walmart + 50 Fleet Farm + 20 Scheels + 19 Academy Sports + 164 independents). CSV saved.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Assistant
Whisker Seeker research session initialized. What would you like to research? For example:

- Website and online presence
- Product line and pricing
- Location, manufacturing, and company details
- Social media / YouTube presence
- Customer reviews and reputation
- Distributor / dealer network
- Competitor comparison

Provide URLs or direction and I'll start crawling.
https://whiskerseeker.com

Crawl all of the dealers in https://whiskerseeker.com/dealers
Put into a csv file.
Use chromium-bridge if that helps.

### Re: Site crawl & dealers

Crawled whiskerseeker.com and searched extensively. The dealers page uses a **Storemapper** widget that loads dynamically via JavaScript — can't be scraped via static fetch, and Chrome DevTools timed out. I extracted all publicly discoverable dealers/retailers from web search instead.

**CSV saved to:** `tasks/research/whiskerseeker-dealers.csv`

---

#### Company Overview

| Field | Detail |
|-------|--------|
| **Name** | Whisker Seeker Tackle (WST) |
| **Founded** | 2015 by Matthew Davis |
| **Location** | Grimes, Iowa |
| **Phone** | 1-855-WSEEKER (1-855-973-5537) |
| **Focus** | Premium catfish tackle — rods, reels, rigs, lures, hooks, nets, line |
| **Shipping** | Same-day on orders before 2pm CST; free over $75 |
| **Acquired** | **October 4, 2022 by PRADCO Fishing** (subsidiary of EBSCO Industries) |

#### PRADCO Acquisition (Oct 2022)
- PRADCO Outdoor Brands acquired Whisker Seeker Tackle — their first acquisition since 2019
- **Matthew Davis** continues as VP & General Manager, operating from Iowa
- Direct-to-consumer model continues post-acquisition
- Bruce Stanton (PRADCO VP/GM): "We kept saying we wanted to be like Whisker Seeker Tackle"
- Strategic entry into the catfish market for PRADCO
- Sources: [Fishing Tackle Retailer](https://fishingtackleretailer.com/pradco-fishing-acquires-whisker-seeker-tackle/), [The Fishing Wire](https://thefishingwire.com/pradco-fishing-acquires-whisker-seeker-tackle/), [Mergr](https://mergr.com/transaction/pradco-outdoor-brands-acquires-whisker-seeker-tackle)

#### Product Lines
- **Rods:** FMJ, GFX, Hog Seekers, Bump'n Stick series + combos
- **Reels:** Baitcasters (new), baitrunners, spinning reels ("Seeker Reels")
- **Terminal Tackle:** Hooks (Triple Threat hybrid circle hooks), swivels, sinkers, clips
- **Lures & Rigs:** Spooks, jigs, spoons, rattlers, floats (LED slip floats)
- **Line:** Mono, braid, leader
- **Tools:** Knives, pliers, scales, grips
- **Accessories:** Tackle boxes (57-piece kit), PowerLock Rod Holders, XTN Landing Nets, Shad Buster Cast Nets
- **Bait:** Mr. Whiskers Bait, Solar Bait Shack (solar-powered)
- **Food:** Fish Fry Breading
- **Apparel:** Hats, shirts, decals

#### Known Dealers & Retailers (CSV)

| Dealer | Type | Notes |
|--------|------|-------|
| **whiskerseeker.com** | Direct / DTC | Primary store, same-day shipping |
| **Scheels All Sports** | Retail chain | All WST products — customer demand drove retail entry |
| **Walmart** | Online marketplace | walmart.com listings |
| **Amazon** | Online marketplace | Multiple products |
| **eBay** | Online marketplace | eBay store |
| **Vance Outdoors** | Retail (Ohio) | Rods, reels, hard baits, jigs |
| **Tackle Bandit** | Online retailer | Wix-based shop |
| **Nicklow's Wholesale Tackle** | Wholesale distributor | Carries Whisker Seeker Punch Baits |

**Note:** The full dealer list on whiskerseeker.com/dealers is behind a Storemapper JS widget that couldn't be scraped. There are likely many more local bait shops and tackle stores in the network. To get the complete list, the page would need to be loaded in a browser with JS execution (Chrome DevTools was unavailable this session).

#### Social Media
- **Facebook:** [Whisker Seeker Tackle](https://www.facebook.com/WhiskerSeeker/)
- **Instagram:** @whiskerseekertackle
- **Website social handles:** @whiskerseekertackle / @Whiskerseekertackle

#### Industry Presence
- [Catfish Angler Forum (USCA)](https://www.catfish1.com/threads/whisker-seeker-tackle.215036/) — community discussion
- [Manta.com listing](https://www.manta.com/c/mkbxhnp/whisker-seeker-tackle-llc) — Grimes, IA business record
- PRADCO Outdoor Brands parent: [pradcooutdoorbrands.com](https://www.pradcooutdoorbrands.com/)

Use chromium-bridge with brave to scrape the Storemapper JS widget.

### Re: Storemapper scrape via chromium-bridge

Used chromium-bridge to navigate to the dealers page, found the Storemapper widget's API endpoint (account ID 5207), and extracted the full store database directly.

**CSV updated:** `tasks/research/whiskerseeker-dealers.csv` — **1,458 dealer locations**

---

#### Dealer Network Summary

| Chain / Type | Locations | Notes |
|-------------|-----------|-------|
| **Walmart** | 1,205 | Massive national distribution |
| **Fleet Farm** | 50 | Midwest regional chain |
| **Scheels** | 20 | Sporting goods chain (Midwest/West) |
| **Academy Sports** | 19 | Southern US chain |
| **Independent shops** | 164 | Local bait shops, tackle stores, outdoors retailers |
| **Total** | **1,458** | |

#### Top Independent Dealers (sampling)
- 2nd Amendment Outdoors — Corinth, MS
- 367 Bait & Tackle — St. Louis, MO
- 64 Bait Shop & Tackle — Muldrow, OK
- AI Outdoors
- AJ's Mercantile
- Back-N-Sticks Outdoors
- Badwoods Outdoors
- Bait Bucket
- Base Camp Outfitters
- Beginner's Luck Tackle and Supply
- (and 154 more in the CSV)

#### Geographic Distribution (top states)
TX (26), WI (24), MN (18), IA (10), KY (7), NE (5), MO (5), TN (5), OH (5)
Plus 17 more states. Heavy Midwest/Southern concentration matching catfish fishing geography.

#### CSV Fields
`name, address, phone, email, url, latitude, longitude, city_custom, state_custom, categories`

#### Method
1. `chromium-bridge navigate` → whiskerseeker.com/dealers
2. `chromium-bridge evaluate` → found Storemapper widget loaded from `storemapper.co/js/widget.js`
3. Extracted API endpoint from performance entries: `storemapper-herokuapp-com.global.ssl.fastly.net/api/users/5207/stores.json`
4. `curl` direct API call → full JSON with all 1,458 stores
5. Parsed to CSV with all fields including lat/lng coordinates
<!-- agent:boundary:45ab8aa3 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] Full Storemapper dealer list — DONE (1,458 dealers)
- [ ] Product pricing deep-dive
- [ ] Competitor comparison (Monster Rod Holders, Team Catfish, etc.)
- [ ] Customer review aggregation
- [ ] PRADCO portfolio analysis (sister brands)
- Awaiting user direction
<!-- /agent:pending -->
