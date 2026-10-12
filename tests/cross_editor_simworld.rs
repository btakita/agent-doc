use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::Value;
use std::ffi::OsStr;
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

fn cargo_target_debug_dir(manifest_dir: &Path, configured: Option<&OsStr>) -> PathBuf {
    let target = configured
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target"));
    let target = if target.is_absolute() {
        target
    } else {
        manifest_dir.join(target)
    };
    target.join("debug")
}

impl NativePluginHarness {
    fn spawn(
        kind: PluginHarnessKind,
        manifest_dir: &Path,
        project_root: &Path,
        file: &Path,
    ) -> anyhow::Result<Self> {
        let configured_target = std::env::var_os("CARGO_TARGET_DIR");
        let target_debug = cargo_target_debug_dir(manifest_dir, configured_target.as_deref());
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

    fn pull_and_project(&mut self) -> anyhow::Result<usize> {
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
            let content_hash = agent_doc_hash::content_hash(&self.replica.text());
            let projection = request(
                &self.project_root,
                &self.file,
                self.kind,
                "replica_projection",
                serde_json::Map::from_iter([
                    ("content_hash".to_string(), Value::String(content_hash)),
                    ("disk_persisted".to_string(), Value::Bool(false)),
                ]),
            )?;
            if projection["projected"] != Value::Bool(true) {
                anyhow::bail!("replica projection was not accepted: {projection}");
            }
        }
        self.pushed_state_vector = self.replica.state_vector();
        Ok(updates.len())
    }

    /// Replace this replica's membership at an exact observed controller cut.
    /// `expected_canonical_hash` is the registration CAS the plugin fences a
    /// captured splice batch with, so a stale captured base must refuse here
    /// rather than after the old replica is already retired.
    fn reregister_at_canonical_cut(&mut self, expected_canonical_hash: &str) -> anyhow::Result<()> {
        let data = register_at_canonical_cut(
            &self.project_root,
            &self.file,
            self.kind,
            expected_canonical_hash,
        )?;
        let client_id = data["client_id"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("replacement register omitted client_id: {data}"))?;
        anyhow::ensure!(
            data["bootstrap_kind"].as_str() != Some("delta"),
            "a replacement cut must bootstrap from the full canonical state: {data}"
        );
        let bootstrap = decode_field(&data, "bootstrap_b64")?;
        self.pushed_state_vector = decode_field(&data, "canonical_state_vector_b64")?;
        self.replica =
            agent_doc_merge::crdt_sync::ReplicaState::from_encoded(client_id, &bootstrap)?;
        Ok(())
    }

