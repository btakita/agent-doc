//! `#nestleafselect`: an answered Waiting-on-you item with two operator
//! replies. Observed 2026-11-15 00:23 on the lazily session (binary
//! 2fe011082c): the operator added `- try again` under `[#lzpypidevrevoke]`,
//! whose older reply sat deep under generated `**Next actions:**`. The item
//! was pending subagent dispatch, so preflight's selection filter dropped the
//! item row but kept its packet. The orphaned replies became leaves, and the
//! older one was submitted as `(this leaf)`.

use crate::document_queue::{
    self, LATEST_REPLY_LABEL, QueueEntry, row_matches_submitted_head_text, without_binary_markers,
};
use crate::queue_response::submitted_prompts_with_context_for_heads;
use crate::waiting_packet::without_rows_and_their_packets;

const ITEM: &str = "[#lzpypidevrevoke] Remove the old PyPI API token \"dev\". (source: agent:review [#lzambienttokenresolve])";
const OLD_REPLY: &str = "remove the old PyPI API token for dev.";
const NEW_REPLY: &str = "try again";

fn head() -> String {
    format!(
        "\u{27e6}\u{1f4e5} you: 0 \u{b7} agent: 1 \u{b7} done: 0\u{27e7} {ITEM} <!-- agent-doc:waiting-item -->"
    )
}

/// The observed waiting tree, with the older reply nested under generated
/// `**Next actions:**` and the newer one a direct child of the item.
fn document() -> String {
    format!(
        concat!(
            "---\nagent_doc_session: x\n---\n\n",
            "## Run <!-- agent:run go -->\n",
            "- ### Waiting on you \u{2014} answer by adding a child line under an item <!-- agent-doc:waiting-on-you -->\n",
            "  - {head}\n",
            "    - **Next:** agent \u{2014} pick up your reply \u{201c}{old}\u{201d} and attempt `#lzpypidevrevoke` (evidence: your reply) \u{25ab}\u{fe0f}\n",
            "    - **Response:** {item} \u{25ab}\u{fe0f}\n",
            "      - **Timestamp:** 2026-11-13T09:36-05:00 \u{25ab}\u{fe0f}\n",
            "      - **Question:** What decision or completion result should be recorded? \u{25ab}\u{fe0f}\n",
            "      - **Next actions:** \u{25ab}\u{fe0f}\n",
            "        - **Action:** {item} \u{25ab}\u{fe0f}\n",
            "        - {old}\n",
            "    - {new}\n",
            "- ### do [#other]\n",
            "<!-- /agent:run -->\n",
        ),
        head = head(),
        item = ITEM,
        old = OLD_REPLY,
        new = NEW_REPLY,
    )
}

fn entries(doc: &str) -> Vec<QueueEntry> {
    let comp = agent_doc_element::element::parse(doc)
        .unwrap()
        .into_iter()
        .find(|c| c.name == "queue")
        .unwrap();
    document_queue::parse(comp.content(doc)).unwrap()
}

fn index_of(entries: &[QueueEntry], needle: &str) -> usize {
    entries
        .iter()
        .position(|entry| match entry {
            QueueEntry::Prompt(prompt) => prompt.text.contains(needle),
            _ => false,
        })
        .unwrap_or_else(|| panic!("no row containing {needle:?}: {entries:#?}"))
}

fn executable_texts(entries: &[QueueEntry]) -> Vec<String> {
    document_queue::executable_prompt_indexes(entries)
        .into_iter()
        .map(|index| match &entries[index] {
            QueueEntry::Prompt(prompt) => prompt.text.clone(),
            other => panic!("{other:?}"),
        })
        .collect()
}

#[test]
fn dropping_a_dispatched_waiting_item_drops_its_replies_too() {
    let entries = entries(&document());
    let item = index_of(&entries, "<!-- agent-doc:waiting-item -->");
    assert_eq!(
        executable_texts(&entries),
        vec![head(), "do [#other]".to_string()],
        "an answered item is the one selectable identity"
    );

    let mut drop = vec![false; entries.len()];
    drop[item] = true;

    // The pre-fix filter: drop only the item row. Its replies become leaves
    // and the older reply is the first one (the observed wrong leaf).
    let naive: Vec<QueueEntry> = entries
        .iter()
        .zip(&drop)
        .filter(|(_, dropped)| !**dropped)
        .map(|(entry, _)| entry.clone())
        .collect();
    assert_eq!(
        executable_texts(&naive).first().map(String::as_str),
        Some(OLD_REPLY),
        "fixture must reproduce the orphaned-reply selection"
    );

    let filtered = without_rows_and_their_packets(&entries, &drop);
    assert_eq!(
        executable_texts(&filtered),
        vec!["do [#other]".to_string()],
        "no packet row may surface as a leaf once its item is out of the loop"
    );
    assert!(
        filtered.iter().all(|entry| match entry {
            QueueEntry::Prompt(prompt) => prompt.text != OLD_REPLY && prompt.text != NEW_REPLY,
            _ => true,
        }),
        "{filtered:#?}"
    );
}

