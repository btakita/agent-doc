//! LSP input ownership for `zed-lsp`.
//!
//! The server owns every byte it reads from Zed in one buffer
//! (`Server::pending_input`), and frames are cut from that buffer only on the
//! serving thread. That is what makes the binary handoff
//! ([`crate::handoff`]) lossless: at any point between two handled messages,
//! the bytes Zed has written are either still in the pipe or in the buffer,
//! never in a reader thread's private buffer. A handoff serializes the buffer,
//! and the replacement image parses it first.

use anyhow::{Context, Result, anyhow};
use serde_json::Value;
use std::io::{self, Read};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Duration;

/// What one bounded read produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReadOutcome {
    /// Bytes were appended to the buffer.
    Data,
    /// Nothing arrived within the timeout.
    Timeout,
    /// Zed closed the pipe.
    Eof,
}

/// A byte source the serving thread drains into its own buffer.
pub(crate) trait LspInput {
    fn read_available(
        &mut self,
        buffer: &mut Vec<u8>,
        timeout: Duration,
    ) -> io::Result<ReadOutcome>;

    /// Whether every byte this source has read is in the caller's buffer.
    ///
    /// Only then may the process image be replaced: a source that reads on a
    /// helper thread can hold bytes the serving thread has not seen.
    fn handoff_safe(&self) -> bool;
}

/// Standard input read directly on the serving thread with `poll(2)`.
#[cfg(unix)]
pub(crate) struct StdinFd;

#[cfg(unix)]
impl LspInput for StdinFd {
    fn read_available(
        &mut self,
        buffer: &mut Vec<u8>,
        timeout: Duration,
    ) -> io::Result<ReadOutcome> {
        let mut poll = libc::pollfd {
            fd: libc::STDIN_FILENO,
            events: libc::POLLIN,
            revents: 0,
        };
        let millis = timeout.as_millis().min(i32::MAX as u128) as i32;
        // SAFETY: one valid pollfd for the lifetime of the call.
        let ready = unsafe { libc::poll(&mut poll, 1, millis) };
        if ready < 0 {
            let error = io::Error::last_os_error();
            return if error.kind() == io::ErrorKind::Interrupted {
                Ok(ReadOutcome::Timeout)
            } else {
                Err(error)
            };
        }
        if ready == 0 {
            return Ok(ReadOutcome::Timeout);
        }
        let mut chunk = [0u8; 64 * 1024];
        // SAFETY: `chunk` is a valid writable buffer of the given length.
        let read = unsafe {
            libc::read(
                libc::STDIN_FILENO,
                chunk.as_mut_ptr().cast::<libc::c_void>(),
                chunk.len(),
            )
        };
        if read < 0 {
            let error = io::Error::last_os_error();
            return match error.kind() {
                io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock => Ok(ReadOutcome::Timeout),
                _ => Err(error),
            };
        }
        if read == 0 {
            return Ok(ReadOutcome::Eof);
        }
        buffer.extend_from_slice(&chunk[..read as usize]);
        Ok(ReadOutcome::Data)
    }

    fn handoff_safe(&self) -> bool {
        true
    }
}

/// Chunks delivered over a channel: a reader thread on hosts without
/// `poll(2)`, and scripted input in tests.
pub(crate) struct ChunkInput {
    receiver: Receiver<io::Result<Vec<u8>>>,
    /// A helper thread reads ahead of the serving thread.
    threaded: bool,
}

impl ChunkInput {
    #[cfg_attr(unix, allow(dead_code))]
    pub(crate) fn spawn_reader<R: Read + Send + 'static>(mut reader: R) -> Self {
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let mut chunk = vec![0u8; 64 * 1024];
            loop {
                match reader.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(read) => {
                        if sender.send(Ok(chunk[..read].to_vec())).is_err() {
                            break;
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(error) => {
                        let _ = sender.send(Err(error));
                        break;
                    }
                }
            }
        });
        Self {
            receiver,
            threaded: true,
        }
    }

    #[cfg(test)]
    pub(crate) fn from_receiver(receiver: Receiver<io::Result<Vec<u8>>>) -> Self {
        Self {
            receiver,
            threaded: false,
        }
    }
}

