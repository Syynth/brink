//! Map-literal key diagnostics: the T1b **key-domain** warning
//! (`docs/t1b-surface-spec.md` §3, issue #598, [`check`]) and the B5
//! **duplicate-key** error (`docs/stdlib-spec.md` §9.6, issue #1464,
//! [`check_duplicate_keys`]). Both walk the same `MapLiteral` set, which is
//! why they live together; their *wiring* differs (see
//! [`check_duplicate_keys`]'s own doc).
//!
//! §3 rules the map-literal key domain to int/string/bool at runtime
//! (enforced by `brink-runtime`'s `MapKey::from_value` /
//! `RuntimeError::InvalidMapKeyType`) and says "the analyzer warns on
//! statically-visible non-key types" — a claim that, until this module,
//! nothing implemented (`MapLiteral` lowering did zero key-domain checking;
//! only the runtime fault enforced the domain at all).
//!
//! Policy-independent, like `structs::check_duplicates`'s `E084`: a
//! non-key-domain literal key is a structural authoring mistake detectable
//! from the literal alone, so this runs under *both* `types` policies
//! (unlike `structs::check`'s missing/extra/mistyped trio, which is
//! strict-only because it needs a resolved shape). Both [`check`] and
//! [`check_duplicate_keys`] are wired in under `dialect = Brink ||
//! is_native` — wider than the brink-only block the rest of
//! `per_file_diagnostics` uses, matching `structs::check_duplicates`'s own
//! `E084` wiring (same module doc reasoning): B5 (issue #1464, #1103
//! cascade ruling (A)) made `TypeName { … }` construction reach
//! `MapLiteral` through the native surface (`Map { k: v }`) as well as the
//! brink dialect's own `#{…}` spelling, so a `.brink` file compiled under
//! the default `strict-ink` dialect must still get both `E106` and `E138`.
//! Under `strict-ink` *ink* map literals don't exist at all — `#{…}` is
//! already rejected whole by `dialect_gate`'s E051, so critiquing the
//! inside of rejected syntax would be noise — and no native surface exists
//! there to reach one either, so nothing new fires.
//!
//! Scoped to **statically classifiable** key expressions — a literal kind
//! is either obviously in the domain (`Expr::Int`/`Expr::Bool`/`Expr::String`)
//! or obviously not (a float/array/nested-map/struct/fn/list/divert-target
//! literal). A dynamic key (a variable, call, index, or any other
//! non-literal expression) is not statically visible at all — "Unknown
//! never disagrees", the same posture `structs::literal_ty` takes — and is
//! silently left to the runtime fault.

use rowan::TextRange;

use brink_ir::hir::visit::{self, HirVisitor};
use brink_ir::{Diagnostic, DiagnosticCode, Expr, FileId, HirFile, MapLiteral};

/// One per-literal check, applied to every map literal the walk below
/// reaches. Both of this module's passes are this shape, so they share one
/// walker.
type LiteralCheck = fn(&MapLiteral, FileId, &mut Vec<Diagnostic>);

/// Map-literal key-domain checks over every `#{...}` literal in the project.
/// Callers wire this in per-file, under `dialect = Brink || is_native` (see
/// module doc) — no `types`-policy gate, no shape/resolution table needed.
#[must_use]
pub fn check(files: &[(FileId, &HirFile)]) -> Vec<Diagnostic> {
    walk_map_literals(files, check_literal)
}

/// Duplicate-key check over every map literal in the project (`E138`; B5,
/// issue #1464, #1103 cascade ruling (A) — "duplicate keys in a map literal
/// are a **compile error**, consistent with struct dup-field").
///
/// Split from [`check`] because it is a distinct rule (key-domain vs.
/// duplicate-key), not because of wiring — both now share the same
/// `dialect = Brink || is_native` gate (see module doc): this rule is about
/// the *construction* protocol, which the native surface reaches through
/// `Map { k: v }` (`brink_ir::hir::construct`) as well as the brink
/// dialect's `#{…}` sigil — both lower to the same [`MapLiteral`], so one
/// pass serves both surfaces.
#[must_use]
pub fn check_duplicate_keys(files: &[(FileId, &HirFile)]) -> Vec<Diagnostic> {
    walk_map_literals(files, check_duplicates_in_literal)
}

