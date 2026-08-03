# Effects/comparator diagnostic family: dialect-gate audit (issue #2099)

**Status: NOT RULED.** This document is a measurement, not a decision — it
answers issue #2099's ask #1 ("enumerate every diagnostic … gated on
`AnalysisOptions.dialect == Dialect::Brink` with no native fallback") and lays
out the two options #2099 itself poses for ask #2, without choosing between
them. Every code coordinate below was read at `origin/main` @
`cb95872214477af7d43110a448530919c38a6bda` (2026-08-03). Follows the
`docs/live-typing-diagnostics-divergence.md` precedent (#1347): audit + a
characterization test suite that pins today's behavior either way the ruling
lands.

## 1. What #2099 asked

> should native `.brink` projects get this diagnostic family by default (an
> `is_native` fallback, mirroring `map_keys`), or is the current opt-in-only
> posture intentional and just under-documented?

This is an explicit either/or design question in the issue's own body, not a
defect report with one correct fix. Per the standing "diagnostics needing a
policy call get declined with options, not unilaterally decided" rule, this
PR does not pick (a) or (b).

## 2. Confirmed: the premise holds

`crates/internal/brink-analyzer/src/lib.rs:1255`:

```rust
if opts.dialect == Dialect::Brink && needs_effects {
```

This single gate is the only thing standing between a project and three
downstream checks — `effects_assertions::check` (E102/E103/E108/E109),
`await_purity::check` (E105), and `comparator_contract::check` (E119) — for
the monolithic `analyze`/`analyze_with_modules` path, specifically
`whole_project_diagnostics` (`lib.rs:1167-1321`). `opts.dialect` is
`brink_project_config::Dialect`, an **ink-only axis** defined at
`crates/internal/brink-project-config/src/lib.rs:134-138`
(`{StrictInk, Brink}`, `#[default]` on `StrictInk`; re-exported for
consumers at `crates/internal/brink-analyzer/src/dialect_gate.rs:51`) — a
native `.brink` project has no opinion on it at all, so it silently carries
whatever `AnalysisOptions::default()` gives it (`StrictInk`) unless
something explicitly sets `dialect = brink` in `brink.toml` or `--dialect
brink` — a flag that means nothing for native source.

The incremental salsa path (`brink-db`, what `brink compile`/`brink
check`/`@brink-lang/web` actually run) mirrors the same gate at each of the
three query boundaries, verbatim:

- `crates/internal/brink-db/src/queries/analysis.rs:330` —
  `effects_assertion_diagnostics_query` (E102/E103/E108/E109)
- `crates/internal/brink-db/src/queries/analysis.rs:376` —
  `await_purity_diagnostics_query` (E105)
- `crates/internal/brink-db/src/queries/analysis.rs:599` —
  `comparator_contract_diagnostics_query` (E119)

All three begin:

```rust
if project.analysis_options(db).dialect != brink_analyzer::Dialect::Brink {
    return Arc::new(Vec::new());
}
```

None of the three consult `is_native` / the file's `Language` at all — unlike
a sibling gate elsewhere in `lib.rs`:

```rust
// lib.rs:844
if dialect == Dialect::Brink || is_native {
    // structs::check_duplicates (E084), map_keys::check (E106),
    // map_keys::check_duplicate_keys (E138)
```

**Correction (this gate is further away than an earlier draft of this audit
claimed):** `lib.rs:844` sits inside `per_file_diagnostics` (`lib.rs:785-888`)
— a **different pass** from `whole_project_diagnostics` (`lib.rs:1167-1321`),
which is where the effects gate at `lib.rs:1255` lives. They are 411 lines
and a pass boundary apart, not "four lines away" — an earlier version of
this section conflated the two. The two passes also source `is_native`
differently, which matters for what "mirroring `map_keys`" would actually
require:

- `per_file_diagnostics`'s `is_native` (the `map_keys` precedent) is a
  **per-file** language check computed fresh at each call site:
  `crates/internal/brink-db/src/queries/analysis.rs:139` —
  `super::file_language(file.path(db)) == super::Language::Native` — inside
  `per_file_diagnostics_query`, one file at a time.
- `whole_project_diagnostics` takes `is_native: bool` as a caller-supplied
  parameter, not a per-file recomputation. In the db path the caller derives
  it from `project_is_native` (`crates/internal/brink-db/src/queries/
  mod.rs:576-585`), which is **entry-derived**: it returns `false` whenever
  `project.entry(db)` is `None`, and the same file's own doc comment
  (`mod.rs:597`) notes `Backend` (the LSP) never calls
  `ProjectDb::set_entry` — so in an entry-less project (the IDE/LSP path),
  `project_is_native` is always `false`, regardless of how native every file
  in the project actually is.

So `map_keys`'s fallback is real precedent for the *shape* of a fix, but it
is not a drop-in mirror: the effects family's `is_native` source, if wired
the same way, would go dark for exactly the caller (the entry-less
IDE/LSP path) that most needs it. See the correction to §5(a) below.

## 3. This is not a hairline gap — it is already documented as a live gap in three places

The repo has independently discovered and written down this exact behavior
three times in the current wave, from three different diagnostics, each
without ruling it:

**a. `t2_2_effects_assertions.rs:511-514`** (E103, brink-db test):

> `.brink` files route through `lower_native`; the fixtures below need
> exactly the same `Dialect::Brink` posture `analyze` already sets (which
> also resolves `types` to `Strict`, native's requirement — `E137`).

The helper this comments on, `analyze_native` (line 514), sets
`brink_opts()` (`dialect: Dialect::Brink`) on every one of its fixtures —
including `main.brink` files, i.e. files that are native by path already.

**b. `issue_1840_register_intrinsic_confinement.rs:31-37`** (E103, brink-db
test, this wave's #1840/#2095 build):

> `[opts_with_elements]`, plus `dialect: Brink` — every effects-assertion
> check (`effects_assertions_diagnostics_query` and its siblings) gates on
> `AnalysisOptions.dialect == Brink` regardless of the file's own native
> `.brink` syntax … `opts_with_elements` alone (as the `E175`-only tests
> above use it) never triggers `E103`.

Its own `a_pure_conventions_fn_now_exceeds_on_the_registry_write` test
(line 294) has to reach for `opts_with_elements_and_brink_dialect` to observe
`E103` on `@[effects(pure)] fn conventions() { register(scene); }` — a
`conventions.brink` file, i.e. native by extension and by grammar
(`fn`/`flow`, not `===`/`function`).

**c. `crates/brink-compiler/tests/driver.rs:633-640`** (E119, this wave's
#2085/#2097 build):

> `comparator_contract`'s E119 gate is `dialect == Brink`-only, with no
> `is_native` fallback the way `map_keys`'s gate has … so it must be
> requested explicitly even though the source is already native-surface —
> the same "brink-dialect analysis over native-surface source" combination
> issue #1887 is about.

`compile_native_brink_dialect` (line 641) exists *solely* to force `dialect:
Dialect::Brink` onto an otherwise-ordinary native `.brink` compile so its
own new E119 tests (including the #1887 bare-name-callback arm) can observe
anything at all.

None of these three call sites treat the gap as their own to fix — each
correctly scoped it out and left a trail. #2099 is the first place asking
what should happen about the trail as a whole.

## 4. What is (and isn't) in "the same family"

Confirmed same shape, same fix if (a) is ruled:

| Diagnostics | Check fn | Gate site(s) |
|---|---|---|
| E102, E103, E108, E109 | `effects_assertions::check` | `lib.rs:1255`; `analysis.rs:330` |
| E105 | `await_purity::check` | `lib.rs:1255`; `analysis.rs:376` |
| E119 | `comparator_contract::check` | `lib.rs:1255`; `analysis.rs:599` |

All three share one `needs_effects` lazy-gate and one `effects_project`
whole-project inference call in the monolithic path (`lib.rs:1249-1281`), and
each has its own dedicated, independently-lazy salsa query in the db path. An
`is_native` fix, if ruled, is naturally one change (widen `lib.rs:1255`'s
condition, plus one condition per db query) rather than three unrelated
ones.

**Explicitly out of scope for this audit** (surveyed, not confirmed as the
same family): `lib.rs:811`'s `if dialect == Dialect::Brink` block
(`annotations::check` E061, `fn_values::check` T1c E079/E080/E081,
`ref_projection::check` E080/E097, `protocols::check_reserved_names` E113).
Some of these are genuinely ink/brink-dialect-only by construction — the
`#fn` creation-site literal `fn_values::check` polices is "not reachable from
`.brink` source at all" per its own call site's comment (`lib.rs:867-869`).
Others (`annotations::check`, `protocols::check_reserved_names`) were not
traced for native reachability here — `brink-syntax-native` does parse its
own `@[…]` annotation grammar (`crates/internal/brink-syntax-native/src/
parser/annotation.rs`), so whether E061/E113 have the same dark-by-default
shape is a real open question, but confirming it needs tracing `annotations
::check`'s HIR inputs against native lowering, which this pass did not do.
Flagged as a candidate follow-up, not asserted.

## 5. The options, restated from #2099 (not decided here)

**(a) Add the `is_native` fallback.** This is directionally the `map_keys`
shape but, per the §2 correction, **not a drop-in mirror** — none of the four
gate sites (`lib.rs:1255` and the three `analysis.rs` query guards) already
holds an `is_native` binding the way `lib.rs:844` does, so each site needs
its own wiring, not a copy-paste:

- `whole_project_diagnostics` (`lib.rs:1167-1321`) already takes
  `is_native: bool` as a parameter (`lib.rs:1172`) — widen `lib.rs:1255` to
  `(opts.dialect == Dialect::Brink || is_native) && needs_effects` using that
  existing parameter, no new plumbing needed for the monolithic path itself.
- The three `analysis.rs` db queries (`effects_assertion_diagnostics_query`,
  `await_purity_diagnostics_query`, `comparator_contract_diagnostics_query`)
  have no per-file `is_native` in scope today; each would need to derive one
  the way `per_file_diagnostics_query` does at `analysis.rs:139`
  (`super::file_language(file.path(db)) == super::Language::Native`) — a
  **per-file** check, not the project-level `project_is_native` the
  whole-project db path (`analysis.rs:694`) uses elsewhere. Reaching for
  `project_is_native` instead would look like the same fix but silently stay
  dark for exactly the entry-less IDE/LSP path (`ProjectDb`/`Backend` never
  calls `set_entry`, `mod.rs:597`) — the caveat is worth stating explicitly
  in any implementation, not just this audit.

With whichever `is_native` source is correct per site, every native `.brink`
project would start receiving E102/E103/E105/E108/E109/E119 the moment it
writes an `@[effects(…)]` assertion, an `await`, or a
`sort_by`/`sorted_by`/`map`/`filter`/`fold` call with a callback the analyzer
can name — with no opt-in required. This is a real, wasm-observable behavior
change for every native project that has any of those constructs today and
was silently relying on the checks being dark (none are known, since the
whole family has never fired for a stock native project, but the theoretical
risk is nonzero for anyone who already wrote `@[effects(pure)]` annotations
expecting them to be decorative).

**(b) Rule opt-in-only is correct**, and land a decision-log entry + a note
on this doc (and ideally `docs/effects-spec.md` §10, which is currently
silent on which dialects/surfaces activate the author-facing surface at
all) saying so, so the next agent who trips over this in an unrelated PR
(this has now happened three times, per §3) finds a ruling instead of
silence.

Nothing in `docs/decision-log.md` rules this either way today (checked: the
2026-07-19 "native surface is strict-only" ruling is about **type** policy,
not effects; the T2 effects rulings, sitting 1/2/3/4, are silent on which
dialects activate the checker; §10's "author-facing surface" ruling
describes the assertion *grammar*, never the gate that decides whether the
checker runs at all; the 2026-07-20 "Unified block/effect/coroutine model
ratified (native surface)" entry (`decision-log.md:1788`) settles the
suspension-ladder/coroutine substrate but never mentions the `dialect` flag
or an activation gate; the 2026-07-21 "Effect system for the native
surface: one unified row, checking-not-handlers, bounds+row-poly" entry
(`decision-log.md:1812`) is the most on-point of the two — its item (8)
says the work "wires row inference to native-lowered HIR" and treats the
row itself as a native-surface-first design, but it rules the *shape* of
the effect row, not *whether* `effects_assertions`/`await_purity`/
`comparator_contract` run at all for a given dialect — so neither entry
rules the activation gate this audit is about, though the 1812 entry is
the strongest directional signal that native-surface effect checking is
where the design is headed).

## 6. Regression coverage

`crates/internal/brink-db/tests/issue_2099_effects_family_dialect_gate.rs`
pins the current (dark-by-default) behavior for all three diagnostics on a
genuinely native `.brink` project under default `AnalysisOptions` (no
`dialect` override), alongside the same fixture with `dialect: Dialect::
Brink` forced to prove the checks are fully capable of firing and it is
specifically the gate suppressing them. Whichever way #2099 is ruled, this
suite needs a look: (a) turns the "empty under default" assertions into
"fires under default" assertions; (b) leaves them as permanent regression
coverage of the intentional posture, and this doc is the place to link the
ruling from.
