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

## Assistant

### In-House Transcription Pipeline vs Vendor-Provided

Architecture: Phone Service (recording only) → Webhook → Worker → Whisper → Storage → AI Agents

### Cost comparison (5 users, ~2,000 min/mo call volume)

| Component | Vendor (Dialpad) | In-House (Cloudflare) | In-House (GPU VPS) |
|---|---|---|---|
| **Phone service** | $75/mo (includes transcription) | $33-$75/mo (recording-only: Twilio or Dialpad) | Same |
| **Transcription** | Included | Whisper API: ~$12/mo (2K min × $0.006/min) | Self-hosted Whisper: $0 (included in VPS) |
| **Compute** | — | Workers: ~$5/mo (webhook + orchestration) | GPU VPS: ~$30-$80/mo (Hetzner/Lambda) |
| **Storage** | Vendor-hosted | D1: ~$1/mo (transcripts); R2: ~$1/mo (recordings) | Same or local |
| **Total** | **$75/mo** | **$51-$93/mo** | **$63-$155/mo** |

### Whisper transcription options

| Option | Cost/min | Diarization | Quality | Latency |
|---|---|---|---|---|
| **OpenAI Whisper API** | $0.006 | No (need post-processing) | Best | ~30s/min |
| **Groq Whisper** | $0.0028 | No | Good | ~5s/min (fastest) |
| **Deepgram** | $0.0043 | Yes (built-in) | Good | Real-time |
| **Self-hosted Whisper** | $0 (GPU cost) | Yes (your corky pipeline) | Best + diarized | Depends on GPU |
Self-hosted would be CPU. With some delay to process phone calls on cloud instance(s).
| **Cloudflare Workers AI** | $0.019/1K neurons | Limited models | Moderate | Workers edge |

### Storage options on Cloudflare

| Storage | Best for | Cost | Query capability |
|---|---|---|---|
| **D1** (SQLite) | Transcript text, metadata, search | $0.75/mo per 1M reads | Full SQL — best for AI agent queries |
What are the storage fees?
| **R2** | Audio file storage (recordings) | $0.015/GB/mo, no egress fees | Object storage only |
| **KV** | Fast key-value lookups | $5/mo (10M reads) | Key-based only |
| **Vectorize** | Semantic search across transcripts | $0.01/1K vectors stored | Embedding similarity search |

Best Cloudflare stack: D1 for transcripts + metadata (searchable) + R2 for audio files + Vectorize for semantic AI agent queries.

### Architecture comparison

**Vendor (Dialpad):**
```
Call → Dialpad records + transcribes → API query → AI agent
```
- Pro: Zero infrastructure, works immediately
- Con: Vendor lock-in, basic transcription (no diarization), transcript format limited to their schema

**In-House (Cloudflare + corky Whisper):**
```
Call → Phone service records → Webhook → CF Worker →
  → Download recording → R2 storage
  → corky Whisper (diarized) → D1 transcript + Vectorize embedding
  → AI agent queries D1/Vectorize
```
- Pro: Diarized transcripts, full control, semantic search, no vendor lock-in on data
- Con: Dev work upfront, pipeline maintenance

**Hybrid (recommended):**
```
Call → Dialpad (records + basic transcript) → Webhook →
  → CF Worker downloads recording → R2
  → corky Whisper (diarized, higher quality) → D1 + Vectorize
  → AI agents query both Dialpad API (quick) and D1/Vectorize (deep)
```
- Dialpad's built-in transcript for immediate/basic needs
- Your Whisper pipeline for diarized, searchable, AI-ready transcripts
- Best of both worlds

### CRM integration

| CRM | Transcript storage | AI agent access | Notes |
|---|---|---|---|
| **GoHighLevel** | Custom fields on contact/call records | GHL API | Robert already uses; push transcripts via API |
Are the transcripts stored in GHL CRM itself? What are the fees?
| **Cloudflare D1** | Full SQL database | Workers API / direct query | Most flexible for custom AI agents |
| **Both** | D1 as primary + sync to GHL | Query either | Recommended if Robert's CRM is central |

### Bottom line

