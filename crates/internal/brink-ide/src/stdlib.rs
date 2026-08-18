//! T1b stdlib slice 1 metadata (docs/t1b-surface-spec.md §5) — the shared
//! data table behind completion, hover, and signature help for the eight
//! brink-dialect free functions (`len`/`keys`/`values`/`contains`/`push`/
//! `insert`/`remove`/`remove_at` — `remove_at` added by issue #1484's
//! `remove`/`remove_at` split, decision log "Quick-docket closures"
//! 2026-07-26).
//!
//! A pure, dialect-agnostic data table: callers decide whether/how to gate
//! by [`brink_analyzer::Dialect`] (completion and signature help gate to
//! `Brink` only, per the issue's "never offered in `StrictInk`" rule; hover
//! stays informational in both dialects, mirroring [`crate::builtin_hover_text`]).
//!
//! NOT a full hand-kept copy of `brink_ir::lir::is_t1b_stdlib_name` (issue
//! #2863 investigation): that list has grown well past slice 1 (NS-A1
//! through NS-A8, the numeric tower, the fn-value verb layer, …) while
//! this table has deliberately stayed at slice 1's original eight — an
//! older version of this comment claimed it was "a third hand-kept copy of
//! the same names," which stopped being true the first time
//! `is_t1b_stdlib_name` grew past slice 1 without a matching entry here.
//! This is a genuinely smaller, differently-shaped table: hover/completion
//! metadata (param names, lvalue-ness, return type, doc text) that the
//! canonical name-only list doesn't carry, curated to the names this
//! surface actually documents.
//!
//! What *is* mechanically enforced (issue #2863): every name in
//! [`STDLIB_FUNCTIONS`] is a real stdlib name — `tests::every_stdlib_fn_name_is_a_real_t1b_stdlib_name`
//! below asserts each one against `brink_ir::lir::is_t1b_stdlib_name`
//! (the canonical list `brink_analyzer::resolve::is_t1b_stdlib_name`
//! itself delegates to), catching a rename/removal upstream leaving a
//! stale entry here. It does not — and structurally cannot — assert the
//! reverse (every canonical name has an entry here): that direction is a
//! deliberate, ongoing content gap (slice 2+ names have no hover/
//! completion metadata yet), not a bug.

/// One stdlib slice-1 function's static signature + docs.
#[derive(Debug, Clone, Copy)]
pub struct StdlibFn {
    pub name: &'static str,
    pub params: &'static [StdlibParam],
    /// `None` for the lvalue mutators (`push`/`insert`/`remove`/
    /// `remove_at`) — they return nothing and are statement-only
    /// (docs/t1b-surface-spec.md §5).
    pub returns: Option<&'static str>,
    /// One-line semantics, used as the hover body and signature-help
    /// documentation.
    pub doc: &'static str,
}

/// One parameter's static shape.
#[derive(Debug, Clone, Copy)]
pub struct StdlibParam {
    pub name: &'static str,
    /// Whether this parameter must be an lvalue — a variable, temp, or
    /// indexed path (docs/t1b-surface-spec.md §4/§5). True only for a
    /// mutator's first argument. Rendered as `name: lvalue` in signature
    /// help and completion detail, matching the issue text's
    /// `push(a: lvalue, v)`.
    pub is_lvalue: bool,
}

impl StdlibParam {
    /// Render one parameter label, e.g. `a: lvalue` or plain `v`.
    #[must_use]
    pub fn label(&self) -> String {
        if self.is_lvalue {
            format!("{}: lvalue", self.name)
        } else {
            self.name.to_owned()
        }
    }
}

impl StdlibFn {
    /// Render the signature label, e.g. `push(a: lvalue, v)` or
    /// `contains(x, v) -> bool`.
    #[must_use]
    pub fn signature_label(&self) -> String {
        let params = self
            .params
            .iter()
            .map(StdlibParam::label)
            .collect::<Vec<_>>()
            .join(", ");
        let ret = self.returns.map_or(String::new(), |r| format!(" -> {r}"));
        format!("{}({params}){ret}", self.name)
    }

    /// Whether this function is a mutator: returns nothing, mutates its
    /// first (lvalue) argument in place, and is statement-only
    /// (docs/t1b-surface-spec.md §5).
    #[must_use]
    pub fn is_mutator(&self) -> bool {
        self.returns.is_none()
    }
}

const fn param(name: &'static str) -> StdlibParam {
    StdlibParam {
        name,
        is_lvalue: false,
    }
}

const fn lvalue_param(name: &'static str) -> StdlibParam {
    StdlibParam {
        name,
        is_lvalue: true,
    }
}

