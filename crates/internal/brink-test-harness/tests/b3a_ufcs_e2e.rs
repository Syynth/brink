//! B3a end-to-end: UFCS resolution on the native surface, through the real
//! `.brink` pipeline (issue #1482; D1–D5 RULED 2026-07-26 —
//! `docs/decision-log.md` "UFCS resolution pass designed"). Issue #1506
//! wired LIR lowering to actually consume the verdict table instead of
//! refusing every resolved site with `E144`; issue #1462 landed **D5**, the
//! auto-ref rider — a `ref` first parameter now mutates its receiver here
//! instead of refusing the compile.
//!
//! The unit tests in `brink-analyzer/tests/ufcs_resolution.rs` prove the
//! *pass* (verdicts + the four diagnostic outcomes). This file proves the
//! **user path**: a real `.brink` source reaching
//! `brink_test_harness::corpus::explore_from_brink_native` — the same
//! honest minimal native pipeline `b5_construction_e2e.rs` and
//! `first_light.rs` run — gets each ruled outcome as either a compile
//! refusal with the *right* code, or — for the two verdicts LIR lowering
//! now emits real code for (`FreeFnDesugar`, `PreludeDesugar`) — actually
//! compiles and plays, byte-identical to the equivalent explicit free call.
//!
//! `FieldCall` (field access wins over a free function of the same name) is
//! exercised only at the LIR-lowering unit level
//! (`brink-ir/tests/ufcs_field_call.rs`): NG-E (issue #1505) widened
//! `brink-syntax-native`'s `struct_field` grammar to spell a function-typed
//! field (`greet: fn(int): int`) for real, so that unit test now drives a
//! real `.brink` fixture through parse → lower → analyze → resolve →
//! LIR-lower. What's still missing for a fixture *here* — one that actually
//! plays through the VM like every other test in this file — is a
//! first-class function *value* on the native surface (`#fn(target, args…)`,
//! T1c §2, is `brink-syntax`-only today); a struct literal can declare a
//! `fn`-typed field but has nothing real to assign it. Tracked as a
//! follow-up on #1505 rather than bundled here.
//!
//! Before this pass every UFCS-shaped call was refused indiscriminately as
//! `E025` ("unresolved variable reference"); the bar the pass (and now its
//! LIR consumer) has to clear is that each site fails — or succeeds — *for
//! its own ruled reason*.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_test_harness::ExploreConfig;
use brink_test_harness::corpus::explore_from_brink_native;

/// Compile-and-play, expecting refusal; returns the rendered error.
fn refuse(src: &str) -> String {
    match explore_from_brink_native(src, &ExploreConfig::default()) {
        Ok(_) => panic!("fixture must be refused, but it compiled"),
        Err(e) => e,
    }
}

/// Compile-and-play, expecting success; returns the concatenated text of
/// the single episode.
fn play(src: &str) -> String {
    let episodes = explore_from_brink_native(src, &ExploreConfig::default())
        .unwrap_or_else(|e| panic!("native fixture must compile and play: {e}"));
    episodes
        .first()
        .expect("one episode")
        .steps
        .iter()
        .map(|s| s.text.clone())
        .collect()
}

/// A `.brink` file whose `shout()` body is `BODY`.
fn program(decls: &str, body: &str) -> String {
    format!(
        "\
struct Guest {{
  name: string
}}

{decls}
flow main() {{
  {{shout()}}
}}

fn shout() {{
  let g = Guest {{ name: \"ada\" }};
  {body}
}}
"
    )
}

const GREET: &str = "\
fn greet(g, loudness) {
  return loudness;
}
";

/// Issue #1506: a resolvable method call whose verdict is `FreeFnDesugar`
/// now compiles and plays for real — `g.greet(3)` desugars to
/// `greet(g, 3)` at LIR lowering (`lower_ufcs_call`/
/// `lower_ufcs_desugared_call`), producing the identical output the
/// explicit free-call spelling does (see
/// `the_explicit_free_call_spelling_still_plays` below). Before #1506 this
/// same source reached `E144` at LIR lowering; before #1482 it was `E025`.
#[test]
fn a_free_fn_desugar_compiles_and_plays_via_ufcs_syntax() {
    let out = play(&program(GREET, "return g.greet(3);"));
    assert_eq!(out, "3\n");
}