| Approach | Monthly cost | Dev effort | Quality | AI agent readiness |
|---|---|---|---|---|
| **Dialpad only** | $75 | None | Basic transcript | Good (API search) |
| **Dialpad + CF Whisper** | ~$90-$95 | Medium (one-time) | Diarized + semantic search | Best |
| **Twilio + CF Whisper** | ~$50-$55 | Higher (no phone UX) | Diarized + semantic search | Best |
| **GHL + CF Whisper** | ~$110-$135 | Medium | Diarized + CRM integrated | Best |

**Recommendation:** Dialpad ($75) + Cloudflare in-house pipeline (~$18/mo extra). You get immediate working phone system with basic transcripts, plus your corky Whisper pipeline produces superior diarized transcripts stored in D1/Vectorize for AI agent access. The Cloudflare infrastructure cost is minimal — most of the value comes from the dev work you've already done in corky.

## User

Dialpad + Cloudflare looks good. Depending on GHL costs. Is there a better solution for cloud computing such as AWS or Google Cloud?

## Assistant

### Inline clarifications

**CPU-based Whisper:** whisper.cpp on CPU is very viable for batch processing. A Hetzner CPX41 (8 vCPU, ~$18/mo) transcribes at ~0.3-0.5x real-time with the medium model. For 2,000 min/mo → ~2-4 hours/day as a background queue. No GPU needed. Updated in-house total: **~$57/mo**.

| Component | In-House (CPU VPS) |
|---|---|
| **Phone service** | $33/mo (Twilio recording-only) |
| **Transcription** | $0 (whisper.cpp on VPS) |
| **Compute** | ~$18/mo (Hetzner CPX41) + ~$5/mo (CF Workers) |
| **Storage** | ~$0 (D1 free tier) + ~$1/mo (R2) |
| **Total** | **~$57/mo** |

**D1 storage fees:**

| Tier | Storage | Reads | Writes |
|---|---|---|---|
| **Free** | 5 GB | 5M reads/day | 100K writes/day |
| **Paid** | $0.75/GB-month | $0.75/million reads | $1.00/million writes |

~2,000 calls/mo × ~5 KB = ~10 MB/mo. Stays in free tier for years.

**GHL transcript storage:** Included in your plan ($97-$297/mo) — no per-record storage fees. Transcripts go in as contact notes/custom fields. Audio storage is $0.0005/min. Best approach: full transcripts in D1, push summaries + links to GHL.

### Cloud platform comparison for transcript pipeline

| Platform | Compute (webhook + orchestration) | Database | Object storage | Vectorize/search | Free tier | Total estimate |
|---|---|---|---|---|---|---|
| **Cloudflare** | Workers: $5/mo (paid plan) | D1: free (5GB) | R2: $0.015/GB, no egress | Vectorize: $0.01/1K vectors | Generous | **~$6-8/mo** |
| **AWS** | Lambda: ~$1-3/mo | DynamoDB: ~$1-5/mo; RDS: ~$15-30/mo | S3: $0.023/GB + $0.09/GB egress | OpenSearch: ~$25+/mo | 12-month trial | **~$27-58/mo** |
| **Google Cloud** | Cloud Functions: ~$1-3/mo | Firestore: ~$1-5/mo; Cloud SQL: ~$10-25/mo | GCS: $0.020/GB + $0.12/GB egress | Vertex AI Search: ~$25+/mo | $300 credit | **~$27-53/mo** |
| **Azure** | Functions: ~$1-3/mo | Cosmos DB: ~$25+/mo | Blob: $0.018/GB + egress | AI Search: ~$25+/mo | $200 credit | **~$51-53/mo** |
| **Supabase** | Edge Functions: $10/mo (Pro) | PostgreSQL: included (8GB) | Storage: included (100GB) | pgvector: included | Good | **~$25/mo** |
| **Hetzner + self-hosted** | VPS: $18/mo (also runs Whisper) | SQLite/PostgreSQL: $0 | Local: $0 | pgvector: $0 | N/A | **~$18/mo (all-in-one)** |

### Key differences

**Cloudflare wins on:**
- Zero egress fees (AWS/GCP charge $0.09-$0.12/GB for downloads — matters when AI agents pull recordings)
- D1 free tier is very generous for this workload
- Workers are cheapest for webhook/orchestration
- Vectorize is built-in and cheap
- Global edge deployment (low latency)

