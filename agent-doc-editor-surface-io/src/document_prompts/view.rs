//! Install a host-neutral Lazily View for an authoritative document snapshot.
//! The browser and editor hosts lower the same resolved view into their native controls.

use super::Target;
use anyhow::{Result, anyhow};
use lazily::{Context, view::View};
use serde_json::{Map, Value, json};
use std::collections::HashMap;

const DEFINITION: &str = include_str!("document-explorer-view.json");

pub(super) struct Presentation {
    pub rows: Vec<Value>,
    pub controls: Value,
}

pub(super) fn project(targets: &[Target]) -> Result<Presentation> {
    let mut definition: Value = serde_json::from_str(DEFINITION)?;
    let nodes = definition["nodes"]
        .as_object_mut()
        .ok_or_else(|| anyhow!("Document Explorer view must define nodes"))?;

    // View node ids are scoped to this projection. Target handles sent over
    // the document protocol keep their existing identity and revision fence.
    let mut lineage: Vec<(usize, String)> = Vec::new();
    for (index, target) in targets.iter().enumerate() {
        while lineage
            .last()
            .is_some_and(|(depth, _)| *depth >= target.depth)
        {
            lineage.pop();
        }
        let parent = lineage.last().map_or("outline", |(_, id)| id.as_str());
        let id = format!("row:{index}");
        children(nodes, parent)?.push(json!(id));
        nodes.insert(
            id.clone(),
            json!({"role": "button", "text": target.label, "children": []}),
        );
        if let Some(task) = &target.task {
            let task_id = format!("task:{index}");
            children(nodes, &id)?.push(json!(task_id));
            nodes.insert(task_id, json!({"role": "text", "text": task.state.label()}));
        }
        lineage.push((target.depth, id));
    }

    // The document snapshot is already authoritative. The View context lives
    // for this synchronous projection and is never an additional editor session.
    let context = Context::new();
    let view = View::install(&context, &definition).map_err(|err| anyhow!("{err}"))?;
    let expressions = view.expressions_json(&context);
    let layout = view.layout_json();
    let rows = lower_outline(targets, &layout, &expressions)?;

    let mut prompt_actions = Map::new();
    for action in ["exchange", "queue", "subprompt", "navigate"] {
        let button = format!("prompt-{action}");
        prompt_actions.insert(
            action.into(),
            json!({
                "label": expression(&expressions, &button, "text")?,
                "disabled": expression(&expressions, &button, "disabled")?,
                "hint": expression(&expressions, &format!("hint-{action}"), "text")?,
            }),
        );
    }
    let controls = json!({
        "title": expression(&expressions, "title", "text")?,
        "document_tab": expression(&expressions, "document-tab", "text")?,
        "repository_tab": expression(&expressions, "repository-tab", "text")?,
        "follow_editor_caret": expression(&expressions, "follow-editor-caret", "text")?,
        "linked_documents": expression(&expressions, "linked-documents", "text")?,
        "prompt_label": expression(&expressions, "prompt-label", "text")?,
        "prompt_actions": prompt_actions,
    });
    Ok(Presentation { rows, controls })
}

fn children<'a>(nodes: &'a mut Map<String, Value>, node: &str) -> Result<&'a mut Vec<Value>> {
    nodes
        .get_mut(node)
        .and_then(|node| node["children"].as_array_mut())
        .ok_or_else(|| anyhow!("Document Explorer view node {node} must have children"))
}

fn expression<'a>(expressions: &'a Value, node: &str, field: &str) -> Result<&'a Value> {
    expressions
        .get(format!("{node}.{field}"))
        .ok_or_else(|| anyhow!("Document Explorer view node {node} must define {field}"))
}

fn lower_outline(targets: &[Target], layout: &Value, expressions: &Value) -> Result<Vec<Value>> {
    let mut rows = Vec::with_capacity(targets.len());
    let mut depths = HashMap::<&str, usize>::new();
    let layout = layout
        .as_array()
        .ok_or_else(|| anyhow!("Document Explorer view layout must be an array"))?;
    for node in layout {
        let id = node["id"]
            .as_str()
            .ok_or_else(|| anyhow!("Document Explorer view layout node must have an id"))?;
        let row_index = id
            .strip_prefix("row:")
            .and_then(|index| index.parse::<usize>().ok());
        let child_depth = if let Some(index) = row_index {
            let depth = depths.remove(id).ok_or_else(|| {
                anyhow!("Document Explorer view row {id} must descend from the outline")
            })?;
            let target = &targets[index];
            let mut row = serde_json::to_value(target)?;
            row["depth"] = json!(depth);
            row["label"] = expression(expressions, id, "text")?.clone();
            if target.task.is_some() {
                row["task"]["label"] =
                    expression(expressions, &format!("task:{index}"), "text")?.clone();
            }
            rows.push(row);
            depth + 1
        } else if id == "outline" {
            0
        } else {
            continue;
        };
        let children = node["children"]
            .as_array()
            .ok_or_else(|| anyhow!("Document Explorer view layout node {id} must have children"))?;
        for child in children
            .iter()
            .filter_map(Value::as_str)
            .filter(|id| id.starts_with("row:"))
        {
            depths.insert(child, child_depth);
        }
    }
    Ok(rows)
}
