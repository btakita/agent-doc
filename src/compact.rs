//! # Module: compact
//!
//! ## Spec
//! - Reduces document size by archiving old content to `.agent-doc/archives/<hash>-<timestamp>.md`.
//! - **Inline/append mode:** parses `## User` / `## Assistant` exchange pairs from the document
//!   body; archives all but the `keep` most-recent complete pairs; rebuilds the document with an
//!   archive summary line and a trailing `## User` prompt block.
//!   - Trailing `## User` blocks without an assistant reply are never counted or archived.
//!   - Code blocks containing `## User` / `## Assistant` headings are not treated as section
//!     boundaries.
//! - **Template/stream mode (full compact):** when `keep` is `None`, archives the full content of
//!   a named component (default `exchange`) and replaces it with a summary marker; optionally
//!   accepts a custom `--message`.
//! - **Template/stream mode (partial compact):** when `keep` is `Some(N)`, archives all but the
//!   last N `### Re:` topic sections; rebuilds the component with an archive summary + kept topics.
//!   - The preamble (content before the first `### Re:` heading) is always preserved; use
//!     `--message` to replace it with a fresh session summary.
//!   - If section count ≤ N, logs to stderr and exits `Ok(())` without modifying the document.
//!   - If the document uses CRDT write strategy, the CRDT state is compacted (GC tombstones)
//!     before the component replacement.
//! - Archive filenames are derived from the snapshot hash + a UTC timestamp computed without
//!   the `chrono` crate.
//! - Document replacement prefers the editor IPC full-content path when an official plugin is
//!   present, so active editor buffers are updated through editor APIs. If IPC is unavailable or
//!   times out, writes fall back to atomic temp-file rename. The snapshot is updated after each
//!   successful replacement.
//! - `commit: true` closes out compaction through the binary-owned `agent-doc commit` path and
//!   verifies the VCS refresh signal when that channel exists.
//!
//! ## Agentic Contracts
//! - `run(file, keep, component_name, message, tag, commit) -> Result<()>` — entry point;
//!   dispatches to component compact (template/stream) or exchange compact (inline) based on
//!   frontmatter mode.
//! - `keep: None` in template mode → full compact (archive all).
//! - `keep: Some(N)` in template mode → partial compact (archive all but last N `### Re:` sections).
//! - `keep: None` in inline mode → uses default of 2.
//! - If `exchanges.len() <= keep` in inline mode, logs to stderr and exits `Ok(())` without
//!   modifying the document.
//! - Archive path is always under `.agent-doc/archives/` relative to the project root (found by
//!   walking up to the directory containing `.agent-doc/`).
//! - Returns `Err` if the file does not exist, the named component is not found (template mode),
//!   or any I/O operation fails.
//! - `tag: Some(name)` creates a lightweight git tag at HEAD before compaction (pre-compact checkpoint).
//!   `tag: None` auto-generates `agent-doc/<doc-name>/pre-compact-N` where N is the next ordinal.
//! - `message: Some("-")` reads from stdin (standard Unix convention for CLIs that accept
//!   file-or-string arguments).
//! - `commit: true` uses `git::commit_with_outcome` after a successful mutation. If the commit
//!   path reports that a VCS refresh signal target existed but writing it failed, compact fails
//!   closed instead of silently accepting the closeout.
//! - `apply_compacted_document` is the single replacement boundary used by inline, full component,
//!   and partial component compaction.
//!
//! ## Evals
//! - parse_basic: two complete exchanges + trailing User → 2 exchanges parsed, trailing skipped
//! - parse_code_blocks: `## User` inside fenced block → not treated as section boundary
//! - keep_threshold: exchange count ≤ keep → no-op, `Ok(())` returned
//! - archive_format: archived content contains `archived_from: compact`, session ID, exchange text
//! - compacted_format: result has archive summary line, kept exchanges, trailing `## User\n\n`
//! - timestamp_format: `chrono_timestamp()` → 15-char string matching `YYYYMMDD-HHMMSS`
//! - component_archive_format: template-mode archive contains component name, session ID, content
//! - partial_compact_keep_threshold: sections ≤ keep → no-op
//! - partial_compact_archive_format: archive contains preamble + archived sections
//! - partial_compact_result_format: result has archive pointer + preamble (or message) + kept sections
//! - exchange_compact_default_summary_includes_archived_content_digest: default full compact summary
//!   includes an archive pointer and compacted exchange digest
//! - compact_with_commit_writes_vcs_refresh_signal: `--commit` closeout creates an `agent-doc`
//!   commit and updates `.agent-doc/patches/vcs-refresh.signal` when that refresh channel exists
//! - component_compact_delivers_active_document_replacement_via_editor_ipc: compact sends a
//!   fullContent IPC patch and commits the editor-applied result instead of rewriting the open file
//! - message_dash_reads_stdin: `--message -` reads from stdin instead of using literal "-"

use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

use crate::{archive_index, component, frontmatter, snapshot};

/// A parsed exchange pair (User prompt + Assistant response).
#[derive(Debug)]
struct Exchange {
    /// The user's content (without the `## User` heading)
    user: String,
    /// The assistant's content (without the `## Assistant` heading)
    assistant: String,
}

const COMPACT_SUMMARY_ITEM_LIMIT: usize = 3;
const COMPACT_SUMMARY_TEXT_LIMIT: usize = 120;

