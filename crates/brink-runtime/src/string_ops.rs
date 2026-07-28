//! T1b stdlib slice 1 completion: `char_at(s, i)` stdlib pure function
//! opcode implementation (`docs/t1b-surface-spec.md` §5, issue #857; fault
//! semantics from `docs/value-model-spec.md` §11c).
//!
//! Chars-indexed (Unicode scalar values via `str::chars`), not UTF-8 bytes —
//! author sanity, per the issue: a byte-indexed `s[i]` would panic or split a
//! multi-byte UTF-8 sequence for any non-ASCII text, which is exactly the
//! "silent garbage" the value model's §11c posture forbids. An out-of-range
//! index or a wrong-typed argument is a turn-terminating fault (never a
//! clamp, never a silently-empty result) — mirrors `conversion_ops`'s
//! `int`/`float` domain-fault posture; both are VM-native, author-
//! shadowable-with-warning (`E035`) T1b stdlib-slice-1 intrinsics.

use alloc::string::String;

use brink_format::Value;

use crate::error::RuntimeError;
use crate::story::Flow;

/// `CharAt`: `[s, i]` → single-character `String`. See module doc for
/// domain/fault rules.
pub(crate) fn char_at(flow: &mut Flow) -> Result<(), RuntimeError> {
    let index = flow.pop_value()?;
    let s = flow.pop_value()?;
    let Value::String(text) = &s else {
        return Err(RuntimeError::NotIndexable(type_name(&s)));
    };
    let Value::Int(i) = index else {
        return Err(RuntimeError::CharAtIndexNotInt(type_name(&index)));
    };
    let nth = if i >= 0 {
        #[expect(
            clippy::cast_sign_loss,
            reason = "i >= 0 just checked; the `else` branch below handles negative i"
        )]
        let idx = i as usize;
        text.chars().nth(idx)
    } else {
        None
    };
    match nth {
        Some(c) => {
            let mut out = String::new();
            out.push(c);
            flow.value_stack.push(Value::String(out.into()));
            Ok(())
        }
        None => Err(RuntimeError::CharAtOutOfBounds {
            index: i,
            len: text.chars().count(),
        }),
    }
}

/// `StrFind`: `[s, sub]` → `Option[int]` — the `find(s, sub)` stdlib verb
/// (NS-A1, `docs/stdlib-spec.md` §3: martyr #1 redeemed — "the -1 sentinel
/// dies unshipped"). The returned index counts **Unicode scalar values**
/// (chars), the same §3 indexing unit `char_at`/`slice` use, so
/// `char_at(s, find(s, sub) payload)` lands on `sub`'s first char even in
/// non-ASCII text. Absence (`sub` not in `s`) is `none`; wrong-typed
/// arguments are turn-terminating faults (a malformed question is a bug).
/// The empty substring is found at index 0, matching `str::find`.
pub(crate) fn str_find(flow: &mut Flow) -> Result<(), RuntimeError> {
    let sub = flow.pop_value()?;
    let s = flow.pop_value()?;
    let Value::String(text) = &s else {
        return Err(RuntimeError::StdlibWrongType {
            verb: "find",
            expected: "a string",
            found: type_name(&s),
        });
    };
    let Value::String(needle) = &sub else {
        return Err(RuntimeError::StdlibWrongType {
            verb: "find",
            expected: "a string",
            found: type_name(&sub),
        });
    };
    #[expect(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
    let result = text.find(needle.as_ref()).map_or_else(Value::none, |byte| {
        // Convert the byte offset to a char (USV) index.
        Value::some(Value::Int(text[..byte].chars().count() as i32))
    });
    flow.value_stack.push(result);
    Ok(())
}

