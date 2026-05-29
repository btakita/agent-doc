//! # Module: stream
//!
//! ## Spec
//! - `run()` is the top-level entry point for `agent-doc stream <FILE>`.
//! - Requires the document write strategy to be CRDT (`resolved.is_crdt()`); bails
//!   with a user-facing message if the document uses a non-CRDT write mode.
//! - Reads `StreamConfig` from frontmatter (interval, target component, thinking flags).
//! - Computes a diff against the last snapshot; exits early with no error when nothing changed.
//! - Ensures a session UUID exists in frontmatter before sending; writes it back if absent.
//! - Resolves the streaming agent from the `--agent` flag, frontmatter, or config default.
//! - Pre-commits user changes to git before sending; final-commits after stream completes.
//! - `stream_loop()` spawns a timer thread that flushes the accumulated text buffer to
//!   the document on a configurable interval (default 2 s). Flush uses **replace** mode
//!   for the target component regardless of component default, because the buffer is
//!   cumulative (full text so far, not a delta).
//! - While streaming, the first non-empty partial response and then changed
//!   partial responses at most once every 30 seconds are saved as durable
//!   `.partial.json` checkpoints next to the final response capture ledger.
//! - Thinking content (`chunk.thinking`) is routed to a separate component when
//!   `thinking_target` is set, or interleaved as `<details>` when unset.
//! - On stream completion: saves crash-recovery pending file, writes final content, saves
//!   CRDT state + snapshot, clears pending file, and updates `resume` frontmatter field.
//! - `flush_to_document()` tries IPC to the IDE plugin first; falls back to flock +
//!   atomic write on IPC absence or timeout.
//! - `build_prompt()` produces distinct prompts for first submit (no `resume`) vs.
//!   resumed sessions (includes diff + full document). Resumed prompts also restate
//!   ordered request blocks extracted from the diff so the agent does not anchor only
//!   on the newest question in a changed exchange tail. When session accretion has
//!   already reached warn/block severity and the diff still contains live prompt
//!   targets, resumed prompts replace the full exchange tail with a bounded
//!   response-context pack containing prompt targets, session summary, backlog
//!   head, and available component names.
//!
//! Write-back loop:
//! ```text
//! [Agent chunks] → [Buffer] → [Timer] → [IPC or Lock → Read → patch(replace) → Write → Unlock]
//! [User edits]   → [File]  → [Detected by CRDT merge on next flush]
//! ```
//!
//! ## Agentic Contracts
//! - `run()` is the sole public entry point; all internal state is encapsulated.
//! - `flush_to_document()` is `pub` so `watch.rs` can reuse it for stream-capture polling.
//! - `flush_to_document()` is safe to call concurrently from multiple documents; each
//!   call acquires a per-file advisory lock before writing.
//! - Callers may pass `--no-git` to suppress all git operations (useful in tests).
//! - The `_baseline` parameter of `flush_to_document` is currently unused; kept for
//!   future CRDT merge path.
//!
//! ## Evals
//! - flush_to_document_applies_patch: document with `output` component → new streamed content replaces old
//! - flush_replaces_exchange_in_stream_mode: exchange component with existing text → old text absent after flush
//! - flush_cumulative_does_not_duplicate: two flushes with "Hello" then "Hello world" → exactly one "Hello" in doc
//! - flush_preserves_other_components: flush to `output` → `status` component untouched
//! - stream_loop_processes_chunks: three chunks ending with is_final → session_id captured, final text in doc
//! - stream_loop_captures_partial_checkpoints: non-final streamed text persists
//!   in the partial checkpoint ledger without waiting for final closeout
//! - stream_loop_empty_chunks: empty-text chunks → session_id None, doc unchanged
//! - build_prompt_first_submit: no resume → prompt says "starting a session", no diff included
//! - build_prompt_resume: resume ID set → prompt includes diff and full document
//! - build_prompt_mentions_patch_blocks: prompt always references `patch:exchange` format
//! - build_prompt_resume_lists_required_response_targets: resumed prompt with two user request
//!   blocks → prompt includes the ordered turn-completeness section
//! - stream_loop_thinking_to_separate_component: thinking_target="log" → thinking in log, response in exchange
//! - stream_loop_thinking_interleaved: no thinking_target → `<details>` block in target component
//! - stream_loop_no_thinking_skips_thinking_blocks: thinking_cfg=None → thinking content absent from doc
//! - mode_validation_rejects_non_crdt: non-CRDT document → error containing "expected crdt"
//! - content_ours_exchange_no_duplicate: replace mode applied → user prompt appears exactly once
//! - content_ours_exchange_duplicates_without_replace: append mode (no override) → prompt duplicated (regression baseline)

use anyhow::{Context, Result};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::agent::streaming::{StreamChunk, StreamingAgent};
use crate::{agent, config::Config, crdt, diff, frontmatter, git, repair, snapshot, template};

