//! NS-A8 numeric-tower opcode implementations (`docs/tower-mini-spec.md`,
//! issue #1114; ruled shape: `docs/stdlib-spec.md` §2b via
//! `docs/stdlib-inventory.md` §2).
//!
//! One VM opcode (`Opcode::Tower(TowerOp)`) carries the whole family:
//! constructors (`vec2(x, y)` … `mat4(c0, c1, c2, c3)`), `dot`/`cross`, and
//! the tower-wide `min`/`max`/`clamp`/`lerp`. All operations are **pure**
//! (read nothing, write nothing, emit nothing); the only fault path is a
//! wrong-operand-type — a malformed *question* per the ruled
//! fault-vs-absence doctrine, surfaced as
//! [`RuntimeError::StdlibWrongType`] with the author-facing verb name.
//!
//! Semantics are glam's, wholesale (T1/T3): glam is the in-memory compute
//! type — column-major matrices, `(x, y, z, w)` quats, `Quat::lerp`'s
//! normalizing interpolation. The `+`/`-`/`*` operator family does NOT live
//! here — it rides the frozen arithmetic opcodes via
//! `value_ops::binary_op`'s tower arms. Equality/ordering doctrine (T4)
//! also lives there and in `collection_ops::total_order_cmp` (tower kinds
//! are NOT orderable — `NotOrderable` at every ordering verb).

use brink_format::{TowerOp, Value};
use glam::{Mat2, Mat3, Mat4, Quat, Vec2, Vec3, Vec4};

use crate::collection_ops::type_name;
use crate::error::RuntimeError;
use crate::story::Flow;

