use std::collections::HashSet;

use agent_doc_document::queue_projection::{
    QueueLifecycleMarker, apply_lifecycle_marker, apply_operator_action_ids, lifecycle_marker,
    strip_priority_markers,
};
use agent_doc_element_backlog::backlog::{parse_items, set_in_progress_item_ids};
use agent_doc_element_queue::{QueueItemIdentity, queue_head_identity};
use agent_doc_queue_icons::{QueueIconConfig, install_queue_icon_config};

#[test]
fn multi_character_prefixes_render_parse_and_preserve_identity() {
    install_queue_icon_config(&QueueIconConfig {
        reported: Some("READY FOR COORDINATOR>".into()),
        running: Some("WORKING NOW>".into()),
        prioritized: Some("TOP::".into()),
        operator_action: Some("QUESTION::".into()),
        ..QueueIconConfig::default()
    })
    .unwrap();

    let rendered = apply_lifecycle_marker(
        "TOP:: do [#configurable-icons]",
        QueueLifecycleMarker::Reported,
    );
    assert_eq!(
        rendered,
        "READY FOR COORDINATOR> TOP:: do [#configurable-icons]"
    );
    assert_eq!(
        lifecycle_marker(&rendered),
        Some(QueueLifecycleMarker::Reported)
    );
    assert_eq!(
        strip_priority_markers(&rendered),
        "do [#configurable-icons]"
    );
    assert_eq!(
        queue_head_identity(&rendered),
        QueueItemIdentity::Id("configurable-icons".into())
    );
    let with_question = apply_operator_action_ids(&rendered, &["choose-format".into()]);
    assert_eq!(
        with_question,
        "QUESTION::READY FOR COORDINATOR> TOP:: do [#configurable-icons] [#choose-format]"
    );
    assert_eq!(
        queue_head_identity(&with_question),
        QueueItemIdentity::Id("configurable-icons".into())
    );

    // Both former reported/outbox glyphs remain parse-compatible when a custom
    // prefix is active and after the documented default changes to ☑️.
    for legacy in ["📫", "📤"] {
        let row = format!("{legacy} do [#legacy-outbox]");
        assert_eq!(
            lifecycle_marker(&row),
            Some(QueueLifecycleMarker::Reported)
        );
        assert_eq!(
            queue_head_identity(&row),
            QueueItemIdentity::Id("legacy-outbox".into())
        );
    }

    let ids = HashSet::from(["configurable-icons".to_string()]);
    let (backlog, changed) = set_in_progress_item_ids(
        "- [ ] [#configurable-icons] make the prefixes configurable\n",
        &ids,
    );
    assert!(changed);
    assert!(backlog.contains("[ ] WORKING NOW> [#configurable-icons]"));
    let (_, parsed, _) = parse_items(&backlog);
    assert!(parsed[0].in_progress);
}
