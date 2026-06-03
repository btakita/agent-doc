# Plan: #img-describe-toolcall — agent-doc describe-image subcommand

## Summary

Add `agent-doc describe-image <IMAGE_PATH>` subcommand that sends an image to a vision API and returns a text description to stdout. Enables non-vision models (like GLM-5.1) to analyze images via a bash toolcall.

## Design

### CLI interface

```
agent-doc describe-image <IMAGE_PATH> [--provider <provider>] [--model <model>] [--api-key <key>]
```

- `IMAGE_PATH` — path to image file (png, jpg, gif, webp)
- `--provider` — vision provider: `openai` (default), `anthropic`
- `--model` — model override (default: provider-specific, e.g. `gpt-4o`)
- `--api-key` — API key override (default: from env var)

### Provider resolution

1. `--provider` CLI flag (highest precedence)
2. `AGENT_DOC_VISION_PROVIDER` env var
3. Default: `openai`

### API key resolution

1. `--api-key` CLI flag
2. `AGENT_DOC_VISION_API_KEY` env var
3. Provider-specific: `OPENAI_API_KEY` or `ANTHROPIC_API_KEY`
4. Error if no key found

### Model resolution

1. `--model` CLI flag
2. `AGENT_DOC_VISION_MODEL` env var
3. Provider default: `gpt-4o` for openai, `claude-sonnet-4-20250514` for anthropic

### HTTP transport

Uses existing `ureq` v2 (already in Cargo.toml). Base64 encoding requires adding `base64` crate.

#### OpenAI-compatible provider

POST to configurable endpoint (default: `https://api.openai.com/v1/chat/completions`):

```json
{
  "model": "<model>",
  "messages": [
    {
      "role": "user",
      "content": [
        {"type": "text", "text": "Describe this image in detail."},
        {"type": "image_url", "image_url": {"url": "data:<mime>;base64,<data>"}}
      ]
    }
  ],
  "max_tokens": 1024
}
```

#### Anthropic provider

POST to `https://api.anthropic.com/v1/messages`:

```json
{
  "model": "<model>",
  "max_tokens": 1024,
  "messages": [
    {
      "role": "user",
      "content": [
        {"type": "image", "source": {"type": "base64", "media_type": "<mime>", "data": "<data>"}},
        {"type": "text", "text": "Describe this image in detail."}
      ]
    }
  ]
}
```

### Output

Plain text description to stdout. Errors to stderr with nonzero exit code.

### Files to modify

1. **`src/agent-doc/Cargo.toml`** — add `base64` dependency
2. **`src/agent-doc/src/describe_image.rs`** — new module with `run()` function
3. **`src/agent-doc/src/main.rs`** — add `mod describe_image;`, `Commands::DescribeImage` variant, dispatch match arm

### Testing

- Unit tests: MIME detection from extension, provider config resolution
- Integration: `assert_cmd` test for missing file, missing API key
- Live test: manual with `AGENT_DOC_VISION_API_KEY` set