/// Run the stream command: stream agent output to document in real-time.
///
/// `lint_override` mirrors the `--lint=off|warn|strict` flag on
/// `agent-doc write` / `agent-doc finalize`. The lint gate runs after
/// the final flush has merged the streamed response into the document
/// and before the final git commit so malformed directives fail the
/// stream cycle closed instead of being committed. Mode resolution
/// precedence: CLI > frontmatter `agent_doc_lint_dialect` > workspace
/// `.agent-doc/config.toml` `[lint] dialect` > default (`warn`).
pub fn run(
    file: &Path,
    interval_ms: u64,
    agent_name: Option<&str>,
    model: Option<&str>,
    no_git: bool,
    config: &Config,
    lint_override: Option<crate::lint_gate::LintCliMode>,
) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    // Validate mode — requires CRDT write strategy
    let raw_content = std::fs::read_to_string(file)?;
    let (fm, _body) = frontmatter::parse(&raw_content)?;
    let resolved = fm.resolve_mode();
    if !resolved.is_crdt() {
        anyhow::bail!(
            "document write mode is {:?}, expected crdt. Use `agent-doc mode {} --set stream` first.",
            resolved.write,
            file.display()
        );
    }

    // Read stream config from frontmatter (overrides CLI args where set)
    let stream_config = fm.stream_config.clone().unwrap_or_default();
    let interval = stream_config.interval.unwrap_or(interval_ms);
    let target = stream_config.target.as_deref().unwrap_or("exchange");
    let thinking_enabled = stream_config.thinking.unwrap_or(false);
    let thinking_target = stream_config.thinking_target.clone();

    eprintln!(
        "[stream] starting for {} (interval: {}ms, target: {}, thinking: {}{})",
        file.display(),
        interval,
        target,
        thinking_enabled,
        thinking_target
            .as_ref()
            .map(|t| format!(", thinking_target: {}", t))
            .unwrap_or_default()
    );

    // Compute diff
    let the_diff = match diff::compute(file)? {
        Some(d) => {
            eprintln!("[stream] diff computed ({} bytes)", d.len());
            d
        }
        None => {
            eprintln!(
                "[stream] Nothing changed since last run for {}",
                file.display()
            );
            return Ok(());
        }
    };

    // Ensure session UUID
    let (content_original, _session_id) = frontmatter::ensure_session(&raw_content)?;
    if content_original != raw_content {
        std::fs::write(file, &content_original)?;
    }
    let (fm, _body) = frontmatter::parse(&content_original)?;

    // Resolve agent
    let agent_name = agent_name
        .or(fm.agent.as_deref())
        .or(config.default_agent.as_deref())
        .unwrap_or("claude");
    let agent_config = config.agents.get(agent_name);

    // Expand frontmatter env vars (applied to the streaming agent's child process).
    // Values may contain $(passage ...) — expanded in document order via a single
    // shell invocation so later values can reference earlier keys.
    let expanded_env = if fm.env.is_empty() {
        Vec::new()
    } else {
        match crate::env::expand_values(&fm.env) {
            Ok(e) => e,
            Err(e) => {
                eprintln!(
                    "[stream] env expansion failed: {} — continuing without env",
                    e
                );
                Vec::new()
            }
        }
    };

    // Resolve streaming agent
    let streaming_agent = resolve_streaming(agent_name, agent_config, expanded_env, file, &fm)?;

    // Build prompt
    let session_accretion = crate::session_accretion::inspect(file).ok();
    let prompt = build_prompt(
        file,
        &fm,
        &the_diff,
        &content_original,
        session_accretion.as_ref(),
    );

    // Pre-commit user changes
    if !no_git && let Err(e) = git::commit(file) {
        eprintln!("[stream] git commit skipped: {}", e);
    }

    eprintln!("[stream] Submitting to {} (streaming)...", agent_name);

    // Send to streaming agent
    let fork = fm.resume.is_none();
    let harness = agent_doc_core::model_tier::harness_key_for_agent_name(agent_name);
    let resolved_model = model
        .or(fm.resolve_harness_model(&harness))
        .map(|m| agent_doc_core::model_tier::canonical_model_name(m, &harness, &config.model));
    let chunks = streaming_agent.send_streaming(
        &prompt,
        fm.resume.as_deref(),
        fork,
        resolved_model.as_deref(),
    )?;

    // Build thinking config
    let thinking_cfg = if thinking_enabled {
        Some(ThinkingConfig {
            target: thinking_target,
        })
    } else {
        None
    };

    // Run the write-back loop
    let result = stream_loop(
        file,
        chunks,
        interval,
        target,
        &content_original,
        thinking_cfg.as_ref(),
    )?;

    // Update resume ID if we got a session_id
    if let Some(ref sid) = result.session_id {
        let current = std::fs::read_to_string(file)?;
        let updated = frontmatter::set_resume_id(&current, sid)?;
        crate::write::atomic_write_pub(file, &updated)?;
        snapshot::save(file, &updated)?;
    }

    // Lint gate: runs on the merged document AFTER the final flush /
    // resume-id write / snapshot save and BEFORE the final git commit so
    // malformed directives fail the stream cycle closed instead of being
    // committed. Mirrors the gate position in
    // `write::run_command` (Phase 3b.1). Mode resolution precedence:
    // CLI override > frontmatter `agent_doc_lint_dialect` > workspace
    // `.agent-doc/config.toml` `[lint] dialect` > default (`warn`).
    crate::lint_gate::run(file, lint_override)?;

    // Final git commit
    if !no_git && let Err(e) = git::commit(file) {
        eprintln!("[stream] git commit skipped: {}", e);
    }

    // #a010: Fire post_write hook for cross-session coordination. The append /
    // template write paths already do this in write.rs after each successful
    // IPC/disk write, but stream mode used its own flush loop and never fired
    // post_write. Missing this hook broke supervisors that trigger follow-up
    // actions (gutter refresh, downstream doc sync) when a stream completes.
    let session_id = result.session_id.clone().unwrap_or_default();
    crate::hooks::fire_post_write(file, &session_id, 1);
    crate::hooks::fire_doc_event(file, "post_write");

    eprintln!("[stream] Stream complete for {}", file.display());
    Ok(())
}

