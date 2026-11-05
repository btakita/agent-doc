//! `agent-doc build-slot -- <cmd...>` (`#buildconcurrencygate`): run a heavy
//! build or test command holding one of N machine-wide build slots.
//!
//! Slots are `flock(2)` files in a stable per-user directory
//! ([`default_slot_dir`]), so every worktree, coordinator session, and harness
//! on the machine shares one set. The kernel drops a lock when its descriptor
//! closes, so a slot is released when this process exits for any reason,
//! SIGKILL included; on Linux the child also gets SIGTERM if this process dies
//! first, so a killed gate does not leave an unslotted build running.
//!
//! Waiting is FIFO through a gate lock: a blocked `flock` on `gate.lock` queues
//! waiters in the kernel, and only the gate holder polls the slots. `flock` has
//! no "any one of N" wait, so that single waiter's bounded poll is the narrow
//! imperative exception: this is an actorless one-shot CLI boundary with no
//! lifecycle graph to host an `Effect`. The poll also re-reads the load
//! average, which is how a load spike closes slots
//! ([`agent_doc_queue::build_slot::effective_build_slots`]).
//!
//! Sizing is pure and lives in `agent_doc_queue::build_slot`.

use std::ffi::OsString;
use std::fs::{File, OpenOptions, TryLockError};
use std::io::{Read, Seek, Write};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use agent_doc_queue::build_slot as policy;
use anyhow::{Context, Result, bail};

/// How often the gate holder re-checks the slots and the load average.
pub const BUILD_SLOT_POLL: Duration = Duration::from_secs(2);

/// Cores this process may use.
pub fn host_cores() -> usize {
    std::thread::available_parallelism().map_or(1, NonZeroUsize::get)
}

/// The one-minute load average, when the host exposes one.
pub fn host_load1() -> Option<f64> {
    std::fs::read_to_string("/proc/loadavg")
        .ok()
        .as_deref()
        .and_then(policy::parse_loadavg)
}

/// `AGENT_DOC_BUILD_SLOTS`, validated.
pub fn configured_build_slots() -> Result<Option<NonZeroUsize>> {
    match std::env::var(policy::BUILD_SLOTS_ENV) {
        Ok(value) => policy::parse_build_slots(&value)
            .map(Some)
            .map_err(|err| anyhow::anyhow!("{}: {err}", policy::BUILD_SLOTS_ENV)),
        Err(_) => Ok(None),
    }
}

/// The host's base slot count: the seam a subagent dispatch pool
/// sizes from. An invalid override is ignored here (the `build-slot` command
/// reports it), so a caller never fails on it.
pub fn host_base_build_slots() -> NonZeroUsize {
    policy::base_build_slots(host_cores(), configured_build_slots().ok().flatten())
}

/// Slots open on this host right now (base scaled by the current load).
pub fn host_effective_build_slots() -> NonZeroUsize {
    policy::effective_build_slots(host_base_build_slots(), host_cores(), host_load1())
}

/// The shared slot directory: `AGENT_DOC_BUILD_SLOT_DIR`, else
/// `$XDG_RUNTIME_DIR/agent-doc/build-slots`, else
/// `/tmp/agent-doc-build-slots-<uid>`. It is deliberately not `TMPDIR`: a
/// harness may give each session its own `TMPDIR`, and slots must be shared.
pub fn default_slot_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os(policy::BUILD_SLOT_DIR_ENV).filter(|dir| !dir.is_empty()) {
        return PathBuf::from(dir);
    }
    if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR").filter(|dir| !dir.is_empty()) {
        let runtime = PathBuf::from(runtime);
        if runtime.is_dir() {
            return runtime.join("agent-doc").join("build-slots");
        }
    }
    #[cfg(unix)]
    let user = unsafe { libc::getuid() }.to_string();
    #[cfg(not(unix))]
    let user = std::env::var("USERNAME").unwrap_or_else(|_| "user".to_string());
    PathBuf::from("/tmp").join(format!("agent-doc-build-slots-{user}"))
}

/// The slot directory and its lock files.
#[derive(Debug, Clone)]
pub struct SlotGate {
    dir: PathBuf,
}

/// One held slot; dropping it releases the lock.
#[derive(Debug)]
pub struct HeldSlot {
    pub index: usize,
    pub of: usize,
    file: File,
}

impl HeldSlot {
    /// Record who holds the slot, for the wait message other commands print.
    pub fn record_holder(&mut self, holder: &str) {
        let _ = self.file.set_len(0);
        let _ = self.file.rewind();
        let _ = self.file.write_all(holder.as_bytes());
        let _ = self.file.flush();
    }
}

fn open_lock(path: &Path) -> Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .with_context(|| format!("open build slot lock {}", path.display()))
}

