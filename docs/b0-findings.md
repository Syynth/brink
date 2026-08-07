# B0 decomposition — open findings (PROPOSED, phone-rulable)

Companion to `b0-sequencing.md`. Reply `NF1: a` style. Each finding: the
tension, options, recommendation. All PROPOSED; nothing binds until the
decision-log says so. Standing caveat inherited from the batch-4 delegated
ruling: these build on Q1–Q7 rulings that were adopted not-fully-reviewed —
surprises get flagged, not presumed settled.

**Status update — RULED 2026-07-19 (evening walkthrough): NF-1–NF-6 are
all ruled** (batch 7); dated stamps sit at each section below. NF-4's
ruling **overrides** the recommendation; NF-5's extends it with a new
slice (B0.8b). See the decision-log's consolidated 2026-07-19
evening-walkthrough entry. No B0 slice remains ruling-blocked.

---

## NF-1 — Where does the native lexer/CST live?

**Tension.** `brink-syntax` is the ink frontend: its `SyntaxKind` is the
~230-variant **ink grammar** enum (`crates/internal/brink-syntax/src/
syntax_kind.rs`), and its rowan/AST infrastructure is shaped around ink's
weave. The native grammar shares almost no token vocabulary with ink
(braces-as-structure, keyword declarations, `::`, `@[…]`, pipes).

- **(a) New internal crate — `crates/internal/brink-syntax-native`.** Own
  `SyntaxKind` space, own rowan tree, own AST layer; depends on nothing
  ink-shaped. The oracle-guarded ink parser is physically untouchable from
  the parser lane; B0.5 pumps in parallel with zero merge risk against the
  contract spine. Small cost: some rowan boilerplate duplicated (green-tree
  builder setup, ptr utilities) — extract a tiny shared-infra crate only if
  duplication demonstrably hurts.
- **(b) A `native::` module inside `brink-syntax`.** Shares infra; but the
  only *architectural* reason to co-locate was sharing `SyntaxKind` space so
  `AstPtr` keeps working — which is exactly the contract's rejected Q1(c),
  and Q1(b)'s opaque `Provenance` + per-frontend resolver removes the need
  entirely. Co-location also puts every native-grammar PR in the
  blast-radius review scope of the ink parser.

**Recommendation: (a).** Q1(b) already spent the decision that makes this
clean: frontends are peers behind opaque provenance, so the second
frontend's syntax layer should be a peer crate. The two-client framing
(chart as a body-dialect *inside* the native frontend, Q5(a)) also lives
more honestly in a crate whose parser owns the body-dialect dispatch seam.

**RULED 2026-07-19 (evening walkthrough): (a)** — the native lexer/CST is
a new peer crate, **`crates/internal/brink-syntax-native`**. B0.5's text
in `b0-sequencing.md` names the crate.

---

## NF-2 — Full charter surface, or a deliberately-minimal writer-sufficient subset first?

**Tension.** The §10 exit criterion is **writer validation, not
completeness** — ratification follows real authored content. But the
charter + 2026-07-19 rulings now describe a large surface (lambdas,
enums, companions, `use`-trees, `match` exhaustiveness), parts of which
have **no HIR support** (lambdas, enums — F-K) and parts of which are
explicitly reshaped later (B1–B5, deliberately unfiled).

- **(a) Full charter surface in B0.** Maximizes coverage; but it drags
  semantics-adjacent HIR additions (lambda node, `EnumDecl`) into what is
  chartered as a same-semantics respelling round, co-develops spellings the
  code-dialect sitting owns, and pushes the writer's first scene months
  out.
