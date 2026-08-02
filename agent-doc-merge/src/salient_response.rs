//! Cycle-keyed non-final response projection.

use agent_doc_element::element;
use agent_doc_markdown_ast::exchange_tree::{
    salient_response_materialized as salient_inner_materialized, upsert_salient_response,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SalientResponseUpsertOutcome {
    pub content: String,
    pub cell_id: String,
    pub applied: bool,
}

pub fn upsert_salient_response_node(
    doc: &str,
    cycle_id: &str,
    body: &str,
) -> anyhow::Result<SalientResponseUpsertOutcome> {
    let components = element::parse(doc)?;
    let exchange = components
        .iter()
        .find(|component| component.name == "exchange")
        .ok_or_else(|| anyhow::anyhow!("document has no agent:exchange component"))?;
    let current = exchange.content(doc);
    let next = upsert_salient_response(current, cycle_id, body);
    let applied = next != current;
    Ok(SalientResponseUpsertOutcome {
        content: if applied {
            exchange.replace_content(doc, &next)
        } else {
            doc.to_string()
        },
        cell_id: format!("s:{cycle_id}"),
        applied,
    })
}

pub fn salient_response_materialized(
    doc: &str,
    cycle_id: &str,
    body: &str,
) -> anyhow::Result<bool> {
    let components = element::parse(doc)?;
    let exchange = components
        .iter()
        .find(|component| component.name == "exchange")
        .ok_or_else(|| anyhow::anyhow!("document has no agent:exchange component"))?;
    Ok(salient_inner_materialized(
        exchange.content(doc),
        cycle_id,
        body,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "---\nagent_doc_format: template\n---\n\
<!-- agent:exchange -->\n❯ Investigate.\n<!-- /agent:exchange -->\n";

    #[test]
    fn append_replace_and_replay_are_cycle_keyed() {
        let first = upsert_salient_response_node(DOC, "cycle-1", "First.").unwrap();
        assert!(first.applied);
        assert!(salient_response_materialized(&first.content, "cycle-1", "First.").unwrap());

        let replay = upsert_salient_response_node(&first.content, "cycle-1", "First.").unwrap();
        assert!(!replay.applied);
        assert_eq!(replay.content, first.content);

        let replaced =
            upsert_salient_response_node(&first.content, "cycle-1", "Confirmed.").unwrap();
        assert!(replaced.applied);
        assert!(!replaced.content.contains("First."));
        assert!(replaced.content.contains("Confirmed."));
    }
}
