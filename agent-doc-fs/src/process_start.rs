//! A process's start time from the kernel's own record (GH #263).
//!
//! The only start time Linux records for a process is field 22 of
//! `/proc/<pid>/stat` (`starttime`, clock ticks since boot). Turning it into a
//! wall-clock instant needs the boot time (`btime` in `/proc/stat`) and the tick
//! rate (`sysconf(_SC_CLK_TCK)`).
//!
//! The mtime of the `/proc/<pid>` directory is NOT a start time: procfs stamps
//! it when it instantiates the inode, which is usually seconds before the read
//! and changes between reads. Comparing a timestamp against that mtime made a
//! days-old process look freshly started (GH #263).
//!
//! Every reader takes the proc root as a parameter so tests can model the
//! kernel contract with a fake tree holding real `stat` contents.

use std::path::Path;
use std::time::{Duration, SystemTime};

/// `(state, starttime)` from one `/proc/<pid>/stat` line. `comm` (field 2) may
/// contain spaces and parentheses, so fields are counted from the LAST `)`.
pub fn parse_proc_stat_state_and_start_ticks(stat: &str) -> Option<(char, u64)> {
    let rest = &stat[stat.rfind(')')? + 1..];
    let mut fields = rest.split_whitespace();
    let state = fields.next()?.chars().next()?;
    // After `state` (field 3), `starttime` is field 22: skip fields 4..=21.
    let start_ticks = fields.nth(18)?.parse::<u64>().ok()?;
    Some((state, start_ticks))
}

/// `btime` (boot time, seconds since the Unix epoch) from `/proc/stat`.
pub fn parse_boot_time_secs(proc_stat: &str) -> Option<u64> {
    proc_stat.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        (fields.next()? == "btime")
            .then(|| fields.next()?.parse::<u64>().ok())
            .flatten()
    })
}

/// Kernel clock ticks per second (`USER_HZ`), the unit of `starttime`.
#[cfg(unix)]
pub fn clock_ticks_per_sec() -> Option<u64> {
    // SAFETY: `sysconf` reads a process-constant configuration value.
    let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    u64::try_from(ticks).ok().filter(|ticks| *ticks > 0)
}

#[cfg(not(unix))]
pub fn clock_ticks_per_sec() -> Option<u64> {
    None
}

/// The raw `/proc/<pid>/stat` line under `proc_root`.
pub fn read_proc_stat_at(proc_root: &Path, pid: u32) -> Option<String> {
    std::fs::read_to_string(proc_root.join(pid.to_string()).join("stat")).ok()
}

/// `starttime` (clock ticks since boot) of `pid` under `proc_root`. Stable for
/// the life of the process, so it also identifies one process across pid reuse.
pub fn process_start_ticks_at(proc_root: &Path, pid: u32) -> Option<u64> {
    parse_proc_stat_state_and_start_ticks(&read_proc_stat_at(proc_root, pid)?)
        .map(|(_, start_ticks)| start_ticks)
}

/// [`process_start_ticks_at`] against the live `/proc`.
pub fn process_start_ticks(pid: u32) -> Option<u64> {
    process_start_ticks_at(Path::new("/proc"), pid)
}

/// Boot time (`btime`, seconds since the Unix epoch) under `proc_root`.
pub fn boot_time_secs_at(proc_root: &Path) -> Option<u64> {
    parse_boot_time_secs(&std::fs::read_to_string(proc_root.join("stat")).ok()?)
}

/// The wall-clock instant `start_ticks` after boot. `btime` is whole seconds
/// (truncated) and `starttime` whole ticks, so the result is at most about one
/// second EARLIER than the true start, never later.
pub fn start_time_from_ticks(
    start_ticks: u64,
    boot_secs: u64,
    ticks_per_sec: u64,
) -> Option<SystemTime> {
    if ticks_per_sec == 0 {
        return None;
    }
    let whole_secs = start_ticks / ticks_per_sec;
    let sub_nanos = (start_ticks % ticks_per_sec).checked_mul(1_000_000_000)? / ticks_per_sec;
    SystemTime::UNIX_EPOCH
        .checked_add(Duration::from_secs(boot_secs.checked_add(whole_secs)?))?
        .checked_add(Duration::from_nanos(sub_nanos))
}

/// When `pid` started, from the kernel record under `proc_root`. `None` when
/// the process is gone, `/proc` is unavailable (non-Linux), or any field is
/// unreadable; callers decide their own fail-safe direction.
pub fn process_start_time_at(proc_root: &Path, pid: u32) -> Option<SystemTime> {
    start_time_from_ticks(
        process_start_ticks_at(proc_root, pid)?,
        boot_time_secs_at(proc_root)?,
        clock_ticks_per_sec()?,
    )
}

/// [`process_start_time_at`] against the live `/proc`.
pub fn process_start_time(pid: u32) -> Option<SystemTime> {
    process_start_time_at(Path::new("/proc"), pid)
}

/// Whole seconds `pid` has been running at `now`. A start time after `now`
/// (wall clock stepped backwards) reads as age 0.
pub fn process_start_age_secs_at(proc_root: &Path, pid: u32, now: SystemTime) -> Option<u64> {
    let started_at = process_start_time_at(proc_root, pid)?;
    Some(
        now.duration_since(started_at)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or(0),
    )
}

/// [`process_start_age_secs_at`] against the live `/proc` and clock.
pub fn process_start_age_secs(pid: u32) -> Option<u64> {
    process_start_age_secs_at(Path::new("/proc"), pid, SystemTime::now())
}

/// Fake-`/proc` writers that model the kernel contract for regressions in this
/// and dependent crates: a real `stat` line and a `/proc/stat` `btime`, never a
/// hand-set directory mtime (GH #263).
#[doc(hidden)]
pub mod fixture {
    use std::path::Path;

