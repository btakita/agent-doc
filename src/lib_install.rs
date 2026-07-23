use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static BINARY_INSTALL_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(crate) fn platform_lib_name() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        "libagent_doc.so"
    }
    #[cfg(target_os = "macos")]
    {
        "libagent_doc.dylib"
    }
    #[cfg(target_os = "windows")]
    {
        "agent_doc.dll"
    }
}

fn platform_lib_ext() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        "so"
    }
    #[cfg(target_os = "macos")]
    {
        "dylib"
    }
    #[cfg(target_os = "windows")]
    {
        "dll"
    }
}

pub fn versioned_lib_name(version: &str) -> String {
    #[cfg(target_os = "linux")]
    {
        format!("libagent_doc-{}.so", version)
    }
    #[cfg(target_os = "macos")]
    {
        format!("libagent_doc-{}.dylib", version)
    }
    #[cfg(target_os = "windows")]
    {
        format!("agent_doc-{}.dll", version)
    }
}

pub fn install_versioned(source: &Path, target_dir: &Path, version: &str) -> Result<PathBuf> {
    let versioned = versioned_lib_name(version);
    let dst = target_dir.join(&versioned);
    let symlink = target_dir.join(platform_lib_name());
    let tmp_symlink = target_dir.join(format!("{}.tmp", platform_lib_name()));

    // Atomic file replace: copy to temp, then rename. This creates a new inode
    // so any existing mmap (e.g., IDEA's FFI handle) remains valid on the old inode.
    let tmp_dst = target_dir.join(format!(".{}.tmp", versioned));
    std::fs::copy(source, &tmp_dst)
        .with_context(|| format!("copy {} -> {}", source.display(), tmp_dst.display()))?;
    std::fs::rename(&tmp_dst, &dst)
        .with_context(|| format!("rename {} -> {}", tmp_dst.display(), dst.display()))?;

    // Atomic symlink swap: create temp symlink, then rename over the real one.
    // Use relative target so the symlink works if the directory is moved.
    if tmp_symlink.exists() {
        std::fs::remove_file(&tmp_symlink)?;
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(&versioned, &tmp_symlink)
        .with_context(|| format!("symlink {} -> {}", tmp_symlink.display(), versioned))?;
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(&versioned, &tmp_symlink)
        .with_context(|| format!("symlink {} -> {}", tmp_symlink.display(), versioned))?;

    std::fs::rename(&tmp_symlink, &symlink)
        .with_context(|| format!("rename {} -> {}", tmp_symlink.display(), symlink.display()))?;

    Ok(dst)
}

/// Resolve Cargo's binary install directory without consulting the currently
/// executing binary. The caller may be a just-built staging executable, so
/// `current_exe().parent()` would point at `target/`, not the live install.
pub fn default_binary_target_dir() -> Result<PathBuf> {
    if let Some(root) = std::env::var_os("CARGO_INSTALL_ROOT") {
        return Ok(PathBuf::from(root).join("bin"));
    }
    if let Some(home) = std::env::var_os("CARGO_HOME") {
        return Ok(PathBuf::from(home).join("bin"));
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .context("cannot resolve Cargo binary directory: HOME/USERPROFILE is unset")?;
    Ok(PathBuf::from(home).join(".cargo").join("bin"))
}

fn platform_binary_name() -> &'static str {
    #[cfg(windows)]
    {
        "agent-doc.exe"
    }
    #[cfg(not(windows))]
    {
        "agent-doc"
    }
}

/// Install an already-built agent-doc executable with a same-directory atomic
/// rename. Existing supervisors keep their old inode alive while new exec/spawn
/// calls see either the complete old binary or the complete new binary; there is
/// never a missing-path handoff window.
pub fn install_binary_atomic(source: &Path, target_dir: &Path) -> Result<PathBuf> {
    let metadata = std::fs::metadata(source)
        .with_context(|| format!("read built binary metadata {}", source.display()))?;
    if !metadata.is_file() {
        anyhow::bail!("built binary source is not a file: {}", source.display());
    }

    std::fs::create_dir_all(target_dir)
        .with_context(|| format!("create binary install directory {}", target_dir.display()))?;
    let destination = target_dir.join(platform_binary_name());
    let sequence = BINARY_INSTALL_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = target_dir.join(format!(
        ".{}.install-{}-{sequence}",
        platform_binary_name(),
        std::process::id()
    ));

    let install_result = (|| -> Result<()> {
        std::fs::copy(source, &temporary).with_context(|| {
            format!(
                "copy staged binary {} -> {}",
                source.display(),
                temporary.display()
            )
        })?;
        std::fs::set_permissions(&temporary, metadata.permissions()).with_context(|| {
            format!("preserve executable permissions on {}", temporary.display())
        })?;
        std::fs::File::open(&temporary)
            .with_context(|| format!("open staged binary {} for sync", temporary.display()))?
            .sync_all()
            .with_context(|| format!("sync staged binary {}", temporary.display()))?;
        std::fs::rename(&temporary, &destination).with_context(|| {
            format!(
                "atomically replace installed binary {} -> {}",
                temporary.display(),
                destination.display()
            )
        })?;
        Ok(())
    })();

    if install_result.is_err() && temporary.exists() {
        let _ = std::fs::remove_file(&temporary);
    }
    install_result?;
    eprintln!(
        "[binary-install] {} -> {} (atomic rename; live process inodes preserved)",
        source.display(),
        destination.display()
    );
    Ok(destination)
}

pub fn run(source: Option<&str>, target_dir: Option<&str>, profile: &str) -> Result<()> {
    let source_path = source.map(PathBuf::from);
    let target_dir = target_dir.map(PathBuf::from);
    run_paths(source_path.as_deref(), target_dir.as_deref(), profile)
}

pub(crate) fn run_paths(
    source: Option<&Path>,
    target_dir: Option<&Path>,
    profile: &str,
) -> Result<()> {
    let version = env!("CARGO_PKG_VERSION");
    let ext = platform_lib_ext();
    let profile = normalized_profile(profile);

    let source_path = match source {
        Some(s) => s.to_path_buf(),
        None => {
            let cwd = std::env::current_dir()?;
            let cargo_target_dir = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from);
            profile_lib_path(&cwd, cargo_target_dir.as_deref(), profile)
        }
    };

    if !source_path.exists() {
        anyhow::bail!(
            "[lib-install] source not found: {}\nBuild with: cargo build --profile {} --lib",
            source_path.display(),
            profile
        );
    }
    validate_required_editor_abi_symbols(&source_path)?;

    let target = match target_dir {
        Some(d) => d.to_path_buf(),
        None => {
            let exe = std::env::current_exe()?;
            exe.parent()
                .context("cannot determine binary directory")?
                .to_path_buf()
        }
    };

    let installed = install_versioned(&source_path, &target, version)?;
    eprintln!(
        "[lib-install] {} -> {} (symlink: libagent_doc.{})",
        source_path.display(),
        installed.display(),
        ext,
    );
    // #autorecycle-on-install (upgrades #ctlrecycle R4 from print-only to action):
    // the JetBrains plugin hot-reloads this cdylib by mtime, but already-running
    // agent-doc controllers/supervisors keep serving the PRIOR binary until they
    // recycle. Instead of only printing the recycle hint, automatically mark every
    // running controller and route-owned supervisor to recycle at its next idle
    // boundary so the new build goes live everywhere, not just in the editor cdylib.
    // The controller recycle path is idle-gated
    // (`recycle_controllers_all_projects` sends a `recycle` RPC that fires only at a
    // turn/inter-queue-item boundary, never mid-turn), so triggering it from
    // `lib-install` is safe. Opt out with a falsey AGENT_DOC_RECYCLE_ON_INSTALL.
    auto_recycle_after_install();

    // Proactively send the shared `reload_library` intent to editor adapters that
    // explicitly support safe hot reload. JetBrains uses a quiesce/drain/close
    // generation handoff; unknown adapters still require a process restart.
    signal_reload_after_install(version);

    Ok(())
}

