//! Law: **the iterate protocol's contract** (NS-A3, issue #1109,
//! docs/stdlib-spec.md §9.6) over the closed builtin iterable set —
//! "**every element once; `none` is terminal and sticky**" — property-
//! enforced over arbitrary generated arrays and maps, not just hand-picked
//! fixtures, plus the agreement leg: the pull iterator
//! ([`brink_runtime::ValueIter`]) and the `for` desugar (`CollectionKeys`
//! index walk) observe the SAME canonical sequence, pinned end-to-end by
//! compiling and running a brink `for` loop and comparing its visit order
//! against a drain of the iterator.
//!
//! Laws:
//! 1. **Every element once, in canonical order** — draining yields exactly
//!    the array's values (identity) / the map's keys (insertion order),
//!    element for element, no repeats, no omissions.
//! 2. **`none` is terminal and sticky** — once `next()` returns `None`,
//!    every further pull returns `None` (the machine form makes this
//!    structural; the law pins it against regression).
//! 3. **Snapshot at creation (F10)** — mutating the source collection
//!    after creating the iterator never changes what the iterator yields.
//! 4. **Desugar agreement** — a compiled `for x in c { … }` visits the
//!    same elements in the same order the iterator pulls.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_format::{MapKey, OrderedMap, Value};
use brink_runtime::ValueIter;
use proptest::prelude::*;

// ─── Generators ─────────────────────────────────────────────────────────

/// Scalar element values (plus one nested-array case so the law covers
/// non-scalar elements — the iterator must treat elements opaquely).
fn arb_element() -> impl Strategy<Value = Value> {
    prop_oneof![
        any::<i32>().prop_map(Value::Int),
        any::<bool>().prop_map(Value::Bool),
        "[a-z]{0,8}".prop_map(|s| Value::String(s.into())),
        proptest::collection::vec(any::<i32>().prop_map(Value::Int), 0..3).prop_map(Value::array),
    ]
}

fn arb_array() -> impl Strategy<Value = Value> {
    proptest::collection::vec(arb_element(), 0..12).prop_map(Value::array)
}

fn arb_map_key() -> impl Strategy<Value = MapKey> {
    prop_oneof![
        any::<i32>().prop_map(MapKey::Int),
        any::<bool>().prop_map(MapKey::Bool),
        "[a-z]{0,6}".prop_map(|s| MapKey::Str(s.into())),
    ]
}

fn arb_map() -> impl Strategy<Value = OrderedMap> {
    proptest::collection::vec((arb_map_key(), arb_element()), 0..12).prop_map(|entries| {
        let mut m = OrderedMap::new();
        for (k, v) in entries {
            m.insert(k, v);
        }
        m
    })
}

fn key_to_value(k: &MapKey) -> Value {
    match k {
        MapKey::Int(n) => Value::Int(*n),
        MapKey::Str(s) => Value::String(std::sync::Arc::clone(s)),
        MapKey::Bool(b) => Value::Bool(*b),
    }
}

fn drain(it: ValueIter) -> Vec<Value> {
    let mut out = Vec::new();
    for v in it {
        out.push(v);
        assert!(out.len() <= 64, "iterator failed to terminate");
    }
    out
}

// ─── Laws 1 + 2 over arrays ─────────────────────────────────────────────

proptest! {
    #[test]
    fn array_yields_every_element_once_in_order(a in arb_array()) {
        let Value::Array(items) = &a else { unreachable!() };
        let drained = drain(ValueIter::new(&a).unwrap());
        prop_assert_eq!(&drained, items.as_ref());
    }

    #[test]
    fn array_exhaustion_is_terminal_and_sticky(a in arb_array(), extra in 1usize..8) {
        let mut it = ValueIter::new(&a).unwrap();
        while it.next().is_some() {}
        for _ in 0..extra {
            prop_assert_eq!(it.next(), None);
        }
        prop_assert_eq!(it.remaining(), 0);
    }

    #[test]
    fn map_yields_every_key_once_in_insertion_order(m in arb_map()) {
        let expected: Vec<Value> = m.keys().map(key_to_value).collect();
        let drained = drain(ValueIter::new(&Value::map(m)).unwrap());
        prop_assert_eq!(drained, expected);
    }

    #[test]
    fn map_exhaustion_is_terminal_and_sticky(m in arb_map(), extra in 1usize..8) {
        let mut it = ValueIter::new(&Value::map(m)).unwrap();
        while it.next().is_some() {}
        for _ in 0..extra {
            prop_assert_eq!(it.next(), None);
        }
    }

    // ─── Law 3: snapshot at creation (F10) ──────────────────────────────

    #[test]
    fn source_mutation_after_creation_is_invisible(a in arb_array(), pushed in any::<i32>()) {
        let Value::Array(items) = &a else { unreachable!() };
        let expected: Vec<Value> = items.as_ref().clone();
        let mut source = a.clone();
        let it = ValueIter::new(&source).unwrap();
        if let Some(items) = source.array_make_mut() {
            items.push(Value::Int(pushed));
            items.reverse();
        }
        prop_assert_eq!(drain(it), expected);
    }
}

