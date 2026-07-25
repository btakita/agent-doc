#![cfg(target_os = "linux")]

use libloading::{Library, Symbol};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{CStr, CString, c_char, c_int};
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};

type StartListener =
    unsafe extern "C" fn(*const c_char, extern "C" fn(*const c_char) -> c_int) -> c_int;
type EnqueueLiveness = unsafe extern "C" fn(*const c_char, *const c_char, *const c_char) -> c_int;
type FlushLiveness = unsafe extern "C" fn(*const c_char, *const c_char) -> i64;
type Quiesce = unsafe extern "C" fn(i64) -> c_int;
type Version = unsafe extern "C" fn() -> *mut c_char;
type FreeString = unsafe extern "C" fn(*mut c_char);
type ReplicaOpen = unsafe extern "C" fn(u64, *const u8, usize) -> c_int;

extern "C" fn accept_message(_message: *const c_char) -> c_int {
    1
}

fn built_cdylib() -> PathBuf {
    let test_exe = std::env::current_exe().expect("current test executable");
    let target_profile = test_exe
        .parent()
        .and_then(Path::parent)
        .expect("target profile directory");
    let library = target_profile.join("libagent_doc.so");
    assert!(
        library.is_file(),
        "{} is missing; the test target must build --lib before running integration tests",
        library.display()
    );
    library
}

fn mapped(path: &Path) -> bool {
    let needle = path.to_string_lossy();
    std::fs::read_to_string("/proc/self/maps")
        .expect("read process maps")
        .lines()
        .any(|line| line.contains(needle.as_ref()))
}

fn plugin_state_db_fd_count() -> usize {
    std::fs::read_dir("/proc/self/fd")
        .expect("read process descriptors")
        .filter_map(Result::ok)
        .filter_map(|entry| std::fs::read_link(entry.path()).ok())
        .filter(|target| target.to_string_lossy().contains(".agent-doc/state.db"))
        .count()
}

fn process_thread_names() -> Vec<String> {
    std::fs::read_dir("/proc/self/task")
        .expect("read process threads")
        .filter_map(Result::ok)
        .filter_map(|entry| std::fs::read_to_string(entry.path().join("comm")).ok())
        .map(|name| name.trim().to_string())
        .collect()
}

fn assert_one_controller_state_inode_generation(project: &Path) {
    let status =
        agent_doc_controller_io::project_controller::status(project).expect("controller status");
    let pid = status.pid.expect("controller pid");
    let mut inodes = BTreeMap::<String, BTreeSet<(u64, u64)>>::new();
    for entry in std::fs::read_dir(format!("/proc/{pid}/fd"))
        .expect("read controller descriptors")
        .filter_map(Result::ok)
    {
        let Ok(target) = std::fs::read_link(entry.path()) else {
            continue;
        };
        let target = target.to_string_lossy();
        let kind = ["state.db", "state.db-wal", "state.db-shm"]
            .into_iter()
            .find(|name| target.contains(name));
        let Some(kind) = kind else {
            continue;
        };
        assert!(
            !target.contains("(deleted)"),
            "controller retained deleted SQLite descriptor: {target}"
        );
        let metadata = std::fs::metadata(entry.path()).expect("stat controller descriptor");
        inodes
            .entry(kind.to_string())
            .or_default()
            .insert((metadata.dev(), metadata.ino()));
    }
    for (kind, generations) in inodes {
        assert!(
            generations.len() <= 1,
            "controller has split {kind} inode generations: {generations:?}"
        );
    }
}

#[test]
fn native_generation_version_call_unmaps_after_owned_worker_exits() {
    let source = built_cdylib();
    let temp = tempfile::tempdir().unwrap();
    let generation = temp.path().join("libagent_doc-version-only.so");
    std::fs::copy(&source, &generation).unwrap();

    let worker_path = generation.clone();
    let library = std::thread::spawn(move || unsafe {
        let library = Library::new(&worker_path).expect("load version-only generation");
        {
            let version: Symbol<Version> = library.get(b"agent_doc_version").unwrap();
            let free_string: Symbol<FreeString> = library.get(b"agent_doc_free_string").unwrap();
            let version_ptr = version();
            assert!(!version_ptr.is_null());
            free_string(version_ptr);
        }
        library
    })
    .join()
    .expect("version-only worker exits");
    drop(library);
    assert!(
        !mapped(&generation),
        "a generation called only from its retired worker must unmap"
    );
}

#[test]
fn native_generation_listener_unmaps_after_owned_worker_exits() {
    let source = built_cdylib();
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    std::fs::create_dir_all(project.join(".agent-doc")).unwrap();
    let generation = temp.path().join("libagent_doc-listener-only.so");
    std::fs::copy(&source, &generation).unwrap();
    let root = CString::new(project.to_string_lossy().as_bytes()).unwrap();

    let worker_path = generation.clone();
    let library = std::thread::spawn(move || unsafe {
        let library = Library::new(&worker_path).expect("load listener-only generation");
        {
            let start: Symbol<StartListener> =
                library.get(b"agent_doc_start_ipc_listener_v2").unwrap();
            let quiesce: Symbol<Quiesce> = library.get(b"agent_doc_quiesce_for_reload").unwrap();
            assert_eq!(start(root.as_ptr(), accept_message), 1);
            assert_eq!(quiesce(10_000), 1);
        }
        library
    })
    .join()
    .expect("listener-only worker exits");
    drop(library);
    assert!(
        !mapped(&generation),
        "joined native listener threads must not pin their generation"
    );
}

