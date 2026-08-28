use std::collections::BTreeMap;
#[cfg(debug_assertions)]
use std::collections::BTreeSet;
use std::sync::Arc;

use brink_format::Value;
use serde::Serialize;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

/// Map an ink [`Value`] to a native JS value for a binding argument. Scalars,
/// collections, and records cross the boundary; VM-internal variants
/// (pointers, divert targets, fragment refs, lists) map to `null` for now.
#[expect(
    clippy::too_many_lines,
    reason = "one marshal arm per Value variant (the #667 no-wildcard \
              discipline) — the NS-A8 tower arms pushed this past 100"
)]
pub(crate) fn value_to_js(v: &Value) -> JsValue {
    match v {
        Value::Int(i) => JsValue::from_f64(f64::from(*i)),
        Value::Float(f) => JsValue::from_f64(f64::from(*f)),
        Value::Bool(b) => JsValue::from_bool(*b),
        Value::String(s) => JsValue::from_str(s),
        // Collections marshal to native JS structures (value-model-spec §8: the
        // wasm boundary serializes trees; the host may never retain a handle
        // into script state — a JS array/object is an independent copy). Arrays
        // are lossless. A `Map` becomes a plain object with string keys: this
        // is the ergonomic native form, and — like ink's scalar coercions at
        // this boundary — it is deliberately lossy on key *type* and on the
        // ordering of integer-like keys (JS object rules). The lossless form is
        // `TypedValueJs` (the `eval_function` JSON boundary).
        Value::Array(items) => {
            let arr = js_sys::Array::new();
            for item in items.iter() {
                arr.push(&value_to_js(item));
            }
            arr.into()
        }
        Value::Map(map) => {
            let obj = js_sys::Object::new();
            #[cfg(debug_assertions)]
            {
                warn_on_map_key_collisions(map.keys());
                warn_on_map_key_reordering(map.keys());
                warn_on_float_precision_noise(map.values());
            }
            for (k, val) in map.iter() {
                // `Reflect::set` on a fresh, extensible object cannot fail;
                // discard the `Result` rather than unwrap it.
                let _ = js_sys::Reflect::set(&obj, &map_key_to_js(k), &value_to_js(val));
            }
            obj.into()
        }
        // A record (TM-4) is a named-field aggregate — far closer to `Map`
        // than to a VM-internal pointer, so it must not fall through to the
        // `null` arm (that would be a silent data drop on a wasm-observable
        // path). Field *names* live in the program's shape table, which this
        // native leg has no access to, so the ergonomic native form is a JS
        // array of the field values in shape order — the same deliberate
        // lossiness caveat as `Map` key types above. The lossless form
        // (shape id + fields) is `TypedValueJs::Record` on the
        // `eval_function` JSON boundary.
        Value::Record { fields, .. } => {
            let arr = js_sys::Array::new();
            for field in fields.iter() {
                arr.push(&value_to_js(field));
            }
            arr.into()
        }
        // A handle (T1d, `docs/t1d-spec.md` §2/§6) is a host-boundary
        // primitive — the whole point of the type is to cross into binding
        // code, so it must never fall into the `null` wildcard below (the
        // #667 Record-to-null hazard class this arm exists to close). No
        // `Program` is available at this layer (unlike `value_to_typed_js`,
        // the lossless JSON boundary), so `kind` crosses as its raw
        // `NameId` — a JS binding cannot resolve the kind *name* here, only
        // pass the token back verbatim to another binding call. `id`
        // crosses as a decimal string, not a `number`: a full-range `u64`
        // would silently lose precision above 2^53 as an `f64` (the same
        // lossiness class documented on the `Map` arm above), and a token's
        // whole point is exact equality, never arithmetic.
        Value::Handle { kind, id } => {
            let obj = js_sys::Object::new();
            let _ = js_sys::Reflect::set(
                &obj,
                &JsValue::from_str("kind"),
                &JsValue::from_f64(f64::from(kind.0)),
            );
            let _ = js_sys::Reflect::set(
                &obj,
                &JsValue::from_str("id"),
                &JsValue::from_str(&id.to_string()),
            );
            obj.into()
        }
        // An Option (NS-A1, `docs/stdlib-spec.md` §1.4) marshals to the
        // ergonomic native JS form: value-or-null (`none` -> null,
        // `some(x)` -> x's own native form). Like the `Map` key-type
        // mapping above this is deliberately lossy — `some(none)` flattens
        // to null, and a `some(x)` is indistinguishable from a bare `x` —
        // the lossless form is `TypedValueJs::Option` on the JSON boundary.
        Value::OptionVal(inner) => match inner {
            None => JsValue::NULL,
            Some(v) => value_to_js(v),
        },
        // A range (NS-A5, F7) is a real script value, so it must not fall
        // through to the VM-internal `null` arm (#667 hazard class). The
        // ergonomic native form is a plain `{start, end, inclusive}` object
        // — same shape as the lossless `TypedValueJs::Range` on the JSON
        // boundary.
        Value::Range {
            start,
            end,
            inclusive,
        } => {
            let obj = js_sys::Object::new();
            let _ = js_sys::Reflect::set(
                &obj,
                &JsValue::from_str("start"),
                &JsValue::from_f64(f64::from(*start)),
            );
            let _ = js_sys::Reflect::set(
                &obj,
                &JsValue::from_str("end"),
                &JsValue::from_f64(f64::from(*end)),
            );
            let _ = js_sys::Reflect::set(
                &obj,
                &JsValue::from_str("inclusive"),
                &JsValue::from_bool(*inclusive),
            );
            obj.into()
        }
        // Tower values (NS-A8, `docs/tower-mini-spec.md`): the ergonomic
        // native JS form — vectors/quats as `{x, y(, z, w)}` objects,
        // matrices as `{x_axis: {…}, …}` of their column vectors (glam's
        // field vocabulary, matching the display form and the component
        // accessors). Lane values cross as numbers (f32 → f64 exactly).
        // The lossless kind+lanes form is `TypedValueJs::Tower` on the
        // JSON boundary.
        // A weighted table (NS-A7, `docs/stdlib-spec.md` §8) is a real
        // script value, so it must not fall through to the VM-internal
        // `null` arm (#667 hazard class). The ergonomic native form is a
        // JS array of `{weight, value}` objects in construction order —
        // the same shape as the lossless `TypedValueJs::Weighted` on the
        // JSON boundary.
        Value::Weighted(w) => {
            let arr = js_sys::Array::new();
            for (weight, value) in &w.entries {
                let obj = js_sys::Object::new();
                let _ = js_sys::Reflect::set(
                    &obj,
                    &JsValue::from_str("weight"),
                    &JsValue::from_f64(f64::from(*weight)),
                );
                let _ =
                    js_sys::Reflect::set(&obj, &JsValue::from_str("value"), &value_to_js(value));
                arr.push(&obj);
            }
            arr.into()
        }
        Value::Vec2(v) => lanes_to_js(&[("x", v.x), ("y", v.y)]),
        Value::Vec3(v) => lanes_to_js(&[("x", v.x), ("y", v.y), ("z", v.z)]),
        Value::Vec4(v) => lanes_to_js(&[("x", v.x), ("y", v.y), ("z", v.z), ("w", v.w)]),
        Value::Quat(q) => lanes_to_js(&[("x", q.x), ("y", q.y), ("z", q.z), ("w", q.w)]),
        Value::Mat2(m) => cols_to_js(&[
            ("x_axis", Value::Vec2(m.x_axis)),
            ("y_axis", Value::Vec2(m.y_axis)),
        ]),
        Value::Mat3(m) => cols_to_js(&[
            ("x_axis", Value::Vec3(m.x_axis)),
            ("y_axis", Value::Vec3(m.y_axis)),
            ("z_axis", Value::Vec3(m.z_axis)),
        ]),
        Value::Mat4(m) => cols_to_js(&[
            ("x_axis", Value::Vec4(m.x_axis)),
            ("y_axis", Value::Vec4(m.y_axis)),
            ("z_axis", Value::Vec4(m.z_axis)),
            ("w_axis", Value::Vec4(m.w_axis)),
        ]),
        // Every remaining variant is VM-internal (a pointer, a divert target,
        // a fragment ref, a raw list, or an unmaterialized fn/projection
        // value) and has no useful native-JS shape at this scalar-only
        // binding-argument boundary, so it maps to `null` — same as before.
        // Spelled out explicitly, not folded into a trailing `_` wildcard:
        // the #667 hazard is precisely a wildcard silently absorbing a
        // *future* `Value` variant (the way `Record` once did, PR #664, and
        // `Handle` almost did). Listing every variant by name makes adding
        // one to the enum a compile error here, not a silent null.
        Value::List(_)
        | Value::DivertTarget(_)
        | Value::VariablePointer(_)
        | Value::TempPointer { .. }
        | Value::Null
        | Value::FragmentRef(_)
        | Value::FnRef(_)
        | Value::Closure(_)
        | Value::Projection(_) => JsValue::NULL,
    }
}