    /// Forward one bounded splice batch, exactly like `CrdtReplicaForwarder`:
    /// apply each code-point splice in order against the registered cut, prove
    /// the planned result, then publish one incremental update.
    fn forward_local_edits(
        &mut self,
        batch: &agent_doc_merge::captured_splice::CapturedSpliceBatch,
    ) -> anyhow::Result<()> {
        for edit in &batch.edits {
            self.replica.apply_local_edit(
                u32::try_from(edit.offset_code_points)?,
                u32::try_from(edit.delete_code_points)?,
                &edit.insert,
            );
        }
        anyhow::ensure!(
            self.replica.text() == batch.resulting_text,
            "forwarded splices did not reproduce the planned cut"
        );
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

    /// Publish this replica's settled visible cut, like the manager's
    /// `projectSettledVisibleState` after a forwarded splice batch. Without it
    /// the controller keeps the retained canonical projection and no peer is
    /// released.
    fn project_visible_state(&self) -> anyhow::Result<()> {
        let content_hash = agent_doc_hash::content_hash(&self.replica.text());
        let projection = request(
            &self.project_root,
            &self.file,
            self.kind,
            "replica_projection",
            serde_json::Map::from_iter([
                ("content_hash".to_string(), Value::String(content_hash)),
                ("disk_persisted".to_string(), Value::Bool(false)),
            ]),
        )?;
        anyhow::ensure!(
            projection["projected"] == Value::Bool(true),
            "settled visible projection was not accepted: {projection}"
        );
        Ok(())
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

fn register_at_canonical_cut(
    project_root: &Path,
    file: &Path,
    kind: PluginHarnessKind,
    expected_canonical_hash: &str,
) -> anyhow::Result<Value> {
    request(
        project_root,
        file,
        kind,
        "replica_register",
        serde_json::Map::from_iter([(
            "expected_canonical_hash".to_string(),
            Value::String(expected_canonical_hash.to_string()),
        )]),
    )
}

/// The controller cut the plugin observes before planning a captured batch.
fn controller_canonical_text(project_root: &Path, file: &Path) -> anyhow::Result<String> {
    let data = agent_doc_controller_io::project_controller::request_crdt_current_text_for_test(
        project_root,
        file,
        "captured-local-splice-recovery",
    )?;
    anyhow::ensure!(
        data["status"].as_str() == Some("current"),
        "controller did not serve a current cut: {data}"
    );
    Ok(data["text"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("current cut omitted text: {data}"))?
        .to_string())
}

fn splice(
    offset_code_points: usize,
    delete_code_points: usize,
    insert: &str,
) -> agent_doc_merge::captured_splice::CapturedSplice {
    agent_doc_merge::captured_splice::CapturedSplice {
        offset_code_points,
        delete_code_points,
        insert: insert.to_string(),
    }
}

fn apply_splices(base: &str, edits: &[agent_doc_merge::captured_splice::CapturedSplice]) -> String {
    let mut chars: Vec<char> = base.chars().collect();
    for edit in edits {
        let end = edit.offset_code_points + edit.delete_code_points;
        chars.splice(edit.offset_code_points..end, edit.insert.chars());
    }
    chars.iter().collect()
}

/// The editor's captured typing burst: ordered code-point splices plus the cut
/// they produced. The resulting cut is what proves the stream is real.
fn captured_splice_batch(
    base: &str,
    edits: Vec<agent_doc_merge::captured_splice::CapturedSplice>,
) -> agent_doc_merge::captured_splice::CapturedSpliceBatch {
    let resulting_text = apply_splices(base, &edits);
    agent_doc_merge::captured_splice::CapturedSpliceBatch {
        edits,
        resulting_text,
    }
}

/// Code-point offset of `needle`, so a Unicode prefix cannot hide a byte-offset
/// bug behind an ASCII fixture.
fn code_point_offset(text: &str, needle: &str) -> usize {
    let byte = text
        .find(needle)
        .unwrap_or_else(|| panic!("fixture is missing `{needle}`"));
    text[..byte].chars().count()
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
        assert!(source.contains("replica_projection"));
        assert!(source.contains("replica_deregister"));
        assert!(source.contains("controller.sock"));
    }
}

#[test]
fn cargo_target_debug_dir_honors_default_relative_and_absolute_targets() {
    let manifest = Path::new("/work/agent-doc");
    assert_eq!(
        cargo_target_debug_dir(manifest, None),
        Path::new("/work/agent-doc/target/debug")
    );
    assert_eq!(
        cargo_target_debug_dir(manifest, Some(OsStr::new("build-cache"))),
        Path::new("/work/agent-doc/build-cache/debug")
    );
    assert_eq!(
        cargo_target_debug_dir(manifest, Some(OsStr::new("/var/tmp/agent-doc-target"))),
        Path::new("/var/tmp/agent-doc-target/debug")
    );
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
        jetbrains.pull_and_project().unwrap();
        vscode.pull_and_project().unwrap();
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
    jetbrains.pull_and_project().unwrap();
    vscode.pull_and_project().unwrap();
    assert_eq!(jetbrains.text(), vscode.text());
    assert!(vscode.text().contains("while-vscode-offline"));

    let ops = std::fs::read_to_string(project_root.join(".agent-doc/logs/ops.log")).unwrap();
    assert!(ops.contains("source=jetbrains_plugin"));
    assert!(ops.contains("source=vscode_plugin"));
    assert!(ops.contains("method=replica_projection"));

    let _ = jetbrains.disconnect().unwrap();
    let _ = vscode.disconnect().unwrap();
}

/// Captured-edit splice recovery across an INDEPENDENTLY ADVANCED canonical
/// response, through the real agent-doc controller.
///
/// The JetBrains peer holds a typing burst its replica never published; the
/// VS Code peer advances canonical with an unrelated response block meanwhile.
/// Recovery must then run the plugin-side sequence in order: observe the
/// controller cut, plan the bounded splices BEFORE replacing membership,
/// CAS-register that exact cut, forward the splices, and land the translated
/// offsets without losing either change.
#[test]
fn captured_editor_splice_recovery_lands_translated_offsets_across_an_independent_response() {
    let _env_guard = agent_doc_test_support::env_lock();
    let temp = tempfile::TempDir::new().unwrap();
    let project_root = temp.path();
    // The emoji keeps every captured offset code-point-scoped: a byte-offset
    // regression cannot pass this fixture by accident.
    let baseline = concat!(
        "<!-- agent:exchange -->\n",
        "do the 🌍 thing\n",
        "<!-- /agent:exchange -->\n",
        "<!-- agent:queue -->\n",
        "- fix bug\n",
        "<!-- /agent:queue -->\n",
    );
    let file = agent_doc_test_support::init_repo_with_doc(project_root, "splice.md", baseline);
    std::fs::create_dir_all(project_root.join(".agent-doc/logs")).unwrap();
    std::fs::create_dir_all(project_root.join(".agent-doc/snapshots")).unwrap();

    let kinds = supported_plugin_harnesses();
    assert_eq!(
        kinds,
        [PluginHarnessKind::JetBrains, PluginHarnessKind::VsCode],
        "splice recovery peers only the implementations whose required features are supported"
    );
    agent_doc_test_support::seed_reliable_sync_editor_registration(
        &file,
        "captured-splice-simworld",
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
    let captured_base = jetbrains.text();
    assert_eq!(captured_base, baseline);

    // The operator retypes a queue line. The burst is captured against
    // `captured_base` and never reaches the controller, so the JetBrains replica
    // is the stale member recovery has to replace.
    let bug_offset = code_point_offset(&captured_base, "bug");
    let captured = captured_splice_batch(
        &captured_base,
        vec![
            splice(bug_offset, 3, "queue "),
            splice(bug_offset + "queue ".chars().count(), 0, "retrieval"),
        ],
    );
    assert!(captured.resulting_text.contains("- fix queue retrieval\n"));

    // Canonical advances independently while that burst is still retained.
    let response = "### Re: the thing — simworld\n";
    let response_offset = code_point_offset(&captured_base, "<!-- /agent:exchange -->");
    vscode
        .edit_without_pulling(response_offset, response)
        .unwrap();
    vscode.pull_and_project().unwrap();

    // 1. Observe the controller cut.
    let canonical = controller_canonical_text(project_root, &file).unwrap();
    assert!(canonical.contains(response), "canonical cut: {canonical:?}");
    assert!(
        canonical.contains("- fix bug\n"),
        "canonical cut: {canonical:?}"
    );
    assert_ne!(canonical, captured_base);

    // 2. Plan the bounded splices BEFORE replacing membership.
    let rebased =
        agent_doc_merge::captured_splice::rebase(&captured_base, &canonical, &captured).unwrap();
    let shift = response.chars().count();
    assert_eq!(
        rebased
            .edits
            .iter()
            .map(|edit| edit.offset_code_points)
            .collect::<Vec<_>>(),
        vec![
            bug_offset + shift,
            bug_offset + "queue ".chars().count() + shift
        ],
        "each captured offset must translate through the independent response insertion"
    );
    let expected = canonical.replacen("- fix bug\n", "- fix queue retrieval\n", 1);
    assert_eq!(rebased.resulting_text, expected);
    assert_ne!(
        apply_splices(&canonical, &captured.edits),
        expected,
        "the untranslated offsets must NOT already produce the right cut, or this proves nothing"
    );

    // 3. CAS-register that exact cut. A stale captured base must refuse while
    //    the existing replica is still the document's only JetBrains peer.
    let stale = register_at_canonical_cut(
        project_root,
        &file,
        PluginHarnessKind::JetBrains,
        &agent_doc_hash::content_hash(&captured_base),
    )
    .expect_err("a stale captured base must not retire the live replica");
    assert!(
        stale
            .to_string()
            .contains("replica_register_canonical_precondition_failed"),
        "unexpected refusal: {stale:#}"
    );
    jetbrains
        .reregister_at_canonical_cut(&agent_doc_hash::content_hash(&canonical))
        .unwrap();
    assert_eq!(
        jetbrains.text(),
        canonical,
        "the replacement replica must start from the cut its CAS proved"
    );
    // The replacement holds exactly the cut it registered against, so it
    // acknowledges the retained canonical projection immediately. Until that
    // receipt lands the controller quarantines every additive update from this
    // member (`recovery=lazy_canonical_projection`).
    jetbrains.project_visible_state().unwrap();

    // 4. Forward the bounded splices, and 5. assert the translated offsets land
    //    on both peers with the unrelated response intact.
    jetbrains.forward_local_edits(&rebased).unwrap();
    jetbrains.project_visible_state().unwrap();
    for _ in 0..3 {
        vscode.pull_and_project().unwrap();
    }
    assert_eq!(jetbrains.text(), expected);
    assert_eq!(vscode.text(), expected);
    assert!(vscode.text().contains(response));
    assert!(vscode.text().contains("- fix queue retrieval\n"));

    let ops = std::fs::read_to_string(project_root.join(".agent-doc/logs/ops.log")).unwrap();
    assert!(ops.contains("source=jetbrains_plugin"));
    assert!(ops.contains("source=vscode_plugin"));

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
    // Rebinding twice must not re-register process-global named Koffi structs.
    // Reconnect reconstructs native replicas from the retained encoded state.
    for _ in 0..2 {
        vscode
            .request(serde_json::json!({ "command": "reload" }))
            .unwrap();
    }
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
    assert!(ops.contains("method=replica_projection"));

    jetbrains.shutdown().unwrap();
    vscode.shutdown().unwrap();
}

/// `#ideriderindependentnav` P5: two simulated JetBrains installations (IDEA
/// and Rider) on one project, end to end on an isolated tmux server, through
/// the enable/disable migration and lane cleanup that `agent-doc gc` runs.
///
/// - Enabling `per_installation` on a project with an existing shared window
///   moves no pane: the shared document stays in the project session, and each
///   installation's lane session starts empty (provisioned exactly as the IDE
///   terminal attach does for that editor client).
/// - Cleanup closes only an empty lane session agent-doc created with no live
///   surface; a lane holding a document pane or a live installation is kept,
///   and a session that only carries a lane's name is never touched.
/// - Disabling (`shared`) folds every lane document pane back into the project
///   session (same pane ids, each exactly once, none lost), then closes the
///   lanes; a second pass is a no-op.
#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn two_ide_navigation_lanes_migrate_and_clean_up_end_to_end() {
    use agent_doc_editor_surface::{LaneReconcileAction, LaneRetainReason};
    use std::collections::{BTreeMap, BTreeSet};

    const IDEA: &str = "jetbrains@IU.0a1b2c3d-pid:41";
    const RIDER: &str = "jetbrains@RD.9f8e7d6c-pid:42";
    const CLION: &str = "jetbrains@CL.11223344-pid:43";

    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
    std::fs::create_dir_all(root.join("tasks")).unwrap();
    let configure = |mode: &str| {
        std::fs::write(
            root.join(".agent-doc/config.toml"),
            format!("tmux_session = \"proj\"\neditor_navigation = \"{mode}\"\n"),
        )
        .unwrap();
    };
    configure("shared");
    let iso = agent_doc_tmux_io::isolated_test_tmux("navlane-p5-e2e");

    let register = |name: &str, pane: &str| {
        let file = root.join(format!("tasks/{name}.md"));
        std::fs::write(&file, format!("# {name}\n")).unwrap();
        let pid: u32 = iso
            .raw_cmd(&["display-message", "-p", "-t", pane, "#{pane_pid}"])
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let window = iso.pane_window(pane).unwrap();
        agent_doc_session_registry_io::registration::register_full_with_cwd_in(
            &root,
            name,
            pane,
            &file.to_string_lossy(),
            pid,
            &window,
            &root.to_string_lossy(),
        )
        .unwrap();
    };
    let sessions = || -> BTreeSet<String> {
        iso.raw_cmd(&["list-sessions", "-F", "#{session_name}"])
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect()
    };
    let panes_by_session = || -> BTreeMap<String, String> {
        iso.raw_cmd(&["list-panes", "-a", "-F", "#{pane_id} #{session_name}"])
            .unwrap()
            .lines()
            .filter_map(|line| {
                let (pane, session) = line.split_once(' ')?;
                Some((pane.to_string(), session.to_string()))
            })
            .collect()
    };

    // The project's existing shared window, with one document pane.
    let shared_pane = iso.new_session("proj", &root).unwrap();
    iso.raw_cmd(&["rename-window", "-t", &shared_pane, "agent-doc"])
        .unwrap();
    let shared_doc = iso.new_window("proj", &root).unwrap();
    register("shared-doc", &shared_doc);
    // A session that only carries a lane's name: never agent-doc's.
    let foreign = iso.new_session("proj-XX-55667788", &root).unwrap();
    let before_enable = panes_by_session();

    // Enable. Each IDE's terminal attach provisions its own lane session.
    configure("per_installation");
    let lane = |client: &str| {
        agent_doc_controller_io::project_controller::navigation_lane_session_for_client(
            &iso, &root, client,
        )
        .unwrap()
        .expect("an installation lane")
        .session
    };
    let (idea, rider, clion) = (lane(IDEA), lane(RIDER), lane(CLION));
    assert_eq!(
        (idea.as_str(), rider.as_str(), clion.as_str()),
        ("proj-IU-0a1b2c3d", "proj-RD-9f8e7d6c", "proj-CL-11223344")
    );
    let after_enable = panes_by_session();
    for (pane, session) in &before_enable {
        assert_eq!(
            after_enable.get(pane),
            Some(session),
            "enable moved no pane ({pane})"
        );
    }
    // IDEA starts a document in its own lane; Rider's lane stays empty while
    // Rider is live; CLion's lane is empty and CLion is gone.
    let idea_doc = iso.new_window(&idea, &root).unwrap();
    register("idea-doc", &idea_doc);

    let live = BTreeSet::from(["RD.9f8e7d6c".to_string()]);
    let reconcile = |live: &BTreeSet<String>, dry_run: bool| {
        agent_doc_controller_io::project_controller::reconcile_navigation_lane_sessions(
            &iso, &root, live, dry_run,
        )
    };
    let dry = reconcile(&live, true);
    assert!(dry.closed.is_empty() && dry.folded.is_empty());
    assert!(sessions().contains(&clion), "a dry run changes nothing");
    let cleanup = reconcile(&live, false);
    let mut actions = cleanup.actions.clone();
    actions.sort_by_key(|action| format!("{action:?}"));
    let mut expected = vec![
        LaneReconcileAction::Close {
            session: clion.clone(),
        },
        LaneReconcileAction::Retain {
            session: idea.clone(),
            reason: LaneRetainReason::OwnedPanes,
        },
        LaneReconcileAction::Retain {
            session: rider.clone(),
            reason: LaneRetainReason::LiveSurface,
        },
        LaneReconcileAction::Retain {
            session: "proj-XX-55667788".to_string(),
            reason: LaneRetainReason::NotMarked,
        },
    ];
    expected.sort_by_key(|action| format!("{action:?}"));
    assert_eq!(actions, expected);
    assert_eq!(cleanup.closed, vec![clion.clone()]);
    assert!(cleanup.failures.is_empty(), "{:?}", cleanup.failures);
    let live_sessions = sessions();
    assert!(!live_sessions.contains(&clion));
    for kept in [
        &idea,
        &rider,
        &"proj".to_string(),
        &"proj-XX-55667788".to_string(),
    ] {
        assert!(live_sessions.contains(kept), "{kept} kept");
    }
    assert_eq!(panes_by_session()[&shared_doc], "proj");
    assert_eq!(panes_by_session()[&idea_doc], idea);

    // Rider places a document in its lane too, then the project disables lanes.
    let rider_doc = iso.new_window(&rider, &root).unwrap();
    register("rider-doc", &rider_doc);
    configure("shared");
    let disable = reconcile(&BTreeSet::new(), false);
    assert!(disable.failures.is_empty(), "{:?}", disable.failures);
    let mut folded = disable.folded.clone();
    folded.sort();
    let mut expected_folded = vec![
        (idea.clone(), idea_doc.clone()),
        (rider.clone(), rider_doc.clone()),
    ];
    expected_folded.sort();
    assert_eq!(folded, expected_folded);
    let mut closed = disable.closed.clone();
    closed.sort();
    let mut expected_closed = vec![idea.clone(), rider.clone()];
    expected_closed.sort();
    assert_eq!(closed, expected_closed);
    let after_disable = panes_by_session();
    for document_pane in [&shared_doc, &idea_doc, &rider_doc] {
        assert_eq!(
            after_disable.get(document_pane).map(String::as_str),
            Some("proj"),
            "{document_pane} lives in the project session exactly once"
        );
    }
    assert_eq!(
        iso.raw_cmd(&["display-message", "-p", "-t", &idea_doc, "#{window_name}"])
            .unwrap()
            .trim(),
        "stash",
        "a folded pane lands in the project session's stash"
    );
    assert_eq!(
        after_disable.get(&foreign).map(String::as_str),
        Some("proj-XX-55667788")
    );
    assert_eq!(
        sessions(),
        BTreeSet::from(["proj".to_string(), "proj-XX-55667788".to_string()])
    );

    // Idempotent: nothing left to fold or close.
    let again = reconcile(&BTreeSet::new(), false);
    assert!(again.folded.is_empty() && again.closed.is_empty());
    assert_eq!(
        again.actions,
        vec![LaneReconcileAction::Retain {
            session: "proj-XX-55667788".to_string(),
            reason: LaneRetainReason::NotMarked,
        }]
    );
}

/// `#ideriderindependentnav` P6, end to end through the real CLI on an isolated
/// tmux server: in `per_installation` mode, `agent-doc claim --new-pane
/// --client-id <id>` (what each IDE's Claim actions run) starts the document's
/// pane in the requesting IDE's own lane session, provisioned on first use, and
/// stages it in that lane's `stash` (the lane's layout places it; the lane's
/// idle placeholder never blocks it). A claim without a client, or from an
/// unscoped client, starts in the shared project session; the shared window
/// is never touched by a lane request. A pane already in the requester's lane
/// passes the cross-session check for that requester only; another IDE's
/// claim of it is still refused. In `shared` mode a client id changes nothing.
#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn two_ide_lane_claims_start_panes_in_their_own_lanes_end_to_end() {
    const IDEA: &str = "jetbrains@IU.0a1b2c3d-pid:41";
    const RIDER: &str = "jetbrains@RD.9f8e7d6c-pid:42";
    const IDEA_LANE: &str = "proj-IU-0a1b2c3d";
    const RIDER_LANE: &str = "proj-RD-9f8e7d6c";

    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
    let configure = |mode: &str| {
        std::fs::write(
            root.join(".agent-doc/config.toml"),
            format!("tmux_session = \"proj\"\neditor_navigation = \"{mode}\"\n"),
        )
        .unwrap();
    };
    configure("per_installation");
    let iso = agent_doc_tmux_io::isolated_test_tmux("navlane-p6-e2e");
    let socket = iso.server_socket.clone().expect("isolated socket");

    // The operator's shared window.
    let shared_pane = iso.new_session("proj", &root).unwrap();
    iso.raw_cmd(&["rename-window", "-t", &shared_pane, "agent-doc"])
        .unwrap();
    let shared_window_panes = || iso.list_window_panes("proj:agent-doc").unwrap();
    let shared_before = shared_window_panes();

    let document = |name: &str| {
        let file = root.join(format!("{name}.md"));
        std::fs::write(
            &file,
            format!("---\nagent: claude\nclaude_args: --version\n---\n# {name}\n"),
        )
        .unwrap();
        file
    };
    let claim = |file: &Path, extra: &[&str], client: Option<&str>| {
        let mut command = agent_doc();
        command
            .current_dir(&root)
            .arg("claim")
            .arg(file)
            .args(extra)
            .env("AGENT_DOC_TMUX_SOCKET", &socket)
            .env_remove("TMUX")
            .env_remove("TMUX_PANE");
        if let Some(client) = client {
            command.args(["--client-id", client]);
        }
        command.output().unwrap()
    };
    let all_panes = || -> std::collections::BTreeSet<String> {
        iso.raw_cmd(&["list-panes", "-a", "-F", "#{pane_id}"])
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect()
    };
    // Run a provisioning claim and return the document pane it started (a
    // lane's first use also creates the lane's own placeholder panes).
    let provision = |file: &Path, client: Option<&str>| {
        let before = all_panes();
        let output = claim(file, &["--new-pane"], client);
        assert!(output.status.success(), "{output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let pane = stderr
            .lines()
            .find(|line| line.starts_with("[route] Started "))
            .and_then(|line| line.split(" in pane ").nth(1))
            .and_then(|rest| rest.split_whitespace().next())
            .unwrap_or_else(|| panic!("no started pane: {stderr}"))
            .to_string();
        assert!(!before.contains(&pane), "a new pane: {stderr}");
        pane
    };
    let session_of = |pane: &str| iso.pane_session(pane).unwrap();

    // IDEA and Rider each start a document in their own lane.
    let idea_pane = provision(&document("idea"), Some(IDEA));
    assert_eq!(session_of(&idea_pane), IDEA_LANE);
    let idea_lane_window = iso
        .list_window_panes(&format!("{IDEA_LANE}:agent-doc"))
        .unwrap();
    assert!(
        !idea_lane_window.contains(&idea_pane),
        "the pane is staged for the lane's layout, not split into its window"
    );
    assert_eq!(
        session_of(&provision(&document("rider"), Some(RIDER))),
        RIDER_LANE
    );

    // No client, and an unscoped client: the shared session.
    assert_eq!(session_of(&provision(&document("plain"), None)), "proj");
    assert_eq!(
        session_of(&provision(&document("vscode"), Some("vscode-pid:9"))),
        "proj"
    );
    assert_eq!(
        shared_window_panes(),
        shared_before,
        "no request changed the shared agent-doc window"
    );

    // An unowned pane in IDEA's lane: IDEA may claim it; Rider may not.
    let spare = iso.new_window(IDEA_LANE, &root).unwrap();
    let bound_doc = document("bound");
    let refused = claim(&bound_doc, &["--pane", &spare], Some(RIDER));
    assert!(!refused.status.success(), "{refused:?}");
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("project session is 'proj'"),
        "{refused:?}"
    );
    let panes_before_bind = all_panes();
    let output = claim(&bound_doc, &["--pane", &spare], Some(IDEA));
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        all_panes(),
        panes_before_bind,
        "the claim bound the existing lane pane instead of provisioning"
    );

    // Shared mode: a client id is ignored.
    configure("shared");
    assert_eq!(
        session_of(&provision(&document("shared-mode"), Some(IDEA))),
        "proj"
    );
}
