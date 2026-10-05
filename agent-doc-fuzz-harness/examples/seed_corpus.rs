//! Regenerate `fuzz/corpus/` from REDACTED real traffic (`#netadv7`).
//!
//! ```text
//! python3 fuzz/seed/redact.py md  <session.md>  <redacted>/doc1.md   # docs: doc*.md
//! python3 fuzz/seed/redact.py md  <response.md> <redacted>/resp1.md  # responses: resp*.md
//! python3 fuzz/seed/redact.py yrs <state.yrs>   <redacted>/x.yrs     # legacy CRDT state
//! cargo run -p agent-doc-fuzz-harness --example seed_corpus -- <redacted> fuzz/corpus
//! ```
//!
//! Only redacted inputs may be passed in. IPC seeds come from the protocol's own
//! message builders plus receipt lines observed in real `ops.log` traffic, and
//! CRDT seeds are encoded from the redacted documents in every wire format a
//! reader still accepts (columnar ADCR2, compact ADCR1, legacy JSON, ADN1
//! multinode container, lossless projection, version vectors).

use agent_doc_ipc_protocol as ipc;
use agent_doc_merge::crdt::MultiNodeState;
use agent_doc_merge::crdt_sync::{ReplicaState, decode_update_ops};
use base64::Engine as _;
use std::path::{Path, PathBuf};

fn write(out: &Path, target: &str, name: &str, bytes: impl AsRef<[u8]>) {
    let dir = out.join(target);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(name), bytes).unwrap();
}

fn read_sorted(dir: &Path, prefix: &str, ext: &str) -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            let name = path.file_name().unwrap().to_string_lossy();
            name.starts_with(prefix) && name.ends_with(ext)
        })
        .collect();
    files.sort();
    files
        .into_iter()
        .map(|path| {
            let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
            (stem, std::fs::read(&path).unwrap())
        })
        .collect()
}