/// Build a native JS object of named f32 lanes (`{x, y, …}`) for the
/// vector/quat legs of [`value_to_js`]'s NS-A8 tower marshaling.
fn lanes_to_js(lanes: &[(&str, f32)]) -> JsValue {
    let obj = js_sys::Object::new();
    for (name, lane) in lanes {
        // `Reflect::set` on a fresh, extensible object cannot fail.
        let _ = js_sys::Reflect::set(
            &obj,
            &JsValue::from_str(name),
            &JsValue::from_f64(f64::from(*lane)),
        );
    }
    obj.into()
}

/// Build a native JS object of named column vectors (`{x_axis: {…}, …}`)
/// for the matrix legs of [`value_to_js`]'s NS-A8 tower marshaling.
fn cols_to_js(cols: &[(&str, Value)]) -> JsValue {
    let obj = js_sys::Object::new();
    for (name, col) in cols {
        let _ = js_sys::Reflect::set(&obj, &JsValue::from_str(name), &value_to_js(col));
    }
    obj.into()
}

/// The JS-property-string a [`brink_format::MapKey`] coerces to at the
/// `value_to_js` wasm boundary: `int`/`bool` keys render to their canonical
/// string form, `string` keys pass through unchanged.
///
/// This is the **one coercion source** [`map_key_to_js`] (the always-on
/// marshaling path) and the debug-only diagnostics below both derive from
/// (#560 — a prior version duplicated this match in a debug-only twin,
/// `map_key_coercion_string`, so nothing caught it drifting from the real
/// coercion). Being plain Rust with no `JsValue`, it — and everything built
/// on it — is unit-testable on the host without a JS runtime or wasm32
/// target.
fn map_key_coercion_string(key: &brink_format::MapKey) -> String {
    match key {
        brink_format::MapKey::Int(n) => n.to_string(),
        brink_format::MapKey::Str(s) => s.to_string(),
        brink_format::MapKey::Bool(b) => if *b { "true" } else { "false" }.to_string(),
    }
}

/// Stringify a [`Value::Map`] key for the native-JS-object boundary (the
/// lossy leg documented on [`value_to_js`]). Thin wrapper over
/// [`map_key_coercion_string`] — see that function's doc for why it, not a
/// second copy of this match, is the coercion source of truth.
pub(crate) fn map_key_to_js(key: &brink_format::MapKey) -> JsValue {
    JsValue::from_str(&map_key_coercion_string(key))
}

// ── map_key_to_js debug diagnostics (§8's native-leg failure modes) ───────
//
// `map_key_to_js` deliberately coerces every `MapKey` variant to a JS
// property string (value-model-spec §8: `value_to_js`'s `Map` arm is the
// lossy native leg; `value_to_typed_js` — the `eval_function` JSON boundary
// — is the lossless one). Three distinct failure modes follow from that:
//
// 1. **Collision** (#551/#555): distinct keys can coerce to the same
//    string — `MapKey::Int(1)` and `MapKey::Str("1")` both become `"1"` —
//    and `Reflect::set` then silently drops one entry.
// 2. **Reordering** (#559): even without collisions, a real JS engine
//    enumerates an object's own "array index" property names (ECMA-262
//    §6.1.7 — canonical non-negative integer strings below `2^32 - 1`) in
//    ascending numeric order *before* any other string keys, regardless of
//    `Reflect::set` call order. That silently reorders a map's author
//    insertion order whenever an integer-like key is inserted somewhere
//    other than first.
// 3. **Float precision-display noise** (#568): this one is on the *value*
//    side of a map entry, not the key side — `MapKey`'s ratified domain
//    (value-model-spec §4/§11c) is `{int(i32), string, bool}`, which has no
//    float variant, and `Value::Int` (i32) widens to `f64` exactly (no i32
//    magnitude can lose precision in a 53-bit mantissa), so neither a
//    literal "float map key" nor a "large-int" precision loss is reachable.
//    What *is* reachable: a `Value::Float` (f32) map **value** widens to
//    `f64` via `f64::from` — mathematically exact, bit-for-bit — but a real
//    JS engine's `Number.prototype.toString()` computes the *shortest*
//    decimal that round-trips to that exact f64 bit pattern, which is
//    almost never the short decimal an author recognizes as "their" f32
//    (`0.1f32` widens to the f64 whose shortest round-trip decimal is
//    `0.10000000149011612`). No value precision is actually lost — the
//    widening is exact — but the extra digits are a genuine "where did
//    these come from" surprise at the same marshaling boundary, so this
//    diagnostic runs alongside the two key-coercion ones in the same `Map`
//    arm even though it inspects values, not keys.
//
// None of these want a behavior change: these are debug-build visibility
// aids so the failure modes are diagnosable once T1b makes them
// author-reachable. Detection is plain Rust with no `JsValue`, so it is
// unit-testable on the host without a JS runtime; only the `warn_on_*`
// wrappers touch
// `web_sys::console`.

