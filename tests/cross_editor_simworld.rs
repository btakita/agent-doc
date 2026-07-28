use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command as ProcessCommand, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

fn agent_doc() -> Command {
    cargo_bin_cmd!("agent-doc")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PluginHarnessKind {
    JetBrains,
    VsCode,
}

impl PluginHarnessKind {
    fn manifest_column(self) -> usize {
        match self {
            Self::JetBrains => 2,
            Self::VsCode => 3,
        }
    }

    fn source(self) -> &'static str {
        match self {
            Self::JetBrains => "jetbrains_plugin",
            Self::VsCode => "vscode_plugin",
        }
    }

    fn identity(self) -> &'static str {
        match self {
            Self::JetBrains => "intellij:simworld",
            Self::VsCode => "vscode:simworld",
        }
    }

    fn marker(self) -> &'static str {
        match self {
            Self::JetBrains => "jetbrains-harness\n",
            Self::VsCode => "vscode-harness\n",
        }
    }

    fn harness_name(self) -> &'static str {
        match self {
            Self::JetBrains => "jetbrains",
            Self::VsCode => "vscode",
        }
    }
}

struct ControllerGuard {
    project_root: PathBuf,
}

impl Drop for ControllerGuard {
    fn drop(&mut self) {
        let _ = agent_doc()
            .args([
                "controller",
                "shutdown",
                "--project-root",
                &self.project_root.to_string_lossy(),
            ])
            .output();
    }
}

struct NativePluginHarness {
    kind: PluginHarnessKind,
    child: Child,
    input: ChildStdin,
    responses: Receiver<Value>,
}

impl NativePluginHarness {
    fn spawn(
        kind: PluginHarnessKind,
        manifest_dir: &Path,
        project_root: &Path,
        file: &Path,
    ) -> anyhow::Result<Self> {
        let target_debug = manifest_dir.join("target/debug");
        let native_lib = target_debug.join(format!(
            "{}agent_doc{}",
            std::env::consts::DLL_PREFIX,
            std::env::consts::DLL_SUFFIX
        ));
        anyhow::ensure!(
            native_lib.is_file(),
            "native plugin harness requires {} (run `make cross-editor-simworld`)",
            native_lib.display()
        );

        let mut command = match kind {
            PluginHarnessKind::JetBrains => {
                let mut command =
                    ProcessCommand::new(manifest_dir.join("editors/jetbrains/gradlew"));
                command
                    .current_dir(manifest_dir.join("editors/jetbrains"))
                    .args([
                        "--no-daemon",
                        "--console=plain",
                        "-q",
                        "runCrossEditorHarness",
                    ]);
                let inherited_path = std::env::var_os("PATH").unwrap_or_default();
                let path = std::env::join_paths(
                    std::iter::once(target_debug.clone())
                        .chain(std::env::split_paths(&inherited_path)),
                )?;
                command.env("PATH", path);
                command
            }
            PluginHarnessKind::VsCode => {
                let mut command = ProcessCommand::new("node");
                command.arg(manifest_dir.join("editors/vscode/out/crossEditorHarness.js"));
                command
            }
        };
        command
            .env("AGENT_DOC_HARNESS_PROJECT_ROOT", project_root)
            .env("AGENT_DOC_HARNESS_FILE", file)
            .env(
                "AGENT_DOC_HARNESS_IDENTITY",
                format!("{}:native-simworld", kind.harness_name()),
            )
            .env("AGENT_DOC_LIB_PATH", &native_lib)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        let mut child = command.spawn()?;
        let input = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("{} harness omitted stdin", kind.harness_name()))?;
        let output = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("{} harness omitted stdout", kind.harness_name()))?;
        let (send, responses) = mpsc::channel();
        let harness_name = kind.harness_name();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                match line {
                    Ok(line) => match serde_json::from_str::<Value>(&line) {
                        Ok(value) if value["harness"].as_str() == Some(harness_name) => {
                            if send.send(value).is_err() {
                                break;
                            }
                        }
                        Ok(_) => eprintln!("[{harness_name}-harness] {line}"),
                        Err(_) if !line.trim().is_empty() => {
                            eprintln!("[{harness_name}-harness] {line}");
                        }
                        Err(_) => {}
                    },
                    Err(error) => {
                        eprintln!("[{harness_name}-harness] stdout read failed: {error}");
                        break;
                    }
                }
            }
        });
        Ok(Self {
            kind,
            child,
            input,
            responses,
        })
    }

    fn request(&mut self, command: Value) -> anyhow::Result<Value> {
        serde_json::to_writer(&mut self.input, &command)?;
        self.input.write_all(b"\n")?;
        self.input.flush()?;
        let response = self
            .responses
            .recv_timeout(Duration::from_secs(180))
            .map_err(|error| {
                anyhow::anyhow!(
                    "{} native harness did not respond: {error}",
                    self.kind.harness_name()
                )
            })?;
        anyhow::ensure!(
            response["ok"] == Value::Bool(true),
            "{} native harness command failed: {response}",
            self.kind.harness_name()
        );
        Ok(response)
    }

    fn text(&mut self) -> anyhow::Result<String> {
        let response = self.request(serde_json::json!({ "command": "text" }))?;
        response["text"].as_str().map(str::to_owned).ok_or_else(|| {
            anyhow::anyhow!(
                "{} native harness omitted text: {response}",
                self.kind.harness_name()
            )
        })
    }

    fn shutdown(&mut self) -> anyhow::Result<()> {
        self.request(serde_json::json!({ "command": "shutdown" }))?;
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(status) = self.child.try_wait()? {
                anyhow::ensure!(
                    status.success(),
                    "{} native harness exited with {status}",
                    self.kind.harness_name()
                );
                return Ok(());
            }
            anyhow::ensure!(
                Instant::now() < deadline,
                "{} native harness did not exit after shutdown",
                self.kind.harness_name()
            );
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}

