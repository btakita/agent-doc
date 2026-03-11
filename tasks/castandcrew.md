---
agent_doc_session: d3df3cf3-c822-4e2e-88cd-4f266fa2ff1b
tmux_session: '0'
agent_doc_mode: template
---

Please help me prep for my interview with Jonathan with castandcrew.com.

See my correspondence and calendar
## Assistant

## Interview Prep: Cast & Crew — Senior Python Software Engineer

**When:** Today, Monday March 9, 2026 @ 3:30 PM ET
**With:** Jonathan Martinez — Recruiter, Cast & Crew (jonathan.martinez@castandcrew.com, 818-525-3391, Burbank CA)
**How:** Microsoft Teams — Meeting ID: 246 228 269 695 84, Passcode: wy9z9DS9

---

### Company Overview

**Cast & Crew** — Entertainment payroll & technology company founded 1976, headquartered in Burbank, CA. Provides payroll, production finance, and workforce management for film/TV/streaming. Acquired **Backstage.com** in 2022 — an online casting and talent marketplace (founded 1960 as a print magazine, went digital).

The role is on the **Backstage engineering team** — Python/Django backend, team of 8 engineers, fully remote (US).

---

### Role Details

**Senior Python Software Engineer** — Backend-focused on Backstage's casting/talent platform.

**Core tech:** Python, Django, MySQL, Elasticsearch, REST APIs
**Likely also:** Redis, Celery, Docker, CI/CD, possibly AWS

---

### Your Strongest Talking Points

1. **Python depth** — Django, FastAPI, Flask in your skills. Production Python systems across multiple roles.
2. **Presence AI** — Real-time multimodal AI pipeline. Demonstrates complex backend architecture.
3. **Censible** — 8 years building a product end-to-end. Shows ownership and longevity.
4. **Pivotal Labs** — Mentoring engineers, pair programming, TDD. Senior-level leadership.
5. **corky / agent-doc** — Full correspondence toolkit + interactive document system. Initiative and system design.
6. **REST API experience** — Multiple roles building APIs (TrueCar, Censible, Pivotal). Direct match.

---

### Likely Recruiter Questions

| Question | Your angle |
|----------|-----------|
| Walk me through your background | 20+ years full-stack → recent AI pipelines and developer tooling. Python is core. |
| Why Cast & Crew / Backstage? | Interesting domain (entertainment tech), mature product, Python/Django — great match. |
| Python/Django experience? | Django across projects, FastAPI for newer services, Flask for lightweight APIs. Full Django ecosystem. |
| Challenging project? | Presence AI — real-time video pipeline across NVIDIA GPUs. Or Censible — 8 years iterating on complex product. |
| Salary / availability? | Have your range ready. Confirm remote US works for your location. |

---

### Questions to Ask Jonathan

- What does the engineering culture look like on the Backstage team?
- What's the tech stack beyond Python/Django? (Redis, Celery, Docker, cloud?)
- What's the team's current focus — new features, scaling, migration?
- What does the full interview process look like after this call?
- How does the Backstage team fit within the larger Cast & Crew org?
- What's the deployment cadence like?

---

### Pre-Interview Checklist

- [ ] Join Teams link 5 min early
- [ ] Have resume open for reference
- [ ] Quiet environment, camera on
- [ ] Notepad for questions/answers

## User

How can I record the call using my screencast?
With audio
## Assistant

You have **OBS Studio** (`/usr/bin/obs`) installed — best option for recording with audio.

**Quick setup:**

1. **Launch:** `obs &`
2. **Add Sources** (bottom-left +):
   - **Window Capture** → pick your Teams window
   - **Audio Output Capture** (PulseAudio) → system audio (call audio)
   - **Audio Input Capture** (PulseAudio) → your microphone
3. **Settings → Output:** Recording format **mkv** (crash-safe), path `~/Videos/`
4. **Start Recording** before the call
5. **After:** Stop recording → File → Remux Recordings → .mkv to .mp4

**Simpler alternative:** You also have **Kazam** (`/usr/bin/kazam`) — does fullscreen/window capture with audio, less setup but less control.

**Tip:** Test audio levels before the call — OBS shows live audio meters in the mixer panel. Make sure both Desktop Audio and Mic/Aux are picking up signal.

## User