/// Keep seeds small: cut at the last line break before `limit` bytes.
fn clip(text: &str, limit: usize) -> &str {
    if text.len() <= limit {
        return text;
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    match text[..end].rfind('\n') {
        Some(newline) => &text[..=newline],
        None => &text[..end],
    }
}

fn ipc_seeds(out: &Path) {
    let identity = ipc::IpcPeerIdentity::new(ipc::IPC_PROTOCOL_VERSION, "fuzz-build-0000");
    let other = ipc::IpcPeerIdentity::new(ipc::IPC_PROTOCOL_VERSION, "0.35.0+aaaaaaaaaaaa");
    let mut lines: Vec<String> = vec![
        ipc::ipc_hello_message(&identity).to_string(),
        ipc::ipc_hello_ack_message(&identity).to_string(),
        ipc::ipc_hello_message(&other).to_string(),
        ipc::ipc_handshake_rejection(
            &ipc::IpcHandshakeError::BuildMismatch {
                listener: identity.build_id.clone(),
                client: other.build_id.clone(),
            },
            &identity,
        ),
        ipc::ipc_handshake_rejection(
            &ipc::IpcHandshakeError::ProtocolMismatch {
                expected: 1,
                received: 2,
            },
            &identity,
        ),
        ipc::ipc_handshake_rejection(
            &ipc::IpcHandshakeError::Malformed {
                expected_type: "ipc_hello",
            },
            &identity,
        ),
        ipc::patch_message(
            "tasks/sample.md",
            serde_json::json!([{"component": "exchange", "content": "### Re: lorem\n\nipsum\n"}]),
            Some("agent_doc_format: template\n"),
        )
        .to_string(),
        ipc::early_receipt_tagged_message(
            &ipc::patch_message("tasks/sample.md", serde_json::json!([]), None),
            true,
        )
        .to_string(),
        ipc::queue_convergence_message("tasks/sample.md", true, None, Some("- lorem\n")).to_string(),
        ipc::reposition_message("tasks/sample.md", Some("a1b2c3d4"), true).to_string(),
        ipc::refresh_content_message("tasks/sample.md", "# lorem\n", "00ff", 8).to_string(),
        ipc::normalization_repair_patch_message(
            "tasks/sample.md",
            "patch-1",
            &["<!-- agent:exchange -->".to_string()],
            "00ff",
            8,
            true,
        )
        .to_string(),
        ipc::persist_current_message("tasks/sample.md", "00ff", 8).to_string(),
        ipc::vcs_refresh_probe_message("tasks/sample.md", "probe-1").to_string(),
        ipc::reload_lib_message("0.35.456").to_string(),
        ipc::early_receipt_line().to_string(),
        serde_json::to_string(&ipc::callback_request(
            "tasks/sample.md",
            "00ff",
            ["commit", "compact"],
            Some("ctx"),
            1_700_000_000,
            30,
            "req-1",
        ))
        .unwrap(),
        serde_json::to_string(&ipc::callback_response(
            "req-1",
            "completed",
            "lorem",
            None::<String>,
            None,
            1_700_000_001,
        ))
        .unwrap(),
        // Receipt lines exactly as observed in real ops.log traffic.
        r#"{"type":"receipt","status":"applied"}"#.to_string(),
        r#"{"type":"receipt","status":"rejected"}"#.to_string(),
        r#"{"type":"receipt","status":"rejected","reason":"already_applied"}"#.to_string(),
        // State-wire delta ops (lazily-spec delta.json vocabulary).
        r#"[{"op":"cell_set","slot_id":1,"payload":"e30="},{"op":"invalidate","slot_id":1},{"op":"node_add","slot_id":2,"type_tag":"agent_doc.queue"},{"op":"edge_add","dependent":2,"dependency":1},{"op":"edge_remove","dependent":2,"dependency":1},{"op":"node_remove","slot_id":2},{"op":"slot_value","slot_id":3,"payload":"bnVsbA=="}]"#.to_string(),
        r#"{"dependent":18446744073709551615,"dependency":0}"#.to_string(),
    ];
    lines.dedup();
    for (index, line) in lines.iter().enumerate() {
        write(out, "ipc_wire", &format!("seed-{index:02}.ndjson"), line);
    }
}

fn markdown_seeds(out: &Path, docs: &[(String, String)], responses: &[(String, String)]) {
    for (name, doc) in docs {
        write(out, "markdown_patch", &format!("{name}.md"), clip(doc, 12 * 1024));
    }
    for (name, response) in responses {
        write(out, "markdown_patch", &format!("{name}.md"), clip(response, 8 * 1024));
    }
    if let (Some((_, doc)), Some((_, response))) = (docs.first(), responses.first()) {
        let pair = format!("{}\0{}", clip(doc, 6 * 1024), clip(response, 3 * 1024));
        write(out, "markdown_patch", "pair-doc-response.md", pair);
    }
    let handcrafted: &[(&str, &str)] = &[
        ("nested", "<!-- agent:exchange -->\n<!-- agent:queue -->\n- a\n<!-- /agent:queue -->\n<!-- /agent:exchange -->\n"),
        ("fenced-marker", "```\n<!-- patch:exchange -->\n```\n<!-- patch:exchange -->\nbody\n<!-- /patch:exchange -->\n"),
        ("unterminated", "<!-- patch:exchange\n<!-- agent:queue -->\n"),
        ("replace-pending", "<!-- replace:pending -->\n- [ ] lorem\n<!-- /replace:pending -->\ntrailing\n"),
        ("boundary", "<!-- agent:exchange -->\n### Re: lorem\n<!-- agent:boundary:0a1b2c3d -->\n<!-- /agent:exchange -->\n"),
        ("attrs", "<!-- patch:exchange transfer-source=\"a b\" mode=append -->\nx\n<!-- /patch:exchange -->\n"),
        ("empty-comment", "<!---->\n<!-- -->\n<!--->\n"),
    ];
    for (name, text) in handcrafted {
        write(out, "markdown_patch", &format!("hand-{name}.md"), text);
    }
}

fn frontmatter_seeds(out: &Path, docs: &[(String, String)]) {
    for (name, doc) in docs {
        let head = match doc.find("<!-- agent:") {
            Some(marker) => &doc[..marker],
            None => clip(doc, 2048),
        };
        write(out, "frontmatter", &format!("{name}.md"), head);
    }
    let handcrafted: &[(&str, &str)] = &[
        ("empty-block", "---\n---\nbody\n"),
        ("bom", "\u{feff}---\nagent: claude\n---\nbody\n"),
        ("indented", "  ---\nagent: codex\n---\n"),
        ("presets-alias", "---\npresets:\n  '#a': lorem\nprompt_presets:\n  '#b': ipsum\n---\n"),
        ("null-key", "---\nagent:\n'#k':\nqueue: start\n---\nbody"),
        ("unterminated", "---\nagent: claude\n"),
        ("pipeline", "---\nagent_doc_format: template\nagent_doc_pipeline:\n  state: running\n---\n"),
    ];
    for (name, text) in handcrafted {
        write(out, "frontmatter", &format!("hand-{name}.md"), text);
    }
}

fn crdt_seeds(out: &Path, docs: &[(String, String)], legacy: &[(String, Vec<u8>)]) {
    for (name, doc) in docs.iter().take(2) {
        let text = clip(doc, 1024);
        let replica = ReplicaState::from_text(1, text);
        // Exercise tombstones and a second peer so the envelope is not trivial.
        replica.apply_local_edit(3, 5, "lorem");
        let peer = ReplicaState::from_encoded(2, &replica.encode_state()).unwrap();
        peer.apply_local_edit(0, 0, "# ");
        let columnar = peer.encode_state();
        write(out, "crdt_update", &format!("{name}-adcr2.bin"), &columnar);
        let ops = decode_update_ops(&columnar).unwrap();
        write(out, "crdt_update", &format!("{name}-legacy.json"), serde_json::to_vec(&ops).unwrap());
        let packed = rmp_serde::to_vec(&ops).unwrap();
        let compressed = zstd::stream::encode_all(packed.as_slice(), 3).unwrap();
        let mut adcr1 = b"ADCR1:".to_vec();
        adcr1.extend_from_slice(base64::engine::general_purpose::STANDARD.encode(compressed).as_bytes());
        write(out, "crdt_update", &format!("{name}-adcr1.bin"), adcr1);
        let delta = peer.diff(&replica.state_vector()).unwrap();
        write(out, "crdt_update", &format!("{name}-delta.bin"), delta);
        write(out, "crdt_update", &format!("{name}-vv.json"), peer.state_vector());
        if let Ok(state) = MultiNodeState::from_text(text) {
            write(out, "crdt_update", &format!("{name}-adn1.bin"), state.encode());
        }
        let projection = agent_doc_markdown_lossless::project(clip(text, 512));
        write(
            out,
            "crdt_update",
            &format!("{name}-lossless.json"),
            agent_doc_markdown_lossless::projection_to_bytes(&projection).unwrap(),
        );
    }
    for (name, bytes) in legacy {
        write(out, "crdt_update", &format!("{name}.yrs"), bytes);
    }

    // Edit scripts: [offset_lo, offset_hi, delete, insert_len] + insert bytes.
    let mut scripts: Vec<Vec<u8>> = Vec::new();
    for (_, doc) in docs.iter().take(2) {
        let mut script = Vec::new();
        for (index, chunk) in clip(doc, 256).as_bytes().chunks(24).enumerate() {
            script.extend_from_slice(&[(index * 7) as u8, 0, (index % 3) as u8, chunk.len() as u8]);
            script.extend_from_slice(chunk);
        }
        scripts.push(script);
    }
    scripts.push(b"\x00\x00\x00\x05hello\x02\x00\x01\x03\xc3\xa9x\xff\x0f\x3f\x00".to_vec());
    for (index, script) in scripts.iter().enumerate() {
        write(out, "crdt_edits", &format!("script-{index:02}.bin"), script);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let redacted = Path::new(&args[1]);
    let out = Path::new(&args[2]);
    let text = |files: Vec<(String, Vec<u8>)>| -> Vec<(String, String)> {
        files
            .into_iter()
            .map(|(name, bytes)| (name, String::from_utf8(bytes).unwrap()))
            .collect()
    };
    let docs = text(read_sorted(redacted, "doc", ".md"));
    let responses = text(read_sorted(redacted, "resp", ".md"));
    let legacy = read_sorted(redacted, "", ".yrs");
    ipc_seeds(out);
    markdown_seeds(out, &docs, &responses);
    frontmatter_seeds(out, &docs);
    crdt_seeds(out, &docs, &legacy);
}
