//! The panic log: every panic, on any thread, written to a file before the
//! process goes down.
//!
//! A studio launched from the Dock has no terminal, so a panic's message —
//! the one thing that says what happened — went nowhere: the app vanished
//! and the only trace was `exit-state` still reading `running`. Each panic
//! now leaves `<settings dir>/crashes/panic-<UTC time>.log` holding the
//! message, where it was raised, the thread, and a backtrace. A panic on a
//! worker thread (the Player's story worker) is logged too, though it does
//! not take the app down.
//!
//! The previous hook still runs afterwards, so a terminal launch prints as
//! it always did.

use std::backtrace::Backtrace;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Logs kept; the oldest go first. A crash loop must not fill the disk.
const KEEP: usize = 20;

/// Where panic logs go: `crashes/` under the settings folder.
#[must_use]
pub fn crash_dir() -> Option<PathBuf> {
    crate::settings::settings_dir().map(|dir| dir.join("crashes"))
}

/// Install the hook. Call first thing in `main`, before anything that can
/// panic. No settings folder means no log, and the previous hook alone.
pub fn install() {
    let Some(dir) = crash_dir() else {
        return;
    };
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "(a panic with no message)".to_owned());
        let location = info.location().map_or_else(String::new, |l| {
            format!("{}:{}:{}", l.file(), l.line(), l.column())
        });
        let thread = std::thread::current();
        let report = Report {
            message: &message,
            location: &location,
            thread: thread.name().unwrap_or("(unnamed)"),
            backtrace: &Backtrace::force_capture().to_string(),
        };
        // A hook must not panic; a log that cannot be written is dropped.
        let _ = write(&dir, SystemTime::now(), &report);
        previous(info);
    }));
}

/// What one panic log holds.
pub struct Report<'a> {
    pub message: &'a str,
    pub location: &'a str,
    pub thread: &'a str,
    pub backtrace: &'a str,
}

/// Write `report` into `dir` as of `now`, then prune to the newest
/// [`KEEP`]. Answers the file written.
///
/// # Errors
/// The folder or the file could not be written.
pub fn write(dir: &Path, now: SystemTime, report: &Report<'_>) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let stamp = utc_stamp(now);
    let mut path = dir.join(format!("panic-{stamp}.log"));
    // Two panics in one second (a worker and the main thread) keep both.
    let mut n = 1;
    while path.exists() {
        n += 1;
        path = dir.join(format!("panic-{stamp}-{n}.log"));
    }
    let text = format!(
        "brink studio {version} panicked at {stamp}\n\
         thread: {thread}\n\
         at: {location}\n\
         \n\
         {message}\n\
         \n\
         backtrace:\n\
         {backtrace}\n",
        version = env!("CARGO_PKG_VERSION"),
        thread = report.thread,
        location = report.location,
        message = report.message,
        backtrace = report.backtrace,
    );
    std::fs::write(&path, text)?;
    prune(dir);
    Ok(path)
}

/// Remove all but the newest [`KEEP`] logs. The names sort by time.
fn prune(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut logs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("panic-") && n.ends_with(".log"))
        })
        .collect();
    logs.sort();
    let excess = logs.len().saturating_sub(KEEP);
    for old in &logs[..excess] {
        let _ = std::fs::remove_file(old);
    }
}

/// `now` as `YYYY-MM-DDTHH-MM-SSZ` — sortable, and safe in a file name.
fn utc_stamp(now: SystemTime) -> String {
    let secs = now.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let rem = secs % 86_400;
    // Howard Hinnant's days-to-civil.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}-{:02}-{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "brink-gpui-crash-log-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn report() -> Report<'static> {
        Report {
            message: "index out of bounds",
            location: "app/src/player.rs:1:1",
            thread: "main",
            backtrace: "0: somewhere",
        }
    }

    #[test]
    fn a_stamp_is_the_utc_date_and_time() {
        // 2026-10-09 23:49:09 UTC.
        let at = UNIX_EPOCH + Duration::from_secs(1_791_589_749);
        assert_eq!(utc_stamp(at), "2026-10-09T23-49-09Z");
        assert_eq!(utc_stamp(UNIX_EPOCH), "1970-01-01T00-00-00Z");
        // A leap day.
        let leap = UNIX_EPOCH + Duration::from_secs(951_782_400);
        assert_eq!(utc_stamp(leap), "2000-02-29T00-00-00Z");
    }

    #[test]
    fn a_log_holds_the_message_place_thread_and_backtrace() {
        let dir = scratch("holds");
        let path = write(&dir, UNIX_EPOCH, &report()).expect("written");
        let text = std::fs::read_to_string(&path).expect("reads");
        for part in [
            "index out of bounds",
            "app/src/player.rs:1:1",
            "thread: main",
            "0: somewhere",
        ] {
            assert!(text.contains(part), "{part:?} missing from {text}");
        }
    }

    #[test]
    fn two_panics_in_one_second_keep_both_and_old_logs_are_pruned() {
        let dir = scratch("prune");
        let first = write(&dir, UNIX_EPOCH, &report()).expect("first");
        let second = write(&dir, UNIX_EPOCH, &report()).expect("second");
        assert_ne!(first, second);
        for s in 0..(KEEP as u64 + 5) {
            write(&dir, UNIX_EPOCH + Duration::from_secs(10 + s), &report()).expect("one more");
        }
        let left = std::fs::read_dir(&dir).expect("lists").count();
        assert_eq!(left, KEEP);
        assert!(!first.exists(), "the oldest went first");
    }
}
