//! Blocker-first response for a stopped queue row (`#blockerfront`).
//!
//! A blocked attempt is not a completion report. Its response structure still
//! has to lead with the blocker reason: once in `agent:exchange`, and again
//! as the first generated child under the live row. Compact Exchange reads
//! that reason back from either shape.

use agent_doc_element::element::{self, Component};
use agent_doc_hash::short_content_hash;
use anyhow::Context;

use crate::queue_projection::strip_in_progress_marker;

/// One blocked row's response. The exchange cell and the queue children share
/// `blocker`, so a retry finds the reason already written and does not add a
/// second copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockedRowResponse {
    pub item: String,
    pub blocker: String,
    pub summary: String,
    pub state: String,
    pub guidance: String,
    pub next_actions: Vec<String>,
    /// `[#r-` plus at least 12 hex digits.
    pub response_id: String,
    /// `YYYY-MM-DDTHH:MM:SS.000Z`.
    pub timestamp: String,
}

/// Stable id for one blocked attempt. The reason is part of the id so a new
/// blocker is a new response, and the same blocker retries as the same one.
pub fn blocked_response_id(owner: &str, generation: u64, blocker: &str) -> String {
    format!(
        "[#r-{}]",
        short_content_hash(&format!("blocked\n{owner}\n{generation}\n{blocker}"))
    )
}

/// Format `secs` since the Unix epoch as `YYYY-MM-DDTHH:MM:SS.000Z`.
pub fn format_utc_timestamp(secs: u64) -> String {
    let days = secs / 86_400;
    let time_of_day = secs % 86_400;
    let hours = time_of_day / 3_600;
    let minutes = (time_of_day % 3_600) / 60;
    let seconds = time_of_day % 60;
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}.000Z")
}

/// Write the blocker-first response into every matching live queue row and
/// into `agent:exchange`, when those components are present. A second call
/// with the same blocker leaves the document unchanged.
pub fn project_blocked_row_response(
    doc: &str,
    response: &BlockedRowResponse,
) -> anyhow::Result<String> {
    let components = element::parse(doc).context("blocked response document did not parse")?;
    let mut targets: Vec<Component> = components
        .into_iter()
        .filter(|component| component.name == "exchange" || component.name == "queue")
        .collect();
    targets.sort_by_key(|component| std::cmp::Reverse(component.open_start));
    let mut updated = doc.to_string();
    for component in targets {
        let body = component.content(&updated);
        let next = if component.name == "exchange" {
            upsert_exchange_cell(body, response)
        } else {
            upsert_queue_rows(body, response)
        };
        if next != body {
            updated = component.replace_content(&updated, &next);
        }
    }
    Ok(updated)
}

fn upsert_exchange_cell(body: &str, response: &BlockedRowResponse) -> String {
    let blocker = response.blocker.trim();
    if blocker.is_empty()
        || body.contains(&format!("**Blocker**\n\n{blocker}"))
        || body.contains(&format!("Blocked: {blocker}"))
    {
        return body.to_string();
    }
    let cell = render_exchange_cell(response);
    let insert_at = body.rfind("<!-- agent:boundary:").unwrap_or(body.len());
    let mut result = String::with_capacity(body.len() + cell.len() + 2);
    result.push_str(&body[..insert_at]);
    if !result.is_empty() && !result.ends_with('\n') {
        result.push('\n');
    }
    if !result.is_empty() && !result.ends_with("\n\n") {
        result.push('\n');
    }
    result.push_str(cell.trim_end());
    result.push('\n');
    result.push_str(&body[insert_at..]);
    result
}

fn render_exchange_cell(response: &BlockedRowResponse) -> String {
    let item = collapse_whitespace(&response.item);
    let mut cell = format!(
        "### Re: Blocked: {item} — agent-doc\n\n**Blocker**\n\n{}\n\n**Summary**\n\n{}",
        response.blocker.trim(),
        response.summary.trim(),
    );
    if !response.next_actions.is_empty() {
        cell.push_str("\n\n**Next actions**\n\n");
        for action in &response.next_actions {
            let action = collapse_whitespace(action);
            if action.is_empty() {
                continue;
            }
            cell.push_str("- ");
            cell.push_str(&action);
            cell.push('\n');
        }
    }
    cell
}

