//! Fuzz harness functions for the bytes that cross an agent-doc process or
//! network boundary (`#netadv7`).
//!
//! Every function here takes raw, untrusted bytes and drives one decoder family
//! through its public entry points. The oracles are:
//!
//! * **no panic** — a panic is a crash in the controller or, through the
//!   cdylib, in the host IDE;
//! * **no hang** — enforced by libFuzzer's `-timeout` in `fuzz/`, and by the
//!   ordinary test timeout in the stable replay tests;
//! * **lossless round-trip** wherever the code claims one (the markdown
//!   lossless tree, the CRDT op envelopes, the multinode container, the IPC
//!   wire types, frontmatter write/re-parse).
//!
//! The functions are toolchain-neutral. `fuzz/` (cargo-fuzz, nightly, excluded
//! from the workspace) calls them from libFuzzer, and `tests/corpus_replay.rs`
//! replays the committed corpus and every promoted crasher through them on
//! stable, so `make check` exercises the same oracles without nightly.

use agent_doc_ipc_protocol::{
    CallbackRequest, CallbackResponse, IPC_PROTOCOL_VERSION, IpcPeerIdentity,
    SocketReceiptClassification, classify_socket_receipt, ipc_handshake_rejection,
    message_is_reload_library, message_requests_early_receipt, validate_ipc_hello,
    validate_ipc_hello_ack,
};
use agent_doc_merge::crdt::MultiNodeState;
use agent_doc_merge::crdt_sync::{ReplicaState, decode_update_ops, encode_update_ops};
use agent_doc_state_wire::{WireDeltaOp, WireEdgeSnapshot};

/// Upper bound on input size accepted by every harness. libFuzzer is also run
/// with `-max_len` at or below this; the cap keeps the stable replay tests and a
/// hand-fed corpus file from turning a quadratic-but-correct path into a
/// spurious timeout.
pub const MAX_INPUT_LEN: usize = 64 * 1024;

/// The fuzz targets, by corpus directory name. `fuzz/fuzz_targets/<name>.rs`
/// and `fuzz/corpus/<name>/` use the same names.
pub const TARGETS: &[(&str, fn(&[u8]))] = &[
    ("ipc_wire", ipc_wire),
    ("markdown_patch", markdown_patch),
    ("frontmatter", frontmatter),
    ("crdt_update", crdt_update),
    ("crdt_edits", crdt_edits),
];

/// Look up a harness function by target name.
pub fn target(name: &str) -> Option<fn(&[u8])> {
    TARGETS
        .iter()
        .find(|(target, _)| *target == name)
        .map(|(_, run)| *run)
}

fn utf8(data: &[u8]) -> Option<&str> {
    if data.len() > MAX_INPUT_LEN {
        return None;
    }
    std::str::from_utf8(data).ok()
}

/// Typed wire values must survive `decode → encode → decode` unchanged.
fn assert_json_round_trip<T>(line: &str)
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    let Ok(first) = serde_json::from_str::<T>(line) else {
        return;
    };
    let encoded = serde_json::to_string(&first).expect("decoded wire value must re-encode");
    let second: T = serde_json::from_str(&encoded).expect("re-encoded wire value must decode");
    let reencoded = serde_json::to_string(&second).expect("wire value must re-encode");
    assert_eq!(encoded, reencoded, "wire value is not a fixed point of decode/encode");
}