/// Issue #1506: `PreludeDesugar` also lowers for real — `m.len()` desugars
/// through the same T1b/NS stdlib dispatch the bare `len(m)` call in
/// `b5_construction_e2e.rs`'s `map_construction_plays` reaches, over the
/// same `Map { … }` construction literal, so the two are directly
/// comparable: identical output, method-call spelling.
#[test]
fn a_prelude_desugar_compiles_and_plays_via_ufcs_syntax() {
    let out = play(
        "\
fn count() {
  let m = Map { \"a\": 1, \"b\": 2, \"c\": 3 };
  return m.len();
}

flow main() {
  Size is {count()}.
}
",
    );
    assert_eq!(out, "Size is 3.\n");
}

/// Review finding on issue #1506: `PreludeDesugar` must also cover the
/// statement-only collection mutators (`push`/`insert`/`remove`/`clear`/
/// `sort`/`sort_by`/`heap_push`) — these never reach
/// `lower_ufcs_prelude_desugar`'s stdlib dispatch, which unconditionally
/// refuses every mutator name with `E056` ("used in expression position").
/// `try_lower_mutator_stmt` (the same statement-position recognizer a bare
/// `insert(m, k, v)` call reaches) must instead recognize the UFCS shape
/// and splice the receiver in as the mutator's first argument. Before the
/// fix, `m.insert("c", 3)` spuriously hit `E056` even though it's used as a
/// statement, not an expression.
#[test]
fn a_prelude_desugar_mutator_compiles_and_plays_via_ufcs_syntax() {
    let out = play(
        "\
fn count() {
  let m = Map { \"a\": 1, \"b\": 2 };
  m.insert(\"c\", 3);
  return m.len();
}

flow main() {
  Size is {count()}.
}
",
    );
    assert_eq!(out, "Size is 3.\n");
}

/// D1: a matching but non-callable field is a hard error, and it never
/// falls through to the free `greet` that is also in scope.
#[test]
fn a_non_callable_field_refuses_the_compile_with_e140() {
    let src = format!(
        "\
struct Guest {{
  greet: string
}}

{GREET}
flow main() {{
  {{shout()}}
}}

fn shout() {{
  let g = Guest {{ greet: \"hi\" }};
  return g.greet(3);
}}
"
    );
    let err = refuse(&src);
    assert!(err.contains("E140"), "expected E140, got: {err}");
    assert!(
        !err.contains("E141"),
        "field access wins outright — no second attempt is reported: {err}"
    );
}

/// Step 4: neither a field nor a free function.
#[test]
fn an_unresolvable_method_refuses_the_compile_with_e141() {
    let err = refuse(&program("", "return g.nope(3);"));
    assert!(err.contains("E141"), "expected E141, got: {err}");
}

/// D3: an unknown receiver type demands an annotation instead of deferring.
#[test]
fn an_unknown_receiver_refuses_the_compile_with_e142() {
    let src = format!(
        "\
{GREET}
flow main() {{
  {{shout(1)}}
}}

fn shout(guest) {{
  return guest.greet(3);
}}
"
    );
    let err = refuse(&src);
    assert!(err.contains("E142"), "expected E142, got: {err}");
}

/// D5 (issue #1462): a free function whose first parameter is declared
/// `ref` **auto-refs** its receiver. This is the whole point of the ruling —
/// the mutation has to be visible in the *caller* afterwards, so the
/// assertion is on the receiver's value after the call, not on the call's
/// own result. Before #1462 this exact source was refused with `E143`.
#[test]
fn auto_ref_mutates_a_local_receiver_end_to_end() {
    let out = play(
        "\
fn bump(ref n, amount) {
  n = n + amount;
}

fn total() {
  let g = 1;
  g.bump(5);
  return g;
}

flow main() {
  Total is {total()}.
}
",
    );
    assert_eq!(out, "Total is 6.\n");
}

/// The other durable receiver shape: a global `VAR` receiver auto-refs to
/// the global cell itself (`RefGlobal`), and the write survives the call.
#[test]
fn auto_ref_mutates_a_global_var_receiver_end_to_end() {
    let out = play(
        "\
var hp: int = 1

fn bump(ref n, amount) {
  n = n + amount;
}

fn total() {
  hp.bump(5);
  return hp;
}

flow main() {
  HP is {total()}.
}
",
    );
    assert_eq!(out, "HP is 6.\n");
}

/// Review finding on issue #1462: the auto-ref desugar's central claim is
/// that the receiver "binds exactly as an author-written `ref` argument" —
/// this mirrors `auto_ref_mutates_a_local_receiver_end_to_end` with the
/// explicit free-call spelling (`bump(g, 5)`, no call-site `ref` keyword —
/// the native surface doesn't have one) and asserts byte-identical output,
/// pinning the equivalence at the user surface instead of only asserting it
/// in prose.
#[test]
fn the_explicit_free_call_spelling_matches_auto_ref_for_a_local_receiver() {
    let out = play(
        "\
fn bump(ref n, amount) {
  n = n + amount;
}

fn total() {
  let g = 1;
  bump(g, 5);
  return g;
}

flow main() {
  Total is {total()}.
}
",
    );
    assert_eq!(out, "Total is 6.\n");
}

