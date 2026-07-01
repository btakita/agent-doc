//! Pure task extraction and DAG planning policy for `agent-doc orchestrate`.

use std::collections::HashSet;

use agent_doc_frontmatter::frontmatter::ResolvedMode;
use anyhow::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExchangeTaskSourceFingerprint {
    pub tasks: Vec<String>,
    pub requested_presets: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExchangeTaskSourceBlock {
    tasks: Vec<String>,
    requested_presets: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionTask {
    pub label: String,
    pub prompt: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DagTask {
    pub id: String,
    pub prompt: String,
    pub deps: Vec<String>,
}

#[derive(Debug, Default)]
struct DagMetadata {
    id: Option<String>,
    after: Vec<String>,
}

pub fn extract_tasks_from_text(text: &str) -> Vec<String> {
    let code_blocks = collect_fenced_task_blocks(text);
    if let Some(block) = code_blocks.last()
        && !block.is_empty()
    {
        return block.clone();
    }

    let list_blocks = collect_markdown_list_blocks(text);
    if let Some(block) = list_blocks.last()
        && !block.is_empty()
    {
        return block.clone();
    }

    text.lines()
        .map(normalize_task)
        .filter(|line| !line.is_empty())
        .collect()
}

pub fn find_exchange_task_source(
    exchange: &str,
    original: &ExchangeTaskSourceFingerprint,
) -> Option<ExchangeTaskSourceFingerprint> {
    if original.tasks.is_empty() {
        return None;
    }
    let mut candidates = collect_markdown_list_source_blocks(exchange)
        .into_iter()
        .filter(|block| contains_ordered_subsequence(&block.tasks, &original.tasks))
        .collect::<Vec<_>>();

    let candidate = candidates
        .iter()
        .rev()
        .find(|block| block.requested_presets == original.requested_presets)
        .cloned()
        .or_else(|| candidates.pop())?;

    Some(ExchangeTaskSourceFingerprint {
        tasks: candidate.tasks,
        requested_presets: candidate.requested_presets,
    })
}

fn collect_markdown_list_source_blocks(text: &str) -> Vec<ExchangeTaskSourceBlock> {
    let lines = text.lines().collect::<Vec<_>>();
    let mut blocks = Vec::new();
    let mut idx = 0usize;

    while idx < lines.len() {
        let Some(first_task) = parse_list_item(lines[idx]) else {
            idx += 1;
            continue;
        };

        let list_start = idx;
        let mut tasks = vec![first_task];
        idx += 1;
        while idx < lines.len() {
            let Some(task) = parse_list_item(lines[idx]) else {
                break;
            };
            tasks.push(task);
            idx += 1;
        }
        let list_end = idx;

        let mut context_start = list_start;
        while context_start > 0 {
            let previous = lines[context_start - 1].trim();
            if previous.is_empty()
                || previous.starts_with("### ")
                || previous.starts_with("## ")
                || previous.starts_with("<!-- agent:boundary:")
            {
                break;
            }
            context_start -= 1;
        }
        let source_text = lines[context_start..list_end].join("\n");
        blocks.push(ExchangeTaskSourceBlock {
            tasks,
            requested_presets: agent_doc_diff::extract_prompt_preset_requests_from_text(
                &source_text,
            ),
        });
    }

    blocks
}

fn contains_ordered_subsequence(haystack: &[String], needle: &[String]) -> bool {
    if needle.is_empty() {
        return true;
    }
    let mut cursor = 0usize;
    for item in haystack {
        if item == &needle[cursor] {
            cursor += 1;
            if cursor == needle.len() {
                return true;
            }
        }
    }
    false
}

/// Scope current exchange text to lines appended after a snapshot exchange.
pub fn scope_exchange_tail(exchange: &str, snapshot_exchange: &str) -> String {
    let current_lines: Vec<&str> = exchange.lines().collect();
    let snap_lines: Vec<&str> = snapshot_exchange.lines().collect();

    // Compare lines, ignoring boundary artifacts like (HEAD) markers.
    let normalize_for_compare = |line: &str| -> String {
        let trimmed = line.trim();
        if trimmed.starts_with("<!-- agent:boundary:") {
            return String::new();
        }
        line.replace(" (HEAD)", "")
    };

    let mut matching = 0;
    for (curr, snap) in current_lines.iter().zip(snap_lines.iter()) {
        if normalize_for_compare(curr) == normalize_for_compare(snap) {
            matching += 1;
        } else {
            break;
        }
    }

    if matching >= snap_lines.len() && current_lines.len() > snap_lines.len() {
        let tail: String = current_lines[snap_lines.len()..]
            .iter()
            .filter(|line| !line.trim().starts_with("<!--"))
            .copied()
            .collect::<Vec<_>>()
            .join("\n");
        if !tail.trim().is_empty() {
            return tail;
        }
    }

    exchange.to_string()
}

pub fn apply_prompt_preset_block(task: &str, prompt_preset_block: Option<&str>) -> String {
    match prompt_preset_block {
        Some(block) if !block.trim().is_empty() => format!("{}\n{}", block.trim_end(), task),
        _ => task.to_string(),
    }
}

pub fn append_worker_result_line(
    response: &str,
    worker_result_line: &str,
    mode: ResolvedMode,
) -> String {
    if response.contains(worker_result_line) {
        return response.to_string();
    }
    if mode.is_template() {
        const CLOSE: &str = "<!-- /patch:exchange -->";
        if let Some(idx) = response.rfind(CLOSE) {
            let mut out = String::with_capacity(response.len() + worker_result_line.len() + 2);
            out.push_str(response[..idx].trim_end());
            out.push('\n');
            out.push_str(worker_result_line);
            out.push('\n');
            out.push_str(&response[idx..]);
            return out;
        }
    }
    let mut out = response.trim_end().to_string();
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(worker_result_line);
    out.push('\n');
    out
}

fn collect_fenced_task_blocks(text: &str) -> Vec<Vec<String>> {
    let mut blocks = Vec::new();
    let mut in_fence = false;
    let mut fence_char = '\0';
    let mut fence_len = 0usize;
    let mut current = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if !in_fence {
            if let Some((fc, fl)) = fence_open(trimmed) {
                in_fence = true;
                fence_char = fc;
                fence_len = fl;
                current.clear();
            }
            continue;
        }

        if fence_close(trimmed, fence_char, fence_len) {
            let tasks = collect_list_items(&current.join("\n"));
            if !tasks.is_empty() {
                blocks.push(tasks);
            }
            in_fence = false;
            current.clear();
            continue;
        }

        current.push(line.to_string());
    }

    blocks
}

fn collect_markdown_list_blocks(text: &str) -> Vec<Vec<String>> {
    let mut blocks = Vec::new();
    let mut current = Vec::new();

    for line in text.lines() {
        if let Some(task) = parse_list_item(line) {
            current.push(task);
        } else if !current.is_empty() {
            blocks.push(std::mem::take(&mut current));
        }
    }

    if !current.is_empty() {
        blocks.push(current);
    }

    blocks
}

fn collect_list_items(text: &str) -> Vec<String> {
    text.lines().filter_map(parse_list_item).collect()
}

pub fn parse_list_item(line: &str) -> Option<String> {
    let trimmed = line.trim();
    // Strip the binary-owned prompt prefix that write-back adds to user prompts.
    let trimmed = trimmed
        .strip_prefix("❯ ")
        .or_else(|| trimmed.strip_prefix("❯"))
        .unwrap_or(trimmed);
    if let Some(rest) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
    {
        let task = normalize_task(rest);
        return (!task.is_empty()).then_some(task);
    }

    let digit_count = trimmed.chars().take_while(|ch| ch.is_ascii_digit()).count();
    if digit_count == 0 {
        return None;
    }

    let rest = trimmed[digit_count..].trim_start();
    let rest = rest
        .strip_prefix(". ")
        .or_else(|| rest.strip_prefix(") "))
        .or_else(|| rest.strip_prefix(".\t"))
        .or_else(|| rest.strip_prefix(")\t"))?;
    let task = normalize_task(rest);
    (!task.is_empty()).then_some(task)
}

fn fence_open(trimmed: &str) -> Option<(char, usize)> {
    let fence_char = trimmed.chars().next()?;
    if fence_char != '`' && fence_char != '~' {
        return None;
    }
    let fence_len = trimmed.chars().take_while(|ch| *ch == fence_char).count();
    (fence_len >= 3).then_some((fence_char, fence_len))
}

fn fence_close(trimmed: &str, fence_char: char, fence_len: usize) -> bool {
    if !trimmed.starts_with(fence_char) {
        return false;
    }
    let close_len = trimmed.chars().take_while(|ch| *ch == fence_char).count();
    close_len >= fence_len && trimmed[close_len..].trim().is_empty()
}

pub fn normalize_task(task: &str) -> String {
    task.trim().trim_start_matches('❯').trim().to_string()
}

pub fn parse_dag_task_line(task: &str, index: usize) -> Result<DagTask> {
    let normalized = normalize_task(task);
    if normalized.is_empty() {
        anyhow::bail!("dag task {} is empty", index + 1);
    }

    let (metadata, prompt) = split_dag_metadata(&normalized)?;
    if prompt.is_empty() {
        anyhow::bail!("dag task {} is missing a prompt", index + 1);
    }

    let prompt_id = extract_prompt_task_id(&prompt);
    let id = metadata
        .id
        .or(prompt_id)
        .unwrap_or_else(|| format!("step-{}", index + 1));

    Ok(DagTask {
        id,
        prompt,
        deps: metadata.after,
    })
}

fn split_dag_metadata(task: &str) -> Result<(DagMetadata, String)> {
    let trimmed = task.trim();
    let Some(rest) = trimmed.strip_prefix('[') else {
        return Ok((DagMetadata::default(), trimmed.to_string()));
    };

    let closing = rest
        .find(']')
        .ok_or_else(|| anyhow::anyhow!("dag task metadata is missing closing `]`"))?;
    let metadata_text = &rest[..closing];
    let prompt = rest[closing + 1..].trim().to_string();
    let metadata = parse_dag_metadata(metadata_text)?;
    Ok((metadata, prompt))
}

fn parse_dag_metadata(metadata: &str) -> Result<DagMetadata> {
    let mut parsed = DagMetadata::default();
    for token in metadata.split_whitespace() {
        if let Some(value) = token.strip_prefix("after=") {
            parsed.after = parse_dependency_list(value);
            continue;
        }
        if let Some(value) = token.strip_prefix("deps=") {
            parsed.after = parse_dependency_list(value);
            continue;
        }
        if let Some(value) = token.strip_prefix("id=") {
            let value = value.trim();
            if value.is_empty() {
                anyhow::bail!("dag task metadata has empty `id=`");
            }
            parsed.id = Some(value.to_string());
            continue;
        }
        if parsed.id.is_none() {
            parsed.id = Some(token.to_string());
            continue;
        }
        anyhow::bail!("unsupported dag task metadata token `{}`", token);
    }
    Ok(parsed)
}

fn parse_dependency_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|dep| !dep.is_empty())
        .map(str::to_string)
        .collect()
}

