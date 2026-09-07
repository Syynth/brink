//! The notification service — `docs/studio-shell-spec.md` §7.5.
//!
//! The toasts themselves were never the missing part: the kit draws those,
//! and the app root composes the layer they land in. What was missing is
//! the SERVICE around them — severities with their own dismissal rule, a
//! source on every notice, a capped history, and one place every producer
//! reports through so a failure cannot be mistaken for a success.
//!
//! Every producer in the studio goes through [`notify`]. It does two
//! things at once: shows the toast, and records the notice in the history
//! the status bar's bell opens. A toast is gone in seconds; the record is
//! what lets an author come back and ask what that red thing said.
//!
//! **Dismissal is by severity** (§7.5): info and success fade, a warning
//! fades, an error stays until it is dismissed. An error that vanishes
//! while you are reading the line it is about is an error nobody read.
//!
//! **Bounded**: [`CAP`] notices, oldest dropped — the project's standing
//! rule for anything that accumulates.
//!
//! Not built here, and deliberately: notification ACTIONS (§7.5's
//! "actions dispatch commands only"). Nothing in the studio yet has an
//! undoable operation to offer — the Binder's undo stack is not built —
//! so the button would have nothing to dispatch. The shape leaves room:
//! a notice carries its `source`, and an `actions` field can join it
//! without moving anything else.

use std::collections::VecDeque;

use gpui::{App, Global, SharedString, Window};
use gpui_component::WindowExt as _;
use gpui_component::notification::Notification;

/// The most notices kept in the history.
pub const CAP: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    Success,
    Warning,
    Error,
}

impl Severity {
    /// Whether the toast fades on its own. An error does not: §7.5 makes
    /// it sticky, and the reason is that the one notice you must not miss
    /// is the one saying something failed.
    #[must_use]
    pub const fn autohides(self) -> bool {
        !matches!(self, Self::Error)
    }

    /// The word the history row shows.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Success => "ok",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

/// One notice, as the history keeps it.
#[derive(Debug, Clone)]
pub struct Notice {
    pub severity: Severity,
    pub message: SharedString,
    /// Which part of the studio spoke — "binder", "rename", "settings".
    /// Shown subdued, so a reader can tell a compiler complaint from a
    /// file operation's without reading the whole line.
    pub source: SharedString,
    /// Wall-clock `hh:mm:ss`, as the Output log writes it.
    pub at: SharedString,
}

/// The session's notices, newest last, and how many have not been seen.
#[derive(Debug, Default)]
pub struct Notifications {
    notices: VecDeque<Notice>,
    dropped: usize,
    unread: usize,
}

impl Global for Notifications {}

impl Notifications {
    /// The notices as they are, or an empty history before anything has
    /// been notified (which is also every test that never notifies).
    #[must_use]
    pub fn get(cx: &App) -> Vec<Notice> {
        cx.try_global::<Self>()
            .map(|n| n.notices.iter().cloned().collect())
            .unwrap_or_default()
    }

    #[must_use]
    pub fn unread(cx: &App) -> usize {
        cx.try_global::<Self>().map_or(0, |n| n.unread)
    }

    #[must_use]
    pub fn dropped(cx: &App) -> usize {
        cx.try_global::<Self>().map_or(0, |n| n.dropped)
    }

    /// Record a notice. Split from [`notify`] so the rule is testable
    /// without a window to show a toast in.
    pub fn record(&mut self, notice: Notice) {
        self.notices.push_back(notice);
        self.unread += 1;
        while self.notices.len() > CAP {
            self.notices.pop_front();
            self.dropped += 1;
        }
    }

    /// Everything has been seen — the bell's badge goes.
    pub fn mark_read(cx: &mut App) {
        cx.default_global::<Self>().unread = 0;
    }

    /// Forget the history. The unread count goes with it: there is
    /// nothing left to have not read.
    pub fn clear(cx: &mut App) {
        let this = cx.default_global::<Self>();
        this.notices.clear();
        this.dropped = 0;
        this.unread = 0;
    }
}

/// Show a notice and record it — the one door every producer uses.
pub fn notify(
    severity: Severity,
    source: &str,
    message: impl Into<SharedString>,
    window: &mut Window,
    cx: &mut App,
) {
    let message = message.into();
    let toast = match severity {
        Severity::Info => Notification::info(message.clone()),
        Severity::Success => Notification::success(message.clone()),
        Severity::Warning => Notification::warning(message.clone()),
        Severity::Error => Notification::error(message.clone()),
    };
    window.push_notification(toast.autohide(severity.autohides()), cx);
    cx.default_global::<Notifications>().record(Notice {
        severity,
        message,
        source: SharedString::from(source.to_owned()),
        at: clock(),
    });
}

/// Local wall-clock as `hh:mm:ss`, UTC — the Output log's own clock, for
/// the same reason and with the same trade-off (a date library for three
/// fields would cost more than it is worth).
#[must_use]
pub fn clock() -> SharedString {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let day = secs % 86_400;
    SharedString::from(format!(
        "{:02}:{:02}:{:02}",
        day / 3600,
        (day % 3600) / 60,
        day % 60
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notice(severity: Severity, message: &str) -> Notice {
        Notice {
            severity,
            message: message.into(),
            source: "test".into(),
            at: "01:00:00".into(),
        }
    }

    #[test]
    fn an_error_stays_up_and_everything_else_fades() {
        assert!(!Severity::Error.autohides(), "the one you must not miss");
        assert!(Severity::Warning.autohides());
        assert!(Severity::Info.autohides());
        assert!(Severity::Success.autohides());
    }

    #[test]
    fn the_history_is_capped_and_counts_what_it_dropped() {
        let mut n = Notifications::default();
        for i in 0..CAP + 5 {
            n.record(notice(Severity::Info, &format!("row {i}")));
        }
        assert_eq!(n.notices.len(), CAP);
        assert_eq!(n.dropped, 5);
        assert_eq!(
            n.notices.front().map(|x| x.message.to_string()),
            Some("row 5".to_owned()),
            "oldest dropped first"
        );
        assert_eq!(
            n.unread,
            CAP + 5,
            "unread counts what happened, not what is kept"
        );
    }

    #[test]
    fn every_severity_says_something_different() {
        let mut labels: Vec<&str> = [
            Severity::Info,
            Severity::Success,
            Severity::Warning,
            Severity::Error,
        ]
        .iter()
        .map(|s| s.label())
        .collect();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), 4);
    }
}
