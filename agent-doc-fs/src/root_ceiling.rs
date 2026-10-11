//! Project-root discovery ceilings (`#testisolationtests`).
//!
//! `.agent-doc` discovery walks up from a document or the working directory to
//! the nearest ancestor holding `.agent-doc/`. Unbounded, that walk let a test
//! whose `TMPDIR` sat under `$HOME` adopt the operator's real `~/.agent-doc`
//! as its project root: tests wrote the operator's `state.db`, `ops.log`,
//! proof ledger and snapshots, and spawned controllers with
//! `--project-root $HOME` (2026-10-10).
//!
//! [`ROOT_CEILING_ENV`] bounds the walk the way `GIT_CEILING_DIRECTORIES`
//! bounds git's repository discovery: a list of absolute directories
//! (separated like `PATH`) that discovery never climbs *into*. The starting
//! directory is always examined, even when it is itself a ceiling, but no
//! ceiling above the start and nothing above a ceiling is ever examined.
//! Relative and empty entries are ignored, as git does.
//!
//! Unset, production discovery is unchanged. A process that is a cargo test
//! binary (its executable lives in a `deps/` directory) and has no explicit
//! ceiling gets an implicit ceiling at [`std::env::temp_dir`], so a raw
//! `cargo test` / `cargo nextest run` cannot walk out of its own temp tree
//! either. Child processes (the spawned `agent-doc` binary, controllers) only
//! inherit the explicit variable, which is why the Makefile and the nextest
//! setup script export it.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Environment variable naming the discovery ceiling directories.
pub const ROOT_CEILING_ENV: &str = "AGENT_DOC_ROOT_CEILING_DIRECTORIES";

/// The ceiling directories in force for this process: the explicit
/// [`ROOT_CEILING_ENV`] list when the variable is present (even empty), else
/// the implicit test-binary ceiling, else none.
pub fn root_ceiling_directories() -> Vec<PathBuf> {
    match std::env::var_os(ROOT_CEILING_ENV) {
        Some(value) => parse_root_ceilings(&value),
        None if running_as_cargo_test_binary() => with_canonical(vec![std::env::temp_dir()]),
        None => Vec::new(),
    }
}

/// Parse a `PATH`-style ceiling list. Relative and empty entries are ignored;
/// each surviving entry is kept both as written and canonicalized so a symlinked
/// temp root (`/tmp` vs `/private/tmp`) is matched whichever spelling the walk
/// carries.
pub fn parse_root_ceilings(value: &OsString) -> Vec<PathBuf> {
    with_canonical(
        std::env::split_paths(value)
            .filter(|entry| !entry.as_os_str().is_empty() && entry.is_absolute())
            .collect(),
    )
}

fn with_canonical(entries: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut out = Vec::with_capacity(entries.len() * 2);
    for entry in entries {
        let canonical = entry.canonicalize().ok();
        let trimmed = strip_trailing_separators(entry);
        if !out.contains(&trimmed) {
            out.push(trimmed);
        }
        if let Some(canonical) = canonical
            && !out.contains(&canonical)
        {
            out.push(canonical);
        }
    }
    out
}

fn strip_trailing_separators(path: PathBuf) -> PathBuf {
    path.components().collect()
}

/// Whether this process is a cargo-built test harness binary
/// (`target/<profile>/deps/<crate>-<hash>`). Shipped binaries never live in a
/// `deps/` directory, so production behaviour is unaffected.
pub fn running_as_cargo_test_binary() -> bool {
    static IS_TEST_BINARY: OnceLock<bool> = OnceLock::new();
    *IS_TEST_BINARY.get_or_init(|| {
        std::env::current_exe().ok().is_some_and(|exe| {
            exe.parent()
                .and_then(Path::file_name)
                .is_some_and(|name| name == "deps")
        })
    })
}

/// The directories project-root discovery may examine, nearest first, starting
/// at `start` and stopping below the first ceiling in `ceilings`.
pub fn ancestors_within(start: &Path, ceilings: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for (index, dir) in start.ancestors().enumerate() {
        if dir.as_os_str().is_empty() {
            break;
        }
        let is_ceiling = ceilings.iter().any(|ceiling| ceiling == dir);
        if index > 0 && is_ceiling {
            break;
        }
        out.push(dir.to_path_buf());
        if is_ceiling {
            break;
        }
    }
    out
}

