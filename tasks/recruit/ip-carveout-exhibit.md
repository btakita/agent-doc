# IP Carve-Out Exhibit — DRAFT (for either offer's agreement)

Status: draft for Brian's review. Attach the relevant section as "Exhibit A" (or
equivalent) to whichever agreement is signed. Not legal advice... have an employment
attorney do a one-hour review of the final agreement language, especially for Haiven
(Virginia has no statutory invention-assignment protection, so the contract text is the
entire protection).

## Part 1 — Pre-existing works (Exhibit A list)

All works below were created prior to the commencement of employment/engagement and are
excluded from any assignment of inventions, together with all future modifications,
extensions, derivatives, ports, bindings, documentation, and successor projects thereof:

- **lazily** — cross-language reactive-signals + CRDT library family (Rust, JS/TS,
  Python, Kotlin, Go, Zig, C++, Dart), including lazily-spec and lazily-formal
- **agent-doc** — CRDT multi-replica document/session synchronization system and
  associated editor plugins
- **tsift** — AST-aware code search, call-graph, and code-RAG CLI (all constituent
  crates)
- **patchboard / patchboard-app** — realtime patching/composition application and
  related infrastructure
- **tmux-router** — terminal multiplexer routing infrastructure
- **corky** — correspondence toolkit
- **tagpath** — tag-based identifier tooling
- **chromium-bridge** — browser automation CLI
- **freehold**, **boost-client**, **ctx-core / rappstack** ecosystem packages
- **briantakita.me** and related personal sites/content
- All other public repositories under Brian Takita's personal accounts (github.com
  /btakita and associated orgs) as of the start date

Note: the WebSocket-hub take-home design produced during Haiven's interview process was
authored pre-offer and uncompensated; it is Brian's work product and included in the
pre-existing list unless separately agreed.

## Part 2 — Future independent side-work clause (the actual ask)

Pre-existing lists only protect what already exists. The clause below protects future
side projects. Two tiers:

**Tier 1 (preferred ask):**

> Company acknowledges and agrees that Employee/Contractor may develop software,
> writings, and other works on Employee's own time, without use of Company equipment,
> facilities, or Confidential Information ("Independent Works"). Company claims no
> ownership of or interest in Independent Works, including Independent Works that relate
> to Company's current or anticipated business, provided they do not incorporate Company
> Confidential Information or code. Employee will not be required to seek approval for
> Independent Works, including their public release under open-source licenses.

**Tier 2 (fallback, if Tier 1 is refused on "related to our business" grounds):**

> Company claims no ownership of Independent Works developed on Employee's own time
> without Company resources or Confidential Information that are outside Company's
> Excluded Field. "Excluded Field" means [narrow, specific description of the company's
> actual product — insist on specificity, e.g. "AI chat moderation and player-safety
> systems for multiplayer games"], and expressly does not include general-purpose
> developer tooling, reactive-state libraries, CRDT/synchronization libraries, editor
> tooling, or works listed in Exhibit A and their successors.

Red-line list (strike these if present in their draft):
- "All inventions conceived or reduced to practice during the term of employment" with
  no own-time/own-resources exclusion
- Moonlighting clauses requiring prior written approval for outside work (or amend to
  "approval not to be unreasonably withheld, conditioned, or delayed")
- Non-competes broad enough to cover open-source realtime/reactive infrastructure
- Assignment of "improvements to Company products" defined loosely enough to capture
  Exhibit A successors

## Part 3 — Per-company application

**Acadian (via PGC employer-of-record):** structurally the easy case. The engagement is
a 12-month contract; market-standard contractor/EOR terms assign only work product
delivered under the engagement, and Acadian's business (financial services) has zero
domain overlap with the Exhibit A projects. Action: read the PGC agreement's assignment
clause, confirm it is deliverables-scoped, attach Part 1 as a precaution, and strike any
"all inventions during the term" language. Tier 1 should be uncontroversial here.

**Haiven:** the hard-but-important case. A Founder Engineer PIIA will default broad, and
the domain overlap is real — lazily (reactive/CRDT), agent-doc (realtime multi-replica
sync), and patchboard are adjacent to Haiven's realtime chat/presence/SDK business, so a
default "related to the Company's business" assignment clause could arguably reach them
and their successors. Open with Tier 1; if refused, Tier 2 with a tightly-scoped
Excluded Field. Getting Exhibit A signed is necessary but NOT sufficient — without a
future-works clause, next year's lazily feature or patchboard iteration is exposed.
Virginia provides no statutory floor (no CA §2870 equivalent), so whatever the contract
says, governs.