/// Apply `check` to every map literal reachable in `files`.
fn walk_map_literals(files: &[(FileId, &HirFile)], check: LiteralCheck) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for &(file, hir) in files {
        let mut v = MapKeyVisitor {
            file,
            check,
            diagnostics: &mut out,
        };
        visit::visit(hir, &mut v);
        // File-level declaration initializers aren't part of `visit::visit`'s
        // block-tree walk (see its module doc) — same pattern
        // `structs::check`/`conversions::check`/`dialect_gate`/`annotations`
        // use for VAR/CONST.
        for var in &hir.variables {
            check_expr(&var.value, file, check, &mut out);
        }
        for c in &hir.constants {
            check_expr(&c.value, file, check, &mut out);
        }
    }
    out
}

struct MapKeyVisitor<'a> {
    file: FileId,
    check: LiteralCheck,
    diagnostics: &'a mut Vec<Diagnostic>,
}

impl HirVisitor for MapKeyVisitor<'_> {
    fn visit_exprs(&self) -> bool {
        true
    }

    fn enter_expr(&mut self, expr: &Expr) {
        if let Expr::MapLiteral(m) = expr {
            (self.check)(m, self.file, self.diagnostics);
        }
    }
}

/// Recurse into `expr` looking for map literals — used only for the
/// file-level VAR/CONST initializers `visit::visit` doesn't cover; every
/// other position is already reached through the `HirVisitor` walk above.
/// Mirrors `structs::check_expr`/`conversions::check_expr`'s own shape (a
/// small hand recursion, not worth sharing across modules for one call site
/// each — same rationale `conversions::expr_children`'s doc gives).
fn check_expr(expr: &Expr, file: FileId, check: LiteralCheck, out: &mut Vec<Diagnostic>) {
    if let Expr::MapLiteral(m) = expr {
        check(m, file, out);
    }
    for child in expr_children(expr) {
        check_expr(child, file, check, out);
    }
}

/// Direct child expressions of `expr` — mirrors `structs::expr_children` /
/// `conversions::expr_children` (same rationale: needed only because
/// `check_expr` runs outside the `HirVisitor` walk).
fn expr_children(expr: &Expr) -> Vec<&Expr> {
    match expr {
        Expr::Prefix(_, inner) | Expr::Postfix(inner, _) => vec![inner],
        Expr::FieldAccess(fa) => vec![&fa.base],
        Expr::Infix(ie) => vec![&ie.lhs, &ie.rhs],
        Expr::Call(_, args) => args.iter().collect(),
        Expr::ArrayLiteral(a) => a.elements.iter().collect(),
        Expr::MapLiteral(m) => m.entries.iter().flat_map(|(k, v)| [k, v]).collect(),
        Expr::Index(idx) => vec![&idx.base, &idx.index],
        Expr::StructLiteral(sl) => sl.fields.iter().map(|(_, v)| v).collect(),
        // T1c `#fn(target, args…)`: only the bound arguments are child
        // expressions — the target is a static `Path` field, same as `Call`.
        Expr::FnLiteral(fl) => fl.args.iter().collect(),
        // T1e `ref lvalue-path`: only the operand is a child expression.
        Expr::RefArg(ra) => vec![&ra.operand],
        // A lambda's value expression (issue #1685). A braced body's
        // *statements* are not expressions and cannot be handed to an
        // expression-only walker — see `LambdaBody::value_exprs`.
        Expr::Lambda(l) => l.body.value_exprs(),
        Expr::Range(r) => vec![&r.start, &r.end],
        Expr::String(s) => s
            .parts
            .iter()
            .filter_map(|p| match p {
                brink_ir::StringPart::Interpolation(e) => Some(e.as_ref()),
                brink_ir::StringPart::Literal(_) => None,
            })
            .collect(),
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Path(_)
        | Expr::DivertTarget(_)
        | Expr::ListLiteral(_) => Vec::new(),
    }
}

