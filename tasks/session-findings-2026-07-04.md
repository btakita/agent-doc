# Session Findings — 2026-07-04 (living document)

Session scope: tooling-alpha assessment → cross-harness memory migration → realtime-merge experience as marketable alpha.

## 1. Tooling alpha assessment

- **Passive layer is yield-positive and needs nothing**: RTK measured ~4.85B tokens saved (94.2%) across 114,695 commands; tsift + memory index compound silently.
- **Past 7 days**: 50 superproject commits — ~38 agent-doc extraction/refactor, ~7 equityfundingsource sessions (several of those were recovery commits), 2 tmux-router, 1 lazily bump.
- **agent-doc**: ~50+ documented recovery procedures accumulated in operator memory = high maintenance cost as a *tool*, but reframed by operator as deliberate architecture paydown after the turn-based → realtime migration wall. agent-doc is a **long-term product play, marketed later**.
- **Sequencing**: **Signals Patchboard markets first** — full 9-phase plan exists (#spb1–#spb9, `tasks/software/plan-signals-patchboard-full.md`), no phase started as of today. Recommendation on record: cap agent-doc at a dogfood-stability bar (per-cell CRDT #qcellmerge1 + realtime IPC reliability), put #spb1 in the primary slot.

## 2. Cross-harness memory migration (DONE, commit d8b3f77)

- claude-mem SQLite fully imported into `.tsift/memory.db`: **82,461/82,461** supported rows (59,129 observations, 9,504 session summaries, 13,828 user prompts; `pending_messages` intentionally excluded). 31,836 newly imported this session.
- Curated operator memories (145 files) moved from `~/.claude/projects/-home-brian-work-btakita-agent-loop/memory/` → **`memory/`** in-repo; Claude Code path is now a symlink to it. Single source of truth; Codex/OpenCode read the same files.
- `tsift index .` re-run; memory files verified retrievable via `tsift --envelope search` (top hit resolves to `memory/project_agent_doc_percell_crdt_merge_intent.md`).
- `tsift memory project-graph` materialized 5,186 nodes / 5,000 edges into `.tsift/graph.db`; `graph-db related` retrieval reports `ready`.
- AGENTS.md Dynamic Context Router gained an **Operator memory (all harnesses)** entry.
- claude-mem retirement gate: still `hold` — parity eval (`tsift dci-benchmark --fixture packages/tsift-cli/fixtures/memory-retrieval-eval.json`) and one clean normal-session cycle remain before retiring direct claude-mem reads.

## 3. Realtime multi-replica merge experience as the alpha

**Claim assessed: yes — the durable alpha of the agent-doc investment is the expertise more than the code.**

What the experience actually is (the hard-won, non-one-shottable part):
- **Application-level merge semantics**, not library CRDTs: authority/provenance models (operator vs agent edits), per-cell/node merge granularity, intent preservation, recovery when invariants break.
- **Convergence with live editor buffers** (JetBrains plugin, IPC snapshot drift, stale-replica detection) — merge correctness against a replica you don't control.
- **Human + multiple AI agents as concurrent replicas** — near-zero incumbents; classic CRDT veterans have human-human collab scars, not agent-authority ones.
- **A failure-mode taxonomy**: the 50+ recovery memories are field notes almost nobody else can write.

**Moat assessment**: library CRDTs (Yjs/Automerge/Loro) are commoditized; the moat is application-level merge judgment + failure taxonomy, which is experiential and timing-dependent — genuinely hard for off-the-shelf agentic solutions to replicate because failures are emergent and need live systems to reproduce. Caveat: the moat erodes as sync engines abstract more; the durable core is judgment about which merge architecture fits which product.

## 4. Where the demand is

**Market signals (2026)**: FOSDEM 2026 got its first dedicated local-first/CRDT/sync devroom; SyncConf exists; Supabase acquired Triplit (2025); Linear (~$1.25B, ~120 people) treats sync-engine design as central to platform hiring; ElectricSQL now markets "agents on sync" (electric.ax) — the agent+sync convergence is arriving exactly where this experience sits.

**Industries/companies**:
- Collaborative SaaS / dev tools: Linear, Figma, Notion, Zed (CRDT editor; Zed ACP integration already planned), Miro, Canva, Atlassian.
- Sync-engine infra: ElectricSQL, PowerSync, Ditto, Liveblocks, Rocicorp (Zero), InstantDB, Jazz, Convex, Supabase.
- Offline-first field ops (Ditto's market): aviation, defense, healthcare, logistics, retail POS.
- AI agent platforms: multi-agent shared state, human-in-the-loop workspaces — newest and least-served segment.

**Product ideas ranked by fit**:
1. Agent-collaboration substrate — realtime shared document/state runtime for human + multi-agent teams with authority/provenance semantics (agent-doc generalized; already the roadmap).
2. CRDT/merge observability & forensics — divergence detection, replay, merge debugging. The recovery-memory corpus proves the pain; commercial tooling ~absent.
3. Application-level merge toolkit above Yjs/Automerge/Loro — authority models, permission-aware merge, cross-replica undo.

**Financial sustainability read**: employment demand is real and senior (staff/principal sync roles at the named companies); consulting is viable but requires visibility first (content/talks — cold-start is slow in this niche); product is the longest path and already sequenced (Patchboard → agent-doc). Pragmatic order: publish the failure-mode taxonomy now (markets both operator and future products), keep Patchboard moving, treat employment/contract as the revenue backstop.

**Marketing channels**: sanitized failure-taxonomy writing series ("field notes from human+agent realtime merges"), localfirst.fm, FOSDEM local-first devroom, SyncConf, lofi.so community, open-source lazily/agent-doc merge layer as proof artifacts.

## Open threads

- [ ] Run `tsift dci-benchmark` memory-retrieval parity eval + one clean session cycle → then retire direct claude-mem reads.
- [ ] Decide first marketing artifact (taxonomy blog series vs localfirst.fm pitch vs FOSDEM talk proposal).
- [ ] #spb1 start decision (Patchboard Phase 1) vs agent-doc stability bar work.