unsafe fn exercise_generation(
    library_path: &Path,
    project_root: &CString,
    document_hash: &CString,
    ops: &CString,
) -> Library {
    let library = unsafe { Library::new(library_path) }.expect("load native generation");
    {
        let start: Symbol<StartListener> =
            unsafe { library.get(b"agent_doc_start_ipc_listener_v2") }.unwrap();
        let enqueue: Symbol<EnqueueLiveness> =
            unsafe { library.get(b"agent_doc_reliable_sync_liveness_enqueue") }.unwrap();
        let flush: Symbol<FlushLiveness> =
            unsafe { library.get(b"agent_doc_reliable_sync_liveness_flush") }.unwrap();
        let version: Symbol<Version> = unsafe { library.get(b"agent_doc_version") }.unwrap();
        let free_string: Symbol<FreeString> =
            unsafe { library.get(b"agent_doc_free_string") }.unwrap();
        let quiesce: Symbol<Quiesce> =
            unsafe { library.get(b"agent_doc_quiesce_for_reload") }.unwrap();
        let replica_open: Symbol<ReplicaOpen> =
            unsafe { library.get(b"agent_doc_replica_open") }.unwrap();

        let version_ptr = unsafe { version() };
        assert!(!version_ptr.is_null());
        assert!(
            !unsafe { CStr::from_ptr(version_ptr) }
                .to_string_lossy()
                .is_empty()
        );
        unsafe { free_string(version_ptr) };
        assert_eq!(
            unsafe { start(project_root.as_ptr(), accept_message) },
            1,
            "listener must start in the active generation"
        );
        assert_eq!(
            unsafe { replica_open(0xA6E17, std::ptr::null(), 0) },
            0,
            "an active CRDT replica must be owned by this generation"
        );
        assert_eq!(
            unsafe { enqueue(project_root.as_ptr(), document_hash.as_ptr(), ops.as_ptr()) },
            0,
            "thin native client must durably enqueue through the controller"
        );
        assert!(
            unsafe { flush(project_root.as_ptr(), document_hash.as_ptr()) } >= 1,
            "controller-owned outbox must acknowledge the frame"
        );
        assert_eq!(
            plugin_state_db_fd_count(),
            0,
            "reloadable plugin process must not own a SQLite state connection"
        );
        assert_eq!(
            unsafe { quiesce(10_000) },
            1,
            "listener threads and native replicas must reach the unload fence"
        );
    }
    library
}

#[test]
fn native_generations_handoff_without_sqlite_or_deleted_library_state() {
    let source = built_cdylib();
    let agent_doc_bin = source
        .parent()
        .expect("target profile directory")
        .join("agent-doc");
    assert!(
        agent_doc_bin.is_file(),
        "{} is missing; build the test binary before this regression",
        agent_doc_bin.display()
    );
    // The integration-test executable is deliberately not a controller-capable
    // `agent-doc` binary. Point controller launch at the sibling build so the
    // loaded cdylib and controller exercise the same source generation.
    unsafe { std::env::set_var("AGENT_DOC_BIN", &agent_doc_bin) };
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    std::fs::create_dir_all(project.join(".agent-doc")).unwrap();
    let generation_one = temp.path().join("libagent_doc-generation-1.so");
    let generation_two = temp.path().join("libagent_doc-generation-2.so");
    std::fs::copy(&source, &generation_one).unwrap();
    std::fs::copy(&source, &generation_two).unwrap();

    let project_root = CString::new(project.to_string_lossy().as_bytes()).unwrap();
    let document_hash = CString::new("native-reload-handoff").unwrap();
    let ops = CString::new(
        r#"[{"Open":{"document_hash":"native-reload-handoff","pid":4242,"tag":"integration"}}]"#,
    )
    .unwrap();

    {
        let library = generation_one.clone();
        let root = project_root.clone();
        let hash = document_hash.clone();
        let frame = ops.clone();
        let library = std::thread::spawn(move || unsafe {
            exercise_generation(&library, &root, &hash, &frame)
        })
        .join()
        .expect("first generation worker exits cleanly");
        drop(library);
    }
    assert!(
        !agent_doc_ipc_io::socket_path(&project).exists(),
        "the retired first generation must not retain a listener"
    );
    assert!(
        !mapped(&generation_one),
        "worker exit must run Rust TLS destructors so dlclose unmaps generation one; threads={:?}",
        process_thread_names(),
    );

    {
        let library = generation_two.clone();
        let root = project_root.clone();
        let hash = document_hash.clone();
        let frame = ops.clone();
        let library = std::thread::spawn(move || unsafe {
            exercise_generation(&library, &root, &hash, &frame)
        })
        .join()
        .expect("second generation worker exits cleanly");
        drop(library);
    }
    assert!(
        !agent_doc_ipc_io::socket_path(&project).exists(),
        "the retired second generation must not retain a listener"
    );
    assert!(
        !mapped(&generation_two),
        "worker exit must run Rust TLS destructors so dlclose unmaps generation two"
    );

    let connection =
        agent_doc_sqlite::state_store::open_state_db(&project).expect("open controller state db");
    let quick_check: String = connection
        .query_row("PRAGMA quick_check", [], |row| row.get(0))
        .expect("SQLite quick_check");
    assert_eq!(quick_check, "ok");
    drop(connection);
    assert_eq!(plugin_state_db_fd_count(), 0);

    let maps = std::fs::read_to_string("/proc/self/maps").unwrap();
    assert!(
        !maps.lines().any(|line| {
            line.contains("libagent_doc-generation-") && line.contains("(deleted)")
        }),
        "generation handoff must not leave deleted native mappings"
    );
    assert_one_controller_state_inode_generation(&project);
}