/// [`ancestors_within`] bounded by the process ceilings
/// ([`root_ceiling_directories`]).
pub fn ancestors_within_root_ceiling(start: &Path) -> Vec<PathBuf> {
    ancestors_within(start, &root_ceiling_directories())
}

/// The nearest directory at or above `path` holding an `.agent-doc/`
/// directory, ignoring every ceiling. This is the test-harness guard's probe:
/// a test `TMPDIR` for which this returns `Some` would let unbounded discovery
/// adopt a real project (often `$HOME/.agent-doc`) as a test's root.
pub fn agent_doc_ancestor(path: &Path) -> Option<PathBuf> {
    let anchored = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    anchored
        .ancestors()
        .find(|dir| dir.join(".agent-doc").is_dir())
        .map(Path::to_path_buf)
}

/// Refuse a test temp root that sits under a real `.agent-doc` project.
pub fn ensure_test_tmpdir_isolated(tmpdir: &Path) -> anyhow::Result<()> {
    if let Some(root) = agent_doc_ancestor(tmpdir) {
        anyhow::bail!(
            "test TMPDIR {} is inside the agent-doc project {} ({}); tests would walk up and \
             write that project's state. Use a TMPDIR outside any .agent-doc tree, e.g. \
             TMPDIR=/tmp/agent-doc-test (#testisolationtests)",
            tmpdir.display(),
            root.display(),
            root.join(".agent-doc").display(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walk_stops_below_a_ceiling_and_keeps_the_start() {
        let ceilings = vec![PathBuf::from("/a/b")];
        assert_eq!(
            ancestors_within(Path::new("/a/b/c/d"), &ceilings),
            vec![PathBuf::from("/a/b/c/d"), PathBuf::from("/a/b/c")]
        );
        // The start is examined even when it is itself a ceiling, but nothing above it.
        assert_eq!(
            ancestors_within(Path::new("/a/b"), &ceilings),
            vec![PathBuf::from("/a/b")]
        );
        // A ceiling that is not an ancestor of the start bounds nothing.
        assert_eq!(
            ancestors_within(Path::new("/x/y"), &ceilings),
            vec![
                PathBuf::from("/x/y"),
                PathBuf::from("/x"),
                PathBuf::from("/")
            ]
        );
    }

    #[test]
    fn relative_and_empty_ceiling_entries_are_ignored() {
        let value = std::env::join_paths(["", "relative/dir", "/abs/ceiling/"]).unwrap();
        assert_eq!(
            parse_root_ceilings(&value),
            vec![PathBuf::from("/abs/ceiling")]
        );
    }

    #[test]
    fn this_unit_test_process_is_recognised_as_a_cargo_test_binary() {
        assert!(running_as_cargo_test_binary());
    }

    /// Child-process probe for the env-driven tests below: discovery reads the
    /// process environment, and mutating it in-process would race every other
    /// test's discovery, so each env variant runs in a fresh copy of this binary.
    #[test]
    #[ignore = "child-process probe; driven by the ceiling regression tests"]
    fn root_ceiling_child_probe() {
        let Some(start) = std::env::var_os("AGENT_DOC_ROOT_CEILING_PROBE_START") else {
            return;
        };
        let resolved = crate::find_project_root(Path::new(&start));
        println!(
            "PROBE_ROOT={}",
            resolved.map_or_else(|| "<none>".to_string(), |root| root.display().to_string())
        );
    }

    fn probe_root(start: &Path, ceiling: &OsString) -> String {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "root_ceiling::tests::root_ceiling_child_probe",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("AGENT_DOC_ROOT_CEILING_PROBE_START", start)
            .env(ROOT_CEILING_ENV, ceiling)
            .output()
            .expect("spawn ceiling probe");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success(),
            "probe failed: stdout={stdout} stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
        stdout
            .lines()
            .find_map(|line| line.split_once("PROBE_ROOT=").map(|(_, root)| root))
            .unwrap_or_else(|| panic!("probe printed no root: {stdout}"))
            .to_string()
    }

    /// `#testisolationtests` regression: a test TMPDIR under `$HOME` walked up to
    /// the operator's `~/.agent-doc` and wrote its state.db / ops.log / snapshots.
    /// Model it as an outer project holding `.agent-doc/` with a test temp root
    /// (and a test's tempdir) beneath it.
    fn outer_project_with_nested_test_tmpdir() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let outer = tempfile::tempdir().unwrap();
        let outer_root = outer.path().canonicalize().unwrap();
        std::fs::create_dir_all(outer_root.join(".agent-doc")).unwrap();
        let test_tmpdir = outer_root.join("cache/agent-tmp");
        let test_tempdir = test_tmpdir.join(".tmpAbC123");
        std::fs::create_dir_all(&test_tempdir).unwrap();
        std::fs::write(test_tempdir.join("session.md"), "# session\n").unwrap();
        (outer, test_tmpdir, test_tempdir)
    }

    #[test]
    fn a_test_tempdir_below_an_outer_agent_doc_does_not_adopt_it_under_a_ceiling() {
        let (outer, test_tmpdir, test_tempdir) = outer_project_with_nested_test_tmpdir();
        let outer_root = outer.path().canonicalize().unwrap();
        let doc = test_tempdir.join("session.md");

        // Pre-fix behaviour, reproduced with every ceiling disabled (an explicit
        // empty list also disables the implicit test-binary ceiling): the
        // tempdir adopts the outer project.
        assert_eq!(
            probe_root(&doc, &OsString::new()),
            outer_root.display().to_string(),
            "without a ceiling the walk must still reach the outer root, or this test proves nothing"
        );

        // With the ceiling at the test temp root, discovery stops below it.
        assert_eq!(
            probe_root(&doc, &test_tmpdir.clone().into_os_string()),
            "<none>",
            "a ceiling at the test TMPDIR must stop discovery from adopting the outer .agent-doc"
        );

        // A trailing separator and an extra, unrelated entry change nothing.
        let ceiling = std::env::join_paths([
            PathBuf::from("/nonexistent/ceiling"),
            PathBuf::from(format!("{}/", test_tmpdir.display())),
        ])
        .unwrap();
        assert_eq!(probe_root(&doc, &ceiling), "<none>");

        // A project inside the ceiling is still found.
        std::fs::create_dir_all(test_tempdir.join(".agent-doc")).unwrap();
        assert_eq!(
            probe_root(&doc, &test_tmpdir.into_os_string()),
            test_tempdir.display().to_string()
        );
    }

    #[test]
    fn the_test_tmpdir_guard_refuses_a_tmpdir_inside_an_agent_doc_project() {
        let (outer, test_tmpdir, _) = outer_project_with_nested_test_tmpdir();
        let outer_root = outer.path().canonicalize().unwrap();

        assert_eq!(agent_doc_ancestor(&test_tmpdir), Some(outer_root.clone()));
        let refusal = ensure_test_tmpdir_isolated(&test_tmpdir).unwrap_err();
        assert!(
            refusal
                .to_string()
                .contains(&outer_root.display().to_string()),
            "the refusal must name the project the TMPDIR would adopt: {refusal}"
        );

        // The Makefile / nextest guard script refuses the same TMPDIR ...
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts/check-test-tmpdir");
        let refused = std::process::Command::new(&script)
            .arg(&test_tmpdir)
            .env_remove(ROOT_CEILING_ENV)
            .output()
            .expect("run scripts/check-test-tmpdir");
        assert!(!refused.status.success(), "the guard script must refuse it");
        assert!(
            String::from_utf8_lossy(&refused.stderr).contains(".agent-doc"),
            "stderr: {}",
            String::from_utf8_lossy(&refused.stderr)
        );

        // ... and accepts a temp root with no agent-doc ancestor, printing the
        // ceiling list that bounds the tests it guards.
        let clean = tempfile::tempdir().unwrap();
        let clean_root = clean.path().canonicalize().unwrap();
        if agent_doc_ancestor(&clean_root).is_none() {
            let accepted = std::process::Command::new(&script)
                .arg(&clean_root)
                .env_remove(ROOT_CEILING_ENV)
                .output()
                .expect("run scripts/check-test-tmpdir");
            assert!(accepted.status.success());
            assert!(
                String::from_utf8_lossy(&accepted.stdout)
                    .starts_with(&format!("{}:", clean_root.display()))
            );
            assert!(ensure_test_tmpdir_isolated(&clean_root).is_ok());
        }
    }

    fn run_guard_script(cwd: &Path, args: &[&OsString], ceiling: Option<&OsString>) -> String {
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../scripts/check-test-tmpdir")
            .canonicalize()
            .unwrap();
        let mut command = std::process::Command::new(script);
        command.current_dir(cwd).args(args);
        match ceiling {
            Some(value) => command.env(ROOT_CEILING_ENV, value),
            None => command.env_remove(ROOT_CEILING_ENV),
        };
        let output = command.output().expect("run scripts/check-test-tmpdir");
        assert!(
            output.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .unwrap()
            .trim_end()
            .to_string()
    }

    /// `#checktmpdirleak`: every checkout's test ceiling names that checkout's
    /// own workspace -- whether the guard runs concurrently from sibling
    /// checkouts, or inherits a ceiling another checkout's `make` exported.
    /// Before the fix, the nextest setup script honored an inherited list
    /// verbatim: it printed the other checkout's path and left this workspace
    /// unbounded, so discovery from inside it adopted a project above it.
    #[test]
    fn the_test_tmpdir_guard_bounds_each_checkout_by_its_own_workspace() {
        let outer = tempfile::tempdir().unwrap();
        let outer_root = outer.path().canonicalize().unwrap();
        let test_tmp = tempfile::tempdir().unwrap();
        let test_tmp_root = test_tmp.path().canonicalize().unwrap();
        if agent_doc_ancestor(&outer_root).is_some() || agent_doc_ancestor(&test_tmp_root).is_some()
        {
            return; // the guard would (rightly) refuse; covered by the test above.
        }
        // A real project ABOVE both checkouts (as `$HOME/.agent-doc` is).
        std::fs::create_dir_all(outer_root.join(".agent-doc")).unwrap();
        let checkout_a = outer_root.join("checkout-a");
        let checkout_b = outer_root.join("checkout-b");
        let nested = checkout_b.join("crate/src");
        std::fs::create_dir_all(&checkout_a).unwrap();
        std::fs::create_dir_all(&nested).unwrap();
        let tmp_arg = test_tmp_root.clone().into_os_string();

        // Sibling checkouts running the guard concurrently each get their own.
        let runs: Vec<_> = (0..8)
            .map(|i| {
                let cwd = if i % 2 == 0 {
                    checkout_a.clone()
                } else {
                    checkout_b.clone()
                };
                let tmp_arg = tmp_arg.clone();
                std::thread::spawn(move || (run_guard_script(&cwd, &[&tmp_arg], None), cwd))
            })
            .collect();
        for run in runs {
            let (ceiling, cwd) = run.join().unwrap();
            assert_eq!(
                ceiling,
                format!("{}:{}", test_tmp_root.display(), cwd.display())
            );
        }

        // Checkout B inherits the ceiling checkout A's `make` exported.
        let foreign: OsString =
            format!("{}:{}", test_tmp_root.display(), checkout_a.display()).into();
        let nextest_env = test_tmp_root.join("nextest-env");
        std::fs::write(&nextest_env, "").unwrap();
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../scripts/check-test-tmpdir")
            .canonicalize()
            .unwrap();
        let output = std::process::Command::new(script)
            .current_dir(&checkout_b)
            .arg("--nextest-env")
            .env("TMPDIR", &test_tmp_root)
            .env("NEXTEST_ENV", &nextest_env)
            .env(ROOT_CEILING_ENV, &foreign)
            .output()
            .expect("run scripts/check-test-tmpdir --nextest-env");
        assert!(output.status.success());
        let printed = String::from_utf8(output.stdout).unwrap();
        let exported = std::fs::read_to_string(&nextest_env).unwrap();
        let exported = exported
            .trim_end()
            .strip_prefix(&format!("{ROOT_CEILING_ENV}="))
            .unwrap_or_else(|| panic!("NEXTEST_ENV: {exported:?}"))
            .to_string();
        assert_eq!(printed.trim_end(), exported);
        let entries: Vec<&str> = exported.split(':').collect();
        assert!(
            entries.contains(&checkout_b.to_str().unwrap()),
            "checkout B's own workspace must bound its tests: {exported}"
        );
        assert!(
            entries.contains(&test_tmp_root.to_str().unwrap()),
            "{exported}"
        );

        // The bound is what keeps discovery from inside B off the project above it.
        assert_eq!(
            probe_root(&nested, &foreign),
            outer_root.display().to_string()
        );
        assert_eq!(probe_root(&nested, &exported.into()), "<none>");
    }
}
