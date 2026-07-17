//! Law: **transcript round-trip** over arbitrary output parts and values —
//! issue #746 item 1, the residue #738 left of #672 workstream B's
//! serialization-round-trip laws. `transcript.rs`'s own `#[cfg(test)]`
//! module already hand-picks a handful of fixtures (`round_trip_value_ref_*`,
//! `round_trip_line_ref_with_slots`, …); this suite generalizes that to
//! arbitrary generated `Vec<OutputPart>`/`Value`/`Fragment` content, proving
//! `write_transcript` → `read_transcript` is the identity over the whole
//! reachable shape, not just the fixtures someone thought to hand-write.
//!
//! ## Two known, deliberate exclusions
//!
//! - **`Value::TempPointer`**: `write_transcript`'s own doc comment says it
//!   plainly — "`TempPointer` is runtime-only" — and the encoder collapses
//!   it to `VAL_NULL` on write (a temp-frame pointer has no meaning once
//!   detached from the frame that produced it, so there is nothing durable
//!   to write). This is intentional, documented lossy behavior, not a round
//!   -trip bug, so the generator below never produces one.
//! - **`Checkpoint`**: `write_transcript` filters every `OutputPart::Checkpoint`
//!   out on write (transient capture markers, never meant to persist,
//!   already proven by the existing `checkpoint_filtered_on_write` unit
//!   test) — the generator never produces one either, so "decoded parts ==
//!   original parts" holds as a plain identity rather than needing a
//!   filter-then-compare dance.
//!
//! ## One real finding, deliberately left unfixed here (see PR scope notes)
//!
//! `Fragment::tags` does **not** round-trip: `write_transcript` never
//! serializes a fragment's `tags` field at all, and `read_transcript`
//! unconditionally reconstructs every decoded fragment with `tags:
//! Vec::new()`. This is live, populated data in production
//! (`OutputBuffer::end_fragment`/`push_fragment_tag`,
//! `crates/brink-runtime/src/output.rs`) — a real silent-data-drop bug per
//! `CLAUDE.md`'s "flag silent data drops" rule, found by building this law.
//! It is **not** a value-model-spec §4 concern (it's the `.brkt` wire
//! layout for `Fragment`, not `Value` semantics), and fixing it changes the
//! per-fragment byte layout — a `.brkt` wire-format change wants its own
//! versioning/back-compat consideration, out of scope for this test-only
//! issue. So: the main round-trip law below compares `Fragment::parts` only
//! (documented exclusion, not an oversight), and
//! `fragment_tags_do_not_round_trip` pins the gap as a named, tracked
//! regression rather than a silent one.
//!
//! Reproducibility (house determinism rule, `CLAUDE.md`): proptest's default
//! RNG is entropy-seeded per run, not fixed — generated cases differ run to
//! run. Reproducibility instead comes from `ProptestConfig::with_cases`
//! (a fixed, deterministic *count* of cases every run) and from proptest's
//! own failure-persistence file (`.proptest-regressions`), which pins the
//! exact seed of any failing case for replay. Set `PROPTEST_RNG_SEED` if
//! bit-for-bit seed reproducibility across every run — not just failures —
//! is ever required.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use brink_format::{
    ClosureEnvEntry, DefinitionId, DefinitionTag, LineFlags, ListValue, MapKey, NameId, OrderedMap,
    ProjSegment, ShapeId, Value,
};
use brink_runtime::transcript::{read_transcript, write_transcript};
use brink_runtime::{Fragment, OutputPart};
use proptest::prelude::*;

// ── Strategies ───────────────────────────────────────────────────────────

fn arb_tag() -> impl Strategy<Value = DefinitionTag> {
    prop_oneof![
        Just(DefinitionTag::Address),
        Just(DefinitionTag::GlobalVar),
        Just(DefinitionTag::ListDef),
        Just(DefinitionTag::ListItem),
        Just(DefinitionTag::ExternalFn),
    ]
}