/// Run the compact command.
///
/// - `keep`: number of recent exchanges/topics to keep.
///   - Inline mode: `None` defaults to 2.
///   - Template mode: `None` archives all (full compact); `Some(N)` archives all but last N `### Re:` sections.
/// - `component_name`: targets a specific component in template/stream mode.
/// - `message`: summary marker text (default: auto-generated).
/// - `tag`: git tag to create at HEAD before compaction. `None` auto-generates
///   `agent-doc/<doc-name>/pre-compact-N`. Pass `Some("skip")` to skip tagging entirely.
pub fn run(
    file: &Path,
    keep: Option<usize>,
    component_name: Option<&str>,
    message: Option<&str>,
    tag: Option<&str>,
    commit: bool,
) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    let stdin_message;
    let message = if message == Some("-") {
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("failed to read --message from stdin")?;
        stdin_message = buf;
        Some(stdin_message.as_str())
    } else {
        message
    };

    // Create a pre-compact git tag at HEAD before modifying the document.
    // Skipped if tag == Some("skip").
    if tag != Some("skip")
        && let Err(e) = create_pre_compact_tag(file, tag)
    {
        eprintln!("[compact] Warning: could not create pre-compact tag: {}", e);
    }

    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;

    let (fm, body) = frontmatter::parse(&content)?;

    let resolved = fm.resolve_mode();
    if resolved.is_template() {
        // Compact CRDT state if CRDT write mode
        if resolved.is_crdt()
            && let Ok(Some(crdt_state)) = snapshot::load_crdt(file)
        {
            let compacted_crdt = crate::crdt::compact(&crdt_state)?;
            snapshot::save_crdt(file, &compacted_crdt)?;
            eprintln!(
                "[compact] CRDT state compacted: {} → {} bytes",
                crdt_state.len(),
                compacted_crdt.len()
            );
        }

        let target = component_name.unwrap_or("exchange");
        let is_crdt = resolved.is_crdt();
        match keep {
            Some(n) => run_component_compact_partial(file, &content, target, n, message, is_crdt),
            None => run_component_compact(file, &content, target, message, is_crdt),
        }?;
    } else {
        let keep_n = keep.unwrap_or(2);

        // Parse exchanges from the body
        let exchanges = parse_exchanges(body);

        if exchanges.len() <= keep_n {
            eprintln!(
                "[compact] Only {} exchange(s) found, keeping all (threshold: {})",
                exchanges.len(),
                keep_n
            );
            return Ok(());
        }

        let to_archive = &exchanges[..exchanges.len() - keep_n];
        let to_keep = &exchanges[exchanges.len() - keep_n..];

        // Build archive content
        let archive_content = build_archive(file, &content, to_archive);

        // Save archive
        let archive_path = save_archive(file, &archive_content)?;

        // Build compacted document
        let mut compacted =
            build_compacted(&content, body, to_keep, &archive_path, to_archive.len());
        if let Some(reconciled) =
            crate::status_cmd::reconcile_top_backlog_status_content(&compacted)?
        {
            compacted = reconciled;
        }

        apply_compacted_document(file, &compacted, &compacted, &content, false)?;

        eprintln!(
            "[compact] Archived {} exchange(s) to {}",
            to_archive.len(),
            archive_path.display()
        );
        eprintln!(
            "[compact] {} exchange(s) remain in {}",
            to_keep.len(),
            file.display()
        );
    }

    if component_name.is_none() || component_name == Some("exchange") {
        crate::session_accretion::record_recent_exchange_compaction(file)?;
    }

    if commit {
        let updated = std::fs::read_to_string(file)
            .with_context(|| format!("failed to re-read {} after compact", file.display()))?;
        if updated != content {
            closeout_compact_with_commit(file)?;
        }
    }

    Ok(())
}

fn closeout_compact_with_commit(file: &Path) -> Result<()> {
    let outcome = crate::git::commit_with_outcome(file)?;
    if outcome.did_commit && outcome.vcs_refresh_signaled == Some(false) {
        anyhow::bail!(
            "compact closeout committed {} but failed to write vcs-refresh.signal",
            file.display()
        );
    }
    eprintln!(
        "[compact] note: --commit persists only the compacted document state now in HEAD; any later console explanation still needs its own `agent-doc finalize` or `agent-doc write --commit` cycle to land in `exchange`"
    );
    Ok(())
}

fn apply_compacted_document(
    file: &Path,
    compacted: &str,
    snapshot_content: &str,
    source_content: &str,
    refresh_crdt: bool,
) -> Result<()> {
    let mut applied_via_editor = false;
    match crate::write::try_ipc_full_content_operator_mutation_from_source(
        file,
        compacted,
        source_content,
    ) {
        Ok(true) => {
            applied_via_editor = true;
            eprintln!(
                "[compact] delivered compacted document through editor IPC for {}",
                file.display()
            );
        }
        Ok(false) => {}
        Err(err) => {
            eprintln!(
                "[compact] warning: editor IPC compact delivery failed for {}: {}; falling back to direct disk write",
                file.display(),
                err
            );
        }
    }

    if !applied_via_editor {
        crate::write::atomic_write_if_current_pub(
            file,
            compacted,
            source_content,
            "compact_full_content_fallback",
        )?;
    }

    snapshot::save(file, snapshot_content)?;

    if refresh_crdt {
        let new_crdt = crate::crdt::CrdtDoc::from_text(compacted).encode_state();
        snapshot::save_crdt(file, &new_crdt)?;
        eprintln!("[compact] CRDT state refreshed from post-compact content");
    }

    Ok(())
}

/// Compact a named component in a template/stream-mode document.
///
/// Archives the component content and replaces it with a summary marker.
/// Single atomic write — no intermediate state.
fn run_component_compact(
    file: &Path,
    content: &str,
    target: &str,
    message: Option<&str>,
    is_crdt: bool,
) -> Result<()> {
    let components = component::parse(content)?;
    let comp = components
        .iter()
        .find(|c| c.name == target)
        .ok_or_else(|| anyhow::anyhow!("component '{}' not found in document", target))?;

    let old_content = comp.content(content);
    let trimmed = old_content.trim();

    if trimmed.is_empty() {
        eprintln!("[compact] Component '{}' is already empty", target);
        return Ok(());
    }

    // Archive old content
    let archive_path = save_archive(
        file,
        &build_component_archive(file, content, target, old_content),
    )?;

    // Build summary marker
    let summary = match message {
        Some(msg) => format!("{}\n", msg),
        None if target == "exchange" => build_exchange_compact_summary(content, &archive_path),
        None => format!(
            "*Compacted. Content archived to `{}`*\n",
            archive_path.display()
        ),
    };

    let compacted = comp.replace_content(content, &summary);
    let mut compacted = crate::template::repair_conversation_tail_outside_exchange(&compacted)?
        .unwrap_or(compacted);
    if let Some(reconciled) = crate::status_cmd::reconcile_top_backlog_status_content(&compacted)? {
        compacted = reconciled;
    }
    apply_compacted_document(file, &compacted, &compacted, content, is_crdt)?;

    let line_count = old_content.lines().count();
    eprintln!(
        "[compact] Archived {} lines from component '{}' to {}",
        line_count,
        target,
        archive_path.display()
    );

    Ok(())
}

