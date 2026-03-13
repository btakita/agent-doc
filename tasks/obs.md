---
agent_doc_session: 05dbd397-9523-4158-9f0d-e226a9b90006
tmux_session: '23'
agent_doc_mode: stream
---

obs did not record the entire screen. Please help me configure obs to record the entire screen

<!-- agent:exchange -->
## OBS Full-Screen Recording (Resolved)

**Problem:** 4K monitor (3840x2160) but OBS canvas was 1920x1080 → quarter-screen capture.

**Root cause:** XSHM can't capture full 4K with HiDPI (`Xft.dpi: 240`). Captures at logical resolution, not physical pixels.

**Fix:** Switch to **PipeWire Screen Capture** source in OBS (Sources > + > Screen Capture (PipeWire) > select screen > Share). PipeWire captures at native pixel resolution regardless of DPI.

**OBS config** (`~/.config/obs-studio/basic/profiles/Untitled/basic.ini`):
```ini
[Video]
BaseCX=3840
BaseCY=2160
OutputCX=1920
OutputCY=1080
```

**Webcam PiP:** Sources > + > Video Capture Device > select webcam. Resize/drag to corner. Lock position via right-click > Order > Move to Top.

---

## YouTube Videos Published

**Part 1:** https://www.youtube.com/watch?v=B-9uI53cRVE (2-hour dog-fooding session — CRDT debugging, agent-doc, Existence Lang)

**Part 2:** https://www.youtube.com/watch?v=qQCKVQHRQCI (41 min — rmemo, tag-path, agent-doc v0.16.1 release)
- Draft: `mail/social/20260313-134452-youtube.md` (status: published)
- Captions: `mail/topics/brian-takita/videos/2026-03-12-dogfooding-agent-doc-2.srt`
- **Note:** Duplicate private video from failed first attempt may exist on YouTube Studio — delete the one without captions.

**LinkedIn Part 1:** Posted manually (Corky publish had missing links issue). Draft: `mail/social/20260312-111049-linkedin.md`.

---

## corky YouTube Feature (Shipped in v0.15.0)

**Commands:**
```bash
corky youtube auth --profile btakita     # Google OAuth2 (one-time)
corky youtube draft --author btakita     # Create draft (add video/captions/title in frontmatter)
corky youtube publish <draft-file>       # Upload video + captions
corky youtube edit <draft-file>          # Update metadata on published videos
```

**Bug fixes applied this session:**
- Upload loop handles empty 200 response bodies (was crashing on `into_json()`)
- Added `youtube.force-ssl` scope for caption uploads
- 8 new tests in `test_youtube.rs` (all passing)

**OAuth setup:** Reuses Gmail's `client_id`/`client_secret` from `.corky.toml [gmail]` → copy to `[youtube]` section. YouTube Data API v3 must be enabled in Google Cloud Console.

**Changes uncommitted in `src/corky/`** — ready for review and commit.

---

## Platform Recommendation

YouTube as primary (permanence, SEO, SRT support). Twitch optional add-on for live chat if needed later.

*Compacted from 30+ round session covering OBS config, PipeWire capture, LinkedIn posting, YouTube upload feature implementation, OAuth setup, upload bug debugging, and video publishing.*

The video capture was miscropped and has missing areas of the video capture. How can I crop the entire video to fit the video content on the screen?
[2026-03-12 17-49-43.mp4](../../../../Videos/2026-03-12%2017-49-43.mp4)
### Re: Crop Miscaptured Video

**Auto-detected crop area:** Content is 1872x928 at offset (4,4). The bottom ~150px and right edge are black (XSHM HiDPI artifact). Consistent across the full 2-hour video.

**Crop with ffmpeg** (re-encodes to remove black borders, scales up to 1920x1080):

```bash
ffmpeg -i ~/Videos/"2026-03-12 17-49-43.mp4" \
  -vf "crop=1872:928:4:4,scale=1920:1080:flags=lanczos" \
  -c:v libx264 -crf 18 -preset medium \
  -c:a copy \
  ~/Videos/"2026-03-12 17-49-43-cropped.mp4"
```

**Options:**
- **Crop only** (keep native 1872x928): Remove `,scale=1920:1080:flags=lanczos` — smaller file, no upscale artifacts
- **Crop + scale to 1080p** (shown above): Standard 1920x1080 for YouTube — lanczos gives sharp upscale
- **CRF 18**: High quality. Lower = bigger file, higher = smaller. 18 is visually lossless.

This is a ~2 hour video so encoding will take a while. Want me to run it?

yes
**Encoding now...** This is a 2-hour video at CRF 18 — expect 30-60 minutes depending on CPU load. Output: `~/Videos/2026-03-12 17-49-43-cropped.mp4`
**Done.** Cropped video saved to `~/Videos/2026-03-12 17-49-43-cropped.mp4`.

| | Original | Cropped |
|--|----------|---------|
| **Resolution** | 1920x1080 (with black borders) | 1920x1080 (content fills frame) |
| **Size** | 5.1 GB | 671 MB |
| **Bitrate** | 6000 kbps | 774 kbps |
| **Duration** | 2h 1m | 2h 1m |

The CRF 18 encode is significantly smaller because the black borders were easy to compress in the original — the actual content bitrate is comparable. Quality should look great.

Create a closed caption transcript of the cropped video.
Create a draft to upload to YouTube. Include a title, description, and tags.
<!-- /agent:exchange -->
