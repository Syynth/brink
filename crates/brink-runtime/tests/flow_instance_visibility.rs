//! M-2b follow-up (#783): the `FlowInstance`-level host surface —
//! `begin_function_eval` / `begin_function_value_eval` /
//! `resume_function_eval` / `choose_path_string(_with_args)` — refuses
//! `#@private` definitions exactly like the `Story`-level surface
//! (`crates/brink-runtime/tests/visibility.rs`, PR #781), for consumers
//! (`bevy-brink`'s per-entity orchestration, `Speculation`) that drive a
//! `FlowInstance` directly and never construct a `Story` at all. Same
//! [`RuntimeError::PrivateAccess`] error, same dev-override behavior — see
//! `docs/modules-spec.md` §4 boundary rules 2 and 3.
#![allow(clippy::unwrap_used, clippy::panic)]

use brink_compiler::{AnalysisOptions, Dialect};
use brink_format::Value;
use brink_runtime::{DotNetRng, FallbackHandler, FlowInstance, RuntimeError, link};

/// An undeclared (default-public) module with a per-definition `#@private`
/// override on the `secret` VAR, the `hidden` knot, and the `hidden_fn`
/// function; `start` and `reveal` stay public. Extends the
/// `tests/visibility.rs` fixture with a private *function* (`hidden_fn`) and
/// a public one (`reveal`) — a knot's `-> DONE` isn't valid function-eval
/// content (`RuntimeError::FunctionYielded`), so `begin_function_eval`
/// needs an actual `function` target to exercise past the privacy check.
const SRC: &str = "#@private\n\
                   VAR secret = 5\n\
                   == start ==\n\
                   Hi\n\
                   -> DONE\n\
                   == hidden ==\n\
                   #@private\n\
                   Secret content\n\
                   -> DONE\n\
                   === function reveal() ===\n\
                   ~ return secret\n\
                   === function hidden_fn() ===\n\
                   #@private\n\
                   ~ return 99\n";

fn compiled() -> brink_format::StoryData {
    let options = AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    };
    brink_compiler::compile_with_options("story.ink", |_p| Ok(SRC.to_owned()), options)
        .unwrap()
        .data
}

/// The compiled `DefinitionId` of the container named `name` — looked up
/// directly from `StoryData`, independent of the runtime's own (crate-private)
/// path resolution, so this test doesn't rely on any API surfaced only for
/// this fix.
fn container_id(data: &brink_format::StoryData, name: &str) -> brink_format::DefinitionId {
    let name_id = data
        .name_table
        .iter()
        .position(|n| n == name)
        .unwrap_or_else(|| panic!("no name table entry for {name:?}"));
    data.containers
        .iter()
        .find(|c| c.name.is_some_and(|n| n.0 as usize == name_id))
        .unwrap_or_else(|| panic!("no container named {name:?}"))
        .id
}

#[test]
fn flow_instance_choose_path_string_refuses_private_def() {
    let data = compiled();
    let (program, _line_tables) = link(&data).unwrap();

    let (mut flow, mut world) = FlowInstance::new_at_root(&program);
    assert!(
        flow.visibility_enforced(),
        "enforcement is on by default, matching Story"
    );

    // Host-directed entry into a private knot: refused, not "not found".
    assert!(matches!(
        flow.choose_path_string(&program, &mut world, "hidden"),
        Err(RuntimeError::PrivateAccess { .. })
    ));

    // A private path with args is refused the same way, before the arity
    // check would otherwise fire.
    assert!(matches!(
        flow.choose_path_string_with_args(&program, &mut world, "hidden", &[]),
        Err(RuntimeError::PrivateAccess { .. })
    ));

    // The public entry point is unaffected.
    flow.choose_path_string(&program, &mut world, "start")
        .unwrap();
}

#[test]
fn flow_instance_begin_function_eval_refuses_private_def() {
    let data = compiled();
    let (program, line_tables) = link(&data).unwrap();

    let (mut flow, mut world) = FlowInstance::new_at_root(&program);
    let container_idx = program.find_address("hidden_fn").unwrap().0;

    let err = flow
        .begin_function_eval::<DotNetRng>(
            &program,
            &line_tables,
            &mut world,
            &FallbackHandler,
            container_idx,
            &[],
            None,
        )
        .unwrap_err();
    assert!(matches!(err, RuntimeError::PrivateAccess { .. }));

    // The refused call never started an evaluation — `is_evaluating_function`
    // stays false, and `resume_function_eval` reports there is nothing to
    // resume rather than silently continuing a private call.
    assert!(!flow.is_evaluating_function());
    let resume_err = flow
        .resume_function_eval::<DotNetRng>(
            &program,
            &line_tables,
            &mut world,
            &FallbackHandler,
            None,
        )
        .unwrap_err();
    assert!(matches!(resume_err, RuntimeError::NotEvaluatingFunction));

    // The public entry point is unaffected.
    let public_idx = program.find_address("reveal").unwrap().0;
    let outcome = flow
        .begin_function_eval::<DotNetRng>(
            &program,
            &line_tables,
            &mut world,
            &FallbackHandler,
            public_idx,
            &[],
            None,
        )
        .unwrap();
    assert!(matches!(
        outcome,
        brink_runtime::FunctionEval::Returned(Value::Int(5))
    ));
}

#[test]
fn flow_instance_begin_function_value_eval_refuses_private_def() {
    let data = compiled();
    let hidden_fn_id = container_id(&data, "hidden_fn");
    let (program, line_tables) = link(&data).unwrap();

    let (mut flow, mut world) = FlowInstance::new_at_root(&program);
    let callee = Value::FnRef(hidden_fn_id);

    let err = flow
        .begin_function_value_eval::<DotNetRng>(
            &program,
            &line_tables,
            &mut world,
            &FallbackHandler,
            &callee,
            &[],
            None,
        )
        .unwrap_err();
    assert!(matches!(err, RuntimeError::PrivateAccess { .. }));
    assert!(!flow.is_evaluating_function());
}

#[test]
fn flow_instance_dev_override_allows_private_access() {
    let data = compiled();
    let (program, line_tables) = link(&data).unwrap();

    let (mut flow, mut world) = FlowInstance::new_at_root(&program);
    flow.set_visibility_enforcement(false);
    assert!(!flow.visibility_enforced());

    // A private knot can now be entered directly — the play-from-here path,
    // reproduced at the `FlowInstance` level.
    flow.choose_path_string(&program, &mut world, "hidden")
        .unwrap();

    // And a private function can now be evaluated out-of-band.
    let (mut flow2, mut world2) = FlowInstance::new_at_root(&program);
    flow2.set_visibility_enforcement(false);
    let container_idx = program.find_address("hidden_fn").unwrap().0;
    let outcome = flow2
        .begin_function_eval::<DotNetRng>(
            &program,
            &line_tables,
            &mut world2,
            &FallbackHandler,
            container_idx,
            &[],
            None,
        )
        .unwrap();
    assert!(matches!(
        outcome,
        brink_runtime::FunctionEval::Returned(Value::Int(99))
    ));
}