fn arb_def_id() -> impl Strategy<Value = DefinitionId> {
    (arb_tag(), any::<u64>()).prop_map(|(tag, hash)| DefinitionId::new(tag, hash))
}

fn arb_name_id() -> impl Strategy<Value = NameId> {
    any::<u16>().prop_map(NameId)
}

fn arb_map_key() -> impl Strategy<Value = MapKey> {
    prop_oneof![
        any::<i32>().prop_map(MapKey::Int),
        "[a-z]{0,6}".prop_map(|s: String| MapKey::Str(s.into())),
        any::<bool>().prop_map(MapKey::Bool),
    ]
}

/// One `ProjSegment` — an index or a scalar map key, never a full nested
/// `Value` (mirrors `proptest_inkb.rs`'s `arb_proj_segment`: segment values
/// are always scalar in practice).
fn arb_proj_segment() -> impl Strategy<Value = ProjSegment> {
    prop_oneof![
        any::<i32>().prop_map(ProjSegment::Index),
        any::<i32>().prop_map(|n| ProjSegment::Key(Value::Int(n))),
        "[a-z]{0,6}".prop_map(|s: String| ProjSegment::Key(Value::String(s.into()))),
    ]
}

/// Leaf values the transcript codec can encode losslessly (every `Value`
/// variant except `TempPointer` — see module doc).
fn arb_value_leaf() -> impl Strategy<Value = Value> {
    prop_oneof![
        any::<i32>().prop_map(Value::Int),
        any::<f32>().prop_map(Value::Float),
        any::<bool>().prop_map(Value::Bool),
        "[a-z]{0,8}".prop_map(|s: String| Value::String(s.into())),
        (
            prop::collection::vec(arb_def_id(), 0..3),
            prop::collection::vec(arb_def_id(), 0..3),
        )
            .prop_map(|(items, origins)| Value::List(ListValue { items, origins }.into())),
        arb_def_id().prop_map(Value::DivertTarget),
        arb_def_id().prop_map(Value::VariablePointer),
        Just(Value::Null),
        any::<u32>().prop_map(Value::FragmentRef),
        arb_def_id().prop_map(Value::FnRef),
        (arb_name_id(), any::<u64>()).prop_map(|(kind, id)| Value::handle(kind, id)),
    ]
}

/// Every `Value` variant the transcript codec supports, `Array`/`Map`/
/// `Record`/`Closure`/`Projection` nested to a bounded depth (matches the
/// shape of `law_support::arb_value_full` in `brink-format`, reimplemented
/// here since this test lives in a different crate with its own dev-dep on
/// `proptest` and no path to that internal `tests/` module).
fn arb_value() -> impl Strategy<Value = Value> {
    arb_value_leaf().prop_recursive(3, 16, 4, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(Value::array),
            prop::collection::vec((arb_map_key(), inner.clone()), 0..4).prop_map(|entries| {
                let mut map = OrderedMap::new();
                for (key, value) in entries {
                    map.insert(key, value);
                }
                Value::map(map)
            }),
            (any::<u32>(), prop::collection::vec(inner.clone(), 0..4))
                .prop_map(|(shape, fields)| Value::record(ShapeId(shape), fields)),
            (
                arb_def_id(),
                prop::collection::vec((arb_name_id(), any::<bool>(), inner.clone()), 0..3),
            )
                .prop_map(|(target, raw_env)| {
                    let env = raw_env
                        .into_iter()
                        .map(|(name, is_ref, payload)| ClosureEnvEntry {
                            name,
                            is_ref,
                            payload,
                        })
                        .collect();
                    Value::closure(target, env)
                }),
            (
                arb_def_id(),
                prop::collection::vec(arb_proj_segment(), 0..3),
            )
                .prop_map(|(cell, segments)| Value::projection(cell, segments)),
        ]
    })
}

