//! [`KindTieredHandler`] — a composable [`ExternalFnHandler`] wrapper that
//! gates externals during speculative/watch evaluation by their effect
//! kind, without the runtime knowing anything about a host manifest.
//!
//! The runtime stays **manifest-blind**: this module has no dependency on
//! `brink-ir` or any analyzer type. [`PolicyKind`] is plain data — the
//! consumer (e.g. celeris, bevy-brink) maps the analyzer's `ExternalKind`
//! onto it (`Query` → [`PolicyKind::Query`]; `Effect`/`Presentation`/
//! unclassified → [`PolicyKind::Effect`], conservative-by-default) and
//! hands the resulting `name -> PolicyKind` table to
//! [`KindTieredHandler::new`].
//!
//! Like [`crate::RecordingHandler`]/[`crate::ReplayHandler`], this *wraps*
//! a real [`ExternalFnHandler`] rather than threading a policy through the
//! stepping hot loop — the caller stacks it: e.g.
//! `spec.advance(budget, &KindTieredHandler::new(&real_bindings, kinds, EvalContext::Watch, false))`.

use std::cell::RefCell;
use std::collections::HashMap;

use brink_format::Value;

use crate::story::{ExternalFnHandler, ExternalResult};

/// The effect category a [`KindTieredHandler`] gates on. Plain data — the
/// consumer maps its own (richer) external classification onto this
/// two-way split.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyKind {
    /// A read-only query (no side effects). Always delegated live.
    Query,
    /// A state-changing (or otherwise non-query) effect. Gated by
    /// [`EvalContext`] and the handler's `live_effects` arming.
    Effect,
}

/// Which evaluation regime a [`KindTieredHandler`] is gating for.
///
/// `Watch` is the conservative default: no effect ever fires live,
/// regardless of arming. `Eval` additionally requires `live_effects` to be
/// armed before an effect is allowed through — a deliberate two-key gate
/// so effects don't fire just because the caller happens to be in an
/// engine→ink eval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvalContext {
    /// Read-only inspection (e.g. a live-inspector "what would this show"
    /// probe). Effects never fire live.
    Watch,
    /// An engine→ink function evaluation that may be permitted to run
    /// live effects if `live_effects` is armed.
    Eval,
}

/// Diagnostic record of which externals a [`KindTieredHandler`] let
/// through live versus fell back, across the handler's lifetime.
///
/// Purely informational — nothing in the runtime reads this back, and its
/// ordering (call order) has no bearing on story-visible behavior.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExternalsReport {
    /// Ink-declared names of externals delegated to the real handler
    /// (live), in call order.
    pub live: Vec<String>,
    /// Ink-declared names of externals that fell back to the ink fallback
    /// body (blocked by tiering, or a `Watch`/disarmed `Effect`), in call
    /// order.
    pub fallback: Vec<String>,
}

/// A stackable [`ExternalFnHandler`] that tiers externals by
/// [`PolicyKind`] before delegating to a real handler.
///
/// - `Query` externals are always delegated live to `inner` (including a
///   `Pending` result, passed straight through for async resolution).
/// - `Effect` externals are delegated live only when `context ==
///   EvalContext::Eval` *and* `live_effects` is armed; otherwise they
///   resolve to [`ExternalResult::Fallback`] (the ink fallback body, or a
///   named-external VM error if none is declared).
/// - A name absent from `kinds` is treated as `Effect` — conservative by
///   default.
///
/// No coupling to [`crate::Speculation`] — this is a plain composable
/// handler, usable anywhere an `&dyn ExternalFnHandler` is accepted.
pub struct KindTieredHandler<'h> {
    inner: &'h dyn ExternalFnHandler,
    kinds: HashMap<String, PolicyKind>,
    context: EvalContext,
    live_effects: bool,
    report: RefCell<ExternalsReport>,
}

impl<'h> KindTieredHandler<'h> {
    /// Build a handler gating calls to `inner` by `kinds`.
    ///
    /// `kinds` maps ink-declared external names to their [`PolicyKind`];
    /// a name absent from the map is treated as `Effect`. `context` picks
    /// the evaluation regime; `live_effects` arms `Effect` externals when
    /// `context == EvalContext::Eval` (ignored under `Watch`, where
    /// effects never fire live).
    #[must_use]
    pub fn new(
        inner: &'h dyn ExternalFnHandler,
        kinds: HashMap<String, PolicyKind>,
        context: EvalContext,
        live_effects: bool,
    ) -> Self {
        Self {
            inner,
            kinds,
            context,
            live_effects,
            report: RefCell::new(ExternalsReport::default()),
        }
    }

    /// Snapshot of which externals have run live versus fallen back so
    /// far. Diagnostic only.
    #[must_use]
    pub fn report(&self) -> ExternalsReport {
        self.report.borrow().clone()
    }

