# agent-loop — Local

Dog-fooding the agent-doc CLI tool and `/agent-doc` Claude Code skill via plan.md sessions.

## agent-doc Release Checklist

When releasing agent-doc, always run `cargo build --release` inside the submodule after publishing. The `.bin/agent-doc` symlink points to `src/agent-doc/target/release/agent-doc` — without a release build, the symlink target stays stale.

## Audio Transcription

Prefer `corky transcribe` for audio transcription over raw whisper CLI. Use with `--diarize` for speaker diarization and `--speakers "Name1,Name2"` for labeling. GPU acceleration is used automatically when available.