/// Execute one `Opcode::Tower(op)` instruction against the flow's value
/// stack. Operands were pushed left-to-right, so they pop in reverse.
pub(crate) fn tower_op(flow: &mut Flow, op: TowerOp) -> Result<(), RuntimeError> {
    let result = match op {
        TowerOp::MakeVec2 => {
            let [x, y] = pop_lanes(flow, "vec2")?;
            Value::Vec2(Vec2::new(x, y))
        }
        TowerOp::MakeVec3 => {
            let [x, y, z] = pop_lanes(flow, "vec3")?;
            Value::Vec3(Vec3::new(x, y, z))
        }
        TowerOp::MakeVec4 => {
            let [x, y, z, w] = pop_lanes(flow, "vec4")?;
            Value::Vec4(Vec4::new(x, y, z, w))
        }
        TowerOp::MakeQuat => {
            let [x, y, z, w] = pop_lanes(flow, "quat")?;
            Value::Quat(Quat::from_xyzw(x, y, z, w))
        }
        TowerOp::MakeMat2 => {
            let [c0, c1] = pop_cols(flow, "mat2", as_vec2)?;
            Value::Mat2(Mat2::from_cols(c0, c1))
        }
        TowerOp::MakeMat3 => {
            let [c0, c1, c2] = pop_cols(flow, "mat3", as_vec3)?;
            Value::Mat3(Mat3::from_cols(c0, c1, c2))
        }
        TowerOp::MakeMat4 => {
            let [c0, c1, c2, c3] = pop_cols(flow, "mat4", as_vec4)?;
            Value::Mat4(Mat4::from_cols(c0, c1, c2, c3))
        }
        // `dot(a, b)` — same-size vectors → float (§2b: vec2/3/4).
        TowerOp::Dot => {
            let b = flow.pop_value()?;
            let a = flow.pop_value()?;
            match (&a, &b) {
                (Value::Vec2(a), Value::Vec2(b)) => Value::Float(a.dot(*b)),
                (Value::Vec3(a), Value::Vec3(b)) => Value::Float(a.dot(*b)),
                (Value::Vec4(a), Value::Vec4(b)) => Value::Float(a.dot(*b)),
                _ => return Err(wrong_type("dot", "two same-size vectors", &a, &b)),
            }
        }
        // `cross(a, b)` — vec3 only (§2b).
        TowerOp::Cross => {
            let b = flow.pop_value()?;
            let a = flow.pop_value()?;
            match (&a, &b) {
                (Value::Vec3(a), Value::Vec3(b)) => Value::Vec3(a.cross(*b)),
                _ => return Err(wrong_type("cross", "two vec3 values", &a, &b)),
            }
        }
        // Componentwise two-vector forms (glam `min`/`max`).
        TowerOp::Min => componentwise_pair(flow, "min", Vec2::min, Vec3::min, Vec4::min)?,
        TowerOp::Max => componentwise_pair(flow, "max", Vec2::max, Vec3::max, Vec4::max)?,
        // `clamp(x, lo, hi)` — three same-kind vectors, componentwise.
        TowerOp::Clamp => {
            let hi = flow.pop_value()?;
            let lo = flow.pop_value()?;
            let x = flow.pop_value()?;
            match (&x, &lo, &hi) {
                (Value::Vec2(x), Value::Vec2(lo), Value::Vec2(hi)) => {
                    Value::Vec2(x.clamp(*lo, *hi))
                }
                (Value::Vec3(x), Value::Vec3(lo), Value::Vec3(hi)) => {
                    Value::Vec3(x.clamp(*lo, *hi))
                }
                (Value::Vec4(x), Value::Vec4(lo), Value::Vec4(hi)) => {
                    Value::Vec4(x.clamp(*lo, *hi))
                }
                _ => {
                    return Err(RuntimeError::StdlibWrongType {
                        verb: "clamp",
                        expected: "three same-size vectors",
                        found: type_name(&x),
                    });
                }
            }
        }
        // `lerp(a, b, t)` — scalar `t` (glam's signature): vectors
        // componentwise, quats via glam's normalizing `Quat::lerp`.
        TowerOp::Lerp => {
            let t = flow.pop_value()?;
            let b = flow.pop_value()?;
            let a = flow.pop_value()?;
            let Some(t) = scalar(&t) else {
                return Err(RuntimeError::StdlibWrongType {
                    verb: "lerp",
                    expected: "a numeric `t`",
                    found: type_name(&t),
                });
            };
            match (&a, &b) {
                (Value::Vec2(a), Value::Vec2(b)) => Value::Vec2(a.lerp(*b, t)),
                (Value::Vec3(a), Value::Vec3(b)) => Value::Vec3(a.lerp(*b, t)),
                (Value::Vec4(a), Value::Vec4(b)) => Value::Vec4(a.lerp(*b, t)),
                (Value::Quat(a), Value::Quat(b)) => Value::Quat(a.lerp(*b, t)),
                _ => {
                    return Err(wrong_type("lerp", "two same-kind vectors or quats", &a, &b));
                }
            }
        }
    };
    flow.value_stack.push(result);
    Ok(())
}

/// The named component of a tower value, if `name` is one of its glam
/// component names — vectors/quats expose `x`/`y`/`z`/`w` lanes as `Float`,
/// matrices expose their `x_axis`/`y_axis`/`z_axis`/`w_axis` columns as
/// vectors. Pure accessors (T3: glam's field vocabulary, wholesale); the
/// `record_ops::record_get_dyn` field-access path consults this so `v.x`
/// works through the existing dynamic field opcode. `None` = not a tower
/// value or not a component this kind declares.
pub(crate) fn tower_component(v: &Value, name: &str) -> Option<Value> {
    match (v, name) {
        (Value::Vec2(v), "x") => Some(Value::Float(v.x)),
        (Value::Vec2(v), "y") => Some(Value::Float(v.y)),
        (Value::Vec3(v), "x") => Some(Value::Float(v.x)),
        (Value::Vec3(v), "y") => Some(Value::Float(v.y)),
        (Value::Vec3(v), "z") => Some(Value::Float(v.z)),
        (Value::Vec4(v), "x") => Some(Value::Float(v.x)),
        (Value::Vec4(v), "y") => Some(Value::Float(v.y)),
        (Value::Vec4(v), "z") => Some(Value::Float(v.z)),
        (Value::Vec4(v), "w") => Some(Value::Float(v.w)),
        (Value::Quat(q), "x") => Some(Value::Float(q.x)),
        (Value::Quat(q), "y") => Some(Value::Float(q.y)),
        (Value::Quat(q), "z") => Some(Value::Float(q.z)),
        (Value::Quat(q), "w") => Some(Value::Float(q.w)),
        (Value::Mat2(m), "x_axis") => Some(Value::Vec2(m.x_axis)),
        (Value::Mat2(m), "y_axis") => Some(Value::Vec2(m.y_axis)),
        (Value::Mat3(m), "x_axis") => Some(Value::Vec3(m.x_axis)),
        (Value::Mat3(m), "y_axis") => Some(Value::Vec3(m.y_axis)),
        (Value::Mat3(m), "z_axis") => Some(Value::Vec3(m.z_axis)),
        (Value::Mat4(m), "x_axis") => Some(Value::Vec4(m.x_axis)),
        (Value::Mat4(m), "y_axis") => Some(Value::Vec4(m.y_axis)),
        (Value::Mat4(m), "z_axis") => Some(Value::Vec4(m.z_axis)),
        (Value::Mat4(m), "w_axis") => Some(Value::Vec4(m.w_axis)),
        _ => None,
    }
}