/// One `OutputPart`. Never produces `Checkpoint` (filtered on write, see
/// module doc) or a `TempPointer` (excluded from `arb_value`). Calls
/// `arb_value()` fresh at each use site rather than threading a shared
/// strategy through — `arb_value`'s `prop_recursive` strategy isn't `Clone`,
/// so building it twice (once for `LineRef::slots`, once for `ValueRef`) is
/// simpler than fighting that bound.
fn arb_output_part() -> impl Strategy<Value = OutputPart> {
    prop_oneof![
        "[^\\x00]{0,16}".prop_map(OutputPart::Text),
        (
            any::<u32>(),
            any::<u16>(),
            any::<u8>(),
            prop::collection::vec(arb_value(), 0..3),
        )
            .prop_map(
                |(container_idx, line_idx, flag_bits, slots)| OutputPart::LineRef {
                    container_idx,
                    line_idx,
                    slots,
                    flags: LineFlags::from_bits_truncate(flag_bits),
                }
            ),
        arb_value().prop_map(OutputPart::ValueRef),
        Just(OutputPart::Newline),
        Just(OutputPart::Spring),
        Just(OutputPart::Glue),
        "[^\\x00]{0,16}".prop_map(OutputPart::Tag),
    ]
}

/// One `Fragment` — a small run of parts plus tags. `tags` is generated
/// (never empty-by-construction) specifically so the round-trip law below
/// exercises — and the dedicated regression test pins — the known
/// tags-don't-round-trip gap (module doc) rather than vacuously passing
/// because every generated fragment happened to have no tags.
fn arb_fragment() -> impl Strategy<Value = Fragment> {
    (
        prop::collection::vec(arb_output_part(), 0..6),
        prop::collection::vec("[a-z]{1,6}", 0..3),
    )
        .prop_map(|(parts, tags)| Fragment { parts, tags })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// `write_transcript` → `read_transcript` is the identity over
    /// arbitrary parts/values (and fragment `parts`), for any
    /// `source_checksum`.
    #[test]
    fn transcript_round_trips_arbitrary_parts_and_values(
        parts in prop::collection::vec(arb_output_part(), 0..12),
        fragments in prop::collection::vec(arb_fragment(), 0..4),
        source_checksum in any::<u32>(),
    ) {
        let bytes = write_transcript(&parts, source_checksum, &fragments);
        let data = read_transcript(&bytes).expect("a freshly-written transcript must decode");

        prop_assert_eq!(data.source_checksum, source_checksum);
        prop_assert_eq!(&data.parts, &parts);
        prop_assert_eq!(data.fragments.len(), fragments.len());
        for (decoded, original) in data.fragments.iter().zip(fragments.iter()) {
            // `.tags` deliberately excluded — see module doc's "known
            // finding" section and `fragment_tags_do_not_round_trip` below.
            prop_assert_eq!(&decoded.parts, &original.parts);
        }
    }
}

/// Pins the `Fragment::tags` data-drop as a named, tracked regression
/// (module doc) rather than letting the property test above silently
/// exclude it forever. If this test ever starts failing (i.e. tags do come
/// back non-empty), the gap has been fixed — update/remove this test and
/// fold `tags` into the main round-trip law's fragment comparison instead.
#[test]
fn fragment_tags_do_not_round_trip_through_transcript_codec() {
    let fragment = Fragment {
        parts: vec![OutputPart::Text("hello".to_string())],
        tags: vec!["a_tag".to_string()],
    };
    let bytes = write_transcript(&[], 0, std::slice::from_ref(&fragment));
    let data = read_transcript(&bytes).expect("well-formed transcript must decode");

    assert_eq!(data.fragments.len(), 1);
    assert_eq!(
        data.fragments[0].parts, fragment.parts,
        "parts do round-trip"
    );
    assert!(
        data.fragments[0].tags.is_empty(),
        "known gap (issue #746): Fragment::tags is silently dropped by the \
         .brkt transcript codec — write_transcript never serializes it and \
         read_transcript always reconstructs an empty Vec. If this assertion \
         starts failing, the codec has been fixed; update this test."
    );
}
