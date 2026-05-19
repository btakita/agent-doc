# Example Composition Plan

## Decision Boundary

The workflow should split because research, writing, and publishing have different reuse patterns and safety policies.

## Proposed Skills

- name: research-video-ideas
  purpose: Find and rank reusable video topics.
  triggers: "research video ideas", "find content angles".
  inputs: Audience notes, seed topics, source URLs.
  outputs: Ranked topic list with rationale.
  resources: `references/` for audience notes and `scripts/` for source collection.
  invocation: implicit.
  handoff: `write-video-script`.

- name: write-video-script
  purpose: Draft a video script from a selected topic.
  triggers: "write a video script", "turn this topic into a script".
  inputs: Selected topic, voice examples, target duration.
  outputs: Script draft and revision notes.
  resources: `references/` for voice and structure examples.
  invocation: implicit.
  handoff: `draft-social-post`.

- name: publish-content
  purpose: Publish approved content to a platform.
  triggers: "publish this", "schedule this post".
  inputs: Approved content, platform, credentials.
  outputs: Published or scheduled content confirmation.
  resources: `scripts/` for platform API calls.
  invocation: user-only.
  handoff: None.

## Resource Inventory

Research needs reusable source collection scripts. Writing needs references with style examples. Publishing needs deterministic API scripts and should not share implicit invocation with lower-risk drafting.

## Invocation Policy

Research and writing can be implicit because they are reversible drafting tasks. Publishing must be user-only because it mutates an external system.

## Validation Plan

Test one prompt for research only, one prompt that chains research into script writing, and one unrelated prompt that should not activate this skill system. Test publishing separately with explicit user approval.

## Recommendation

Create three skills. Implement research and writing first with `skill-creator`; keep publishing stubbed until credentials and approval flow are defined.