/// Configuration for chain-of-thought streaming.
struct ThinkingConfig {
    /// If set, route thinking to this component. If None, interleave in target.
    target: Option<String>,
}

/// Result of a completed stream.
struct StreamResult {
    session_id: Option<String>,
}

/// The core write-back loop: accumulates chunks, periodically merges into document.
fn stream_loop(
    file: &Path,
    chunks: Box<dyn Iterator<Item = Result<StreamChunk>>>,
    interval_ms: u64,
    target: &str,
    baseline: &str,
    thinking_cfg: Option<&ThinkingConfig>,
) -> Result<StreamResult> {
    let buffer = Arc::new(Mutex::new(String::new()));
    let thinking_buffer = Arc::new(Mutex::new(String::new()));
    let (done_tx, done_rx) = mpsc::channel::<()>();

    // Timer thread: periodically flush buffer to document
    let timer_buffer = Arc::clone(&buffer);
    let timer_thinking = Arc::clone(&thinking_buffer);
    let file_path = file.to_path_buf();
    let target_name = target.to_string();
    let baseline_copy = baseline.to_string();
    let timer_interval = Duration::from_millis(interval_ms);
    let thinking_target = thinking_cfg.and_then(|c| c.target.clone());
    let has_thinking = thinking_cfg.is_some();
    let timer_flushed_final = Arc::new(AtomicBool::new(false));
    let timer_flushed_final_clone = Arc::clone(&timer_flushed_final);

    let timer_handle = std::thread::spawn(move || {
        let mut last_written = String::new();
        let mut last_thinking = String::new();
        loop {
            let is_done = match done_rx.recv_timeout(timer_interval) {
                Ok(()) => true,
                Err(mpsc::RecvTimeoutError::Timeout) => false,
                Err(mpsc::RecvTimeoutError::Disconnected) => true,
            };

            let text = timer_buffer.lock().unwrap().clone();
            let thinking_text = if has_thinking {
                timer_thinking.lock().unwrap().clone()
            } else {
                String::new()
            };

            // Flush response text
            if text != last_written && !text.is_empty() {
                match flush_to_document(&file_path, &text, &target_name, &baseline_copy) {
                    Ok(()) => {
                        last_written = text;
                        if is_done {
                            timer_flushed_final_clone.store(true, Ordering::Release);
                        } else {
                            eprint!(".");
                        }
                    }
                    Err(e) => {
                        let label = if is_done { "final flush" } else { "flush" };
                        // Redact in case the error body interpolates document
                        // content (e.g. a failed atomic write echoing a chunk).
                        eprintln!(
                            "[stream] {} error: {}",
                            label,
                            crate::secret_redact::redact(&e.to_string())
                        );
                    }
                }
            }

            // Flush thinking text to separate component (or skip if interleaved)
            if has_thinking && thinking_text != last_thinking && !thinking_text.is_empty() {
                if let Some(ref tt) = thinking_target {
                    match flush_to_document(&file_path, &thinking_text, tt, &baseline_copy) {
                        Ok(()) => {
                            last_thinking = thinking_text;
                        }
                        Err(e) => {
                            eprintln!(
                                "[stream] thinking flush error: {}",
                                crate::secret_redact::redact(&e.to_string())
                            );
                        }
                    }
                } else {
                    // Thinking interleaved — already part of text buffer
                    last_thinking = thinking_text;
                }
            }

            if is_done {
                return;
            }
        }
    });

    // Main thread: consume chunks and accumulate in buffer
    let mut session_id = None;
    let mut chunk_count = 0;
    let mut checkpoint_writer = crate::capture::PartialCheckpointWriter::new(file);

    for chunk_result in chunks {
        let chunk = chunk_result.context("stream chunk error")?;

        // Accumulate thinking first (before text, so interleaving can use it)
        if let Some(ref thinking) = chunk.thinking
            && thinking_cfg.is_some()
        {
            let mut tbuf = thinking_buffer.lock().unwrap();
            *tbuf = thinking.clone();
        }

        if !chunk.text.is_empty() {
            let checkpoint_text = {
                let mut buf = buffer.lock().unwrap();
                // For assistant messages, the text is cumulative (full text so far)
                // For result messages, it's the final full text
                if thinking_cfg.is_some() && thinking_cfg.unwrap().target.is_none() {
                    // Interleave: prepend thinking as collapsible details
                    let thinking_text = thinking_buffer.lock().unwrap().clone();
                    if !thinking_text.is_empty() {
                        *buf = format!(
                            "<details>\n<summary>Thinking</summary>\n\n{}\n</details>\n\n{}",
                            thinking_text, chunk.text
                        );
                    } else {
                        *buf = chunk.text.clone();
                    }
                } else {
                    *buf = chunk.text.clone();
                }
                buf.clone()
            };
            if !chunk.is_final {
                checkpoint_writer.maybe_checkpoint(&checkpoint_text)?;
            }
            chunk_count += 1;
        }

        if chunk.is_final {
            session_id = chunk.session_id;
            break;
        }
    }

    // Signal timer thread to do final flush
    let _ = done_tx.send(());
    timer_handle
        .join()
        .map_err(|_| anyhow::anyhow!("timer thread panicked"))?;

    eprintln!("\n[stream] Received {} chunks", chunk_count);

    // Final flush: ensure the complete response is written
    let final_text = buffer.lock().unwrap().clone();
    if !final_text.is_empty() {
        // Save as pending for crash recovery
        repair::save_pending(file, &final_text)?;

        // Gate: skip if timer thread already flushed the final text (avoids IPC double-append)
        if !timer_flushed_final.load(Ordering::Acquire) {
            flush_to_document(file, &final_text, target, baseline)?;
        }

        // Flush final thinking if routed to separate component
        if let Some(cfg) = thinking_cfg
            && let Some(ref tt) = cfg.target
        {
            let final_thinking = thinking_buffer.lock().unwrap().clone();
            if !final_thinking.is_empty() && !timer_flushed_final.load(Ordering::Acquire) {
                flush_to_document(file, &final_thinking, tt, baseline)?;
            }
        }

        // Compute content_ours: baseline + final response patches (without user edits).
        // Save this as snapshot so the next diff detects any concurrent user edits.
        // Must use replace mode for the target — stream buffer is cumulative, not incremental.
        let content_ours = {
            let patch = format!(
                "<!-- patch:{} -->\n{}\n<!-- /patch:{} -->",
                target, final_text, target
            );
            let (patches, unmatched) = crate::template::parse_patches(&patch).unwrap_or_default();
            let mut mode_overrides = std::collections::HashMap::new();
            mode_overrides.insert(target.to_string(), "replace".to_string());
            crate::template::apply_patches_with_overrides(
                baseline,
                &patches,
                &unmatched,
                file,
                &mode_overrides,
            )
            .unwrap_or_else(|_| std::fs::read_to_string(file).unwrap_or_default())
        };
        snapshot::save(file, &content_ours)?;
        let doc = crdt::CrdtDoc::from_text(&content_ours);
        snapshot::save_crdt(file, &doc.encode_state())?;

        repair::clear_pending(file)?;
    }

    Ok(StreamResult { session_id })
}

