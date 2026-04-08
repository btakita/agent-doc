---
agent_doc_session: null
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
New feature: `corky youtube pipeline` — automate the full video publishing workflow.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Feature: `corky youtube pipeline`

Automate the full YouTube video publishing workflow from raw recording to published playlist with blog and social posts.

**Pipeline steps:**
1. Light edit (silence removal via ffmpeg silencedetect)
2. GPU transcription (Whisper CUDA) → SRT closed captions
3. YouTube draft creation with metadata
4. YouTube upload with captions
5. Playlist creation/management
6. Blog post generation (configurable, optional)
7. Social media posts — LinkedIn, etc. (configurable, optional)

**Configuration (`.corky.toml`):**
```toml
[youtube.pipeline]
silence_threshold = "-30dB"
silence_duration = 3
encode_preset = "fast"
encode_crf = 23
auto_blog = false          # optional blog post
auto_linkedin = false      # optional LinkedIn post

[youtube.pipeline.blog]
site = "briantakita.me"
content_dir = "src/content/posts"

[youtube.pipeline.linkedin]
author = "btakita"
```

**CLI:**
```sh
corky youtube pipeline VIDEO1 VIDEO2 ... \
  --title "Series Title" \
  --playlist "Playlist Name" \
  --blog \
  --linkedin
```
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Design CLI interface and config schema
- [ ] Implement silence detection + light edit step
- [ ] Implement transcription + SRT conversion step
- [ ] Implement draft creation + upload step
- [ ] Implement playlist management step
- [ ] Implement optional blog post generation
- [ ] Implement optional LinkedIn post generation
<!-- /agent:pending -->
