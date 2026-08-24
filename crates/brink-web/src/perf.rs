//! Wasm-internal perf counters (measure-first ruling, `docs/decision-log.md`
//! 2026-08-24).
//!
//! The JS-side probe times every boundary call (`wasm.<method>`); these
//! counters decompose the inside of the expensive ones — the
//! `update_and_analyze` phases, the compile pull, the outline/story-graph
//! builds and their `byte_to_utf16` share — so a 400 ms boundary span can be
//! attributed to a phase instead of guessed at.
//!
//! Off by default (`EditorSession::set_perf_enabled`); the wasm binary is
//! shared between dev and production hosts, so gating is a runtime boolean
//! rather than a build flavor. Disabled cost is one branch per site. The
//! store is a `thread_local` (wasm is single-threaded; the native `--lib`
//! test build gets per-thread counters, which the tests treat as isolated)
//! so free functions like `byte_to_utf16` can record without threading a
//! handle through every call site.

use std::cell::RefCell;
use std::collections::BTreeMap;

#[derive(Default, Clone, Copy)]
struct Row {
    count: u64,
    total_ms: f64,
    max_ms: f64,
}

#[derive(Default)]
struct Store {
    enabled: bool,
    // BTreeMap: deterministic report order (project rule).
    rows: BTreeMap<&'static str, Row>,
}

thread_local! {
    static STORE: RefCell<Store> = RefCell::new(Store::default());
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn now_ms() -> f64 {
    // High-resolution clock when the host has one (a window/webview always
    // does); `Date.now()` keeps the counters functional in bare JS shells.
    thread_local! {
        static PERF: Option<web_sys::Performance> =
            web_sys::window().and_then(|w| w.performance());
    }
    PERF.with(|p| {
        p.as_ref()
            .map_or_else(js_sys::Date::now, web_sys::Performance::now)
    })
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn now_ms() -> f64 {
    use std::time::Instant;
    thread_local! {
        static START: Instant = Instant::now();
    }
    START.with(|s| s.elapsed().as_secs_f64() * 1000.0)
}

pub fn enabled() -> bool {
    STORE.with(|s| s.borrow().enabled)
}

pub fn set_enabled(on: bool) {
    STORE.with(|s| s.borrow_mut().enabled = on);
}

pub fn reset() {
    STORE.with(|s| s.borrow_mut().rows.clear());
}

fn record(name: &'static str, ms: f64) {
    STORE.with(|s| {
        let mut store = s.borrow_mut();
        let row = store.rows.entry(name).or_default();
        row.count += 1;
        row.total_ms += ms;
        if ms > row.max_ms {
            row.max_ms = ms;
        }
    });
}

/// Time `f` under `name` when enabled; transparent otherwise.
pub fn time<T>(name: &'static str, f: impl FnOnce() -> T) -> T {
    if !enabled() {
        return f();
    }
    let start = now_ms();
    let out = f();
    record(name, now_ms() - start);
    out
}

/// Count an event without timing it (hot tiny functions where per-call
/// clocking would distort the measurement more than inform it).
pub fn count(name: &'static str) {
    if !enabled() {
        return;
    }
    record(name, 0.0);
}

/// The counters as a JSON object: `{ name: { count, totalMs, maxMs } }`.
pub fn report_json() -> String {
    use std::fmt::Write as _;
    STORE.with(|s| {
        let store = s.borrow();
        let mut out = String::from("{");
        let mut first = true;
        for (name, row) in &store.rows {
            if !first {
                out.push(',');
            }
            first = false;
            // Names are compile-time literals (no escaping needed); numbers
            // are finite by construction. Writing to a String cannot fail.
            let _ = write!(
                out,
                "\"{name}\":{{\"count\":{},\"totalMs\":{:.3},\"maxMs\":{:.3}}}",
                row.count, row.total_ms, row.max_ms
            );
        }
        out.push('}');
        out
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_records_nothing_and_passes_through() {
        set_enabled(false);
        reset();
        assert_eq!(time("t", || 41) + 1, 42);
        count("c");
        assert_eq!(report_json(), "{}");
    }

    #[test]
    fn enabled_counts_and_reports_deterministically() {
        set_enabled(true);
        reset();
        time("b", || ());
        time("a", || ());
        count("a");
        let json = report_json();
        // BTreeMap order: "a" before "b"; "a" has two entries.
        let a_pos = json.find("\"a\"");
        let b_pos = json.find("\"b\"");
        assert!(a_pos < b_pos, "deterministic order violated: {json}");
        assert!(json.contains("\"a\":{\"count\":2"), "{json}");
        set_enabled(false);
        reset();
    }
}