/// Flush accumulated text to the document via template patch.
///
/// Wraps the text in a patch block targeting the specified component,
/// applies template patches, and uses advisory locking for safe writes.
///
/// Stream mode uses **replace** mode for the target component regardless of
/// the component's configured mode (e.g., exchange defaults to append). This is
/// because the stream buffer is cumulative — each flush contains the full text
/// so far, not just the delta.
///
/// When a JetBrains/VS Code plugin is active (`.agent-doc/patches/` directory
/// exists), attempts IPC first to avoid "externally modified" dialogs. Falls
/// back to direct write on IPC timeout.
pub fn flush_to_document(
    file: &Path,
    text: &str,
    target: &str,
    _baseline: &str,
) -> Result<()> {
    // Build a patch block targeting the component
    let patch_response = format!(
        "<!-- patch:{} -->\n{}\n<!-- /patch:{} -->\n",
        target, text, target
    );

    let (patches, unmatched) =
        template::parse_patches(&patch_response).context("failed to parse patch blocks")?;

    // Try IPC first — if plugin is active, it applies patches via Document API
    // (no "externally modified" dialog, cursor preserved, undo preserved)
    if crate::write::try_ipc(file, &patches, &unmatched, None, None, None, None, None)?.success {
        return Ok(());
    }

    // IPC not available or timed out — fall back to direct write

    // Force replace mode for stream target — buffer is cumulative, not incremental
    let mut mode_overrides = std::collections::HashMap::new();
    mode_overrides.insert(target.to_string(), "replace".to_string());

    // Acquire lock
    let lock_path = snapshot::lock_path_for(file)?;
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)?;
    fs2::FileExt::lock_exclusive(&lock_file)?;

    // Read current file content
    let content_current = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;

    // Apply patches with replace override for stream target
    let content_patched = template::apply_patches_with_overrides(
        &content_current,
        &patches,
        &unmatched,
        file,
        &mode_overrides,
    )
    .context("failed to apply template patches")?;

    // Write atomically
    crate::write::atomic_write_pub(file, &content_patched)?;

    drop(lock_file);
    Ok(())
}

/// Build the prompt for the streaming agent.
fn build_prompt(
    file: &Path,
    fm: &frontmatter::Frontmatter,
    the_diff: &str,
    content: &str,
    session_accretion: Option<&crate::session_accretion::SessionAccretionReport>,
) -> String {
    let prompt_bearing_changes = diff::format_prompt_bearing_changes(the_diff)
        .map(|section| format!("\n\n{}\n", section))
        .unwrap_or_default();
    let active_format_requirements =
        crate::prompt_contract::format_active_format_requirements(content)
            .map(|section| format!("\n\n{}\n", section))
            .unwrap_or_default();
    let document_section =
        crate::prompt_context::build_document_section(file, the_diff, content, session_accretion);
    if fm.resume.is_some() {
        format!(
            "The user edited the session document. Here is the diff since the last run:\n\n\
             <diff>\n{}\n</diff>\n\n\
             {}{}\
             {}\
             Respond to the user's new content. Write your response in markdown.\n\
             Format your response as patch blocks targeting document components.\n\
             Example: <!-- patch:exchange -->\\nYour response\\n<!-- /patch:exchange -->",
            the_diff, prompt_bearing_changes, active_format_requirements, document_section
        )
    } else {
        format!(
            "The user is starting a session document. Here is the full document:\n\n\
             {}\
             <document>\n{}\n</document>\n\n\
             Respond to the user's content. Write your response in markdown.\n\
             Format your response as patch blocks targeting document components.\n\
             Example: <!-- patch:exchange -->\\nYour response\\n<!-- /patch:exchange -->",
            active_format_requirements, content
        )
    }
}