/// Type-name label for the fault variants above — mirrors
/// `collection_ops`'/`conversion_ops`'/`record_ops`' own small hand-
/// duplicated `type_name` helpers (no shared export exists for this purpose
/// across the ops modules).
fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Int(_) => "int",
        Value::Float(_) => "float",
        Value::Bool(_) => "bool",
        Value::String(_) => "string",
        Value::List(_) => "list",
        Value::DivertTarget(_) => "divert_target",
        Value::VariablePointer(_) => "var_pointer",
        Value::TempPointer { .. } => "temp_pointer",
        Value::Null => "null",
        Value::FragmentRef(_) => "fragment_ref",
        Value::Array(_) => "array",
        Value::Map(_) => "map",
        Value::Record { .. } => "record",
        Value::FnRef(_) | Value::Closure(_) => "fn",
        Value::Handle { .. } => "handle",
        Value::Projection(_) => "projection",
        Value::OptionVal(_) => "option",
        Value::Range { .. } => "range",
        Value::Vec2(_) => "vec2",
        Value::Vec3(_) => "vec3",
        Value::Vec4(_) => "vec4",
        Value::Quat(_) => "quat",
        Value::Mat2(_) => "mat2",
        Value::Mat3(_) => "mat3",
        Value::Mat4(_) => "mat4",
        Value::Weighted(_) => "weighted",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::OutputBuffer;

    fn test_flow() -> Flow {
        Flow {
            threads: alloc::vec::Vec::new(),
            value_stack: alloc::vec::Vec::new(),
            output: OutputBuffer::new(),
            pending_choices: alloc::vec::Vec::new(),
            current_tags: alloc::vec::Vec::new(),
            in_tag: false,
            skipping_choice: false,
            did_safe_exit: false,
            did_unsafe_yield: false,
            exec_mode: crate::story::ExecMode::default(),
            pure_callback: crate::story::PureCallbackState::default(),
        }
    }

    fn push_args(flow: &mut Flow, args: [Value; 2]) {
        for v in args {
            flow.value_stack.push(v);
        }
    }

    #[test]
    fn char_at_first_char() {
        let mut flow = test_flow();
        push_args(&mut flow, [Value::from("hello"), Value::Int(0)]);
        char_at(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::String("h".into()));
    }

    #[test]
    fn char_at_last_char() {
        let mut flow = test_flow();
        push_args(&mut flow, [Value::from("hello"), Value::Int(4)]);
        char_at(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::String("o".into()));
    }

    #[test]
    fn char_at_counts_unicode_scalar_values_not_bytes() {
        // "café" is 4 chars but 5 UTF-8 bytes ('é' is 2 bytes) — a
        // byte-indexed read at 3 would land mid-codepoint. Chars indexing
        // (the ruled semantics) reads the 4th char cleanly.
        let mut flow = test_flow();
        push_args(&mut flow, [Value::from("café"), Value::Int(3)]);
        char_at(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::String("é".into()));
    }

    #[test]
    fn char_at_index_equal_to_len_faults() {
        let mut flow = test_flow();
        push_args(&mut flow, [Value::from("hi"), Value::Int(2)]);
        let err = char_at(&mut flow).unwrap_err();
        assert_eq!(err, RuntimeError::CharAtOutOfBounds { index: 2, len: 2 });
    }

    #[test]
    fn char_at_negative_index_faults() {
        let mut flow = test_flow();
        push_args(&mut flow, [Value::from("hi"), Value::Int(-1)]);
        let err = char_at(&mut flow).unwrap_err();
        assert_eq!(err, RuntimeError::CharAtOutOfBounds { index: -1, len: 2 });
    }

    #[test]
    fn char_at_out_of_bounds_reports_char_count_not_byte_length() {
        // "café" is 4 chars / 5 bytes — the fault's `len` must be the char
        // count (4), never the byte length (5), matching the "chars not
        // bytes" ruling end to end (success path and fault path agree).
        let mut flow = test_flow();
        push_args(&mut flow, [Value::from("café"), Value::Int(4)]);
        let err = char_at(&mut flow).unwrap_err();
        assert_eq!(err, RuntimeError::CharAtOutOfBounds { index: 4, len: 4 });
    }

    #[test]
    fn char_at_non_string_faults() {
        let mut flow = test_flow();
        push_args(&mut flow, [Value::Int(5), Value::Int(0)]);
        let err = char_at(&mut flow).unwrap_err();
        assert_eq!(err, RuntimeError::NotIndexable("int"));
    }

    #[test]
    fn char_at_non_int_index_faults() {
        let mut flow = test_flow();
        push_args(&mut flow, [Value::from("hi"), Value::from("nope")]);
        let err = char_at(&mut flow).unwrap_err();
        assert_eq!(err, RuntimeError::CharAtIndexNotInt("string"));
    }

    #[test]
    fn char_at_empty_string_faults() {
        let mut flow = test_flow();
        push_args(&mut flow, [Value::from(""), Value::Int(0)]);
        let err = char_at(&mut flow).unwrap_err();
        assert_eq!(err, RuntimeError::CharAtOutOfBounds { index: 0, len: 0 });
    }

    // ── NS-A1 `find(s, sub)` (docs/stdlib-spec.md §3, #1107) ────────────

    #[test]
    fn find_present_returns_some_index_absent_returns_none() {
        let mut flow = test_flow();
        push_args(&mut flow, [Value::from("hello"), Value::from("ll")]);
        str_find(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::some(Value::Int(2)));

        let mut flow = test_flow();
        push_args(&mut flow, [Value::from("hello"), Value::from("xyz")]);
        str_find(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::none());
    }

    #[test]
    fn find_counts_unicode_scalar_values_not_bytes() {
        // "café au lait": "au" starts at char 5 but byte 6 ('é' is 2 bytes)
        // — the §3 indexing unit is USVs, so `char_at(s, find payload)`
        // composes correctly.
        let mut flow = test_flow();
        push_args(&mut flow, [Value::from("café au lait"), Value::from("au")]);
        str_find(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::some(Value::Int(5)));
    }

    #[test]
    fn find_empty_substring_is_index_zero() {
        let mut flow = test_flow();
        push_args(&mut flow, [Value::from("hi"), Value::from("")]);
        str_find(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::some(Value::Int(0)));
    }

    #[test]
    fn find_non_string_arguments_fault() {
        let mut flow = test_flow();
        push_args(&mut flow, [Value::Int(3), Value::from("x")]);
        let err = str_find(&mut flow).unwrap_err();
        assert_eq!(
            err,
            RuntimeError::StdlibWrongType {
                verb: "find",
                expected: "a string",
                found: "int",
            }
        );

        let mut flow = test_flow();
        push_args(&mut flow, [Value::from("x"), Value::Int(3)]);
        let err = str_find(&mut flow).unwrap_err();
        assert_eq!(
            err,
            RuntimeError::StdlibWrongType {
                verb: "find",
                expected: "a string",
                found: "int",
            }
        );
    }
}