/// Partial compact a named component in a template/stream-mode document.
///
/// Archives all but the last `keep` `### Re:` topic sections; rebuilds the component
/// with an archive pointer + preamble (or `message`) + kept sections.
fn run_component_compact_partial(
    file: &Path,
    content: &str,
    target: &str,
    keep: usize,
    message: Option<&str>,
    is_crdt: bool,
) -> Result<()> {
    let components = component::parse(content)?;
    let comp = components
        .iter()
        .find(|c| c.name == target)
        .ok_or_else(|| anyhow::anyhow!("component '{}' not found in document", target))?;

    let old_content = comp.content(content);

    let parsed = parse_topic_sections_with_tail(old_content);
    let preamble = parsed.preamble;
    let sections = parsed.sections;
    let trailing = parsed.trailing;

    if sections.len() <= keep {
        eprintln!(
            "[compact] Only {} topic section(s) found, keeping all (threshold: {})",
            sections.len(),
            keep
        );
        return Ok(());
    }

    let to_archive = &sections[..sections.len() - keep];
    let to_keep = &sections[sections.len() - keep..];

    // Build archive: preamble + archived sections
    let mut archive_body = String::new();
    if !preamble.trim().is_empty() {
        archive_body.push_str(preamble.trim_end());
        archive_body.push_str("\n\n");
    }
    for section in to_archive {
        archive_body.push_str(section.trim_end());
        archive_body.push_str("\n\n");
    }

    let archive_path = save_archive(
        file,
        &build_component_archive(file, content, target, &archive_body),
    )?;

    // Build new component content. The committed snapshot intentionally omits
    // unresolved trailing prompt text after the boundary so compact+commit can
    // reduce history without marking that prompt as answered.
    let mut base_new_content = String::new();

    // Preamble: use --message if provided, else keep original preamble
    match message {
        Some(msg) => {
            base_new_content.push_str(msg.trim_end());
            base_new_content.push('\n');
        }
        None => {
            if !preamble.trim().is_empty() {
                base_new_content.push_str(preamble.trim_end());
                base_new_content.push('\n');
            }
        }
    }

    // Archive pointer
    base_new_content.push_str(&format!(
        "\n*{} earlier topic(s) archived to `{}`*\n",
        to_archive.len(),
        archive_path.display()
    ));

    // Kept sections
    for section in to_keep {
        base_new_content.push('\n');
        base_new_content.push_str(section.trim_end());
        base_new_content.push('\n');
    }
    let mut new_content = base_new_content.clone();
    if !trailing.trim().is_empty() {
        if !new_content.ends_with('\n') {
            new_content.push('\n');
        }
        new_content.push_str(trailing.trim_end());
        new_content.push('\n');
    }

    let compacted = comp.replace_content(content, &new_content);
    let mut compacted = crate::template::repair_conversation_tail_outside_exchange(&compacted)?
        .unwrap_or(compacted);
    if let Some(reconciled) = crate::status_cmd::reconcile_top_backlog_status_content(&compacted)? {
        compacted = reconciled;
    }
    let mut snapshot_compacted = if trailing.trim().is_empty() {
        compacted.clone()
    } else {
        let snapshot_content = comp.replace_content(content, &base_new_content);
        crate::template::repair_conversation_tail_outside_exchange(&snapshot_content)?
            .unwrap_or(snapshot_content)
    };
    if let Some(reconciled) =
        crate::status_cmd::reconcile_top_backlog_status_content(&snapshot_compacted)?
    {
        snapshot_compacted = reconciled;
    }
    apply_compacted_document(file, &compacted, &snapshot_compacted, content, is_crdt)?;

    eprintln!(
        "[compact] Archived {} topic(s) from component '{}' to {}",
        to_archive.len(),
        target,
        archive_path.display()
    );
    eprintln!(
        "[compact] {} topic(s) remain in {}",
        to_keep.len(),
        file.display()
    );

    Ok(())
}

/// Parse a component's content into a preamble and a list of `### Re:` topic sections.
///
/// The preamble is everything before the first `### Re:` heading.
/// Each section is the `### Re:` heading line + its content until the next `### Re:` (or end).
/// Boundary markers (`<!-- agent:boundary:... -->`) are stripped from the final section
/// so they are not archived.
pub fn parse_topic_sections(content: &str) -> (String, Vec<String>) {
    let parsed = parse_topic_sections_with_tail(content);
    (parsed.preamble, parsed.sections)
}

#[derive(Debug, Default)]
struct TopicSections {
    preamble: String,
    sections: Vec<String>,
    trailing: String,
}

fn parse_topic_sections_with_tail(content: &str) -> TopicSections {
    let mut preamble = String::new();
    let mut sections: Vec<String> = Vec::new();
    let mut current: Option<String> = None;
    let mut found_first = false;
    let mut after_boundary = false;
    let mut trailing = String::new();

    for line in content.lines() {
        // Strip boundary markers — they are managed by the binary, not archived
        if line.starts_with("<!-- agent:boundary:") {
            after_boundary = true;
            continue;
        }
        if after_boundary {
            trailing.push_str(line);
            trailing.push('\n');
            continue;
        }

        if line.starts_with("### Re:") || line.starts_with("#### Re:") || line.starts_with("## Re:")
        {
            if let Some(prev) = current.take() {
                sections.push(prev);
            }
            found_first = true;
            current = Some(format!("{}\n", line));
        } else if found_first {
            let section = current.get_or_insert_with(String::new);
            section.push_str(line);
            section.push('\n');
        } else {
            preamble.push_str(line);
            preamble.push('\n');
        }
    }

    if let Some(last) = current {
        sections.push(last);
    }

    TopicSections {
        preamble,
        sections,
        trailing,
    }
}

/// Build archive content from a component.
fn build_component_archive(
    doc: &Path,
    original: &str,
    component_name: &str,
    content: &str,
) -> String {
    let mut archive = String::new();

    archive.push_str("---\n");
    archive.push_str("archived_from: compact\n");
    archive.push_str(&format!("archived_at: {}\n", chrono_timestamp()));
    archive.push_str(&format!("component: {}\n", component_name));
    if let Ok(document) = archive_document_value(doc) {
        archive.push_str(&format!("document: {}\n", document));
    }

    if let Ok((fm, _)) = frontmatter::parse(original)
        && let Some(session) = &fm.session
    {
        archive.push_str(&format!("session: {}\n", session));
    }

    archive.push_str("---\n\n");
    archive.push_str(content.trim());
    archive.push('\n');

    archive
}

fn build_exchange_compact_summary(content: &str, archive_path: &Path) -> String {
    let mut summary = String::from("### Session Summary\n\n");
    summary.push_str(&format!(
        "*Compacted. Content archived to `{}`*\n",
        archive_path.display()
    ));

    let Ok(components) = component::parse(content) else {
        return summary;
    };

    if let Some(exchange) = components.iter().find(|c| c.name == "exchange") {
        let digest = summarize_compacted_exchange(exchange.content(content));
        append_compact_summary_section(&mut summary, "Compacted content", &digest);
    }

    summary
}

fn append_compact_summary_section(summary: &mut String, title: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    summary.push('\n');
    summary.push_str(title);
    summary.push_str(":\n");
    for item in items {
        summary.push_str("- ");
        summary.push_str(item);
        summary.push('\n');
    }
}

