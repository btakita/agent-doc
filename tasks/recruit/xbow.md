---
agent_doc_session: b3206de4-6170-4b8f-8f2a-246f8a45bc5c
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Please help me prepare for an interview with xbow.com
Please help me prepare for an interview with xbow.com

### Re: Interview Prep — XBOW (HEAD)

**XBOW** builds an autonomous offensive security platform — an AI that finds, exploits, and validates real vulnerabilities in web applications at machine scale, without human intervention. Founded January 2024 by Oege de Moor (founder of Semmle → GitHub, built GitHub Copilot and Advanced Security) and engineers from the original Copilot team.

**Just raised $120M Series C (March 18, 2026)** at unicorn valuation. Total ~$237M raised. Investors: DFJ Growth, Northzone, Sequoia, Alkeon, Altimeter.

---

#### Company Overview

| Fact | Detail |
|------|--------|
| **Founded** | January 2024 |
| **Headquarters** | San Francisco |
| **Stage** | Series C, ~$1B+ valuation |
| **Headcount** | ~100+ (growing fast, ~35 open roles) |
| **Key milestone** | #1 on HackerOne global leaderboard (June 2025) — first AI to outrank all human hackers |

**Core claim:** "Proof, not probability." Every finding is only surfaced after a deterministic validator confirms the exploit is reproducible. No probabilistic alerts — real exploits only.

---

#### Technology Stack

| Layer | Technologies |
|-------|-------------|
| **Backend** | TypeScript, Kotlin, Go |
| **Cloud** | AWS |
| **AI architecture** | Multi-agent swarm (thousands of short-lived LLM agents) |
| **Key components** | Coordinator (orchestration), Attack Machine (shared exec env + headless browser), Validators (deterministic verification) |

**Architecture insight:** They separate AI exploration (creative, uncertain) from verification (deterministic). This is the core innovation that eliminates false positives.

---

#### Key Technical Domains (likely interview topics)

1. **Multi-agent orchestration** — coordinating thousands of short-lived, parallel LLM agents without interference; managing context degradation by keeping agents short-lived and narrowly scoped
2. **LLM tool design** — building tools optimized for LLM reasoning (e.g., LLM-steerable headless browser for web app attacks)
3. **Benchmark-driven ML engineering** — using CTF challenges (PortSwigger, PentesterLab, public CTFs) to measure every system/model change; custom scoring to reduce hallucination
4. **Offensive security fundamentals** — web app vulnerabilities (SQLi, XSS, SSRF, IDOR, auth bypass, etc.); OWASP Top 10 depth expected
5. **Production safety** — validating real exploits without modifying live data; non-destructive exploit verification
6. **Scale & reliability** — running continuous, parallel attack campaigns at machine speed; SRE/incident response at this scale

---

#### What Makes XBOW Unique (talk points)

- **Speed:** Completed in 28 min what took a 20-year veteran pentester 40 hours (104-challenge benchmark)
- **Supply crisis angle:** Critical shortage of human pentesters; XBOW operates 24/7 without headcount constraints
- **Third-party validation:** HackerOne #1 ranking is credible proof — not a lab benchmark, real production targets
- **Microsoft integration:** Partnership with Microsoft Security Ecosystem signals enterprise distribution
- **Founder credibility:** Oege de Moor built GitHub Copilot — unique track record applying LLMs to developer tooling at scale

---

#### Questions to Ask Them

1. How do you evaluate LLM performance improvements vs. system architecture improvements? What does your benchmark feedback loop look like?
2. How are agents isolated to prevent one agent's actions from contaminating another's attack surface?
3. How do you handle the "context degradation" problem at scale — is short-lived agent lifecycle the whole answer, or are there other mechanisms?
4. What's the hardest class of web vulnerability to automate, and why?
5. How does the coordinator decide when to escalate vs. abandon an attack path?
6. What does your on-call/incident response look like when the AI itself is the "attacker" and something goes wrong?
7. How are you thinking about the agentic reasoning improvements in newer frontier models — do model upgrades translate directly to capability gains?

---

#### Role-Specific Prep

*What role are you interviewing for?* Tailor further based on:

- **AI Systems / Platform** → focus on multi-agent coordination, LLM tool design, benchmark-driven eval, TypeScript/Go/Kotlin
- **Security Researcher / Offensive Security** → deep web vuln knowledge, CTF experience, exploit writing
- **SRE / Infrastructure** → AWS, reliability at scale, production safety constraints
- **Product** → attack surface coverage metrics, customer journey from POC to continuous monitoring

Add your target role here and I'll drill down.
<!-- agent:boundary:bfc1877a -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
<!-- /agent:pending -->