- **(b) Writer-sufficient subset: prose dialect complete, code dialect
  minimal.** Prose is the writer's medium — B0.7 ships all of charter §5/§6
  (points, dissolved gather, annotated braces, diverts/tunnels, tags).
  Code dialect (B0.8) ships only what lowers to *existing, Track-A-tested*
  HIR: let/assign/call/if/match-stmt/while/for(single-binding)/return/
  UFCS-call-shape/`#fn`-values/await. Deferred loudly (parsed, rejected
  with "ruled but not yet lowered" per the §4.4 additive-open posture):
  lambdas, enums, impl/companions, `for k,v` (B2's additive field), B1/B4/
  B5 spellings.

**Recommendation: (b).** Every deferred construct is either waiting on the
code-dialect sitting anyway or needs an HIR addition that violates the
same-semantics ground rule. The three-axes ruling is still honored: a
code-bodied `flow` is honestly spellable in the subset. Rider: publish the
gap list to the writer at first light so the friction journal separates
"missing feature" from "confusing syntax" — both are data, but they route
differently.

**RULED 2026-07-19 (evening walkthrough): (b)** — writer-sufficient
subset: prose dialect complete, code dialect minimal (only constructs that
lower to existing HIR); deferred constructs parse but are rejected loudly
("ruled but not yet lowered"); the gap list is published at **writer
onboarding** (which, per NF-4's ruling below, is B0 completion, not first
light).

---

## NF-3 — How does `.brink` file discovery integrate with the db/project layer?

**Tension.** Today the db discovers sources through the **entry +
INCLUDE-closure** model (`brink-db` `resolve_include_path`, closure-scoped
error gating per the 2026-07-19 compileProject ruling), and
`brink-project-config` walks up from an entry `.ink` file to `brink.toml`.
The modules ruling says the native tree is **filesystem-derived, the tree
is the compilation universe, imports are naming only, textual INCLUDE is
dead** — a different discovery model with many roots.

- **(a) Full module-system discovery in B0**: walk the project tree for
  `.brink`, `story::` root, many roots, engine-only-reachable modules.
  The ruled end state, but it fronts a pile of db/salsa work (directory
  watching, tree-shaped file sets, root enumeration) before any scene
  compiles.
- **(b) Declared source root, tree-derived paths**: `brink.toml` names a
  source root (riding the existing config-discovery machinery); every
  `.brink` under it is in the universe; module path = relative path
  (snake_case segments, compile-checked per S4's casing partition); the
  INCLUDE machinery is never consumed for `.brink`. A strict subset of (a)
  — nothing throwaway; (a)'s remaining work is watching + multi-root
  ergonomics, later.
- **(c) Ride the ink INCLUDE closure.** Rejected out of hand — the charter
  kills textual INCLUDE on the native surface (§13.2, D6).

**Recommendation: (b).** It lands the load-bearing semantic rule (path on
disk = path in language) with the smallest project-layer footprint, and it
keeps mixed trees simple for B0: ink stories and `.brink` stories coexist
at the project level as separate entries/roots; intra-story mixing and
converters stay in charter §8.5's later round. Rider to confirm: an
ink-dialect `INCLUDE` of a `.brink` file (or vice versa) is a hard error,
not a silent skip.

**RULED 2026-07-19 (evening walkthrough): (b)** — declared source root in
`brink.toml`; module path = relative path; the INCLUDE machinery is never
consumed for `.brink`. **RIDER, ruled with it (confirming the above):
cross-dialect INCLUDE — `.ink`↔`.brink`, either direction — is a hard
error, never a silent skip.**

---

## NF-4 — What is the earliest slice at which the writer can author a scene? (the season clock)

**The answer under this decomposition**: after **B0.7 + B0.10's
first-light checkpoint** — prose-dialect complete, declarations lowering,
`.brink` compiling end-to-end — with B0.8 (code dialect) allowed to trail.
On the critical path that is 6 spine slices + the checkpoint
(B0.1→B0.2→B0.3→B0.4→B0.6→B0.7), with B0.5 hidden in parallel.

- **(a) Gate writer onboarding on all of B0** (incl. B0.8/B0.9 complete).
  Cleaner story, later journal; the friction journal — the season's actual
  deliverable per §10 — gets the least calendar time.
- **(b) Open the journal at first light** (post-B0.7), gap list published
  (NF-2 rider), code-dialect scenes arriving as B0.8 lands.

**Recommendation: (b).** The exit criterion is a *journal with entries*,
and the journal instrument improves with time-in-hands more than with
surface completeness; prose-first is also how the writer works. **The real
schedule risk is NS-T (#1131), not the parser**: the writer validates
through the editor, so first light needs at least highlighting +
diagnostics surfacing. Rider: the maintainer should explicitly
prioritize the NS-T minimum bar against B0.6/B0.7's timeline — B0 cannot
ship the season's exit criterion alone.

**RULED 2026-07-19 (evening walkthrough): (a) — this OVERRIDES the
recommendation (b) above.** Writer onboarding gates on **ALL of B0**
(including B0.8/B0.9/B0.10 — and B0.8b's ratification gate, per NF-5's
ruling), not on the first-light checkpoint. The NS-T timing pressure is
correspondingly **relaxed but not void**: the editor minimum bar has B0's
full runway, but it still gates onboarding itself. B0.10's text in
`b0-sequencing.md` is updated accordingly.

---

## NF-5 — How is "lowers to *tested* HIR" actually tested? (the differential method)

**Tension.** The tracker's end state says the prototype parser lowers to
*tested* HIR. Native surface is vanilla-unreachable, so the oracle never
sees it directly; HIR snapshots alone test shape, not semantics.

- **(a) Native fixture HIR snapshots only.** Cheap, but proves nothing
  about behavior.
- **(b) Respelled-differential episode tests.** For selected tier-1 cases
  (+ the two charter exhibits): author the same story in ink and `.brink`,
  compile both, assert **episode-identical** replay on the existing
  harness. The oracle then guards the native surface transitively (ink ↔
  oracle is already held; native ↔ ink is the new leg). For code-dialect
  logic, the cheaper leg: `.brink` vs brink-dialect lowered HIR asserted
  equal modulo provenance.
- **(c) Both — snapshots for shape/regression granularity, differentials
  for semantics.**

**Recommendation: (c), with (b) as the flagship exit gate for B0.7/B0.8.**
Riders: the respelled corpus is hand-curated (a handful of cases spanning
weave/choice/tunnel/thread/alternation semantics), not a mechanical
translation of all 390 cases — a converter is charter §8.5's later round;
and respelled cases live beside their ink twins in-tree so drift is
reviewable.

**RULED 2026-07-19 (evening walkthrough): (c+)** — (c) as recommended
(snapshots + hand-curated respelled differentials as the B0.7/B0.8 exit
gates), **PLUS a new slice — B0.8b: HIR→brink emitter + mechanical corpus
converter** (parallel lane; entry: B0.8 merged): emit `.brink` from HIR
(comment-free output is fine for corpus purposes), mechanically convert
the full 390-case corpus, and the **full-corpus episode-identity
differential becomes the B0 ratification exit gate**. This extends the
hand-curated-only rider above: the hand-curated set remains the B0.7/B0.8
gate; the mechanical corpus differential is B0's *ratification* gate. The
ruling's rationale is deliberate machinery-sharing — the emitter is shared
with the future `.brink` formatter and printer-based IDE rewrites.
Crate-name suggestion: **`brink-respell`** — NEVER `brink-converter`,
which names the retired `.ink.json` crate (#544). Slice text in
`b0-sequencing.md` §2.

---

## NF-6 — Does the B0.3 admission validator run always-on, or dev/test-only?

**Tension.** Q2(a) rules "loud admission checks", and the tier-1 posture
(2026-07-18 capability-manifest ruling) says never-fail-silently at the
admission boundary. But the validator runs inside `lowered_query` — the
salsa hot path every keystroke touches in the editor.

- **(a) Always-on.** The ruled posture, honestly applied: admission is a
  boundary, boundaries don't have modes. Checks are O(n)/O(n log n) over a
  single file's HIR + manifest — plausibly cheap; measure, don't assume.
- **(b) Dev/test-only.** Protects the editor hot path, but reintroduces
  exactly the silent-in-production failure mode D2 exists to kill, and
  does it at the boundary whose whole point is loudness.

**Recommendation: (a)**, with a measured perf budget in B0.3's exit
criteria. If a *specific* check breaches the budget, that one check may be
proposed for dev-mode demotion via the A4 dev/prod knob precedent — a
maintainer ruling per check, never a blanket switch. (Note the fence:
the dev/prod split is ruled available only where prod behavior is defined
and fabricates nothing — a skipped admission check "fabricates" trust, so
any demotion request should expect pushback; that is by design.)

**RULED 2026-07-19 (evening walkthrough): (a)** — the admission validator
is always-on; a **measured perf budget** joins B0.3's exit criteria;
per-check dev-mode demotion happens only by an individual maintainer
ruling per check, never a blanket switch.

---

## Tensions noted for the record (not phone-rulable, watch items)

- **T-1 — D4 neutrals.** B0.7 stamps `depth = 0` / `context = Inline` as
  native-normal instead of faking ink's weave fold. The compiler-spec
  claims downstream never inspects `depth`; the contract wanted the fields
  optional/removed but the Q-batch didn't rule it. If any pass disagrees,
  it surfaces in the respelled-differential tests — flag loudly, do not
  patch the neutral values to whatever makes the diff pass (that would be
  symptom-patching the exact coupling the contract exists to kill).
- **T-2 — Q7-before-Q1 ordering.** The contract text calls Q7(a) "a
  prerequisite for Q1(b)"; the ruled sequence puts Q1(b) first. The
  decomposition reconciles via the B0.1 shim + immediate B0.2 retirement
  (one review train). If the shim turns out ugly in practice, landing
  B0.2's `ReturnKind` *first* is semantically safe and even smaller — the
  build agent may propose the swap at plan time.
- **T-3 — `@[effects]` recognizer drift (D9).** B0.5 implements the ruled
  paren-clause grammar for native; the *ink* recognizer still parses the
  colon form, contradicting the tower ruling. That fix belongs to the ink
  frontend's own light lane, not B0 — but the longer both exist, the more
  fixtures accrete against the drifted form. Worth a light-lane slot soon.
- **T-4 — the accept-list's double bookkeeping.** B0.9's accept-list and
  B0.3's frontend-agnostic validator must not drift into overlapping
  half-checks of the same invariants. Discipline: B0.3 checks what *every*
  frontend owes; B0.9 checks what *native* additionally forbids; no check
  appears in both.
