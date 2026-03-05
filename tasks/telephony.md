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