/// The coerced JS-property strings that more than one of `keys` maps to
/// (via [`map_key_coercion_string`]), in first-collision order. Empty when
/// every key coerces to a distinct string.
#[cfg(debug_assertions)]
fn map_key_collisions<'a>(keys: impl Iterator<Item = &'a brink_format::MapKey>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut collided = Vec::new();
    for key in keys {
        let coerced = map_key_coercion_string(key);
        if !seen.insert(coerced.clone()) && !collided.contains(&coerced) {
            collided.push(coerced);
        }
    }
    collided
}

/// Debug-only diagnostic: log a `console.warn` for each JS-property string
/// that two or more of a map's keys coerce to via [`map_key_to_js`] — the
/// silent-overwrite failure mode this issue tracks. Purely observational;
/// the caller's `Reflect::set` loop still runs unchanged.
#[cfg(debug_assertions)]
fn warn_on_map_key_collisions<'a>(keys: impl Iterator<Item = &'a brink_format::MapKey>) {
    for coerced in map_key_collisions(keys) {
        web_sys::console::warn_1(&JsValue::from_str(&format!(
            "brink: map key coercion collision at the wasm value_to_js boundary: \
             multiple map keys stringify to {coerced:?} — one entry silently \
             overwrites another (value-model-spec §8; the lossless boundary is \
             value_to_typed_js / eval_function's TypedValueJs result)"
        )));
    }
}

/// The `u32` array-index value of a coerced key string `s`, or `None` if `s`
/// is not a JS "array index" property name (ECMA-262 §6.1.7): a canonical
/// non-negative integer string — no leading zeros unless `s == "0"` — whose
/// value is strictly below `2^32 - 1`. Such property names are the ones a
/// real JS engine reorders ahead of other string keys during own-property
/// enumeration (`Reflect::ownKeys`, `for...in`, `Object.keys`).
#[cfg(debug_assertions)]
fn js_array_index_value(s: &str) -> Option<u32> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if s.len() > 1 && s.starts_with('0') {
        return None;
    }
    let n: u32 = s.parse().ok()?;
    (n != u32::MAX).then_some(n)
}

/// If a real JS engine's own-property enumeration order for `keys` (coerced
/// via [`map_key_coercion_string`]) would differ from the map's author
/// insertion order, returns `Some((insertion_order, js_order))`. `None` if
/// the map has no array-index keys out of place — insertion order already
/// matches what JS would produce.
#[cfg(debug_assertions)]
fn map_key_reordering<'a>(
    keys: impl Iterator<Item = &'a brink_format::MapKey>,
) -> Option<(Vec<String>, Vec<String>)> {
    let insertion_order: Vec<String> = keys.map(map_key_coercion_string).collect();
    let mut index_keys: Vec<(u32, usize)> = Vec::new();
    let mut string_keys: Vec<usize> = Vec::new();
    for (i, s) in insertion_order.iter().enumerate() {
        match js_array_index_value(s) {
            Some(n) => index_keys.push((n, i)),
            None => string_keys.push(i),
        }
    }
    index_keys.sort_by_key(|&(n, _)| n);
    let js_order: Vec<String> = index_keys
        .into_iter()
        .map(|(_, i)| i)
        .chain(string_keys)
        .map(|i| insertion_order[i].clone())
        .collect();
    (insertion_order != js_order).then_some((insertion_order, js_order))
}

/// Debug-only diagnostic: log a `console.warn` when JS's own-property
/// enumeration order would reorder a map's coerced keys relative to its
/// author insertion order — the integer-like-key-reordering failure mode
/// this issue tracks. Purely observational; the caller's `Reflect::set`
/// loop still runs unchanged (and `Reflect::set` call order does not
/// actually control JS enumeration order — that is exactly the bug being
/// diagnosed).
#[cfg(debug_assertions)]
fn warn_on_map_key_reordering<'a>(keys: impl Iterator<Item = &'a brink_format::MapKey>) {
    if let Some((insertion_order, js_order)) = map_key_reordering(keys) {
        web_sys::console::warn_1(&JsValue::from_str(&format!(
            "brink: map key reordering at the wasm value_to_js boundary: JS objects \
             enumerate integer-like keys in ascending numeric order before other keys, \
             silently reordering this map's author insertion order {insertion_order:?} \
             to {js_order:?} (value-model-spec §8; the lossless, order-preserving \
             boundary is value_to_typed_js / eval_function's TypedValueJs result)"
        )));
    }
}

/// Whether widening `f` (a `Value::Float`) to `f64` — exactly what
/// `value_to_js`'s scalar arm does via `JsValue::from_f64(f64::from(*f))`
/// — would print with more digits than `f`'s own shortest round-trip
/// decimal. Both Rust's `f32`/`f64` `Display` and a real JS engine's
/// `Number.prototype.toString()` compute the shortest decimal string that
/// round-trips to the exact same bit pattern (IEEE 754 shortest-round-trip
/// formatting), so this is host-testable without a JS runtime and without
/// `JsValue`.
///
/// `Value::Int` (i32) never triggers this: `f64::from(i32)` is exact for
/// every i32 magnitude (i32's full range fits well inside f64's 53-bit
/// mantissa), so the widened f64's shortest decimal always equals the i32's
/// own decimal string. A "large-int" precision-loss variant of this
/// diagnostic is not meaningfully constructible — see the module doc above.
#[cfg(debug_assertions)]
fn float_widening_shows_precision_noise(f: f32) -> bool {
    f64::from(f).to_string() != f.to_string()
}

/// The map values (in iteration order) that are `Value::Float`s whose f64
/// widening shows precision-display noise (`(index, value)` pairs — `index`
/// is the value's position among ALL map values, matching how a consumer
/// would locate it against `map.values()`/`map.iter()`).
#[cfg(debug_assertions)]
fn float_precision_noise_values<'a>(values: impl Iterator<Item = &'a Value>) -> Vec<(usize, f32)> {
    values
        .enumerate()
        .filter_map(|(i, v)| match v {
            Value::Float(f) if float_widening_shows_precision_noise(*f) => Some((i, *f)),
            _ => None,
        })
        .collect()
}