**AWS/GCP win on:**
- Managed ML services if you want to run Whisper as a cloud function (SageMaker, Vertex AI)
- More mature monitoring/logging
- Broader ecosystem if you need other services later
- Better for teams already on those platforms

**Hetzner VPS wins on:**
- All-in-one: Whisper transcription + database + storage on one $18/mo box
- Simplest architecture — no cloud functions, no managed services
- But: single point of failure, you manage the server

### Recommendation

**For this workload, Cloudflare is the best fit.** The transcript pipeline is lightweight — webhook triggers, file storage, database writes, vector search. Cloudflare's free/cheap tiers cover it all. AWS and GCP are overkill and 3-8x more expensive for this use case due to egress fees and managed service minimums.

**If you want the absolute cheapest:** Hetzner CPX41 ($18/mo) runs everything — Whisper, SQLite, file storage — on one box. Add Cloudflare Workers ($5/mo) just for the webhook/routing layer. Total: ~$23/mo for the entire pipeline.

**If already invested in AWS/GCP:** Possible, but you'd pay ~$30-50/mo more for equivalent functionality. Only worth it if you need the broader ecosystem.

## User

I want Cloudflare for hosting. What are the costs for diarized transcription compute?
Do I need vector storage or can I just have the agents query SQLite?

## Assistant

### Diarized transcription compute costs

Cloudflare Workers AI runs Whisper but does **not** offer speaker diarization. Diarization requires separate models (pyannote, resemblyzer) that need more compute.

| Approach | Diarized? | Cost/mo | Complexity | Latency |
|---|---|---|---|---|
| **CF Workers AI (Whisper only)** | No | ~$8/mo | Low | Minutes |
| **CF Worker → Deepgram API** | Yes | ~$14/mo ($9 Deepgram + $5 Workers) | Low | Minutes |
| **CF Worker → AssemblyAI** | Yes | ~$18/mo ($13 AssemblyAI + $5 Workers) | Low | Minutes |
| **CF Worker → Hetzner (corky)** | Yes (your pipeline) | ~$23/mo ($18 VPS + $5 Workers) | Medium | Hours (batch) |
| **CF Worker → GPU burst** | Yes (your pipeline) | ~$10-15/mo | High | Minutes |

**Deepgram** ($0.0043/min) is the simplest — built-in diarization, just call their API from a Worker. **Hetzner** ($18/mo) is best for your corky pipeline — unlimited minutes, full control, CPU batch processing (~3-7 hours/day for 2K min).

### Vector storage vs SQLite

**You don't need vector storage.** D1 (SQLite) with FTS5 handles mortgage brokerage call search very well:

```sql
-- FTS5 full-text search (built into D1)
CREATE VIRTUAL TABLE transcripts_fts USING fts5(speaker, text, call_date);
SELECT * FROM transcripts_fts WHERE transcripts_fts MATCH 'refinancing AND credit score';
```

**SQLite/D1 handles well:**
- Keyword search across all calls ("refinancing", "credit score", borrower name)
- Date/time filtering ("calls from last week")
- Speaker-specific search ("what did John say about...")
- Structured queries (call duration, phone number, contact lookup)

**Vector storage only matters for:**
- Semantic/fuzzy search: "calls where client seemed hesitant" (no exact keyword)
- Finding similar calls without shared keywords
- RAG-style retrieval for AI agents doing open-ended analysis

For mortgage calls, the domain vocabulary is specific — loan amounts, rates, property addresses, borrower names. **Keyword search works great.** Start with D1 + FTS5 (free, built-in). Add Vectorize later only if agents can't find what they need via keywords.

### Revised total cost (Cloudflare + Dialpad)

| Component | Cost/mo |
|---|---|
| **Dialpad** (5 users, recording + basic transcript) | $75 |
| **CF Workers** (webhook, orchestration) | $5 |
| **Diarized transcription** (Deepgram or Hetzner corky) | $9-$18 |
| **D1** (transcript storage + FTS5 search) | $0 (free tier) |
| **R2** (audio recording storage) | ~$1 |
| **Total** | **$90-$99/mo** |

## User

