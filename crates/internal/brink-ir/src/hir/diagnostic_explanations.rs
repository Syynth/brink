//! Generated table of written diagnostic explanations (#3169).
//!
//! The prose lives in `docs/diagnostics/Exxx.md` — one file per code, all
//! 189 of them — under a `## Explanation` heading. Only the ones actually
//! WRITTEN appear here; a code whose Explanation section is empty is absent,
//! so [`DiagnosticCode::explanation`] returns `None` rather than handing a
//! caller an empty string to test for.
//!
//! Embedded rather than `include_str!`-ed because the docs live outside this
//! crate's package directory, and embedded rather than read at runtime
//! because the wasm build has no filesystem. `explanations_match_the_docs`
//! in `diagnostics.rs` re-reads the files and fails if this drifts — that
//! test runs in the workspace, where `docs/` exists.
//!
//! To regenerate: see that test's failure message.

use super::DiagnosticCode;

pub(super) const EXPLANATIONS: &[(DiagnosticCode, &str)] = &[
    (
        DiagnosticCode::E031,
        r"`brink_analyzer::resolve::check_arity` compares an ordinary call site's
supplied argument count against the resolved target's declared parameter
count. This is `Warning`-tier: the mismatched program still compiles and
runs — the call site's excess or missing arguments are a mechanical
problem the compiler can point at, not one that blocks the build.
`E176` is this diagnostic's sibling for a divert/tunnel/thread-start call
shape rather than an ordinary call.",
    ),
    (
        DiagnosticCode::E035,
        r#"`brink-analyzer::manifest`'s symbol-declaration pass warns whenever an
author declares a `VAR`, `CONST`, `EXTERNAL`, or knot (including a
`=== function NAME(...) ===` tunnel-as-function) whose name collides with
one of two reserved-name sets: the classic uppercase ink intrinsics
(`is_builtin_function` — `RANDOM`, `FLOOR`, `TURNS_SINCE`, …) or the T1b
stdlib slice-1 lowercase free functions (`is_t1b_stdlib_name` — `len`,
`push`, `insert`, `remove`, …, brink-dialect only). The bare `none` Option
literal rides the same warning for the same reason, though it is not a
function name at all. This is the ruled posture, stated in
`docs/t1b-surface-spec.md` §5 and `docs/stdlib-spec.md` item 3: prelude and
builtin names are **shadowable, not reserved** — an author declaration of
the same name is legal, and E035 exists only to flag that it is happening,
not to block it. (The one exception is the protocol registry's `display` /
`compare` / `next` — those three names are a **hard compile error, `E113`**
to redeclare, not an E035 warning, and only under the brink dialect; see
"What does NOT fire" below and `docs/stdlib-spec.md` item 6, F6.)

The warning fires once, at the shadowing symbol's own **declaration site**,
in the "merge manifests" step of analysis (`docs/compiler-spec.md` Pass 3) —
independent of whether the symbol is ever referenced or called anywhere in
the program. `SymbolKind::List` and `SymbolKind::ListItem` are **not** in
the warned-kind set. That is not a gap: see "What does NOT fire" below for
why a list item colliding with a stdlib verb name has nothing to warn
about.

Until PR #2859 (issue #2856), this rule was **documented but not actually
implemented**: `resolve_variable`/`resolve_function` (and, independently,
`lir::lower::expr::lower_call`) checked the reserved-name sets *before*
consulting real declared symbols, so a `VAR RANDOM` was silently ignored in
favor of the real `RANDOM()` at every read, and a knot `=== function
FLOOR(x) ===` was silently ignored in favor of the real `FLOOR()` at its own
call site — a clean compile with the E035 warning present, and the wrong
program running underneath it. PR #2859 reordered both layers so a real
resolved declaration wins first, with builtin recognition demoted to a
fallback consulted only once resolution has already failed — the "rules
attach to resolved definitions, never names" ruling (`docs/decision-log.md`,
2026-07-12). This page documents the rule as it now actually behaves."#,
    ),
    (
        DiagnosticCode::E061,
        r#"A bare struct-name annotation is checked against the same referrer-scoped `ImportScope`/import-visibility rules an ordinary reference is, so a struct declared in a module this file has not imported does not count as "recognized" even though the name exists elsewhere in the project."#,
    ),
    (
        DiagnosticCode::E063,
        r#"Two independent producers reach this page, gated by the same `TypePolicy::Strict` check and reported through the same code, but otherwise unrelated:

**1. Annotation vs. body-inference mismatch** (`brink_analyzer::annotations::mismatches`, TM-2's "annotation = firewall"). For each named knot and stitch, this compares its own declared param/return type annotations against the body-inferred types `infer_project` derives independently, and reports a disagreement — `Unknown`/`Conflicted` body types never disagree (an unconstrained or genuinely-conflicting body offers nothing an annotation could contradict), and `unify(annotation, body) == annotation` covers the one legal directional coercion (`int` body, `float` annotation).

**2. Calls through a value** (`brink_analyzer::strict::check_value_calls`, docs/t1c-spec.md §4/§8). For the same named-knot/stitch set, this walks every recorded [`ValueCallFact`] — a call site whose callee is a `temp`/param rather than a knot, external, or annotated function value — and reports the ones whose *known* (non-`Unknown`, non-`Conflicted`) type disagrees with being called: a concrete non-function type (`NotCallable`), the wrong argument count (`ArityMismatch`/`OverBind`), or a mismatched argument type (`ArgMismatch`). A callee whose type escapes inference as `Unknown` or `Conflicted` is reported too, but as **`E065`**/**`E066`** respectively, not E063 — see "What does NOT fire".

Both producers are wired inside `strict::check`, which `strict_diagnostics` (`crates/internal/brink-analyzer/src/lib.rs`) invokes only `if opts.type_policy() == TypePolicy::Strict`; under `Gradual` the whole block — inference included — is skipped, so neither producer runs at all, confirmed empirically:

| Effective policy | Annotation mismatch (`hp: string` vs. int-inferred body) |
|---|---|
| `Brink`, `types` unset (→ `Strict`, `resolve_type_policy`) | `E063`, `Severity::Error` |
| `Brink`, `types = gradual` | compiles clean — no diagnostic |
| `Brink`, `types = strict` | `E063`, `Severity::Error` |

`resolve_type_policy` (`strict.rs`) is why an unset `types` under `Dialect::Brink` defaults to `Strict` while the same unset `types` under `Dialect::StrictInk` defaults to `Gradual` — "brink dialect" and "strict typing" read as independent axes but a bare `dialect = brink` project is running `Strict` by default unless `types = gradual` is set explicitly. `StrictInk` can never reach `Strict` at all: `types = strict` under `dialect = strict-ink` is a project-level config error, `E064`, reported instead of running `strict::check` (TM-2's annotation syntax is brink-extension syntax, so strict typing requires the brink dialect).

`effective_severity` (`strict.rs`) additionally hard-codes E063 to `Severity::Error` whenever `types == Strict` — but since both producers on this page already require `types == Strict` to run at all, in practice every E063 diagnostic either of them reports is `Error`; `DiagnosticCode::E063`'s own declared base severity (`Warning`) never actually surfaces for these two producers as currently wired."#,
    ),
    (
        DiagnosticCode::E082,
        r#"A block-scoped `temp` — one declared inside a `~ { … }` multi-line logic block, or inside the block a `while`/`for`/`if` opens (`docs/t1b-surface-spec.md` §2) — is visible only for the rest of *that* block. Once the block's closing `}` (or, for `for`/`while`/`if`, the end of the construct it desugars into) is reached, the name goes out of scope, exactly like a local variable leaving a `{ … }` scope in a C-family language. This is a **brink-extension (T1b) construct**: it exists under `--dialect brink` (or `[project] dialect = "brink"` in `brink.toml`); under the default `strict-ink` dialect, `~ { … }` itself is rejected first, by [E051](E051.md), so E082 can never fire there.

The confusing part is that the declaration is still sitting right there in the source, a few lines up — nothing about the shape of the code signals that it stopped applying. `lower_path` (the by-value/`ref`-argument read path) and `lower_call` (the call-position path, added by #2848 for issue #2837) both resolve a bare name to a `Temp` symbol first via `LowerCtx::temp_slot`, which only consults currently-*open* block scopes. When a name was *at some point* declared with `declare_block_local` (i.e. it really is a block-scoped temp, not a classic one) but has nothing open for it right now, that is unambiguous: the block it belonged to already closed, and this diagnostic fires — naming the block-close, not silently falling through.

This mechanism is shared LIR lowering, reached from both source surfaces: ink's own `~ { … }` block syntax, and the native `.brink` surface's own code-ground logic blocks, which lower through the identical scope-tracking pass (see the native lowering's own cross-reference to this exact E082 arm in `hir::lower_native::body::mark_split_logic_block_scopes`'s doc comment, guarding a `> text` prose-line split against attributing a later read to the wrong block).

**Plain classic temps behave differently.** A *classic* `temp` — one declared directly in a knot/stitch body, not inside a nested block — used before its own declaring statement is a forward reference on the flow graph, not a lexical-scope defect: it lives in the same call frame, so since issue #3362 it resolves to that frame's own slot (`temp_slot_raw`) and is reported as [E193](E193.md), a `[lints]`-overridable warning, while the runtime reads the still-unset slot as ink's missing-variable default. (Until #3362 it emitted a hashed `GetGlobal`/`RefGlobal` id — matching how the converter's own hashing works — with no compile diagnostic at all, which failed at link with `unresolved global`.) A block-scoped temp read after its block closes gets the opposite treatment on purpose — it is unambiguously a real defect, and one that was never expressible in inklecate at all, so it is refused at compile time instead of deferred to a runtime fault (the #680 root cause this diagnostic replaced)."#,
    ),
    (
        DiagnosticCode::E092,
        r"`brink-analyzer::manifest::insert_symbol`'s `effective_visibility` applies
declaration-flips-default (`docs/modules-spec.md` §4): a declared module
(`#@module(name)` present) defaults `Private`; an undeclared stem-module
defaults `Public`. An explicit `#@private`/`#@public` override that names
exactly that default changes nothing — the effective visibility is the same
either way — so it warns rather than silently doing nothing.",
    ),
    (
        DiagnosticCode::E156,
        r#"Brink lambdas capture **by value, always** (RULED 2026-07-19, `docs/decision-log.md` "Lambdas ruled: Rust pipes under the RustScript north star"): there is no `move` keyword because move semantics are the only mode, and there are no reference captures in v1.

A captured binding inside the lambda is therefore the lambda's own *snapshot* of the outer binding. Writing to it can never be observed by the enclosing scope — a snapshot write is always a lost write. Rather than let that surface as a silent runtime no-op, the ruling makes it a compile error, which kills the closure-mutation confusion structurally.

The check is lexical: it fires when the assignment's target name is bound outside the lambda (an enclosing `fn`/`flow` parameter, an enclosing `let`, a `for` binding, an `as` binding, or an enclosing lambda's parameter). Assignments to the lambda's own parameters and to `let` bindings declared inside the lambda are ordinary local writes and are not flagged. Assignments to a **global** (a module-level `var` cell) are not captures at all — a global is a durable cell reached by name, not a snapshotted binding — and are likewise not flagged."#,
    ),
    (
        DiagnosticCode::E157,
        r#"A save's visit/turn counts key on a scope's compiled id. A **named** scope (a knot, stitch, or a choice/gather carrying an author `(label)`) hashes its id from that name — stable no matter what else in the project changes. An **anonymous** scope (an unlabeled once-only choice's target, or a sequence's wrapper container) hashes its id from *position* instead: inserting or removing a sibling construct earlier in the same **weave block** shifts every later positional counter, and with it every later anonymous id. (Counters are block-local, so an edit in one choice's body never renumbers a sibling's body — the exposure is bounded to the construct's own block. A `(label)` goes further: it anchors its entire subtree, so everything inside a labeled choice or block is independent of sibling edits anywhere.)

Issue #1674 measured this exposure and found it bounded: globals are keyed by name, so only visit/turn counts are exposed to the *runtime*, and an anonymous container's count is unreadable by author expressions (there is no way to write `READ_COUNT` against something with no name). (`docs/decision-log.md`'s 2026-07-27 "CORRECTION to the R1 entry" widens this: anonymous scopes also carry translation units, so intl is exposed too — this lint and `LoadReport::anonymous_states_dropped` still only cover the visit/turn-count half.) The fallout, when a patch shifts an anonymous id, is exactly two shapes:

- a once-only choice may **reappear**, as if never chosen;
- a sequence may **restart** from its first branch.

`brink_runtime::save::load_state` reports this after the fact through `LoadReport::anonymous_states_dropped` (a saved anonymous visit/turn count that no longer resolves). This lint is the *before*: a compile-time nudge to name the construct so the exposure never happens in the first place.

Naming is the fix ruled proportionate for choices — a labeled choice (`* (label) …`) resolves its identity by name, not position, is immune to this drift, and anchors everything inside its body along with it. A sequence has no label syntax of its own; the mitigation is structural — place it inside a `(label)`ed choice or block (whose anchor insulates it), or in its own small, stably-named stitch, so nothing can renumber it.

This lint is **off/info by default** (RULED: a single-shot project that never patches its content should not be nagged) and tier-able through `[lints]` like any other diagnostic code — a team doing live-ops or shipping user-generated content can raise it:

```toml
[lints]
E157 = "warn"   # or "deny", "hint" — any LintLevel
```

Only constructs that genuinely carry durable state are flagged:

- a `+` (sticky/repeatable) choice is never flagged — it has no "already chosen" state to begin with;
- a fallback (`else`) choice is never flagged;
- a single-branch, non-`once` sequence is never flagged — its computed branch index is always `0` regardless of visit count, so despite the alternation syntax it is genuinely stateless."#,
    ),
    (
        DiagnosticCode::E158,
        r#"Lambda lifting (issue #1709) turns `|params| body` into a synthesized, callable function. Before it can do that, it walks the body once to find every **free** name — a name the body reads that is not one of the lambda's own params or an inner binding — because each free local becomes a capture, snapshotted into the closure at the point the lambda value is created.

A free name that is not a local of the enclosing frame at all (a module-level `var`, a knot or function name) is left alone: it resolves the same way from inside the lifted function as it did outside, so no capture is needed. But a free name the analyzer *did* resolve as a `Temp`/`Param` of the enclosing frame is, by construction, a local — and if lifting still cannot find a slot for it, that is not "not a capture", it is a capture the pass cannot perform.

The one shape that happens in practice is recursion:

```brink,fires(E158)
fn a() {
  let f = |x| {
    if x <= 0 { return 0; }
    return f(x - 1) + 1;
  };
  return f(3);
}
```

`f`'s initializer (the lambda itself) is scanned for captures *before* the enclosing `let f = …` finishes binding `f` — so `f` has no temp slot yet when the lambda body's `f(x - 1)` call is scanned, even though the analyzer resolves that same `f` as a real local. Falling through silently here would leave call lowering to target the `let`'s own `DefinitionId` as though it were a callable function — not a compile error, but a program that compiles clean and faults at runtime when `f` is called from inside itself.

Recursive lambdas are not supported in this slice. `E158` refuses the program at compile time instead of shipping that miscompile."#,
    ),
    (
        DiagnosticCode::E164,
        r#"Inline markup is **freeform by default** (`docs/prose-dialect-spec.md` §4.2): an unrecognized `<tag>` is never a parse error, and a project that declares no vocabulary is never diagnosed. The host capability manifest is what *tightens* this. Once its `markup` section declares at least one span kind, every span in the project is checked against that vocabulary and an undeclared tag reports `E164`.

The vocabulary is **host-authored** and lives alongside `externals` in the host capability manifest (`docs/host-capability-manifest.md`), by §3.4's authorship test: a text-effect plugin can generate its tag declarations the same way bindings generate externals. Element conventions are project-authored and live elsewhere — they are a different surface.

`E164` is a `Warning` by default, so its severity is configurable: `[lints] E164 = "deny"` makes a declared vocabulary binding, and `@[allow(E164)]` / `// brink-disable E164` turn it off for one declaration or one line."#,
    ),
    (
        DiagnosticCode::E165,
        r#"The per-kind counterpart of [`E164`](E164.md). When the host capability manifest's `markup` section declares a span kind, it also declares the attribute names that kind accepts; an attribute outside that set reports `E165`.

Gated the same way `E164` is: it fires only for a span whose *name* the manifest does declare. An undeclared tag reports `E164` alone rather than cascading one report per attribute, and a project that declares no vocabulary at all is never diagnosed (markup is freeform by default — `docs/prose-dialect-spec.md` §4.2).

Attribute *values* are not checked. Span attribute values are static text by construction, so there is no type or domain to check them against — only the attribute name is part of the declared vocabulary.

`E165` is a `Warning` by default, so its severity is configurable through `[lints]`, `@[allow(E165)]`, and `// brink-disable E165`, exactly like `E164`.

`E165` ranges against the exact `name="value"` attribute, not the whole enclosing span (issue #1829) — so a span carrying several undeclared attributes gets one squiggle per attribute rather than several identical whole-span squiggles."#,
    ),
    (
        DiagnosticCode::E166,
        r#"`@[element(args = "…", block)]` declares that the annotated handler captures the run of content following its matched line into a `content` param (`docs/decision-log.md`, 2026-07-31, "Conventions are annotated handlers"). `block` widens the same capture contract [`E160`](E160.md) already enforces for `args`' named captures: the declaration must actually have somewhere for the captured run to bind, checked statically rather than deferred to dispatch. A `block` annotation with nothing to bind the captured run to is a defect in the declaration itself, not a per-call-site concern.

`E166` fires in two cases:

- the annotated declaration has no parameter whose type annotation is `content`, or that parameter is not the *last* parameter;
- the `content`-typed parameter's name collides with one of `args`' own named capture groups (a capture and the block receiver cannot be the same param).

`@[element(…, block)]` only declares the capture contract — it does not by itself implement the terminator search (a blank line or any element-level line) that would collect the block's content run and bind it to the receiver. The plain `!name` dispatch rewrite (matching a line and calling the handler by name) shipped in issue #2004; the `block` capture's own trailing-receiver binding — a `block`-declared handler's dispatch has no capture to bind the collected run from — remains issue #1839's scope. Until #1839 lands, a well-formed `block` declaration parses and validates, and a bare `!name` line dispatches to it, but the `content`-typed receiver parameter itself is never populated from a captured block.

**`content` is a resolvable annotation type (issue #1846).** A `block`-flagged declaration's qualifying trailing parameter must be *written* as `content` for this check (a shallow text match on the raw `TypeExpr`, per [`hir::lower_native::annotation`]'s own doc), and `content` is now in `brink_analyzer::annotations::is_known_leaf`'s vocabulary — the same declaration's `content`-typed parameter no longer raises `E061` under `dialect = brink` (the dialect brink-lsp and brink-web resolve from `brink.toml`). So the Fix example below parses and validates as a well-formed `block` declaration (no `E166`) and now compiles cleanly end-to-end at the declaration-surface level. It is still **not usable end-to-end** in the block-capture sense: the `block` receiver binding — running the terminator search, collecting the captured run, and populating the `content` param — remains issue #1839's scope (see above), not delivered here."#,
    ),
    (
        DiagnosticCode::E167,
        r#"Issue #1838 and the 2026-07-31 ruling ("Conventions are annotated handlers", `docs/decision-log.md`) collapsed the declarative element table into an annotation surface. Issue #2164's 2026-08-03 ruling then split that surface in two: `@[convention(claims = "…", order = N)]` for pattern-claiming, `@[element(args = "…")]` for `!name`-dispatched, self-announcing handlers. A handler spelled with `claims = "…"` claims prose lines that announce nothing — a scene heading, a transition — and the compiler rewrites each claimed line into a single call on the handler.

Because the rewrite has no other source of arguments, the compiler checks the binding contract in both directions at the declaration:

- `E160` — a named capture that matches no parameter (it could never bind anything);
- `E167` — a parameter that no named capture matches (the rewrite could never supply it).

Reporting at the declaration rather than at a claimed line is deliberate: the defect is in the pattern/signature pair, and it would otherwise surface as a confusing error on whichever prose line happened to be claimed first."#,
    ),
    (
        DiagnosticCode::E168,
        r#"Issue #1838 rewrote every prose line a claiming handler's pattern matches into exactly one call. When a file declares more than one claiming handler, and more than one of them could match the same line, something has to decide which handler wins. Issue #2164's 2026-08-03 ruling (`docs/decision-log.md`, "`order` is REQUIRED on `@[convention]`…") makes that decision **total, explicit, and authored**: `try_claim` (`crates/internal/brink-ir/src/hir/lower_native/element.rs`) tries each handler in ascending `order` and dispatches to the first pattern that matches. This retired the interim issue #1848 declaration-order rule — a claiming `fn`'s textual position in the file has no bearing on precedence any more.

`E168` catches the narrowest, fully provable instance of two patterns competing for the same line: **identical patterns**. Identical patterns match identical inputs, so wherever both handlers are eligible to claim a line, the lower-`order` one wins first.

That is not quite the same as "the higher-`order` twin can never claim anything", though. `try_claim` excludes a handler from claiming lines that live inside its **own** declaration (a handler's own body is not claimable by itself — the staging rule). That exclusion does not extend to a higher-`order`, byte-identical twin: the twin is exactly the handler that *can* claim a line inside the lower-`order` one's own body, precisely because the lower-`order` one is barred from claiming there. So `E168` runs after the whole file has been lowered and only fires when the higher-`order` twin produced **zero** actual claims — a twin that won even one claim (necessarily somewhere inside the lower-`order` twin's own body) is live and is not diagnosed.

**What this does not catch.** Two *different* patterns whose matched-line sets merely overlap — one a strict subset of the other, an alternation sharing a branch, two competing prefixes — are the more common and more valuable case to flag ("pattern power proportional to auditability", `docs/prose-dialect-spec.md` §3.5b). That case is now covered by `E170` (issue #1859), which proves subsumption from a set of witness strings generated from the higher-`order` pattern's structure rather than requiring byte-identical text."#,
    ),
    (
        DiagnosticCode::E169,
        r#"The 2026-07-31 §9.1 ruling ("Conventions are annotated handlers", `docs/decision-log.md`) settled an asymmetry between the two ways a handler can be reached (item 4):

> **Pattern-claiming is confined to ONE module** — the conventions module named in `brink.toml`. `!name`-dispatched handlers stay legal anywhere precisely because they self-announce.

A `!name`-dispatched line spells the handler it calls right there at the call site — a reader sees exactly what runs. A *claiming* pattern (`claims = "…"`) works the other way: it silently reinterprets ordinary prose that happens to match its regex as a call, with nothing at the call site marking that a rewrite happened. That asymmetry is only safe if every claiming handler lives in one file a reader (or reviewer) already knows to open — scattered across the project, "did this line get claimed, and by what" stops being answerable by inspection.

Issue #1838 built the dispatch mechanism itself and issue #1847 closed a related silent-drop (a claiming `fn` nested inside a `module { … }` block). Both landed the *placement* half of the asymmetry: `E112` fires when a `claims` annotation sits somewhere other than a top-level `fn`. **This code is the *module* half**: even a validly-placed top-level claiming `fn` is misplaced if it isn't declared in the file `brink.toml` names. Issue #2164's 2026-08-03 ruling later split the annotation surface into `@[convention(…)]` (claiming) and `@[element(…)]` (`!name`-dispatched) — this code's confinement rule stays with `@[convention]`, the claiming half, unchanged.

⚠ **Issue #2180 renamed the config key** from `[project] elements` to `[project] conventions`: the key predates the `@[element]`/`@[convention]` split above and, post-split, named a module of the *latter*, not the former — a misnomer once the split landed. `brink-project-config` still accepts `elements` as a deprecated alias (it sets the same value, but emits a `ConfigWarning` naming the rename) for a deprecation window rather than hard-breaking every existing project's `brink.toml`.

⚠ **Issue #2289 (2026-08-05 ruling) corrected a defect in this confinement rule that survived unnoticed since #1844 landed**: confinement restricted *where a handler may be declared*, but nothing made the configured module's handlers actually claim prose in any *other* file — a correctly-declared conventions module claimed nothing outside its own file. That is now fixed (see `hir::lower_native::element`'s "Cross-file claiming reach" module doc): the configured module's handlers claim across the WHOLE PROJECT. The confinement rule this code enforces is what makes that coherent — see the maintainer's own framing in the decision log: *"it's never file local. you configure conventions for a project, that's why they're conventions and not 'local patterns.'"*"#,
    ),
    (
        DiagnosticCode::E170,
        r#"This extends `E168` (byte-identical patterns) to cover the more common and more valuable case: two *different* patterns whose matched-line sets overlap.

When a file declares more than one claiming handler, and more than one of them could match the same line, something has to decide which handler wins. Issue #2164's 2026-08-03 ruling (`docs/decision-log.md`, "`order` is REQUIRED on `@[convention]`…") makes that decision **total, explicit, and authored**: `try_claim` (`crates/internal/brink-ir/src/hir/lower_native/element.rs`) tries each handler in ascending `order` and dispatches to the first pattern that matches. This retired the interim issue #1848 declaration-order rule — a claiming `fn`'s textual position in the file has no bearing on precedence any more.

`E170` catches overlapping (but non-identical) patterns where the higher-`order` handler provably can never win a claim on its own — because every string the higher-`order` pattern can match, the lower-`order` pattern also matches, so the lower-`order` one always wins first under order-sorted, first-match-wins dispatch. Mere overlap (some string both patterns match, but each also matches strings the other doesn't) is *not* enough to flag — a higher-`order` pattern that is genuinely more specific in a way that also matches different lines is live, not dead code.

The detection uses a sound-but-incomplete heuristic: generating a set of candidate strings from the higher-`order` pattern's structure (recursing into named capture groups, expanding every alternation branch, picking a representative character for classes and repetitions) and checking that the lower-`order` pattern accepts **every** one of them. If it does, the higher-`order` pattern's language is provably subsumed by the lower-`order` one. If the higher-`order` pattern contains a construct the generator doesn't know how to expand, no witnesses are produced and nothing is flagged — a false negative (missing a real subsumption) is safer than a false positive (incorrectly flagging patterns that never actually overlap).

The higher-`order` handler is only flagged if it produced **zero** actual claims — if it actually won even one claim (necessarily for a line the lower-`order` pattern couldn't match, or where the lower-`order` pattern was barred by the staging rule), it is live and is not diagnosed.

**What this does not catch.** Subsumption so subtle that no witness set proves it — complex alternations, look-ahead assertions, or patterns where subsumption depends on interactions the generator can't expand — are not detected.

Each higher-`order` handler is reported **at most once**, against the first (lowest-`order`) handler it provably subsumes — a handler subsumed by two or more lower-`order` handlers is not re-reported once per subsuming handler."#,
    ),
    (
        DiagnosticCode::E171,
        r#"`hir::lower_native::element::try_claim` rewrites a claimed prose line into exactly one call, and every argument of that call comes from a named capture. The rewrite binds each capture as a plain `Expr::String` literal, **unconditionally** — regardless of what type the receiving parameter declares:

```brink,fires(E171)
@[convention(claims = "^Take (?<n>\\d+)$", order = 10)]
fn take(n: int) {
  inventory_add(n)
}
```

`n` is declared `int`, but `try_claim` always passes it as a string. Left unchecked, this is **silent today** — nothing checks a direct call's arguments against the callee's declared parameter types yet. That generic check (`E063` for this shape) is exactly what open issue #1864 asks to build; until it lands, a mismatched claiming handler like the one above compiles with zero diagnostics and just receives the wrong value at runtime.

Numeric capture coercion is `docs/prose-dialect-spec.md` §3.5b's own **Deferred** list — the underlying gap is ruled-deferred, not itself a defect.

`E171` is reported at the **declaration**, the same static-defect-in-the-declaration posture `E160`/`E166`/`E167` already take for the rest of the capture contract — pointing at the mismatched parameter's own type annotation, not the whole `@[convention(…)]` line or a claimed line's whole text. A handler that fails this check is never registered as a claiming handler at all (like a handler that fails `E160`/`E166`/`E167`), so no line is ever rewritten to a call with an argument that could never match its declared type.

An **untyped** parameter (no `: type` annotation) is unaffected — it takes whatever the rewrite gives it, exactly as before this check existed.

### Why `content` is exempt

`content` might look like the same case as `int` — a capture can no more produce a `FragmentRef` than it can produce an integer, and binding a `content`-typed parameter to an actual captured value is the *same* Deferred list's own item, separate from numeric coercion (`docs/prose-dialect-spec.md` §3.5b, issue #1846/#1838/#1839). But `content` is exempted because it is the spec-ruled capture annotation form (§3.5b, issue #1846/#1839): the spec's own worked example (`fn radio(chan: string, text: content)`) and the `tests/tier1-native/annotations-element` golden fixture both declare a captured `content` parameter today, and both compile clean. Flagging it here would turn an already-shipped, spec-ruled pattern into a fresh hard error for no compiler-observable reason.

Every *other* declared type (`int`, `float`, `bool`, a struct name, a generic, a `fn` type) has no such precedent and no such rescue — those are this diagnostic's actual target."#,
    ),
    (
        DiagnosticCode::E172,
        r#"`#@…` is not its own grammar production in either dialect. It is an ordinary tag — `HASH` followed by free text — and only ink's HIR lowerer (`hir::lower::directive::parse_directive_tag`) gives a leading `@` special, compile-time-consumed meaning: it strips the directive name, matches it against a fixed set (`private`, `public`, `was`, `module`, `local`, `effects`), and erases the tag from the compiled output before anything reaches the runtime.

`hir::lower_native` never had a matching check. `#` is already the runtime-tag sigil in native content position — that is exactly *why* `#@…` parses as a tag rather than a directive on the native surface too, not a gap in the grammar. But nothing downstream of the parser treated a leading `@` as meaningful, so before this diagnostic existed, `#@was("old_name")` in a `.brink` file compiled clean and shipped `@was("old_name")` as a literal tag on the compiled story — no error, no warning, and the mistake surfaced (if at all) as mysterious tag content at runtime rather than as a compile-time failure. That silence is worse than a plain no-op: an author porting a file from ink, or splitting time between the two dialects, has no signal that the line did nothing they intended.

`E172` closes that gap: `hir::lower_native::body::lower_tag` checks every tag's text for a leading `@` and raises this code, naming the fix.

**Four outcomes, by directive name:**

- `was` and `effects` are real ink directive names (`hir::lower::directive::parse_directive_tag`'s recognized set) that each have a real native counterpart — the `@[…]` annotation channel (`hir::lower_native::annotation`) recognizes `@[was("old::path")]` and `@[effects(…)]`. The message names the matching annotation spelling directly.
- `module`, `public`, `private`, and `local` are real ink directive names with no native equivalent today — native has no per-declaration visibility or locality syntax yet, and file identity is established structurally, not through a tag directive. The message says so plainly instead of inventing a spelling that doesn't exist.
- `allow` is **not** an ink directive name at all — ink's own directive recognizer only knows the six names above, so `#@allow` is an unknown directive there too. It gets its own wording: native's `@[allow(…)]` annotation is the diagnostic-suppression channel, but that is unrelated to this tag, which has no directive meaning in either dialect.
- any other name is unrecognized by both dialects. The message only says the tag has the *shape* of a directive (a leading `@`) — it never asserts ink would recognize that specific name, since a project may deliberately use its own `@`-led runtime tag convention (the issue's own caution, e.g. `#@narrator`).

**`Warning` by default, not `Error`.** A literal `@`-led tag can be a deliberate runtime convention for a host that wants one — the compiler cannot tell "this author meant an ink directive" from "this project really does tag lines with `@`" just from the text. So the diagnostic is `[lints]`-configurable and suppressible at the source with `@[allow(E172)]`, the same posture `E132`/`E168`/`E170` take for other directive-adjacent, non-fatal misuses. A project that wants the literal tag keeps it; a project that meant the ink directive gets pointed at the fix."#,
    ),
    (
        DiagnosticCode::E173,
        r#"The host capability manifest's `markup` section (`docs/host-capability-manifest.md` § "Markup vocabulary") declares each span kind's accepted attributes. Until issue #1997, that section was an *allow*-list only: `attrs` named which attributes a kind accepts, and an attribute outside that set reported [`E165`](E165.md) — but a declared attribute that was simply *absent* from a span went entirely undiagnosed. There was no way to say "this attribute is mandatory."

Issue #1997 (ruling `#1780`'s gap 1) adds a `required` flag to each declared attribute. A span whose kind is declared, and which omits one of that kind's `required` attributes, reports `E173`.

Gated the same way [`E164`](E164.md)/`E165` are:

- It only ever fires for a span whose *name* the manifest does declare — an undeclared tag reports `E164` alone, with no `E173` alongside it (there is no declared attribute set to be missing *from*).
- It only fires for the subset of a kind's attributes actually marked `required`; a kind with none required never raises this for any span of that kind.
- One diagnostic per missing attribute, not one combined message — a span missing several required attributes gets one `E173` per name, mirroring `E165`'s one-per-attribute posture.

`E173` stays ranged against the whole span, unlike `E165` (issue #1829) — a *missing* attribute has no `name="value"` node in source to point at, so there is nothing narrower to range against.

Attribute *values* are still never checked — this code is about presence, not typing. Span attribute values stay static text by construction (`SyntaxKind::SPAN_ATTR_VALUE`); the manifest's `attrs` schema widened to a per-attribute record to make room for a future value type (`ManifestSpanKind`'s own doc), but that is schema headroom only — no attribute value is parsed, resolved, or checked against anything by this code or by `required`.

`E173` is a `Warning` by default, so its severity is configurable through `[lints]`, `@[allow(E173)]`, and `// brink-disable E173`, exactly like `E164`/`E165`."#,
    ),
    (
        DiagnosticCode::E174,
        r#"Ruled 2026-08-01 (issue #1994, closing #1932): for a lambda specifically, a written annotation **governs** that slot's resulting type — it is not merely a fallback consulted when the body-derived type comes back `Unknown`. If the body's own independent derivation resolves to something concrete and it disagrees with the annotation, that is an error raised immediately at the lambda's own declaration, not a deferred surprise at whatever calls the lambda later.

This is deliberately different from a top-level `fn`/`flow`, where a written annotation is only the `Unknown`-fallback overlay and a disagreement is reported as the gradual/advisory `E063` (`docs/typed-mode-spec.md` §2's "annotation = firewall" precedence rule, and its lambda counterpart recorded alongside it). The two are ruled to differ on purpose: a lambda is typically small and locally scoped, and is more likely annotated specifically to pin down what its body should mean — so a wrong body should not be able to silently override a correct annotation the way it can for a `fn`.

An unannotated param or return is unaffected: it still exports whatever its body derives, exactly as before (issue #1910).

This check runs only under `types = strict` (`docs/typed-mode-spec.md`), the same policy gate every other TM-3 type-mismatch diagnostic uses. Unlike `E063`, it is `Error` by default, not `Warning` — it is not `[lints]`-downgradable the way the advisory codes are."#,
    ),
    (
        DiagnosticCode::E175,
        "This code no longer fires. It documented `register`'s placement rule while that intrinsic existed; the intrinsic (and the mechanism it served) has since been deleted.",
    ),
    (
        DiagnosticCode::E176,
        r#"A knot or stitch that declares parameters (`=== accuse(who) ===` in ink, `flow accuse(who) { … }` in native) is diverted to exactly like a function call: the call site must supply one argument per declared parameter. `E176` is `E031`'s sibling for this shape — `E031` is "function call argument count mismatch" and is scoped to ordinary calls (`f(args)`); `E176` covers the divert/tunnel/thread-start call shape instead, so the two can be told apart and suppressed independently.

This diagnostic was previously unreachable for a divert on either dialect, and unreachable for a native `-> knot(args)` site at all until recently:

- Before PR #2150 (issue #2136), native's `-> knot(args)` call-args syntax hard-failed with `E129` ("parses but has no HIR lowering yet") — the argument list never reached HIR at all, so no argument-checking pass could see it.
- Even after that fix wired `DivertTarget::args` for real, the arity check still could not fire: `brink_ir::symbols::project`'s divert-reference projection always recorded `arg_count: None` for a divert, regardless of how many arguments the divert actually supplied. `brink_analyzer::resolve::check_arity` (the mechanism behind `E031`) only runs when a reference's `arg_count` is `Some`, so a divert's arity was never checked on ink either, not just native. Issue #2156 closed both gaps: the reference now carries `Some(target.args.len())`, and a dedicated `E176` check runs whenever that resolves to a `Knot`, `Stitch`, or `Label`.

`E176` deliberately does **not** fire when a divert resolves through a `Variable` or a divert-typed local parameter (`=== knot(-> return_to) ===`, then `-> return_to`) — see "Advanced: sending divert targets as parameters" in the ink documentation. Those are stored/forwarded divert-target values: the variable or parameter itself carries no declared parameter row, so there is nothing meaningful to check arity against at that indirection site."#,
    ),
    (
        DiagnosticCode::E178,
        r#"Issue #2164 (`docs/decision-log.md` 2026-08-03, "`order` is REQUIRED on `@[convention]`, and duplicates within a module are a compile error") split the old `@[element(claims = "…")]` spelling into its own `@[convention(…)]` annotation, and made `order` a **required** property of it, not an optional one:

> `order` is a REQUIRED property of `@[convention]`, not an optional one... there is no "default when `order` is absent", because it can never be absent.

A pattern-claiming handler competes for prose lines it did not announce — unlike a `!name`-dispatched `@[element(…)]` handler, which self-announces and therefore needs no precedence at all. When more than one claiming handler in a module could match the same line, something has to decide which one is tried first. Before this ruling that "something" was implicit: a claiming `fn`'s own textual position in the file. That interim rule (issue #1848) is retired — precedence is now **total, explicit, and authored** on the declaration itself, via `order = N`, and the compiler never falls back to declaration position, file order, or any other inferred tie-break to decide.

Because there is no default, a `@[convention]` written without `order` is not silently assigned "declared-order" precedence the way it would have been before this ruling — it is a compile error instead, so an author is never surprised by a claiming handler's place in the resolution order they never actually chose.

`@[element(args = "…")]` (the `!name`-dispatched, self-announcing form) is unaffected: it takes no `order` at all, since a handler that names itself never competes for a line, and declaring one there is simply an unrecognized clause (`E159`), not this code."#,
    ),
    (
        DiagnosticCode::E179,
        r#"`order` (issue #2164, `docs/decision-log.md` 2026-08-03) makes a claiming handler's precedence "total, explicit, and authored" — every `@[convention]` in a module names a bare integer, and the walk tries lower-`order` handlers before higher ones. That only works if `order` actually totally orders the module's handlers. Two declarations sharing one value would leave their relative precedence undefined again — exactly the ambiguity `order` exists to remove — so the ruling closes that gap by rejecting it outright rather than resolving it:

> `order` is REQUIRED, and duplicates are a compile error... there is no tie-breaking rule, because ties are rejected rather than resolved.

Unlike most duplicate-declaration diagnostics (which report only the second occurrence, the "one wins" posture `E048` takes for a repeated directive), `E179` is reported against **both** conflicting declarations — the duplicate-*definition* posture, since neither declaration is more "the real one" than the other: an author who opens either `fn` sees the conflict, not just the one that happened to be declared later.

This check is scoped to handlers declared **in one file** — the same "declared IN THIS FILE" ground truth `E168`'s duplicate-pattern check and `HirFile::claim_handlers` already use. A handler injected from the project's conventions module into some OTHER file's dispatch table (issue #2289's cross-file claiming reach) is not compared against that file's own `order` values here — not because it lacks a real `order` (it carries one, read straight off its own declaration, unlike the deleted #1863 injection seam this replaces), but because it was not declared in that file: two declarations sharing an `order` inside ONE module is what this check means, and an injected handler is not a second declaration, just the same one being used elsewhere."#,
    ),
    (
        DiagnosticCode::E180,
        r#"`attach` (issue #2178, split from #2164's 2026-08-03 design-backport comment) declares the **schema** a claiming handler attaches to the run its claimed line produces: a plain `struct` name, naming which keys are attached and their types. `docs/decision-log.md`'s 2026-08-03 entry ("The element output model") states the governing split plainly:

> The attachment schema is a STRUCT — do not invent a DSL... Declared (projection, editor-readable): pattern, `order`, mode, `kind`, which keys are attached and their types. Computed (handler body, compiler-only): emitted text, normalized values, side effects... A `struct` is already declarative, statically known, serialized, and understood by compiler + editor + host — so the projection carries a type and no new declarative sub-language exists.

That split only holds if the handler's declared return type actually **is** the struct `attach` names — otherwise the schema a tool reads off the annotation and the value the handler could ever actually return would disagree, and the projection would describe an output the compiler could never produce. `E180` closes that gap: the annotation is checked against the declaration's own `: type` clause, the same way `E166`'s `block` check and `E171`'s captured-parameter check are checked against the declaration itself, never against the handler's runtime behavior.

This is a **declaration-surface** check, like every other check in `hir::lower_native::annotation`: it compares `attach`'s name against the return type's own bare name (`TypeExpr::Named`), and never resolves whether a struct of that name is actually *declared* anywhere in the project — that is real name resolution's job, out of scope for this code (the same posture `E171`'s own doc explains for a captured parameter's declared type)."#,
    ),
    (
        DiagnosticCode::E181,
        r#"`brink_ir::lir::lower::structs::build_shape_table` walks every file's declared `STRUCT`s and resolves each one's own `DefinitionId` via `decls::lookup_global(index, file_id, name, SymbolKind::Struct)`, using the struct's own declaring file as referrer. Because the symbol index always keeps an entry for `(file_id, name)` when a struct genuinely declares itself in that file, this lookup almost always hits `lookup_global`'s exact-file arm and never fails.

It can fail in one narrow case: `brink-analyzer` already dropped this HIR declaration's own symbol entry as a true intra-module duplicate (`E023` — the same declared module as an earlier same-name declaration elsewhere), so no symbol carries `(file_id, name)` any more. Ordinarily `lookup_global`'s unscoped fallback then rescues the surviving sibling's id instead — that rescue is exactly what lets `build_shape_table`'s own `by_def`-keyed dedup recognize "this is a true intra-module duplicate, not a fresh shape" and skip it a second time. But that fallback itself excludes any candidate declared in a mounted `std…` module (issue #2197's std-visibility carve-out). If **every** surviving same-name candidate happens to be std-declared, the fallback comes back empty too, `lookup_global` returns `None`, and — before this diagnostic existed — the struct was silently dropped from both the shape table and the seeded name table, shifting every subsequent `ShapeId`/`NameId` and the bytecode built from them with no diagnostic at all.

`E181` is the non-suppressible backstop that makes that drop loud instead, the same defense-in-depth posture as `E060`/`E073`: it should never fire from an ordinary compile, and reaching it means the invariant `build_shape_table`'s dedup logic depends on (a self-declaring struct always resolves against its own file, or against a non-std surviving duplicate) has been violated.

Reachable **today**, not only from a future multi-file std mount: the standard library already declares `struct Cue` and `struct Parenthetical` (`std/conventions/screenplay.brink`), and `symbol_index_query` builds the shared symbol index from every registered file regardless of the compilation closure — so the mounted std declaration sits in the index even for an ink entry whose LIR closure never reaches the std file itself. An ordinary project need not declare a `#@module` at all: if the project's own file and the std file don't coexist under M-2d (either isn't module-qualified, or the project isn't `Dialect::Brink`, or it isn't all-native), a same-named project `STRUCT` collides with std's as an ordinary same-module duplicate, and it is the *project's* declaration that gets dropped whenever its own file sorts after the std key in `FileId`-mint order (a project file named `story.ink`, `world.ink`, or `types.ink` reliably does, since `"std/…"` sorts first). It would *also* become reachable the multi-file-std way this doc previously described, if a future std mount ever gains two files sharing a declared module and duplicating a struct name.

`build_struct_shape_data` (the `NameId`-free, `Eq`-cutoff twin of `build_shape_table` that `brink-db`'s `struct_shape_data_query` memoizes for per-knot chunk lowering) performs the textually identical lookup over the same inputs and does not raise this diagnostic itself — see its own doc comment for why that is a deliberate, documented ruling rather than a second silent drop: every real compile computes both functions in the same salsa revision, over the same symbol index and the same files' `STRUCT` declarations, so the same drop condition always raises `E181` from `build_shape_table`'s side in that same compile."#,
    ),
    (
        DiagnosticCode::E182,
        r#"A `@[convention]` handler competes for lines it never announced — it claims prose by pattern, not by the author writing `!name` at the call site (issue #2179, `docs/decision-log.md` 2026-08-06 "No-world-reads fence: analyzer effect-row check; unclassified externals are diagnosed"). That only holds together if classification is a **pure function of the text**: if a handler's claim depended on live game state, the editor could never display which handler would fire, the claiming projection could never be cached, and explain-match tooling would depend on a save file existing. So the rule is narrow and absolute: a handler may call pure (in-project) functions and `Effect`/`Presentation`-kind externals ("commands" — a state-changing or client-only call, neither of which the handler *reads back*), but it may **never** call an `EXTERNAL` classified `Query` (a world read), transitively.

`ExternalKind` (`brink_ir::host_manifest::ExternalKind`) is the classification vocabulary: `Query`, `Effect`, `Presentation`, or `Plain` (the default, meaning unclassified). Before this issue `ExternalKind` was advisory tooling metadata only; this check makes it **load-bearing** — an `EXTERNAL` with neither an inline `@kind` doc tag nor a matching registered host-manifest entry stays `Plain`, and a `Plain` external reached from a handler is diagnosed exactly like a proven `Query` one. "Unprovable is not passable": the compiler has no way to know a `Plain` external doesn't read world state, so it does not assume the best case.

This is checked over the handler's **transitive** call closure, not just its own direct calls: `handler() -> helper() -> get_health()` is diagnosed even though `handler` never calls `get_health` itself — the diagnostic is anchored at the real offending call site (inside `helper`'s own body here), which may be in a different definition, and a different file, than the handler's own declaration."#,
    ),
    (
        DiagnosticCode::E183,
        r#"`lower_call` (`brink-ir::lir::lower::expr`) turns a resolved call-site symbol into LIR. Most resolved kinds have an explicit lowering (`External` → `CallExternal`, `List` → a list-conversion builtin, `Variable`/`Constant` → `CallVariable`, `Knot` → `Call` — ink allows any knot as a function via tunnels, so no other kind needs its own arm to be *legitimately* callable). Every other kind reaching this point is not callable, and this code refuses it with a diagnostic instead of falling through a catch-all that would emit `lir::ExprKind::Call` against whatever id happens to be resolved there.

That catch-all used to exist with no check at all (issue #2837). It is exactly the mechanism that let a resolution bug in PR #2836's first attempt ship as a silent miscompile: the program compiled clean — 7,941 tests, the oracle ratchet, and clippy all green — and then faulted at runtime with `UnresolvedDefinition(ListItem(..))`. The reason a resolution mistake became a *runtime* fault instead of a *compile* error was this unguarded catch-all, not the specific resolution bug that first exposed it (that one was fixed separately, in `brink-analyzer::resolve::resolve_function`). This code is the backstop: whatever puts a non-callable symbol at a call position in the future, the compiler refuses it here rather than shipping it.

`resolve_function` cannot legitimately hand back `Stitch`, `Label`, or `Struct` for a real call site today, and a bare `ListItem` only ever comes back for a `#fn(target)` literal site (`arg_count: None`), which never reaches `lower_call` at all — those four kinds are a defensive backstop against a future resolution regression. `Param`/`Temp` are different: they are reachable from ordinary author source today, whenever `LowerCtx::temp_slot` has nothing open for the name at the call site — the normal shape of a genuine forward reference, not a `temp_slot` bug (see "When this fires" below)."#,
    ),
    (
        DiagnosticCode::E184,
        r#"`brink_ir::lir::lower::decls::collect_globals` (for `CONST`/`VAR`) and `collect_externals` (for `EXTERNAL`) each resolve a declaration's own `DefinitionId` via `decls::lookup_global(index, file_id, name, kind)`, using the declaration's own declaring file as referrer. Because the symbol index always keeps an entry for `(file_id, name, kind)` when a declaration genuinely declares itself in that file, this lookup almost always hits `lookup_global`'s exact-file arm and never fails.

It can fail in one narrow case: `brink-analyzer` already dropped this HIR declaration's own symbol entry as a true intra-module duplicate (`E023` — the same declared module as an earlier same-name/same-kind declaration elsewhere), so no symbol carries `(file_id, name, kind)` any more. Ordinarily `lookup_global`'s unscoped fallback then rescues the surviving sibling's id instead. But that fallback itself excludes any candidate declared in a mounted `std…` module (issue #2197's std-visibility carve-out). If **every** surviving same-name/same-kind candidate happens to be std-declared, the fallback comes back empty too, `lookup_global` returns `None`, and — before this diagnostic existed — the declaration was silently dropped from `PreludeDecls` (no `lir::GlobalDef` for a `CONST`/`VAR`, no `lir::ExternalDef` for an `EXTERNAL`) with no diagnostic at all.

`E184` is the non-suppressible backstop that makes that drop loud instead, the same defense-in-depth posture as `E060`/`E073`/`E181` (this diagnostic's own `STRUCT` twin, issue #2240): it should never fire from an ordinary compile, and reaching it means the invariant these self-declaration lookups depend on (a self-declaring symbol always resolves against its own file, or against a non-std surviving duplicate) has been violated.

Reachable **today** for `EXTERNAL`, the same way issue #2240 found `E181` reachable for `STRUCT`: the standard library declares `extern scene_entered(title, slug)` (`std/conventions/screenplay.brink`), and `symbol_index_query` builds the shared symbol index from every registered file regardless of the compilation closure — so the mounted std declaration sits in the index even for an ink entry whose LIR closure never reaches the std file itself. An ordinary project need not declare a `#@module`, and — unlike `STRUCT` — `EXTERNAL` needs no `dialect` override to parse at all: a plain `EXTERNAL scene_entered(...)` in a `.ink` file collides with std's own `extern scene_entered` as an ordinary same-module duplicate (neither side is module-qualified, so M-2d cross-declared-module coexistence never applies), and it is the *project's* declaration that gets dropped whenever its own file sorts after the std key in `FileId`-mint order (a project file named `story.ink` reliably does, since `"std/…"` sorts first).

`std` declares no `CONST`/`VAR` today, so the `CONST`/`VAR` call sites stay reachable only in principle — the same status `E181` itself carried before its own reachable `EXTERNAL`-shaped case was found here. A future std module adding a `CONST`/`VAR` would make them reachable the same way."#,
    ),
    (
        DiagnosticCode::E185,
        r#"Issue #1900 (PR #1939) added strict-mode type checking for a plain dotted-assignment target's *value* against the field's declared type — `check_declared_field_assign_target` records a candidate fact for `~ p.x = expr`, later walked by `structs::check_field_assign_mismatch` against the receiver's declared shape to compare `expr`'s type with `x`'s declared type (`E063` on a mismatch). That check is deliberately silent when the field name itself doesn't resolve on the shape — "Unknown never disagrees" is its posture, by design, for the *type-mismatch* comparison.

But that left a real gap: nothing checked whether the field name *exists* on the shape at all. `ref_projection::check_strict`'s `E098` covers an unknown segment only in `ref`-argument position (`ref npc.bogus`, handed to a call); the construction-literal path already had this check (`structs::check`'s `E070`, `docs/typed-mode-spec.md` §6) but only for `Point#{bogus: 1}`-shaped literals. A plain assignment to an unknown field had no equivalent, and compiled clean under `types = strict` with zero diagnostics.

`E185` closes that gap: `structs::check_field_assign_mismatch` (the same function `E063` comes from) now reports it the moment the walk resolves the receiver's shape but the shape declares no field by the name being assigned."#,
    ),
    (
        DiagnosticCode::E186,
        r#"`try_claim` (`brink_ir::hir::lower_native::element`) dispatches a claimed line to exactly one of two shapes, chosen by an `if is_block { .. } else if is_attach { .. }`: **wrap mode** (`block`, issue #1839) captures the following run into the handler's own trailing `content`-typed parameter, and **attach mode** (`attach = StructName`, issue #2178) captures the following run as block-level metadata, merging the handler's returned struct fields into `OutputLine.element.data` for every line in that run (issue #2108, `docs/decision-log.md` 2026-08-03 "The element output model").

Before this code existed, nothing checked whether a single handler declared both. `parse_convention_clauses`/`convention_annotation` (`annotation.rs`) parsed and stored `attach` regardless of `block`, and `try_claim`'s `if`/`else if` always took the `block` arm when both were set — `attach` was accepted syntax that silently did nothing: no event, no data merge, no error, no warning, no hint. Issue #2264 names this exactly the shape house rule 9 ("flag silent data drops") calls always-a-bug-until-proven-otherwise.

This code is a deliberate refusal to define combined semantics, not an oversight of one: "wrap AND attach" — does the wrapped call's own return value *also* attach to the run it wraps? does it attach to itself? — is an open design question with no ruling and no test pinning any answer. Rather than invent one, `parse_convention` rejects the co-occurrence outright, the same "never a partial `ConventionAnnotation`" posture [E159](E159.md)/[E166](E166.md)/[E167](E167.md)/[E178](E178.md)/[E180](E180.md) already take — a handler declaring both is never registered as a claiming handler at all.

Reachable through both element shapes `try_claim` dispatches: an ordinary block-form claim (`@NAME` on its own line) and the compact-cue desugar (`@NAME: text`, issue #2079) — both route through the same function, so a compact-cue-claiming handler declaring both clauses hits this same check."#,
    ),
    (
        DiagnosticCode::E187,
        r#"Issue #2201: before this code existed, `lir::lower::stmts::lower_assign_target` — the shared choke point most write shapes resolve their root through — treated `SymbolKind::Constant` identically to `SymbolKind::Variable`: it handed back an ordinary writable `AssignTarget::Global`, with no distinction at all between the two symbol kinds. A story that reassigned a declared `CONST` compiled clean, with zero diagnostics anywhere in the pipeline, and the mutated value was observable in the story's own output.

This is broader than issue #2122's earlier finding for `as`-binding immutability: that issue's fix (E148) covered only `lower_field_mutator`/`lower_single_level_field_write`'s two field-write shapes. `CONST` reassignment needed the fix at every choke point that resolves a `Global` write root — seven shapes in total, enumerated in [E148](E148.md)'s own doc for the `as`-binding case and mirrored exactly here for `CONST`:

- `lower_assign_target` itself — plain/compound assignment, a postfix's bare-target conversion, an indexed-assignment root (via `lower_indexed_assignment`, which resolves its flattened root through this same function), the `pop`/`heap_pop` mutator intrinsics' lvalue argument, `lower_bare_mutator`'s root (the bare-variable fast path for the entire `MutatorKind` family — `push`/`insert`/`remove`/`remove_at`, not just `pop`/`heap_pop`), and `lower_lvalue_container_chain`'s root (the indexed-lvalue mutator path, e.g. `push(grid[y], v)`) — all of which call this same function for their root.
- `lower_single_level_field_write`/`lower_field_mutator` — a single-level struct-field write/mutator (`c.field = v`, `push(c.items, v)`) resolves its root `SymbolInfo` independently of `lower_assign_target` (the caller has already split a two-segment path into head/field before either function runs), so each needs its own call to the shared check.
- `lower_ref_path_call_arg`/`lower_ref_projection_arg` — passing a `CONST` by `ref` (bare or as a projection root) hands the callee a raw pointer to the storage cell without ever routing through assignment lowering at all.

All five are raised through the same shared helper, `lir::lower::stmts::reject_const_write` — the `CONST` analog of `reject_as_binding_write` (E148's own helper) — called individually from each choke point above, exactly the same "no single call site sees every write shape" reasoning E148's helper already establishes.

### Posture: a lowering refusal, not an analyzer diagnostic

This is deliberately placed at LIR lowering (the same posture as [E074](E074.md)/[E148](E148.md)), not as a `brink-analyzer` diagnostic (the posture [E185](E185.md) takes, which does surface through both editor analysis roads). The write-channel enumeration above already lives entirely inside `lir::lower` — it is the layer that resolves every one of these shapes to a `Global` root today. Re-implementing the same enumeration inside the analyzer, as a separate HIR-level walk, would risk exactly the channel-undercounting drift that made this issue's own premise true in the first place (issue #2122 named only two of these seven channels; the rest went unnoticed until this issue's audit). Putting the check at the layer that already catches every channel once, rather than duplicating that catalogue at a second layer, is the fix that can't silently regress to catching only *some* of the shapes again.

One consequence of this posture: like E074/E148, E187 fires during a real compile (`brink compile`, or any pipeline that runs LIR lowering) — it does **not** currently appear in either editor analysis road's live Problems panel (`ProjectDb`'s db-direct road or `IdeSnapshot::analyze`'s off-db road), since neither road runs LIR lowering. This matches E074/E148's existing, unchanged posture; it is not a regression introduced by this code.

### Applies to both surfaces

This check applies identically to `.ink` and `.brink` source — it is not native-gated. It mirrors ink's own compile-time rejection (see the Summary above), and `SymbolKind::Constant` is resolved the same way by name resolution for both frontends by the time LIR lowering runs, so the same `reject_const_write` call fires for either dialect."#,
    ),
    (
        DiagnosticCode::E188,
        r#"Issue #1865 (filed from wave retro on #1846/PR #1861's review): `annotations::resolve` matches a fixed set of literal names — the scalar leaves, plus `content` (issue #1846's capture-contract leaf), plus the NS-A8 tower kinds — **before** it ever consults `names.structs`. That ordering is deliberate, and this issue does not change it: `resolve`'s own doc already calls it out — "Checked after the fixed scalar-keyword set so a struct can never shadow `int`/`float`/etc. (those names aren't legal `STRUCT` identifiers by convention, but this ordering is the unambiguous choice regardless)" — and the tower-kind arm carries the matching comment: "checked before the struct lookup, so a STRUCT can never shadow a tower type name (the same ordering that keeps `int`/`float` unshadowable)".

The consequence nothing diagnosed before this code existed: a project that declares, say, `STRUCT content { … }` silently changes what every `content`-typed annotation means. `VAR v: content = ...` still compiles — it just always resolves to the builtin `Ty::Content`, never to the user's struct — with no diagnostic anywhere, in either direction (not at the struct declaration, not at any annotation site).

`E188` closes that gap at the declaration site: `annotations::check_reserved_type_names` walks every declared `STRUCT` and flags one whose name is in the same reserved set `resolve`'s `Named` arm checks first.

### What this code deliberately does NOT cover, and why (verified, not assumed)

- **The generic heads** (`List`/`Array`/`Map`/`Option`/`Weighted`/`Handle`). These names are special-cased only inside `TypeExpr::Generic`'s own dispatch (`Array<T>`, with angle brackets) — a *bare* `Named` reference to a struct sharing one of those names (`f: Array`, no `<...>`) still falls through to the ordinary `names.structs.contains(name)` arm and resolves to the struct correctly. There is no real collision for `resolve` to have, so `E188` never fires for a `STRUCT Array = #{...}`-shaped declaration. (Structs are never generic, so there is no way to write `Array<T>` meaning "a struct named Array parameterized by T" in the first place — the collision the generic-head special case exists to arbitrate simply cannot arise for a struct.)
- **`void`**. Unlike the scalar leaves, `resolve`'s `Named` arm has no explicit `"void"` case at all — an unmatched name falls straight through to the struct-lookup arm. A `STRUCT void = #{...}` therefore resolves fine through a bare annotation; `E188` never fires for it.
- **Declared `LIST` names or registered `Handle<K>` kinds.** `names.lists`/`names.handles` are only ever consulted inside `List<L>`/`Handle<K>`'s own generic-argument position, never against a bare `Named` annotation — a different namespace from `names.structs` entirely, with nothing to collide."#,
    ),
    (
        DiagnosticCode::E193,
        r#"A classic `~ temp` belongs to its knot's **call frame**, not to a lexical
block. Every read anywhere in that frame — the knot body, any choice branch,
the gather, any of the knot's stitches — resolves to the same slot. What
resolution alone cannot say is whether the declaring statement has *run* by
the time a given read executes.

`brink_analyzer::temp_dominance` answers that structurally, over the HIR
block tree, with no control-flow graph:

> a `~ temp` declaration `D` sitting directly in block `B` dominates exactly
> those reads that lie inside `B`'s own subtree and start at or after `D`'s
> end.

Reaching any point in `B`'s subtree past `D` means executing `B`'s
statements in order through `D` first, so nesting below `D` — a choice set,
a conditional, a labeled gather — is still behind it. Everything else in the
region is a different block's subtree, which is what makes both of the
ruled shapes fall out of one rule:

1. a sibling choice branch declares it, another one reads it;
2. a gather is reached from a branch that did not declare it;
3. the read is written textually ahead of the declaration.

Each of a knot's root body and every one of its stitch bodies is checked as
its own independent region: a `~ temp` declared in one is never looked up
for a read in another. (A fourth shape this page used to enumerate — a
stitch reading a temp declared at its knot's root — turned out not to be a
dominance question at all: it fires unconditionally, dominance aside, and
inklecate rejects the identical program outright rather than warning on it.
The 2026-09-01 follow-up ruling on #3373 moved it into its own compat-deny
code, [`E194`](E194.md).) The rule deliberately does not model diverts
within a region either — a divert that re-enters a gather inside the same
block *after* the declaration ran is not a defect and is not reported.

**Why it is a warning and not an error.** The C# reference runtime prints
`RUNTIME WARNING: Variable not found: 'n'. Using default value of 0 (false).
This can happen with temporary variables if the declaration hasn't yet been
hit.` and keeps playing, which is why the pattern reaches authors as "it
works fine in Inky". Brink now plays it the same way — `Opcode::GetTemp`
reads an unset slot as `0` and raises a `brink_runtime::RuntimeWarning`
through the same channel — so the ink-compat floor stays honest, and this
diagnostic is the half that arrives before the author ever presses play.
(RULED 2026-09-01, option C on issue #3354; `docs/compiler-spec.md` "Temp
scope and definite assignment" and `docs/runtime-spec.md` "Uninitialized temp
reads".)

**What does not fire.** A knot or stitch *parameter* is bound at call time,
so a name that is also a parameter of its enclosing definition is never
reported — matching `lir::lower::temps::alloc_temps`, which gives the
parameter the slot and lets a same-named `~ temp` write through it. A plain
assignment target (`~ n = 1`) is a write, not a read. A `temp` declared
inside a `~ { … }` block is [`E082`](E082.md)'s subject — a lexical-scope
defect, not a definite-assignment one. Reads inside a lambda body are skipped
because the lambda's own parameters shadow the enclosing frame."#,
    ),
    (
        DiagnosticCode::E194,
        r#"`brink_ir::lir::lower::temps::alloc_temps` walks a knot's own body plus
every one of its stitch bodies before lowering begins and gives each `~
temp` name one slot in that shared frame — so, mechanically, nothing stops
a stitch from reading a name only the knot's root declares. The program
compiles and plays.

Ink's own compiler does not extend a knot's `~ temp` visibility into its
stitches at all. A stitch is, for `~ temp` purposes, a separate scope from
its knot's root content — referencing the knot's temp from inside a stitch
is `Unresolved variable` in inklecate, full stop, independent of whether
the divert that entered the stitch happened to run the declaration first:

```ink,fires(E194)
-> k
=== k ===
~ temp n = 7
-> s
= s
Stitch sees {n}.
-> END
```

By default this does not compile at all (`E194` is `Error`-tier); once
downgraded (see "Fixing it" below) it plays `Stitch sees 7.` in brink,
while inklecate rejects it outright — the declaration having already run
when the divert reaches `s` makes no difference to ink's compiler. That is
what separates this from
[`E193`](E193.md): `E193` is a genuine dominance question (did the
declaring statement run *on this path* before the read?) that the runtime
resolves the same way ink's runtime does, by substituting a default and
warning. This is not a runtime question at all — ink's compiler refuses the
reference regardless of the runtime path, so there is no runtime fallback
to lean on the way `E193`'s has.

**Why compat-deny, not a plain warning.** `docs/compiler-spec.md`
"Compat-deny diagnostics" (issue #3373, RULED 2026-09-01) names the tier:
"inklecate rejects this; brink can run it; you must opt in." Defaulting to
`Error` matches inklecate's own hard rejection, so an ink-compat project
sees the same wall Inky would show it. What makes the tier different from
an ordinary hard error is the admission invariant: brink genuinely produces
a *working* program once a project opts in, so the code stays
`[lints]`-overridable rather than staying a permanent, non-negotiable
error — the ruling's own words: "we should allow it to be turned off if the
user wants, it's annoying."

**What fires: reads AND plain writes.** A plain assignment (`~ n = 9`) in a
stitch to a name only the knot's root declares fires exactly like a read —
assigning still has to *resolve* `n` to a slot before it can store into it,
and inklecate rejects that resolution too, just with a different message
(`Variable could not be found to assign to: 'n'` rather than `Unresolved
variable: n`):

```ink,fires(E194)
-> k
=== k ===
~ temp n = 7
-> s
= s
~ n = 9
Knot temp is now assigned.
-> END
```

**What does not fire.** A stitch parameter of the same name is bound at
call time and is never reported. A stitch that declares its *own* `~ temp`
of the same name shadows the knot's for that stitch's reads and writes
entirely — that is [`E193`](E193.md)'s question (does the stitch's own
declaration dominate its own reads?), not this one. A read or write inside
the knot's own root body, or inside another stitch that itself declares the
name, is untouched by this check. A compound assignment (`~ n += 1`) or
`~ n++`/`~ n--` reads the name before writing it back, so it is reported as
a read, not a write — the message still names the right operation because
`ReadCollector` (shared with `E193`) only discounts a plain `Set` target as
"not a read", never a compound one."#,
    ),
    (
        DiagnosticCode::E195,
        r#"The check runs once per choice line, during HIR lowering
(`hir::lower::choice::LowerChoice::lower_choice`), and looks at exactly the
evidence inklecate's own parser looks at: the choice's own line, not
whatever is nested underneath it. It fires only when **all** of the
following hold:

- no divert on the choice's own line — `* ->` counts as having one, even
  though the divert has no target; only a line with no `->` token at all
  counts as "no divert",
- no tag directly on the choice line (`* #tag`) — matching inklecate, which
  does not warn on a tag-only choice either,
- and no real text in any of the three same-line content regions ink's
  grammar gives a choice (`text[bracket]inner`) — including an *explicit but
  empty* `[]`, which still parses to a zero-width content node, not to
  nothing.

**A `(label)` or `{condition}` guard does not exempt a choice from this
check.** The reference's own `emptyContent` computation
(`startContent`/`innerContent`/`optionOnlyContent`) has no such carve-out,
and measurement against inklecate confirms it fires anyway: both `* (opt)`
and `VAR x = true` / `* {x}`, each followed by a blank line, still emit
"Choice is completely empty…" — see the fires examples below.

Nested content *underneath* the choice line — the block that plays after the
choice is selected — is never consulted. `* []` followed by an indented
paragraph still fires: inklecate's own check works the same way, since the
nested block is parsed as a separate weave continuation, after the single
line `Choice()` has already decided whether to warn.

**Why the check lives in lowering, not in a later analyzer pass over the
built `hir::Choice`** (contrast [E034](E034.md), which runs entirely over
already-lowered `Choice` values): an explicit-but-empty divert (`* ->`) and
no divert at all (`* []`) are indistinguishable once lowered — both leave no
`Stmt::Divert` in the choice's `body.stmts`, since a target-less divert
carries no target to lower into one. Whether a `->` token was written at
all is evidence that exists only on the AST, at the point `lower_choice`
already has it in hand, so the check runs there instead of being
reconstructed later from a shape that has already thrown the distinction
away.

**Ink surface only.** This is not wired into the native `{? … }` surface's
own `lower_choice`. inklecate is an ink-only tool, so ink is the surface
this diagnostic's parity claim is actually about — but the deeper reason is
that the same rule would be actively wrong for native: native choices
routinely put their only divert *inside* the choice's braced body
(`{? * { -> knot } }`), which this check's same-line-only evidence does not
see, so wiring it in as written would warn on completely ordinary native
code. Native already has its own, unambiguous slot for "no visible option"
— `else { … }` — which lowers with `is_fallback: true` and needs no warning
about being empty; it is supposed to be."#,
    ),
    (
        DiagnosticCode::E110,
        r"`#@effects(…)` was the original tag-channel spelling of a knot/stitch's effects assertion. The `@[effects(…)]` annotation is the final NS-A2 form (`docs/stdlib-spec.md` §9.2, ruled 2026-07-18), and the two spellings are **not** interchangeable text: `#@effects(…)` keeps the legacy **colon** argument grammar (`reads: gold, hp`) frozen forever, while `@[effects(…)]` uses the amended **paren-clause** grammar (`reads(gold, hp)`, 2026-07-19). The tag spelling still parses — nothing about the assertion's meaning changes — but every new definition should use the annotation spelling, and this warning is how an existing `#@effects(…)` site is found.",
    ),
];