/// Debug-only diagnostic: log a `console.warn` for each `Value::Float` map
/// value whose `f64` widening would print with more digits in a real JS
/// engine than the value's own f32 shortest decimal — the precision-display
/// noise failure mode this issue tracks (see the module doc above for why
/// this is a *value*-side check even though it lives alongside the two
/// key-coercion diagnostics). Purely observational; the caller's widening
/// (`value_to_js`'s `Value::Float` arm) still runs unchanged — the widening
/// itself is exact, only its JS-visible string form is surprising.
#[cfg(debug_assertions)]
fn warn_on_float_precision_noise<'a>(values: impl Iterator<Item = &'a Value>) {
    for (index, f) in float_precision_noise_values(values) {
        web_sys::console::warn_1(&JsValue::from_str(&format!(
            "brink: float precision-display noise at the wasm value_to_js boundary: \
             map value #{index} ({f}f32) widens losslessly to f64::from({f}) = {} \
             — a real JS engine's Number.prototype.toString() will print that longer \
             decimal, not {f} (value-model-spec §8; the lossless boundary is \
             value_to_typed_js / eval_function's TypedValueJs result, which preserves \
             the f32 value directly)",
            f64::from(f),
        )));
    }
}

/// Plain-Rust tests for the debug diagnostics' *detection* logic (no
/// `JsValue`/`web_sys`, so these run under host `cargo test` — no wasm32
/// target needed).
#[cfg(all(test, debug_assertions))]
mod map_key_diagnostic_tests {
    use super::{
        float_precision_noise_values, float_widening_shows_precision_noise, js_array_index_value,
        map_key_coercion_string, map_key_collisions, map_key_reordering,
    };
    use brink_format::{MapKey, Value};

    #[test]
    fn int_and_string_coerce_to_the_same_property_name() {
        assert_eq!(map_key_coercion_string(&MapKey::from(1)), "1");
        assert_eq!(map_key_coercion_string(&MapKey::from("1")), "1");
    }

    #[test]
    fn bool_and_string_coerce_to_the_same_property_name() {
        assert_eq!(map_key_coercion_string(&MapKey::from(true)), "true");
        assert_eq!(map_key_coercion_string(&MapKey::from("true")), "true");
    }

    #[test]
    fn no_collision_among_distinct_keys() {
        let keys = [MapKey::from(1), MapKey::from("two"), MapKey::from(false)];
        assert!(map_key_collisions(keys.iter()).is_empty());
    }

    #[test]
    fn detects_int_str_collision() {
        let keys = [MapKey::from(1), MapKey::from("1")];
        assert_eq!(map_key_collisions(keys.iter()), vec!["1".to_string()]);
    }

    #[test]
    fn detects_bool_str_collision() {
        let keys = [MapKey::from(true), MapKey::from("true")];
        assert_eq!(map_key_collisions(keys.iter()), vec!["true".to_string()]);
    }

    #[test]
    fn reports_each_colliding_group_once_regardless_of_group_size() {
        // Three keys all coercing to "1": the collision is reported once,
        // not once per extra colliding key.
        let keys = [MapKey::from(1), MapKey::from("1"), MapKey::from("1")];
        assert_eq!(map_key_collisions(keys.iter()), vec!["1".to_string()]);
    }

    #[test]
    fn multiple_independent_collisions_are_all_reported() {
        let keys = [
            MapKey::from(1),
            MapKey::from("1"),
            MapKey::from(true),
            MapKey::from("true"),
        ];
        let mut collided = map_key_collisions(keys.iter());
        collided.sort();
        assert_eq!(collided, vec!["1".to_string(), "true".to_string()]);
    }

    #[test]
    fn array_index_values_accept_canonical_non_negative_integers() {
        assert_eq!(js_array_index_value("0"), Some(0));
        assert_eq!(js_array_index_value("1"), Some(1));
        assert_eq!(js_array_index_value("4294967293"), Some(4_294_967_293));
    }

    #[test]
    fn array_index_values_reject_the_2_32_minus_1_boundary() {
        // 2^32 - 1 is explicitly excluded by ECMA-262 §6.1.7.
        assert_eq!(js_array_index_value("4294967295"), None);
    }

    #[test]
    fn array_index_values_reject_leading_zeros_and_non_digits() {
        assert_eq!(js_array_index_value("00"), None);
        assert_eq!(js_array_index_value("01"), None);
        assert_eq!(js_array_index_value("-1"), None);
        assert_eq!(js_array_index_value("1.0"), None);
        assert_eq!(js_array_index_value("abc"), None);
        assert_eq!(js_array_index_value(""), None);
    }

    #[test]
    fn no_reordering_for_string_only_keys_in_any_order() {
        let keys = [MapKey::from("b"), MapKey::from("a"), MapKey::from("c")];
        assert_eq!(map_key_reordering(keys.iter()), None);
    }

    #[test]
    fn no_reordering_when_integer_like_keys_already_lead_in_ascending_order() {
        let keys = [MapKey::from(0), MapKey::from(1), MapKey::from("z")];
        assert_eq!(map_key_reordering(keys.iter()), None);
    }

    #[test]
    fn detects_reordering_when_integer_like_key_inserted_after_a_string_key() {
        // Author order: "z" then 0. JS enumerates array-index keys first,
        // so "0" jumps ahead of "z".
        let keys = [MapKey::from("z"), MapKey::from(0)];
        assert_eq!(
            map_key_reordering(keys.iter()),
            Some((
                vec!["z".to_string(), "0".to_string()],
                vec!["0".to_string(), "z".to_string()],
            ))
        );
    }

    #[test]
    fn detects_reordering_when_integer_like_keys_are_out_of_ascending_order() {
        // Author inserted 2 before 1; JS still enumerates 1 before 2.
        let keys = [MapKey::from(2), MapKey::from(1)];
        assert_eq!(
            map_key_reordering(keys.iter()),
            Some((
                vec!["2".to_string(), "1".to_string()],
                vec!["1".to_string(), "2".to_string()],
            ))
        );
    }

    #[test]
    fn non_canonical_integer_like_string_key_is_not_treated_as_array_index() {
        // "007" is not a canonical array index (leading zero), so it stays
        // a plain string key and does not get reordered ahead of "z".
        let keys = [MapKey::from("z"), MapKey::from("007")];
        assert_eq!(map_key_reordering(keys.iter()), None);
    }

    // ── #568: float precision-display noise (third lossy-leg failure mode) ─

    #[test]
    fn zero_point_one_widens_with_visible_precision_noise() {
        // The canonical example: 0.1f32's exact value, widened to f64, has
        // a much longer shortest-round-trip decimal than "0.1".
        assert!(float_widening_shows_precision_noise(0.1_f32));
    }