/// The global-`VAR`-receiver mirror of the test above.
#[test]
fn the_explicit_free_call_spelling_matches_auto_ref_for_a_global_var_receiver() {
    let out = play(
        "\
var hp: int = 1

fn bump(ref n, amount) {
  n = n + amount;
}

fn total() {
  bump(hp, 5);
  return hp;
}

flow main() {
  HP is {total()}.
}
",
    );
    assert_eq!(out, "HP is 6.\n");
}

/// D5's other half, stated as its own test: a **non-`ref`** first parameter
/// is untouched by auto-ref — plain by-value desugar, and *no* lvalue
/// requirement on the receiver, so the very `const` receiver the auto-ref
/// case refuses below compiles and plays here.
#[test]
fn a_non_ref_first_param_puts_no_lvalue_requirement_on_the_receiver() {
    let out = play(
        "\
const START: int = 1

fn plus(n, amount) {
  return n + amount;
}

fn total() {
  return START.plus(5);
}

flow main() {
  Total is {total()}.
}
",
    );
    assert_eq!(out, "Total is 6.\n");
}

/// D5's refusal half: an immutable receiver under a `ref` first parameter is
/// a compile error ("cannot mutate a temporary" — the ruled posture), never a
/// silent by-value desugar that would drop the mutation. A `const` is the
/// reachable spelling of that family on today's native surface.
#[test]
fn auto_ref_onto_a_const_receiver_refuses_the_compile() {
    let err = refuse(
        "\
const START: int = 1

fn bump(ref n, amount) {
  n = n + amount;
}

fn total() {
  START.bump(5);
  return START;
}

flow main() {
  {total()}
}
",
    );
    assert!(err.contains("E143"), "expected E143, got: {err}");
    assert!(err.contains("cannot mutate"), "ruled wording: {err}");
}

/// **RULED 2026-07-27** (issue #1531, `docs/decision-log.md`): the exact
/// case #1462's conflict left unresolved — `let g = Guest { … };
/// g.hp.heal(5)` — is legal. A frame-local cell is a valid projection root,
/// and needs no effect row (the mutation is unobservable outside the
/// frame). LIR lowering never reuses `RefProjection` for this (that
/// variant's root stays durable-only); it splices a read/call/write-back
/// RMW sequence instead (`brink_ir::lir::lower::blocks::
/// try_lower_frame_local_auto_ref_stmt`), the same discipline `g.hp = 5`
/// already rides. Before this ruling this exact source was refused with
/// `E143`, whose "bind the receiver to a durable cell" advice was a dead
/// end until #1530 (below) — struct-typed durable globals were unspellable
/// until then.
#[test]
fn auto_ref_mutates_a_projection_off_a_frame_local_end_to_end() {
    let out = play(
        "\
struct Guest {
  hp: int
}

fn heal(ref h, amount) {
  h = h + amount;
}

fn total() {
  let g = Guest { hp: 1 };
  g.hp.heal(5);
  return g.hp;
}

flow main() {
  HP is {total()}.
}
",
    );
    assert_eq!(out, "HP is 6.\n");
}

/// The ruling's own boundary: LIR's RMW expansion for a frame-local
/// projection is single-field-level only (the same limit plain assignment
/// draws — `try_lower_field_assignment`'s `E074`), so a receiver two fields
/// deep off a frame-local still refuses at the analyzer gate rather than
/// silently mis-desugaring.
#[test]
fn auto_ref_onto_a_two_field_deep_frame_local_projection_still_refuses_the_compile() {
    let err = refuse(
        "\
struct Hp {
  current: int
}

struct Guest {
  hp: Hp
}

fn heal(ref h, amount) {
  h = h + amount;
}

fn total() {
  let g = Guest { hp: Hp { current: 1 } };
  g.hp.current.heal(5);
  return g.hp.current;
}

flow main() {
  {total()}
}
",
    );
    assert!(err.contains("E143"), "expected E143, got: {err}");
    assert!(err.contains("cannot mutate"), "ruled wording: {err}");
}

