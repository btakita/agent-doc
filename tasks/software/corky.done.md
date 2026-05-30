# Agent Doc Completed Work

- 2026-05-29 [#vbpm] [recommended] corky crate-split phase 1: convert corky to a cargo workspace, root crate keeps lib+bin — scaffolding only, no module moves yet
- 2026-05-29 [#69ge] [recommended] corky crate-split phase 2: extract corky-transcribe (transcribe/ + whisper-rs/pyannote-rs/ort/symphonia/rubato/hound, feature-gated transcribe/cuda/metal/diarize). Only main.rs calls it, sole coupling crate::config; isolates the 2m+ CUDA build from core. Watch cross-crate #[cfg(test)] — use a test-support feature
- 2026-05-29 [#0k50] [recommended] corky crate-split phase 3: extract corky-core foundation (config, accounts, resolve, oauth_loopback, filter::gmail_auth token store, file_store, util, app_config, desktop_notify) so domain crates share auth + the tokens.json/.sync-state.json stores
- 2026-05-29 [#5wnq] [recommended] corky crate-split phase 4+: extract corky-google (doc/cal/gsc/tasks), corky-mail (sync/draft/mailbox/filter/contact/search/topics/label), corky-social; slim corky bin to CLI dispatch (cli.rs/main.rs). Keep workspace members publish=false except the corky bin; update maturin/pyproject + multi-crate release order