/// IPC wire decode: one NDJSON line as it arrives on an editor or actor socket.
pub fn ipc_wire(data: &[u8]) {
    let Some(line) = utf8(data) else {
        return;
    };
    let identity = IpcPeerIdentity::new(IPC_PROTOCOL_VERSION, "fuzz-build-0000");

    match validate_ipc_hello(line, &identity) {
        Ok(()) => {
            // Admission is proof of identity: the line must actually carry the
            // listener's own protocol and build.
            let value: serde_json::Value =
                serde_json::from_str(line).expect("admitted hello must be JSON");
            assert_eq!(value["type"], "ipc_hello");
            assert_eq!(value["build_id"], identity.build_id.as_str());
            assert_eq!(
                value["protocol_version"].as_u64(),
                Some(u64::from(IPC_PROTOCOL_VERSION))
            );
        }
        Err(error) => {
            // The rejection the listener sends back must itself be well-formed
            // and must never be admitted by the peer that receives it.
            let rejection = ipc_handshake_rejection(&error, &identity);
            assert!(validate_ipc_hello_ack(&rejection, &identity).is_err());
            assert_eq!(
                classify_socket_receipt(&rejection),
                SocketReceiptClassification::Rejected
            );
        }
    }
    if validate_ipc_hello_ack(line, &identity).is_ok() {
        let value: serde_json::Value =
            serde_json::from_str(line).expect("admitted hello ack must be JSON");
        assert_eq!(value["type"], "ipc_hello_ack");
        assert_eq!(value["build_id"], identity.build_id.as_str());
    }

    let classification = classify_socket_receipt(line);
    if classification != SocketReceiptClassification::Unsupported {
        let value: serde_json::Value =
            serde_json::from_str(line).expect("a classified receipt must be JSON");
        assert!(
            value["type"]
                .as_str()
                .is_some_and(|kind| kind.eq_ignore_ascii_case("receipt")),
            "only a receipt may be classified as a delivery outcome"
        );
    }
    let _ = message_is_reload_library(line);
    let _ = message_requests_early_receipt(line);

    assert_json_round_trip::<IpcPeerIdentity>(line);
    assert_json_round_trip::<CallbackRequest>(line);
    assert_json_round_trip::<CallbackResponse>(line);
    assert_json_round_trip::<WireDeltaOp>(line);
    assert_json_round_trip::<Vec<WireDeltaOp>>(line);
    assert_json_round_trip::<WireEdgeSnapshot>(line);
}

/// Split one fuzz input into a (document, response) pair at the first NUL.
fn split_doc_response(text: &str) -> (&str, &str) {
    match text.split_once('\0') {
        Some((doc, response)) => (doc, response),
        None => (text, text),
    }
}

/// Markdown component / patch parsers: the `patch:exchange` response path and
/// the session-document component parser, plus the lossless tree that claims
/// `render(parse(doc)) == doc`.
pub fn markdown_patch(data: &[u8]) {
    let Some(text) = utf8(data) else {
        return;
    };
    let (doc, response) = split_doc_response(text);

    // Lossless tree: the defining invariant, the durable projection, and the
    // projection's serialized form must all reproduce the source byte-for-byte.
    let tree = agent_doc_markdown_lossless::parse(doc);
    assert_eq!(tree.render(), doc, "lossless tree must render its source");
    let projection = agent_doc_markdown_lossless::project(doc);
    assert!(projection.is_current_for(doc));
    assert_eq!(agent_doc_markdown_lossless::restore(&projection), doc);
    let bytes = agent_doc_markdown_lossless::projection_to_bytes(&projection)
        .expect("projection must serialize");
    let decoded = agent_doc_markdown_lossless::projection_from_bytes(&bytes)
        .expect("serialized projection must decode");
    assert_eq!(agent_doc_markdown_lossless::restore(&decoded), doc);
    assert!(agent_doc_markdown_lossless::shadow_audit(doc).matches);

    // Component parser: every reported span must be an in-bounds, ordered,
    // char-aligned slice of the document.
    if let Ok(components) = agent_doc_element::element::parse(doc) {
        for component in &components {
            assert!(component.open_start <= component.open_end);
            assert!(component.open_end <= component.close_start);
            assert!(component.close_start <= component.close_end);
            assert!(component.close_end <= doc.len());
            for offset in [
                component.open_start,
                component.open_end,
                component.close_start,
                component.close_end,
            ] {
                assert!(doc.is_char_boundary(offset));
            }
        }
    }

    // Patch parser: each patch body is a verbatim slice of the response.
    if let Ok((patches, unmatched)) = agent_doc_template::parse_patches(response) {
        for patch in &patches {
            assert!(
                response.contains(&patch.content),
                "patch {:?} content is not a slice of the response",
                patch.name
            );
        }
        let configs = std::collections::HashMap::new();
        let max_lines = std::collections::HashMap::new();
        let _ = agent_doc_template::apply_patches_pure(
            doc, &patches, &unmatched, None, &configs, &max_lines,
        );
    }
    let _ = agent_doc_template::normalize_editor_visible_template_structure(doc);
}