fn signal_reload_after_install(version: &str) {
    let report = agent_doc_controller_io::project_controller::reload_library_all_projects(version);
    eprintln!(
        "[lib-install] reload_library intent: delivered {}/{} editor endpoints across {} projects ({} restart required, {} unavailable)",
        report.delivered, report.endpoints, report.projects, report.restart_required, report.failed
    );
}

/// Deterministic report from [`reload_lib`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReloadLibReport {
    /// The cdylib version announced in the typed intent.
    pub lib_version: String,
    pub editor_projects: usize,
    pub editor_endpoints: usize,
    pub delivered: usize,
    pub restart_required: usize,
    pub failed: usize,
}

/// Send a typed `reload_library` intent to safe hot-reload editor members.
pub fn reload_lib() -> Result<ReloadLibReport> {
    let version = env!("CARGO_PKG_VERSION");
    let fanout = agent_doc_controller_io::project_controller::reload_library_all_projects(version);
    Ok(ReloadLibReport {
        lib_version: version.to_string(),
        editor_projects: fanout.projects,
        editor_endpoints: fanout.endpoints,
        delivered: fanout.delivered,
        restart_required: fanout.restart_required,
        failed: fanout.failed,
    })
}

fn normalized_profile(profile: &str) -> &str {
    match profile.trim() {
        "" => "release",
        trimmed => trimmed,
    }
}

