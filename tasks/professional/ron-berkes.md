---
agent_doc_session: 375061ae-6496-4ab9-a303-2f0b0da4f61a
agent_doc_mode: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Summary

Collaboration with Ron Berkes on smart contract manufacturing + call transcription pipeline.

### Smart Contract Manufacturing

Ron Berkes runs [manufacturingchina.com](https://manufacturingchina.com/) — a factory listing platform (products, services, equipment). Built in Astro/Vercel/Supabase, ~85% done. Critical SEO/indexing issue (Google not indexing factory pages — was fine on WordPress).

**Smart contract concept:** USDT-based payments (Tron network), milestone escrow (blueprint → PO → deposit → production → delivery). Blueprint is source of truth for disputes. Insurance built into contract as prerequisite.

**Julian / 3A Credit:** Publicly traded credit risk management company. Will open resources once Manufacturing China launches. Could handle dispute resolution + potentially acquire the smart contract platform.

**Composable stack:** Uniscrow (escrow) + VeChain (tracking) + Chainlink (oracles) + Otonomi (insurance) + Kleros (disputes). No turnkey solution — MVP would be multi-milestone escrow with manual confirmations.

**Action items from call:**
1. Brian to sign up as factory on manufacturingchina.com
2. Ron to share GitHub, Supabase, Google Search Console access
3. Evaluate: fix Astro SEO or migrate to WordPress
4. ~7-10 day ramp-up timeline

### Transcription & Diarization

**Two calls transcribed** (Feb 26, 2026):
- `20260226-ron-berkes.md` — short call (3:16)
- `20260226-ron-berkes-diarized-1.md` — long call (69:16), completed

Location: `mail/contacts/ron-berkes/recordings/`

**Pipeline:** whisper-rs transcription + pyannote-rs (ONNX Runtime, zero Python) diarization. Implemented as `corky transcribe --diarize` with `pyannote-rs = "0.3"` + `ort` crate.

**Features built:**
- Confidence scores — `(0.XX)` after speaker name, predicts accuracy (low scores = likely mislabeled)
- Unknown segment elimination — `reassign_unknown_segments()` assigns to nearest known speaker by temporal proximity
- Interactive speaker labeling — `--diarize` without `--speakers` shows excerpts, user assigns names
- `--speakers "Ron Berkes,Brian Takita"` shortcut (maps by first appearance order, can drift)

**Key files:**
- `corky/src/transcribe/diarize.rs` — segmentation → embedding → clustering → labeling
- `corky/src/transcribe/model.rs` — ONNX model auto-download (no gated access needed)
- `corky/src/transcribe/engine.rs` — merges speaker labels with whisper timestamps

**Known issues:**
- Name swaps at end of calls (clustering issue, not labeling). Voice fingerprints would fix.
- Short segments (< 0.5s) produce unreliable embeddings → low confidence scores
- tdrz fails on mono phone recordings (needs distinct audio channels)

### Voice Fingerprint Design (not yet implemented)

Per-contact embeddings stored in `mail/contacts/*/voice.bin` (~1KB). On future calls: extract embeddings → cosine similarity against stored voiceprints → auto-label above 0.85 threshold → interactive prompt for unknowns → save/update embedding on confirmation. Cross-contact, improves over time via rolling average.

**Rollout plan:**
1. Done: pyannote-rs + interactive labeling + confidence scores + unknown segment elimination
2. Next: Voice fingerprint storage on label assignment
3. Later: Auto-match on future calls + content-based fallback

### Correspondence Setup

Ron is a contact (`mail/contacts/ron-berkes/`). Emails `rberkes@gmail.com` and `berkes06@yahoo.com` in correspondence filter. Topic: `mail/topics/manufacturing-china/`.

<!-- agent:exchange -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/2dd4980668edf895cd3963ca4e82829a902c4e876acbfc1bac3220c85f56eee8-20260512-185151.md`*

Compacted content:
- Archived 3 response topic(s): Compact exchange + transcription requests; 4 calls transcribed with GPU diarization; transcribe + diarize
- Prior summary/context: **Compacted.** 770 lines → structured summary preserving all decisions, outcomes, and technical details. --- ### Tran...
<!-- agent:boundary:7626d1fa -->
<!-- /agent:exchange -->

<!-- agent:pending patch=replace -->
- [ ] Start MRH WordPress optimization (admin access: vtac1)
- [ ] Research Liquid Web dedicated WordPress hosting vs alternatives
- [ ] Set up Cloudflare CDN for MonsterRodHolders.com
- [ ] Reinstall Google Analytics JS on MRH site
- [ ] Patent automation proposal for Ron (reduce Patricia's 15-30 min/patent)
- [ ] Review Ron's patent spreadsheet (needs access -- share publicly or export CSV)
- [ ] Corky Google Docs/Sheets support (added to tasks/software/corky.md)
- [ ] Investigate RealWeeds.com / MonsterOutdoors.com acquisition opportunity
- [ ] Wait for Steve to reach out directly
- [ ] Draft Julian API integration proposal (factory reports, revenue share model)
- [x] Transcribe phone_20260402-125728_6504368181.m4a with GPU diarization
- [x] Transcribe phone_20260402-152955_6504368181.m4a with GPU diarization
- [x] Transcribe phone_20260402-203929__16504368181.m4a with GPU diarization
- [x] Transcribe phone_20260403-121627_6504368181.m4a with GPU diarization
<!-- /agent:pending -->
 
