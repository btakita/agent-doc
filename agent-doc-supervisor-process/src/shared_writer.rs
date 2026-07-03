//! Shared, interruptible writer for supervisor-owned PTYs.

use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, TryLockError};
use std::time::{Duration, Instant};

use anyhow::Result;

const SHARED_WRITER_LOCK_POLL_INTERVAL: Duration = Duration::from_millis(25);
const SHARED_WRITER_WRITE_POLL_INTERVAL_MS: i32 = 50;
const SHARED_WRITER_CHUNK_MAX: usize = 1024;

pub struct SharedPtyWriter {
    writer: Box<dyn Write + Send>,
    #[cfg(unix)]
    raw_fd: Option<std::os::unix::io::RawFd>,
}

/// Signal to stop the stdin-to-PTY writer thread.
///
/// Uses a self-pipe: the writer thread polls both stdin and the pipe read end.
/// Calling [`StopSignal::signal`] writes a byte to the pipe, waking the poll and
/// causing the writer thread to exit cleanly so stdin is available for the
/// supervisor prompt.
#[cfg(unix)]
pub struct StopSignal {
    read_fd: std::os::unix::io::RawFd,
    write_fd: std::os::unix::io::RawFd,
}

#[cfg(unix)]
impl StopSignal {
    pub fn new() -> Result<Self> {
        let mut fds = [0i32; 2];
        if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
            anyhow::bail!("pipe() failed: {}", std::io::Error::last_os_error());
        }
        Ok(Self {
            read_fd: fds[0],
            write_fd: fds[1],
        })
    }

    /// Wake the writer thread so it exits.
    pub fn signal(&self) {
        unsafe {
            libc::write(self.write_fd, b"x".as_ptr() as *const libc::c_void, 1);
        }
    }

    pub fn read_fd(&self) -> std::os::unix::io::RawFd {
        self.read_fd
    }
}

#[cfg(unix)]
impl Drop for StopSignal {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.read_fd);
            libc::close(self.write_fd);
        }
    }
}

#[cfg(not(unix))]
pub struct StopSignal;

#[cfg(not(unix))]
impl StopSignal {
    pub fn new() -> Result<Self> {
        Ok(Self)
    }

    pub fn signal(&self) {}
}

impl SharedPtyWriter {
    pub fn new(writer: Box<dyn Write + Send>) -> Self {
        Self {
            writer,
            #[cfg(unix)]
            raw_fd: None,
        }
    }

    #[cfg(unix)]
    pub fn with_raw_fd(writer: Box<dyn Write + Send>, raw_fd: std::os::unix::io::RawFd) -> Self {
        Self {
            writer,
            raw_fd: Some(raw_fd),
        }
    }

    pub fn write_all_interruptibly(&mut self, bytes: &[u8], stop: &AtomicBool) -> io::Result<()> {
        #[cfg(unix)]
        if let Some(fd) = self.raw_fd {
            return write_all_fd_interruptibly(fd, bytes, stop);
        }

        write_all_with_stop(self.writer.as_mut(), bytes, stop)
    }

    pub fn write_all_blocking(&mut self, bytes: &[u8]) -> io::Result<()> {
        let never_stop = AtomicBool::new(false);
        self.write_all_interruptibly(bytes, &never_stop)
    }
}

#[cfg(unix)]
impl Drop for SharedPtyWriter {
    fn drop(&mut self) {
        if let Some(fd) = self.raw_fd.take() {
            unsafe {
                libc::close(fd);
            }
        }
    }
}

pub fn lock_writer_interruptibly<'a>(
    writer_arc: &'a Arc<Mutex<SharedPtyWriter>>,
    stop: &AtomicBool,
) -> Option<std::sync::MutexGuard<'a, SharedPtyWriter>> {
    loop {
        if stop.load(Ordering::Relaxed) {
            return None;
        }
        match writer_arc.try_lock() {
            Ok(guard) => return Some(guard),
            Err(TryLockError::WouldBlock) => {
                if !sleep_with_stop(stop, SHARED_WRITER_LOCK_POLL_INTERVAL) {
                    return None;
                }
            }
            Err(TryLockError::Poisoned(err)) => return Some(err.into_inner()),
        }
    }
}

fn write_all_with_stop(writer: &mut dyn Write, bytes: &[u8], stop: &AtomicBool) -> io::Result<()> {
    let mut written = 0;
    while written < bytes.len() {
        if stop.load(Ordering::Relaxed) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "writer cancelled",
            ));
        }
        let end = (written + SHARED_WRITER_CHUNK_MAX).min(bytes.len());
        let n = writer.write(&bytes[written..end])?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "writer returned 0 bytes",
            ));
        }
        written += n;
    }
    if stop.load(Ordering::Relaxed) {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "writer cancelled",
        ));
    }
    writer.flush()
}

#[cfg(unix)]
fn write_all_fd_interruptibly(
    fd: std::os::unix::io::RawFd,
    bytes: &[u8],
    stop: &AtomicBool,
) -> io::Result<()> {
    let mut written = 0;
    while written < bytes.len() {
        if stop.load(Ordering::Relaxed) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "writer cancelled",
            ));
        }

        let mut fds = [libc::pollfd {
            fd,
            events: libc::POLLOUT,
            revents: 0,
        }];
        let ret = unsafe {
            libc::poll(
                fds.as_mut_ptr(),
                fds.len() as libc::nfds_t,
                SHARED_WRITER_WRITE_POLL_INTERVAL_MS,
            )
        };
        if ret < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err);
        }
        if ret == 0 {
            continue;
        }

        let revents = fds[0].revents;
        if revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                format!("pty writer poll failed: revents=0x{revents:x}"),
            ));
        }
        if revents & libc::POLLOUT == 0 {
            continue;
        }

        let end = (written + SHARED_WRITER_CHUNK_MAX).min(bytes.len());
        let chunk = &bytes[written..end];
        let n = unsafe { libc::write(fd, chunk.as_ptr() as *const libc::c_void, chunk.len()) };
        if n < 0 {
            let err = io::Error::last_os_error();
            if matches!(
                err.kind(),
                io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
            ) {
                continue;
            }
            return Err(err);
        }
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "pty master write returned 0 bytes",
            ));
        }
        written += n as usize;
    }
    Ok(())
}

fn sleep_with_stop(stop: &AtomicBool, total: Duration) -> bool {
    let deadline = Instant::now() + total;
    loop {
        if stop.load(Ordering::Relaxed) {
            return false;
        }
        let now = Instant::now();
        if now >= deadline {
            return true;
        }
        let remaining = deadline.saturating_duration_since(now);
        std::thread::sleep(std::cmp::min(remaining, Duration::from_millis(100)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn blocking_write_reaches_inner_writer() {
        let mut writer = SharedPtyWriter::new(Box::new(Vec::<u8>::new()));
        writer.write_all_blocking(b"abc").unwrap();
    }

    #[test]
    fn lock_writer_interruptibly_returns_none_after_stop() {
        let writer = Arc::new(Mutex::new(SharedPtyWriter::new(Box::new(Vec::<u8>::new()))));
        let guard = writer.lock().unwrap();
        let stop = AtomicBool::new(true);

        assert!(lock_writer_interruptibly(&writer, &stop).is_none());
        drop(guard);
    }
}