fn summarize_compacted_exchange(exchange: &str) -> Vec<String> {
    let parsed = parse_topic_sections_with_tail(exchange);
    let mut summary = Vec::new();

    let topics: Vec<String> = parsed
        .sections
        .iter()
        .filter_map(|section| summarize_response_topic(section))
        .collect();
    if !topics.is_empty() {
        let limit = COMPACT_SUMMARY_ITEM_LIMIT.min(topics.len());
        let mut item = format!(
            "Archived {} response topic(s): {}",
            topics.len(),
            topics[..limit].join("; ")
        );
        if topics.len() > limit {
            item.push_str(&format!("; {} more", topics.len() - limit));
        }
        summary.push(item);
    }

    let preamble = parsed.preamble.trim();
    if !preamble.is_empty() {
        summary.push(format!(
            "Prior summary/context: {}",
            truncate_with_ellipsis(&collapse_whitespace(preamble), COMPACT_SUMMARY_TEXT_LIMIT)
        ));
    }

    let trailing = parsed.trailing.trim();
    if !trailing.is_empty() {
        summary.push(format!(
            "Trailing prompt/context: {}",
            truncate_with_ellipsis(&collapse_whitespace(trailing), COMPACT_SUMMARY_TEXT_LIMIT)
        ));
    }

    if summary.is_empty() {
        summarize_freeform_component(exchange.trim())
    } else {
        summary
    }
}

fn summarize_response_topic(section: &str) -> Option<String> {
    let heading = section.lines().next()?.trim();
    let heading = heading.trim_start_matches('#').trim();
    let topic = heading.strip_prefix("Re:").unwrap_or(heading).trim();
    let topic = topic.strip_suffix("(HEAD)").unwrap_or(topic).trim();
    let topic = topic.split(" — ").next().unwrap_or(topic).trim();
    if topic.is_empty() {
        None
    } else {
        Some(truncate_with_ellipsis(topic, COMPACT_SUMMARY_TEXT_LIMIT))
    }
}

fn summarize_freeform_component(body: &str) -> Vec<String> {
    let excerpt = truncate_with_ellipsis(&collapse_whitespace(body), COMPACT_SUMMARY_TEXT_LIMIT);
    if excerpt.is_empty() {
        Vec::new()
    } else {
        vec![excerpt]
    }
}

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_with_ellipsis(text: &str, max_chars: usize) -> String {
    let total = text.chars().count();
    if total <= max_chars {
        return text.to_string();
    }

    let mut out: String = text.chars().take(max_chars.saturating_sub(3)).collect();
    out.push_str("...");
    out
}

/// Parse the document body into User/Assistant exchange pairs.
fn parse_exchanges(body: &str) -> Vec<Exchange> {
    let mut exchanges = Vec::new();
    let mut sections: Vec<(&str, String)> = Vec::new(); // (type, content)

    // Split by ## User and ## Assistant headings
    let mut current_type = "";
    let mut current_content = String::new();
    let mut in_code_block = false;

    for line in body.lines() {
        // Track code blocks to avoid matching headings inside them
        if line.starts_with("```") {
            in_code_block = !in_code_block;
        }

        if !in_code_block {
            if line == "## User" {
                if !current_type.is_empty() {
                    sections.push((current_type, current_content.clone()));
                }
                current_type = "user";
                current_content.clear();
                continue;
            } else if line == "## Assistant" {
                if !current_type.is_empty() {
                    sections.push((current_type, current_content.clone()));
                }
                current_type = "assistant";
                current_content.clear();
                continue;
            }
        }

        if !current_type.is_empty() {
            current_content.push_str(line);
            current_content.push('\n');
        }
    }

    // Push last section
    if !current_type.is_empty() {
        sections.push((current_type, current_content));
    }

    // Pair up User + Assistant sections into exchanges
    let mut i = 0;
    while i < sections.len() {
        if sections[i].0 == "user" {
            let user = sections[i].1.trim().to_string();
            let assistant = if i + 1 < sections.len() && sections[i + 1].0 == "assistant" {
                i += 1;
                sections[i].1.trim().to_string()
            } else {
                String::new()
            };
            // Only include complete exchanges (with assistant response)
            if !assistant.is_empty() {
                exchanges.push(Exchange { user, assistant });
            }
            // If no assistant response, this is the active/pending user block — skip it
        }
        i += 1;
    }

    exchanges
}

/// Build archive file content from exchanges.
fn build_archive(doc: &Path, original_header: &str, exchanges: &[Exchange]) -> String {
    let mut archive = String::new();

    // Add a header noting the source
    archive.push_str("---\n");
    archive.push_str("archived_from: compact\n");
    archive.push_str(&format!("archived_at: {}\n", chrono_timestamp()));
    archive.push_str("component: exchange\n");
    archive.push_str(&format!("exchange_count: {}\n", exchanges.len()));
    if let Ok(document) = archive_document_value(doc) {
        archive.push_str(&format!("document: {}\n", document));
    }

    // Preserve original frontmatter session ID if present
    if let Ok((fm, _)) = frontmatter::parse(original_header)
        && let Some(session) = &fm.session
    {
        archive.push_str(&format!("session: {}\n", session));
    }

    archive.push_str("---\n\n");

    for (i, exchange) in exchanges.iter().enumerate() {
        archive.push_str("## User\n\n");
        archive.push_str(&exchange.user);
        archive.push('\n');
        archive.push_str("\n## Assistant\n\n");
        archive.push_str(&exchange.assistant);
        archive.push('\n');
        if i < exchanges.len() - 1 {
            archive.push('\n');
        }
    }

    archive
}

/// Save archive to `.agent-doc/archives/<hash>-<timestamp>.md`.
fn save_archive(doc: &Path, content: &str) -> Result<std::path::PathBuf> {
    let snap_path = snapshot::path_for(doc)?;
    // Extract the hash from snapshot path (filename without .md)
    let hash = snap_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");

    // Build archive dir relative to project root
    let project_root = find_project_root(doc)?;
    let archive_dir = project_root.join(".agent-doc/archives");
    std::fs::create_dir_all(&archive_dir)
        .with_context(|| format!("failed to create {}", archive_dir.display()))?;

    let timestamp = chrono_timestamp();
    let archive_path = archive_dir.join(format!("{}-{}.md", hash, timestamp));

    std::fs::write(&archive_path, content)
        .with_context(|| format!("failed to write {}", archive_path.display()))?;
    if let Err(err) = archive_index::index_archive(doc, &archive_path) {
        eprintln!(
            "[compact] warning: archive index update failed for {}: {}",
            archive_path.display(),
            err
        );
    }

    Ok(archive_path)
}

fn archive_document_value(doc: &Path) -> Result<String> {
    let project_root = find_project_root(doc)?;
    let canonical = doc
        .canonicalize()
        .with_context(|| format!("failed to canonicalize {}", doc.display()))?;
    Ok(canonical
        .strip_prefix(&project_root)
        .unwrap_or(&canonical)
        .to_string_lossy()
        .replace('\\', "/"))
}

