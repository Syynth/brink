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
the monolithic `analyze`/`analyze_with_modules` path. `opts.dialect` is
`brink_project_config::Dialect`, an **ink-only axis** (`{StrictInk, Brink}`,
`crates/internal/brink-analyzer/src/dialect_gate.rs:51`) — a native `.brink`
project has no opinion on it at all, so it silently carries whatever
`AnalysisOptions::default()` gives it (`StrictInk`) unless something
explicitly sets `dialect = brink` in `brink.toml` or `--dialect brink` — a
flag that means nothing for native source.

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
the sibling gate four lines above them in `lib.rs`:

```rust
// lib.rs:844
if dialect == Dialect::Brink || is_native {
    // structs::check_duplicates (E084), map_keys::check (E106),
    // map_keys::check_duplicate_keys (E138)
```

`map_keys`'s `is_native` fallback is exactly the shape #2099 asks about
adding here. It is not hypothetical precedent — it already exists in the same
function, four lines away.

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

**(a) Add the `is_native` fallback.** Widen `lib.rs:1255` to `(opts.dialect
== Dialect::Brink || is_native) && needs_effects`, and each of the three
`analysis.rs` query guards to the equivalent `!= Brink && !is_native` form,
mirroring `map_keys`'s existing precedent exactly. Every native `.brink`
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
checker runs at all).

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
