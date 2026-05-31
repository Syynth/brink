//! Integration tests for engine→ink function evaluation
//! ([`FlowInstance::begin_function_eval`] / `resume_function_eval`).
//!
//! These compile small ink stories with the brink compiler, link them, and
//! evaluate functions out-of-band — exercising argument order, return
//! values, output isolation, the `AwaitingExternal` pause/resume cycle, and
//! the guard against functions that try to yield.

use std::cell::RefCell;

use brink_format::Value;
use brink_runtime::{
    ExternalFnHandler, ExternalResult, FastRng, FlowInstance, FunctionEval, Program, StoryStatus,
};

type LineTables = Vec<Vec<brink_format::LineEntry>>;

/// Compile an inline ink source and link it into a `Program` + line tables.
#[expect(clippy::expect_used, reason = "test helper: panic on bad fixtures")]
fn compile(src: &str) -> (Program, LineTables) {
    let out = brink_compiler::compile("t.ink", |p| {
        if p == "t.ink" {
            Ok(src.to_string())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no such include",
            ))
        }
    })
    .expect("compile");
    brink_runtime::link(&out.data).expect("link")
}

/// Handler that resolves named externals from a closure and records calls.
struct ClosureHandler<F: Fn(&str, &[Value]) -> ExternalResult> {
    f: F,
    calls: RefCell<Vec<String>>,
}

impl<F: Fn(&str, &[Value]) -> ExternalResult> ClosureHandler<F> {
    fn new(f: F) -> Self {
        Self {
            f,
            calls: RefCell::new(Vec::new()),
        }
    }
}

impl<F: Fn(&str, &[Value]) -> ExternalResult> ExternalFnHandler for ClosureHandler<F> {
    fn call(&self, name: &str, args: &[Value]) -> ExternalResult {
        self.calls.borrow_mut().push(name.to_string());
        (self.f)(name, args)
    }
}

fn fallback() -> ClosureHandler<impl Fn(&str, &[Value]) -> ExternalResult> {
    ClosureHandler::new(|_, _| ExternalResult::Fallback)
}

#[expect(clippy::expect_used, reason = "test helper: panic on bad fixtures")]
fn func_idx(program: &Program, name: &str) -> u32 {
    program.find_address(name).expect("function not found").0
}

/// Two-arg function with an order-sensitive body: `sub(10, 3)` must be 7,
/// not -7 — proving arguments are passed in declaration order.
#[test]
fn arg_order_and_explicit_return() {
    let (program, tables) = compile("-> END\n=== function sub(a, b) ===\n~ return a - b\n");
    let (mut flow, mut ctx) = FlowInstance::new_at_root(&program);
    let idx = func_idx(&program, "sub");

    let result = flow
        .begin_function_eval::<FastRng>(
            &program,
            &tables,
            &mut ctx,
            &fallback(),
            idx,
            &[Value::Int(10), Value::Int(3)],
            None,
        )
        .unwrap();
    assert!(
        matches!(result, FunctionEval::Returned(Value::Int(7))),
        "sub(10, 3) should be 7 (forward arg order), got {result:?}"
    );
    assert!(!flow.is_evaluating_function());
}

/// A function with no explicit `~ return` yields Null.
#[test]
fn void_function_returns_null() {
    let (program, tables) = compile("-> END\n=== function noop() ===\n~ temp x = 1\n");
    let (mut flow, mut ctx) = FlowInstance::new_at_root(&program);
    let idx = func_idx(&program, "noop");

    let result = flow
        .begin_function_eval::<FastRng>(&program, &tables, &mut ctx, &fallback(), idx, &[], None)
        .unwrap();
    assert!(matches!(result, FunctionEval::Returned(Value::Null)));
}