impl Drop for NativePluginHarness {
    fn drop(&mut self) {
        match self.child.try_wait() {
            Ok(Some(_)) => {}
            Ok(None) => {
                if let Err(error) = self.child.kill() {
                    eprintln!(
                        "warning: failed to kill {} native harness: {error}",
                        self.kind.harness_name()
                    );
                }
                if let Err(error) = self.child.wait() {
                    eprintln!(
                        "warning: failed to reap {} native harness: {error}",
                        self.kind.harness_name()
                    );
                }
            }
            Err(error) => eprintln!(
                "warning: failed to inspect {} native harness: {error}",
                self.kind.harness_name()
            ),
        }
    }
}

struct PluginProtocolHarness {
    kind: PluginHarnessKind,
    project_root: PathBuf,
    file: PathBuf,
    replica: agent_doc_merge::crdt_sync::ReplicaState,
    pushed_state_vector: Vec<u8>,
}

impl PluginProtocolHarness {
    fn attach(kind: PluginHarnessKind, project_root: &Path, file: &Path) -> anyhow::Result<Self> {
        Self::attach_with_retained(kind, project_root, file, None)
    }

    fn attach_with_retained(
        kind: PluginHarnessKind,
        project_root: &Path,
        file: &Path,
        retained: Option<(Vec<u8>, Vec<u8>)>,
    ) -> anyhow::Result<Self> {
        let mut fields = serde_json::Map::new();
        if let Some((_, state_vector)) = retained.as_ref() {
            fields.insert(
                "state_vector_b64".to_string(),
                Value::String(BASE64.encode(state_vector)),
            );
        }
        let data = request(project_root, file, kind, "replica_register", fields)?;
        let client_id = data["client_id"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("replica register omitted client_id: {data}"))?;
        let bootstrap = decode_field(&data, "bootstrap_b64")?;
        let canonical_state_vector = decode_field(&data, "canonical_state_vector_b64")?;
        let incremental = data["bootstrap_kind"].as_str() == Some("delta");
        let replica = match retained {
            Some((encoded_state, _)) if incremental => {
                let replica = agent_doc_merge::crdt_sync::ReplicaState::from_encoded(
                    client_id,
                    &encoded_state,
                )?;
                if !bootstrap.is_empty() {
                    replica.apply_update(&bootstrap)?;
                }
                replica
            }
            _ => agent_doc_merge::crdt_sync::ReplicaState::from_encoded(client_id, &bootstrap)?,
        };
        Ok(Self {
            kind,
            project_root: project_root.to_path_buf(),
            file: file.to_path_buf(),
            pushed_state_vector: canonical_state_vector,
            replica,
        })
    }

    fn edit_without_pulling(&mut self, offset: usize, insert: &str) -> anyhow::Result<()> {
        self.replica.apply_local_edit(offset as u32, 0, insert);
        let update = self.replica.diff(&self.pushed_state_vector)?;
        self.pushed_state_vector = self.replica.state_vector();
        request(
            &self.project_root,
            &self.file,
            self.kind,
            "replica_update",
            serde_json::Map::from_iter([(
                "update_b64".to_string(),
                Value::String(BASE64.encode(update)),
            )]),
        )?;
        Ok(())
    }

