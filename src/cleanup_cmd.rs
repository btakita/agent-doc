//! # Module: cleanup_cmd
//!
//! Umbrella command that handles document cleanup via callback orchestration.
//!
//! - Writes a callback request for exchange compaction and pending pruning
//! - Polls for a Claude session response with a configurable timeout
//! - Falls back to `claude --print` subagent summarization on timeout
//! - Applies the result to the document

use anyhow::{Context, Result};
use std::path::Path;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::callback;

/// Main cleanup entry point.
pub fn run(
    file: &Path,
    timeout_secs: u64,
    poll_interval_ms: u64,
    fallback_model: &str,
) -> Result<()> {
    // Step 1: Write callback request
    eprintln!("[cleanup] writing callback request for {}", file.display());
    let _request = callback::create_request(
        file,
        &["compact", "prune-pending", "summary"],
        None,
        timeout_secs + 10, // TTL slightly longer than timeout
    )?;

    eprintln!("[cleanup] polling for response (timeout: {}s, interval: {}ms)",
        timeout_secs, poll_interval_ms);

    // Step 2: Poll for response
    let deadline = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + timeout_secs;

    let mut response = None;
    while SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        < deadline
    {
        thread::sleep(Duration::from_millis(poll_interval_ms));
        if let Some(resp) = callback::read_response(file)? {
            response = Some(resp);
            break;
        }
    }

    // Step 3: Fallback to subagent if no response
    let summary = if let Some(resp) = response {
        callback::delete_response(file)?;
        eprintln!("[cleanup] received callback response from Claude session");
        eprintln!("[cleanup] {}", resp.summary);
        resp.summary.clone()
    } else {
        eprintln!("[cleanup] no response from session, spawning fallback agent");
        spawn_fallback_agent(file, fallback_model)?
    };

    eprintln!("[cleanup] done: {}", summary);
    Ok(())
}

/// Spawn a claude --print subprocess for summarization fallback.
fn spawn_fallback_agent(file: &Path, model: &str) -> Result<String> {
    eprintln!("[cleanup] spawning fallback agent: claude --print --model {}", model);

    let content = std::fs::read_to_string(file)
        .context("could not read document for fallback summarization")?;

    // Truncate to avoid exceeding context limits on the subagent
    let max_chars = 30_000;
    let truncated = if content.len() > max_chars {
        &content[..max_chars]
    } else {
        &content
    };

    let prompt = format!(
        "Summarize the following document exchange into a concise paragraph. \
         Focus on decisions made, work completed, and open items. \
         Output ONLY the summary text, nothing else.\n\n---\n\n{}",
        truncated
    );

    let output = std::process::Command::new("claude")
        .args(["--print", "--model", model, "-p"])
        .arg(&prompt)
        .output()
        .context("failed to spawn claude --print for fallback summarization")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("claude --print failed: {}", stderr);
    }

    let summary = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(summary)
}
