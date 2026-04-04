---
session: 655fad0d-fc98-412e-b951-2956603ad69e
resume: null
agent: null
model: null
branch: null
---
Please summarize key points in mail/topics/interactive-brokers/Zero_Hash_and_Zero_Hash_Liquidity_Services_User_Agreement.pdf

Anything of concern?

## Assistant

**Zero Hash US User Agreement — Summary & Concerns**

Agreement between you and Zero Hash LLC (Delaware) — the back-end crypto technology provider behind Interactive Brokers' crypto offerings. Last updated Dec 11, 2025.

**Entities involved:**
- zerohash LLC (Chicago, IL) — technology/settlement
- ZHLS (zerohash liquidity services) — acts as agent for trades
- ZHT (zerohash trust company, NC) — custody/fiduciary for US customers
- Connect Ltd. (Bermuda) — identity verification/wallet linking

**21 Services covered:** Buy/Sell, Liquidity, Settlement, Custody, Tax Reporting, P2P, Rewards, Deposits, Withdrawals, ACH, RTP, Remittance, Staking, Swap, Account Funding, Tokenization, Pay Services, Payout Services, Auth (wallet linking), OneID, Specially Designated Accounts.

**Items of Concern:**

1. **Mandatory arbitration (S16)** — No class actions, no jury trial, no court. All disputes go to binding arbitration via New Era ADR (or JAMS for batch). Illinois law governs. You waive all rights to participate in class actions.

2. **Unilateral modification (S2)** — They can change the agreement at any time at sole discretion. Continued use = acceptance. Only NY residents get prior notice by law. They're not even obligated to notify you directly.

3. **No FDIC/SIPC protection on crypto (S5.2, S10.6)** — Digital assets are NOT insured by FDIC or SIPC. Fiat currency is held in FDIC-insured omnibus accounts, but the digital assets carry no deposit insurance. You bear all risk of loss.

4. **Biometric data collection (S4.2.1)** — They may collect retina/iris scans, fingerprints, voiceprints, hand scans, facial geometry. Retained "as needed and at our discretion."

5. **Staking risks (S1.13)** — Lock-up periods up to 10 days for unstaking. Slashing penalties can cause loss of staked assets. No guarantee of staking rewards — estimated, variable, not guaranteed.

6. **24-hour error reporting window (S9.10)** — You must report transaction errors within 24 hours or waive your right to dispute them.

7. **Account suspension at sole discretion (S7.2)** — They can freeze/terminate your account and all assets without prior notice. 2+ years of inactivity = escheatment (abandoned property laws).

8. **Omnibus wallets (S10.7)** — Your crypto may be held in shared blockchain addresses with other users. No obligation to create segregated addresses.

9. **Negative balance liability (S9.8)** — If your account goes negative, they can charge interest, offset from other accounts, and pursue collection.

10. **ZHLS spread markup (S1.2)** — ZHLS can add undisclosed spreads to any trade at sole discretion. Execution price "is not meant to imply the market price."

11. **Force majeure broad (S9.13)** — Covers acts of God through pandemics. No liability for delays/failures.

12. **Transaction data ownership (S11.5)** — They claim joint ownership of all transaction data and can sell/redistribute it (anonymized) without your consent.

13. **Location restrictions (S3.5)** — New York gets special mention: services may be limited/terminated if you're in NY. Moving to unsupported jurisdiction = they can cancel open orders and unstake your assets.

14. **Assignment asymmetry (S18)** — They can assign the agreement to anyone at any time. You cannot.

**What's standard/reasonable:**
- Self-directed account (you make your own decisions)
- KYC/AML verification requirements
- Tax reporting (1099s, etc.)
- Anti-fraud and prohibited activity rules
- Hot/cold wallet security with multi-party computation
- Title to assets remains with you (S10.4) — assets not property of zerohash or its creditors

**Bottom line:** Leans heavily in zerohash's favor, which is typical for crypto infrastructure. Biggest practical concerns: (1) mandatory arbitration eliminates legal leverage, (2) the 24-hour error window is very tight, (3) no insurance on crypto holdings, and (4) broad unilateral modification rights.

## User

