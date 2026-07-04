use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Well-known filename of the global cdylib reload-broadcast file. It lives as a
/// sibling of the installed `libagent_doc` cdylib (in the running binary's
/// directory), so it is a single project-independent path that every editor
/// plugin can derive from the library path it already resolves via
/// `agent-doc lib-path`.
pub const RELOAD_BROADCAST_FILENAME: &str = "agent-doc-reload-broadcast.json";

/// Payload of the global cdylib reload-broadcast file. An install (or
/// `agent-doc admin reload-lib`) writes this so editor plugins force the existing
/// native-reload path immediately instead of waiting for their next lazy
/// mtime-checked FFI call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReloadBroadcast {
    /// The `CARGO_PKG_VERSION` of the freshly-installed cdylib.
    pub lib_version: String,
    /// The installed cdylib's mtime, in epoch milliseconds (0 if unavailable).
    pub mtime_ms: i64,
    /// Caller-supplied wall-clock stamp (epoch milliseconds) of when the
    /// broadcast was written. Supplied by the call site so pure writers never
    /// read the clock.
    pub installed_at_epoch_ms: u128,
}

/// The global reload-broadcast file path: a sibling of the installed cdylib in
/// the running binary's directory. Project-independent.
pub fn reload_broadcast_path() -> Result<PathBuf> {
    let exe = std::env::current_exe().context("cannot determine current executable path")?;
    let dir = exe
        .parent()
        .context("cannot determine binary directory for reload broadcast")?;
    Ok(reload_broadcast_path_in(dir))
}

/// Pure path helper: the reload-broadcast file inside `dir`.
pub fn reload_broadcast_path_in(dir: &Path) -> PathBuf {
    dir.join(RELOAD_BROADCAST_FILENAME)
}

/// Pure writer: serialize `broadcast` to `dir/agent-doc-reload-broadcast.json`
/// atomically (temp + rename). Does not read the clock — the caller stamps
/// `installed_at_epoch_ms`. Returns the written path.
pub fn write_reload_broadcast(dir: &Path, broadcast: &ReloadBroadcast) -> Result<PathBuf> {
    let path = reload_broadcast_path_in(dir);
    let json =
        serde_json::to_vec_pretty(broadcast).context("serialize reload broadcast payload")?;
    agent_doc_fs::write_atomic(&path, &json)
        .with_context(|| format!("write reload broadcast {}", path.display()))?;
    Ok(path)
}

/// Reader counterpart to [`write_reload_broadcast`]. Part of the reload-broadcast
/// contract; currently exercised by the round-trip unit tests and available to
/// external callers that parse the broadcast payload.
#[cfg_attr(not(test), allow(dead_code))]
pub fn read_reload_broadcast(path: &Path) -> Result<ReloadBroadcast> {
    let bytes =
        std::fs::read(path).with_context(|| format!("read reload broadcast {}", path.display()))?;
    serde_json::from_slice(&bytes)
        .with_context(|| format!("parse reload broadcast {}", path.display()))
}

/// Epoch-milliseconds mtime of `path`, if resolvable.
fn mtime_epoch_ms(path: &Path) -> Option<i64> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let dur = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(dur.as_millis() as i64)
}

/// Current wall-clock stamp in epoch milliseconds (0 if the clock is before the
/// epoch, which should never happen in practice).
fn now_epoch_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

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
    // running controller to recycle at its next idle boundary so the new build goes
    // live everywhere, not just in the editor cdylib. The recycle is idle-gated
    // (`recycle_controllers_all_projects` sends a `recycle` RPC that fires only at a
    // turn/inter-queue-item boundary, never mid-turn), so triggering it from
    // `lib-install` is safe. Opt out with a falsey AGENT_DOC_RECYCLE_ON_INSTALL.
    auto_recycle_after_install();

    // `#cdylib-reload-broadcast`: running controllers/supervisors recycle onto the
    // fresh binary above, but editor plugins only hot-reload the cdylib LAZILY on
    // their next FFI call. Proactively announce the reload by writing the global
    // broadcast file next to the installed cdylib; the JetBrains/VS Code plugins
    // watch it and force their existing native-reload path immediately. Best-effort:
    // a broadcast failure is logged (never swallowed) but must NOT fail the install.
    broadcast_reload_after_install(&target, version, &installed);

    Ok(())
}

/// `#cdylib-reload-broadcast`: write the global reload-broadcast file after a
/// successful cdylib install. Best-effort — logs on failure, never returns an
/// error to the install path.
fn broadcast_reload_after_install(target_dir: &Path, version: &str, installed: &Path) {
    let broadcast = ReloadBroadcast {
        lib_version: version.to_string(),
        mtime_ms: mtime_epoch_ms(installed).unwrap_or(0),
        installed_at_epoch_ms: now_epoch_ms(),
    };
    match write_reload_broadcast(target_dir, &broadcast) {
        Ok(path) => eprintln!(
            "[lib-install] broadcast: cdylib reload announced to editor plugins ({})",
            path.display()
        ),
        Err(e) => eprintln!(
            "[lib-install] warning: cdylib reload broadcast failed ({e:#}) — editor plugins will still lazily reload on their next FFI call"
        ),
    }
}