    fn pull_and_ack(&mut self) -> anyhow::Result<usize> {
        let data = request(
            &self.project_root,
            &self.file,
            self.kind,
            "replica_pull",
            serde_json::Map::new(),
        )?;
        if data["kind"].as_str() == Some("replace") {
            anyhow::bail!(
                "{} harness unexpectedly received a replace delivery",
                self.kind.source()
            );
        }
        let updates = data["updates"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("replica pull omitted updates: {data}"))?;
        for update in updates {
            let encoded = decode_field(update, "update_b64")?;
            self.replica.apply_update(&encoded)?;
            let patch_id = update["patch_id"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("replica pull omitted patch_id: {update}"))?;
            let generation = update["generation"]
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("replica pull omitted generation: {update}"))?;
            let content_hash = agent_doc_hash::content_hash(&self.replica.text());
            let expected_content_hash =
                update["expected_content_hash"].as_str().ok_or_else(|| {
                    anyhow::anyhow!("replica pull omitted expected_content_hash: {update}")
                })?;
            let _historical_delivery_hash = expected_content_hash;
            let ack = request(
                &self.project_root,
                &self.file,
                self.kind,
                "replica_ack",
                serde_json::Map::from_iter([
                    ("patch_id".to_string(), Value::String(patch_id.to_string())),
                    ("generation".to_string(), Value::from(generation)),
                    ("content_hash".to_string(), Value::String(content_hash)),
                ]),
            )?;
            if ack["acknowledged"] != Value::Bool(true) {
                anyhow::bail!("replica ACK was not accepted: {ack}");
            }
        }
        self.pushed_state_vector = self.replica.state_vector();
        Ok(updates.len())
    }

    fn disconnect(self) -> anyhow::Result<(Vec<u8>, Vec<u8>)> {
        request(
            &self.project_root,
            &self.file,
            self.kind,
            "replica_deregister",
            serde_json::Map::new(),
        )?;
        Ok((self.replica.encode_state(), self.replica.state_vector()))
    }

    fn text(&self) -> String {
        self.replica.text()
    }
}

fn request(
    project_root: &Path,
    file: &Path,
    kind: PluginHarnessKind,
    method: &str,
    mut fields: serde_json::Map<String, Value>,
) -> anyhow::Result<Value> {
    fields.insert("method".to_string(), Value::String(method.to_string()));
    fields.insert(
        "identity".to_string(),
        Value::String(kind.identity().to_string()),
    );
    fields.insert(
        "source".to_string(),
        Value::String(kind.source().to_string()),
    );
    agent_doc_controller_io::project_controller::request_crdt_replica_for_test(
        project_root,
        file,
        Value::Object(fields),
    )
}

fn decode_field(value: &Value, field: &str) -> anyhow::Result<Vec<u8>> {
    let encoded = value[field]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("{field} missing from controller response: {value}"))?;
    BASE64
        .decode(encoded)
        .map_err(|error| anyhow::anyhow!("invalid {field}: {error}"))
}

fn supported_plugin_harnesses() -> Vec<PluginHarnessKind> {
    const REQUIRED: [&str; 3] = [
        "operator_text_authority_v1",
        "lazily_transport_receipts_v1",
        "cross_editor_native_harness_v1",
    ];
    let manifest = include_str!("../editors/plugin-parity.tsv");
    let rows: Vec<Vec<&str>> = manifest
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.split('\t').collect())
        .collect();
    let kinds = [PluginHarnessKind::JetBrains, PluginHarnessKind::VsCode];
    let supported = kinds
        .into_iter()
        .filter(|kind| {
            REQUIRED.iter().all(|feature| {
                rows.iter().any(|row| {
                    row.first() == Some(feature)
                        && row.get(kind.manifest_column()) == Some(&"supported")
                })
            })
        })
        .collect::<Vec<_>>();
    let zed_staged = REQUIRED.iter().all(|feature| {
        rows.iter()
            .any(|row| row.first() == Some(feature) && row.get(4) == Some(&"staged"))
    });
    assert!(
        zed_staged,
        "the real Zed harness must remain excluded until its required capabilities graduate"
    );
    supported
}