#[test]
fn submitted_waiting_item_names_its_newest_reply_and_strips_binary_markers() {
    let doc = document();
    let submitted = submitted_prompts_with_context_for_heads(&doc, &[head()]);
    let (_, body) = agent_doc_prompt_context::lineage::split(&submitted[0]);
    let mut lines = body.lines();
    let vector = lines.next().unwrap();
    assert_eq!(
        vector,
        format!("Waiting on you \u{2014} answer by adding a child line under an item: {ITEM}"),
        "the vector keeps the item identity and drops binary markers: {body}"
    );
    assert_eq!(
        lines.next().unwrap(),
        format!("{LATEST_REPLY_LABEL} {NEW_REPLY}"),
        "{body}"
    );
    assert!(
        body.contains(&format!(">     - {NEW_REPLY} (latest reply)")),
        "{body}"
    );
    assert!(
        body.contains(&format!(">         - {OLD_REPLY}\n")),
        "the older reply stays context: {body}"
    );
    assert!(
        !body.contains(&format!("{OLD_REPLY} (this leaf)")),
        "{body}"
    );
    assert!(
        body.contains("<!-- agent-doc:waiting-item --> (this leaf)"),
        "{body}"
    );
}

#[test]
fn an_inert_reply_under_generated_metadata_is_in_the_tree_and_can_be_latest() {
    // The newest reply typed under the generated `**Choices:**` row parses
    // inert; it is still the operator's reply and the one to act on.
    let doc = document().replace(
        &format!("    - {NEW_REPLY}\n"),
        concat!(
            "    - **Choices:** \u{25ab}\u{fe0f}\n",
            "      - **Retry**: try the removal again \u{25ab}\u{fe0f}\n",
            "        - retry with the fresh login\n",
        ),
    );
    let entries = entries(&doc);
    let item = index_of(&entries, "<!-- agent-doc:waiting-item -->");
    let latest = crate::waiting_packet::latest_operator_reply(&entries, item);
    assert_eq!(
        latest.as_ref().map(|(_, text)| text.as_str()),
        Some("retry with the fresh login"),
        "{entries:#?}"
    );
    let submitted = submitted_prompts_with_context_for_heads(&doc, &[head()]);
    assert!(
        submitted[0].contains(&format!("{LATEST_REPLY_LABEL} retry with the fresh login")),
        "{}",
        submitted[0]
    );
    assert!(
        submitted[0].contains(">         - retry with the fresh login (latest reply)"),
        "{}",
        submitted[0]
    );
}

#[test]
fn a_struck_reply_is_not_the_latest() {
    let doc = document().replace(
        &format!("    - {NEW_REPLY}\n"),
        &format!("    - ~~{NEW_REPLY}~~\n"),
    );
    let entries = entries(&doc);
    let item = index_of(&entries, "<!-- agent-doc:waiting-item -->");
    assert_eq!(
        crate::waiting_packet::latest_operator_reply(&entries, item).map(|(_, text)| text),
        Some(OLD_REPLY.to_string())
    );
}

#[test]
fn binary_markers_strip_without_touching_operator_text() {
    assert_eq!(
        without_binary_markers(
            "Waiting on you <!-- agent-doc:waiting-on-you -->: [#a] Pick a port. <!-- agent-doc:waiting-item -->: reply"
        ),
        "Waiting on you: [#a] Pick a port.: reply"
    );
    assert_eq!(
        without_binary_markers("a <!-- agent-doc:x --> b <!-- note --> c : d"),
        "a b <!-- note --> c : d"
    );
    assert_eq!(without_binary_markers("plain : text"), "plain : text");
    assert_eq!(
        without_binary_markers("open <!-- agent-doc:unterminated"),
        "open <!-- agent-doc:unterminated"
    );
}

#[test]
fn a_marker_free_vector_still_resolves_its_row() {
    let doc = concat!(
        "- Waiting on you <!-- agent-doc:waiting-on-you -->\n",
        "  - parent row <!-- agent-doc:waiting-on-you -->\n",
        "    - leaf\n",
    );
    let entries = document_queue::parse(doc).unwrap();
    let leaf = index_of(&entries, "leaf");
    let vector = document_queue::nest_vector_at(&entries, leaf, "leaf").unwrap();
    assert!(vector.contains("<!-- agent-doc:"), "{vector}");
    assert!(row_matches_submitted_head_text(&entries, leaf, &vector));
    assert!(row_matches_submitted_head_text(
        &entries,
        leaf,
        &without_binary_markers(&vector)
    ));
    assert!(!row_matches_submitted_head_text(
        &entries,
        leaf,
        "Waiting on you: parent row: other leaf"
    ));
}
