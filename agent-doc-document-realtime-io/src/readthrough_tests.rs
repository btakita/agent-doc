//! `#composefault` root, part 2: a binary-owned writer reads the document
//! through the binary's own pending retained write.
//!
//! Measured on haiven-dev `tasks/sdk/sdk-dev.md`, 2026-11-24T21:45:07Z
//! (ops.log): `subagent_completion_feedback_accepted
//! delivery=retained_controller_projection` stored report-complete only as a
//! retained intent (`publish_exchange_update` retains it while an editor is
//! attached and does not write the CP model). Before the controller projected
//! that intent, `mechanical_reported_closeout` read the authority, which did
//! not carry it, rebuilt the "missing" response cell over that text, and
//! wrote it (`mechanical_reported_closeout_write_response`). Its retained
//! successor was then a composition of report-complete's target with a write
//! derived without it (`stale_queue_rows_reverted reverted=1`).
//!
//! Part 1 (`chainedwrite_tests`) made a write derived over a cut that already
//! carries the retained write that write's successor. These tests cover the
//! case where no cut carried it yet: the reader returns the retained write
//! projected over the current text (what the controller is about to write),
//! and the write path treats a base equal to that projection as the relay text
//! and replaces the retained write with its successor.
use super::chainedwrite_tests::{
    NOTE, OTHER, PR, TYPED, TYPED_PARTIAL, WORK, assert_struck_with_tree, consume_over, doc,
    ops_log, origin_id, project, report_complete, temp_doc,
};
use super::tests::{seed_reliable_sync_close, seed_reliable_sync_open};
use super::*;
use agent_doc_document_realtime::concurrent_row_ops::probe;

const IDENTITY: &str = "test-composefault-readthrough";
const READ_SOURCE: &str = "mechanical_reported_closeout";
const WRITE_SOURCE: &str = "mechanical_reported_closeout_write_response";

fn claimed() -> String {
    format!("  - 🚧 [#subagent:jwtadopt#1] {WORK}\n")
}

struct Fixture {
    dir: tempfile::TempDir,
    file: std::path::PathBuf,
    client_id: u64,
    base: String,
    followups: Vec<String>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        seed_reliable_sync_close(&self.file, IDENTITY);
    }
}

/// An attached editor replica holds `base`; report-complete is retained only
/// (the CP model never receives it); the operator types a row the editor
/// never received report-complete under.
fn report_complete_retained_while_the_operator_types() -> Fixture {
    let claimed = claimed();
    let base_rows = [PR, NOTE, claimed.as_str(), OTHER];
    let base = doc("", &base_rows);
    let (reported, followups) = report_complete(&base_rows);
    let (dir, file) = temp_doc(&base);
    seed_reliable_sync_open(&file, IDENTITY);
    let (client_id, _bootstrap) = test_support_register_replica_for_file(&file, IDENTITY)
        .unwrap()
        .expect("editor replica should attach");
    // `publish_exchange_update` with an editor attached.
    retain_deferred_document_write_target(
        &file,
        &base,
        &reported,
        "queue_subagent_completion_feedback",
        DocumentWriteDeferredReason::EditorProjectionPending,
    )
    .unwrap();
    let fixture = Fixture {
        dir,
        file,
        client_id,
        base,
        followups,
    };
    type_into_editor(&fixture, &fixture.base.clone(), TYPED_PARTIAL);
    fixture
}

/// The operator's editor replaces `previous` with the base rows plus `typed`.
fn type_into_editor(fixture: &Fixture, previous: &str, typed: &str) -> String {
    let claimed = claimed();
    let editor = doc("", &[PR, NOTE, claimed.as_str(), OTHER, typed]);
    agent_doc_crdt_relay_io::with_hub(&fixture.file, |hub| {
        hub.apply_local(
            fixture.client_id,
            0,
            previous.chars().count() as u32,
            &editor,
        )
        .unwrap();
    })
    .unwrap();
    editor
}

fn relay_text(file: &std::path::Path) -> String {
    match agent_doc_crdt_relay_io::current_text_for_file(file).unwrap() {
        agent_doc_crdt_relay_io::CurrentText::Current { text, .. } => text,
        other => panic!("expected the attached relay's current text, got {other:?}"),
    }
}

fn assert_carries_report_complete(label: &str, text: &str, fixture: &Fixture) {
    let id = origin_id(&fixture.followups[0]);
    assert_eq!(
        text.matches(&format!("☑️ [#subagent:jwtadopt#1] {WORK} [#{id}]"))
            .count(),
        1,
        "{label}: the reported row, with its admitted id, exactly once:\n{text}"
    );
    for row in &fixture.followups {
        assert_eq!(text.matches(row.as_str()).count(), 1, "{label}:\n{text}");
    }
    assert_eq!(
        text.matches("### Re: Subagent completed: `subagent:jwtadopt`")
            .count(),
        1,
        "{label}: one completion cell:\n{text}"
    );
    assert_eq!(
        text.matches(TYPED_PARTIAL.trim_end()).count(),
        1,
        "{label}: the operator's row once:\n{text}"
    );
    assert!(
        !text.contains(&claimed()),
        "{label}: no stale `🚧` copy of the row:\n{text}"
    );
}