fn upsert_queue_rows(body: &str, response: &BlockedRowResponse) -> String {
    let blocker = response.blocker.trim();
    if blocker.is_empty() || response.item.trim().is_empty() {
        return body.to_string();
    }
    let lines = split_lines(body);
    let needle = format!("**Blocker:** {blocker}");
    let mut out = String::with_capacity(body.len() + 512);
    let mut index = 0;
    while index < lines.len() {
        let line = &lines[index];
        out.push_str(line);
        let bare = line.trim_end_matches(['\n', '\r']);
        if let Some(work) = list_item_work(bare)
            && same_queue_work(&work, &response.item)
            && !descendants_contain(&lines, index, leading_spaces(bare), &needle)
        {
            out.push_str(&render_queue_children(leading_spaces(bare), response));
        }
        index += 1;
    }
    out
}

fn descendants_contain(lines: &[String], row: usize, row_indent: usize, needle: &str) -> bool {
    let mut index = row + 1;
    while index < lines.len() {
        let bare = lines[index].trim_end_matches(['\n', '\r']);
        if bare.trim().is_empty() {
            index += 1;
            continue;
        }
        if leading_spaces(bare) <= row_indent {
            break;
        }
        if bare.contains(needle) {
            return true;
        }
        index += 1;
    }
    false
}

fn render_queue_children(indent: usize, response: &BlockedRowResponse) -> String {
    let child = " ".repeat(indent + 2);
    let grand = " ".repeat(indent + 4);
    let action_indent = " ".repeat(indent + 6);
    let mut block = String::new();
    block.push_str(&child);
    block.push_str("- **Response ");
    block.push_str(response.response_id.trim());
    block.push_str("**\n");
    for (label, value) in [
        ("Blocker", response.blocker.trim()),
        ("Timestamp", response.timestamp.trim()),
        ("Summary", response.summary.trim()),
        ("State", response.state.trim()),
        ("Guidance", response.guidance.trim()),
    ] {
        if value.is_empty() {
            continue;
        }
        block.push_str(&grand);
        block.push_str("- **");
        block.push_str(label);
        block.push_str(":** ");
        block.push_str(&collapse_whitespace(value));
        block.push('\n');
    }
    let actions: Vec<String> = response
        .next_actions
        .iter()
        .map(|action| collapse_whitespace(action))
        .filter(|action| !action.is_empty())
        .collect();
    if !actions.is_empty() {
        block.push_str(&grand);
        block.push_str("- **Next actions:**\n");
        for action in actions {
            block.push_str(&action_indent);
            block.push_str("- **Action:** ");
            block.push_str(&action);
            block.push('\n');
        }
    }
    block
}

fn list_item_work(line: &str) -> Option<String> {
    let content = line.trim_start();
    let content = content
        .strip_prefix("- ")
        .or_else(|| content.strip_prefix("* "))?;
    Some(strip_in_progress_marker(content))
}

/// Work text matches when both sides name the same row. A trailing `[#q-…]`
/// id is row identity, not different work.
fn same_queue_work(row: &str, item: &str) -> bool {
    fn core(text: &str) -> &str {
        let text = text.trim();
        let Some((work, id)) = text.rsplit_once("[#q-") else {
            return text;
        };
        let id = id.strip_suffix(']').unwrap_or("");
        if id.is_empty() || id.chars().any(char::is_whitespace) {
            return text;
        }
        work.trim()
    }
    let item = item.trim();
    !item.is_empty() && core(row) == core(item)
}

fn leading_spaces(line: &str) -> usize {
    line.chars().take_while(|ch| *ch == ' ').count()
}