/// Numeric scalar for a constructor lane / `lerp` `t` — ints promote to
/// f32 (ink's int→float coercion), everything else is not a lane.
fn scalar(v: &Value) -> Option<f32> {
    match v {
        Value::Float(f) => Some(*f),
        #[expect(
            clippy::cast_precision_loss,
            reason = "int->float promotion matches ink coercion semantics"
        )]
        Value::Int(n) => Some(*n as f32),
        _ => None,
    }
}

/// Pop `N` numeric lanes pushed left-to-right (so: reversed on the stack).
fn pop_lanes<const N: usize>(
    flow: &mut Flow,
    verb: &'static str,
) -> Result<[f32; N], RuntimeError> {
    let mut lanes = [0.0f32; N];
    for i in (0..N).rev() {
        let v = flow.pop_value()?;
        let Some(f) = scalar(&v) else {
            return Err(RuntimeError::StdlibWrongType {
                verb,
                expected: "numeric components",
                found: type_name(&v),
            });
        };
        lanes[i] = f;
    }
    Ok(lanes)
}

/// Pop `N` matrix columns pushed left-to-right, each converted through
/// `col` (the expected vector kind for this matrix size).
fn pop_cols<T: Copy + Default, const N: usize>(
    flow: &mut Flow,
    verb: &'static str,
    col: impl Fn(&Value) -> Option<T>,
) -> Result<[T; N], RuntimeError> {
    let mut cols = [T::default(); N];
    for i in (0..N).rev() {
        let v = flow.pop_value()?;
        let Some(c) = col(&v) else {
            return Err(RuntimeError::StdlibWrongType {
                verb,
                expected: "matching-size vector columns",
                found: type_name(&v),
            });
        };
        cols[i] = c;
    }
    Ok(cols)
}

fn as_vec2(v: &Value) -> Option<Vec2> {
    match v {
        Value::Vec2(v) => Some(*v),
        _ => None,
    }
}

fn as_vec3(v: &Value) -> Option<Vec3> {
    match v {
        Value::Vec3(v) => Some(*v),
        _ => None,
    }
}

fn as_vec4(v: &Value) -> Option<Vec4> {
    match v {
        Value::Vec4(v) => Some(*v),
        _ => None,
    }
}

/// The two-operand componentwise vector forms (`min`/`max`).
fn componentwise_pair(
    flow: &mut Flow,
    verb: &'static str,
    f2: fn(Vec2, Vec2) -> Vec2,
    f3: fn(Vec3, Vec3) -> Vec3,
    f4: fn(Vec4, Vec4) -> Vec4,
) -> Result<Value, RuntimeError> {
    let b = flow.pop_value()?;
    let a = flow.pop_value()?;
    match (&a, &b) {
        (Value::Vec2(a), Value::Vec2(b)) => Ok(Value::Vec2(f2(*a, *b))),
        (Value::Vec3(a), Value::Vec3(b)) => Ok(Value::Vec3(f3(*a, *b))),
        (Value::Vec4(a), Value::Vec4(b)) => Ok(Value::Vec4(f4(*a, *b))),
        _ => Err(wrong_type(verb, "two same-size vectors", &a, &b)),
    }
}

