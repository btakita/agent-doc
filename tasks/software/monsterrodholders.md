---
agent_doc_session: a9421282-fd96-4943-9af5-3561ed5cb799
agent_doc_format: template
agent_doc_write: crdt
agent_doc_model_tier: low
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Resolved shop page issues: (1) image heights via mu-plugin CSS + force-recreate (2) pagination removal with loop_shop_per_page=-1 (3) footer logo size matching (4) New Cloth Technologies aside hiding. All deployed to production, verified. Key insight: must-use plugins auto-load; regular plugins need admin activation.
<!-- agent:boundary:7a5c98a0 -->
<!-- /agent:exchange -->
## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] **Match footer logo size to header (~75px)** — CSS cap + srcset sizes fix in mrh-performance v1.6, deployed and verified
- [x] **Hide "New Cloth Technologies" aside column on /shop/** — mrh-performance v1.7, deployed and verified
<!-- /agent:pending -->