/// The T1b stdlib slice 1 functions (docs/t1b-surface-spec.md §5), pure
/// functions first, then the lvalue mutators — the spec's own presentation
/// order.
pub const STDLIB_FUNCTIONS: &[StdlibFn] = &[
    StdlibFn {
        name: "len",
        params: &[param("x")],
        returns: Some("int"),
        doc: "Number of elements in an array, or keys in a map.",
    },
    StdlibFn {
        name: "keys",
        params: &[param("m")],
        returns: Some("array"),
        doc: "A map's keys, in insertion order.",
    },
    StdlibFn {
        name: "values",
        params: &[param("m")],
        returns: Some("array"),
        doc: "A map's values, in key insertion order.",
    },
    StdlibFn {
        name: "contains",
        params: &[param("x"), param("v")],
        returns: Some("bool"),
        doc: "Whether v is an element of array x, or a key of map x — \
              total: never faults, even when v isn't in the key domain.",
    },
    StdlibFn {
        name: "push",
        params: &[lvalue_param("a"), param("v")],
        returns: None,
        doc: "Mutates array a in place, appending v. a must be an lvalue \
              — bind it to a variable first.",
    },
    StdlibFn {
        name: "insert",
        params: &[lvalue_param("x"), param("k_or_i"), param("v")],
        returns: None,
        doc: "Mutates x in place: sets a map key, or inserts into an array \
              at index k_or_i. x must be an lvalue.",
    },
    StdlibFn {
        name: "remove",
        params: &[lvalue_param("m"), param("k")],
        returns: None,
        doc: "Mutates map m in place, removing key k — a no-op if k was \
              already absent. m must be an lvalue. For an array, see \
              remove_at.",
    },
    StdlibFn {
        name: "remove_at",
        params: &[lvalue_param("a"), param("i")],
        returns: None,
        doc: "Mutates array a in place, removing the element at index i \
              (shifts later elements left). a must be an lvalue; i out of \
              bounds faults.",
    },
];

/// Look up one stdlib function's metadata by name.
#[must_use]
pub fn stdlib_fn(name: &str) -> Option<&'static StdlibFn> {
    STDLIB_FUNCTIONS.iter().find(|f| f.name == name)
}

#[cfg(test)]
mod tests {
    use super::{STDLIB_FUNCTIONS, stdlib_fn};

    #[test]
    fn all_eight_names_present() {
        let names: Vec<_> = STDLIB_FUNCTIONS.iter().map(|f| f.name).collect();
        assert_eq!(
            names,
            [
                "len",
                "keys",
                "values",
                "contains",
                "push",
                "insert",
                "remove",
                "remove_at"
            ]
        );
    }

    #[test]
    fn mutators_have_an_lvalue_first_param_and_no_return() {
        for name in ["push", "insert", "remove", "remove_at"] {
            let f = stdlib_fn(name).expect("stdlib function present");
            assert!(f.is_mutator(), "{name} is a mutator");
            assert!(f.params[0].is_lvalue, "{name}'s first param is an lvalue");
            assert!(
                f.params[1..].iter().all(|p| !p.is_lvalue),
                "{name}'s remaining params are plain"
            );
        }
    }

    #[test]
    fn pure_functions_have_no_lvalue_params_and_a_return() {
        for name in ["len", "keys", "values", "contains"] {
            let f = stdlib_fn(name).expect("stdlib function present");
            assert!(!f.is_mutator(), "{name} is pure");
            assert!(
                f.params.iter().all(|p| !p.is_lvalue),
                "{name} takes no lvalue params"
            );
            assert!(f.returns.is_some(), "{name} returns a value");
        }
    }

    #[test]
    fn push_signature_label_matches_the_lvalue_mutator_rule() {
        let f = stdlib_fn("push").expect("push present");
        assert_eq!(f.signature_label(), "push(a: lvalue, v)");
    }

    #[test]
    fn contains_signature_label_has_no_lvalue_and_a_return_type() {
        let f = stdlib_fn("contains").expect("contains present");
        assert_eq!(f.signature_label(), "contains(x, v) -> bool");
    }

    #[test]
    fn unknown_name_is_not_a_stdlib_function() {
        assert!(stdlib_fn("push_back").is_none());
        assert!(
            stdlib_fn("INT").is_none(),
            "classic uppercase builtins aren't in this table"
        );
    }

    /// Issue #2863: the mechanical half of "makes the ide copies
    /// consistent" — every name this hover/completion table documents must
    /// be a real T1b stdlib name, checked against `brink_ir::lir::
    /// is_t1b_stdlib_name` (the canonical list `brink_analyzer::resolve::
    /// is_t1b_stdlib_name` itself now delegates to; see that function's own
    /// doc). This is a subset check, not an equality check, by design —
    /// [`STDLIB_FUNCTIONS`]'s module doc explains why the reverse direction
    /// (every canonical name has an entry here) is a deliberate content
    /// gap, not a bug this test should flag.
    #[test]
    fn every_stdlib_fn_name_is_a_real_t1b_stdlib_name() {
        for f in STDLIB_FUNCTIONS {
            assert!(
                brink_ir::lir::is_t1b_stdlib_name(f.name),
                "STDLIB_FUNCTIONS lists `{}`, which brink_ir::lir::is_t1b_stdlib_name \
                 (the canonical T1b stdlib name list) no longer recognizes — renamed \
                 or removed upstream without updating this hover/completion table",
                f.name,
            );
        }
    }
}
