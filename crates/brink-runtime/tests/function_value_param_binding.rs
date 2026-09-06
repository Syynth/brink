//! `.inkb` v10 (`docs/compiler-spec.md` §"Parameter binding"): the
//! function-**value** entry site binds its arguments.
//!
//! `begin_function_value_eval` is the fifteenth entry site, and the one the
//! first cut of v10 missed. It is easy to miss by reading: it does not
//! resolve a container and enter it the way the other host-side entry
//! points do — it goes through `vm::prepare_fn_value_call` to assemble the
//! callee's *full* argument row (a `Closure`'s bound prefix, then the
//! host's supplied arguments), then pushes that row and enters. Before v10
//! the callee's own `DeclareTemp` prologue consumed it; with the prologue
//! gone, the push had no matching bind and the callee read an unbound slot.
//!
//! Both callee shapes are covered, because they differ in exactly the part
//! `prepare_fn_value_call` synthesises: a bare `FnRef` whose whole argument
//! row comes from the host, and a `bind`-curried `Closure` whose row is
//! part captured and part supplied — an off-by-the-bound-prefix bind is
//! invisible in the first and wrong in the second.
#![expect(clippy::expect_used, reason = "test harness")]

use brink_compiler::{AnalysisOptions, Dialect};
use brink_format::StoryData;
use brink_format::Value;
use brink_runtime::{DotNetRng, FallbackHandler, FlowInstance, FunctionEval, link};

const SRC: &str = "-> END\n\
                   === function make_doubler() ===\n\
                   ~ return #fn(double)\n\
                   === function make_adder() ===\n\
                   ~ return bind(#fn(add), 10)\n\
                   === function double(x: int): int ===\n\
                   ~ return x + x\n\
                   === function add(a: int, b: int): int ===\n\
                   ~ return a + b\n";

fn compiled() -> StoryData {
    let options = AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    };
    let out = brink_compiler::compile_with_options("story.ink", |_p| Ok(SRC.to_owned()), options);
    assert!(out.is_ok(), "compile failed: {out:?}");
    out.expect("just asserted above").data
}

/// Evaluate `name()` to get an opaque function value, then re-invoke that
/// value with `args` — the two-step shape a host callback surface uses.
fn reinvoke(name: &str, args: &[Value]) -> Value {
    let data = compiled();
    let linked = link(&data);
    assert!(linked.is_ok(), "link failed: {:?}", linked.as_ref().err());
    let (program, line_tables) = linked.expect("just asserted above");

    let (mut flow, mut world) = FlowInstance::new_at_root(&program);
    let found = program.find_address(name);
    assert!(found.is_some(), "no address for {name:?}");
    let (idx, _) = found.expect("just asserted above");

    let made = flow.begin_function_eval::<DotNetRng>(
        &program,
        &line_tables,
        &mut world,
        &FallbackHandler,
        idx,
        &[],
        None,
    );
    assert!(made.is_ok(), "{name} eval failed: {made:?}");
    let made = made.expect("just asserted above");
    assert!(
        matches!(made, FunctionEval::Returned(_)),
        "{name} must return a value, not await an external: {made:?}"
    );
    let FunctionEval::Returned(callee) = made else {
        unreachable!("just asserted above")
    };

    let invocation = flow.begin_function_value_eval::<DotNetRng>(
        &program,
        &line_tables,
        &mut world,
        &FallbackHandler,
        &callee,
        args,
        None,
    );
    assert!(invocation.is_ok(), "re-invoke failed: {invocation:?}");
    let outcome = invocation.expect("just asserted above");
    assert!(
        matches!(outcome, FunctionEval::Returned(_)),
        "the callee must return a value, not await an external: {outcome:?}"
    );
    let FunctionEval::Returned(v) = outcome else {
        unreachable!("just asserted above")
    };
    v
}

#[test]
fn a_bare_function_value_binds_the_arguments_the_host_supplies() {
    assert_eq!(
        reinvoke("make_doubler", &[Value::Int(21)]).as_int(),
        Some(42)
    );
}

#[test]
fn a_curried_closure_binds_its_captured_prefix_and_the_supplied_rest() {
    // `add(10, 5)`: slot 0 comes from the closure's capture, slot 1 from the
    // host. Binding only the supplied argument, or binding it into slot 0,
    // both land somewhere other than 15.
    assert_eq!(reinvoke("make_adder", &[Value::Int(5)]).as_int(), Some(15));
}
