use anyhow::{Context, Result};
use fs2::FileExt;
use parking_lot::Mutex;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Cloneable append handle for a long-lived log.
///
/// Clones share one descriptor instead of calling `File::try_clone`. When a
/// completed-cycle cleanup rotates the path, the next append notices the inode
/// change and reopens the active segment before writing.
#[derive(Clone)]
pub struct SharedAppendLog {
    inner: Arc<Mutex<SharedAppendLogInner>>,
}

struct SharedAppendLogInner {
    path: PathBuf,
    file: std::fs::File,
    rotation_lock: std::fs::File,
}

impl SharedAppendLog {
    pub fn open(path: impl Into<PathBuf>) -> std::io::Result<Self> {
        let path = path.into();
        let file = open_append_file(&path)?;
        let rotation_lock = open_rotation_lock(&path)?;
        Ok(Self {
            inner: Arc::new(Mutex::new(SharedAppendLogInner {
                path,
                file,
                rotation_lock,
            })),
        })
    }

    /// Compatibility with the old `File` call sites without allocating a new
    /// descriptor: the cloned handle shares the same synchronized writer.
    pub fn try_clone(&self) -> std::io::Result<Self> {
        Ok(self.clone())
    }
}

impl Write for SharedAppendLog {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut inner = self.inner.lock();
        FileExt::lock_shared(&inner.rotation_lock)?;
        let result = inner
            .reopen_after_rotation()
            .and_then(|()| inner.file.write(buf));
        let unlock = FileExt::unlock(&inner.rotation_lock);
        match (result, unlock) {
            (Err(err), _) | (Ok(_), Err(err)) => Err(err),
            (Ok(written), Ok(())) => Ok(written),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let mut inner = self.inner.lock();
        FileExt::lock_shared(&inner.rotation_lock)?;
        let result = inner.file.flush();
        let unlock = FileExt::unlock(&inner.rotation_lock);
        match (result, unlock) {
            (Err(err), _) | (Ok(()), Err(err)) => Err(err),
            (Ok(()), Ok(())) => Ok(()),
        }
    }
}

impl SharedAppendLogInner {
    fn reopen_after_rotation(&mut self) -> std::io::Result<()> {
        let active = match std::fs::metadata(&self.path) {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                self.file = open_append_file(&self.path)?;
                return Ok(());
            }
            Err(err) => return Err(err),
        };
        let opened = self.file.metadata()?;
        if !same_file(&opened, &active) {
            self.file = open_append_file(&self.path)?;
        }
        Ok(())
    }
}

#[cfg(unix)]
fn same_file(left: &std::fs::Metadata, right: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    left.dev() == right.dev() && left.ino() == right.ino()
}

#[cfg(not(unix))]
fn same_file(_left: &std::fs::Metadata, _right: &std::fs::Metadata) -> bool {
    // Reopen on every write on platforms without a stable std inode identity.
    false
}

fn open_append_file(path: &Path) -> std::io::Result<std::fs::File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
}

fn open_rotation_lock(log_path: &Path) -> std::io::Result<std::fs::File> {
    let logs_dir = log_path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(logs_dir)?;
    std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(logs_dir.join(".rotation.lock"))
}

fn segment_path(log_path: &Path, suffix: &str) -> Result<PathBuf> {
    let name = log_path
        .file_name()
        .and_then(|name| name.to_str())
        .with_context(|| format!("non-UTF-8 log file name: {}", log_path.display()))?;
    Ok(log_path.with_file_name(format!("{name}{suffix}")))
}

