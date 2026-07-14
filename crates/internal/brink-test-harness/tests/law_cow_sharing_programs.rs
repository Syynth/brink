//! Law: **sharing is unobservable**, at the compiled-program level — issue
//! #672 workstream B item 1 ("randomized programs proving no observable
//! difference between shared and deep-copied values").
//!
//! `docs/value-model-spec.md` §3: "Programs and hosts can never distinguish
//! two structurally equal values — no pointer identity, no refcounts, no
//! copy timing." `crates/internal/brink-format/tests/law_cow_sharing.rs`
//! proves this at the `Value` primitive level (direct `array_make_mut`/
//! `map_make_mut`/`record_make_mut` calls); `tests/proptest_t1b.rs` already
//! proves it for arrays at the compiled-program level (`copy_then_mutate_…`,
//! `copy_then_push_…`: `b = a; b[...] = v` / `push(b, v)` never perturbs
//! `a`). This file extends that *program*-level law to the two collection
//! kinds `proptest_t1b.rs` doesn't cover: `Value::Map` and `Value::Record`
//! (struct) — `VAR b = a` copies the value under §2 ("data is values"), and
//! a subsequent indexed/field write to `b` must never reach `a`, regardless
//! of whatever `Arc` sharing the VM happens to use underneath (§5).
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

    /// `b = a` then `b[key] = v` on a compiled brink program never changes
    /// `a` — the sharing-unobservable law (§3) applied to `Value::Map`.
    #[test]
    fn map_copy_then_index_write_never_observes_through_original(
        keys in prop::collection::vec("[a-e]", 1..6),
        write_key_idx in 0usize..6,
        new_val in -1000i32..1000,
    ) {
        let write_key = keys[write_key_idx % keys.len()].clone();

        let mut original: Vec<(String, i32)> = Vec::new();
        for (i, k) in keys.iter().enumerate() {
            ordered_insert(&mut original, k.clone(), i32::try_from(i).unwrap());
        }
        let mut mutated = original.clone();
        ordered_insert(&mut mutated, write_key.clone(), new_val);

        let entries: Vec<String> = keys
            .iter()
            .enumerate()
            .map(|(i, k)| format!("\"{k}\": {i}"))
            .collect();
        let source = format!(
            "VAR a = 0\nVAR b = 0\nVAR out_a = \"\"\nVAR out_b = \"\"\n~ {{\n    a = #{{{}}}\n    b = a\n    b[\"{write_key}\"] = {new_val}\n    for k in a {{\n        out_a = out_a + k + \":\" + a[k] + \" \"\n    }}\n    for k in b {{\n        out_b = out_b + k + \":\" + b[k] + \" \"\n    }}\n}}\n{{out_a}}\nSPLIT\n{{out_b}}\n-> END\n",
            entries.join(", "),
        );
        let mut story = compile(&source);
        let out = run_to_completion(&mut story);
        let mut parts = out.split("SPLIT");
        let a_text = parts.next().unwrap_or_default();
        let b_text = parts.next().unwrap_or_default();

        let expected_a = render_entries(&original);
        let expected_b = render_entries(&mutated);
        prop_assert_eq!(a_text.trim(), expected_a.trim());
        prop_assert_eq!(b_text.trim(), expected_b.trim());
    }

    /// `b = a` then `b.field = v` on a compiled brink program never changes
    /// `a` — the sharing-unobservable law (§3) applied to `Value::Record`.
    #[test]
    fn struct_copy_then_field_write_never_observes_through_original(
        x0 in -1000i32..1000,
        y0 in -1000i32..1000,
        new_x in -1000i32..1000,
    ) {
        let original = (x0, y0);
        let mutated = (new_x, y0);

        let source = format!(
            "{POINT_STRUCT}VAR a = 0\nVAR b = 0\n~ {{\n    a = Point#{{x: {x0}, y: {y0}}}\n    b = a\n    b.x = {new_x}\n}}\n{{a.x}} {{a.y}}\nSPLIT\n{{b.x}} {{b.y}}\n-> DONE\n",
        );
        let mut story = compile(&source);
        let out = run_to_completion(&mut story);
        let mut parts = out.split("SPLIT");
        let a_text = parts.next().unwrap_or_default();
        let b_text = parts.next().unwrap_or_default();

        prop_assert_eq!(a_text.trim(), format!("{} {}", original.0, original.1));
        prop_assert_eq!(b_text.trim(), format!("{} {}", mutated.0, mutated.1));
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