/// Evaluating a function must not pollute the player-visible transcript,
/// and must not disturb the main story's position.
#[test]
fn eval_does_not_touch_transcript_or_pending_choice() {
    let (program, tables) =
        compile("First line.\n* [Go] -> END\n=== function tally(n) ===\n~ return n * 2\n");
    let (mut flow, mut ctx) = FlowInstance::new_at_root(&program);

    // Advance the main story to its choice point.
    loop {
        let line = flow
            .step_single_line::<FastRng>(&program, &tables, &mut ctx, &fallback(), None)
            .unwrap();
        if line.is_terminal() {
            break;
        }
    }
    assert_eq!(flow.status(), StoryStatus::WaitingForChoice);
    let transcript_before = flow.transcript_len();

    // Evaluate a function out-of-band while a choice is pending.
    let idx = func_idx(&program, "tally");
    let result = flow
        .begin_function_eval::<FastRng>(
            &program,
            &tables,
            &mut ctx,
            &fallback(),
            idx,
            &[Value::Int(21)],
            None,
        )
        .unwrap();
    assert!(matches!(result, FunctionEval::Returned(Value::Int(42))));

    // The eval disturbed neither the transcript nor the pending choice:
    // the story is still waiting for a choice, and we can still make it.
    assert_eq!(flow.transcript_len(), transcript_before);
    assert_eq!(flow.status(), StoryStatus::WaitingForChoice);
    flow.choose(&mut ctx, 0).unwrap();
}

/// A function calling an external resolved synchronously by the handler.
#[test]
fn function_calls_external_resolved_inline() {
    let (program, tables) = compile(
        "EXTERNAL bonus(base)\n-> END\n=== function score(base) ===\n~ return base + bonus(base)\n",
    );
    let (mut flow, mut ctx) = FlowInstance::new_at_root(&program);
    let idx = func_idx(&program, "score");

    // bonus(x) := x * 10, resolved inline.
    let handler = ClosureHandler::new(|name, args| {
        assert_eq!(name, "bonus");
        let base = args[0].as_int().unwrap();
        ExternalResult::Resolved(Value::Int(base * 10))
    });

    let result = flow
        .begin_function_eval::<FastRng>(
            &program,
            &tables,
            &mut ctx,
            &handler,
            idx,
            &[Value::Int(5)],
            None,
        )
        .unwrap();
    // 5 + (5 * 10) = 55
    assert!(matches!(result, FunctionEval::Returned(Value::Int(55))));
    assert_eq!(handler.calls.borrow().as_slice(), ["bonus"]);
}

/// The pause/resume cycle: a handler returns Pending; the caller resolves
/// the external out-of-band and resumes to completion.
#[test]
fn function_external_pauses_then_resumes() {
    let (program, tables) = compile(
        "EXTERNAL world_value()\n-> END\n=== function query() ===\n~ return world_value() + 1\n",
    );
    let (mut flow, mut ctx) = FlowInstance::new_at_root(&program);
    let idx = func_idx(&program, "query");

    // Handler always defers world_value.
    let handler = ClosureHandler::new(|name, _| {
        assert_eq!(name, "world_value");
        ExternalResult::Pending
    });

    let outcome = flow
        .begin_function_eval::<FastRng>(&program, &tables, &mut ctx, &handler, idx, &[], None)
        .unwrap();
    assert!(matches!(outcome, FunctionEval::AwaitingExternal));
    assert!(flow.is_evaluating_function());
    assert_eq!(flow.pending_external_name(&program), Some("world_value"));
    assert!(flow.pending_external_args().is_empty());

    // The engine resolves the external (simulating a world query → 41).
    flow.resolve_external(Value::Int(41));

    // Resume to completion. We can use a fallback handler now — no further
    // external is hit.
    let outcome = flow
        .resume_function_eval::<FastRng>(&program, &tables, &mut ctx, &fallback(), None)
        .unwrap();
    assert!(matches!(outcome, FunctionEval::Returned(Value::Int(42))));
    assert!(!flow.is_evaluating_function());
}

/// A function that tries to present choices is rejected (functions can't
/// yield), and the eval state is cleaned up.
#[test]
fn function_presenting_choices_errors() {
    let (program, tables) =
        compile("-> END\n=== function bad() ===\nsome text\n* [choice] -> END\n");
    let (mut flow, mut ctx) = FlowInstance::new_at_root(&program);
    let idx = func_idx(&program, "bad");

    let result = flow.begin_function_eval::<FastRng>(
        &program,
        &tables,
        &mut ctx,
        &fallback(),
        idx,
        &[],
        None,
    );
    assert!(result.is_err(), "expected FunctionYielded, got {result:?}");
    assert!(
        !flow.is_evaluating_function(),
        "eval state cleaned up on error"
    );
}

/// `resume_function_eval` without an in-progress eval is an error.
#[test]
fn resume_without_begin_errors() {
    let (program, tables) = compile("-> END\n");
    let (mut flow, mut ctx) = FlowInstance::new_at_root(&program);
    let result =
        flow.resume_function_eval::<FastRng>(&program, &tables, &mut ctx, &fallback(), None);
    assert!(result.is_err());
}
