---
agent_doc_session: ae91195b-f87b-4448-9f5b-515ea4af72f3
tmux_session: '1'
---

Research costs for office phone system with recording.
Recording with transcripts available for search and AI integration.

Compare Twilo, JustCall, and other competing services.

## Assistant

### Cloud Phone Systems (5-user mortgage brokerage pricing)

| Service | $/user/mo | Recording | Transcription | AI Features | API | 5-user total |
|---|---|---|---|---|---|---|
| **Dialpad** | $15-$25 | Included (all) | Included (all) | Real-time transcription, summaries (all plans) | Pro+ | **$75/mo** |
| **Zoom Phone** | $10-$15 | Included | Included (AI Companion) | Summaries, voicemail priority, task extraction | Yes | **$75/mo** |
| **OpenPhone (Quo)** | $15-$35 | On-demand (Starter); auto (Business+) | Business+ plans | AI summaries (Business+) | Yes | $75-$175/mo |
| **Google Voice** | $17-$37 (incl. Workspace) | Standard+: on-demand; Premier: auto | Voicemail only | Minimal | Limited | $85-$185/mo |
| **RingCentral** | $20-$45 | Manual (Core); auto (Advanced+) | Included | Basic included; **RingSense $60/user extra** | Yes | $100-$225/mo |
| **JustCall** | $19-$49 | Included | **Capped: 60 min/user/mo** | $9/user add-on | Yes | $95-$245/mo |
| **Aircall** | $30-$50 (min 3 users) | Included | Not included | AI Assist $9-$49/user extra | Yes | $150-$250/mo |
| **Vonage** | $14-$28 base | **$5-$50/mo add-on** | **$50/mo per 200hrs add-on** | Minimal | Yes | $70-$140 + $55-$100 add-ons |

### Flat-rate / API Services

| Service | Pricing | Recording | Transcription | AI | Notes |
|---|---|---|---|---|---|
| **GoHighLevel** | $97-$297/mo flat (unlimited users) | Included ($0.0005/min storage) | $0.024/min add-on | CRM AI, workflows | Robert already uses. ~$173/mo total w/ 2K min calls + transcription |
| **Twilio** | Pay-as-you-go or Flex $150/user | $0.0025/min | $0.035/min | Build-your-own | Requires dev work. Only for custom solutions |

### Self-Hosted / Open Source

| Platform | Recording | Transcription | AI | Cost estimate |
|---|---|---|---|---|
| **FusionPBX 5.5** (FreeSWITCH) | Built-in | **Built-in** (Jan 2026 release) | Call summaries, TTS | ~$50/mo (VPS + SIP trunking) |
| **FreePBX** (Asterisk) | Built-in | Scribe add-on (AI) | Via Scribe module | ~$50/mo |
| **Wazo Platform** | Built-in | Add Whisper/etc. | Programmable API | ~$30-$80/mo |

### Recommendations

| Priority | Best Choice | Why | 5-user cost |
|---|---|---|---|
| **Cheapest with AI** | Dialpad Standard | AI transcription + recording on all plans | $75/mo |
| **Already on GHL** | GoHighLevel + Voice Intelligence | Flat rate, integrates with CRM Robert uses | ~$173/mo |
| **Already on Zoom** | Zoom Phone Unlimited | AI Companion included | $75/mo |
| **Self-hosted control** | FusionPBX 5.5 | Transcription now built-in, unlimited | ~$50/mo |
| **Custom integration** | Twilio or GHL API | Full API control | Variable |

**For mortgage brokerage:** Compliance recording matters (CFPB/TILA). Dialpad at $15/user is the best value — automatic recording + AI transcription on all plans. If Robert's GoHighLevel is already in play, adding Voice Intelligence transcription there avoids switching costs and keeps everything in one CRM.

**Key gotchas:**
- JustCall transcription is **capped at 60 min/user/mo** — mortgage calls run long
- Vonage base price is deceptive — recording + transcription add-ons cost $55-$100/mo on top
- RingCentral's RingSense AI is **$60/user/mo extra** — steep for small team
- Google Voice has **no call transcription** — voicemail only

I can do transcription with custom software. I already have diarized whisper integration in corky library. I just need access to the recordings via api. Which service has the best api? 

## User

Which service(s) are easy to transition existing office phone system + cell phone plans? Which services have a good api to access recordings? Which services provide reasonable transcription that can be saved and accessed via AI agents?

## Assistant

### 1. Easy transition (number porting + onboarding)

| Service | Office port | Cell port | Desk phones | Mobile app | Transition ease |
|---|---|---|---|---|---|
| **Dialpad** | Free | Yes | IP phones optional | iOS/Android | Easy — port, install app, done |
| **RingCentral** | Free | Yes | Ships pre-configured | iOS/Android | Easy — white-glove setup |
| **OpenPhone/Quo** | Free | Yes | App-only | iOS/Android | Easiest — app-only |
| **GoHighLevel** | Yes | Limited | App/browser | iOS/Android | Moderate — CRM-first |
| **Google Voice** | Free | **No cell porting** | No | iOS/Android | Limited |

All major services port existing numbers. Staff keeps personal cell separate — business number runs as a second line through the app.

### 2. Best API for recording access

| Service | API | Recording download | Webhook on call end | Notes |
|---|---|---|---|---|
| **Twilio** | Best | RecordingUrl in callback | Yes (StatusCallback) | ~$33/mo. No phone UX — needs softphone |
| **Dialpad** | Good | REST API v2, recording endpoints | Yes (call.completed) | $75/mo. Clean docs |
| **GoHighLevel** | Good | REST API, call log recordings | Yes | ~$97-$125/mo. Robert already uses |
| **OpenPhone/Quo** | Good | Recording URL in call object | Yes | $75/mo. Clean REST API |

For your corky Whisper pipeline: Twilio is cheapest, Dialpad/OpenPhone are most practical (good API + good phone app).

### 3. Searchable transcription for AI agents

| Service | Transcription | Searchable? | AI agent access | Cost |
|---|---|---|---|---|
| **Dialpad** | Included (all plans) | Full text search across all calls | API: transcript text in call objects | $15/user |
| **Zoom Phone** | AI Companion included | Searchable transcripts | API: transcript download | $15/user |
| **RingCentral** | Included | Searchable call logs | API: transcript content endpoint | $20/user; RingSense $60/user extra |
| **GoHighLevel** | $0.024/min add-on | Via CRM search | API: call transcription data | Variable |
| **OpenPhone/Quo** | Business+ ($23/user) | Yes | API: transcript in call objects | $23/user |

**For AI agent access:** Dialpad and OpenPhone expose transcript text directly in their API call objects — your agents can query call history, search by keyword, and pull context without downloading audio. Dialpad does this on the $15/user plan.

### Recommendation

**Dialpad at $15/user ($75/mo for 5 users):**
- Easy transition (port all numbers, install apps)
- Good API with recording download + transcript text
- Built-in searchable transcription for quick access
- Your corky Whisper pipeline for higher-quality diarized transcription
- AI agents query Dialpad API for call context

**GoHighLevel as complement** (Robert already uses): CRM workflows + call routing. Can coexist with Dialpad or replace it if GHL's phone features are sufficient.

## User

Compare with processing transcripts with recording in house on Cloudflare or another cloud service. Saving transcripts in Cloudflare database or CRM.
