# Hand-respelled `.brink` fixtures

**Status: episode-identity proven (B0.8b, issue #1178).** These fixtures
are still hand-curated (the corpus selection itself, and the checked-in
`story.brink` text), but they are no longer merely parse-verified: the new
`brink_ir::hir::emit_native` emitter + the dev-only `brink-respell` crate
(`crates/internal/brink-respell/`) now prove, for every case here, that
(a) this exact `story.brink` round-trips through native lowering + emit +
reparse + relower to an **episode-identical** replay
(`crates/internal/brink-respell/tests/round_trip.rs`), and (b) the ink
origin listed below *mechanically* respells (no hand editing) to a
`.brink` source that is itself episode-identical to the ink original
(`crates/internal/brink-respell/tests/ink_corpus_convert.rs`). Run either
with `cargo test -p brink-respell`. See that crate's module docs for
exactly which HIR constructs the emitter currently supports — it is
conservative by construction (refuses rather than guesses on anything
outside that subset).

This is still the NF-5 (b)-recommendation "differential method" fixture
corpus: per `docs/b0-findings.md` NF-5, the respelled corpus is
**hand-curated — "a handful of cases spanning weave/choice/tunnel/thread/
alternation semantics", not a mechanical translation of all 390 oracle
cases**. Mechanizing the *full* corpus (the "full-corpus episode-identity
differential" `docs/b0-sequencing.md` names as the B0 *ratification*
gate) remains later, larger work — this slice proves the machinery on the
same 9-case seed the hand-curation already targeted, not the whole
corpus. This directory deliberately does not try to be exhaustive.

## What's here

Each subdirectory pairs one respelled `.brink` fixture with its ink
origin and oracle case name:

| Case | Family | Ink origin |
|---|---|---|
| `exhibit-fogg-passage/` | charter exhibit (§9, "the Fogg passage") | `tests/tier2/conditional/condtext-v1/story.ink` |
| `weave-options/` | weave | `tests/tier1/weave/weave-options/story.ink` |
| `sticky-choice/` | choices (sticky vs once) | `tests/tier1/choices/sticky-choice/story.ink` |
| `basic-tunnel/` | diverts/tunnels | `tests/tier1/diverts/basic-tunnel/story.ink` |
| `gather-basic/` | gather (dissolved) | `tests/tier1/gather/gather-basic/story.ink` |
| `simple-glue/` | glue | `tests/tier1/glue/simple-glue/story.ink` |
| `const-vars/` | variables | `tests/tier1/variables/const/story.ink` |
| `manual-stitch-v1/` | knot/stitch structure | `tests/tier1/stitch/manual-stitch-v1/story.ink` |
| `complex-flow-v1/` | weave + gather, deep nesting (bonus — likely the charter's own source example) | `tests/tier1/gather/complex-flow-v1/story.ink` |
| `labeled-mid-flow-gather/` | labeled gather (G-1, issue #1335) — the checked-in `story.brink` is still **hand-written, native-only, not a 1:1 ink respelling**, see its own `manifest.toml`, but the ink origin below now mechanically respells to an episode-identical `.brink` (`ink_corpus_convert.rs`'s `once_only_choices_can_link_back_to_self`) | `tests/tier1/choices/once-only-choices-can-link-back-to-self/story.ink` |
| `typed-annotations/` | `: type` annotations (NG-A/NG-B, issues #1487/#1488) — **native-only**, see its own `manifest.toml` | none (the annotation grammar is native-specific) |

Each case directory has:
- `story.brink` — the respelled native source.
- `manifest.toml` — `ink_source` (path, relative to repo root),
  `oracle_case` (matches the `tier1::<family>::<case>` naming the oracle
  snapshot tests use), `family`, `status`, and `notes` (respelling
  decisions and any caveats specific to that case).

**`FUNC_populate_options_thread`** (the charter's other named exhibit,
§9) is **not represented here** — grepped exhaustively across the tree
(source, docs, fixtures, oracle JSON); its ink original does not exist
anywhere in this repository, only its name (in the charter, in
`docs/b0-sequencing.md`, and in a B0.5 test comment recording the same
absence). No ink source was invented to fill the gap. See finding G-3
below.

## Verification

Two layers, both still run in CI:

1. **Parse-clean (zero errors), lossless round-trip** against the shipped
   native parser — `crates/internal/brink-syntax-native/tests/respell_fixtures.rs`
   walks every `story.brink` under this directory. Run with:

   ```sh
   cargo test -p brink-syntax-native --test respell_fixtures
   ```

2. **Episode-identity** (B0.8b, issue #1178) — `crates/internal/brink-respell/tests/`:
   `round_trip.rs` proves each `story.brink` here plays identically after a
   full native lower → emit → reparse → relower cycle; `ink_corpus_convert.rs`
   proves the ink origin listed above mechanically respells (via the same
   emitter, fed ink-frontend HIR instead of native HIR) to a `.brink` source
   that plays identically to the ink original. Run with:

   ```sh
   cargo test -p brink-respell
   ```

## Gap findings (feed for the native-grammar ruling batch, #1106 G1–G8)

These are constructs encountered while respelling that either have no
ruled native spelling, or where the charter is silent/ambiguous. None of
these blocked the fixtures above (each was either avoidable, or — per
task instructions — the affected case was passed over rather than
inventing surface).

**G-1 — RESOLVED (RULED 2026-07-20, "label any content line"; emitter
support landed for issue #1335, B0.8b).** Every content line now takes an
optional leading `(name)` label (`brink-syntax-native`'s
`content::at_content_label`/`label`), giving both a labeled dissolved-
gather continuation and a genuinely mid-flow labeled re-entry point a
native spelling — `Stmt::LabeledBlock`/`ChoiceSet.continuation.label`
respectively (`lower_native::body`). `emit_native.rs`'s
`emit_labeled_stmt_stream` respells both back to `.brink`; see the
`labeled-mid-flow-gather/` fixture above. Kept below for history.

**G-1 (historical) — No ruled spelling for a labeled mid-flow re-entry
point (a "named gather" as a divert target for content other than a
choice or a container's own start).** Ink lets any gather line carry a
label
(`- (start)`) and be `->`-diverted to from anywhere, including from a
point *after* it in the same weave (a loop-back). The charter dissolves
the gather entirely (§5) and only ever gives labels to *choice lines*
(`* (name) [text]`, kept, §11) — there is no ruled way to label an
arbitrary plain-content line as an addressable point. The only native
workaround is promoting that point to its own nested `flow`, which works
whenever the labeled point happens to sit at the very start of its
enclosing container (the label and the container boundary coincide) but
has no answer for a label sitting in the *middle* of a longer flow's
body, `tests/tier1/choices/default-choices/story.ink` is a real (if
small) example of the degenerate case — its `- (start)` label happens to
be the very first line of the story, so it *is* respellable by promoting
the whole body to `flow start() { ... }`; it was left out of this
fixture set for exactly that reason (it wouldn't have exercised the real
gap). A genuinely mid-flow labeled gather used as a backward-loop target
would need either a new "labeled entry point" construct or a documented
recommendation to always factor the target into its own `flow`/stitch.
Not fabricated a spelling for this — flagging it instead.

**G-2 — Choice-line content cannot embed `{expr}` interpolation on the
choice's own line without prematurely opening a `CHOICE_BODY`.** The
shipped B0.5 grammar's `choice_text` (`crates/internal/brink-syntax-native/src/parser/choice.rs`)
stops `CHOICE_START_CONTENT`/`CHOICE_INNER_CONTENT` scanning at `L_BRACE`
unconditionally (`content_items_until(p, &[..., L_BRACE])`), so a `{`
starting an interpolation on a choice line is indistinguishable from the
`{` that opens a choice body — the parser will always treat it as
opening `CHOICE_BODY` (and then fail to find `if`/`match`/`?`/alternation
markers inside, since there's a real expression there instead). This
never came up in the charter's own exhibit or in any fixture chosen here
(none needed inline interpolation directly in choice text), so no
fixture demonstrates it broken, but it means today's grammar cannot
faithfully respell an ink choice line like `* Gold: {gold}` without
rewording. This reads as B0.5 grammar-completeness debt rather than a
charter silence (the charter never discusses interpolation-in-choice-text
specifically), but it blocks a real ink idiom and is worth a ruling or a
parser fix before B0.7.

**G-3 — `FUNC_populate_options_thread` has no ink original anywhere in
this repository.** The charter (§5, §9) names it as the exhibit for the
`for`-generated-choices watch-list item ("sugar over the community's
recursive-thread generator pattern") and as one of sitting-1's two
concrete anchors. Exhaustively grepped (`FUNC_populate_options_thread`,
`populate_options_thread`) across all of `tests/`, `docs/`, and
`crates/`: the only hits are the charter text itself,
`docs/b0-sequencing.md`, and a B0.5 test-suite comment already recording
this same absence. Per this task's explicit instruction, no ink source
was invented to stand in for it. If this exhibit still matters to the
program, someone needs to either locate/author the original ink pattern
this refers to (the community recursive-thread generator idiom is a
known, documented ink technique — but the *named* function with this
exact identifier is not in-tree) or retire the exhibit reference.

## Implementation notes (not charter gaps — flagged because they're
## surprising, not because the charter is silent)

**N-1 — RESOLVED** (see `n1_affected_fixtures_parse_inline_diverts_as_divert_nodes`
in `crates/internal/brink-syntax-native/tests/respell_fixtures.rs`, and
`brink-respell`'s emitter, which relies on this fix to glue a same-line
`text -> target` back onto one output line — see that module's doc on
`emit_stmt_stream`). Kept below for history.

**N-1 (historical) — Inline diverts inside prose/choice content parse as inert text,
not as `DIVERT_STMT` nodes.** `block::body_line` only recognizes `->` as
a divert when it is the *first* token on a line
(`DIVERT => super::divert::divert_or_tunnel(p)`); `content::content_items_until`
— the loop used for `CONTENT_LINE`, `CHOICE_INNER_CONTENT`, and
alternation bodies — has no special case for `DIVERT` at all, so a `->`
that follows any prose text on the same line (e.g. `* [The wager.] ->
know_about_wager`, or `You eat another donut. -> homers_couch` inside a
choice body) is folded into a literal `TEXT` run by `text_run_until`. It
still parses with **zero errors** (this is exactly what let the charter's
own Fogg exhibit and several fixtures above pass their parse-clean gate),
but the resulting CST does not yet carry a semantic divert node at that
position. The charter is unambiguous that diverts are "kept verbatim"
including in content position (the Fogg exhibit literally spells one this
way) — so this reads as a real gap in the *shipped skeleton*, to be
closed before B0.7 needs to lower these bodies, not a charter-level
ambiguity. Every fixture in this set that contains an inline
same-line divert after choice text is affected by this (`sticky-choice`,
`exhibit-fogg-passage`, `manual-stitch-v1` — all three `* [text] ->
target` choice lines in it; the other fixtures' diverts are either
standalone lines — recognized correctly — or absent). Flagging loudly
per this task's instructions, even though it's implementation debt
rather than a missing ruling.