/// Build the two-operand wrong-type fault: blame the right operand when the
/// left is at least a tower value (the mismatch is then on the right —
/// including a same-family-different-size pair), else the left.
/// Deterministic and boring.
fn wrong_type(verb: &'static str, expected: &'static str, a: &Value, b: &Value) -> RuntimeError {
    let found = if crate::value_ops::is_tower(a) {
        type_name(b)
    } else {
        type_name(a)
    };
    RuntimeError::StdlibWrongType {
        verb,
        expected,
        found,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::OutputBuffer;
    use crate::story::PendingTerminal;
    use alloc::vec::Vec;

    fn test_flow() -> Flow {
        Flow {
            threads: Vec::new(),
            value_stack: Vec::new(),
            output: OutputBuffer::new(),
            pending_choices: Vec::new(),
            current_tags: Vec::new(),
            in_tag: false,
            skipping_choice: false,
            did_safe_exit: false,
            did_unsafe_yield: false,
            ran_out_of_content_cause: crate::RanOutOfContentCause::default(),
            exec_mode: crate::story::ExecMode::default(),
            pure_callback: crate::story::PureCallbackState::default(),
            next_block_id: 0,
            pending_terminal: PendingTerminal::default(),
        }
    }

    fn run(args: Vec<Value>, op: TowerOp) -> Result<Value, RuntimeError> {
        let mut flow = test_flow();
        for v in args {
            flow.value_stack.push(v);
        }
        tower_op(&mut flow, op)?;
        Ok(flow.pop_value().unwrap())
    }

    // ── Constructors ─────────────────────────────────────────────────

    #[test]
    fn constructors_build_glam_values_with_int_promotion() {
        // Int lanes promote to f32 (ink's int→float coercion).
        let v = run(vec![Value::Int(1), Value::Float(2.5)], TowerOp::MakeVec2).unwrap();
        assert_eq!(v, Value::Vec2(Vec2::new(1.0, 2.5)));

        let v = run(
            vec![Value::Float(1.0), Value::Int(2), Value::Int(3)],
            TowerOp::MakeVec3,
        )
        .unwrap();
        assert_eq!(v, Value::Vec3(Vec3::new(1.0, 2.0, 3.0)));

        let v = run(
            vec![
                Value::Float(0.0),
                Value::Float(0.0),
                Value::Float(0.0),
                Value::Float(1.0),
            ],
            TowerOp::MakeQuat,
        )
        .unwrap();
        assert_eq!(v, Value::Quat(Quat::from_xyzw(0.0, 0.0, 0.0, 1.0)));
    }

    #[test]
    fn matrix_constructors_take_column_vectors_column_major() {
        // T3: column-major — mat2(c0, c1) has c0 as x_axis.
        let m = run(
            vec![
                Value::Vec2(Vec2::new(1.0, 2.0)),
                Value::Vec2(Vec2::new(3.0, 4.0)),
            ],
            TowerOp::MakeMat2,
        )
        .unwrap();
        let Value::Mat2(m) = m else {
            unreachable!("expected mat2, got {m:?}");
        };
        assert_eq!(m.x_axis, Vec2::new(1.0, 2.0));
        assert_eq!(m.y_axis, Vec2::new(3.0, 4.0));
        // Column-major lanes exactly as the wire will carry them
        // (bit-compare — clippy float_cmp).
        assert_eq!(
            m.to_cols_array().map(f32::to_bits),
            [1.0f32, 2.0, 3.0, 4.0].map(f32::to_bits)
        );
    }

    #[test]
    fn constructor_wrong_lane_type_faults() {
        let err = run(
            vec![Value::Int(1), Value::String("y".into())],
            TowerOp::MakeVec2,
        )
        .unwrap_err();
        assert!(
            matches!(err, RuntimeError::StdlibWrongType { verb: "vec2", .. }),
            "{err:?}"
        );
    }

    #[test]
    fn matrix_constructor_wrong_column_kind_faults() {
        let err = run(
            vec![Value::Vec3(Vec3::ONE), Value::Vec3(Vec3::ONE)],
            TowerOp::MakeMat2,
        )
        .unwrap_err();
        assert!(
            matches!(err, RuntimeError::StdlibWrongType { verb: "mat2", .. }),
            "{err:?}"
        );
    }

    // ── Verbs ────────────────────────────────────────────────────────

    #[test]
    fn dot_and_cross_follow_glam() {
        let a = Vec3::new(1.0, 0.0, 0.0);
        let b = Vec3::new(0.0, 1.0, 0.0);
        let d = run(vec![Value::Vec3(a), Value::Vec3(b)], TowerOp::Dot).unwrap();
        assert_eq!(d, Value::Float(0.0));
        let c = run(vec![Value::Vec3(a), Value::Vec3(b)], TowerOp::Cross).unwrap();
        assert_eq!(c, Value::Vec3(Vec3::new(0.0, 0.0, 1.0)));
    }

    #[test]
    fn dot_cross_size_mismatch_faults() {
        let err = run(
            vec![Value::Vec2(Vec2::ONE), Value::Vec3(Vec3::ONE)],
            TowerOp::Dot,
        )
        .unwrap_err();
        assert!(
            matches!(err, RuntimeError::StdlibWrongType { .. }),
            "{err:?}"
        );
        let err = run(
            vec![Value::Vec2(Vec2::ONE), Value::Vec2(Vec2::ONE)],
            TowerOp::Cross,
        )
        .unwrap_err();
        assert!(
            matches!(err, RuntimeError::StdlibWrongType { verb: "cross", .. }),
            "{err:?}"
        );
    }

    #[test]
    fn min_max_clamp_lerp_are_componentwise_glam() {
        let a = Value::Vec2(Vec2::new(1.0, 4.0));
        let b = Value::Vec2(Vec2::new(3.0, 2.0));
        assert_eq!(
            run(vec![a.clone(), b.clone()], TowerOp::Min).unwrap(),
            Value::Vec2(Vec2::new(1.0, 2.0))
        );
        assert_eq!(
            run(vec![a.clone(), b.clone()], TowerOp::Max).unwrap(),
            Value::Vec2(Vec2::new(3.0, 4.0))
        );
        assert_eq!(
            run(
                vec![
                    Value::Vec2(Vec2::new(-1.0, 5.0)),
                    Value::Vec2(Vec2::ZERO),
                    Value::Vec2(Vec2::new(2.0, 2.0)),
                ],
                TowerOp::Clamp
            )
            .unwrap(),
            Value::Vec2(Vec2::new(0.0, 2.0))
        );
        assert_eq!(
            run(vec![a, b, Value::Float(0.5)], TowerOp::Lerp).unwrap(),
            Value::Vec2(Vec2::new(2.0, 3.0))
        );
    }

    #[test]
    fn quat_lerp_is_glam_normalizing_lerp() {
        let a = Quat::IDENTITY;
        let b = Quat::from_xyzw(0.0, 1.0, 0.0, 0.0);
        let got = run(
            vec![Value::Quat(a), Value::Quat(b), Value::Float(0.25)],
            TowerOp::Lerp,
        )
        .unwrap();
        assert_eq!(got, Value::Quat(a.lerp(b, 0.25)));
    }

    // ── Component accessors ──────────────────────────────────────────

    #[test]
    fn tower_components_expose_glam_fields() {
        let v = Value::Vec3(Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(tower_component(&v, "x"), Some(Value::Float(1.0)));
        assert_eq!(tower_component(&v, "z"), Some(Value::Float(3.0)));
        assert_eq!(tower_component(&v, "w"), None);
        let m = Value::Mat2(Mat2::from_cols(Vec2::new(1.0, 2.0), Vec2::new(3.0, 4.0)));
        assert_eq!(
            tower_component(&m, "y_axis"),
            Some(Value::Vec2(Vec2::new(3.0, 4.0)))
        );
        assert_eq!(tower_component(&m, "x"), None);
        assert_eq!(tower_component(&Value::Int(1), "x"), None);
    }
}