impl SlotGate {
    pub fn open(dir: &Path) -> Result<Self> {
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        builder
            .create(dir)
            .with_context(|| format!("create build slot directory {}", dir.display()))?;
        Ok(Self {
            dir: dir.to_path_buf(),
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn slot_path(&self, index: usize) -> PathBuf {
        self.dir.join(format!("slot{index}"))
    }

    /// Take the first free slot among `1..=slots`, without waiting.
    pub fn try_acquire(&self, slots: NonZeroUsize) -> Result<Option<HeldSlot>> {
        for index in 1..=slots.get() {
            let file = open_lock(&self.slot_path(index))?;
            match file.try_lock() {
                Ok(()) => {
                    return Ok(Some(HeldSlot {
                        index,
                        of: slots.get(),
                        file,
                    }));
                }
                Err(TryLockError::WouldBlock) => {}
                Err(TryLockError::Error(err)) => {
                    return Err(err).with_context(|| format!("lock build slot {index}"));
                }
            }
        }
        Ok(None)
    }

    /// What each slot's last holder recorded, for slots `1..=slots`.
    pub fn holders(&self, slots: NonZeroUsize) -> Vec<String> {
        (1..=slots.get())
            .map(|index| {
                let mut text = String::new();
                let _ = File::open(self.slot_path(index))
                    .and_then(|mut file| file.read_to_string(&mut text));
                let text = text.trim();
                format!("slot{index}: {}", if text.is_empty() { "?" } else { text })
            })
            .collect()
    }

    /// The FIFO gate in front of the slots. `Ok(None)` from the non-blocking
    /// attempt means another command is already queued ahead.
    fn gate(&self, block: bool) -> Result<Option<File>> {
        let file = open_lock(&self.dir.join("gate.lock"))?;
        if block {
            file.lock().context("lock build slot gate")?;
            return Ok(Some(file));
        }
        match file.try_lock() {
            Ok(()) => Ok(Some(file)),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(err)) => Err(err).context("lock build slot gate"),
        }
    }

    /// Wait (FIFO) for one of the `effective_build_slots` slots. `load` is
    /// re-read every poll; `pause` is the poll wait, injected so tests drive
    /// it without sleeping.
    pub fn acquire(
        &self,
        base: NonZeroUsize,
        cores: usize,
        mut load: impl FnMut() -> Option<f64>,
        mut pause: impl FnMut(),
        mut log: impl FnMut(&str),
    ) -> Result<HeldSlot> {
        let _gate = match self.gate(false)? {
            Some(gate) => gate,
            None => {
                log("another build is queued ahead; waiting for the build slot gate");
                self.gate(true)?.context("build slot gate")?
            }
        };
        let mut announced = false;
        loop {
            let slots = policy::effective_build_slots(base, cores, load());
            if let Some(held) = self.try_acquire(slots)? {
                return Ok(held);
            }
            if !announced {
                announced = true;
                let mut message = format!(
                    "all {} build slots are busy (base {base}, {cores} cores); waiting",
                    slots
                );
                for holder in self.holders(slots) {
                    message.push_str("\n  ");
                    message.push_str(&holder);
                }
                log(&message);
            }
            pause();
        }
    }
}

/// `agent-doc build-slot` options.
#[derive(Debug, Clone, Default)]
pub struct BuildSlotOptions {
    /// Explicit slot count (`--slots`); else `AGENT_DOC_BUILD_SLOTS`, else cores.
    pub slots: Option<NonZeroUsize>,
    /// Explicit slot directory (`--dir`); else [`default_slot_dir`].
    pub dir: Option<PathBuf>,
    /// Niceness increment for the command (0 leaves priority unchanged).
    pub nice: i32,
    pub command: Vec<OsString>,
}

fn log_line(message: &str) {
    eprintln!("[agent-doc build-slot] {message}");
}

fn display_command(command: &[OsString]) -> String {
    command
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Run `options.command` under a build slot and return its exit status.
pub fn run(options: BuildSlotOptions) -> Result<i32> {
    run_with_parent(options, std::env::var_os(policy::BUILD_SLOT_HELD_ENV))
}

/// [`run`] with the inherited `AGENT_DOC_BUILD_SLOT_HELD` value injected.
pub fn run_with_parent(options: BuildSlotOptions, parent: Option<OsString>) -> Result<i32> {
    let Some((program, args)) = options.command.split_first() else {
        bail!("build-slot needs a command: agent-doc build-slot -- <command...>");
    };
    let mut command = std::process::Command::new(program);
    command.args(args);
    // A nested `build-slot` (e.g. a Makefile recipe under a gated `make`)
    // already runs inside its parent's slot; waiting would deadlock at N=1.
    let mut _held = None;
    if let Some(parent) = parent {
        log_line(&format!(
            "running inside held slot {}: {}",
            parent.to_string_lossy(),
            display_command(&options.command)
        ));
    } else {
        let base = match options.slots {
            Some(slots) => slots,
            None => policy::base_build_slots(host_cores(), configured_build_slots()?),
        };
        let gate = SlotGate::open(&options.dir.clone().unwrap_or_else(default_slot_dir))?;
        let started = Instant::now();
        let mut held = gate.acquire(
            base,
            host_cores(),
            host_load1,
            || std::thread::sleep(BUILD_SLOT_POLL),
            log_line,
        )?;
        let shown = display_command(&options.command);
        let cwd = std::env::current_dir()
            .map(|dir| dir.display().to_string())
            .unwrap_or_default();
        held.record_holder(&format!("pid {} in {cwd}: {shown}", std::process::id()));
        log_line(&format!(
            "slot {}/{} acquired after {}s ({}): {shown}",
            held.index,
            held.of,
            started.elapsed().as_secs(),
            gate.dir().display()
        ));
        command.env(
            policy::BUILD_SLOT_HELD_ENV,
            format!("{}/{}", held.index, held.of),
        );
        _held = Some(held);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let nice = options.nice;
        // SAFETY: the hook runs in the forked child before exec and calls only
        // async-signal-safe libc functions.
        unsafe {
            command.pre_exec(move || {
                #[cfg(target_os = "linux")]
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                if nice != 0 {
                    libc::nice(nice);
                }
                Ok(())
            });
        }
    }
    let status = command
        .status()
        .with_context(|| format!("run {}", program.to_string_lossy()))?;
    Ok(exit_code(status))
}

fn exit_code(status: std::process::ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return 128 + signal;
        }
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(value: usize) -> NonZeroUsize {
        NonZeroUsize::new(value).unwrap()
    }

    #[test]
    fn slots_are_exclusive_and_free_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let gate = SlotGate::open(dir.path()).unwrap();
        let first = gate.try_acquire(n(2)).unwrap().unwrap();
        let second = gate.try_acquire(n(2)).unwrap().unwrap();
        assert_eq!((first.index, second.index), (1, 2));
        assert!(gate.try_acquire(n(2)).unwrap().is_none());
        drop(first);
        assert_eq!(gate.try_acquire(n(2)).unwrap().unwrap().index, 1);
    }

    #[test]
    fn acquire_waits_until_a_slot_frees_and_names_the_holders() {
        let dir = tempfile::tempdir().unwrap();
        let gate = SlotGate::open(dir.path()).unwrap();
        let mut busy = gate.try_acquire(n(1)).unwrap().unwrap();
        busy.record_holder("pid 7 in /w: make check");
        let mut busy = Some(busy);
        let mut logs = Vec::new();
        let mut pauses = 0;
        let held = gate
            .acquire(
                n(1),
                32,
                || None,
                || {
                    pauses += 1;
                    busy.take();
                },
                |line| logs.push(line.to_string()),
            )
            .unwrap();
        assert_eq!((held.index, held.of, pauses), (1, 1, 1));
        assert_eq!(logs.len(), 1, "{logs:?}");
        assert!(logs[0].contains("all 1 build slots are busy"), "{logs:?}");
        assert!(
            logs[0].contains("slot1: pid 7 in /w: make check"),
            "{logs:?}"
        );
    }

    #[test]
    fn load_above_cores_closes_slots_until_it_falls() {
        let dir = tempfile::tempdir().unwrap();
        let gate = SlotGate::open(dir.path()).unwrap();
        // Slot 1 is busy; base 4 would admit slot 2, but load 100 on 32 cores
        // leaves one slot, so the command waits until the load falls.
        let _busy = gate.try_acquire(n(1)).unwrap().unwrap();
        let mut loads = vec![Some(20.0), Some(100.0)];
        let mut logs = Vec::new();
        let held = gate
            .acquire(
                n(4),
                32,
                || loads.pop().flatten(),
                || {},
                |line| logs.push(line.to_string()),
            )
            .unwrap();
        assert_eq!((held.index, held.of), (2, 4));
        assert!(logs[0].contains("all 1 build slots are busy"), "{logs:?}");
    }

    #[test]
    fn run_propagates_the_exit_status_and_marks_the_slot_held() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("held");
        let script = format!(
            "printf %s \"${}\" > '{}'; exit 7",
            policy::BUILD_SLOT_HELD_ENV,
            marker.display()
        );
        let options = BuildSlotOptions {
            slots: Some(n(1)),
            dir: Some(dir.path().join("slots")),
            nice: 0,
            command: vec!["sh".into(), "-c".into(), script.into()],
        };
        let code = run_with_parent(options.clone(), None).unwrap();
        assert_eq!(code, 7);
        assert_eq!(std::fs::read_to_string(&marker).unwrap(), "1/1");
        // The slot was released on return.
        let gate = SlotGate::open(&dir.path().join("slots")).unwrap();
        let busy = gate.try_acquire(n(1)).unwrap().unwrap();
        // A nested gate runs inside its parent's slot instead of waiting on it.
        let code = run_with_parent(options, Some("1/1".into())).unwrap();
        assert_eq!(code, 7);
        drop(busy);
    }

    #[test]
    fn an_empty_command_is_refused() {
        let err = run_with_parent(BuildSlotOptions::default(), None).unwrap_err();
        assert!(format!("{err}").contains("agent-doc build-slot -- <command...>"));
    }
}
