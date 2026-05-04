//! # Module: session_actor
//!
//! ## Spec
//! - Freezes the phase-1 single-owner session-actor contract without introducing
//!   the durable actor store from later phases.
//! - Ownership generations are monotonic per document session and start at `1`.
//! - A new authoritative generation is recorded whenever a new `agent-doc start`
//!   session begins or an existing document session is rebound to a different pane.
//! - Generation inference remains backward-compatible with legacy logs that only
//!   have `session_start` lines and no explicit generation markers.
//! - Ownership-transition events render stable machine-readable fields:
//!   `caller`, `reason`, `prior_generation`, `new_generation`, `old_pane`,
//!   `new_pane`, `old_window`, `new_window`.
//!
//! ## Agentic Contracts
//! - This module is read/write-log instrumentation only; it does not own
//!   authoritative session state yet.
//! - Log parsing must tolerate unrelated session-log events and malformed lines.
//! - Legacy logs with at least one `session_start` but no explicit generation
//!   markers infer the current generation from the number of starts.
//!
//! ## Evals
//! - `infer_latest_generation_counts_legacy_session_starts`
//! - `infer_latest_generation_prefers_explicit_generation_markers`
//! - `format_transition_event_uses_stable_placeholders`

use anyhow::Result;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnershipGeneration {
    pub prior_generation: u64,
    pub new_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnershipTransitionEvent<'a> {
    pub caller: &'a str,
    pub reason: &'a str,
    pub prior_generation: u64,
    pub new_generation: u64,
    pub old_pane: Option<&'a str>,
    pub new_pane: &'a str,
    pub old_window: Option<&'a str>,
    pub new_window: Option<&'a str>,
}

fn log_path(file: &Path, session_id: &str) -> Result<Option<PathBuf>> {
    let canonical = match file.canonicalize() {
        Ok(path) => path,
        Err(_) => return Ok(None),
    };
    let Some(root) = crate::snapshot::find_project_root(&canonical) else {
        return Ok(None);
    };
    Ok(Some(
        root.join(".agent-doc/logs")
            .join(format!("{session_id}.log")),
    ))
}

fn extract_event(raw_line: &str) -> &str {
    raw_line
        .split_once("] ")
        .map(|(_, event)| event)
        .unwrap_or(raw_line)
        .trim()
}

fn parse_u64_field(event: &str, name: &str) -> Option<u64> {
    event.split_whitespace().find_map(|part| {
        part.strip_prefix(name)
            .and_then(|value| value.parse::<u64>().ok())
    })
}

fn render_field<'a>(value: Option<&'a str>, fallback: &'a str) -> &'a str {
    value.filter(|value| !value.is_empty()).unwrap_or(fallback)
}

pub fn infer_latest_generation_from_content(content: &str) -> u64 {
    let mut latest_explicit = 0u64;
    let mut legacy_session_starts = 0u64;

    for raw_line in content.lines() {
        let event = extract_event(raw_line);
        if event.is_empty() {
            continue;
        }
        if event.starts_with("session_start ") {
            legacy_session_starts += 1;
            if let Some(generation) = parse_u64_field(event, "generation=") {
                latest_explicit = latest_explicit.max(generation);
            }
            continue;
        }
        if event.starts_with("ownership_transition ")
            && let Some(generation) = parse_u64_field(event, "new_generation=")
        {
            latest_explicit = latest_explicit.max(generation);
        }
    }

    if latest_explicit > 0 {
        latest_explicit
    } else {
        legacy_session_starts
    }
}

pub fn infer_latest_generation(file: &Path, session_id: &str) -> Result<u64> {
    let Some(path) = log_path(file, session_id)? else {
        return Ok(0);
    };
    let Some(content) = crate::fs_util::read_optional_text(&path)? else {
        return Ok(0);
    };
    Ok(infer_latest_generation_from_content(&content))
}

pub fn next_generation(file: &Path, session_id: &str) -> Result<OwnershipGeneration> {
    let prior_generation = infer_latest_generation(file, session_id)?;
    Ok(OwnershipGeneration {
        prior_generation,
        new_generation: prior_generation.saturating_add(1).max(1),
    })
}

pub fn format_transition_event(event: OwnershipTransitionEvent<'_>) -> String {
    format!(
        "ownership_transition caller={} reason={} prior_generation={} new_generation={} old_pane={} new_pane={} old_window={} new_window={}",
        event.caller,
        event.reason,
        event.prior_generation,
        event.new_generation,
        render_field(event.old_pane, "none"),
        render_field(Some(event.new_pane), "none"),
        render_field(event.old_window, "none"),
        render_field(event.new_window, "unknown"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infer_latest_generation_counts_legacy_session_starts() {
        let content = concat!(
            "[1] session_start file=doc.md pane=%41 session=session-1\n",
            "[2] codex_start mode=fresh restart_count=0\n",
            "[10] session_start file=doc.md pane=%52 session=session-1\n",
        );

        assert_eq!(infer_latest_generation_from_content(content), 2);
    }

    #[test]
    fn infer_latest_generation_prefers_explicit_generation_markers() {
        let content = concat!(
            "[1] ownership_transition caller=start reason=session_start prior_generation=0 new_generation=1 old_pane=none new_pane=%41 old_window=none new_window=@1\n",
            "[2] session_start file=doc.md pane=%41 session=session-1 generation=1\n",
            "[10] ownership_transition caller=route reason=registry_rebind prior_generation=1 new_generation=2 old_pane=%41 new_pane=%52 old_window=@1 new_window=@2\n",
        );

        assert_eq!(infer_latest_generation_from_content(content), 2);
    }

    #[test]
    fn format_transition_event_uses_stable_placeholders() {
        let rendered = format_transition_event(OwnershipTransitionEvent {
            caller: "start",
            reason: "session_start",
            prior_generation: 0,
            new_generation: 1,
            old_pane: None,
            new_pane: "%41",
            old_window: None,
            new_window: Some("@1"),
        });

        assert_eq!(
            rendered,
            "ownership_transition caller=start reason=session_start prior_generation=0 new_generation=1 old_pane=none new_pane=%41 old_window=none new_window=@1"
        );
    }
}
