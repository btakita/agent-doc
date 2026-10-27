//! `#startcwdfallback` live: `agent-doc start` of a document with no
//! `.agent-doc` ancestor, run from a pane whose cwd is inside ANOTHER agent-doc
//! project, binds the document in the store every reader of it consults (its
//! canonical parent, `project_root_or_file_parent`) and writes nothing into the
//! cwd's project.
//!
//! Start admission (`prepare_start_runtime_with_admission`) used to resolve the
//! store as `project_root_containing(doc).unwrap_or(current_dir())`, while
//! start's bootstrap paths, the route, claim (`#claimwrongstore`) and sync
//! (`#syncnorootfallback`) fall back to the canonical parent. The cwd project
//! then received the actor record, the registry entry, the project controller,
//! and (through its `tmux_session` pin) a relocation target and a config
//! rebind.
//!
//! The test runs the REAL in-pane start (an isolated tmux server, a temp tree
//! holding both projects, `#prunetestisolation`). The harness is a fake
//! `claude` (`sleep`) placed first on the pane's `PATH`; the wrapper refuses to
//! start unless `claude` resolves to it, so no real harness is ever launched.
//!
//! Live tmux: `#[ignore]`, run through `make tmux-ci`.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const SESSION_ID: &str = "00000000-0000-4000-8000-0000000000d1";
const DOC: &str = "---\nagent_doc_session: 00000000-0000-4000-8000-0000000000d1\nagent: claude\n---\n\n# Session\n\n## User\n\nhello\n";
const FOREIGN_CONFIG: &str = "tmux_session = \"foreign-pin\"\n";
/// Fake harness: records that the supervisor launched it, then idles.
const FAKE_CLAUDE: &str = "#!/bin/sh\n# Fake harness (#startcwdfallback).\ntouch \"$FAKE_CLAUDE_STARTED\"\nexec sleep 600\n";