    /// A `/proc/<pid>/stat` line as the kernel prints it, with a `comm` that
    /// holds spaces and parentheses; `starttime` is field 22.
    pub fn proc_stat_line(pid: u32, state: char, start_ticks: u64) -> String {
        let mut fields = vec![state.to_string()];
        // fields 4..=21
        fields.extend((4..=21).map(|field| field.to_string()));
        fields.push(start_ticks.to_string());
        // trailing fields (vsize, rss, rsslim, ...)
        fields.extend(["1000", "200", "18446744073709551615"].map(str::to_string));
        format!("{pid} (we ird) (comm) {}\n", fields.join(" "))
    }

    /// Write `<proc_root>/stat` with `btime boot_secs` and
    /// `<proc_root>/<pid>/stat` with `starttime start_ticks`.
    pub fn write_fake_process_start(proc_root: &Path, pid: u32, boot_secs: u64, start_ticks: u64) {
        std::fs::create_dir_all(proc_root.join(pid.to_string())).expect("fake proc pid dir");
        std::fs::write(
            proc_root.join("stat"),
            format!("cpu  1 2 3 4\nintr 5\nctxt 6\nbtime {boot_secs}\nprocesses 7\n"),
        )
        .expect("fake /proc/stat");
        std::fs::write(
            proc_root.join(pid.to_string()).join("stat"),
            proc_stat_line(pid, 'S', start_ticks),
        )
        .expect("fake /proc/<pid>/stat");
    }

    /// Ticks for `secs_after_boot` at this host's real tick rate.
    pub fn ticks_after_boot(secs_after_boot: u64) -> u64 {
        secs_after_boot * super::clock_ticks_per_sec().unwrap_or(100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_proc(boot_secs: u64, pid: u32, start_ticks: u64) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        fixture::write_fake_process_start(root.path(), pid, boot_secs, start_ticks);
        root
    }

    #[test]
    fn stat_parse_counts_fields_from_the_last_paren() {
        assert_eq!(
            parse_proc_stat_state_and_start_ticks(&fixture::proc_stat_line(7, 'Z', 12_345)),
            Some(('Z', 12_345))
        );
        assert_eq!(parse_proc_stat_state_and_start_ticks("garbage"), None);
        assert_eq!(parse_proc_stat_state_and_start_ticks("1 (x) S 2 3"), None);
    }

    #[test]
    fn boot_time_parse_reads_only_the_btime_line() {
        assert_eq!(
            parse_boot_time_secs("cpu 1 2\nbtime 1700000000\nprocesses 4\n"),
            Some(1_700_000_000)
        );
        assert_eq!(parse_boot_time_secs("cpu 1 2\nprocesses 4\n"), None);
        assert_eq!(parse_boot_time_secs("btime nope\n"), None);
    }

    #[test]
    fn start_time_adds_ticks_to_boot_time_with_subsecond_precision() {
        assert_eq!(
            start_time_from_ticks(12_350, 1_000, 100),
            Some(SystemTime::UNIX_EPOCH + Duration::from_millis(1_123_500))
        );
        assert_eq!(start_time_from_ticks(1, 1, 0), None);
    }

    /// GH #263: the `/proc/<pid>` directory's mtime is when procfs made the
    /// inode (about "now"), not when the process started. The start time comes
    /// from `starttime` + `btime` alone.
    #[test]
    fn fresh_proc_dir_mtime_does_not_make_an_old_process_young() {
        let hz = clock_ticks_per_sec().expect("unix test host has a tick rate");
        let boot_secs = 1_700_000_000;
        let pid = 4242;
        // started one hour after boot
        let proc_root = fake_proc(boot_secs, pid, 3_600 * hz);
        // The pid directory was just created, so its mtime is "now".
        let expected = SystemTime::UNIX_EPOCH + Duration::from_secs(boot_secs + 3_600);
        assert_eq!(process_start_time_at(proc_root.path(), pid), Some(expected));
        assert_eq!(
            process_start_age_secs_at(
                proc_root.path(),
                pid,
                expected + Duration::from_secs(2 * 86_400)
            ),
            Some(2 * 86_400)
        );
        // A clock stepped behind the start reads as age 0, not as a failure.
        assert_eq!(
            process_start_age_secs_at(proc_root.path(), pid, expected - Duration::from_secs(5)),
            Some(0)
        );
        // Missing pid / missing boot record fail as `None`.
        assert_eq!(process_start_time_at(proc_root.path(), pid + 1), None);
        std::fs::remove_file(proc_root.path().join("stat")).unwrap();
        assert_eq!(process_start_time_at(proc_root.path(), pid), None);
    }

    /// The live kernel: this process started in the past (at or before this
    /// test began), after boot, and its start time is stable between reads,
    /// unlike the `/proc/<pid>` mtime.
    #[cfg(target_os = "linux")]
    #[test]
    fn live_process_start_time_is_in_the_past_and_stable() {
        let entered = SystemTime::now();
        let pid = std::process::id();
        let started = process_start_time(pid).expect("own start time");
        let boot = boot_time_secs_at(Path::new("/proc")).expect("btime");
        assert!(
            started <= entered,
            "{started:?} must not be after {entered:?}"
        );
        assert!(started >= SystemTime::UNIX_EPOCH + Duration::from_secs(boot));
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(process_start_time(pid), Some(started));
        // pid 1 started no later than this process.
        if let Some(init) = process_start_time(1) {
            assert!(init <= started, "{init:?} vs {started:?}");
        }
        assert!(process_start_age_secs(pid).is_some());
    }
}