/// Flag every entry in `m` whose key is a statically-classifiable literal
/// outside the ratified int/string/bool key domain.
fn check_literal(m: &MapLiteral, file: FileId, out: &mut Vec<Diagnostic>) {
    for (key, _value) in &m.entries {
        let Some((kind, own_range)) = non_key_domain_kind(key) else {
            continue;
        };
        out.push(Diagnostic {
            file,
            // The bare scalar variants (float/null/list/divert-target) carry
            // no `ptr` of their own at HIR level (only the wrapper-struct
            // variants — array/map/struct/fn literals — do), so this falls
            // back to the enclosing map literal's own range. Same "point at
            // the whole enclosing literal" fallback `structs::check`'s E069
            // (missing-field) diagnostic uses when there's no more specific
            // span available.
            range: own_range.unwrap_or_else(|| m.ptr.text_range()),
            message: format!(
                "{}: `{kind}` key literal is outside the int/string/bool key domain",
                DiagnosticCode::E106.title(),
            ),
            code: DiagnosticCode::E106,
        });
    }
}

/// A map-literal key that is comparable at compile time — the in-domain
/// literal kinds (`int`/`string`/`bool`, §3's ratified key domain). Two
/// entries collide exactly when their [`StaticKey`]s are equal, which is
/// also the runtime's own `MapKey` identity, so this never reports a
/// collision the runtime wouldn't have.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum StaticKey {
    Int(i32),
    Bool(bool),
    Str(String),
}

impl std::fmt::Display for StaticKey {
    /// Renders the key the way it would be spelled as a map-literal key, so
    /// the `E138` message can name the exact key that collided (`1`,
    /// `true`, `"a"`) instead of only pointing at the enclosing literal.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StaticKey::Int(v) => write!(f, "{v}"),
            StaticKey::Bool(b) => write!(f, "{b}"),
            StaticKey::Str(s) => write!(f, "{s:?}"),
        }
    }
}

/// The compile-time identity of a key expression, when it has one.
///
/// Deliberately narrow, the same "Unknown never disagrees" posture the
/// key-domain check takes: a variable, call, or *interpolated* string is
/// not statically comparable (`#{"{a}": 1, "{b}": 2}` may or may not
/// collide at runtime), so it is skipped rather than guessed at. An
/// out-of-domain key (float, array, …) is skipped too — `E106` already
/// owns that mistake, and reporting a second diagnostic for it would just
/// be noise.
fn static_key(expr: &Expr) -> Option<StaticKey> {
    match expr {
        Expr::Int(v) => Some(StaticKey::Int(*v)),
        Expr::Bool(b) => Some(StaticKey::Bool(*b)),
        Expr::String(s) => match s.parts.as_slice() {
            [brink_ir::StringPart::Literal(text)] => Some(StaticKey::Str(text.clone())),
            _ => None,
        },
        _ => None,
    }
}

/// Flag every entry in `m` whose key repeats an earlier entry's key
/// (`E138`). Order-independent by construction: keys are accumulated in
/// source order into a `BTreeSet`, so the *second* occurrence is the one
/// reported and the diagnostic list is deterministic.
fn check_duplicates_in_literal(m: &MapLiteral, file: FileId, out: &mut Vec<Diagnostic>) {
    let mut seen: std::collections::BTreeSet<StaticKey> = std::collections::BTreeSet::new();
    for (key, _value) in &m.entries {
        let Some(k) = static_key(key) else {
            continue;
        };
        if seen.insert(k.clone()) {
            continue;
        }
        out.push(Diagnostic {
            file,
            // In-domain scalar keys carry no `ptr` of their own at HIR
            // level, so this points at the enclosing literal — the same
            // fallback `check_literal`'s `E106` diagnostic documents.
            range: m.ptr.text_range(),
            message: format!(
                "{}: duplicate key `{k}` — an earlier entry already supplies it",
                DiagnosticCode::E138.title(),
            ),
            code: DiagnosticCode::E138,
        });
    }
}