    #[test]
    fn whole_number_floats_never_show_precision_noise() {
        for f in [0.0_f32, 1.0, -1.0, 5.0, 100.0, -3.5_f32 * 2.0] {
            assert!(
                !float_widening_shows_precision_noise(f),
                "{f} should widen cleanly"
            );
        }
    }

    #[test]
    fn simple_binary_fractions_never_show_precision_noise() {
        // 0.5, 0.25, 0.125, … are exact in both f32 and f64 (powers of two),
        // so their shortest decimals match after widening.
        for f in [0.5_f32, 0.25, 0.125, -0.5] {
            assert!(
                !float_widening_shows_precision_noise(f),
                "{f} should widen cleanly"
            );
        }
    }

    #[test]
    fn large_int_map_values_never_show_precision_noise() {
        // Value::Int (i32) widening to f64 is exact for every magnitude —
        // this function only inspects Value::Float, but the underlying
        // widening rule (f64::from) applies identically, so exercise it
        // directly for completeness with the module doc's claim.
        for n in [0_i32, 1, -1, i32::MAX, i32::MIN, 2_147_483_000] {
            let widened = f64::from(n);
            assert_eq!(
                widened.to_string(),
                n.to_string(),
                "i32 -> f64 widening must never show precision noise"
            );
        }
    }

    #[test]
    fn float_precision_noise_values_finds_the_offending_index() {
        let values = [
            Value::Int(1),
            Value::from("hp"),
            Value::Float(0.1),
            Value::Bool(true),
            Value::Float(2.0),
        ];
        let found = float_precision_noise_values(values.iter());
        assert_eq!(found, vec![(2, 0.1_f32)]);
    }

    #[test]
    fn float_precision_noise_values_empty_when_nothing_noisy() {
        let values = [Value::Int(1), Value::Float(2.0), Value::from("x")];
        assert!(float_precision_noise_values(values.iter()).is_empty());
    }
}

/// Read a JS value returned from a binding back into an ink [`Value`].
/// `null`/`undefined` → `Null`; booleans → `Bool`; an integer-valued finite
/// number → `Int`, otherwise `Float`; strings → `String`. A JS array becomes a
/// [`Value::Array`] and a plain object a [`Value::Map`] (string keys, in JS
/// property order — value-model-spec §8), each converted recursively.
/// Functions and other exotic objects → `Null`.
///
/// **Deliberately does not reconstruct [`Value::Handle`]** from a
/// `{kind, id}`-shaped object: a plain object with those two properties
/// falls through to the generic `Map` case above like any other object.
/// This is not a coverage gap — it is the T1d security invariant made
/// concrete (`docs/t1d-spec.md` §7: "handles are true object-capability
/// tokens... possession is authority"; the RFC's "no literal syntax" rule).
/// If this function forged a `Handle` from any JS object matching that
/// shape, a JS binding (or, worse, injected page script in the pure-web
/// deployment) could mint a fake capability token out of thin air just by
/// returning `{kind: 3, id: 42}` from any binding call — no possession
/// required. Only Rust-side binding code that already holds a real
/// `Value::Handle` (or the manifest/kind-vocabulary machinery T1d-2/T1d-3
/// add) may construct one; that is enforced by `Value::handle` staying a
/// plain constructor with no `js_to_value` counterpart, not by a check here.
pub(crate) fn js_to_value(js: &JsValue) -> Value {
    if js.is_null() || js.is_undefined() {
        return Value::Null;
    }
    if let Some(b) = js.as_bool() {
        return Value::Bool(b);
    }
    if let Some(n) = js.as_f64() {
        if n.is_finite() && n.fract() == 0.0 && n.abs() <= f64::from(i32::MAX) {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "guarded: finite, integral, within i32 range"
            )]
            return Value::Int(n as i32);
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "ink floats are f32; JS numbers narrow to f32 at the boundary"
        )]
        return Value::Float(n as f32);
    }
    if let Some(s) = js.as_string() {
        return Value::String(Arc::from(s));
    }
    // Array before the generic-object case: a JS array *is* an object.
    if js_sys::Array::is_array(js) {
        let arr = js_sys::Array::from(js);
        let items: Vec<Value> = arr.iter().map(|el| js_to_value(&el)).collect();
        return Value::array(items);
    }
    // A plain object → Map with string keys, in JS own-property order (integer
    // indices ascending, then string keys in insertion order — deterministic).
    // Functions are objects too, but carry no data — they fold to Null.
    if js.is_object() && !js.is_function() {
        let obj: &js_sys::Object = js.unchecked_ref();
        let mut map = brink_format::OrderedMap::new();
        for entry in js_sys::Object::entries(obj).iter() {
            let pair = js_sys::Array::from(&entry);
            if let Some(k) = pair.get(0).as_string() {
                map.insert(
                    brink_format::MapKey::Str(Arc::from(k)),
                    js_to_value(&pair.get(1)),
                );
            }
        }
        return Value::map(map);
    }
    Value::Null
}

// ── Debug snapshot JSON mirror ───────────────────────────────────────

#[derive(Serialize)]
pub(crate) struct DebugStateJs {
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    current_location: Option<String>,
    /// Precise `(container_idx, offset)` for the active flow (issue #3182).
    /// Additive: existing consumers of `current_location`/`call_stack[].location`
    /// are unaffected. Not yet resolved to source — that lands with the
    /// wasm-bridge integration (D9, #3187, `docs/debugger-spec.md` §6).
    #[serde(skip_serializing_if = "Option::is_none")]
    position: Option<DebugPositionJs>,
    turn_index: u32,
    globals: Vec<DebugGlobalJs>,
    call_stack: Vec<DebugFrameJs>,
    visit_counts: Vec<DebugVisitJs>,
    pending_choices: Vec<DebugChoiceJs>,
    rng: DebugRngJs,
}

#[derive(Serialize)]
pub(crate) struct DebugPositionJs {
    container_idx: u32,
    offset: usize,
}

#[derive(Serialize)]
struct DebugGlobalJs {
    name: String,
    value: String,
}

#[derive(Serialize)]
struct DebugFrameJs {
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    position: Option<DebugPositionJs>,
    temps: usize,
    /// D7 (`docs/debugger-spec.md` §3, #3185): this frame's named locals.
    /// Additive alongside `temps` (D4's bare count, kept as-is). `None`
    /// when the linked program carries no `DebugInfo` at all — see
    /// `brink_runtime::debug::DebugFrame::locals`'s own doc.
    #[serde(skip_serializing_if = "Option::is_none")]
    locals: Option<Vec<DebugLocalJs>>,
}