fn extract_prompt_task_id(prompt: &str) -> Option<String> {
    let bytes = prompt.as_bytes();
    let mut idx = 0usize;
    while idx < bytes.len() {
        if bytes[idx] == b'#' {
            let start = idx;
            idx += 1;
            while idx < bytes.len() {
                let ch = bytes[idx] as char;
                if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                    idx += 1;
                } else {
                    break;
                }
            }
            if idx > start + 1 {
                return Some(prompt[start..idx].to_string());
            }
            continue;
        }
        idx += 1;
    }
    None
}

pub fn plan_dag_execution(tasks: &[DagTask]) -> Result<Vec<ExecutionTask>> {
    let mut ids = HashSet::new();
    for task in tasks {
        if !ids.insert(task.id.clone()) {
            anyhow::bail!("duplicate dag task id `{}`", task.id);
        }
    }

    for task in tasks {
        for dep in &task.deps {
            if !ids.contains(dep) {
                anyhow::bail!("dag task `{}` depends on unknown task `{}`", task.id, dep);
            }
        }
    }

    let mut completed = HashSet::new();
    let mut remaining = (0..tasks.len()).collect::<Vec<_>>();
    let mut ordered = Vec::with_capacity(tasks.len());

    while !remaining.is_empty() {
        let mut advanced = false;
        let mut cursor = 0usize;

        while cursor < remaining.len() {
            let idx = remaining[cursor];
            let task = &tasks[idx];
            if task.deps.iter().all(|dep| completed.contains(dep)) {
                let task = tasks[idx].clone();
                completed.insert(task.id.clone());
                ordered.push(ExecutionTask {
                    label: format!("[{}] {}", task.id, task.prompt),
                    prompt: task.prompt,
                });
                remaining.remove(cursor);
                advanced = true;
            } else {
                cursor += 1;
            }
        }

        if !advanced {
            let blocked = remaining
                .iter()
                .map(|idx| tasks[*idx].id.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            anyhow::bail!("dag dependency cycle detected among: {}", blocked);
        }
    }

    Ok(ordered)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_tasks_prefers_last_fenced_list() {
        let text = "Notes\n\n- old one\n\n```md\n- do first\n- do second\n```\n";
        assert_eq!(
            extract_tasks_from_text(text),
            vec!["do first".to_string(), "do second".to_string()]
        );
    }

    #[test]
    fn extract_tasks_uses_last_markdown_list() {
        let text = "alpha\n\n- first\n- second\n\nTail\n";
        assert_eq!(
            extract_tasks_from_text(text),
            vec!["first".to_string(), "second".to_string()]
        );
    }

    #[test]
    fn resolve_dag_tasks_supports_fan_in_dependencies() {
        let tasks = [
            "do #prep. Prepare context",
            "[after=#prep] do #bench. Run benchmarks",
            "[id=report after=#prep,#bench] Summarize both results",
        ];

        let parsed = tasks
            .iter()
            .enumerate()
            .map(|(idx, task)| parse_dag_task_line(task, idx).unwrap())
            .collect::<Vec<_>>();

        assert_eq!(parsed[0].id, "#prep");
        assert!(parsed[0].deps.is_empty());
        assert_eq!(parsed[1].id, "#bench");
        assert_eq!(parsed[1].deps, vec!["#prep".to_string()]);
        assert_eq!(parsed[2].id, "report");
        assert_eq!(
            parsed[2].deps,
            vec!["#prep".to_string(), "#bench".to_string()]
        );
        assert_eq!(parsed[2].prompt, "Summarize both results");
    }

    #[test]
    fn dag_schedule_rejects_unknown_dependency() {
        let tasks = vec![DagTask {
            id: "#prep".to_string(),
            prompt: "do #prep".to_string(),
            deps: vec!["#missing".to_string()],
        }];

        let err = plan_dag_execution(&tasks).unwrap_err().to_string();
        assert!(
            err.contains("unknown task `#missing`"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn dag_schedule_rejects_cycles() {
        let tasks = vec![
            DagTask {
                id: "#a".to_string(),
                prompt: "do #a".to_string(),
                deps: vec!["#b".to_string()],
            },
            DagTask {
                id: "#b".to_string(),
                prompt: "do #b".to_string(),
                deps: vec!["#a".to_string()],
            },
        ];

        let err = plan_dag_execution(&tasks).unwrap_err().to_string();
        assert!(
            err.contains("dag dependency cycle detected"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn parse_list_item_strips_prompt_prefix() {
        let result = parse_list_item("❯ - do #task1");
        assert_eq!(result, Some("do #task1".to_string()));
    }

    #[test]
    fn parse_list_item_strips_prompt_prefix_with_star() {
        let result = parse_list_item("❯ * do #task2");
        assert_eq!(result, Some("do #task2".to_string()));
    }

    #[test]
    fn exchange_task_source_fingerprint_detects_list_mutations() {
        let original = ExchangeTaskSourceFingerprint {
            tasks: vec!["do #first".to_string(), "do #second".to_string()],
            requested_presets: vec!["#spec".to_string()],
        };
        let source = find_exchange_task_source(
            "sync orchestra\npreset #spec\n- do #first\n- do #second\n<!-- agent:boundary:keep -->\n",
            &original,
        )
        .unwrap();
        assert_eq!(source, original);

        let boundary_only = find_exchange_task_source(
            "sync orchestra\npreset #spec\n- do #first\n- do #second\n<!-- agent:boundary:new -->\n",
            &source,
        )
        .unwrap();
        assert_eq!(boundary_only, source);

        let inserted = find_exchange_task_source(
            "sync orchestra\npreset #spec\n- do #first\n- do #inserted\n- do #second\n",
            &source,
        )
        .unwrap();
        assert_ne!(inserted, source);

        let reordered = find_exchange_task_source(
            "sync orchestra\npreset #spec\n- do #second\n- do #first\n",
            &source,
        );
        assert!(reordered.is_none());

        let quoted_later = find_exchange_task_source(
            "sync orchestra\npreset #spec\n- do #first\n- do #second\n\n### Re: response - gpt-5\n\n- do #first\n- do #extra\n- do #second\n",
            &source,
        )
        .unwrap();
        assert_eq!(quoted_later, source);
    }

    #[test]
    fn scope_exchange_tail_returns_appended_non_marker_lines() {
        let snapshot = "old\n<!-- agent:boundary:abc -->\n";
        let current = "old\n<!-- agent:boundary:def -->\n<!-- comment -->\ndo #new\n";
        assert_eq!(scope_exchange_tail(current, snapshot), "do #new");
    }

    #[test]
    fn apply_prompt_preset_block_prefixes_task_prompt() {
        let rendered = apply_prompt_preset_block(
            "do #prep",
            Some("(preset #1)\nToday is 2026-04-25.\nKeep the work tree clean.\n"),
        );
        assert_eq!(
            rendered,
            "(preset #1)\nToday is 2026-04-25.\nKeep the work tree clean.\ndo #prep"
        );
    }

    #[test]
    fn append_worker_result_line_inserts_before_template_exchange_close() {
        let mode = ResolvedMode {
            format: agent_doc_frontmatter::frontmatter::AgentDocFormat::Template,
            write: agent_doc_frontmatter::frontmatter::AgentDocWrite::Merge,
        };
        let response = "<!-- patch:exchange -->\nbody\n<!-- /patch:exchange -->\n";

        let updated = append_worker_result_line(response, "[worker] done", mode);

        assert_eq!(
            updated,
            "<!-- patch:exchange -->\nbody\n[worker] done\n<!-- /patch:exchange -->\n"
        );
    }
}
