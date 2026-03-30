---
agent_doc_session: bacfe7f2-6288-479b-8d56-d8b2cb9a27ca
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
Site crawled. WST founded 2015 (Grimes, IA), acquired by PRADCO Oct 2022. 8 known dealers/retailers exported to CSV. Full dealer list requires JS-rendered Storemapper page.
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
<!-- agent:boundary:dec33625 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Full Storemapper dealer list (requires browser JS execution — retry when Chrome DevTools available)
- [ ] Product pricing deep-dive
- [ ] Competitor comparison (Monster Rod Holders, Team Catfish, etc.)
- [ ] Customer review aggregation
- [ ] PRADCO portfolio analysis (sister brands)
- Awaiting user direction
<!-- /agent:pending -->