pub(crate) fn profile_lib_path(
    cwd: &Path,
    cargo_target_dir: Option<&Path>,
    profile: &str,
) -> PathBuf {
    let target_root = cargo_target_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(|| cwd.join("target"));
    target_root
        .join(normalized_profile(profile))
        .join(platform_lib_name())
}

const REQUIRED_EDITOR_ABI_SYMBOLS: &[&str] = &[
    "agent_doc_lazily_current_observed_v1",
    "agent_doc_editor_content_applied_for_editor_v1",
    "agent_doc_editor_patch_applied",
    "agent_doc_editor_patch_rejected",
    "agent_doc_document_closed_for_editor",
];

fn validate_required_editor_abi_symbols(source: &Path) -> Result<()> {
    let bytes = std::fs::read(source)
        .with_context(|| format!("read shared library {}", source.display()))?;
    let missing: Vec<&str> = REQUIRED_EDITOR_ABI_SYMBOLS
        .iter()
        .copied()
        .filter(|symbol| !contains_ascii_symbol(&bytes, symbol.as_bytes()))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    anyhow::bail!(
        "[lib-install] source {} is missing required editor authority ABI symbol(s): {}. Rebuild the current checkout with `cargo build --lib` before installing.",
        source.display(),
        missing.join(", ")
    );
}

fn contains_ascii_symbol(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

/// `#autorecycle-on-install`: default-on resolution for auto-recycling running
/// controllers after a `lib-install`. Falsey `AGENT_DOC_RECYCLE_ON_INSTALL`
/// (`0`/`false`/`no`/`off`) opts out and restores the print-only hint.
fn recycle_on_install_enabled() -> bool {
    match std::env::var("AGENT_DOC_RECYCLE_ON_INSTALL") {
        Ok(v) => !matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "no" | "off"
        ),
        Err(_) => true,
    }
}

