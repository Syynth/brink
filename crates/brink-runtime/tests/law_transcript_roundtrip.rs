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
//! ## `Fragment::tags` — fixed (#953)
//!
//! `Fragment::tags` used to **not** round-trip: `write_transcript` never
//! serialized a fragment's `tags` field at all, and `read_transcript`
//! unconditionally reconstructed every decoded fragment with `tags:
//! Vec::new()`. This was live, populated data in production
//! (`OutputBuffer::end_fragment`/`push_fragment_tag`,
//! `crates/brink-runtime/src/output.rs`) — a real silent-data-drop bug per
//! `CLAUDE.md`'s "flag silent data drops" rule, found by building this law.
//! Fixed by appending a trailing tag section to the `.brkt` body (after the
//! fragment section) rather than changing the per-fragment record layout —
//! see `write_transcript`/`read_transcript`'s comments in
//! `crates/brink-runtime/src/transcript.rs` — so pre-fix `.brkt` files (with
//! no tag section) keep decoding, falling back to empty tags exactly as
//! before. The main round-trip law below now compares the full `Fragment`
//! (parts *and* tags), and `fragment_tags_round_trip_through_transcript_codec`
//! pins the fix as a named regression test.
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

/// Structural exhaustiveness guard (issue #1521, mirroring the identical
/// guard `brink-format`'s `proptest_inkb.rs`/`proptest_inkt.rs` added for
/// #667/#883): a match over every current [`OutputPart`] variant with **no
/// wildcard arm**, so this fails to compile the moment a new variant is
/// added to the enum. Never called — the only purpose is the compile-time
/// forcing function: `arb_output_part` below used to be a free-standing
/// `prop_oneof!` over 7 hand-listed variants, so a newly added `OutputPart`
/// variant got zero generated coverage and the round-trip law stayed green
/// having tested nothing about it. Whoever adds an `OutputPart` variant must
/// now also add an arm here — and either extend `arb_output_part` to
/// generate it, or (like `Checkpoint` below) document why it is deliberately
/// excluded. This matters now: the `.inkb` v6 bump is expected to add new
/// part kinds (`docs/prose-dialect-spec.md`).
#[expect(dead_code, reason = "compile-time-only exhaustiveness guard, see doc")]
fn assert_output_part_variants_exhaustive(part: &OutputPart) {
    match part {
        OutputPart::Text(_)
        | OutputPart::LineRef { .. }
        | OutputPart::ValueRef(_)
        | OutputPart::Newline
        | OutputPart::Spring
        | OutputPart::Glue
        | OutputPart::Tag(_)
        // `Checkpoint` is deliberately excluded from `arb_output_part` —
        // `write_transcript` filters every `Checkpoint` out on write
        // (transient capture marker, never meant to persist; see the
        // module doc above and `checkpoint_filtered_on_write` in
        // `transcript.rs`), so there is nothing for the round-trip law to
        // generate coverage for. It stays listed in this match (rather
        // than a wildcard) so the guard still trips if `Checkpoint` is
        // ever removed or split.
        | OutputPart::Checkpoint => {}
    }
}

/// One `Fragment` — a small run of parts plus tags. `tags` is generated
/// (including the empty case) so the round-trip law below exercises the
/// full `Fragment` — parts *and* tags — pinning the #953 fix (module doc).
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
            prop_assert_eq!(&decoded.parts, &original.parts);
            prop_assert_eq!(&decoded.tags, &original.tags);
        }
    }
}

/// Pins the #953 fix as a named regression test: `Fragment::tags` now
/// round-trips through the `.brkt` transcript codec. If this test starts
/// failing, the trailing tag section (`write_transcript`/`read_transcript`
/// in `crates/brink-runtime/src/transcript.rs`) has regressed.
#[test]
fn fragment_tags_round_trip_through_transcript_codec() {
    let fragment = Fragment {
        parts: vec![OutputPart::Text("hello".to_string())],
        tags: vec!["a_tag".to_string()],
    };
    let bytes = write_transcript(&[], 0, std::slice::from_ref(&fragment));
    let data = read_transcript(&bytes).expect("well-formed transcript must decode");

    assert_eq!(data.fragments.len(), 1);
    assert_eq!(data.fragments[0].parts, fragment.parts, "parts round-trip");
    assert_eq!(
        data.fragments[0].tags, fragment.tags,
        "tags round-trip (fixed by #953)"
    );
}