fn assert_harness_matches_shipped_plugin_sources(manifest_dir: &Path) {
    let jetbrains = std::fs::read_to_string(manifest_dir.join(
        "editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaForwarder.kt",
    ))
    .unwrap();
    let vscode =
        std::fs::read_to_string(manifest_dir.join("editors/vscode/src/crdtReplica.ts")).unwrap();
    for (source, source_token) in [
        (&jetbrains, "\"jetbrains_plugin\""),
        (&vscode, "'vscode_plugin'"),
    ] {
        assert!(source.contains(source_token));
        assert!(source.contains("replica_register"));
        assert!(source.contains("replica_update"));
        assert!(source.contains("replica_pull"));
        assert!(source.contains("replica_ack"));
        assert!(source.contains("replica_deregister"));
        assert!(source.contains("controller.sock"));
    }
}

#[test]
fn cross_editor_plugin_protocol_harnesses_peer_through_real_agent_doc_controller() {
    let _env_guard = agent_doc_test_support::env_lock();
    let temp = tempfile::TempDir::new().unwrap();
    let project_root = temp.path();
    let baseline = "shared controller baseline\n";
    let file = agent_doc_test_support::init_repo_with_doc(project_root, "network.md", baseline);
    std::fs::create_dir_all(project_root.join(".agent-doc/logs")).unwrap();
    std::fs::create_dir_all(project_root.join(".agent-doc/snapshots")).unwrap();

    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    assert_harness_matches_shipped_plugin_sources(manifest_dir);
    let kinds = supported_plugin_harnesses();
    assert_eq!(
        kinds,
        [PluginHarnessKind::JetBrains, PluginHarnessKind::VsCode],
        "required feature selection must stage unsupported plugins without a vacuous green"
    );

    // Seed the durable Lazily authority before the real binary starts its
    // controller. The controller restores this registration from state.db and
    // owns every subsequent relay, pull, and ACK operation.
    agent_doc_test_support::seed_reliable_sync_editor_registration(
        &file,
        "cross-editor-simworld",
        &["operator_text_authority_v1", "lazily_transport_receipts_v1"],
    );
    agent_doc()
        .args([
            "controller",
            "status",
            "--project-root",
            &project_root.to_string_lossy(),
            "--ensure",
        ])
        .assert()
        .success();
    let _controller = ControllerGuard {
        project_root: project_root.to_path_buf(),
    };

    let mut jetbrains =
        PluginProtocolHarness::attach(PluginHarnessKind::JetBrains, project_root, &file).unwrap();
    let mut vscode =
        PluginProtocolHarness::attach(PluginHarnessKind::VsCode, project_root, &file).unwrap();

    // Both production-shaped plugin harnesses edit from the same frontier before
    // either pulls. The real agent-doc controller merges and fans out both ops.
    let frontier = baseline.len();
    jetbrains
        .edit_without_pulling(frontier, jetbrains.kind.marker())
        .unwrap();
    vscode
        .edit_without_pulling(frontier, vscode.kind.marker())
        .unwrap();
    for _ in 0..3 {
        jetbrains.pull_and_ack().unwrap();
        vscode.pull_and_ack().unwrap();
    }
    assert_eq!(jetbrains.text(), vscode.text());
    assert!(jetbrains.text().contains("jetbrains-harness"));
    assert!(jetbrains.text().contains("vscode-harness"));

    // Reconnect uses the retained state vector, exactly like both plugin
    // forwarders. The returning peer receives only the missing controller delta.
    let retained = vscode.disconnect().unwrap();
    let next_offset = jetbrains.text().len();
    jetbrains
        .edit_without_pulling(next_offset, "while-vscode-offline\n")
        .unwrap();
    let mut vscode = PluginProtocolHarness::attach_with_retained(
        PluginHarnessKind::VsCode,
        project_root,
        &file,
        Some(retained),
    )
    .unwrap();
    jetbrains.pull_and_ack().unwrap();
    vscode.pull_and_ack().unwrap();
    assert_eq!(jetbrains.text(), vscode.text());
    assert!(vscode.text().contains("while-vscode-offline"));

    let ops = std::fs::read_to_string(project_root.join(".agent-doc/logs/ops.log")).unwrap();
    assert!(ops.contains("source=jetbrains_plugin"));
    assert!(ops.contains("source=vscode_plugin"));
    assert!(ops.contains("method=replica_ack"));

    let _ = jetbrains.disconnect().unwrap();
    let _ = vscode.disconnect().unwrap();
}

