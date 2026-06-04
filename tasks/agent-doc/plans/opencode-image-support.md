---
agent_doc_session: plan
agent: opencode
agent_doc_format: template
agent_doc_write: crdt
---

# Plan: opencode-image-support (#takv)

## Problem

`agent-doc describe-image` only supports `OpenAI` and `Anthropic` providers. When running under `opencode`, the user must manually configure vision credentials in `[vision]` config or env vars, and there's no way to leverage the provider already configured inside opencode.

## Investigation

- `describe_image.rs` makes direct HTTP calls via `ureq` — no agent/harness awareness
- `opencode run` CLI accepts text only (`[message..]`), no `--image` flag
- The `Agent` trait (`agent/mod.rs:350-358`) accepts only `prompt: &str` — no image payload
- Route dispatch communicates via tmux text injection — images cannot flow through this channel
- OpenCode's underlying provider is opaque to agent-doc (configured in opencode's own config)

## Design Options

### Option A: Config passthrough (Recommended)

Keep `describe_image.rs` provider-agnostic. Add a `Passthrough` provider that reads opencode's provider config (typically `~/.config/opencode/config.json` or env) and delegates to the matching HTTP provider (OpenAI/Anthropic/Google).

**Steps:**
1. Add `Provider::Passthrough` variant to `describe_image.rs`
2. Add `fn resolve_passthrough_provider()` that reads opencode config
3. When provider is `Passthrough`, detect the underlying provider and credentials
4. Fall through to the existing `call_openai` / `call_anthropic` / new `call_google`

**Pros:** Minimal change, reuses existing HTTP infrastructure, no opencode CLI dependency
**Cons:** Fragile to opencode config format changes

### Option B: CLI delegation

Add a provider that shells out to `opencode run "describe this image"` with the image somehow attached. Requires opencode to support image input in its CLI, which it currently does not.

**Pros:** Uses opencode's own provider resolution
**Cons:** Blocked on opencode CLI image support, slow (process spawn per image)

### Option C: Google provider + docs

Add `Provider::Google` (Gemini has vision) and document that opencode users should configure their vision provider explicitly. No auto-detection.

**Pros:** Simple, no opencode coupling
**Cons:** Users must configure manually, doesn't auto-detect provider

## Recommended: Option A + C

1. Add `Provider::Google` for Gemini vision support (Option C, universally useful)
2. Add `Provider::Passthrough` that reads opencode config for auto-detection (Option A)
3. Default for `agent: opencode` sessions: try passthrough first, fall back to explicit config

## Implementation Plan

### Phase 1: Google provider (~30 min)
- [ ] Add `Provider::Google` variant with `gemini-2.0-flash` default model
- [ ] Implement `call_google()` using Gemini API format
- [ ] Add `GOOGLE_API_KEY` env var resolution
- [ ] Add tests for new provider parsing and model defaults
- [ ] Update `resolve_api_key` and `resolve_endpoint` for Google

### Phase 2: Passthrough provider (~1 hr)
- [ ] Add `Provider::Passthrough` variant
- [ ] Implement `fn resolve_passthrough_provider()` reading opencode config
- [ ] Map opencode provider names to agent-doc Provider variants
- [ ] Add fallback chain: passthrough detect → config → env → default
- [ ] Add tests with mock opencode config

### Phase 3: Auto-default for opencode sessions (~30 min)
- [ ] In harness detection, when agent is opencode, default vision provider to Passthrough
- [ ] Add `[vision] default_provider = "passthrough"` config option
- [ ] Document in README/SPEC

## Files to Change

| File | Change |
|------|--------|
| `src/describe_image.rs` | Add Google + Passthrough providers |
| `agent-doc-orchestration/src/harness.rs` | Vision provider default per harness |
| `agent-doc-core/src/project_config.rs` | VisionConfig extensions |
| `SPEC.md` / `README.md` | Document new providers |

### Phase 4: Naked image path/URL detection (~30 min)
- [ ] Add `ImageRef` struct with kind (Markdown, NakedPath, Url) and path fields
- [ ] Implement `extract_image_references(text) -> Vec<ImageRef>` in `describe_image.rs`
- [ ] Detect: `![alt](path)`, naked file paths (`img.png`), image URLs (`https://...png`)
- [ ] Add `describe_image` subcommand `extract-images` to invoke extraction from CLI
- [ ] Add tests for all three reference kinds + edge cases

### Phase 5: Per-agent vision extraction config (~45 min)
- [ ] Add `AgentVisionConfig` to `project_config.rs`: `mode` (passthrough/describe) + optional provider/model/api_key overrides
- [ ] Add `agent_overrides: BTreeMap<String, AgentVisionConfig>` to `VisionConfig`
- [ ] Wire `VisionConfig` + `AgentVisionConfig` into `describe_image.rs` resolution chain
- [ ] Default: Claude Code + Codex = passthrough, OpenCode/GLM = describe via GPT
- [ ] Config example:
  ```toml
  [vision]
  provider = "openai"
  [vision.agents.claude]
  mode = "passthrough"
  [vision.agents.codex]
  mode = "passthrough"
  [vision.agents.opencode]
  mode = "describe"
  provider = "openai"
  model = "gpt-4o"
  ```
- [ ] Add tests for config resolution + per-agent overrides

## Files to Change

| File | Change |
|------|--------|
| `src/describe_image.rs` | Add Google + Passthrough providers, image extraction, per-agent config wiring |
| `agent-doc-orchestration/src/harness.rs` | Vision provider default per harness |
| `agent-doc-core/src/project_config.rs` | VisionConfig + AgentVisionConfig extensions |
| `SPEC.md` / `README.md` | Document new providers, extraction, per-agent config |

## Open Questions

- Does opencode store provider config in a stable location/format?
- Should Passthrough also support OpenRouter or other providers?
- Should `describe-image` be integrated into the route dispatch path (pre-process images before agent dispatch)?
- Should naked path detection resolve relative to the session document directory?
