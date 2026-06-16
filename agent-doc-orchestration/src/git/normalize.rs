//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;

pub(crate) fn strip_boundary_markers(content: &str) -> String {
    content
        .lines()
        .filter(|line| !line.trim().starts_with("<!-- agent:boundary:"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn normalize_transient_agent_doc_markers(content: &str) -> String {
    // #22a8: also drop the managed `agent_doc_pipeline:` frontmatter block. It is
    // mirrored onto the document mid-cycle (after response capture, cleared at a
    // terminal phase), so a comparison that kept it would read the managed write
    // as a direct response patchback / closeout drift. Stripping it here keeps
    // every doc-vs-snapshot/HEAD comparison routed through this normalizer
    // invariant to the pipeline mirror.
    crate::frontmatter::strip_pipeline_block_lines(&strip_guard_markers(&strip_head_markers(
        &strip_boundary_markers(content),
    )))
}

/// Replace the `agent:queue` component (opening-tag attributes + body) with a
/// canonical empty placeholder.
///
/// The queue is rewritten by preflight queue-maintenance on essentially every
/// cycle (activation toggles, `auto` strip, head strike, dedup, IPC-buffer
/// merge artifacts) independently of the response body, which always targets
/// `exchange`/`output`. Neutralizing it before hashing keeps response-replay /
/// stale-lock recovery stable across queue churn (#adoc-queue-ipc-buffer-divergence
/// root cause #4: the capture-replay guard must validate the response body, not
/// a whole-document hash that queue-component churn invalidates).
pub(crate) fn neutralize_queue_component(content: &str) -> String {
    let Ok(components) = crate::component::parse(content) else {
        return content.to_string();
    };
    let Some(queue) = components.iter().find(|c| c.name == "queue") else {
        return content.to_string();
    };
    let mut out = String::with_capacity(content.len());
    out.push_str(&content[..queue.open_start]);
    out.push_str("<!-- agent:queue -->\n<!-- /agent:queue -->");
    out.push_str(&content[queue.close_end..]);
    out
}

/// Drop the transient queue activation frontmatter — the canonical `queue:`
/// control (`#queue-state-unify`) and the deprecated `queue_active:` line — which
/// queue maintenance toggles in lockstep with the `agent:queue` component and is
/// likewise independent of the response body. Both are normalized away together
/// so a legacy `queue_active:` and a migrated `queue: start|stop` compare equal,
/// avoiding the snapshot/HEAD drift loop. Only used for replay-hash
/// normalization, never persisted.
pub(crate) fn strip_queue_active_frontmatter(content: &str) -> String {
    content
        .lines()
        .filter(|line| {
            let t = line.trim_start();
            !t.starts_with("queue_active:") && !t.starts_with("queue:")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Normalization used for response-replay / stale-cycle hash matching.
///
/// Builds on [`normalize_transient_agent_doc_markers`] (boundary/`(HEAD)`/guard
/// markers) and additionally neutralizes the `agent:queue` component **and** the
/// `queue_active:` frontmatter flag so that independent queue-maintenance churn
/// does not invalidate the match. Used by both `cycle_state` (store side) and
/// `repair` (compare side) so the two always normalize identically.
pub fn normalize_for_replay_hash(content: &str) -> String {
    normalize_transient_agent_doc_markers(&strip_queue_active_frontmatter(
        &neutralize_queue_component(content),
    ))
}

pub(crate) fn is_response_heading_line(trimmed: &str) -> bool {
    trimmed.starts_with("### Re:")
        || trimmed.starts_with("#### Re:")
        || trimmed.starts_with("##### Re:")
}

pub(crate) fn fence_open(trimmed: &str) -> Option<(char, usize)> {
    let fc = trimmed.chars().next()?;
    if fc != '`' && fc != '~' {
        return None;
    }
    let fl = trimmed.chars().take_while(|&c| c == fc).count();
    if fl >= 3 { Some((fc, fl)) } else { None }
}

pub(crate) fn fence_close(trimmed: &str, fence_char: char, fence_len: usize) -> bool {
    let fc = trimmed.chars().next().unwrap_or('\0');
    if fc != fence_char {
        return false;
    }
    let fl = trimmed.chars().take_while(|&c| c == fence_char).count();
    fl >= fence_len && trimmed[fl..].trim().is_empty()
}

pub(crate) fn prefix_prompt_line(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if trimmed.is_empty()
        || trimmed.starts_with('❯')
        || crate::diff::line_looks_like_markdown_list_item(trimmed)
    {
        return None;
    }
    let indent_len = line.len() - trimmed.len();
    Some(format!("{}❯ {}", &line[..indent_len], trimmed))
}

pub(crate) fn answered_prompt_prelude_start(lines: &[&str]) -> Option<usize> {
    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if crate::diff::line_looks_like_prompt_prefix_repair_start(trimmed, false) {
            return Some(idx);
        }
    }
    None
}

pub(crate) fn canonicalize_answered_prompt_prefixes(exchange_content: &str) -> String {
    // When a response heading is present, the contiguous prose block
    // immediately above it is the user prelude for that turn. Canonicalize
    // that prelude back to `❯ ...` so answered prompts keep their visual
    // marker after staging/cleanup instead of collapsing to bare lines.

    let lines: Vec<&str> = exchange_content.split_inclusive('\n').collect();
    if lines.is_empty() {
        return exchange_content.to_string();
    }

    let mut line_in_fence = vec![false; lines.len()];
    let mut in_fence = false;
    let mut fence_char = '\0';
    let mut fence_len = 0usize;
    for idx in 0..lines.len() {
        let line = lines[idx].trim_end_matches('\n');
        let trimmed = line.trim();
        if !in_fence {
            if let Some((fc, fl)) = fence_open(trimmed) {
                in_fence = true;
                fence_char = fc;
                fence_len = fl;
                line_in_fence[idx] = true;
                continue;
            }
        } else {
            line_in_fence[idx] = true;
            if fence_close(trimmed, fence_char, fence_len) {
                in_fence = false;
            }
            continue;
        }
    }

    let mut prefix_targets = vec![false; lines.len()];
    for idx in 0..lines.len() {
        let trimmed = lines[idx].trim_end_matches('\n').trim();
        if line_in_fence[idx] || !is_response_heading_line(trimmed) {
            continue;
        }

        let mut block_indices = Vec::new();
        let mut cursor = idx;
        let mut stopped_on_response_heading = false;
        while cursor > 0 {
            cursor -= 1;
            let line = lines[cursor].trim_end_matches('\n');
            let trimmed = line.trim();
            if line_in_fence[cursor]
                || trimmed.is_empty()
                || trimmed.starts_with("<!--")
                || is_response_heading_line(trimmed)
            {
                stopped_on_response_heading =
                    !line_in_fence[cursor] && is_response_heading_line(trimmed);
                break;
            }
            block_indices.push(cursor);
        }
        block_indices.reverse();
        if block_indices.is_empty() {
            continue;
        }
        // A prose block that butts directly against a preceding `### Re:`
        // heading with no blank-line / comment separator is the trailing body
        // of that response (e.g. a duplicated response block left by a
        // multi-retry / late-IPC reposition), not a fresh user prelude. Never
        // canonicalize those lines into `❯ ` prompt prefixes — agent response
        // body must never receive the user-prompt marker.
        if stopped_on_response_heading {
            continue;
        }

        let block_lines: Vec<&str> = block_indices
            .iter()
            .map(|&line_idx| lines[line_idx])
            .collect();
        let Some(prefix_start) = answered_prompt_prelude_start(&block_lines) else {
            continue;
        };
        for line_idx in block_indices.into_iter().skip(prefix_start) {
            prefix_targets[line_idx] = true;
        }
    }

    let mut normalized = String::with_capacity(exchange_content.len());
    let mut changed = false;
    for (idx, segment) in lines.iter().enumerate() {
        let line = segment.trim_end_matches('\n');
        if prefix_targets[idx] {
            if let Some(prefixed) = prefix_prompt_line(line) {
                normalized.push_str(&prefixed);
                changed |= prefixed != line;
            } else {
                normalized.push_str(line);
            }
        } else {
            normalized.push_str(line);
        }
        if segment.ends_with('\n') {
            normalized.push('\n');
        }
    }

    if changed {
        normalized
    } else {
        exchange_content.to_string()
    }
}

pub fn normalize_committed_exchange_artifacts(content: &str) -> String {
    let transient = normalize_transient_agent_doc_markers(content);
    let body = match crate::frontmatter::parse(&transient) {
        Ok((_, body)) => body,
        Err(_) => return transient,
    };
    let prefix_len = transient.len().saturating_sub(body.len());
    let Ok(components) = crate::component::parse(body) else {
        return transient;
    };

    let mut rebuilt = String::with_capacity(transient.len());
    rebuilt.push_str(&transient[..prefix_len]);
    let mut last = 0usize;
    let mut changed = false;
    for comp in components {
        if comp.open_end < last {
            continue;
        }
        rebuilt.push_str(&body[last..comp.open_end]);
        if comp.name == "exchange" {
            let normalized = canonicalize_answered_prompt_prefixes(comp.content(body));
            changed |= normalized != comp.content(body);
            rebuilt.push_str(&normalized);
        } else {
            rebuilt.push_str(comp.content(body));
        }
        rebuilt.push_str(&body[comp.close_start..comp.close_end]);
        last = comp.close_end;
    }
    rebuilt.push_str(&body[last..]);

    if changed { rebuilt } else { transient }
}

pub(crate) fn strip_re_heading_attribution(content: &str) -> String {
    let code_ranges = code_block_byte_ranges(content);
    let mut result_lines: Vec<String> = Vec::new();
    let mut offset = 0usize;
    for line in content.lines() {
        if !is_in_code_block(&code_ranges, offset) {
            let trimmed = line.trim_start();
            let hash_count = trimmed.chars().take_while(|&c| c == '#').count();
            if (1..=6).contains(&hash_count) && trimmed.chars().nth(hash_count) == Some(' ') {
                let after_hash = trimmed[hash_count..].trim_start();
                if after_hash.starts_with("Re:")
                    && let Some(pos) = line.rfind(" — ")
                {
                    result_lines.push(line[..pos].to_string());
                    offset += line.len() + 1;
                    continue;
                }
            }
        }
        result_lines.push(line.to_string());
        offset += line.len() + 1;
    }
    let result = result_lines.join("\n");
    if content.ends_with('\n') {
        format!("{result}\n")
    } else {
        result
    }
}

pub fn normalize_post_commit_re_heading_drift(content: &str) -> String {
    strip_re_heading_attribution(&normalize_transient_agent_doc_markers(content))
}

pub(crate) fn normalize_component_content_for_absorb(content: &str) -> String {
    normalize_transient_agent_doc_markers(content)
        .trim()
        .to_string()
}

pub(crate) fn redact_component_contents_for_absorb(body: &str) -> Option<String> {
    let components = crate::component::parse(body).ok()?;
    let mut redacted = String::with_capacity(body.len());
    let mut last = 0usize;
    for comp in components {
        if comp.open_end < last {
            // Nested inside a previously processed component — already redacted
            continue;
        }
        redacted.push_str(&body[last..comp.open_end]);
        redacted.push_str(&body[comp.close_start..comp.close_end]);
        last = comp.close_end;
    }
    redacted.push_str(&body[last..]);
    Some(redacted)
}

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    #[test]
    fn normalize_for_replay_hash_neutralizes_queue_churn() {
        // #adoc-queue-ipc-buffer-divergence root cause #4: queue-maintenance
        // churn (auto strip + activation toggle + drain) must not change the
        // replay-hash normalization, because the response body lives in
        // `exchange`, not `queue`.
        let with_active_queue = concat!(
            "---\nagent_doc_format: template\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: topic — gpt-5\nResponse body.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "preset #spec-test\n- do [#a]\n",
            "<!-- /agent:queue -->\n"
        );
        // Same response; queue halted/drained (the post-maintenance shape).
        let with_drained_queue = concat!(
            "---\nagent_doc_format: template\nqueue_active: false\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: topic — gpt-5\nResponse body.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n"
        );
        assert_eq!(
            normalize_for_replay_hash(with_active_queue),
            normalize_for_replay_hash(with_drained_queue),
            "queue-only churn must not change the replay normalization"
        );

        // A genuine response-body change still registers as different.
        let with_changed_response = with_active_queue.replace("Response body.", "Different body.");
        assert_ne!(
            normalize_for_replay_hash(with_active_queue),
            normalize_for_replay_hash(&with_changed_response),
            "a real response-body change must still change the replay normalization"
        );
    }
    #[test]
    fn canonicalize_answered_prompt_prefixes_uses_opt_in_prompt_start() {
        let exchange = "\
### Re: sync latency — gpt-5

The current tree has already started making this accountable.
### Re: closeout guard — gpt-5

No additional prompt-bearing change was present.
Please rerun the deploy check.
### Re: deploy check — gpt-5

Done.
";

        let normalized = canonicalize_answered_prompt_prefixes(exchange);

        assert!(
            normalized
                .contains("\nThe current tree has already started making this accountable.\n"),
            "plain assistant prose before the next response heading must stay bare:\n{normalized}"
        );
        assert!(
            !normalized
                .contains("\n❯ The current tree has already started making this accountable.\n"),
            "assistant prose must not become a prompt by default:\n{normalized}"
        );
        assert!(
            normalized.contains("\n❯ Please rerun the deploy check.\n"),
            "soft prompt requests before a response heading should still be canonicalized:\n{normalized}"
        );
    }
    #[test]
    fn canonicalize_answered_prompt_prefixes_never_prefixes_duplicate_response_body() {
        // #finalize-retry-ipc-response-duplication: a multi-retry / late-IPC
        // reposition can leave a stale duplicate response block whose body
        // butts directly against the canonical `### Re: … (HEAD)` heading with
        // no blank-line separator. Those lines are agent response body, not a
        // user prelude, and must never receive the `❯ ` prompt prefix.
        let exchange = "\
❯ do [#fix-thing]
### Re: fix thing — opus-4-8
**Scope/honesty:** narrow.
**Commits:** abc123.
### Re: fix thing — opus-4-8 (HEAD)
**Scope/honesty:** narrow.
**Commits:** abc123.
";

        let normalized = canonicalize_answered_prompt_prefixes(exchange);

        assert!(
            !normalized.contains("❯ **Scope/honesty:**"),
            "duplicate response body must not be rewritten as a prompt:\n{normalized}"
        );
        assert!(
            !normalized.contains("❯ **Commits:**"),
            "duplicate response body must not be rewritten as a prompt:\n{normalized}"
        );
        // The only `❯` line is the genuine, already-marked user prompt.
        assert_eq!(
            normalized.matches('❯').count(),
            1,
            "exactly the existing user prompt keeps its marker:\n{normalized}"
        );
    }
    #[test]
    fn canonicalize_answered_prompt_prefixes_preserves_markdown_lists() {
        let exchange = "\
Please compare these options:
- keep this bullet bare
  - keep this nested bullet bare
1. keep this ordered bullet bare
### Re: options — gpt-5

Done.
";

        let normalized = canonicalize_answered_prompt_prefixes(exchange);

        assert!(
            normalized.starts_with(
                "❯ Please compare these options:\n- keep this bullet bare\n  - keep this nested bullet bare\n1. keep this ordered bullet bare\n"
            ),
            "prompt prose should be prefixed without rewriting markdown list items:\n{normalized}"
        );
        assert!(
            !normalized.contains("\n❯ - keep this bullet bare")
                && !normalized.contains("\n❯   - keep this nested bullet bare")
                && !normalized.contains("\n❯ 1. keep this ordered bullet bare"),
            "markdown list items must not receive prompt prefixes:\n{normalized}"
        );
    }
    #[test]
    fn commit_adopts_manual_escaped_tail_cleanup_after_head_current_snapshot() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let committed = "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            <!-- agent:boundary:head -->\n\
            <!-- /agent:exchange -->\n\n\
            The routed prompt escaped below the exchange block.\n\
            It should be cleaned up without being treated as later drift.\n\n\
            do #oobtaildel. spec-test-build-install-commit-push\n\n\
            <!-- agent:backlog -->\n\
            - [ ] keep me\n\
            <!-- /agent:backlog -->\n";
        fs::write(&doc, committed).unwrap();
        crate::snapshot::save(&doc, committed).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let cleaned = "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            <!-- agent:boundary:head -->\n\
            <!-- /agent:exchange -->\n\n\
            <!-- agent:backlog -->\n\
            - [ ] keep me\n\
            <!-- /agent:backlog -->\n";
        fs::write(&doc, cleaned).unwrap();
        crate::snapshot::save(&doc, committed).unwrap();

        let did_commit = commit(&doc).expect("escaped tail cleanup should commit");
        assert!(did_commit, "cleanup deletion should create a commit");

        let head = show_head(&doc).unwrap().unwrap();
        assert_eq!(
            normalize_transient_agent_doc_markers(&head),
            normalize_transient_agent_doc_markers(cleaned),
            "HEAD should contain the cleanup deletion"
        );
        let snap = crate::snapshot::load(&doc).unwrap().unwrap();
        assert_eq!(
            normalize_transient_agent_doc_markers(&snap),
            normalize_transient_agent_doc_markers(cleaned),
            "snapshot should advance to the cleaned file"
        );

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("post_commit_escaped_tail_cleanup file="),
            "cleanup should get a specific ops-log marker:\n{log}"
        );
        assert!(
            !log.contains("post_commit_local_drift file="),
            "cleanup-only deletion must not be classified as local drift:\n{log}"
        );
    }
    #[test]
    fn commit_allows_current_snapshot_to_replace_committed_historical_patchback() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let clean = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "clean exchange\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:icebox -->\n",
            "- [ ] [#tmv7] Release workflow\n",
            "<!-- /agent:icebox -->\n",
        );
        fs::write(&doc, clean).unwrap();
        crate::snapshot::save(&doc, clean).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let historical_head = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "clean exchange\n\n",
            "#code-review\n",
            "### Re: code review — gpt-5\n\n",
            "Historical patchback.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:icebox -->\n",
            "- [ ] [#tmv7] Release workflow\n",
            "<!-- /agent:icebox -->\n",
        );
        fs::write(&doc, historical_head).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "manual patchback", "--no-verify"])
            .output()
            .unwrap();

        let compacted = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "*Compacted.*\n\n",
            "❯ #code-review\n",
            "<!-- agent:boundary:test -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:icebox -->\n",
            "- [x] [#tmv7] Release workflow\n",
            "<!-- /agent:icebox -->\n",
        );
        fs::write(&doc, compacted).unwrap();
        crate::snapshot::save(&doc, compacted).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(compacted), Some(compacted)).unwrap();
        crate::cycle_state::mark_write_applied(
            &doc,
            "write_template",
            Some(compacted),
            Some(compacted),
        )
        .unwrap();

        let did_commit =
            commit(&doc).expect("current snapshot/file should replace the historical patchback");
        assert!(did_commit, "replacement commit should be created");

        let head_doc = show_head(&doc).unwrap().unwrap();
        assert_eq!(
            normalize_transient_agent_doc_markers(&head_doc),
            normalize_transient_agent_doc_markers(compacted),
            "HEAD should advance to the compacted document:\n{head_doc}"
        );

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            !log.contains("commit_blocked_committed_historical_patchback file="),
            "historical patchback should not block replacement commit:\n{log}"
        );
    }
    #[test]
    fn redact_component_contents_handles_nested_components() {
        let body = r#"## Status

<!-- agent:status patch=replace -->
Status content here.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Some exchange content.
Add <!-- agent:queue -->...<!-- /agent:queue --> to the template.
More content.
<!-- /agent:exchange -->

## Pending

<!-- agent:pending -->
- [ ] task
<!-- /agent:pending -->
"#;
        let result = redact_component_contents_for_absorb(body);
        assert!(result.is_some(), "should not panic on nested components");
        let redacted = result.unwrap();
        assert!(
            redacted.contains("<!-- agent:status patch=replace -->"),
            "should contain status open marker"
        );
        assert!(
            redacted.contains("<!-- /agent:status -->"),
            "should contain status close marker"
        );
        assert!(
            !redacted.contains("Status content here."),
            "should redact status content"
        );
        assert!(
            !redacted.contains("Some exchange content."),
            "should redact exchange content (including nested markers)"
        );
    }
}