#[test]
#[ignore = "run through `make cross-editor-simworld` after compiling both editor harnesses"]
fn native_plugin_harnesses_peer_through_real_agent_doc_controller() {
    let _env_guard = agent_doc_test_support::env_lock();
    let temp = tempfile::TempDir::new().unwrap();
    let project_root = temp.path();
    let baseline = "shared native-plugin baseline\n";
    let file =
        agent_doc_test_support::init_repo_with_doc(project_root, "native-network.md", baseline);
    std::fs::create_dir_all(project_root.join(".agent-doc/logs")).unwrap();
    std::fs::create_dir_all(project_root.join(".agent-doc/snapshots")).unwrap();

    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let kinds = supported_plugin_harnesses();
    assert_eq!(
        kinds,
        [PluginHarnessKind::JetBrains, PluginHarnessKind::VsCode],
        "native test peers only the implementations whose required features are supported"
    );
    agent_doc_test_support::seed_reliable_sync_editor_registration(
        &file,
        "native-cross-editor-simworld",
        &[
            "operator_text_authority_v1",
            "lazily_transport_receipts_v1",
            "cross_editor_native_harness_v1",
        ],
    );
    agent_doc()
        .args([
            "controller",
            "status",
            "--project-root",
            &project_root.to_string_lossy(),
            "--ensure",
        ])
        .assert()
        .success();
    let _controller = ControllerGuard {
        project_root: project_root.to_path_buf(),
    };

    let mut jetbrains = NativePluginHarness::spawn(
        PluginHarnessKind::JetBrains,
        manifest_dir,
        project_root,
        &file,
    )
    .unwrap();
    let mut vscode =
        NativePluginHarness::spawn(PluginHarnessKind::VsCode, manifest_dir, project_root, &file)
            .unwrap();
    jetbrains
        .request(serde_json::json!({ "command": "attach" }))
        .unwrap();
    vscode
        .request(serde_json::json!({ "command": "attach" }))
        .unwrap();

    // Both native nodes edit the same causal frontier before either production
    // forwarder pulls. The real controller must merge and fan out both operations.
    let frontier = baseline.len();
    jetbrains
        .request(serde_json::json!({
            "command": "edit",
            "offset": frontier,
            "deleteLen": 0,
            "insert": PluginHarnessKind::JetBrains.marker(),
        }))
        .unwrap();
    vscode
        .request(serde_json::json!({
            "command": "edit",
            "offset": frontier,
            "deleteLen": 0,
            "insert": PluginHarnessKind::VsCode.marker(),
        }))
        .unwrap();
    for _ in 0..3 {
        jetbrains
            .request(serde_json::json!({ "command": "pull" }))
            .unwrap();
        vscode
            .request(serde_json::json!({ "command": "pull" }))
            .unwrap();
    }
    let jetbrains_text = jetbrains.text().unwrap();
    let vscode_text = vscode.text().unwrap();
    assert_eq!(jetbrains_text, vscode_text);
    assert!(jetbrains_text.contains("jetbrains-harness"));
    assert!(jetbrains_text.contains("vscode-harness"));

    // The VS Code production forwarder retains its native encoded state and state
    // vector while offline, then requests the missing controller delta on reconnect.
    vscode
        .request(serde_json::json!({ "command": "disconnect" }))
        .unwrap();
    jetbrains
        .request(serde_json::json!({
            "command": "edit",
            "offset": jetbrains_text.len(),
            "deleteLen": 0,
            "insert": "while-vscode-native-offline\n",
        }))
        .unwrap();
    vscode
        .request(serde_json::json!({ "command": "reconnect" }))
        .unwrap();
    jetbrains
        .request(serde_json::json!({ "command": "pull" }))
        .unwrap();
    vscode
        .request(serde_json::json!({ "command": "pull" }))
        .unwrap();
    assert_eq!(jetbrains.text().unwrap(), vscode.text().unwrap());
    assert!(
        vscode
            .text()
            .unwrap()
            .contains("while-vscode-native-offline")
    );

    let ops = std::fs::read_to_string(project_root.join(".agent-doc/logs/ops.log")).unwrap();
    assert!(ops.contains("source=jetbrains_plugin"));
    assert!(ops.contains("source=vscode_plugin"));
    assert!(ops.contains("method=replica_ack"));

    jetbrains.shutdown().unwrap();
    vscode.shutdown().unwrap();
}
