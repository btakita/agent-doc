# Transcription

Transcribe audio and video files with optional speaker diarization.

## Basic Usage
```bash
corky transcribe <FILE>
```

## With Diarization
```bash
corky transcribe <FILE> --diarize --speakers "Brian,Robert"
```

- `--diarize` enables speaker detection via pyannote-rs
- `--speakers` provides speaker names (also constrains speaker count)
- Adaptive chunking is on by default (cascade: full → 10min → 2min → 30s)
- Unknown speaker resolution via LLM is on by default
- Opt out: `--no-adaptive-chunk`, `--no-resolve-unknown`

## Configuration (.corky.toml)
```toml
[transcription]
model = "large-v3-turbo"
language = "en"
adaptive_chunk = true
resolve_unknown = true
```
