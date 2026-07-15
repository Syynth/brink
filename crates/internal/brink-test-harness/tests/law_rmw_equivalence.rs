//! Law: **RMW is equivalent to manual take/mutate/write-back** — issue #672
//! workstream B item 3.
//!
//! `docs/value-model-spec.md` §5 specifies the mechanism: "variable
//! read-modify-write compiles to *take out of slot → `make_mut` → write
//! back*". `tests/proptest_t1b.rs` and `tests/take_rmw.rs` already prove this
//! law for array element writes (`a[i] = v`, chained `grid[y][x] = v`) and
//! the array mutator stdlib (`push`/`insert`/`remove`) against a `Vec`
//! reference. This file extends the same law to the two RMW target shapes
//! those files don't cover:
//!
//! - **Struct field write** (`p.field = v`, `p.field += v` —
//!   `docs/value-model-spec.md` §7's `ref`/projection discussion covers the
//!   general case; a single-segment field write is
//!   `lower_single_level_field_write`'s RMW discipline, the record analogue
//!   of `array_make_mut`) against a manual struct-tuple take/mutate/
//!   write-back reference.
//! - **Map index write** (`m[key] = v`) against a manual insertion-order
//!   `Vec<(String, i32)>` reference matching `OrderedMap::insert`'s
//!   overwrite-in-place semantics (value-model-spec §4).
//!
//! (`arr[i].field = v` — an index then a field on the *same* write target —
//! is deliberately not tested here: it's `E074`, "chained field-write
//! projection … not supported", a real compile error the T1e boundary
//! fences off; `crates/brink-compiler/tests/e0xx_diagnostics.rs`'s
//! `e074_chained_field_write_projection` already proves that diagnostic
//! fires. A law suite for an unsupported construct would just be a
//! not-a-law.)
//!
//! Deterministic seeds (house determinism rule): `ProptestConfig` fixes the
//! case count and reads no `PROPTEST_*` env override.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod law_support;

use law_support::{compile, run_to_completion};
use proptest::prelude::*;

const POINT_STRUCT: &str = "STRUCT Point = #{\n    x: int,\n    y: int,\n}\n";

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// `p.field = v` matches a manual take/mutate/write-back on an
    /// equivalent `(i32, i32)` reference — the single-level struct-field RMW
    /// law.
    #[test]
    fn struct_field_write_matches_manual_rmw(
        x0 in -1000i32..1000,
        y0 in -1000i32..1000,
        new_x in -1000i32..1000,
    ) {
        // Reference: take the pair out, mutate the field, write it back.
        let mut reference = (x0, y0);
        reference.0 = new_x;

        let source = format!(
            "{POINT_STRUCT}VAR p = 0\n~ {{\n    p = Point#{{x: {x0}, y: {y0}}}\n    p.x = {new_x}\n}}\n{{p.x}} {{p.y}}\n-> DONE\n",
        );
        let mut story = compile(&source);
        let out = run_to_completion(&mut story);
        prop_assert_eq!(out.trim(), format!("{} {}", reference.0, reference.1));
    }

    /// `p.field += v` (a compound-assign RMW) matches the manual
    /// take/mutate/write-back reference doing `reference.0 += delta`.
    #[test]
    fn struct_field_compound_assign_matches_manual_rmw(
        x0 in -1000i32..1000,
        y0 in -1000i32..1000,
        delta in -1000i32..1000,
    ) {
        let mut reference = (x0, y0);
        reference.0 += delta;

        let source = format!(
            "{POINT_STRUCT}VAR p = 0\n~ {{\n    p = Point#{{x: {x0}, y: {y0}}}\n    p.x += {delta}\n}}\n{{p.x}} {{p.y}}\n-> DONE\n",
        );
        let mut story = compile(&source);
        let out = run_to_completion(&mut story);
        prop_assert_eq!(out.trim(), format!("{} {}", reference.0, reference.1));
    }

    /// `m[key] = v` matches a manual insertion-order `Vec<(String, i32)>`
    /// reference with `OrderedMap::insert`'s semantics (value-model-spec §4:
    /// "re-inserting an existing key overwrites its value in place, keeping
    /// the key's original position"). `write_key` is drawn from a wider
    /// range (`[a-j]`) than `keys` (`[a-e]`), so about half the generated
    /// cases exercise an overwrite (`write_key` already in `keys`) and about
    /// half exercise a fresh-key insert (`write_key` not in `keys`) — issue
    /// #856, ruled 2026-07-15: `m[newKey] = v` inserts (JS/Python
    /// semantics) rather than faulting `MapKeyNotFound`, matching
    /// `insert`/`push`'s existing insert-on-absent behavior
    /// (`proptest_t1b.rs`).
    #[test]
    fn map_index_write_matches_manual_ordered_insert(
        keys in prop::collection::vec("[a-e]", 1..6),
        write_key in "[a-j]",
        new_val in -1000i32..1000,
    ) {
        let mut reference: Vec<(String, i32)> = Vec::new();
        for (i, k) in keys.iter().enumerate() {
            ordered_insert(&mut reference, k.clone(), i32::try_from(i).unwrap());
        }
        ordered_insert(&mut reference, write_key.clone(), new_val);

        let entries: Vec<String> = keys
            .iter()
            .enumerate()
            .map(|(i, k)| format!("\"{k}\": {i}"))
            .collect();
        let source = format!(
            "VAR m = 0\nVAR out = \"\"\n~ {{\n    m = #{{{}}}\n    m[\"{write_key}\"] = {new_val}\n    for k in m {{\n        out = out + k + \":\" + m[k] + \" \"\n    }}\n}}\n{{out}}\n-> END\n",
            entries.join(", "),
        );
        let mut story = compile(&source);
        let out = run_to_completion(&mut story);

        let expected = render_entries(&reference);
        prop_assert_eq!(out.trim(), expected.trim());
    }
}

/// `OrderedMap::insert`'s semantics (value-model-spec §4): overwrite the
/// value at the key's original position if present, otherwise append.
fn ordered_insert(entries: &mut Vec<(String, i32)>, key: String, value: i32) {
    if let Some(entry) = entries.iter_mut().find(|(k, _)| *k == key) {
        entry.1 = value;
    } else {
        entries.push((key, value));
    }
}

/// Render `entries` as `"k0:v0 k1:v1 … "` — the reference-side mirror of the
/// `for k in m { out = out + k + ":" + m[k] + " " }` accumulation the
/// generated program performs.
fn render_entries(entries: &[(String, i32)]) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    for (k, v) in entries {
        let _ = write!(out, "{k}:{v} ");
    }
    out
}