impl LspInput for ChunkInput {
    fn read_available(
        &mut self,
        buffer: &mut Vec<u8>,
        timeout: Duration,
    ) -> io::Result<ReadOutcome> {
        match self.receiver.recv_timeout(timeout) {
            Ok(Ok(chunk)) => {
                buffer.extend_from_slice(&chunk);
                Ok(ReadOutcome::Data)
            }
            Ok(Err(error)) => Err(error),
            Err(RecvTimeoutError::Timeout) => Ok(ReadOutcome::Timeout),
            Err(RecvTimeoutError::Disconnected) => Ok(ReadOutcome::Eof),
        }
    }

    fn handoff_safe(&self) -> bool {
        !self.threaded
    }
}

/// Cut one complete LSP frame from the front of `buffer`.
///
/// Returns `Ok(None)` and leaves `buffer` untouched while the frame is
/// incomplete. Header lines may end in `\r\n` (the LSP base protocol) or `\n`.
pub(crate) fn take_frame(buffer: &mut Vec<u8>) -> Result<Option<Value>> {
    let mut position = 0usize;
    let mut content_length = None;
    loop {
        let Some(newline) = buffer[position..].iter().position(|byte| *byte == b'\n') else {
            return Ok(None);
        };
        let line = &buffer[position..position + newline];
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        position += newline + 1;
        if line.is_empty() {
            break;
        }
        let line = std::str::from_utf8(line).context("LSP header is not UTF-8")?;
        if let Some(value) = line.trim().strip_prefix("Content-Length:").map(str::trim) {
            content_length = Some(value.parse::<usize>().context("invalid Content-Length")?);
        }
    }
    let length = content_length.ok_or_else(|| anyhow!("missing Content-Length"))?;
    if buffer.len() < position + length {
        return Ok(None);
    }
    let message = serde_json::from_slice(&buffer[position..position + length])
        .context("invalid LSP JSON payload")?;
    buffer.drain(..position + length);
    Ok(Some(message))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn frame(message: &Value) -> Vec<u8> {
        let mut bytes = Vec::new();
        crate::write_lsp_message(&mut bytes, message).unwrap();
        bytes
    }

    #[test]
    fn frames_are_cut_only_when_complete() {
        let first = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize"});
        let second = json!({"jsonrpc": "2.0", "method": "initialized", "params": {}});
        let mut stream = frame(&first);
        stream.extend(frame(&second));

        // Feed one byte at a time: no frame is produced early, none is lost.
        let mut buffer = Vec::new();
        let mut decoded = Vec::new();
        for byte in stream {
            buffer.push(byte);
            while let Some(message) = take_frame(&mut buffer).unwrap() {
                decoded.push(message);
            }
        }
        assert_eq!(decoded, vec![first, second]);
        assert!(buffer.is_empty());
    }

    #[test]
    fn bare_newline_headers_are_accepted() {
        let mut buffer = b"Content-Length: 2\n\n{}rest".to_vec();
        assert_eq!(take_frame(&mut buffer).unwrap(), Some(json!({})));
        assert_eq!(buffer, b"rest");
    }

    #[test]
    fn a_header_without_content_length_is_an_error() {
        let mut buffer = b"Content-Type: x\r\n\r\n{}".to_vec();
        assert!(take_frame(&mut buffer).is_err());
    }

    #[test]
    fn scripted_chunk_input_is_handoff_safe_but_a_reader_thread_is_not() {
        let (_sender, receiver) = mpsc::channel();
        assert!(ChunkInput::from_receiver(receiver).handoff_safe());
        assert!(!ChunkInput::spawn_reader(std::io::empty()).handoff_safe());
    }
}