/// Classify a map-literal key expression as a statically-visible
/// non-key-domain literal, if it is one. `Some((kind, range))` — `kind` is a
/// short human-readable name for the message, `range` is the literal's own
/// text range when it carries a `ptr` (`None` for the bare scalar variants
/// that don't, see [`check_literal`]'s fallback). `Int`/`Bool`/`String` are
/// in-domain (never flagged); any other, non-literal expression (a
/// variable, call, index, field access, or other operator expression) is
/// not statically classifiable at all and returns `None` too — the runtime
/// `InvalidMapKeyType` fault remains the backstop for those.
fn non_key_domain_kind(expr: &Expr) -> Option<(&'static str, Option<TextRange>)> {
    match expr {
        Expr::Float(_) => Some(("float", None)),
        // `Expr::Null` deliberately excluded: there is no `null` keyword in
        // ink/brink source syntax (it's only ever an internal default for an
        // uninitialized `VAR`/`CONST`), so it can never actually appear as a
        // map-literal key expression — a defensive branch the grammar
        // already makes unreachable.
        //
        // The ink `LIST` literal (`(item1, item2)`), distinct from the
        // brink `#[...]` array sigil below — matches `brink-runtime`'s own
        // `type_name`'s "list" label for `Value::List`.
        Expr::ListLiteral(_) => Some(("list", None)),
        Expr::DivertTarget(_) => Some(("divert target", None)),
        Expr::ArrayLiteral(a) => Some(("array", Some(a.ptr.text_range()))),
        Expr::MapLiteral(m) => Some(("map", Some(m.ptr.text_range()))),
        Expr::StructLiteral(sl) => Some(("struct", Some(sl.ptr.text_range()))),
        Expr::FnLiteral(fl) => Some(("function", Some(fl.ptr.text_range()))),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use brink_ir::hir::lower;

    fn build(src: &str) -> HirFile {
        let parsed = brink_syntax::parse(src);
        let (hir, _manifest, _diag) = lower(FileId(0), &parsed.tree());
        hir
    }

    fn check_src(src: &str) -> Vec<Diagnostic> {
        let hir = build(src);
        check(&[(FileId(0), &hir)])
    }

    #[test]
    fn clean_int_string_bool_keys_produce_no_diagnostics() {
        let diags = check_src("=== main ===\n~ temp m = #{1: \"a\", \"k\": 2, true: 3}\n-> DONE\n");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn float_key_is_e106() {
        let diags = check_src("=== main ===\n~ temp m = #{3.5: 1}\n-> DONE\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E106);
        assert!(diags[0].message.contains("float"), "{:?}", diags[0].message);
    }

    #[test]
    fn array_literal_key_is_e106() {
        let diags = check_src("=== main ===\n~ temp m = #{#[1, 2]: 1}\n-> DONE\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E106);
        assert!(diags[0].message.contains("array"), "{:?}", diags[0].message);
    }

    #[test]
    fn nested_map_literal_key_is_e106() {
        let diags = check_src("=== main ===\n~ temp m = #{#{1: 2}: 1}\n-> DONE\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E106);
        assert!(diags[0].message.contains("map"), "{:?}", diags[0].message);
    }

    #[test]
    fn struct_literal_key_is_e106() {
        let diags = check_src(
            "STRUCT Point = #{x: int}\n\
             === main ===\n~ temp m = #{Point#{x: 1}: 1}\n-> DONE\n",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E106);
        assert!(
            diags[0].message.contains("struct"),
            "{:?}",
            diags[0].message
        );
    }

    #[test]
    fn list_literal_key_is_e106() {
        let diags = check_src(
            "LIST Colors = red, green, blue\n\
             === main ===\n~ temp m = #{(red): 1}\n-> DONE\n",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E106);
        assert!(diags[0].message.contains("list"), "{:?}", diags[0].message);
    }

    #[test]
    fn divert_target_key_is_e106() {
        let diags =
            check_src("=== main ===\n~ temp m = #{-> other: 1}\n-> DONE\n=== other ===\n-> DONE\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E106);
        assert!(
            diags[0].message.contains("divert target"),
            "{:?}",
            diags[0].message
        );
    }

    #[test]
    fn fn_literal_key_is_e106() {
        let diags = check_src(
            "=== main ===\n~ temp m = #{#fn(score): 1}\n-> DONE\n\
             === score(x) ===\n~ return x\n",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E106);
        assert!(
            diags[0].message.contains("function"),
            "{:?}",
            diags[0].message
        );
    }

    #[test]
    fn dynamic_variable_key_does_not_fire() {
        let diags = check_src("=== main ===\n~ temp k = 1.5\n~ temp m = #{k: 1}\n-> DONE\n");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn dynamic_call_key_does_not_fire() {
        let diags = check_src(
            "=== main ===\n~ temp m = #{score(1): 1}\n-> DONE\n\
             === score(x) ===\n~ return x\n",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn value_position_non_key_domain_is_not_flagged() {
        // Non-key-domain literals are perfectly legal as map *values* — only
        // the key position is domain-restricted.
        let diags = check_src("=== main ===\n~ temp m = #{1: 3.5}\n-> DONE\n");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn var_decl_initializer_map_literal_is_checked() {
        let diags = check_src("VAR m = #{3.5: 1}\n=== main ===\n-> DONE\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E106);
    }

    /// Full-pipeline reachability: `analyze_with_options` (the entry point
    /// `brink-db`'s real diagnostics query drives, per `lib.rs`'s own doc)
    /// wires this through `finish_analysis` -> `per_file_diagnostics` ->
    /// `map_keys::check`, unconditionally under `dialect = Brink` — no
    /// `types`-policy gate (see module doc), so it fires identically under
    /// both `TypePolicy::Gradual` (the default) and `TypePolicy::Strict`.
    #[test]
    fn fires_through_analyze_under_both_gradual_and_strict_type_policy() {
        use crate::{AnalysisOptions, Dialect, TypePolicy, analyze_with_options};

        let src = "=== main ===\n~ temp m = #{3.5: 1}\n-> DONE\n";
        let parsed = brink_syntax::parse(src);
        let (hir, manifest, diag) = lower(FileId(0), &parsed.tree());
        assert!(diag.is_empty(), "{diag:?}");

        for types in [TypePolicy::Gradual, TypePolicy::Strict] {
            let opts = AnalysisOptions {
                dialect: Dialect::Brink,
                types: Some(types),
                ..Default::default()
            };
            let result = analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts);
            assert!(
                result
                    .diagnostics
                    .iter()
                    .any(|d| d.code == DiagnosticCode::E106),
                "types={types:?}: {:?}",
                result.diagnostics
            );
        }
    }

    // ── Duplicate keys (E138, B5 issue #1464) ───────────────────────

    fn dup_src(src: &str) -> Vec<Diagnostic> {
        let hir = build(src);
        check_duplicate_keys(&[(FileId(0), &hir)])
    }

    /// The brink dialect's own `#{…}` spelling reaches the same rule — the
    /// two surfaces share one `MapLiteral`, so one pass serves both.
    #[test]
    fn a_repeated_string_key_is_e138() {
        let diags = dup_src("=== main ===\n~ temp m = #{\"a\": 1, \"a\": 2}\n-> DONE\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E138);
    }

    #[test]
    fn repeats_are_caught_in_every_in_domain_key_kind() {
        for src in [
            "=== main ===\n~ temp m = #{1: \"a\", 1: \"b\"}\n-> DONE\n",
            "=== main ===\n~ temp m = #{true: 1, true: 2}\n-> DONE\n",
            "=== main ===\n~ temp m = #{\"k\": 1, \"k\": 2}\n-> DONE\n",
        ] {
            let diags = dup_src(src);
            assert_eq!(diags.len(), 1, "{src}: {diags:?}");
            assert_eq!(diags[0].code, DiagnosticCode::E138, "{src}");
        }
    }

    /// Three occurrences of one key report twice — one per overwrite, so
    /// the count matches the number of entries actually lost.
    #[test]
    fn each_extra_occurrence_reports_once() {
        let diags = dup_src("=== main ===\n~ temp m = #{1: \"a\", 1: \"b\", 1: \"c\"}\n-> DONE\n");
        assert_eq!(diags.len(), 2, "{diags:?}");
    }

    /// The message names the offending key rather than describing the
    /// rejected last-wins behavior — cascade ruling (A) makes this a
    /// compile error precisely so nothing ever overwrites anything.
    #[test]
    fn the_message_names_the_duplicated_key() {
        let diags = dup_src("=== main ===\n~ temp m = #{1: \"a\", 1: \"b\"}\n-> DONE\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(
            diags[0].message.contains('1'),
            "message should name the key `1`: {:?}",
            diags[0].message
        );

        let diags = dup_src("=== main ===\n~ temp m = #{\"k\": 1, \"k\": 2}\n-> DONE\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(
            diags[0].message.contains("\"k\""),
            "message should name the key `\"k\"`: {:?}",
            diags[0].message
        );
    }

    /// Three occurrences of one key report twice, and the two diagnostics
    /// are distinguishable — a review finding on an earlier revision of
    /// this pass noted the messages were byte-identical duplicates that
    /// named neither the key nor which occurrence collided.
    #[test]
    fn each_occurrence_reports_the_same_named_key() {
        let diags = dup_src("=== main ===\n~ temp m = #{1: \"a\", 1: \"b\", 1: \"c\"}\n-> DONE\n");
        assert_eq!(diags.len(), 2, "{diags:?}");
        for d in &diags {
            assert!(d.message.contains('1'), "{:?}", d.message);
        }
    }

    #[test]
    fn distinct_keys_do_not_fire() {
        let diags =
            dup_src("=== main ===\n~ temp m = #{1: \"a\", 2: \"b\", true: 3, \"1\": 4}\n-> DONE\n");
        assert!(diags.is_empty(), "{diags:?}");
    }

    /// `1` (int) and `"1"` (string) are different `MapKey`s at runtime, so
    /// they must not be reported as a collision here either.
    #[test]
    fn keys_of_different_kinds_never_collide() {
        let diags = dup_src("=== main ===\n~ temp m = #{1: \"a\", \"1\": \"b\"}\n-> DONE\n");
        assert!(diags.is_empty(), "{diags:?}");
    }

    /// "Unknown never disagrees": a key the compiler cannot compare
    /// statically is left to the runtime rather than guessed at.
    #[test]
    fn dynamic_and_interpolated_keys_do_not_fire() {
        let diags = dup_src("=== main ===\n~ temp k = 1\n~ temp m = #{k: 1, k: 2}\n-> DONE\n");
        assert!(diags.is_empty(), "{diags:?}");

        let diags = dup_src(
            "=== main ===\n~ temp a = \"x\"\n~ temp m = #{\"{a}\": 1, \"{a}\": 2}\n-> DONE\n",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    /// An out-of-domain key is `E106`'s mistake, not this pass's — a
    /// second diagnostic for the same entry would just be noise.
    #[test]
    fn out_of_domain_keys_are_left_to_e106() {
        let diags = dup_src("=== main ===\n~ temp m = #{3.5: 1, 3.5: 2}\n-> DONE\n");
        assert!(diags.is_empty(), "{diags:?}");
    }

    /// Nested literals are reached by the same walk `check` uses.
    #[test]
    fn a_repeat_inside_a_nested_literal_is_reported() {
        let diags = dup_src("=== main ===\n~ temp m = #{1: #{\"a\": 1, \"a\": 2}}\n-> DONE\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E138);
    }

    /// Declaration initializers are outside `visit::visit`'s block walk —
    /// the same VAR/CONST gap `check` covers by hand.
    #[test]
    fn a_repeat_in_a_var_initializer_is_reported() {
        let diags = dup_src("VAR m = #{\"a\": 1, \"a\": 2}\n=== main ===\n-> DONE\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E138);
    }
}