    /// Whether an external of the given kind is allowed to run live under
    /// this handler's current context/arming.
    fn armed(&self, kind: PolicyKind) -> bool {
        match kind {
            PolicyKind::Query => true,
            PolicyKind::Effect => self.context == EvalContext::Eval && self.live_effects,
        }
    }
}

impl ExternalFnHandler for KindTieredHandler<'_> {
    fn call(&self, name: &str, args: &[Value]) -> ExternalResult {
        let kind = self.kinds.get(name).copied().unwrap_or(PolicyKind::Effect);
        if self.armed(kind) {
            self.report.borrow_mut().live.push(name.to_owned());
            self.inner.call(name, args)
        } else {
            self.report.borrow_mut().fallback.push(name.to_owned());
            ExternalResult::Fallback
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stub handler: `Resolved` for names in its table, `Pending` for
    /// the sentinel name `"async"`, else `Fallback`.
    struct Stub;
    impl ExternalFnHandler for Stub {
        fn call(&self, name: &str, _args: &[Value]) -> ExternalResult {
            match name {
                "async" => ExternalResult::Pending,
                "get" => ExternalResult::Resolved(Value::Int(5)),
                "act" => ExternalResult::Resolved(Value::Bool(true)),
                _ => ExternalResult::Fallback,
            }
        }
    }

    fn kinds(pairs: &[(&str, PolicyKind)]) -> HashMap<String, PolicyKind> {
        pairs.iter().map(|(n, k)| ((*n).to_owned(), *k)).collect()
    }

    #[test]
    fn query_delegates_live_including_pending() {
        let inner = Stub;
        let h = KindTieredHandler::new(
            &inner,
            kinds(&[("get", PolicyKind::Query), ("async", PolicyKind::Query)]),
            EvalContext::Watch,
            false,
        );
        assert!(matches!(
            h.call("get", &[]),
            ExternalResult::Resolved(Value::Int(5))
        ));
        assert!(matches!(h.call("async", &[]), ExternalResult::Pending));
        assert_eq!(h.report().live, vec!["get".to_owned(), "async".to_owned()]);
        assert!(h.report().fallback.is_empty());
    }

    #[test]
    fn effect_under_watch_falls_back_inner_not_called() {
        let inner = Stub;
        let h = KindTieredHandler::new(
            &inner,
            kinds(&[("act", PolicyKind::Effect)]),
            EvalContext::Watch,
            true, // even armed, Watch never lets effects through
        );
        assert!(matches!(h.call("act", &[]), ExternalResult::Fallback));
        assert_eq!(h.report().fallback, vec!["act".to_owned()]);
        assert!(h.report().live.is_empty());
    }

    #[test]
    fn effect_under_eval_disarmed_falls_back() {
        let inner = Stub;
        let h = KindTieredHandler::new(
            &inner,
            kinds(&[("act", PolicyKind::Effect)]),
            EvalContext::Eval,
            false,
        );
        assert!(matches!(h.call("act", &[]), ExternalResult::Fallback));
        assert_eq!(h.report().fallback, vec!["act".to_owned()]);
    }

    #[test]
    fn effect_under_eval_armed_runs_live() {
        let inner = Stub;
        let h = KindTieredHandler::new(
            &inner,
            kinds(&[("act", PolicyKind::Effect)]),
            EvalContext::Eval,
            true,
        );
        assert!(matches!(
            h.call("act", &[]),
            ExternalResult::Resolved(Value::Bool(true))
        ));
        assert_eq!(h.report().live, vec!["act".to_owned()]);
    }

    #[test]
    fn unclassified_name_treated_as_effect() {
        let inner = Stub;
        // "act" is absent from `kinds` entirely.
        let h = KindTieredHandler::new(&inner, kinds(&[]), EvalContext::Watch, false);
        assert!(matches!(h.call("act", &[]), ExternalResult::Fallback));
        assert_eq!(h.report().fallback, vec!["act".to_owned()]);

        // Even armed + Eval, an unclassified name is still conservative
        // Effect tiering — it runs live only because Effect is armed here,
        // proving it wasn't silently treated as Query.
        let h2 = KindTieredHandler::new(&inner, kinds(&[]), EvalContext::Eval, true);
        assert!(matches!(
            h2.call("act", &[]),
            ExternalResult::Resolved(Value::Bool(true))
        ));
        assert_eq!(h2.report().live, vec!["act".to_owned()]);
    }

    #[test]
    fn report_reflects_mixed_live_and_fallback_in_call_order() {
        let inner = Stub;
        let h = KindTieredHandler::new(
            &inner,
            kinds(&[("get", PolicyKind::Query), ("act", PolicyKind::Effect)]),
            EvalContext::Watch,
            false,
        );
        let _ = h.call("get", &[]);
        let _ = h.call("act", &[]);
        let _ = h.call("get", &[]);
        let report = h.report();
        assert_eq!(report.live, vec!["get".to_owned(), "get".to_owned()]);
        assert_eq!(report.fallback, vec!["act".to_owned()]);
    }
}