/// Deterministic report from [`reload_lib`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReloadLibReport {
    /// Path of the reload-broadcast file that was written.
    pub broadcast_path: PathBuf,
    /// The cdylib version announced in the broadcast.
    pub lib_version: String,
    /// Best-effort count of discoverable editor-connected project roots the
    /// broadcast could ALSO signal directly (controller-serving roots that carry
    /// an `.agent-doc/patches/` directory). The global broadcast remains the
    /// authoritative fan-out because IDE-hosted listeners are not enumerable.
    pub editor_projects: usize,
}

/// `agent-doc admin reload-lib` core: write the global reload-broadcast file for
/// the currently-installed cdylib (the "recycle via API" entry point) and report
/// how many editor-connected projects it could also signal. `now_epoch_ms` is
/// supplied by the caller so this stays testable and clock-free at the seam.
pub fn reload_lib(now_epoch_ms: u128) -> Result<ReloadLibReport> {
    let version = env!("CARGO_PKG_VERSION");
    let broadcast_target = reload_broadcast_path()?;
    let dir = broadcast_target
        .parent()
        .context("cannot determine binary directory for reload broadcast")?;
    let installed = dir.join(platform_lib_name());
    let broadcast = ReloadBroadcast {
        lib_version: version.to_string(),
        mtime_ms: mtime_epoch_ms(&installed).unwrap_or(0),
        installed_at_epoch_ms: now_epoch_ms,
    };
    let broadcast_path = write_reload_broadcast(dir, &broadcast)?;
    let editor_projects =
        agent_doc_controller_io::project_controller::editor_broadcast_project_root_count();
    Ok(ReloadLibReport {
        broadcast_path,
        lib_version: version.to_string(),
        editor_projects,
    })
}

/// Wall-clock convenience wrapper over [`reload_lib`] for the CLI call site.
pub fn reload_lib_now() -> Result<ReloadLibReport> {
    reload_lib(now_epoch_ms())
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
    "agent_doc_document_changed_digest_content_for_editor_v2",
    "agent_doc_document_changed_digest_content_for_editor_v3",
    "agent_doc_document_synced_digest_content_for_editor_v2",
    "agent_doc_write_ack_content_for_editor_v2",
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

/// Mark every running controller to recycle onto the freshly-installed binary at its
/// next idle boundary. Best-effort: a recycle failure must never fail the install, so
/// errors are logged (never swallowed silently) and the print-only hint is surfaced as
/// a fallback. When opted out, only the hint is printed.
fn auto_recycle_after_install() {
    if !recycle_on_install_enabled() {
        eprintln!(
            "[lib-install] note: auto-recycle opted out (AGENT_DOC_RECYCLE_ON_INSTALL falsey) — running controllers still serve the prior binary; run `agent-doc admin recycle --all-projects` (or restart sessions) to promote the new build"
        );
        return;
    }
    match agent_doc_controller_io::project_controller::recycle_controllers_all_projects() {
        Ok((recycled, skipped)) => {
            eprintln!(
                "[lib-install] auto-recycle: {recycled} controller(s) marked to recycle at next idle boundary, {skipped} skipped (set AGENT_DOC_RECYCLE_ON_INSTALL=0 to disable)"
            );
        }
        Err(e) => {
            eprintln!(
                "[lib-install] warning: auto-recycle failed ({e}) — running controllers still serve the prior binary; run `agent-doc admin recycle --all-projects` (or restart sessions) to promote the new build"
            );
        }
    }
    // `#turnsaferecycle` Goal 1 — controllers (PCPs) are only half the fleet. The
    // long-lived `agent-doc start --route-owned` supervisors that actually write
    // documents must also be marked to recycle onto the freshly-installed binary,
    // otherwise they keep serving the prior build until each independently
    // self-detects staleness.
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn reload_broadcast_round_trips_through_writer_and_reader() {
        let tmp = tempfile::tempdir().unwrap();
        let broadcast = ReloadBroadcast {
            lib_version: "0.34.68".to_string(),
            mtime_ms: 1_700_000_000_123,
            installed_at_epoch_ms: 1_700_000_000_456,
        };

        let written = write_reload_broadcast(tmp.path(), &broadcast).unwrap();
        assert_eq!(written, reload_broadcast_path_in(tmp.path()));
        assert_eq!(
            written.file_name().unwrap().to_str().unwrap(),
            RELOAD_BROADCAST_FILENAME
        );

        let read_back = read_reload_broadcast(&written).unwrap();
        assert_eq!(read_back, broadcast);
        assert_eq!(read_back.lib_version, "0.34.68");
        assert_eq!(read_back.mtime_ms, 1_700_000_000_123);
        assert_eq!(read_back.installed_at_epoch_ms, 1_700_000_000_456);
    }

    #[test]
    fn reload_broadcast_write_overwrites_prior_payload() {
        let tmp = tempfile::tempdir().unwrap();
        let first = ReloadBroadcast {
            lib_version: "0.34.67".to_string(),
            mtime_ms: 1,
            installed_at_epoch_ms: 2,
        };
        let second = ReloadBroadcast {
            lib_version: "0.34.68".to_string(),
            mtime_ms: 3,
            installed_at_epoch_ms: 4,
        };
        let path = write_reload_broadcast(tmp.path(), &first).unwrap();
        write_reload_broadcast(tmp.path(), &second).unwrap();
        assert_eq!(read_reload_broadcast(&path).unwrap(), second);
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
        assert!(message.contains("agent_doc_document_changed_digest_content_for_editor_v2"));
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