/// Build the compacted document content.
fn build_compacted(
    original: &str,
    body: &str,
    kept_exchanges: &[Exchange],
    archive_path: &Path,
    archived_count: usize,
) -> String {
    // Extract frontmatter (everything before the body)
    let body_start = original.len() - body.len();
    let header = &original[..body_start];

    let mut result = header.to_string();

    // Add archive summary
    result.push_str(&format!(
        "*{} earlier exchange(s) archived to `{}`*\n\n",
        archived_count,
        archive_path.display()
    ));

    // Add kept exchanges
    for exchange in kept_exchanges {
        result.push_str("## User\n\n");
        result.push_str(&exchange.user);
        result.push_str("\n\n## Assistant\n\n");
        result.push_str(&exchange.assistant);
        result.push_str("\n\n");
    }

    // Add trailing ## User for next prompt
    result.push_str("## User\n\n");

    result
}

/// Create a lightweight git tag at the current HEAD to mark the pre-compact state.
///
/// If `tag_override` is provided it is used as the tag name verbatim.
/// Otherwise, derives the document name from the file stem and auto-generates
/// `agent-doc/<doc-name>/pre-compact-N` where N is the next unused ordinal.
fn create_pre_compact_tag(file: &Path, tag_override: Option<&str>) -> Result<()> {
    // Resolve git root
    let canonical = file
        .canonicalize()
        .with_context(|| format!("file not found: {}", file.display()))?;
    let parent = canonical.parent().unwrap_or(Path::new("/"));

    let toplevel = Command::new("git")
        .current_dir(parent)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("failed to run git rev-parse")?;

    if !toplevel.status.success() {
        anyhow::bail!("file is not in a git repository");
    }
    let git_root = std::path::PathBuf::from(String::from_utf8_lossy(&toplevel.stdout).trim());

    let tag_name = match tag_override {
        Some(name) => name.to_string(),
        None => {
            let doc_name = file
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("doc")
                .to_string();

            // Count existing pre-compact tags to determine next N
            let pattern = format!("agent-doc/{}/pre-compact-*", doc_name);
            let count = Command::new("git")
                .current_dir(&git_root)
                .args(["tag", "-l", &pattern])
                .output()
                .map(|o| {
                    if o.status.success() {
                        String::from_utf8_lossy(&o.stdout)
                            .lines()
                            .filter(|l| !l.trim().is_empty())
                            .count()
                    } else {
                        0
                    }
                })
                .unwrap_or(0);
            format!("agent-doc/{}/pre-compact-{}", doc_name, count + 1)
        }
    };

    let tag_output = Command::new("git")
        .current_dir(&git_root)
        .args(["tag", &tag_name])
        .output()
        .with_context(|| format!("failed to create git tag {}", tag_name))?;

    if !tag_output.status.success() {
        let stderr = String::from_utf8_lossy(&tag_output.stderr);
        anyhow::bail!("git tag {} failed: {}", tag_name, stderr.trim());
    }

    eprintln!("[compact] Tagged pre-compact state as {}", tag_name);
    Ok(())
}

/// Find project root by walking up to find `.agent-doc/`.
fn find_project_root(file: &Path) -> Result<std::path::PathBuf> {
    let canonical = file
        .canonicalize()
        .with_context(|| format!("failed to canonicalize {}", file.display()))?;
    let mut dir = canonical.parent();
    while let Some(d) = dir {
        if d.join(".agent-doc").is_dir() {
            return Ok(d.to_path_buf());
        }
        dir = d.parent();
    }
    // Fallback to file's parent
    Ok(canonical.parent().unwrap_or(Path::new(".")).to_path_buf())
}