#[derive(Serialize)]
struct DebugLocalJs {
    slot: u16,
    name: String,
    value: DebugValueJs,
}

/// Structured mirror of `brink_runtime::debug::DebugValue`
/// (`docs/debugger-spec.md` §3, D7/#3185) — internally tagged on `type` so
/// a JS consumer can `switch` on it directly rather than probing for which
/// field is present.
#[derive(Serialize)]
#[serde(tag = "type")]
enum DebugValueJs {
    #[serde(rename = "int")]
    Int { value: i32 },
    #[serde(rename = "float")]
    Float { value: f32 },
    #[serde(rename = "bool")]
    Bool { value: bool },
    #[serde(rename = "string")]
    Str { value: String },
    #[serde(rename = "null")]
    Null,
    #[serde(rename = "list")]
    List { members: Vec<String> },
    #[serde(rename = "divertTarget")]
    DivertTarget {
        #[serde(skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },
    #[serde(rename = "struct")]
    Struct {
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        fields: Vec<DebugFieldJs>,
    },
    // `id` crosses as a decimal string, not a `number` — the same
    // full-range-`u64`-as-`f64` precision hazard `value_to_js`'s own
    // `Value::Handle` arm documents (this file, above): a token's whole
    // point is exact equality, never arithmetic.
    #[serde(rename = "handle")]
    Handle { kind: String, id: String },
    /// Every value kind not modeled above — see
    /// `brink_runtime::debug::DebugValue::Other`'s own doc.
    #[serde(rename = "other")]
    Other { display: String },
}

#[derive(Serialize)]
struct DebugFieldJs {
    name: String,
    value: DebugValueJs,
}

fn debug_value_to_js(v: brink_runtime::DebugValue) -> DebugValueJs {
    use brink_runtime::DebugValue;
    match v {
        DebugValue::Int(i) => DebugValueJs::Int { value: i },
        DebugValue::Float(f) => DebugValueJs::Float { value: f },
        DebugValue::Bool(b) => DebugValueJs::Bool { value: b },
        DebugValue::Str(s) => DebugValueJs::Str { value: s },
        DebugValue::Null => DebugValueJs::Null,
        DebugValue::List(members) => DebugValueJs::List { members },
        DebugValue::DivertTarget(path) => DebugValueJs::DivertTarget { path },
        DebugValue::Struct { name, fields } => DebugValueJs::Struct {
            name,
            fields: fields
                .into_iter()
                .map(|(name, value)| DebugFieldJs {
                    name,
                    value: debug_value_to_js(value),
                })
                .collect(),
        },
        DebugValue::Handle { kind, id } => DebugValueJs::Handle {
            kind,
            id: id.to_string(),
        },
        DebugValue::Other(display) => DebugValueJs::Other { display },
    }
}

#[derive(Serialize)]
struct DebugVisitJs {
    path: String,
    count: u32,
}

#[derive(Serialize)]
struct DebugChoiceJs {
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    target: Option<String>,
    index: usize,
}

#[derive(Serialize)]
struct DebugRngJs {
    seed: i32,
    previous: i32,
}

pub(crate) fn debug_snapshot_to_js(s: brink_runtime::DebugSnapshot) -> DebugStateJs {
    DebugStateJs {
        status: s.status,
        current_location: s.current_location,
        position: s.position.map(|p| DebugPositionJs {
            container_idx: p.container_idx,
            offset: p.offset,
        }),
        turn_index: s.turn_index,
        globals: s
            .globals
            .into_iter()
            .map(|g| DebugGlobalJs {
                name: g.name,
                value: g.value,
            })
            .collect(),
        call_stack: s
            .call_stack
            .into_iter()
            .map(|f| DebugFrameJs {
                kind: f.kind,
                location: f.location,
                position: f.position.map(|p| DebugPositionJs {
                    container_idx: p.container_idx,
                    offset: p.offset,
                }),
                temps: f.temps,
                locals: f.locals.map(|locals| {
                    locals
                        .into_iter()
                        .map(|l| DebugLocalJs {
                            slot: l.slot,
                            name: l.name,
                            value: debug_value_to_js(l.value),
                        })
                        .collect()
                }),
            })
            .collect(),
        visit_counts: s
            .visit_counts
            .into_iter()
            .map(|v| DebugVisitJs {
                path: v.path,
                count: v.count,
            })
            .collect(),
        pending_choices: s
            .pending_choices
            .into_iter()
            .map(|c| DebugChoiceJs {
                text: c.text,
                target: c.target,
                index: c.index,
            })
            .collect(),
        rng: DebugRngJs {
            seed: s.rng.seed,
            previous: s.rng.previous,
        },
    }
}

/// The wasm shape of [`brink_runtime::DebugSourceLocation`] — the
/// program→source resolver's result (D9, #3187). `file` is `None` for the
/// reserved synthetic sentinel (no author source); a caller building a
/// `{ kind: "source" }` studio Location (`docs/studio-shell-spec.md` §6.1)
/// treats `file: null` as "unresolvable to source" the same as the whole
/// value being absent.
#[derive(Serialize)]
pub(crate) struct DebugSourceLocationJs {
    pub file: Option<String>,
    pub range_start: u32,
    pub range_len: u32,
}

pub(crate) fn debug_source_location_to_js(
    loc: brink_runtime::DebugSourceLocation,
) -> DebugSourceLocationJs {
    DebugSourceLocationJs {
        file: loc.file,
        range_start: loc.range_start,
        range_len: loc.range_len,
    }
}

// ── Debug control (D8, #3186) — the wasm control-half bridge (#3232) ──
//
// `debug_snapshot`/`resolve_debug_position` above are the read half (D4/D9);
// these mirror the control half's wire shapes — breakpoints and the
// `debugRun`/`debugStep` outcome — for `WebSession`/`StoryRunner`'s new
// `debugRun`/`debugStep`/`debugBreakpoint*` bindings.

/// Wasm mirror of `brink_runtime::Breakpoint` — the wire shape
/// `debugBreakpoints()` returns.
#[derive(Serialize)]
pub(crate) struct BreakpointJs {
    pub id: u32,
    pub container_idx: u32,
    pub offset: usize,
    pub name: String,
    pub enabled: bool,
}

pub(crate) fn breakpoint_to_js(b: &brink_runtime::Breakpoint) -> BreakpointJs {
    BreakpointJs {
        id: b.id,
        container_idx: b.container_idx,
        offset: b.offset,
        name: b.name.clone(),
        enabled: b.enabled,
    }
}

/// Wasm mirror of `brink_runtime::DebugStopReason` — internally tagged on
/// `type`, the same convention `DebugValueJs` above uses.
#[derive(Serialize)]
#[serde(tag = "type")]
pub(crate) enum DebugStopReasonJs {
    #[serde(rename = "breakpoint")]
    Breakpoint { id: u32, name: String },
    #[serde(rename = "watchpoint")]
    Watchpoint { global_idx: u32 },
    #[serde(rename = "choices")]
    Choices,
    #[serde(rename = "step")]
    Step,
    #[serde(rename = "terminal")]
    Terminal,
    #[serde(rename = "noStepOutTarget")]
    NoStepOutTarget,
    /// #3264: a line-granular step was asked for on an artifact that
    /// cannot say which line execution is on.
    #[serde(rename = "noLineInfo")]
    NoLineInfo,
}

/// Wasm mirror of `brink_runtime::DebugRunOutcome` — the result of
/// `debugRun`/`debugStep`.
#[derive(Serialize)]
pub(crate) struct DebugRunOutcomeJs {
    pub reason: DebugStopReasonJs,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<DebugPositionJs>,
    pub depth: usize,
}

pub(crate) fn debug_run_outcome_to_js(o: brink_runtime::DebugRunOutcome) -> DebugRunOutcomeJs {
    use brink_runtime::DebugStopReason;
    DebugRunOutcomeJs {
        reason: match o.reason {
            DebugStopReason::Breakpoint { id, name } => DebugStopReasonJs::Breakpoint { id, name },
            DebugStopReason::Watchpoint { global_idx } => {
                DebugStopReasonJs::Watchpoint { global_idx }
            }
            DebugStopReason::Choices => DebugStopReasonJs::Choices,
            DebugStopReason::Step => DebugStopReasonJs::Step,
            DebugStopReason::Terminal => DebugStopReasonJs::Terminal,
            DebugStopReason::NoStepOutTarget => DebugStopReasonJs::NoStepOutTarget,
            DebugStopReason::NoLineInfo => DebugStopReasonJs::NoLineInfo,
        },
        position: o.position.map(|p| DebugPositionJs {
            container_idx: p.container_idx,
            offset: p.offset,
        }),
        depth: o.depth,
    }
}

/// Parse a `debugStep` mode string ("into" | "over" | "out") into
/// `brink_runtime::StepMode`. Any other string is a `JsError` — a bad mode
/// name is a caller bug, not a runtime outcome.
pub(crate) fn parse_step_mode(mode: &str) -> Result<brink_runtime::StepMode, JsError> {
    match mode {
        "into" => Ok(brink_runtime::StepMode::Into),
        "over" => Ok(brink_runtime::StepMode::Over),
        "out" => Ok(brink_runtime::StepMode::Out),
        other => Err(JsError::new(&format!(
            "unknown debug step mode: {other} (expected \"into\" | \"over\" | \"out\")"
        ))),
    }
}

pub(crate) fn line_to_js(step: brink_runtime::Step) -> LineJs {
    match step {
        brink_runtime::Step::Line(line) => LineJs {
            r#type: "text",
            text: line.text,
            tags: line.tags,
            block_id: Some(line.block_id.0),
            element: Some(ElementJs {
                kind: line.element.kind,
                data: line.element.data,
            }),
            choices: None,
            name: None,
        },
        // Terminals carry no payload of their own (`docs/prose-dialect-spec.md`
        // §7, RULED) — any trailing content already arrived as its own
        // preceding `"text"`-typed `LineJs`. `text`/`tags` are always empty
        // here now; kept (rather than made optional) so existing consumers
        // that always read `.text`/`.tags` see an empty string/array instead
        // of `undefined`.
        brink_runtime::Step::Choices(choices) => LineJs {
            r#type: "choices",
            text: String::new(),
            tags: Vec::new(),
            block_id: None,
            element: None,
            choices: Some(
                choices
                    .into_iter()
                    .map(|c| ChoiceJs {
                        text: c.text,
                        index: c.index,
                        tags: c.tags,
                    })
                    .collect(),
            ),
            name: None,
        },
        brink_runtime::Step::Done => LineJs {
            r#type: "done",
            text: String::new(),
            tags: Vec::new(),
            block_id: None,
            element: None,
            choices: None,
            name: None,
        },
        brink_runtime::Step::End => LineJs {
            r#type: "end",
            text: String::new(),
            tags: Vec::new(),
            block_id: None,
            element: None,
            choices: None,
            name: None,
        },
        // FS-3w (`docs/flow-suspension-spec.md` §10.1): a flow parked at an
        // `await`. Runtime-unreachable until FS-3r — the E052 fence keeps
        // `await` from producing bytecode — but its marshal leg ships now so
        // the `@brink-lang/web` `Line` union carries `"suspended"` and hosts
        // migrate the API shape early.
        brink_runtime::Step::Suspended => LineJs {
            r#type: "suspended",
            text: String::new(),
            tags: Vec::new(),
            block_id: None,
            element: None,
            choices: None,
            name: None,
        },
    }
}

/// Wire mirror of [`brink_runtime::Step`]. Named `LineJs` (not `StepJs`)
/// deliberately, predating #1684's `Line`→`Step` rename: the wire's own
/// `"type"` discriminant for a content step is the string `"text"` (see
/// [`line_to_js`]), and `@brink-lang/web`'s `Line` union
/// (`packages/wasm-types/src/index.ts`) keeps that name too — renaming
/// this internal struct would create a mismatch with the wire contract's
/// actual vocabulary, not fix one. `session.rs`'s `StepOutcomeJs::Line`
/// variant is the same story: its `rename_all = "snake_case"` serializes
/// to `"line"`, which is the wire's real envelope discriminant.
#[derive(Serialize)]
pub(crate) struct LineJs {
    pub(crate) r#type: &'static str,
    pub(crate) text: String,
    pub(crate) tags: Vec<String>,
    /// The run of adjacent content this line belongs to
    /// (`brink_runtime::BlockId`, §3.7/§8d.2) — `Some` only for `"text"`;
    /// terminals carry no line payload, so no block id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) block_id: Option<u64>,
    /// This line's classification (`brink_runtime::Element`, issue #1683)
    /// — `Some` only for `"text"`, mirroring `block_id` above. Today every
    /// line reports the degenerate `{kind: "narrative", data: {}}` case —
    /// see `brink_runtime::Element`'s doc for what's not yet wired.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) element: Option<ElementJs>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) choices: Option<Vec<ChoiceJs>>,
    /// External name for the `awaiting_external` variant; omitted otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<String>,
}

/// Wire mirror of [`brink_runtime::Element`].
#[derive(Serialize)]
pub(crate) struct ElementJs {
    pub(crate) kind: String,
    pub(crate) data: BTreeMap<String, String>,
}

#[derive(Serialize)]
pub(crate) struct ChoiceJs {
    pub(crate) text: String,
    pub(crate) index: usize,
    pub(crate) tags: Vec<String>,
}

// ── Value ↔ native-JS boundary (T1a-3 / #525) ────────────────────────
//
// `value_to_js`/`js_to_value` are the native-JS external-binding boundary.
// They construct/inspect `JsValue`s (which panic off wasm32), so these tests
// are wasm32-gated and run under `wasm-pack test --node`. They lock the
// collection marshaling: arrays round-trip losslessly; a map round-trips with
// the documented string-key coercion (the lossless form is `TypedValueJs`).
#[cfg(all(test, target_arch = "wasm32"))]
mod value_marshal_wasm_tests {
    use super::{js_to_value, value_to_js};
    use brink_format::{MapKey, OrderedMap, Value};
    use wasm_bindgen::JsValue;
    use wasm_bindgen_test::wasm_bindgen_test;

