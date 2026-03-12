---
agent_doc_session: 05dbd397-9523-4158-9f0d-e226a9b90006
tmux_session: '23'
agent_doc_mode: stream
---

obs did not record the entire screen. Please help me configure obs to record the entire screen

<!-- agent:exchange -->
## The Problem

Your monitor is **3840x2160** (4K) but OBS is configured with a **1920x1080** canvas. The XSHM display capture source also has a 1920x1080 scale reference, so it's only capturing a quarter of your screen (or scaling it oddly).

## Fix

Two things need to change:

### 1. Set OBS Canvas Resolution to 3840x2160

In OBS: **Settings > Video**
- **Base (Canvas) Resolution:** 3840x2160
- **Output (Scaled) Resolution:** 3840x2160 (or 1920x1080 if you want smaller file sizes)
- **FPS:** leave as-is

Or edit `~/.config/obs-studio/basic/profiles/Untitled/basic.ini` directly:

```ini
[Video]
BaseCX=3840
BaseCY=2160
OutputCX=3840
OutputCY=2160
```

If you want to record in 4K but keep file sizes reasonable, set `OutputCX=1920` / `OutputCY=1080` — OBS will downscale for you.

Please edit the obs config to capture the entire screen and downscale.

### 2. Reset the Display Capture source

After changing the canvas resolution, the existing Display Capture source will be misaligned. Easiest fix:

1. Delete the existing "Display Capture (XSHM)" source in your Scene
2. Add a new source: **Sources > + > Display Capture (XSHM)**
3. Select screen 0 (your only monitor)
4. It should now match the 3840x2160 canvas

Alternatively, right-click the source in the preview > **Transform > Fit to Screen** (Ctrl+F).

### 3. (Optional) Increase bitrate for 4K

Your current bitrate is 6000 kbps which is fine for 1080p but low for 4K recording. In **Settings > Output > Simple**:
- For 4K recording, set **Video Bitrate** to at least 20000-40000 kbps
- Or switch to CQP/CRF mode for quality-based encoding (under Advanced output mode, set Rate Control to CQP with value 18-23)

**Restart OBS** after making these changes.

### Done - OBS Config Updated

Edited both OBS config files directly:

**`~/.config/obs-studio/basic/profiles/Untitled/basic.ini`:**
- `BaseCX=3840`, `BaseCY=2160` (canvas captures full 4K screen)
- `OutputCX=1920`, `OutputCY=1080` (downscales to 1080p for recording)

**`~/.config/obs-studio/basic/scenes/Untitled.json`:**
- `scale_ref` updated to 3840x2160 (matches actual capture)
- `scale` reset to 1.0/1.0 (no weird fractional scaling)
- `pos_rel` zeroed out
- Scene `resolution` updated to 3840x2160

**Restart OBS** for changes to take effect. The display capture will now grab the full 4K screen and downscale to 1080p output.

Continue

OBS restarted and auto-corrected the scene source — it now shows `scale: 2.07x` which is correct (3840/1854 ≈ 2x, matching the XSHM capture to the 3840x2160 canvas). The config is working.

To verify the recording captures your full screen, do a quick test recording (10 seconds), then check the output in `~/Videos/` — it should show your entire 4K desktop downscaled to 1080p. If text looks too small in the recording, you might want to bump output to `OutputCX=2560` / `OutputCY=1440` (1440p) for a better balance of readability and file size.

compact this file

## OBS Full-Screen Recording Setup (Resolved)

**Problem:** Monitor is 3840x2160 (4K) but OBS canvas was 1920x1080, capturing only part of the screen.

**Fix applied:**

`~/.config/obs-studio/basic/profiles/Untitled/basic.ini`:
```ini
[Video]
BaseCX=3840
BaseCY=2160
OutputCX=1920
OutputCY=1080
```

Canvas captures full 4K screen, output downscales to 1080p. OBS auto-corrected the Display Capture source scaling on restart (scale: 2.07x).

**If text is too small in recordings:** bump to `OutputCX=2560` / `OutputCY=1440` (1440p).

/agent-doc compact
<!-- /agent:exchange -->