/// Generate a compact timestamp for archive filenames.
fn chrono_timestamp() -> String {
    use std::time::SystemTime;
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    // Format as YYYYMMDD-HHMMSS
    let secs = now.as_secs();
    // Simple UTC timestamp without chrono dependency
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Days since epoch to date (simplified)
    let mut y = 1970i64;
    let mut remaining_days = days as i64;
    loop {
        let days_in_year = if is_leap_year(y) { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        y += 1;
    }
    let month_days: &[i64] = if is_leap_year(y) {
        &[31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        &[31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut m = 0;
    for &md in month_days {
        if remaining_days < md {
            break;
        }
        remaining_days -= md;
        m += 1;
    }

    format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}",
        y,
        m + 1,
        remaining_days + 1,
        hours,
        minutes,
        seconds
    )
}

fn is_leap_year(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component::is_backlog_component;

    #[test]
    fn parse_exchanges_basic() {
        let body = "## User\n\nHello\n\n## Assistant\n\nHi there\n\n## User\n\nBye\n\n## Assistant\n\nGoodbye\n\n## User\n\n";
        let exchanges = parse_exchanges(body);
        assert_eq!(exchanges.len(), 2);
        assert_eq!(exchanges[0].user, "Hello");
        assert_eq!(exchanges[0].assistant, "Hi there");
        assert_eq!(exchanges[1].user, "Bye");
        assert_eq!(exchanges[1].assistant, "Goodbye");
    }

    #[test]
    fn parse_exchanges_with_code_blocks() {
        let body = "## User\n\nHere's code:\n\n```\n## User\n## Assistant\n```\n\n## Assistant\n\nNice code\n\n## User\n\n";
        let exchanges = parse_exchanges(body);
        assert_eq!(exchanges.len(), 1);
        assert!(exchanges[0].user.contains("```"));
        assert!(exchanges[0].user.contains("## User"));
    }

    #[test]
    fn parse_exchanges_trailing_user_not_counted() {
        let body = "## User\n\nHello\n\n## Assistant\n\nHi\n\n## User\n\nPending question\n";
        let exchanges = parse_exchanges(body);
        // Only the complete exchange is counted, not the trailing User block
        assert_eq!(exchanges.len(), 1);
    }

    #[test]
    fn build_archive_format() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc_path = dir.path().join("session.md");
        std::fs::write(&doc_path, "---\nsession: test\n---\n").unwrap();
        let exchanges = vec![Exchange {
            user: "Hello".to_string(),
            assistant: "Hi there".to_string(),
        }];
        let archive = build_archive(&doc_path, "---\nsession: test\n---\n", &exchanges);
        assert!(archive.contains("archived_from: compact"));
        assert!(archive.contains("component: exchange"));
        assert!(archive.contains("session: test"));
        assert!(archive.contains("## User\n\nHello"));
        assert!(archive.contains("## Assistant\n\nHi there"));
    }

    #[test]
    fn build_compacted_format() {
        let kept = vec![Exchange {
            user: "Recent question".to_string(),
            assistant: "Recent answer".to_string(),
        }];
        let compacted = build_compacted(
            "---\ntest: true\n---\n\n",
            "\n",
            &kept,
            Path::new("archive.md"),
            3,
        );
        assert!(compacted.contains("3 earlier exchange(s) archived"));
        assert!(compacted.contains("## User\n\nRecent question"));
        assert!(compacted.contains("## Assistant\n\nRecent answer"));
        assert!(compacted.ends_with("## User\n\n"));
    }

    #[test]
    fn chrono_timestamp_format() {
        let ts = chrono_timestamp();
        // Should be YYYYMMDD-HHMMSS format
        assert_eq!(ts.len(), 15);
        assert_eq!(&ts[8..9], "-");
    }

    #[test]
    fn build_component_archive_format() {
        let doc = "---\nagent_doc_session: abc-123\nagent_doc_mode: stream\n---\n\n<!-- agent:exchange -->\nOld conversation\n<!-- /agent:exchange -->\n";
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc_path = dir.path().join("docs/session.md");
        std::fs::create_dir_all(doc_path.parent().unwrap()).unwrap();
        std::fs::write(&doc_path, doc).unwrap();
        let archive = build_component_archive(&doc_path, doc, "exchange", "\nOld conversation\n");
        assert!(archive.contains("archived_from: compact"));
        assert!(archive.contains("component: exchange"));
        assert!(archive.contains("document: docs/session.md"));
        assert!(archive.contains("session: abc-123"));
        assert!(archive.contains("Old conversation"));
    }

    #[test]
    fn parse_topic_sections_basic() {
        let content = "### Session Summary\n\nSome preamble.\n\n### Re: first topic\n\nFirst response.\n\n### Re: second topic\n\nSecond response.\n";
        let (preamble, sections) = parse_topic_sections(content);
        assert!(preamble.contains("Session Summary"));
        assert!(preamble.contains("Some preamble."));
        assert_eq!(sections.len(), 2);
        assert!(sections[0].starts_with("### Re: first topic"));
        assert!(sections[0].contains("First response."));
        assert!(sections[1].starts_with("### Re: second topic"));
        assert!(sections[1].contains("Second response."));
    }

    #[test]
    fn parse_topic_sections_keep_threshold() {
        let content = "### Re: topic 1\nResponse 1.\n### Re: topic 2\nResponse 2.\n";
        let (_, sections) = parse_topic_sections(content);
        // 2 sections ≤ keep=3 → no-op when called from run_component_compact_partial
        assert_eq!(sections.len(), 2);
    }

    #[test]
    fn parse_topic_sections_strips_boundary_marker() {
        let content = "### Re: last topic\n\nContent.\n<!-- agent:boundary:abc123 -->\n";
        let (_, sections) = parse_topic_sections(content);
        assert_eq!(sections.len(), 1);
        assert!(!sections[0].contains("agent:boundary"));
    }

    #[test]
    fn parse_topic_sections_no_re_headings() {
        let content = "Just preamble text.\nNo Re: headings here.\n";
        let (preamble, sections) = parse_topic_sections(content);
        assert!(preamble.contains("Just preamble text."));
        assert_eq!(sections.len(), 0);
    }

    #[test]
    fn partial_compact_preserves_trailing_prompt_after_boundary() {
        let doc = concat!(
            "---\nagent_doc_session: test-tail\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\nExisting summary.\n\n",
            "### Re: first topic\n\nResponse one.\n\n",
            "### Re: second topic\n\nResponse two.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "do #autocmp. spec-test-build-install-commit-push\n",
            "<!-- /agent:exchange -->\n",
        );

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, doc).unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        snapshot::save(&file, doc).unwrap();

        run_component_compact_partial(&file, doc, "exchange", 1, None, false).unwrap();

        let result = std::fs::read_to_string(&file).unwrap();
        let exchange = crate::component::parse(&result)
            .unwrap()
            .into_iter()
            .find(|component| component.name == "exchange")
            .unwrap()
            .content(&result)
            .to_string();

        assert!(exchange.contains("### Re: second topic"));
        assert!(!exchange.contains("### Re: first topic"));
        assert!(exchange.contains("do #autocmp. spec-test-build-install-commit-push"));

        let snapshot_after = snapshot::load(&file).unwrap().unwrap();
        assert!(
            !snapshot_after.contains("do #autocmp. spec-test-build-install-commit-push"),
            "unresolved trailing prompt must remain live drift after compact, not committed snapshot state:\n{snapshot_after}"
        );
    }

    #[test]
    fn component_compact_preserves_non_target_components() {
        let doc = concat!(
            "---\nagent_doc_session: test-123\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "Status: active\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: topic one\n\nLong response about topic one.\n\n",
            "### Re: topic two\n\nLong response about topic two.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Pending\n\n",
            "<!-- agent:pending -->\n",
            "- Task A: do something important\n",
            "- Task B: do something else\n",
            "- Task C: critical item\n",
            "<!-- /agent:pending -->\n",
        );

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, doc).unwrap();

        // Set up .agent-doc dirs
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        snapshot::save(&file, doc).unwrap();

        // Capture pending content before compact
        let components_before = component::parse(doc).unwrap();
        let pending_before = components_before
            .iter()
            .find(|c| is_backlog_component(&c.name))
            .unwrap()
            .content(doc)
            .to_string();
        let status_before = components_before
            .iter()
            .find(|c| c.name == "status")
            .unwrap()
            .content(doc)
            .to_string();

        // Run compact on exchange only
        run_component_compact(&file, doc, "exchange", Some("Compacted summary."), false).unwrap();

        // Read the result and verify non-target components are byte-identical
        let result = std::fs::read_to_string(&file).unwrap();
        let components_after = component::parse(&result).unwrap();
        let pending_after = components_after
            .iter()
            .find(|c| is_backlog_component(&c.name))
            .unwrap()
            .content(&result)
            .to_string();
        let status_after = components_after
            .iter()
            .find(|c| c.name == "status")
            .unwrap()
            .content(&result)
            .to_string();

        assert_eq!(
            pending_before, pending_after,
            "pending component must be byte-identical after compact"
        );
        assert_eq!(
            status_before, status_after,
            "status component must be byte-identical after compact"
        );

        // Verify exchange was actually compacted
        let exchange_after = components_after
            .iter()
            .find(|c| c.name == "exchange")
            .unwrap()
            .content(&result)
            .to_string();
        assert!(exchange_after.contains("Compacted summary."));
        assert!(!exchange_after.contains("### Re: topic one"));
    }

    #[test]
    fn component_compact_delivers_active_document_replacement_via_editor_ipc() {
        use std::sync::mpsc;
        use std::time::{Duration, Instant};

        let doc = concat!(
            "---\nagent_doc_session: test-ipc\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: topic one\n\nResponse one.\n\n",
            "### Re: topic two\n\nResponse two.\n",
            "<!-- /agent:exchange -->\n",
        );

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, doc).unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        let patches_dir = agent_doc_dir.join("patches");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        std::fs::create_dir_all(&patches_dir).unwrap();
        snapshot::save(&file, doc).unwrap();

        let (tx, rx) = mpsc::channel();
        let watcher_dir = patches_dir.clone();
        let watcher_file = file.clone();
        let watcher = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(2);
            while Instant::now() < deadline {
                let Ok(entries) = std::fs::read_dir(&watcher_dir) else {
                    std::thread::sleep(Duration::from_millis(20));
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|value| value.to_str()) != Some("json") {
                        continue;
                    }
                    let raw = std::fs::read_to_string(&path).unwrap();
                    let payload: serde_json::Value = serde_json::from_str(&raw).unwrap();
                    let full_content = payload
                        .get("fullContent")
                        .and_then(|value| value.as_str())
                        .expect("compact IPC payload should carry fullContent")
                        .to_string();
                    std::fs::write(&watcher_file, &full_content).unwrap();
                    std::fs::remove_file(&path).unwrap();
                    tx.send(full_content).unwrap();
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        });

        run_component_compact(&file, doc, "exchange", Some("Compacted summary."), false).unwrap();
        watcher.join().unwrap();
        let ipc_content = rx.recv_timeout(Duration::from_secs(1)).unwrap();

        assert!(ipc_content.contains("Compacted summary."));
        assert!(!ipc_content.contains("### Re: topic one"));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), ipc_content);
        assert_eq!(snapshot::load(&file).unwrap().unwrap(), ipc_content);
    }

    #[test]
    fn component_compact_uses_editor_ipc_after_previous_cycle_committed() {
        use std::sync::mpsc;
        use std::time::{Duration, Instant};

        let doc = concat!(
            "---\nagent_doc_session: test-ipc-committed\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: topic one\n\nResponse one.\n\n",
            "### Re: topic two\n\nResponse two.\n",
            "<!-- /agent:exchange -->\n",
        );

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, doc).unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        let patches_dir = agent_doc_dir.join("patches");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        std::fs::create_dir_all(&patches_dir).unwrap();
        snapshot::save(&file, doc).unwrap();
        crate::cycle_state::start_preflight(&file, Some(doc), Some(doc)).unwrap();
        crate::cycle_state::mark_response_captured(
            &file,
            "test",
            Some(doc),
            Some(doc),
            "fake-sha",
            None,
        )
        .unwrap();
        crate::cycle_state::mark_write_applied(&file, "test", Some(doc), Some(doc)).unwrap();
        crate::cycle_state::mark_committed(&file, "test", Some(doc), Some(doc)).unwrap();

        let (tx, rx) = mpsc::channel();
        let watcher_dir = patches_dir.clone();
        let watcher_file = file.clone();
        let watcher = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(2);
            while Instant::now() < deadline {
                let Ok(entries) = std::fs::read_dir(&watcher_dir) else {
                    std::thread::sleep(Duration::from_millis(20));
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|value| value.to_str()) != Some("json") {
                        continue;
                    }
                    let raw = std::fs::read_to_string(&path).unwrap();
                    let payload: serde_json::Value = serde_json::from_str(&raw).unwrap();
                    let full_content = payload
                        .get("fullContent")
                        .and_then(|value| value.as_str())
                        .expect("compact IPC payload should carry fullContent")
                        .to_string();
                    std::fs::write(&watcher_file, &full_content).unwrap();
                    std::fs::remove_file(&path).unwrap();
                    tx.send(full_content).unwrap();
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        });

        run_component_compact(&file, doc, "exchange", Some("Compacted summary."), false).unwrap();
        watcher.join().unwrap();
        let ipc_content = rx.recv_timeout(Duration::from_secs(1)).expect(
            "compact should not skip full-content IPC just because a previous cycle is committed",
        );

        assert!(ipc_content.contains("Compacted summary."));
        assert!(!ipc_content.contains("### Re: topic one"));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), ipc_content);
        assert_eq!(snapshot::load(&file).unwrap().unwrap(), ipc_content);
        let ops_log = std::fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            !ops_log.contains("late_fallback_patch_rejected"),
            "operator compact is not a stale response fallback"
        );
    }

    #[test]
    fn component_compact_direct_fallback_rejects_late_visible_edit() {
        let doc = concat!(
            "---\nagent_doc_session: test-compact-cas\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: topic one\n\nResponse one.\n",
            "<!-- /agent:exchange -->\n",
        );
        let live = doc.replace(
            "<!-- /agent:exchange -->",
            "live prompt typed during compact\n<!-- /agent:exchange -->",
        );

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, &live).unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        snapshot::save(&file, doc).unwrap();

        let err = run_component_compact(&file, doc, "exchange", Some("Compacted summary."), false)
            .unwrap_err();

        assert!(
            err.to_string().contains("document changed after"),
            "compact fallback should fail with visible-current CAS error: {err}"
        );
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            live,
            "compact fallback must not overwrite a prompt typed after compaction was computed"
        );
        assert_eq!(
            snapshot::load(&file).unwrap().unwrap(),
            doc,
            "failed compact fallback must not advance the snapshot"
        );
        let ops_log = std::fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("visible_write_deferred_current_changed")
                && ops_log.contains("source=compact_full_content_fallback"),
            "compact CAS rejection should be logged:\n{ops_log}"
        );
    }

    #[test]
    fn exchange_compact_default_summary_includes_archived_content_digest() {
        let doc = concat!(
            "---\nagent_doc_session: test-summary\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: topic one\n\nResponse one.\n\n",
            "### Re: topic two\n\nResponse two.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Queue\n\n",
            "<!-- agent:queue -->\n",
            "- do #next. run targeted test\n",
            "- do #later. build and install\n",
            "<!-- /agent:queue -->\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#rpaj] Make compact exchange synthesize backlog-aware context\n",
            "- [/release] [#ship] Wait for release window\n",
            "- [x] [#done] Already handled\n",
            "<!-- /agent:backlog -->\n\n",
            "## Icebox\n\n",
            "<!-- agent:icebox -->\n",
            "- [ ] [#parked] Parked follow-up for later\n",
            "<!-- /agent:icebox -->\n",
        );

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, doc).unwrap();

        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        snapshot::save(&file, doc).unwrap();

        run_component_compact(&file, doc, "exchange", None, false).unwrap();

        let result = std::fs::read_to_string(&file).unwrap();
        let components = component::parse(&result).unwrap();
        let exchange = components
            .iter()
            .find(|c| c.name == "exchange")
            .unwrap()
            .content(&result)
            .to_string();

        assert!(exchange.contains("### Session Summary"));
        assert!(exchange.contains("*Compacted. Content archived to `"));
        assert!(exchange.contains("Compacted content:"));
        assert!(exchange.contains("Archived 2 response topic(s): topic one; topic two"));
        assert!(!exchange.contains("Active backlog:"));
        assert!(!exchange.contains("[#rpaj]"));
        assert!(!exchange.contains("Queue:"));
        assert!(!exchange.contains("Icebox:"));
        assert!(!exchange.contains("### Re: topic one"));
    }

    #[test]
    fn exchange_compact_reconciles_stale_top_backlog_status() {
        let doc = concat!(
            "---\nagent_doc_session: test-summary\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "Top backlog item: #done.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: topic one\n\nResponse one.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n"
        );

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, doc).unwrap();

        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        snapshot::save(&file, doc).unwrap();

        run_component_compact(&file, doc, "exchange", Some("Compacted."), false).unwrap();

        let result = std::fs::read_to_string(&file).unwrap();
        assert!(result.contains("No open backlog items."));
        assert!(!result.contains("Top backlog item: #done."));
        let snap = snapshot::load(&file).unwrap().unwrap();
        assert!(snap.contains("No open backlog items."));
    }

    #[test]
    fn crdt_compact_preserves_pending_with_state_refresh() {
        // Test that CRDT state refresh prevents pending items from being lost
        let doc = concat!(
            "---\nagent_doc_session: test-crdt-123\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: topic one\n\nResponse one.\n\n",
            "### Re: topic two\n\nResponse two.\n\n",
            "### Re: topic three\n\nResponse three.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Pending\n\n",
            "<!-- agent:pending -->\n",
            "- ✅ completed task\n",
            "- 🔄 in-progress work\n",
            "- 🆕 new task to add\n",
            "<!-- /agent:pending -->\n",
        );

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, doc).unwrap();

        // Set up .agent-doc dirs and save initial CRDT state
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        snapshot::save(&file, doc).unwrap();

        // Create and save initial CRDT state
        let initial_crdt = crate::crdt::CrdtDoc::from_text(doc).encode_state();
        snapshot::save_crdt(&file, &initial_crdt).unwrap();

        // Capture pending before compact
        let components_before = component::parse(doc).unwrap();
        let pending_before = components_before
            .iter()
            .find(|c| is_backlog_component(&c.name))
            .unwrap()
            .content(doc)
            .to_string();

        // Run compact with CRDT mode enabled (is_crdt=true)
        run_component_compact(&file, doc, "exchange", Some("Compacted."), true).unwrap();

        // Read result and verify pending survives
        let result = std::fs::read_to_string(&file).unwrap();
        let components_after = component::parse(&result).unwrap();
        let pending_after = components_after
            .iter()
            .find(|c| is_backlog_component(&c.name))
            .unwrap()
            .content(&result)
            .to_string();

        assert_eq!(
            pending_before, pending_after,
            "pending component must survive CRDT state refresh during compact"
        );
        assert!(pending_after.contains("completed task"));
        assert!(pending_after.contains("in-progress work"));
        assert!(pending_after.contains("new task to add"));
    }

    #[test]
    fn compact_preserves_boundary_marker() {
        // Test that boundary markers (❯) survive compact operations
        let doc = concat!(
            "---\nagent_doc_session: test-boundary\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: first topic\n\nResponse one.\n\n",
            "### Re: second topic\n\nResponse two.\n",
            "<!-- agent:boundary:abc123def456 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "❯ Critical item: verify preservation\n",
            "<!-- /agent:status -->\n",
        );

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, doc).unwrap();

        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        snapshot::save(&file, doc).unwrap();

        // Capture status with ❯ before compact
        let components_before = component::parse(doc).unwrap();
        let status_before = components_before
            .iter()
            .find(|c| c.name == "status")
            .unwrap()
            .content(doc)
            .to_string();

        run_component_compact(&file, doc, "exchange", Some("Archived."), false).unwrap();

        // Verify ❯ marker preserved in non-target component
        let result = std::fs::read_to_string(&file).unwrap();
        let components_after = component::parse(&result).unwrap();
        let status_after = components_after
            .iter()
            .find(|c| c.name == "status")
            .unwrap()
            .content(&result)
            .to_string();

        assert_eq!(status_before, status_after);
        assert!(status_after.contains("❯"));
        assert!(status_after.contains("Critical item"));
    }

    #[test]
    fn compact_working_tree_consistency() {
        // Test that compact leaves working tree in consistent state
        // (file unchanged vs disk, snapshot updated, no stale CRDT)
        let doc = concat!(
            "---\nagent_doc_session: test-wt\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: topic A\n\nResponse A.\n\n",
            "### Re: topic B\n\nResponse B.\n",
            "<!-- /agent:exchange -->\n",
        );

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, doc).unwrap();

        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        snapshot::save(&file, doc).unwrap();

        let file_before = std::fs::read_to_string(&file).unwrap();

        run_component_compact(&file, doc, "exchange", Some("Summary."), false).unwrap();

        // After compact: file and snapshot should match
        let file_after = std::fs::read_to_string(&file).unwrap();
        let snap_path = snapshot::path_for(&file).unwrap();
        let snapshot_content = std::fs::read_to_string(&snap_path).unwrap();

        assert_eq!(
            file_after, snapshot_content,
            "file and snapshot must match after compact"
        );

        // Verify the document was actually modified
        assert_ne!(file_before, file_after);
        assert!(file_after.contains("Summary."));
    }

    #[test]
    fn compact_with_commit_writes_vcs_refresh_signal() {
        use std::fs;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/archives")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/patches")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();

        std::process::Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let file = root.join("session.md");
        let doc = concat!(
            "---\nagent_doc_session: test-compact\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: topic one\n\nResponse one.\n\n",
            "### Re: topic two\n\nResponse two.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&file, doc).unwrap();
        snapshot::save(&file, doc).unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        run(
            &file,
            None,
            Some("exchange"),
            Some("Compacted summary."),
            Some("skip"),
            true,
        )
        .unwrap();

        let signal = root.join(".agent-doc/patches/vcs-refresh.signal");
        assert!(
            signal.exists(),
            "expected VCS refresh signal at {}",
            signal.display()
        );

        let log = std::process::Command::new("git")
            .current_dir(root)
            .args(["log", "--oneline", "-n", "1", "--", "session.md"])
            .output()
            .unwrap();
        let log = String::from_utf8_lossy(&log.stdout);
        assert!(
            log.contains("agent-doc(session):"),
            "compact closeout should use agent-doc commit, got:\n{log}"
        );

        let committed = crate::git::show_head(&file).unwrap().unwrap();
        assert!(committed.contains("Compacted summary."));
        assert!(!committed.contains("### Re: topic one"));
    }
}