/// The reader returns report-complete projected over the editor's text,
/// before any controller projection reached the CP model.
#[test]
fn binary_writer_reads_the_document_through_a_retained_report_complete() {
    let fixture = report_complete_retained_while_the_operator_types();
    let editor = doc("", &[PR, NOTE, claimed().as_str(), OTHER, TYPED_PARTIAL]);
    assert_eq!(
        relay_text(&fixture.file),
        editor,
        "precondition: the CP model does not carry the retained report-complete"
    );

    let read =
        try_resolve_current_document_content_through_retained_writes(&fixture.file, READ_SOURCE)
            .unwrap();
    assert_carries_report_complete("read", &read, &fixture);
    assert_eq!(
        read,
        project(&fixture.file, &editor),
        "the read is the controller's own projection of the retained write"
    );
}

/// Once the controller's projection reached the CP model, the reader returns
/// the authority as it is: the retained write is not applied a second time.
#[test]
fn reading_through_a_retained_write_the_authority_already_carries_is_the_authority() {
    let fixture = report_complete_retained_while_the_operator_types();
    let editor = doc("", &[PR, NOTE, claimed().as_str(), OTHER, TYPED_PARTIAL]);
    let projected = project(&fixture.file, &editor);
    agent_doc_crdt_relay_io::with_hub(&fixture.file, |hub| {
        hub.apply_local(
            fixture.client_id,
            0,
            editor.chars().count() as u32,
            &projected,
        )
        .unwrap();
    })
    .unwrap();
    assert_eq!(relay_text(&fixture.file), projected);

    let read =
        try_resolve_current_document_content_through_retained_writes(&fixture.file, READ_SOURCE)
            .unwrap();
    assert_eq!(read, projected);
}

/// The live sequence: the closeout reads through the retained report-complete,
/// strikes the row and attaches its response tree, and writes. The write is
/// report-complete's successor: the retained target is the write itself, the
/// CP model receives it, and its projection over the editor's next cut (the
/// editor never acknowledged anything; the operator typed on) is one struck row
/// with its tree, then the follow-ups, then the operator's row.
fn closeout_reads_through_then_writes(read_through: bool) -> String {
    let fixture = report_complete_retained_while_the_operator_types();
    let editor = doc("", &[PR, NOTE, claimed().as_str(), OTHER, TYPED_PARTIAL]);
    let read = if read_through {
        try_resolve_current_document_content_through_retained_writes(&fixture.file, READ_SOURCE)
            .unwrap()
    } else {
        // What the fixed reader returns, independent of the reader: isolates
        // the write side.
        project(&fixture.file, &editor)
    };
    let id = origin_id(&fixture.followups[0]);
    let closeout = consume_over(&read, &id);

    apply_canonical_replace_if_attached(&fixture.file, &read, &closeout, WRITE_SOURCE)
        .unwrap()
        .expect("attached CRDT write");

    let pending = pending_document_write(&fixture.file).expect("successor retained");
    assert_eq!(
        pending.target_content, closeout,
        "a write derived over the projection of the retained write is that write's successor, not a composition"
    );
    assert_eq!(
        relay_text(&fixture.file),
        closeout,
        "the CP model receives the successor as written"
    );

    let next_editor = doc("", &[PR, NOTE, claimed().as_str(), OTHER, TYPED]);
    let projected = project(&fixture.file, &next_editor);
    assert_struck_with_tree("projected", &projected, &id, &fixture.followups, TYPED);
    ops_log(&fixture.dir)
}

fn assert_superseded_not_composed(log: &str) {
    assert!(
        !log.contains(&format!("{WRITE_SOURCE}_deferred_write_composed")),
        "the closeout composed a second copy of the retained write:\n{log}"
    );
    assert!(
        !log.contains(&format!(
            "{WRITE_SOURCE}_deferred_write_stale_queue_rows_reverted"
        )),
        "the closeout reverted queue rows of the retained write:\n{log}"
    );
    assert!(
        log.contains(&format!(
            "{WRITE_SOURCE}_deferred_write_superseded_by_derived_cut"
        )),
        "the closeout superseded the retained write it read through:\n{log}"
    );
}

#[test]
fn closeout_over_the_projected_retained_write_is_its_successor_without_the_composefault_rule() {
    probe::with_disabled(|| closeout_reads_through_then_writes(false));
}

#[test]
fn closeout_over_the_projected_retained_write_leaves_the_composefault_rule_nothing_to_rewrite() {
    let (log, rewrites) = probe::count_rewrites(|| closeout_reads_through_then_writes(false));
    assert_eq!(rewrites, 0, "the #composefault rule rewrote a row:\n{log}");
    assert_superseded_not_composed(&log);
}

#[test]
fn closeout_reading_through_the_retained_write_is_its_successor_without_the_composefault_rule() {
    probe::with_disabled(|| closeout_reads_through_then_writes(true));
}

#[test]
fn closeout_reading_through_the_retained_write_leaves_the_composefault_rule_nothing_to_rewrite() {
    let (log, rewrites) = probe::count_rewrites(|| closeout_reads_through_then_writes(true));
    assert_eq!(rewrites, 0, "the #composefault rule rewrote a row:\n{log}");
    assert_superseded_not_composed(&log);
}
