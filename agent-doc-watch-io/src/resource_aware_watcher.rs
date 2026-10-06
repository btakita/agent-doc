use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::Duration;

use notify::{Config, Error, ErrorKind, Event, PollWatcher, RecursiveMode, Watcher};

/// The filesystem-event backend currently serving a watcher.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WatchBackend {
    Native,
    Polling,
}

/// Whether an operation had to leave the native backend because the shared
/// account-level watch quota was exhausted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WatchFallback {
    Unchanged,
    Activated,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WatchErrorDecision {
    ActivatePolling,
    Report,
}

fn watch_error_decision(backend: WatchBackend, error: &Error) -> WatchErrorDecision {
    match (&backend, &error.kind) {
        (WatchBackend::Native, ErrorKind::MaxFilesWatch) => WatchErrorDecision::ActivatePolling,
        _ => WatchErrorDecision::Report,
    }
}

fn event_handler(tx: Sender<notify::Result<Event>>) -> impl notify::EventHandler + 'static {
    move |event| {
        if tx.send(event).is_err() {
            eprintln!("[agent-doc] file watcher event receiver closed");
        }
    }
}

/// An event-first watcher that falls back to bounded polling only when the
/// platform watcher reports resource exhaustion.
///
/// Linux inotify limits are per account, not per process. IDEs and build tools
/// can therefore exhaust the quota before agent-doc registers a path. Moving a
/// caller or native binding into another process cannot restore capacity; a
/// polling backend can. The fallback keeps all already-registered paths and
/// publishes the same `notify::Event` stream to the caller.
pub struct ResourceAwareWatcher {
    watcher: Box<dyn Watcher>,
    backend: WatchBackend,
    watched: Vec<(PathBuf, RecursiveMode)>,
    event_tx: Sender<notify::Result<Event>>,
    poll_interval: Duration,
}

impl ResourceAwareWatcher {
    pub fn new(
        event_tx: Sender<notify::Result<Event>>,
        poll_interval: Duration,
    ) -> notify::Result<Self> {
        match notify::recommended_watcher(event_handler(event_tx.clone())) {
            Ok(watcher) => Ok(Self {
                watcher: Box::new(watcher),
                backend: WatchBackend::Native,
                watched: Vec::new(),
                event_tx,
                poll_interval,
            }),
            Err(error)
                if watch_error_decision(WatchBackend::Native, &error)
                    == WatchErrorDecision::ActivatePolling =>
            {
                let watcher = Self::poll_watcher(&event_tx, poll_interval)?;
                Ok(Self {
                    watcher,
                    backend: WatchBackend::Polling,
                    watched: Vec::new(),
                    event_tx,
                    poll_interval,
                })
            }
            Err(error) => Err(error),
        }
    }

    pub fn backend(&self) -> WatchBackend {
        self.backend
    }

    /// Register `path`, preserving prior registrations if quota exhaustion
    /// requires switching the whole watcher to polling.
    pub fn watch(
        &mut self,
        path: &Path,
        recursive_mode: RecursiveMode,
    ) -> notify::Result<WatchFallback> {
        match self.watcher.watch(path, recursive_mode) {
            Ok(()) => {
                self.remember(path, recursive_mode);
                Ok(WatchFallback::Unchanged)
            }
            Err(error)
                if watch_error_decision(self.backend, &error)
                    == WatchErrorDecision::ActivatePolling =>
            {
                self.activate_polling(Some((path.to_path_buf(), recursive_mode)))?;
                Ok(WatchFallback::Activated)
            }
            Err(error) => Err(error),
        }
    }

    /// React to an asynchronous backend error delivered through the event
    /// channel. Only native resource exhaustion changes the backend.
    pub fn recover_from_event_error(&mut self, error: &Error) -> notify::Result<WatchFallback> {
        if watch_error_decision(self.backend, error) == WatchErrorDecision::ActivatePolling {
            self.activate_polling(None)?;
            Ok(WatchFallback::Activated)
        } else {
            Ok(WatchFallback::Unchanged)
        }
    }

    fn poll_watcher(
        event_tx: &Sender<notify::Result<Event>>,
        poll_interval: Duration,
    ) -> notify::Result<Box<dyn Watcher>> {
        // `notify` truncates polling mtimes to whole seconds. Compare contents
        // too so two edits within one second cannot disappear during fallback.
        let config = Config::default()
            .with_poll_interval(poll_interval)
            .with_compare_contents(true);
        PollWatcher::new(event_handler(event_tx.clone()), config)
            .map(|watcher| Box::new(watcher) as Box<dyn Watcher>)
    }

    fn activate_polling(&mut self, extra: Option<(PathBuf, RecursiveMode)>) -> notify::Result<()> {
        let mut watched = self.watched.clone();
        if let Some((path, recursive_mode)) = extra
            && !watched.iter().any(|(existing, _)| existing == &path)
        {
            watched.push((path, recursive_mode));
        }

        let mut replacement = Self::poll_watcher(&self.event_tx, self.poll_interval)?;
        for (path, recursive_mode) in &watched {
            replacement.watch(path, *recursive_mode)?;
        }

        self.watcher = replacement;
        self.backend = WatchBackend::Polling;
        self.watched = watched;
        Ok(())
    }

    fn remember(&mut self, path: &Path, recursive_mode: RecursiveMode) {
        if !self.watched.iter().any(|(existing, _)| existing == path) {
            self.watched.push((path.to_path_buf(), recursive_mode));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    use tempfile::TempDir;

    #[test]
    fn native_quota_exhaustion_activates_polling() {
        let error = Error::new(ErrorKind::MaxFilesWatch);
        assert_eq!(
            watch_error_decision(WatchBackend::Native, &error),
            WatchErrorDecision::ActivatePolling
        );
        assert_eq!(
            watch_error_decision(WatchBackend::Polling, &error),
            WatchErrorDecision::Report
        );
    }

    #[test]
    fn unrelated_watch_errors_remain_visible() {
        let error = Error::new(ErrorKind::PathNotFound);
        assert_eq!(
            watch_error_decision(WatchBackend::Native, &error),
            WatchErrorDecision::Report
        );
    }

    #[test]
    fn polling_transition_preserves_existing_registrations() {
        let dir = TempDir::new().unwrap();
        let first = dir.path().join("first.md");
        let second = dir.path().join("second.md");
        std::fs::write(&first, "before").unwrap();
        std::fs::write(&second, "before").unwrap();

        let (tx, rx) = mpsc::channel();
        let mut watcher = ResourceAwareWatcher::new(tx, Duration::from_millis(25)).unwrap();
        watcher.watch(&first, RecursiveMode::NonRecursive).unwrap();
        watcher
            .activate_polling(Some((second.clone(), RecursiveMode::NonRecursive)))
            .unwrap();
        assert_eq!(watcher.backend(), WatchBackend::Polling);
        assert_eq!(watcher.watched.len(), 2);

        std::thread::sleep(Duration::from_millis(75));
        std::fs::write(&first, "after with a different length").unwrap();

        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let mut saw_first = false;
        while std::time::Instant::now() < deadline {
            let Ok(event) = rx.recv_timeout(Duration::from_millis(100)) else {
                continue;
            };
            let event = event.unwrap();
            if event.paths.iter().any(|path| path == &first) {
                saw_first = true;
                break;
            }
        }
        assert!(saw_first, "polling replacement lost the existing watch");
    }
}