fn split_lines(body: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut rest = body;
    while let Some(index) = rest.find('\n') {
        let (line, tail) = rest.split_at(index + 1);
        lines.push(line.to_string());
        rest = tail;
    }
    if !rest.is_empty() {
        lines.push(rest.to_string());
    }
    lines
}

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn civil_from_days(days: u64) -> (i64, u32, u32) {
    let mut year = 1970i64;
    let mut remaining = days as i64;
    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        year += 1;
    }
    let month_days: [i64; 12] = if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut month = 0u32;
    for days_in_month in month_days {
        if remaining < days_in_month {
            break;
        }
        remaining -= days_in_month;
        month += 1;
    }
    (year, month + 1, (remaining + 1) as u32)
}

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(blocker: &str) -> BlockedRowResponse {
        BlockedRowResponse {
            item: "finish sdk".to_string(),
            blocker: blocker.to_string(),
            summary: "Recorded as blocked by quota, not lost.".to_string(),
            state: "Blocked by quota.".to_string(),
            guidance: "Resume the same worker or requeue. Do not dispatch another worker on the exhausted harness.".to_string(),
            next_actions: vec!["resume via queue start".to_string(), "queue release --requeue".to_string()],
            response_id: blocked_response_id("subagent:sdk", 1, blocker),
            timestamp: format_utc_timestamp(1_709_251_199),
        }
    }

    fn document() -> String {
        concat!(
            "<!-- agent:exchange -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- agent:boundary:keep -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- ⛔ [#subagent:sdk#1] finish sdk\n",
            "- finish dev\n",
            "<!-- /agent:queue -->\n",
        )
        .to_string()
    }

    #[test]
    fn utc_timestamp_formats_the_leap_second_boundary_day() {
        assert_eq!(format_utc_timestamp(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(
            format_utc_timestamp(1_709_251_199),
            "2024-02-29T23:59:59.000Z"
        );
    }

    #[test]
    fn a_blocked_row_keeps_the_blocker_in_front_of_summary_in_both_places() {
        let reason = "Attempt generation 1 of \"finish sdk\" stopped on the session limit of Claude Code (`You've hit your session limit · resets 7:30pm`) before the cross-platform release workflow could publish.";
        assert!(reason.chars().count() > 120, "{}", reason.chars().count());
        let response = response(reason);
        let once = project_blocked_row_response(&document(), &response).unwrap();
        let twice = project_blocked_row_response(&once, &response).unwrap();
        assert_eq!(twice, once, "a second projection must not add another cell");

        let exchange = component_body(&once, "exchange");
        let queue = component_body(&once, "queue");
        let exchange_blocker = exchange.find("**Blocker**").expect("exchange blocker");
        let exchange_summary = exchange.find("**Summary**").expect("exchange summary");
        assert!(exchange_blocker < exchange_summary, "{exchange}");
        assert!(exchange.contains(reason), "{exchange}");
        assert!(exchange.contains("### Re: Blocked: finish sdk — agent-doc"));
        assert!(
            exchange.find("<!-- agent:boundary:").unwrap() > exchange_blocker,
            "the cell belongs on the historical side of the boundary"
        );

        let row_blocker = queue.find("**Blocker:**").expect("queue blocker");
        let row_summary = queue.find("**Summary:**").expect("queue summary");
        let row_timestamp = queue.find("**Timestamp:**").expect("queue timestamp");
        assert!(row_blocker < row_timestamp, "{queue}");
        assert!(row_blocker < row_summary, "{queue}");
        assert!(queue.contains(reason), "{queue}");
        assert!(queue.contains(&format!("**Response {}**", response.response_id)));
        assert!(queue.contains("- finish dev\n"), "the sibling row stays");
    }

    fn component_body<'a>(doc: &'a str, name: &str) -> &'a str {
        let components = element::parse(doc).unwrap();
        let component = components
            .iter()
            .find(|component| component.name == name)
            .unwrap();
        component.content(doc)
    }
}
