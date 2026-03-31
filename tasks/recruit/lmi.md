---
agent_doc_session: 69cd8603-762b-4a92-adc6-a44c3ed88380
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
LIGER deep dive complete — call at 1:30 PM ET today
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Read /home/brian/work/btakita/agent-loop/tasks/software/corky/lmi-email-source.txt

Help me prepare for the call today at 13:30 Eastern
### Re: LMI Interview Prep — Today 1:30 PM ET

**Meeting:** Microsoft Teams video conference, 30 min
**Team:** Luke Stets, Josh Malloy, Andrew Dirks, Adam Hughes
**Recruiter:** Kate Revels (joins at start to check A/V)

---

#### LIGER Platform

Enterprise AI orchestration platform for federal government:
- Agentic workflows — goal-oriented AI agents coordinating multi-step tasks with human-in-the-loop approval gates
- Deployed at classified levels (Secret, TS, SAP) — IL5/IL6 environments, CMMC Level 2, zero-trust
- Customers: USSOCOM, US Army (DASA-DES), intelligence agencies
- Modular, vendor-agnostic architecture (MOSA-aligned)
- Use cases: maintenance/sustainment, software dev automation, intelligence planning, acquisition, help desk

#### Interviewers

1. **Luke Stets** — Engineering role. Limited public info. Expect general software architecture / full-stack questions.

2. **Josh Malloy** — Data Science Principal. Carnegie Mellon (Heinz College). Published "Beyond the Vending Machine: R&D for Agentic AI." Built "Rivanna" (agentic deliverable generation from deal data). Claude Code user. Likely focus: agent design patterns, RAG pipelines, LLM evaluation, data science strategy.

3. **Andrew Dirks** — Full Stack Engineer. Former Boeing/ATA. Wichita State (EE). CompTIA Security+. Likely focus: hands-on full-stack development, secure coding, system integration in government contexts.

4. **Adam Hughes** — Software Developer, PhD biophysics (GWU). Created scikit-spectra, pyparty (scientific Python). Active Claude Code user. Likely focus: Python depth, data pipelines, scientific computing patterns, AI-assisted development workflow.

#### Your Strong Angles

- **agent-doc / agent-loop** — you build agentic workflows daily (tool-calling, multi-step reasoning, state management)
- **MCP experience** — job listing specifically lists MCP as a differentiator; you work with MCP servers in Claude Code
- **Claude Code power user** — 2 of 4 interviewers are Claude Code users; natural rapport
- **Full stack range** — Rust, TypeScript, Python across production projects
- **RAG / document processing** — Ragie integration maps directly to their pipeline needs
- **Builder disposition** — open source contributions, personal projects, side work (exactly what "sets you apart" per job listing)

#### Interview Rules (from email)

- Stay on camera for the full duration
- Do NOT use AI tools or interview guides during the call
- You CAN share examples of how you use AI to make your work more effective
Give more info on LIGER.
### Re: LIGER Deep Dive

**LIGER = LMI Instruction-tuned GEnerative Resource**

**Origin story:** Started when LMI was asked to solve a critical F-35 logistics challenge that grounded the fleet. Built a domain-specific LLM for logistics, evolved into the broader platform. Tested internally for **14 months** before going to market.

**Architecture:**
- **Centralized Model Access Gateway** — model-agnostic (commercial cloud + on-prem LLMs), enterprise cost controls
- **Intent-to-Execution Pipeline** — breaks mission objectives into coordinated actions across distributed systems
- **RAG with citations** — generates content with source citations for fact-checking; hallucination detection built in
- **Real-Time Adaptation** — dynamic replanning based on operational data
- **Human-in-the-Loop Gates** — auditable approval checkpoints for sensitive decisions
- Operates in **air-gapped environments**
- 80%+ time reduction (2-3 days → 2-3 hours per query)

**$47.6M Army Contract (Oct 2024):**
- CalibrateAI pilot program — enterprise license at IL5
- Streamlines acquisition tasks: market research, contract writing
- Specifically focuses on hallucination detection and citation-backed outputs

**Other customers:** USSOCOM (air-gapped CRADA deployment), intelligence community (policy analysis), health/civilian agencies (help desk + Jira integration)

**Sibling platforms (LMI Forge):**
- RAPTR — rapid modeling/simulation (5-10x faster)
- IronSled — government-authorized deployment infrastructure
- SPECTR — real-time asset visibility (Army/Navy)
- SHEPRD — automated analysis and protection

**Key people:**
- Josh Wilson — CEO (Jan 2026), drove Forge strategy
- Alex Adamczyk — VP Advanced Analytics & AI, led LIGER development, $100M+ AI work in 2024
- Matthew Finney — VP Generative AI (from McKinsey/QuantumBlack + Amazon Robotics)

**Josh Malloy's "Beyond the Vending Machine" framework:**
- Traditional software = "vending machine" (predictable, limited); agentic AI = fundamentally different (adaptive, non-deterministic)
- Three tensions to balance: **Capability** vs. **Uncertainty/Variability** vs. **Transparency**
- Three R&D phases: Theoretical possibility → Technical feasibility → Operational viability
- Key insight: this is a **sociotechnical change** — redesigned APIs for machine users, new access control models, process clarity on agent autonomy vs. human escalation
- Non-determinism is necessary — constraining it limits utility
- Agents must be instrumentable for debugging and parameter adjustment

**How your work connects:**
- agent-doc/agent-loop = exactly the agentic workflow pattern LIGER uses
- You manage the capability/variability/transparency tradeoff Malloy describes every day
- Your MCP daily usage maps to their tool-calling infrastructure
- RAG experience with Ragie maps to LIGER's citation-backed RAG
- You understand that federal security (IL5/IL6, air-gapped, zero-trust) shapes architecture from day one, not bolted on
<!-- agent:boundary:5dbb6d94 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Join Teams call at 1:30 PM ET
- [ ] Review Malloy's "Beyond the Vending Machine" framing for conversation hooks
- [ ] Prepare agent-doc demo talking points
<!-- /agent:pending -->