/// LIR lowering's other boundary on the frame-local ruling: the RMW
/// expansion is statement-shaped only (it needs a read, the call, and a
/// write-back as three separate statements), so a single-field-deep
/// frame-local receiver used from *expression* position — nested inside a
/// `let` initializer rather than standing alone as its own statement —
/// still refuses (`expr::lower_ref_projection_arg`'s own frame-local
/// guard) instead of silently building a `RefProjection` whose root has no
/// runtime representation.
#[test]
fn auto_ref_onto_a_frame_local_projection_in_expression_position_still_refuses_the_compile() {
    let err = refuse(
        "\
struct Guest {
  hp: int
}

fn heal(ref h, amount) {
  h = h + amount;
}

fn total() {
  let g = Guest { hp: 1 };
  let unused = g.hp.heal(5);
  return g.hp;
}

flow main() {
  {total()}
}
",
    );
    assert!(err.contains("E143"), "expected E143, got: {err}");
}

/// The **positive** half of the durable-root rule, and the one arm of D5
/// this file could not reach until issue #1530: a projection whose root
/// *is* a durable cell (`docs/t1e-spec.md` §2) auto-refs through the
/// projection and the write survives the call.
///
/// Before #1530 a struct-typed global was unspellable — `var g: Guest =
/// Guest { hp: 1 }` was refused at LIR lowering (`E075`: `lir::ConstValue`
/// had no record-carrying variant), so the only receivers this file could
/// build were frame-locals, and the projection arm was covered solely by
/// `brink-ir/tests/ufcs_auto_ref.rs`'s hand-assembled verdict table. That
/// also made `E143`'s own remediation advice — bind the receiver to a
/// durable cell — point at something the language could not express.
///
/// Note the boundary: this is the durable-**global** root; the frame-local
/// arm was the #1531 conflict this file's other tests now cover.
#[test]
fn auto_ref_mutates_a_projection_off_a_durable_global_end_to_end() {
    let out = play(
        "\
struct Guest {
  hp: int
}

var g: Guest = Guest { hp: 1 }

fn heal(ref h, amount) {
  h = h + amount;
}

fn total() {
  g.hp.heal(5);
  return g.hp;
}

flow main() {
  HP is {total()}.
}
",
    );
    assert_eq!(out, "HP is 6.\n");
}

/// The same durable-global receiver with the type annotation *omitted* —
/// #1530 was reported for both `var g: Guest = Guest { … }` and
/// `var g = Guest { … }`, so both spellings get a fixture. The declared
/// default's own literal carries the shape either way.
#[test]
fn auto_ref_mutates_a_projection_off_an_unannotated_global_end_to_end() {
    let out = play(
        "\
struct Guest {
  hp: int
}

var g = Guest { hp: 1 }

fn heal(ref h, amount) {
  h = h + amount;
}

fn total() {
  g.hp.heal(5);
  return g.hp;
}

flow main() {
  HP is {total()}.
}
",
    );
    assert_eq!(out, "HP is 6.\n");
}

/// The declaration default is a real baked record, not just a shape the
/// projection can be lowered against: reading a field before anything
/// writes to it yields the value the literal declared.
#[test]
fn a_struct_typed_global_reads_its_declared_default_before_any_write() {
    let out = play(
        "\
struct Guest {
  name: string
  hp: int
}

var g = Guest { name: \"ada\", hp: 7 }

fn describe() {
  return g.hp;
}

flow main() {
  HP is {describe()}.
}
",
    );
    assert_eq!(out, "HP is 7.\n");
}

/// The malformed half of #1530's narrowed `E075`: a declaration default
/// that omits a declared field has no `RecordNew` runtime step left to
/// fault at (the value is baked into `StoryData`), so it stays a compile
/// error rather than baking a half-built record.
#[test]
fn a_struct_typed_global_missing_a_declared_field_refuses_the_compile() {
    let err = refuse(
        "\
struct Guest {
  name: string
  hp: int
}

var g = Guest { hp: 7 }

flow main() {
  {g.hp}
}
",
    );
    assert!(err.contains("E075"), "expected E075, got: {err}");
}

/// The explicit free-call spelling keeps working end to end — the sugar is
/// additive, and this is the workaround every refusal above points authors
/// at.
#[test]
fn the_explicit_free_call_spelling_still_plays() {
    let src = format!(
        "\
struct Guest {{
  name: string
}}

{GREET}
flow main() {{
  Loudness is {{shout()}}.
}}

fn shout() {{
  let g = Guest {{ name: \"ada\" }};
  return greet(g, 3);
}}
"
    );
    let episodes = explore_from_brink_native(&src, &ExploreConfig::default())
        .unwrap_or_else(|e| panic!("native fixture must compile and play: {e}"));
    let out: String = episodes
        .first()
        .expect("one episode")
        .steps
        .iter()
        .map(|s| s.text.clone())
        .collect();
    assert_eq!(out, "Loudness is 3.\n");
}
