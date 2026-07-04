---
name: Corky builds must use Makefile targets, not raw cargo
description: Always use make install/make release-gpu for corky — they auto-detect GPU (CUDA/Metal) with CPU fallback. Never run cargo directly.
type: feedback
---

**Use `make` targets instead of raw `cargo` commands** when building or running corky. The Makefile has GPU auto-detection built in.

**Why:** User has an NVIDIA 5090. The Makefile `install` and `release-gpu` targets already auto-detect GPU via `nvidia-smi` and build with `transcribe-cuda`, falling back to CPU if CUDA isn't available. Running `cargo run --features diarize` directly bypasses this and produces a CPU-only build, wasting 62 minutes on a 2h transcription that should take ~5-10 min with GPU.

**How to apply:**
- Build: `make release-gpu` (not `cargo build --release`)
- Install: `make install` (not `cargo install --path .`)
- Run from source: `make release-gpu && ./target/release/corky transcribe --diarize ...`
- The `install.sh` script also auto-detects GPU for prebuilt binary downloads
- GCP env vars still needed: `CORKY_DEFAULT_GCP_CLIENT_ID="$(pass corky/gcp/client_id)" CORKY_DEFAULT_GCP_CLIENT_SECRET="$(pass corky/gcp/client_secret)"`
