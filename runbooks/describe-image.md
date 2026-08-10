# describe-image (non-vision models)

Non-vision agent models (e.g. GLM, Llama, Mistral) cannot read images referenced via `![alt](path.png)` in the session document. When a queue/exchange item references an image whose contents drive the diagnosis, **delegate the image analysis to a vision-capable model via `agent-doc describe-image`** instead of skipping the image or guessing its contents:

```bash
agent-doc describe-image <IMAGE_PATH> [--provider openai|anthropic] [--model <name>] [--prompt "<structured question>"]
```

The subcommand shells out to a configured vision provider and prints a text description to stdout, which the non-vision agent can then reason over like any other text. Default providers: OpenAI `gpt-4o`, Anthropic `claude-sonnet-4-20250514`. API key resolution: `--api-key` flag → `AGENT_DOC_VISION_API_KEY` env var → provider-specific env var (`OPENAI_API_KEY` / `ANTHROPIC_API_KEY`).

**When to use it:** any time the operator attaches a screenshot, diagram, or image as primary evidence for a bug report (e.g. `JB \`Clear Session Context\` … !img_46.png`). Calling `agent-doc describe-image` is the canonical path — do not reinvent per-session `claude -p @img` invocations. Source: `src/describe_image.rs`.