    #[wasm_bindgen_test]
    fn array_round_trips_losslessly() {
        let original = Value::array(vec![
            Value::Int(1),
            Value::from("two"),
            Value::Bool(true),
            Value::array(vec![Value::Int(9)]),
        ]);
        let js = value_to_js(&original);
        assert!(js_sys::Array::is_array(&js), "array marshals to a JS array");
        let back = js_to_value(&js);
        assert_eq!(back, original, "array round-trips through the JS boundary");
    }

    #[wasm_bindgen_test]
    fn plain_object_becomes_a_map_with_string_keys() {
        // Build `{ name: "goblin", hp: 12 }` in JS and read it as a Map.
        let obj = js_sys::Object::new();
        let _ = js_sys::Reflect::set(
            &obj,
            &JsValue::from_str("name"),
            &JsValue::from_str("goblin"),
        );
        let _ = js_sys::Reflect::set(&obj, &JsValue::from_str("hp"), &JsValue::from_f64(12.0));
        let v = js_to_value(&obj);
        let map = v.as_map().expect("object marshals to a Map");
        assert_eq!(map.get(&MapKey::from("name")), Some(&Value::from("goblin")));
        assert_eq!(map.get(&MapKey::from("hp")), Some(&Value::Int(12)));
    }

    #[wasm_bindgen_test]
    fn map_round_trips_with_string_key_coercion() {
        // Int/bool keys stringify at the native boundary; on the way back they
        // are string keys. Values (including nesting) survive.
        let m: OrderedMap = [
            (MapKey::from("s"), Value::Int(1)),
            (MapKey::from(2), Value::from("two")),
        ]
        .into_iter()
        .collect();
        let js = value_to_js(&Value::map(m));
        let back = js_to_value(&js);
        let map = back.as_map().expect("map");
        assert_eq!(map.get(&MapKey::from("s")), Some(&Value::Int(1)));
        // The int key `2` came back as the string key "2".
        assert_eq!(map.get(&MapKey::from("2")), Some(&Value::from("two")));
    }