fn rotate_log_if_oversized_locked(log_path: &Path, max_bytes: u64) -> Result<bool> {
    let len = match std::fs::metadata(log_path) {
        Ok(meta) => meta.len(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err).with_context(|| format!("stat {}", log_path.display())),
    };
    if len < max_bytes {
        return Ok(false);
    }

    let raw = segment_path(log_path, ".1.raw")?;
    let compressed = segment_path(log_path, ".1.zst")?;
    let next_compressed = segment_path(log_path, ".1.zst.next")?;
    if raw.exists() {
        std::fs::remove_file(&raw)
            .with_context(|| format!("remove stale rotation staging file {}", raw.display()))?;
    }
    std::fs::rename(log_path, &raw).with_context(|| {
        format!(
            "atomically rotate active log {} to {}",
            log_path.display(),
            raw.display()
        )
    })?;
    // Publish a new active inode before compression. Long-lived shared writers
    // observe it on their next append and release the retired descriptor.
    open_append_file(log_path)
        .with_context(|| format!("create rotated active log {}", log_path.display()))?;

    let mut input = std::io::BufReader::new(
        std::fs::File::open(&raw)
            .with_context(|| format!("open rotation segment {}", raw.display()))?,
    );
    let output = std::fs::File::create(&next_compressed)
        .with_context(|| format!("create compressed segment {}", next_compressed.display()))?;
    let mut encoder = zstd::stream::Encoder::new(output, 3).context("create zstd log encoder")?;
    std::io::copy(&mut input, &mut encoder).context("compress rotated log segment")?;
    let output = encoder.finish().context("finish compressed log segment")?;
    output.sync_all().context("sync compressed log segment")?;
    if compressed.exists() {
        std::fs::remove_file(&compressed).with_context(|| {
            format!(
                "remove superseded compressed segment {}",
                compressed.display()
            )
        })?;
    }
    std::fs::rename(&next_compressed, &compressed).with_context(|| {
        format!(
            "publish compressed segment {} to {}",
            next_compressed.display(),
            compressed.display()
        )
    })?;
    std::fs::remove_file(&raw)
        .with_context(|| format!("remove raw rotation segment {}", raw.display()))?;
    let legacy = segment_path(log_path, ".1")?;
    if legacy.exists() {
        std::fs::remove_file(&legacy)
            .with_context(|| format!("remove legacy rotation segment {}", legacy.display()))?;
    }
    Ok(true)
}

/// Rotate and compress one log under a project-level advisory lock.
///
/// Callers invoke this only at a closed-cycle/session-admission boundary. The
/// active file plus one compressed segment form a fixed storage budget.
pub fn rotate_log_if_oversized(log_path: &Path, max_bytes: u64) -> Result<bool> {
    let Some(logs_dir) = log_path.parent() else {
        return Ok(false);
    };
    std::fs::create_dir_all(logs_dir)?;
    let lock_path = logs_dir.join(".rotation.lock");
    let lock = open_rotation_lock(log_path)
        .with_context(|| format!("open log rotation lock {}", lock_path.display()))?;
    FileExt::lock_exclusive(&lock)
        .with_context(|| format!("lock log rotation {}", lock_path.display()))?;
    let outcome = rotate_log_if_oversized_locked(log_path, max_bytes);
    let _ = FileExt::unlock(&lock);
    outcome
}

/// Read the retained compressed/raw segment followed by the active log.
pub fn read_rotated_log(log_path: &Path) -> Result<Option<String>> {
    let legacy = segment_path(log_path, ".1")?;
    let compressed = segment_path(log_path, ".1.zst")?;
    let raw = segment_path(log_path, ".1.raw")?;
    let mut content = String::new();
    let mut found = false;
    if let Some(text) = crate::read_optional_text(&legacy)? {
        content.push_str(&text);
        found = true;
    }
    if compressed.exists() {
        let file = std::fs::File::open(&compressed)
            .with_context(|| format!("open compressed log {}", compressed.display()))?;
        let mut decoder = zstd::stream::Decoder::new(file)
            .with_context(|| format!("decode compressed log {}", compressed.display()))?;
        decoder
            .read_to_string(&mut content)
            .with_context(|| format!("read compressed log {}", compressed.display()))?;
        found = true;
    }
    if let Some(text) = crate::read_optional_text(&raw)? {
        content.push_str(&text);
        found = true;
    }
    if let Some(text) = crate::read_optional_text(log_path)? {
        content.push_str(&text);
        found = true;
    }
    Ok(found.then_some(content))
}