/// Resolve a streaming agent backend by name.
fn resolve_streaming(
    name: &str,
    config: Option<&crate::config::AgentConfig>,
    env: Vec<(String, Option<String>)>,
    file: &Path,
    fm: &crate::frontmatter::Frontmatter,
) -> Result<Box<dyn StreamingAgent>> {
    let Some(agent) = agent::resolve_streaming_for_file(name, config, env, file, fm)? else {
        anyhow::bail!(
            "Unknown streaming agent backend: {} (only claude and codex support streaming)",
            name
        );
    };
    Ok(agent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::streaming::StreamChunk;

    /// Create a mock chunk iterator from a list of chunks.
    fn mock_chunks(chunks: Vec<StreamChunk>) -> Box<dyn Iterator<Item = Result<StreamChunk>>> {
        Box::new(chunks.into_iter().map(Ok))
    }

    #[test]
    fn flush_to_document_applies_patch() {
        let dir = tempfile::TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("locks")).unwrap();

        // Use "output" component (default mode: replace) instead of "exchange" (default: append)
        let doc = dir.path().join("test.md");
        let content = "---\nagent_doc_mode: stream\n---\n\n# Test\n\n<!-- agent:output -->\nOld content\n<!-- /agent:output -->\n";
        std::fs::write(&doc, content).unwrap();

        flush_to_document(&doc, "New streamed content", "output", content).unwrap();

        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(
            result.contains("New streamed content"),
            "patched content missing: {}",
            result
        );
        assert!(
            !result.contains("Old content"),
            "old content should be replaced: {}",
            result
        );
    }

    #[test]
    fn flush_replaces_exchange_in_stream_mode() {
        let dir = tempfile::TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("locks")).unwrap();

        let doc = dir.path().join("test.md");
        let content = "---\nagent_doc_mode: stream\n---\n\n<!-- agent:exchange -->\nExisting\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, content).unwrap();

        // Stream flush uses replace mode — cumulative buffer replaces existing content
        flush_to_document(&doc, "New content", "exchange", content).unwrap();

        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(
            !result.contains("Existing"),
            "stream flush should replace, not append: {}",
            result
        );
        assert!(
            result.contains("New content"),
            "new content should be present"
        );
    }

    #[test]
    fn flush_cumulative_does_not_duplicate() {
        let dir = tempfile::TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("locks")).unwrap();

        let doc = dir.path().join("test.md");
        let content = "---\nagent_doc_mode: stream\n---\n\n<!-- agent:exchange -->\nUser prompt\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, content).unwrap();

        // First flush: partial response
        flush_to_document(&doc, "Hello", "exchange", content).unwrap();
        // Second flush: cumulative (full text so far)
        flush_to_document(&doc, "Hello world", "exchange", content).unwrap();

        let result = std::fs::read_to_string(&doc).unwrap();
        // Should contain "Hello world" exactly once, not "Hello\nHello world"
        assert!(
            result.contains("Hello world"),
            "cumulative text should be present: {}",
            result
        );
        let hello_count = result.matches("Hello").count();
        assert_eq!(
            hello_count, 1,
            "Hello should appear exactly once (replace, not append): {}",
            result
        );
    }

    #[test]
    fn flush_preserves_other_components() {
        let dir = tempfile::TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("locks")).unwrap();

        let doc = dir.path().join("test.md");
        let content = "---\nagent_doc_mode: stream\n---\n\n# Test\n\n<!-- agent:status -->\nStatus line\n<!-- /agent:status -->\n\n<!-- agent:output -->\nOld\n<!-- /agent:output -->\n";
        std::fs::write(&doc, content).unwrap();

        flush_to_document(&doc, "New content", "output", content).unwrap();

        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(
            result.contains("Status line"),
            "status component should be preserved"
        );
        assert!(result.contains("New content"), "output should be updated");
    }

    #[test]
    fn stream_loop_processes_chunks() {
        let dir = tempfile::TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("locks")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("pending")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();

        let doc = dir.path().join("test.md");
        let content = "---\nagent_doc_mode: stream\n---\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, content).unwrap();

        let chunks = mock_chunks(vec![
            StreamChunk {
                text: "Hello".to_string(),
                thinking: None,
                is_final: false,
                session_id: None,
            },
            StreamChunk {
                text: "Hello world".to_string(),
                thinking: None,
                is_final: false,
                session_id: None,
            },
            StreamChunk {
                text: "Hello world!".to_string(),
                thinking: None,
                is_final: true,
                session_id: Some("sess-1".to_string()),
            },
        ]);

        let result = stream_loop(&doc, chunks, 100, "exchange", content, None).unwrap();
        assert_eq!(result.session_id.as_deref(), Some("sess-1"));

        let final_doc = std::fs::read_to_string(&doc).unwrap();
        assert!(
            final_doc.contains("Hello world!"),
            "final text should be in document: {}",
            final_doc
        );
    }

    #[test]
    fn stream_loop_captures_partial_checkpoints() {
        let dir = tempfile::TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("locks")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("pending")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("captures")).unwrap();

        let doc = dir.path().join("test.md");
        let content = "---\nagent_doc_session: sid\nagent_doc_mode: stream\n---\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, content).unwrap();
        crate::snapshot::save(&doc, content).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();

        let chunks = mock_chunks(vec![
            StreamChunk {
                text: "Partial checkpoint".to_string(),
                thinking: None,
                is_final: false,
                session_id: None,
            },
            StreamChunk {
                text: "Final response".to_string(),
                thinking: None,
                is_final: true,
                session_id: Some("sess-1".to_string()),
            },
        ]);

        stream_loop(&doc, chunks, 100, "exchange", content, None).unwrap();

        let checkpoint = crate::capture::latest_partial_checkpoint(&doc)
            .unwrap()
            .expect("partial checkpoint should be persisted");
        assert_eq!(checkpoint.response_body, "Partial checkpoint");
        assert_eq!(checkpoint.checkpoint_count, 1);
        let active_capture = crate::capture::load_active(&doc).unwrap().unwrap();
        assert_eq!(active_capture.response_body, "Final response");
    }

    #[test]
    fn stream_loop_empty_chunks() {
        let dir = tempfile::TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("locks")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("pending")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();

        let doc = dir.path().join("test.md");
        let content = "---\nagent_doc_mode: stream\n---\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, content).unwrap();

        let chunks = mock_chunks(vec![
            StreamChunk {
                text: String::new(),
                thinking: None,
                is_final: false,
                session_id: None,
            },
            StreamChunk {
                text: String::new(),
                thinking: None,
                is_final: true,
                session_id: None,
            },
        ]);

        let result = stream_loop(&doc, chunks, 100, "exchange", content, None).unwrap();
        assert!(result.session_id.is_none());
    }

    #[test]
    fn build_prompt_first_submit() {
        let fm = frontmatter::Frontmatter {
            resume: None,
            ..Default::default()
        };
        let prompt = build_prompt(
            Path::new("session.md"),
            &fm,
            "diff here",
            "doc content",
            None,
        );
        assert!(prompt.contains("starting a session"));
        assert!(prompt.contains("doc content"));
        assert!(!prompt.contains("diff here")); // no diff for first submit
    }

    #[test]
    fn build_prompt_resume() {
        let fm = frontmatter::Frontmatter {
            resume: Some("sess-123".to_string()),
            ..Default::default()
        };
        let prompt = build_prompt(
            Path::new("session.md"),
            &fm,
            "diff here",
            "doc content",
            None,
        );
        assert!(prompt.contains("edited the session document"));
        assert!(prompt.contains("diff here"));
        assert!(prompt.contains("doc content"));
    }

    #[test]
    fn build_prompt_mentions_patch_blocks() {
        let fm = frontmatter::Frontmatter::default();
        let prompt = build_prompt(Path::new("session.md"), &fm, "diff", "content", None);
        assert!(
            prompt.contains("patch:exchange"),
            "prompt should mention patch block format"
        );
    }

    #[test]
    fn build_prompt_resume_lists_required_response_targets() {
        let fm = frontmatter::Frontmatter {
            resume: Some("sess-123".to_string()),
            ..Default::default()
        };
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,5 @@\n\
           ctx\n\
           +❯ First unresolved question?\n\
           +\n\
           +❯ Second unresolved question?\n";
        let prompt = build_prompt(Path::new("session.md"), &fm, diff, "doc content", None);
        assert!(prompt.contains("User-authored prompt-bearing changes (oldest first):"));
        assert!(prompt.contains("Do not stop at the newest question"));
        assert!(prompt.contains("kind=\"prompt_target\""));
        assert!(prompt.contains("❯ First unresolved question?"));
        assert!(prompt.contains("❯ Second unresolved question?"));
    }

    #[test]
    fn build_prompt_carries_forward_active_format_requirements() {
        let fm = frontmatter::Frontmatter {
            resume: Some("sess-123".to_string()),
            ..Default::default()
        };
        let doc = concat!(
            "❯ Please organize the backlog into a 2-level list. ",
            "Place the urgent-security matters at the top. ",
            "Use a numeric list where appropriate.\n",
            "### Re: backlog organization — gpt-5\n",
            "Done.\n",
        );

        let prompt = build_prompt(Path::new("session.md"), &fm, "diff", doc, None);
        assert!(
            prompt.contains(
                "Active document-level formatting / structure requirements carried forward"
            )
        );
        assert!(prompt.contains(
            "Please organize the backlog into a 2-level list. Place the urgent-security matters at the top. Use a numeric list where appropriate."
        ));
    }

    #[test]
    fn build_prompt_uses_bounded_context_pack_for_warn_level_prompt_targets() {
        let fm = frontmatter::Frontmatter {
            resume: Some("sess-123".to_string()),
            ..Default::default()
        };
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,4 @@\n\
           Done.\n\
           +do [#ctxpack]. spec-test-build-install-commit-push\n\
           <!-- /agent:exchange -->\n";
        let doc = concat!(
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "Compacted earlier turns.\n\n",
            "### Re: older topic — gpt-5\n\n",
            "Older response body.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#ctxpack] Add bounded context pack\n",
            "<!-- /agent:backlog -->\n",
        );
        let report = crate::session_accretion::SessionAccretionReport {
            level: crate::session_accretion::SessionAccretionLevel::Warn,
            reasons: vec!["document hit 2 no-op closeouts in the last 30 minutes".to_string()],
            ..Default::default()
        };

        let prompt = build_prompt(Path::new("session.md"), &fm, diff, doc, Some(&report));
        assert!(prompt.contains("<response_context level=\"warn\">"));
        assert!(prompt.contains("<recent_exchange_turns limit=\"2\">"));
        assert!(!prompt.contains("<document>\n## Exchange"));
    }

    #[test]
    fn stream_loop_thinking_to_separate_component() {
        let dir = tempfile::TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("locks")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("pending")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();

        let doc = dir.path().join("test.md");
        let content = "---\nagent_doc_mode: stream\n---\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n\n<!-- agent:log -->\n<!-- /agent:log -->\n";
        std::fs::write(&doc, content).unwrap();

        let chunks = mock_chunks(vec![
            StreamChunk {
                text: "".to_string(),
                thinking: Some("Let me think...".to_string()),
                is_final: false,
                session_id: None,
            },
            StreamChunk {
                text: "The answer is 42.".to_string(),
                thinking: Some("Let me think... Yes, 42.".to_string()),
                is_final: true,
                session_id: Some("sess-2".to_string()),
            },
        ]);

        let thinking_cfg = ThinkingConfig {
            target: Some("log".to_string()),
        };
        let result =
            stream_loop(&doc, chunks, 100, "exchange", content, Some(&thinking_cfg)).unwrap();
        assert_eq!(result.session_id.as_deref(), Some("sess-2"));

        let final_doc = std::fs::read_to_string(&doc).unwrap();
        assert!(
            final_doc.contains("The answer is 42."),
            "response text should be in exchange: {}",
            final_doc
        );
        assert!(
            final_doc.contains("Yes, 42."),
            "thinking should be in log: {}",
            final_doc
        );
    }

    #[test]
    fn stream_loop_thinking_interleaved() {
        let dir = tempfile::TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("locks")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("pending")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();

        let doc = dir.path().join("test.md");
        let content =
            "---\nagent_doc_mode: stream\n---\n\n<!-- agent:output -->\n<!-- /agent:output -->\n";
        std::fs::write(&doc, content).unwrap();

        let chunks = mock_chunks(vec![StreamChunk {
            text: "The answer.".to_string(),
            thinking: Some("Reasoning here.".to_string()),
            is_final: true,
            session_id: None,
        }]);

        let thinking_cfg = ThinkingConfig { target: None }; // interleave
        let result =
            stream_loop(&doc, chunks, 100, "output", content, Some(&thinking_cfg)).unwrap();
        assert!(result.session_id.is_none());

        let final_doc = std::fs::read_to_string(&doc).unwrap();
        assert!(
            final_doc.contains("<details>"),
            "interleaved thinking should use details tag: {}",
            final_doc
        );
        assert!(
            final_doc.contains("Reasoning here."),
            "thinking content should be present: {}",
            final_doc
        );
        assert!(
            final_doc.contains("The answer."),
            "response text should be present: {}",
            final_doc
        );
    }

    #[test]
    fn stream_loop_no_thinking_skips_thinking_blocks() {
        let dir = tempfile::TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("locks")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("pending")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();

        let doc = dir.path().join("test.md");
        let content =
            "---\nagent_doc_mode: stream\n---\n\n<!-- agent:output -->\n<!-- /agent:output -->\n";
        std::fs::write(&doc, content).unwrap();

        let chunks = mock_chunks(vec![StreamChunk {
            text: "Response only.".to_string(),
            thinking: Some("Secret thoughts.".to_string()),
            is_final: true,
            session_id: None,
        }]);

        // No thinking config — thinking should be ignored
        let result = stream_loop(&doc, chunks, 100, "output", content, None).unwrap();
        assert!(result.session_id.is_none());

        let final_doc = std::fs::read_to_string(&doc).unwrap();
        assert!(
            final_doc.contains("Response only."),
            "response should be present: {}",
            final_doc
        );
        assert!(
            !final_doc.contains("Secret thoughts"),
            "thinking should NOT appear: {}",
            final_doc
        );
    }

    #[test]
    fn mode_validation_rejects_non_crdt() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        std::fs::write(
            &doc,
            "---\nagent_doc_format: template\nagent_doc_write: merge\n---\n\nBody\n",
        )
        .unwrap();

        let config = Config::default();
        let err = run(&doc, 2000, None, None, true, &config, None).unwrap_err();
        assert!(err.to_string().contains("expected crdt"), "error: {}", err);
    }

    /// Regression test: content_ours computation for exchange component must use
    /// replace mode, not the default append mode. Without replace mode, the
    /// patch content gets appended to the baseline's exchange content, duplicating
    /// the user's prompt text.
    #[test]
    fn content_ours_exchange_no_duplicate() {
        let baseline = "\
---
agent_doc_format: template
agent_doc_write: crdt
---

<!-- agent:exchange -->
commit and push all rappstack packages and sites.
publish briantakita.me
<!-- /agent:exchange -->
";
        let target = "exchange";
        let final_text = "\
commit and push all rappstack packages and sites.
publish briantakita.me

### Re: commit and push

Done — all packages pushed.";

        // Build patch and apply WITH replace mode (the fix)
        let patch = format!(
            "<!-- patch:{} -->\n{}\n<!-- /patch:{} -->",
            target, final_text, target
        );
        let (patches, unmatched) = crate::template::parse_patches(&patch).unwrap();
        let mut mode_overrides = std::collections::HashMap::new();
        mode_overrides.insert(target.to_string(), "replace".to_string());
        let file = std::path::Path::new("test.md");
        let content_ours = crate::template::apply_patches_with_overrides(
            baseline,
            &patches,
            &unmatched,
            file,
            &mode_overrides,
        )
        .unwrap();

        // User prompt should appear exactly once
        assert_eq!(
            content_ours
                .matches("commit and push all rappstack packages and sites.")
                .count(),
            1,
            "User prompt duplicated in content_ours:\n{}",
            content_ours
        );
        assert_eq!(
            content_ours.matches("publish briantakita.me").count(),
            1,
            "User prompt duplicated in content_ours:\n{}",
            content_ours
        );
        // Agent response should be present
        assert!(content_ours.contains("Done — all packages pushed."));
    }

    /// Verify that dedup_exchange_adjacent_lines prevents echo-duplication even
    /// without replace mode override. Previously this test documented the bug
    /// (count == 2); now dedup makes append mode safe (count == 1).
    #[test]
    fn content_ours_exchange_duplicates_without_replace() {
        let baseline = "\
---
agent_doc_format: template
agent_doc_write: crdt
---

<!-- agent:exchange -->
user prompt here
<!-- /agent:exchange -->
";
        let target = "exchange";
        let final_text = "user prompt here\n\nAgent response.";

        let patch = format!(
            "<!-- patch:{} -->\n{}\n<!-- /patch:{} -->",
            target, final_text, target
        );
        let (patches, unmatched) = crate::template::parse_patches(&patch).unwrap();
        let file = std::path::Path::new("test.md");

        // dedup_exchange_adjacent_lines now removes the echo duplication in append mode
        let content_no_override =
            crate::template::apply_patches(baseline, &patches, &unmatched, file).unwrap();

        // "user prompt here" should appear exactly once — dedup prevents echo duplication
        assert_eq!(
            content_no_override.matches("user prompt here").count(),
            1,
            "Expected dedup to prevent echo duplication in append mode:\n{}",
            content_no_override
        );
        assert!(content_no_override.contains("Agent response."));
    }

    // ---- Lint gate integration tests (p6adfstream) --------------------
    //
    // The stream subcommand calls `crate::lint_gate::run` after the final
    // flush merges the response into the document and before the final
    // git commit. These tests exercise the same gate against the
    // post-flush document state to prove malformed streamed responses
    // fail the cycle closed.

    /// Write a session doc in the post-flush state (what the file looks
    /// like after `stream_loop()` completes) and assert that the lint
    /// gate — the same call `stream::run` now performs before the final
    /// git commit — fails closed on a malformed `<!-- agent:done archive
    /// PATH -->` directive missing the `=` between `archive` and the
    /// path. Error message format must match the shared gate output the
    /// `lint_gate` unit tests already pin for `write --commit`.
    #[test]
    fn stream_response_with_malformed_directive_blocks_lint_gate() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        // Simulate the post-stream-flush document state directly to avoid
        // triggering unrelated boundary-marker churn from the template
        // patch path during test setup.
        let post_flush = "---\nagent_doc_session: sid\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
            <!-- agent:exchange -->\n\
            user prompt\n\n\
            ### Re: prompt — gpt-5\n\n\
            Done.\n\
            <!-- /agent:exchange -->\n\
            <!-- agent:done archive tasks/x.done.md -->\n\
            <!-- /agent:done -->\n";
        std::fs::write(&doc, post_flush).unwrap();

        let err = crate::lint_gate::run(&doc, None)
            .expect_err("stream's lint gate must reject malformed directive");
        let msg = format!("{}", err);
        assert!(
            msg.contains("agent-doc/malformed-attr"),
            "expected agent-doc/malformed-attr rule, got: {msg}"
        );
        assert!(
            msg.contains("INTERRUPTED"),
            "expected INTERRUPTED prefix matching shared gate format, got: {msg}"
        );
    }

    /// Clean post-stream document state → the lint gate must pass so the
    /// stream cycle proceeds to the final commit.
    #[test]
    fn stream_response_clean_passes_lint_gate() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        let post_flush = "---\nagent_doc_session: sid\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
            <!-- agent:exchange -->\n\
            user prompt\n\n\
            ### Re: prompt — gpt-5\n\n\
            Done.\n\
            <!-- /agent:exchange -->\n";
        std::fs::write(&doc, post_flush).unwrap();

        crate::lint_gate::run(&doc, None).expect("clean stream output must pass lint gate");
    }

    /// `--lint=off` (via the CLI override) must bypass the gate even
    /// when the streamed response contains a malformed directive — same
    /// override semantics as `agent-doc write --commit --lint=off`.
    #[test]
    fn stream_lint_off_bypasses_malformed_directive() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        let post_flush = "---\nagent_doc_session: sid\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
            <!-- agent:exchange -->\n\
            user prompt\n\n\
            ### Re: prompt — gpt-5\n\n\
            Done.\n\
            <!-- /agent:exchange -->\n\
            <!-- agent:done archive tasks/x.done.md -->\n\
            <!-- /agent:done -->\n";
        std::fs::write(&doc, post_flush).unwrap();

        crate::lint_gate::run(&doc, Some(crate::lint_gate::LintCliMode::Off))
            .expect("--lint=off must bypass the stream lint gate even with malformed directive");
    }

    /// Verify the stream subcommand's `run` signature accepts the
    /// `--lint` CLI override slot. Uses a non-CRDT doc so the run exits
    /// early on mode validation before lint gate would fire — the
    /// purpose of the test is to prove the signature is wired and
    /// matches `main.rs`'s Commands::Stream forwarding.
    #[test]
    fn stream_cli_accepts_lint_flag_via_signature() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        std::fs::write(
            &doc,
            "---\nagent_doc_format: template\nagent_doc_write: merge\n---\n\nBody\n",
        )
        .unwrap();

        let config = Config::default();
        let err = run(
            &doc,
            2000,
            None,
            None,
            true,
            &config,
            Some(crate::lint_gate::LintCliMode::Strict),
        )
        .unwrap_err();
        assert!(err.to_string().contains("expected crdt"), "error: {}", err);
    }
}