/// Mark every running controller and route-owned supervisor to recycle onto the
/// freshly-installed binary at its next idle boundary. Best-effort: a recycle
/// failure must never fail the install, so errors are logged (never swallowed
/// silently) and the print-only hint is surfaced as a fallback. When opted out,
/// only the hint is printed.
fn auto_recycle_after_install() {
    if !recycle_on_install_enabled() {
        eprintln!(
            "[lib-install] note: auto-recycle opted out (AGENT_DOC_RECYCLE_ON_INSTALL falsey) — running controllers still serve the prior binary; run `agent-doc admin recycle --all-projects` (or restart sessions) to promote the new build"
        );
        return;
    }
    // `#installhandoff`: mark the document-owning supervisors first. Their
    // durable per-document recycle requests are the recovery journal if a
    // controller happens to cross its own exec boundary immediately afterward.
    match agent_doc_controller_io::project_controller::recycle_supervisors_all_projects() {
        Ok((marked, skipped)) => {
            eprintln!(
                "[lib-install] auto-recycle: {marked} route-owned supervisor(s) marked to recycle at next idle boundary, {skipped} skipped"
            );
        }
        Err(e) => {
            eprintln!(
                "[lib-install] warning: supervisor recycle fan-out failed ({e}) — route-owned supervisors still serve the prior binary until they self-detect staleness"
            );
        }
    }
    match agent_doc_controller_io::project_controller::recycle_controllers_all_projects() {
        Ok((recycled, skipped)) => {
            eprintln!(
                "[lib-install] auto-recycle: {recycled} controller(s) marked after supervisor handoff, {skipped} skipped (set AGENT_DOC_RECYCLE_ON_INSTALL=0 to disable)"
            );
        }
        Err(e) => {
            eprintln!(
                "[lib-install] warning: auto-recycle failed ({e}) — running controllers still serve the prior binary; run `agent-doc admin recycle --all-projects` (or restart sessions) to promote the new build"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn binary_install_atomically_replaces_existing_executable() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("new-agent-doc");
        let target_dir = tmp.path().join("bin");
        fs::create_dir_all(&target_dir).unwrap();
        let destination = target_dir.join(platform_binary_name());
        fs::write(&destination, b"old complete binary").unwrap();
        fs::write(&source, b"new complete binary").unwrap();

        let installed = install_binary_atomic(&source, &target_dir).unwrap();

        assert_eq!(installed, destination);
        assert_eq!(fs::read(&installed).unwrap(), b"new complete binary");
        assert!(
            fs::read_dir(&target_dir).unwrap().all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".install-")),
            "successful swap must not leave staging files"
        );
    }

    #[test]
    fn binary_install_failure_preserves_existing_executable() {
        let tmp = tempfile::tempdir().unwrap();
        let target_dir = tmp.path().join("bin");
        fs::create_dir_all(&target_dir).unwrap();
        let destination = target_dir.join(platform_binary_name());
        fs::write(&destination, b"old complete binary").unwrap();

        let error = install_binary_atomic(&tmp.path().join("missing"), &target_dir).unwrap_err();

        assert!(error.to_string().contains("read built binary metadata"));
        assert_eq!(fs::read(destination).unwrap(), b"old complete binary");
    }

    #[test]
    fn recycle_on_install_default_on_and_opt_out_is_falsey() {
        // #autorecycle-on-install: default-on, falsey env opts out. Serialize env
        // mutation to avoid cross-test interference.
        let prior = std::env::var("AGENT_DOC_RECYCLE_ON_INSTALL").ok();
        unsafe { std::env::remove_var("AGENT_DOC_RECYCLE_ON_INSTALL") };
        assert!(recycle_on_install_enabled(), "default must be ON");
        for falsey in ["0", "false", "no", "off", "OFF", " False "] {
            unsafe { std::env::set_var("AGENT_DOC_RECYCLE_ON_INSTALL", falsey) };
            assert!(
                !recycle_on_install_enabled(),
                "falsey value {falsey:?} must opt out"
            );
        }
        for truthy in ["1", "true", "yes", "on", "anything"] {
            unsafe { std::env::set_var("AGENT_DOC_RECYCLE_ON_INSTALL", truthy) };
            assert!(
                recycle_on_install_enabled(),
                "truthy/other value {truthy:?} stays ON"
            );
        }
        unsafe {
            match prior {
                Some(v) => std::env::set_var("AGENT_DOC_RECYCLE_ON_INSTALL", v),
                None => std::env::remove_var("AGENT_DOC_RECYCLE_ON_INSTALL"),
            }
        }
    }

    #[test]
    fn versioned_name_includes_version() {
        let name = versioned_lib_name("0.33.4");
        assert!(name.contains("0.33.4"));
        assert!(name.starts_with("libagent_doc-") || name.starts_with("agent_doc-"));
    }

    #[test]
    fn install_creates_versioned_file_and_symlink() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join(platform_lib_name());
        fs::write(&source, b"fake library content").unwrap();

        let target_dir = tmp.path().join("install");
        fs::create_dir(&target_dir).unwrap();

        let installed = install_versioned(&source, &target_dir, "1.2.3").unwrap();

        // Versioned file exists with correct content
        assert!(installed.exists());
        assert_eq!(fs::read(&installed).unwrap(), b"fake library content");
        assert!(
            installed
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .contains("1.2.3")
        );

        // Symlink exists and resolves to the versioned file
        let symlink = target_dir.join(platform_lib_name());
        assert!(symlink.exists());
        assert!(symlink.is_symlink());
        let target = fs::read_link(&symlink).unwrap();
        assert_eq!(target.to_str().unwrap(), versioned_lib_name("1.2.3"));
    }

    #[test]
    fn lib_install_rejects_source_missing_editor_authority_abi() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join(platform_lib_name());
        fs::write(&source, b"fake library content").unwrap();

        let err = validate_required_editor_abi_symbols(&source).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("missing required editor authority ABI"));
        assert!(message.contains("agent_doc_lazily_current_observed_v1"));
    }

    #[test]
    fn lib_install_accepts_source_with_editor_authority_abi() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join(platform_lib_name());
        fs::write(&source, REQUIRED_EDITOR_ABI_SYMBOLS.to_vec().join("\n")).unwrap();

        validate_required_editor_abi_symbols(&source).unwrap();
    }

    #[test]
    fn profile_lib_path_uses_profile_and_cargo_target_dir() {
        let cwd = PathBuf::from("/tmp/agent-doc");

        assert_eq!(
            profile_lib_path(&cwd, None, "release-local"),
            cwd.join("target")
                .join("release-local")
                .join(platform_lib_name())
        );
        assert_eq!(
            profile_lib_path(
                &cwd,
                Some(Path::new("target/local-install")),
                "release-local"
            ),
            PathBuf::from("target/local-install")
                .join("release-local")
                .join(platform_lib_name())
        );
        assert_eq!(
            profile_lib_path(&cwd, None, " "),
            cwd.join("target").join("release").join(platform_lib_name())
        );
    }

    #[test]
    fn install_swaps_symlink_on_version_change() {
        let tmp = tempfile::tempdir().unwrap();
        let target_dir = tmp.path().join("install");
        fs::create_dir(&target_dir).unwrap();

        // First version
        let source_v1 = tmp.path().join("v1.so");
        fs::write(&source_v1, b"v1 content").unwrap();
        install_versioned(&source_v1, &target_dir, "1.0.0").unwrap();

        let symlink = target_dir.join(platform_lib_name());
        assert_eq!(
            fs::read_link(&symlink).unwrap().to_str().unwrap(),
            versioned_lib_name("1.0.0")
        );

        // Second version
        let source_v2 = tmp.path().join("v2.so");
        fs::write(&source_v2, b"v2 content").unwrap();
        install_versioned(&source_v2, &target_dir, "2.0.0").unwrap();

        // Symlink now points to v2
        assert_eq!(
            fs::read_link(&symlink).unwrap().to_str().unwrap(),
            versioned_lib_name("2.0.0")
        );

        // v1 versioned file still exists (for GC later)
        assert!(target_dir.join(versioned_lib_name("1.0.0")).exists());

        // Symlink resolves to v2 content
        assert_eq!(fs::read(&symlink).unwrap(), b"v2 content");
    }

    #[test]
    fn same_version_reinstall_creates_new_inode() {
        use std::os::unix::fs::MetadataExt;

        let tmp = tempfile::tempdir().unwrap();
        let target_dir = tmp.path().join("install");
        fs::create_dir(&target_dir).unwrap();

        let source_v1 = tmp.path().join("v1.so");
        fs::write(&source_v1, b"original content").unwrap();
        install_versioned(&source_v1, &target_dir, "1.0.0").unwrap();

        let versioned_path = target_dir.join(versioned_lib_name("1.0.0"));
        let ino_before = fs::metadata(&versioned_path).unwrap().ino();

        // Reinstall same version with different content
        let source_v1b = tmp.path().join("v1b.so");
        fs::write(&source_v1b, b"updated content").unwrap();
        install_versioned(&source_v1b, &target_dir, "1.0.0").unwrap();

        let ino_after = fs::metadata(&versioned_path).unwrap().ino();

        // Atomic rename must produce a new inode — old mmap stays valid on old inode
        assert_ne!(
            ino_before, ino_after,
            "same-version reinstall must create new inode"
        );
        assert_eq!(fs::read(&versioned_path).unwrap(), b"updated content");
    }

    #[test]
    fn install_replaces_regular_file_with_symlink() {
        let tmp = tempfile::tempdir().unwrap();
        let target_dir = tmp.path().join("install");
        fs::create_dir(&target_dir).unwrap();

        // Pre-existing unversioned file (legacy install)
        let legacy = target_dir.join(platform_lib_name());
        fs::write(&legacy, b"old unversioned").unwrap();
        assert!(!legacy.is_symlink());

        let source = tmp.path().join("new.so");
        fs::write(&source, b"versioned content").unwrap();
        install_versioned(&source, &target_dir, "3.0.0").unwrap();

        // Now it's a symlink
        assert!(legacy.is_symlink());
        assert_eq!(fs::read(&legacy).unwrap(), b"versioned content");
    }
}