Summarize https://ndcdyn.interactivebrokers.com/registration-api/api/agreement/download-agreement?renderedFormId=40796412

Summarize https://ndcdyn.interactivebrokers.com/registration-api/api/agreement/download-agreement?renderedFormId=40796411

## Assistant

Both URLs require an authenticated IB session — they return empty without your login cookies. These are personalized agreement downloads tied to your account registration.

**To proceed:**
1. Download the PDFs from your browser while logged into IB
2. Save them to `mail/topics/interactive-brokers/` (e.g., `ib_40796412.pdf` and `ib_40796411.pdf`)
3. Submit again and I'll summarize them

If you can tell me the agreement titles (visible on the IB registration/agreement page), I can also look up the standard public versions.

## User

Downloaded.

## Assistant

### ib_40796412.pdf — Risk Disclosure for Trading Digital Assets with Zero Hash Through IBKR's Platform
*(Form 4436, Oct 18 2024, 5 pages — risk disclosure, not a binding contract)*

14 key risk points:

1. **Trading is risky** — you may sustain total loss
2. **No investment/tax advice** — execution only
3. **No FDIC/SIPC** — crypto not insured; Zero Hash is not a broker-dealer
4. **Limited trading hours** — IBKR not 24/7; Zero Hash Web App available as backup at ibkr.zerohashforclients.com
5. **Orders must be fully funded** — no margin for crypto
6. **Highly speculative/volatile** — prices influenced by social media, regulation, sentiment
7. **No margin value** — crypto has zero margin value at IBKR; if IBKR account has insufficient equity, IBKR assets (not crypto) may be liquidated
8. **Liquidity risk** — Zero Hash can stop quoting or cease trading at any time
9. **Commissions** — Zero Hash charges commissions
10. **Zero Hash Agreement governs** — your crypto relationship is with Zero Hash, not IBKR
11. **No short selling** — can't short digital assets
12. **Limited asset selection** — subject to change without notice
13. **Disruption risk** — systems can fail; IBKR services "as-is" with limited liability
14. **Transfer out available** — you can move assets to an independent Zero Hash account

**Notable:** NFA disclosure that NFA has NO regulatory authority over crypto.

---

### ib_40796411.pdf — Supplement to IB Client Agreement for Trading Digital Assets with Zero Hash
*(Form 4437, Oct 30 2025, 7 pages — binding legal supplement)*

Key terms:

1. **Crypto services provided solely by Zero Hash** — IB is just the platform. **IB is NOT responsible for any losses** (theft, fraud, cybersecurity breach, loss of private keys, or any loss from trading/holding digital assets).

2. **Two Payment Source Account options:**
   - **Option A: IB Brokerage Account** — automatic cash movement by IB, but limited to Platform Assets only (BTC, ETH, LTC, BCH); trading limited to IB platform hours
   - **Option B: Zero Hash Account** — manual cash movement by you, but access to ALL Zero Hash-supported crypto via Zero Hash Web App; transfers during banking hours only (8am-4pm NYC)

3. **Full funding required** — orders exceeding available cash may be rejected; if they slip through, you're fully liable. IB can set limits on your Zero Hash Account balance.

4. **No margin value, no lien** — Zero Hash assets have zero margin value. Positive: **IB has no lien or security interest** over your Zero Hash Account assets.

5. **Fraudulent transfers** — IB can liquidate any crypto purchased with funds from a revoked ACH/ACATS transfer.

6. **No Customer Protection Rule / No SIPA** — Zero Hash assets NOT protected under SEC Rule 15c3-3 or SIPC.

7. **Withdrawals irreversible** — once broadcast to blockchain, no reversals. IB has zero liability for incorrect transfers.

8. **Conditional orders** — may not execute during IB platform downtime. You waive claims for losses from downtime.

9. **Pre-dispute arbitration** — inherits mandatory arbitration from IBKR Client Agreement (Paragraph 54).

**Key takeaway:** Your IBKR brokerage assets (stocks, bonds) and crypto are in entirely separate legal regimes. IBKR's SIPC protection does NOT extend to anything in your Zero Hash Account. If Zero Hash fails, IBKR takes no responsibility.

## User