    #[wasm_bindgen_test]
    fn a_function_folds_to_null_not_a_map() {
        // Functions are objects but carry no data — they must not become maps.
        let f = js_sys::Function::new_no_args("return 1");
        assert_eq!(js_to_value(&f), Value::Null);
    }

    // ── Handle marshal (T1d, docs/t1d-spec.md §2/§6) ────────────────────────
    //
    // The #667 wildcard-arm hazard class: a Handle must never fall through
    // `value_to_js`'s trailing `_ => JsValue::NULL` arm the way a Record once
    // did (PR #664). These lock the marshaled encoding as faithful (kind/id
    // both readable back out, exactly) and document the deliberate
    // asymmetry with `js_to_value` (see that function's doc comment).

    #[wasm_bindgen_test]
    fn handle_marshals_to_a_non_null_object_not_a_map_wildcard() {
        let h = Value::handle(brink_format::NameId(7), 42);
        let js = value_to_js(&h);
        assert!(!js.is_null(), "Handle must not marshal to null");
        assert!(js.is_object());
        assert!(
            !js_sys::Array::is_array(&js),
            "Handle marshals to a plain object, not an array"
        );
    }

    #[wasm_bindgen_test]
    fn handle_marshal_round_trips_kind_and_id_losslessly() {
        // The marshaled encoding carries kind (as its raw NameId) and id (as
        // an exact decimal string, not an f64 `number`, so no u64 magnitude
        // loses precision) — readable back out via plain JS property access.
        let h = Value::handle(brink_format::NameId(7), u64::MAX);
        let js = value_to_js(&h);
        let kind = js_sys::Reflect::get(&js, &JsValue::from_str("kind")).expect("kind");
        assert_eq!(kind.as_f64(), Some(7.0));
        let id = js_sys::Reflect::get(&js, &JsValue::from_str("id")).expect("id");
        assert_eq!(
            id.as_string().as_deref(),
            Some(u64::MAX.to_string().as_str())
        );
    }

    #[wasm_bindgen_test]
    fn handle_marshaled_object_does_not_reconstruct_via_js_to_value() {
        // Deliberate asymmetry (js_to_value's doc comment): a marshaled
        // handle read back through js_to_value is a plain Map, not a Handle
        // — reconstructing a Handle from an arbitrary JS-shaped object would
        // let any binding forge a capability token.
        let h = Value::handle(brink_format::NameId(1), 99);
        let js = value_to_js(&h);
        let back = js_to_value(&js);
        assert!(back.as_handle().is_none());
        assert!(back.as_map().is_some());
    }
}

/// Wasm-side tests for [`value_to_js`]'s record arm (needs `js_sys`, so
/// wasm32-gated like every other `wasm_bindgen_test` in this crate).
#[cfg(test)]
mod record_marshal_tests {
    use super::value_to_js;
    use brink_format::{ShapeId, Value};
    use std::sync::Arc;
    use wasm_bindgen::JsCast;
    use wasm_bindgen_test::wasm_bindgen_test;

    #[wasm_bindgen_test]
    fn record_marshals_to_field_value_array_not_null() {
        // Regression guard for the silent-drop wildcard: a Record must never
        // fall through to the `null` arm (PR #664 review finding).
        let record = Value::Record {
            shape: ShapeId(0),
            fields: Arc::new(vec![Value::Int(3), Value::String("y".into())]),
        };
        let js = value_to_js(&record);
        assert!(!js.is_null(), "Record must not marshal to null");
        let arr: js_sys::Array = js.unchecked_into();
        assert_eq!(arr.length(), 2);
        assert_eq!(arr.get(0).as_f64(), Some(3.0));
        assert_eq!(arr.get(1).as_string().as_deref(), Some("y"));
    }
}