/// Frontmatter parse: the YAML block at the head of every session document,
/// which editors and operators write directly.
pub fn frontmatter(data: &[u8]) {
    let Some(content) = utf8(data) else {
        return;
    };
    let _ = agent_doc_frontmatter::raw_frontmatter_yaml(content);
    let _ = agent_doc_frontmatter::repair_frontmatter_yaml(content);
    let Ok((fm, body)) = agent_doc_frontmatter::parse(content) else {
        return;
    };
    // `write` canonicalizes (the deprecated `queue_active:` folds onto
    // `queue:`), so the oracle is: one write reaches a fixed point, the body
    // survives, and every reader-visible decision is unchanged.
    let written = agent_doc_frontmatter::write(&fm, body).expect("parsed frontmatter must write");
    let (canonical, rebody) =
        agent_doc_frontmatter::parse(&written).expect("written frontmatter must re-parse");
    assert_eq!(rebody, body, "write/parse must preserve the body");
    assert_same_decisions(&fm, &canonical);
    let rewritten =
        agent_doc_frontmatter::write(&canonical, rebody).expect("canonical frontmatter must write");
    assert_eq!(rewritten, written, "write must reach a fixed point after one canonicalization");
    let expected = serde_yaml::to_value(&canonical).expect("frontmatter must serialize");

    // `write_preserving` claims byte preservation of unchanged keys; at minimum
    // it must produce a document that parses back to the same frontmatter.
    let preserved = agent_doc_frontmatter::write_preserving(content, &fm, body)
        .expect("parsed frontmatter must write preserving");
    let (reparsed, rebody) = agent_doc_frontmatter::parse(&preserved)
        .expect("write_preserving output must re-parse");
    assert_eq!(rebody, body, "write_preserving must preserve the body");
    assert_same_decisions(&fm, &reparsed);
    assert_eq!(
        serde_yaml::to_value(&reparsed).expect("frontmatter must serialize"),
        expected,
        "write_preserving must agree with write"
    );
}

fn assert_same_decisions(
    before: &agent_doc_frontmatter::Frontmatter,
    after: &agent_doc_frontmatter::Frontmatter,
) {
    assert_eq!(before.queue_active, after.queue_active, "queue state changed");
    assert_eq!(before.resolve_mode(), after.resolve_mode(), "document mode changed");
    assert_eq!(before.session, after.session, "session id changed");
    assert_eq!(
        before.active_resume_harness(),
        after.active_resume_harness(),
        "resume harness changed"
    );
}

