//! `#startupcriticalpath`: the whole `agent-doc tmux start` process the editor
//! waits on, for a document whose supervisor pane is already live, is the
//! critical path and nothing else.
//!
//! The unit tests in `agent-doc-start-io` count the bootstrap decision; this
//! runs the REAL binary, so the command-entry preamble (`try_main`) is counted
//! too: no stale-supervisor probe, no upgrade check, three tmux client spawns
//! (`display-message`, `list-panes`, `list-clients`) and no registry lock.
//!
//! The registry lock is proven untaken by holding it in this test process for
//! the whole start: a start that tried to take it would give up at the bounded
//! wait (`#registrylockbound`) and fail naming the lock. Counts, not wall
//! clock. Isolated tmux server; the start never launches a harness because the
//! document's pane is reused.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Output;

const DOC: &str = "---\nagent_doc_session: 00000000-0000-4000-8000-0000000000c7\nagent: claude\n---\n\n# Session\n\n## User\n\nhello\n";

fn agent_doc_bin() -> PathBuf {
    std::env::var_os("AGENT_DOC_BIN")
        .map(PathBuf::from)
        .filter(|path| path.exists())
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_agent-doc")))
}

fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

struct Project {
    root: PathBuf,
    doc: PathBuf,
    home: PathBuf,
    log: PathBuf,
    socket: String,
}

impl Project {
    fn run(&self, args: &[&str], extra_env: &[(&str, &str)]) -> Output {
        let mut command = std::process::Command::new(agent_doc_bin());
        command
            .current_dir(&self.root)
            .args(args)
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .env_remove("CLAUDECODE")
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("AGENT_DOC_TMUX_SOCKET", &self.socket)
            .env("AGENT_DOC_PROGRESS", "1");
        for (key, value) in extra_env {
            command.env(key, value);
        }
        command.output().unwrap()
    }

    /// The tmux verbs logged since the last call, in order.
    fn take_tmux_verbs(&self) -> Vec<String> {
        let verbs = std::fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .map(|line| {
                let mut words = line.split_whitespace();
                while let Some(word) = words.next() {
                    if matches!(word, "-L" | "-S" | "-f") {
                        words.next();
                        continue;
                    }
                    return word.to_string();
                }
                String::new()
            })
            .collect();
        std::fs::write(&self.log, "").unwrap();
        verbs
    }
}

fn describe(output: &Output) -> String {
    format!(
        "status={:?}\n--- stdout ---\n{}\n--- stderr ---\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn reuse_tmux_start_process_is_the_critical_path_only() {
    let tmp = tempfile::TempDir::new().unwrap();
    let _controllers = agent_doc_test_support::ProjectControllerReaper::under(tmp.path());
    let base = tmp.path().canonicalize().unwrap();
    let root = base.join("project");
    let home = base.join("home");
    std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
    std::fs::create_dir_all(home.join(".config")).unwrap();

    let tmux = agent_doc_tmux_io::isolated_test_tmux("startup-critical-path");
    let socket = tmux
        .socket_path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let log = base.join("tmux-calls.log");
    let wrapper = base.join("tmux-wrapper");
    std::fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\necho \"$*\" >> '{}'\nexec '{}' \"$@\"\n",
            log.display(),
            tmux.binary_path().display()
        ),
    )
    .unwrap();
    make_executable(&wrapper);
    std::fs::write(
        root.join(".agent-doc/config.toml"),
        format!(
            "tmux_session = \"proj\"\ntmux_bin = \"{}\"\n",
            wrapper.display()
        ),
    )
    .unwrap();

    let doc = root.join("session.md");
    std::fs::write(&doc, DOC).unwrap();
    let (ready, _) = agent_doc_frontmatter_io::session::ensure_session_for_file(DOC, &doc).unwrap();
    std::fs::write(&doc, &ready).unwrap();
    assert!(agent_doc_frontmatter_io::session::session_metadata_ready_on_disk(&doc));

    let project = Project {
        root: root.clone(),
        doc: doc.clone(),
        home,
        log,
        socket,
    };

    // Provision the project session and record the server identity.
    let ensure = project.run(&["tmux", "ensure", "session.md", "--json"], &[]);
    assert!(ensure.status.success(), "{}", describe(&ensure));

    // The document's live supervisor pane, plus unrelated live rows.
    let mut registry = tmux_router::Registry::new();
    let mut document_pane = String::new();
    for row in 0..8 {
        let pane = tmux.new_window("proj", &root).unwrap();
        let file = if row == 0 {
            document_pane = pane.clone();
            project.doc.clone()
        } else {
            let other = root.join(format!("other-{row}.md"));
            std::fs::write(&other, "---\nagent_doc_session: other\n---\n").unwrap();
            other
        };
        registry.insert(
            format!("key-{row}"),
            tmux_router::RegistryEntry {
                pane,
                pid: 1,
                cwd: root.display().to_string(),
                started: "2026-11-05T00:00:00Z".to_string(),
                session_id: format!("session-{row}"),
                file: file.display().to_string(),
                window: String::new(),
                supervisor_instance_id: String::new(),
            },
        );
    }
    agent_doc_session_registry_io::save_in(&root, &registry).unwrap();
    project.take_tmux_verbs();

    let registry_path = agent_doc_session_registry_io::registry_path_in(&root);
    let held = tmux_router::RegistryLock::acquire(&registry_path).unwrap();
    let start = project.run(
        &["tmux", "start", "session.md", "--client-id", "test-client"],
        &[("AGENT_DOC_REGISTRY_LOCK_WAIT_MS", "2000")],
    );
    drop(held);
    let report = describe(&start);
    assert!(start.status.success(), "{report}");
    let stdout = String::from_utf8_lossy(&start.stdout);
    assert!(
        stdout.contains(&format!("pane {document_pane}")),
        "the start reuses the document's pane\n{report}"
    );
    let stderr = String::from_utf8_lossy(&start.stderr);
    let phases = stderr
        .lines()
        .filter_map(|line| line.strip_prefix("[progress] phase="))
        .collect::<Vec<_>>();
    assert!(phases.contains(&"reused"), "{report}");
    for off_path in ["supervisor_freshness_probe", "registry_lock_wait"] {
        assert!(
            !phases.contains(&off_path),
            "`{off_path}` is not on the start's critical path\n{report}"
        );
    }
    assert_eq!(
        project.take_tmux_verbs(),
        ["display-message", "list-panes", "list-clients"],
        "{report}"
    );
}