fn agent_doc_bin() -> PathBuf {
    std::env::var_os("AGENT_DOC_BIN")
        .map(PathBuf::from)
        .filter(|path| path.exists())
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_agent-doc")))
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn state_db(root: &Path) -> PathBuf {
    root.join(".agent-doc/state.db")
}

/// The document's actor record in `root`'s store, read only once that store
/// exists (a read must never create the store it is asked about).
fn actor_record_in(root: &Path, doc: &Path) -> Option<agent_doc_controller::actor::ActorRecord> {
    if !state_db(root).is_file() {
        return None;
    }
    agent_doc_session_actor_io::load_record_in(root, &doc.to_string_lossy())
        .ok()
        .flatten()
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn start_of_a_rootless_document_from_another_projects_cwd_binds_in_its_parent_store() {
    // Drop order is reverse declaration order: the tmux server (supervisor and
    // fake harness) dies first, then any project controller, then the tree.
    let tmp = tempfile::TempDir::new().unwrap();
    let _controllers = agent_doc_test_support::ProjectControllerReaper::under(tmp.path());
    let base = tmp.path().canonicalize().unwrap();
    let foreign = base.join("foreign");
    let notes = base.join("notes");
    let bin_dir = base.join("bin");
    let home = base.join("home");
    for dir in [
        foreign.join(".agent-doc"),
        notes.clone(),
        bin_dir.clone(),
        home.clone(),
    ] {
        std::fs::create_dir_all(dir).unwrap();
    }
    // The cwd project pins a tmux session, so a start that reads the cwd's
    // config targets (and rebinds) that pin.
    std::fs::write(foreign.join(".agent-doc/config.toml"), FOREIGN_CONFIG).unwrap();
    let doc = notes.join("rootless.md");
    std::fs::write(&doc, DOC).unwrap();
    assert_eq!(
        agent_doc_project_root_io::project_root_containing(&doc),
        None,
        "fixture: the document has no .agent-doc ancestor"
    );
    let fake_claude = bin_dir.join("claude");
    std::fs::write(&fake_claude, FAKE_CLAUDE).unwrap();
    make_executable(&fake_claude);
    std::os::unix::fs::symlink(agent_doc_bin(), bin_dir.join("agent-doc")).unwrap();

    let tmux = agent_doc_tmux_io::isolated_test_tmux("start-rootless-store");
    let socket = tmux
        .socket_path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let path_env = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let ceiling = std::env::var_os("AGENT_DOC_ROOT_CEILING_DIRECTORIES")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let started = base.join("harness-started");
    let run = bin_dir.join("run-start.sh");
    std::fs::write(
        &run,
        format!(
            "#!/bin/sh\nexport PATH={path}\nexport HOME={home}\nexport XDG_CONFIG_HOME={config}\nexport AGENT_DOC_TMUX_SOCKET={socket}\nexport AGENT_DOC_ROOT_CEILING_DIRECTORIES={ceiling}\nexport {launch_env}=startcwdfallback_live_start\nexport FAKE_CLAUDE_STARTED={started}\nunset CLAUDECODE\nif [ \"$(command -v claude)\" != {claude} ]; then echo 'refusing: claude is not the fake harness'; sleep 600; exit 99; fi\ncd {cwd}\nexec agent-doc start {doc}\n",
            path = shell_quote(&path_env),
            home = shell_quote(&home.to_string_lossy()),
            config = shell_quote(&home.join(".config").to_string_lossy()),
            socket = shell_quote(&socket),
            ceiling = shell_quote(&ceiling.to_string_lossy()),
            launch_env = agent_doc_controller_io::project_controller::TEST_CONTROLLER_LAUNCH_REASON_ENV,
            claude = shell_quote(&fake_claude.to_string_lossy()),
            cwd = shell_quote(&foreign.to_string_lossy()),
            started = shell_quote(&started.to_string_lossy()),
            doc = shell_quote(&doc.to_string_lossy()),
        ),
    )
    .unwrap();
    make_executable(&run);
    let output = tmux
        .cmd()
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .args([
            "new-session",
            "-d",
            "-s",
            "main",
            "-x",
            "160",
            "-y",
            "40",
            "-c",
        ])
        .arg(&foreign)
        .args(["-P", "-F", "#{pane_id}"])
        .arg(&run)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "tmux new-session: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let pane = String::from_utf8_lossy(&output.stdout).trim().to_string();

    // Admission (store resolution, controller start, actor + registry bind,
    // relocation) is complete once the supervisor has launched the harness.
    let deadline = Instant::now() + Duration::from_secs(45);
    while Instant::now() < deadline && !started.exists() {
        std::thread::sleep(Duration::from_millis(100));
    }
    let screen = tmux.capture_pane(&pane, None).unwrap_or_default();
    assert!(
        started.exists(),
        "the supervisor never launched the fake harness\n--- pane ---\n{screen}"
    );

    let record = actor_record_in(&notes, &doc).unwrap_or_else(|| {
        panic!(
            "start bound no actor in the document's parent store {}; cwd store exists={}\n--- pane ---\n{screen}",
            notes.display(),
            state_db(&foreign).exists()
        )
    });
    assert_eq!(
        (record.pane_id.as_str(), record.session_id.as_str()),
        (pane.as_str(), SESSION_ID),
        "pane ---\n{screen}"
    );
    assert!(
        agent_doc_session_registry_io::load_in(&notes)
            .unwrap()
            .values()
            .any(|entry| entry.session_id == SESSION_ID && entry.pane == pane),
        "the start's registry entry is in the document's parent store\n--- pane ---\n{screen}"
    );

    // Nothing lands in the cwd's project: no store (so no actor, registry
    // entry or controller), and its config pin is neither the start's target
    // nor rebound to the pane's session.
    assert!(
        !state_db(&foreign).exists(),
        "start created a store in the process cwd's project\n--- pane ---\n{screen}"
    );
    assert_eq!(
        std::fs::read_to_string(foreign.join(".agent-doc/config.toml")).unwrap(),
        FOREIGN_CONFIG,
        "start rewrote the process cwd's project config\n--- pane ---\n{screen}"
    );
    assert_eq!(
        tmux.pane_session(&pane).unwrap(),
        "main",
        "start relocated the pane toward the cwd project's tmux_session pin"
    );
    assert_eq!(
        agent_doc_project_config_io::project_tmux_session_for_root(&notes),
        None,
        "start pinned the document store to a session it read from the cwd's config"
    );
}