/// CRDT op / update decode: replicated text-op deltas, durable replica state,
/// the per-node container, and the lossless projection's durable bytes.
pub fn crdt_update(data: &[u8]) {
    if data.len() > MAX_INPUT_LEN {
        return;
    }
    // Raw bytes exercise envelope detection and framing; the same bytes
    // re-wrapped as an envelope body reach the msgpack / columnar decoder,
    // which base64 + zstd framing otherwise shields from byte mutations.
    for update in [
        data.to_vec(),
        wrap_envelope(b"ADCR2:", data),
        wrap_envelope(b"ADCR1:", data),
    ] {
        if let Ok(ops) = decode_update_ops(&update) {
            let encoded = encode_update_ops(&ops).expect("decoded ops must re-encode");
            let decoded = decode_update_ops(&encoded).expect("re-encoded ops must decode");
            assert_eq!(decoded, ops, "text-op envelope must round-trip");
            let replica = ReplicaState::from_text(9, "seed\n");
            if replica.apply_update(&update).is_ok() {
                let _ = replica.text();
            }
        }
    }

    // A replica must either refuse the update or apply it and still project.
    let replica = ReplicaState::from_text(9, "seed\n");
    if replica.apply_update(data).is_ok() {
        let text = replica.text();
        // Idempotent: re-applying a known delta is a no-op.
        replica.apply_update(data).expect("re-applying an applied update");
        assert_eq!(replica.text(), text);
        // A peer bootstrapped from this replica's state converges with it.
        let peer = ReplicaState::from_encoded(10, &replica.encode_state())
            .expect("own encoded state must bootstrap a peer");
        assert_eq!(peer.text(), text);
    }
    let _ = replica.preview_update_text(data);
    let _ = replica.diff(data);
    let _ = replica.covers_state_vector(data);

    if let Ok(state) = MultiNodeState::decode(data) {
        let encoded = state.encode();
        assert!(
            data.starts_with(&encoded),
            "multinode container must re-encode to its decoded prefix"
        );
        let _ = state.to_text();
    }
    let _ = MultiNodeState::decode_or_migrate(data, "fallback\n");

    if let Ok(projection) = agent_doc_markdown_lossless::projection_from_bytes(data) {
        let _ = agent_doc_markdown_lossless::restore(&projection);
    }
}

/// `magic || base64(zstd(body))`: the framing of the ADCR1 / ADCR2 text-op
/// envelopes around an arbitrary body.
fn wrap_envelope(magic: &[u8], body: &[u8]) -> Vec<u8> {
    use base64::Engine as _;
    let compressed = zstd::stream::encode_all(body, 1).expect("zstd encode in memory");
    let mut envelope = magic.to_vec();
    envelope.extend_from_slice(
        base64::engine::general_purpose::STANDARD
            .encode(compressed)
            .as_bytes(),
    );
    envelope
}

/// Model-based CRDT edit fuzzing: interpret the input as an editor edit script
/// (the FFI `agent_doc_replica_apply_local` call shape: codepoint offset, delete
/// length, insert text), apply it to a replica and to a plain `Vec<char>` model,
/// and require the replica, a snapshot-bootstrapped peer, and an incrementally
/// synced peer to all agree with the model.
pub fn crdt_edits(data: &[u8]) {
    if data.len() > 4096 {
        return;
    }
    let editor = ReplicaState::from_text(1, "");
    let follower = ReplicaState::new(2);
    let mut model: Vec<char> = Vec::new();
    let mut cursor = data;
    while cursor.len() >= 4 {
        let offset = u32::from(cursor[0]) | (u32::from(cursor[1] & 0x0f) << 8);
        let delete_len = u32::from(cursor[2] & 0x3f);
        let insert_len = usize::from(cursor[3] & 0x1f);
        cursor = &cursor[4..];
        let take = insert_len.min(cursor.len());
        let insert = String::from_utf8_lossy(&cursor[..take]).into_owned();
        cursor = &cursor[take..];

        editor.apply_local_edit(offset, delete_len, &insert);
        let start = (offset as usize).min(model.len());
        let end = start.saturating_add(delete_len as usize).min(model.len());
        model.splice(start..end, insert.chars());

        let expected: String = model.iter().collect();
        assert_eq!(editor.text(), expected, "replica diverged from the edit model");

        // Incremental sync round: the follower announces its frontier and
        // applies only the delta it is missing.
        let delta = editor
            .diff(&follower.state_vector())
            .expect("diff against a well-formed frontier");
        follower.apply_update(&delta).expect("apply own delta");
        assert_eq!(follower.text(), expected, "incremental sync diverged");
    }
    let expected: String = model.iter().collect();
    let snapshot = ReplicaState::from_encoded(3, &editor.encode_state())
        .expect("own encoded state must bootstrap a peer");
    assert_eq!(snapshot.text(), expected, "snapshot bootstrap diverged");
}
