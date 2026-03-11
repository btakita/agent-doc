//! `agent-doc stream` — Stream agent output to document in real-time.
//!
//! Reads the document, computes a diff, sends to a streaming agent backend,
//! and periodically writes accumulated output back to the document using
//! CRDT merge for conflict-free concurrent editing.
//!
//! Write-back loop:
//! ```text
//! [Agent chunks] → [Buffer] → [Timer: 2s] → [Lock → Read → CRDT merge → Write → Unlock]
//! [User edits]  → [File]   → [Detected on next tick via content comparison]
//! ```

use anyhow::{Context, Result};
use std::path::Path;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::agent::streaming::{StreamChunk, StreamingAgent};
use crate::{agent, config::Config, crdt, diff, frontmatter, git, recover, snapshot, template};

/// Run the stream command: stream agent output to document in real-time.
pub fn run(
    file: &Path,
    interval_ms: u64,
    agent_name: Option<&str>,
    model: Option<&str>,
    no_git: bool,
    config: &Config,
) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    // Validate mode
    let raw_content = std::fs::read_to_string(file)?;
    let (fm, _body) = frontmatter::parse(&raw_content)?;
    if fm.mode.as_deref() != Some("stream") {
        anyhow::bail!(
            "document mode is {:?}, expected \"stream\". Use `agent-doc mode {} --set stream` first.",
            fm.mode.as_deref().unwrap_or("append"),
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
        thinking_target.as_ref().map(|t| format!(", thinking_target: {}", t)).unwrap_or_default()
    );

    // Compute diff
    let the_diff = match diff::compute(file)? {
        Some(d) => {
            eprintln!("[stream] diff computed ({} bytes)", d.len());
            d
        }
        None => {
            eprintln!("[stream] Nothing changed since last submit for {}", file.display());
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

    // Resolve streaming agent
    let streaming_agent = resolve_streaming(agent_name, agent_config)?;

    // Build prompt
    let prompt = build_prompt(&fm, &the_diff, &content_original);

    // Pre-commit user changes
    if !no_git
        && let Err(e) = git::commit(file)
    {
        eprintln!("[stream] git commit skipped: {}", e);
    }

    eprintln!("[stream] Submitting to {} (streaming)...", agent_name);

    // Send to streaming agent
    let fork = fm.resume.is_none();
    let model = model.or(fm.model.as_deref());
    let chunks = streaming_agent.send_streaming(&prompt, fm.resume.as_deref(), fork, model)?;

    // Build thinking config
    let thinking_cfg = if thinking_enabled {
        Some(ThinkingConfig {
            target: thinking_target,
        })
    } else {
        None
    };

    // Run the write-back loop
    let result = stream_loop(file, chunks, interval, target, &content_original, thinking_cfg.as_ref())?;

    // Update resume ID if we got a session_id
    if let Some(ref sid) = result.session_id {
        let current = std::fs::read_to_string(file)?;
        let updated = frontmatter::set_resume_id(&current, sid)?;
        crate::write::atomic_write_pub(file, &updated)?;
        snapshot::save(file, &updated)?;
    }

    // Final git commit
    if !no_git
        && let Err(e) = git::commit(file)
    {
        eprintln!("[stream] git commit skipped: {}", e);
    }

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
                        if !is_done {
                            eprint!(".");
                        }
                    }
                    Err(e) => {
                        let label = if is_done { "final flush" } else { "flush" };
                        eprintln!("[stream] {} error: {}", label, e);
                    }
                }
            }

            // Flush thinking text to separate component (or skip if interleaved)
            if has_thinking
                && thinking_text != last_thinking
                && !thinking_text.is_empty()
            {
                if let Some(ref tt) = thinking_target {
                    match flush_to_document(&file_path, &thinking_text, tt, &baseline_copy) {
                        Ok(()) => {
                            last_thinking = thinking_text;
                        }
                        Err(e) => {
                            eprintln!("[stream] thinking flush error: {}", e);
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
            let mut buf = buffer.lock().unwrap();
            // For assistant messages, the text is cumulative (full text so far)
            // For result messages, it's the final full text
            if thinking_cfg.is_some()
                && thinking_cfg.unwrap().target.is_none()
            {
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
            chunk_count += 1;
        }

        if chunk.is_final {
            session_id = chunk.session_id;
            break;
        }
    }

    // Signal timer thread to do final flush
    let _ = done_tx.send(());
    timer_handle.join().map_err(|_| anyhow::anyhow!("timer thread panicked"))?;

    eprintln!("\n[stream] Received {} chunks", chunk_count);

    // Final flush: ensure the complete response is written
    let final_text = buffer.lock().unwrap().clone();
    if !final_text.is_empty() {
        // Save as pending for crash recovery
        recover::save_pending(file, &final_text)?;

        flush_to_document(file, &final_text, target, baseline)?;

        // Flush final thinking if routed to separate component
        if let Some(cfg) = thinking_cfg
            && let Some(ref tt) = cfg.target
        {
            let final_thinking = thinking_buffer.lock().unwrap().clone();
            if !final_thinking.is_empty() {
                flush_to_document(file, &final_thinking, tt, baseline)?;
            }
        }

        // Compute content_ours: baseline + final response patches (without user edits).
        // Save this as snapshot so the next diff detects any concurrent user edits.
        let content_ours = {
            let patch = format!("<!-- patch:{} -->\n{}\n<!-- /patch:{} -->", target, final_text, target);
            let (patches, unmatched) = crate::template::parse_patches(&patch)
                .unwrap_or_default();
            crate::template::apply_patches(baseline, &patches, &unmatched, file)
                .unwrap_or_else(|_| std::fs::read_to_string(file).unwrap_or_default())
        };
        snapshot::save(file, &content_ours)?;
        let doc = crdt::CrdtDoc::from_text(&content_ours);
        snapshot::save_crdt(file, &doc.encode_state())?;

        recover::clear_pending(file)?;
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
pub(crate) fn flush_to_document(
    file: &Path,
    text: &str,
    target: &str,
    _baseline: &str,
) -> Result<()> {
    // Build a patch block targeting the component
    let patch_response = format!("<!-- patch:{} -->\n{}\n<!-- /patch:{} -->\n", target, text, target);

    let (patches, unmatched) = template::parse_patches(&patch_response)
        .context("failed to parse patch blocks")?;

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
        &content_current, &patches, &unmatched, file, &mode_overrides,
    ).context("failed to apply template patches")?;

    // Write atomically
    crate::write::atomic_write_pub(file, &content_patched)?;

    drop(lock_file);
    Ok(())
}

/// Build the prompt for the streaming agent.
fn build_prompt(fm: &frontmatter::Frontmatter, the_diff: &str, content: &str) -> String {
    if fm.resume.is_some() {
        format!(
            "The user edited the session document. Here is the diff since the last submit:\n\n\
             <diff>\n{}\n</diff>\n\n\
             The full document is now:\n\n\
             <document>\n{}\n</document>\n\n\
             Respond to the user's new content. Write your response in markdown.\n\
             Format your response as patch blocks targeting document components.\n\
             Example: <!-- patch:exchange -->\\nYour response\\n<!-- /patch:exchange -->",
            the_diff, content
        )
    } else {
        format!(
            "The user is starting a session document. Here is the full document:\n\n\
             <document>\n{}\n</document>\n\n\
             Respond to the user's content. Write your response in markdown.\n\
             Format your response as patch blocks targeting document components.\n\
             Example: <!-- patch:exchange -->\\nYour response\\n<!-- /patch:exchange -->",
            content
        )
    }
}

/// Resolve a streaming agent backend by name.
fn resolve_streaming(
    name: &str,
    config: Option<&crate::config::AgentConfig>,
) -> Result<Box<dyn StreamingAgent>> {
    let (cmd, args) = match config {
        Some(ac) => (Some(ac.command.clone()), Some(ac.args.clone())),
        None => (None, None),
    };
    match name {
        "claude" => Ok(Box::new(agent::claude::Claude::new(cmd, args))),
        other => {
            if config.is_some() {
                Ok(Box::new(agent::claude::Claude::new(cmd, args)))
            } else {
                anyhow::bail!("Unknown streaming agent backend: {} (only claude supports streaming)", other)
            }
        }
    }
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
        assert!(result.contains("New streamed content"), "patched content missing: {}", result);
        assert!(!result.contains("Old content"), "old content should be replaced: {}", result);
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
        assert!(!result.contains("Existing"), "stream flush should replace, not append: {}", result);
        assert!(result.contains("New content"), "new content should be present");
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
        assert!(result.contains("Hello world"), "cumulative text should be present: {}", result);
        let hello_count = result.matches("Hello").count();
        assert_eq!(hello_count, 1, "Hello should appear exactly once (replace, not append): {}", result);
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
        assert!(result.contains("Status line"), "status component should be preserved");
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
            StreamChunk { text: "Hello".to_string(), thinking: None, is_final: false, session_id: None },
            StreamChunk { text: "Hello world".to_string(), thinking: None, is_final: false, session_id: None },
            StreamChunk { text: "Hello world!".to_string(), thinking: None, is_final: true, session_id: Some("sess-1".to_string()) },
        ]);

        let result = stream_loop(&doc, chunks, 100, "exchange", content, None).unwrap();
        assert_eq!(result.session_id.as_deref(), Some("sess-1"));

        let final_doc = std::fs::read_to_string(&doc).unwrap();
        assert!(final_doc.contains("Hello world!"), "final text should be in document: {}", final_doc);
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
            StreamChunk { text: String::new(), thinking: None, is_final: false, session_id: None },
            StreamChunk { text: String::new(), thinking: None, is_final: true, session_id: None },
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
        let prompt = build_prompt(&fm, "diff here", "doc content");
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
        let prompt = build_prompt(&fm, "diff here", "doc content");
        assert!(prompt.contains("edited the session document"));
        assert!(prompt.contains("diff here"));
        assert!(prompt.contains("doc content"));
    }

    #[test]
    fn build_prompt_mentions_patch_blocks() {
        let fm = frontmatter::Frontmatter::default();
        let prompt = build_prompt(&fm, "diff", "content");
        assert!(prompt.contains("patch:exchange"), "prompt should mention patch block format");
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
        let result = stream_loop(&doc, chunks, 100, "exchange", content, Some(&thinking_cfg)).unwrap();
        assert_eq!(result.session_id.as_deref(), Some("sess-2"));

        let final_doc = std::fs::read_to_string(&doc).unwrap();
        assert!(final_doc.contains("The answer is 42."), "response text should be in exchange: {}", final_doc);
        assert!(final_doc.contains("Yes, 42."), "thinking should be in log: {}", final_doc);
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
        let content = "---\nagent_doc_mode: stream\n---\n\n<!-- agent:output -->\n<!-- /agent:output -->\n";
        std::fs::write(&doc, content).unwrap();

        let chunks = mock_chunks(vec![
            StreamChunk {
                text: "The answer.".to_string(),
                thinking: Some("Reasoning here.".to_string()),
                is_final: true,
                session_id: None,
            },
        ]);

        let thinking_cfg = ThinkingConfig { target: None }; // interleave
        let result = stream_loop(&doc, chunks, 100, "output", content, Some(&thinking_cfg)).unwrap();
        assert!(result.session_id.is_none());

        let final_doc = std::fs::read_to_string(&doc).unwrap();
        assert!(final_doc.contains("<details>"), "interleaved thinking should use details tag: {}", final_doc);
        assert!(final_doc.contains("Reasoning here."), "thinking content should be present: {}", final_doc);
        assert!(final_doc.contains("The answer."), "response text should be present: {}", final_doc);
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
        let content = "---\nagent_doc_mode: stream\n---\n\n<!-- agent:output -->\n<!-- /agent:output -->\n";
        std::fs::write(&doc, content).unwrap();

        let chunks = mock_chunks(vec![
            StreamChunk {
                text: "Response only.".to_string(),
                thinking: Some("Secret thoughts.".to_string()),
                is_final: true,
                session_id: None,
            },
        ]);

        // No thinking config — thinking should be ignored
        let result = stream_loop(&doc, chunks, 100, "output", content, None).unwrap();
        assert!(result.session_id.is_none());

        let final_doc = std::fs::read_to_string(&doc).unwrap();
        assert!(final_doc.contains("Response only."), "response should be present: {}", final_doc);
        assert!(!final_doc.contains("Secret thoughts"), "thinking should NOT appear: {}", final_doc);
    }

    #[test]
    fn mode_validation_rejects_non_stream() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        std::fs::write(&doc, "---\nagent_doc_mode: template\n---\n\nBody\n").unwrap();

        let config = Config::default();
        let err = run(&doc, 2000, None, None, true, &config).unwrap_err();
        assert!(err.to_string().contains("expected \"stream\""), "error: {}", err);
    }
}