// ─── Law 4: desugar agreement (end to end) ──────────────────────────────

/// Compile and run a brink `for` loop over a literal collection, returning
/// the loop's per-element output lines.
fn run_for_loop(collection_literal: &str) -> Vec<String> {
    // T1b mutators take their lvalue directly (`push(out, v)` — the RMW
    // discipline); the accumulator is a VAR so the whole-loop result is
    // observable through interpolation after the block.
    let source = format!(
        "VAR out = #[]\n~ {{\n    for x in {collection_literal} {{\n        push(out, string(x))\n    }}\n}}\nSTART\n{{out}}\n-> END\n"
    );
    let files: std::collections::HashMap<&str, String> =
        std::collections::HashMap::from([("main.ink", source)]);
    let options = brink_compiler::AnalysisOptions {
        dialect: brink_compiler::Dialect::Brink,
        ..brink_compiler::AnalysisOptions::default()
    };
    let output = brink_compiler::compile_with_options(
        "main.ink",
        |path| {
            files.get(path).cloned().ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::NotFound, format!("not found: {path}"))
            })
        },
        options,
    )
    .expect("compile");
    let (program, line_tables) = brink_runtime::link(&output.data).expect("link");
    let mut story = brink_runtime::Story::<brink_runtime::DotNetRng>::new(
        std::sync::Arc::new(program),
        line_tables,
    );
    let mut text = String::new();
    loop {
        match story.continue_single().expect("run") {
            brink_runtime::Line::Text { text: t, .. } => text.push_str(&t),
            brink_runtime::Line::Done { text: t, .. }
            | brink_runtime::Line::End { text: t, .. }
            | brink_runtime::Line::Suspended { text: t, .. } => {
                text.push_str(&t);
                break;
            }
            brink_runtime::Line::Choices { .. } => panic!("unexpected choices"),
        }
    }
    // Output shape: "START\n[a, b, c]\n" — parse the bracketed array back
    // into its rendered elements.
    let line = text
        .lines()
        .find(|l| l.starts_with('['))
        .expect("array output line");
    let inner = line.trim_start_matches('[').trim_end_matches(']');
    if inner.is_empty() {
        Vec::new()
    } else {
        inner.split(", ").map(str::to_string).collect()
    }
}

#[test]
fn for_desugar_and_pull_iterator_agree_on_arrays() {
    let a = Value::array(vec![
        Value::Int(3),
        Value::Int(1),
        Value::Int(2),
        Value::Int(1),
    ]);
    let pulled: Vec<Value> = ValueIter::new(&a).unwrap().collect();
    assert_eq!(
        pulled,
        vec![Value::Int(3), Value::Int(1), Value::Int(2), Value::Int(1)]
    );
    let looped = run_for_loop("#[3, 1, 2, 1]");
    assert_eq!(looped, vec!["3", "1", "2", "1"]);
}

#[test]
fn for_desugar_and_pull_iterator_agree_on_map_keys() {
    let mut m = OrderedMap::new();
    m.insert(MapKey::Str("z".into()), Value::Int(1));
    m.insert(MapKey::Str("a".into()), Value::Int(2));
    m.insert(MapKey::Str("m".into()), Value::Int(3));
    let pulled: Vec<Value> = ValueIter::new(&Value::map(m)).unwrap().collect();
    assert_eq!(
        pulled,
        vec![
            Value::String("z".into()),
            Value::String("a".into()),
            Value::String("m".into())
        ]
    );
    let looped = run_for_loop("#{\"z\": 1, \"a\": 2, \"m\": 3}");
    assert_eq!(looped, vec!["z", "a", "m"]);
}
