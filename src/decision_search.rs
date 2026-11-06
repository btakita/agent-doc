//! `agent-doc decision search` (`#decisionsearchmenu`): look an operator
//! answer up before asking again.

use anyhow::Result;
use std::path::Path;

use agent_doc_preflight_io::answer_ledger::{DecisionSearchReport, search_decisions};
use agent_doc_preflight_io::answer_recall::RecallEnv;

pub fn run_search(file: &Path, query: &str, limit: usize, json: bool) -> Result<()> {
    let display = file.display().to_string();
    let report = search_decisions(file, &display, query, &RecallEnv::from_process(), limit)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        for line in render(&report) {
            println!("{line}");
        }
    }
    Ok(())
}

fn when(secs: Option<u64>) -> String {
    secs.filter(|secs| *secs > 0)
        .map(|secs| format!("{} UTC", agent_doc_log_time::format_human_timestamp(secs)))
        .unwrap_or_else(|| "first-seen time unknown".to_string())
}

pub fn render(report: &DecisionSearchReport) -> Vec<String> {
    let mut lines = Vec::new();
    if report.hits.is_empty() {
        lines.push(format!(
            "no answered decision in {} matches “{}”: it has not been answered here; ask the operator, and say you searched",
            report.document, report.query
        ));
    }
    for (rank, hit) in report.hits.iter().enumerate() {
        let elsewhere = if hit.document == report.document {
            String::new()
        } else {
            format!(" in {}", hit.document)
        };
        lines.push(format!(
            "{}. `{}`{} [{}{}] {}",
            rank + 1,
            hit.decision_id,
            elsewhere,
            hit.matched.as_str(),
            if hit.score < 1.0 {
                format!(" {:.2}", hit.score)
            } else {
                String::new()
            },
            hit.question
        ));
        lines.push(format!("   answer: “{}”", hit.answer));
        lines.push(format!(
            "   given: {} · {}",
            hit.location,
            when(hit.first_seen_at)
        ));
        if !hit.earlier_answers.is_empty() {
            lines.push(format!(
                "   earlier: {}",
                hit.earlier_answers
                    .iter()
                    .map(|answer| format!("“{answer}”"))
                    .collect::<Vec<_>>()
                    .join(" -> ")
            ));
        }
        if !hit.open_items.is_empty() {
            lines.push(format!(
                "   open copies: {}",
                hit.open_items
                    .iter()
                    .map(|item| format!("#{item}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    lines.push(report.recall.summary());
    lines
}
