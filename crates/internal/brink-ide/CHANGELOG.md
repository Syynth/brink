# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.12](https://github.com/Syynth/brink/compare/brink-ide-v0.0.11...brink-ide-v0.0.12) - 2026-08-21

### Added

- *(brink-analyzer)* E188 warns when a STRUCT name collides with a reserved builtin/tower type
- *(brink-analyzer,brink-db,brink-ide,brink-lsp,brink-web)* wire the harvest index into cue-name completion ([#2134](https://github.com/Syynth/brink/pull/2134))
- *(brink-ir)* split @[element(claims=…)] into @[convention(claims=…, order=N)]
- *(brink-runtime)* migrate output contract from Line to Step/OutputLine ([#1684](https://github.com/Syynth/brink/pull/1684))
- *(compiler)* markup manifest gains required attributes + widened attr schema ([#1997](https://github.com/Syynth/brink/pull/1997))
- *(lang)* type-name conformance sweep — angle brackets + Uppercase non-primitives, Option<T>/Weighted<T> annotatable ([#1552](https://github.com/Syynth/brink/pull/1552))
- *(compiler)* manifest-validated markup vocabulary (§4.2 second half)
- *(brink-analyzer)* §6.1 row variables on fn-typed params (part of #1680)
- *(brink-ir)* lower inline markup spans to ContentPart::Span ([#1716](https://github.com/Syynth/brink/pull/1716))
- *(brink-ide,brink-lsp,docs)* detect undeclared renames at authoring time (#1672 part 2)
- *(brink-ide,brink-cli)* IDE rename writes #@was on the renamed declaration (#1672 part 1)
- *(ide)* quick-fixes for T1c creation-site + call()/bind() strict diagnostics ([#744](https://github.com/Syynth/brink/pull/744))
- *(brink-ir)* per-branch source spans on CondBranch/SequenceBranch ([#404](https://github.com/Syynth/brink/pull/404))
- *(diagnostics)* brink.toml [lints] control plane — per-code severity + deny-warnings ([#1160](https://github.com/Syynth/brink/pull/1160))
- *(brink-analyzer,brink-db)* B0.9 close — native strict-only enforcement point
- *(NS-A9)* dialect-keyed type-policy default via resolve_type_policy seam
- *(NS-A2)* effect-row extension — emits + tags + faults ([#1108](https://github.com/Syynth/brink/pull/1108))
- *(web)* FS-3w flow-addressed web surface — flow handles, Line::Suspended, wakeCheck ([#978](https://github.com/Syynth/brink/pull/978))
- *(ir,analyzer)* await HIR lowering, strict-ink gate (E051), LIR fence (E052)
- *(effects)* T2-4 tail — effects book chapter, hover row, `brink ide effects-diff`, corpus wing ([#863](https://github.com/Syynth/brink/pull/863))
- *(brink-ide,web,lsp)* auto-import IMPORT quick-fix for out-of-scope refs
- *(brink-ide)* fold the leading IMPORT block
- *(t1c-4)* mechanical tail — corpus growth, book chapter, IDE polish
- *(ide)* inferred-type hover + inlay hints via the per-def FG seam ([#621](https://github.com/Syynth/brink/pull/621))
- *(syntax,ir,analyzer)* TM-4b structs grammar + HIR + analyzer, diagnostics-only ([#665](https://github.com/Syynth/brink/pull/665))
- *(analyzer,db)* TM-3 — types = strict policy, Unknown/Conflicted-escape, E063 wiring ([#619](https://github.com/Syynth/brink/pull/619))
- *(syntax,analyzer)* CONST declarations accept type annotations ([#641](https://github.com/Syynth/brink/pull/641))
- *(syntax)* TM-2 inline type annotation syntax — grammar/HIR/fmt/IDE, feeding signature() ([#618](https://github.com/Syynth/brink/pull/618))
- *(brink-ide,brink-lsp)* T1b-4 IDE polish — stdlib completion, signature help, block folding, hover ([#589](https://github.com/Syynth/brink/pull/589))
- *(compiler)* T1b-1 superset grammar + HIR + dialect gate ([#569](https://github.com/Syynth/brink/pull/569))

### Fixed

- *(analyzer)* resolve a fn-valued CONST global's call site ([#2083](https://github.com/Syynth/brink/pull/2083))
- *(brink-analyzer)* resolve an unannotated ~ temp's shape from its initializer for E063/E185 ([#2906](https://github.com/Syynth/brink/pull/2906))
- *(brink-ide)* address PR #2888 review findings on hover doc drift
- *(brink-ide,brink-web,brink-cli)* apply #2383 review findings for the AnalysisOptions seam
- *(brink-ide,brink-cli,brink-web,brink-lsp)* shared AnalysisOptions forwarding seam ([#2383](https://github.com/Syynth/brink/pull/2383))
- *(brink-analyzer,brink-ide,brink-lsp)* off-db conventions confinement (E169)
- *(brink-ide)* address #2355 review findings on native decorator loss and ink coverage gap
- *(brink-ide)* classify prose text as no token, not variable/operator ([#2293](https://github.com/Syynth/brink/pull/2293))
- *(brink-web,brink-analyzer,brink-ide)* address #2327 review findings on is_all_native reachability and docs
- *(brink-db)* brink.toml sharing a session no longer disqualifies is_all_native ([#2318](https://github.com/Syynth/brink/pull/2318))
- *(brink-ide,brink-lsp)* correct misattributed E169 causal claims (#2316 review F1)
- *(brink-ide,brink-web,brink-lsp)* thread [project] conventions into the editor/LSP live db ([#1880](https://github.com/Syynth/brink/pull/1880))
- *(brink-ide)* fold SCENE_SLUG into the FN_DECL arm — clippy match_same_arms
- *(brink-ide,brink-web)* cover scene headings, cue/tag prose runs, and range-native (#2286 review)
- *(brink-ide,brink-lsp)* stop offering multi-word cue completions past the first word
- *(brink-syntax,brink-analyzer,brink-ir,brink-ide)* address PR #2271 review findings
- *(brink-ir,brink-ide)* apply PR #1845 review findings — claim-ref rename corruption, E166/E167 renumber, spec drift
- *(brink-ide,brink-lsp)* PR #1711 review findings for #1672 rename/#@was
- *(brink-ide)* update #1347 divergence canary now that #1358 resolves it
- *(review)* address #1662 findings — vacuous overlay/projection guards, doc wording
- *(docs)* address review findings on live-typing-diagnostics-divergence.md
- *(brink-ide,brink-lsp)* address PR #1626 review findings
- *(brink-ir)* branchless-body first arm must not contain the else arm ([#404](https://github.com/Syynth/brink/pull/404))
- *(analyzer)* address review findings on import-alias precedence ([#1596](https://github.com/Syynth/brink/pull/1596))
- *(analyzer)* honor import aliases in resolution, not just E089 ([#1590](https://github.com/Syynth/brink/pull/1590))
- *(brink-ide+brink-db+brink-analyzer)* address review findings on PR #1584 ([#530](https://github.com/Syynth/brink/pull/530))
- *(brink-db+brink-analyzer+brink-ide)* serve Param/Temp signatures via a per-file locals path ([#530](https://github.com/Syynth/brink/pull/530))
- *(brink-ide)* cover Label variant in rename tail-narrowing + fix ufcs_hover exclusion doc
- *(brink-ide)* narrow tail-segment + decl-initializer rename ranges ([#1571](https://github.com/Syynth/brink/pull/1571))
- *(lsp)* unpin M-2d native-homonym diagnostics from declared dialect; fix stale prose
- *(brink-ide)* address review findings on #1560 field-access rename fix
- *(brink-ide)* narrow field-access-fallback range for plain p.x.y renames
- *(brink-ide)* guard sync_db_options against unchanged-value memo invalidation
- *(brink-ide)* move UFCS receiver-rename fix from analyzer to brink-ide
- *(brink-ide)* avoid clippy::panic in the new receiver-rename test
- *(brink-analyzer)* UFCS rename no longer corrupts receiver.method calls
- *(brink-db,brink-ide)* address PR #1547 review findings
- *(brink-ide)* close review findings on #1539's UFCS navigation trio
- *(brink-ide)* route def --at, find_references, and rename through the UFCS verdict table
- *(brink-ide)* stop overstating D1 competition in FieldCall hover text
- *(brink-ide)* handle FreeFnAutoRef verdict in UFCS hover/go-to-def
- *(brink-ide)* scope #1385 ruling to today's compile(&Environment) entry point
- *(review)* correct stale merge-semantics docs + strengthen reapply test ([#1397](https://github.com/Syynth/brink/pull/1397))
- *(brink-cli)* Project::ide_session() applies resolved [lints]/dialect/types
- *(brink-web,brink-ide)* review fixes for #1366 wasm-lints PR
- *(brink-web,brink-ide)* wire brink.toml [lints]/deny-warnings into the wasm editor session
- *(brink-ide,brink-lsp)* review fixes for #1367 diagnostic-severity PR
- *(brink-ide,brink-lsp,brink-web)* diagnostic display sites use effective severity, not raw default
- *(brink-web,docs)* correct false [lints]-reachability claims; document schema
- *(ide)* extend semantic-type honesty to inlay hints and argument widgets
- *(brink-analyzer)* preserve nominal LIST name through InferredType ([#628](https://github.com/Syynth/brink/pull/628))
- *(brink-ide)* char-boundary panic in ref/dotted-path completion scans
- *(brink-ide)* scope fn-value hover resolution lookups to the slot's own file
- *(ide)* thread declared dialect into IdeSession's background analysis ([#611](https://github.com/Syynth/brink/pull/611))
- *(editor)* narrative-run fold anchors on the choice line, strips cue sigils in the pill ([#417](https://github.com/Syynth/brink/pull/417))

### Other

- cargo fmt for #2083's analyzer fix and its new tests
- Address review findings on PR #2923 (issue #2918)
- Fix E185 review findings: unresolved-RHS blind spot, spec drift, off-db test
- *(brink-ide)* route native hover fixtures through the real brink.toml config path ([#2885](https://github.com/Syynth/brink/pull/2885))
- *(brink-ide)* pin shadowing-wins-over-builtin hover on both roads ([#2864](https://github.com/Syynth/brink/pull/2864))
- Apply #2866 review findings: missed builtin-hover copy, self-testing mutator test, stale docs
- unify reserved builtin/stdlib name lists into one canonical source
- Merge branch 'main' into auto/issue-2359
- Merge branch 'main' into auto/issue-2291
- Merge branch 'main' into auto/issue-2293
- Merge remote-tracking branch 'origin/main' into train-fix
- checkpoint before merging origin/main (issue #2108)
- rename [project] elements to [project] conventions with deprecated alias ([#2180](https://github.com/Syynth/brink/pull/2180))
- Merge remote-tracking branch 'origin/main' into train-fix
- *(brink-ide)* @[style(...)] declaration surface reaches hover ([#1719](https://github.com/Syynth/brink/pull/1719))
- *(brink-ir/brink-analyzer/brink-db)* confine pattern-claiming handlers to the brink.toml-named conventions module ([#1844](https://github.com/Syynth/brink/pull/1844))
- merge origin/main into train-fix for PR #1662
- *(#1347)* keep the divergence helper out of the expect_used lint
- *(#1347)* measure the live-typing vs. db diagnostic divergence
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge remote-tracking branch 'origin/main' into train-fix
- *(brink-ide)* cover FieldCall and FreeFnAutoRef verdicts in hover/go-to-def
- Merge remote-tracking branch 'origin/main' into auto/issue-1507
- *(brink-ide)* UFCS hover + go-to-def wiring ([#1507](https://github.com/Syynth/brink/pull/1507)) — UNVERIFIED, see issue
- *(ide,web)* rule that IdeSession::compile stays on the imperative salsa path ([#1385](https://github.com/Syynth/brink/pull/1385))
- migrate every HIR ptr field to opaque Provenance
- Merge remote-tracking branch 'origin/main' into train-fix
- *(brink-web)* collapse compileProject onto the analysis ProjectDb ([#1032](https://github.com/Syynth/brink/pull/1032))
- path-projections tooling tail (docs/t1e-spec.md §8 item 3, #850)
- M-2 clippy/fmt polish + @brink-lang/web changeset
- Merge pull request #691 from Syynth/auto/issue-621
- sweep converter mentions after pipeline retirement ([#544](https://github.com/Syynth/brink/pull/544))
- setSemanticTypeDiagnosticSeverity lever ([#532](https://github.com/Syynth/brink/pull/532))
- *(ide)* accept line-context snapshot for choice-body fold re-anchor ([#417](https://github.com/Syynth/brink/pull/417))
- extract the symbol service from brink-analyzer (phase 0 slice A) ([#509](https://github.com/Syynth/brink/pull/509))

## [0.0.10](https://github.com/Syynth/brink/compare/brink-ide-v0.0.9...brink-ide-v0.0.10) - 2026-07-10

### Added

- *(ide)* machinery/narrative fold runs — weave-bounded, opt-in, gated ([#479](https://github.com/Syynth/brink/pull/479))
- *(ide)* weave folding from projection container extents ([#476](https://github.com/Syynth/brink/pull/476))
- HIR structural projection producer — project_hir (#454 phase 1) ([#465](https://github.com/Syynth/brink/pull/465))

### Fixed

- *(ide)* line-classification fixes for the embedder contract ([#478](https://github.com/Syynth/brink/pull/478))

### Other

- LineInfo on one shared projection: cache, option_path, standalone ([#480](https://github.com/Syynth/brink/pull/480)) ([#489](https://github.com/Syynth/brink/pull/489))
- Merge pull request #483 from Syynth/worktree-476-weave-folding
- line_context + folding as views over the HIR projection ([#463](https://github.com/Syynth/brink/pull/463)) ([#471](https://github.com/Syynth/brink/pull/471))
- shared read-only HIR visitor + migrate 4 walkers ([#457](https://github.com/Syynth/brink/pull/457)) ([#464](https://github.com/Syynth/brink/pull/464))
- *(runtime)* Program → Arc, delete <'p> lifetime (F1.1) ([#442](https://github.com/Syynth/brink/pull/442))

## [0.0.9](https://github.com/Syynth/brink/compare/brink-ide-v0.0.7...brink-ide-v0.0.9) - 2026-07-06

### Added

- *(ide,editor,web)* fold kinds — structural/machinery/narrative + summary pills ([#365](https://github.com/Syynth/brink/pull/365)) ([#400](https://github.com/Syynth/brink/pull/400))
- *(ir,ide,web)* dialogue-dialect schema + Rust classification ([#368](https://github.com/Syynth/brink/pull/368)) ([#386](https://github.com/Syynth/brink/pull/386))
- *(ide,web)* story-graph edges carry source-span occurrences ([#371](https://github.com/Syynth/brink/pull/371)) ([#378](https://github.com/Syynth/brink/pull/378))
- *(ide,web)* extract selection to knot/function ops (#315 H) ([#341](https://github.com/Syynth/brink/pull/341))
- *(ide,web)* atomic reference-aware rename_dir ([#314](https://github.com/Syynth/brink/pull/314)) ([#342](https://github.com/Syynth/brink/pull/342))
- *(ide,web)* unified StructuralResult + deleteSymbol + op-wide breakage gate ([#316](https://github.com/Syynth/brink/pull/316)) ([#336](https://github.com/Syynth/brink/pull/336))
- *(brink-web)* wasm resolve_code_action op with self-describing action data ([#321](https://github.com/Syynth/brink/pull/321)) ([#328](https://github.com/Syynth/brink/pull/328))
- *(studio)* knot/stitch Rename — safe-by-default + breakage report ([#305](https://github.com/Syynth/brink/pull/305)) ([#306](https://github.com/Syynth/brink/pull/306))

### Fixed

- *(release)* path-only dev-deps in brink-ide — unblock stuck 0.0.8 publish ([#419](https://github.com/Syynth/brink/pull/419))
- *(ide,editor)* sigil-wins-chain + conditional scaffold classification ([#413](https://github.com/Syynth/brink/pull/413)) ([#425](https://github.com/Syynth/brink/pull/425))

### Other

- release v0.0.8 ([#307](https://github.com/Syynth/brink/pull/307))
- #312 + #313 (Track N core): shared INCLUDE-block detector + fold/auto-import cores ([#331](https://github.com/Syynth/brink/pull/331))
- *(brink-ide,brink-db)* regression coverage for shallower file-move outbound INCLUDE rewrite ([#325](https://github.com/Syynth/brink/pull/325))

## [0.0.8](https://github.com/Syynth/brink/compare/brink-ide-v0.0.7...brink-ide-v0.0.8) - 2026-07-01

### Added

- *(ide,web)* extract selection to knot/function ops (#315 H) ([#341](https://github.com/Syynth/brink/pull/341))
- *(ide,web)* atomic reference-aware rename_dir ([#314](https://github.com/Syynth/brink/pull/314)) ([#342](https://github.com/Syynth/brink/pull/342))
- *(ide,web)* unified StructuralResult + deleteSymbol + op-wide breakage gate ([#316](https://github.com/Syynth/brink/pull/316)) ([#336](https://github.com/Syynth/brink/pull/336))
- *(brink-web)* wasm resolve_code_action op with self-describing action data ([#321](https://github.com/Syynth/brink/pull/321)) ([#328](https://github.com/Syynth/brink/pull/328))
- *(studio)* knot/stitch Rename — safe-by-default + breakage report ([#305](https://github.com/Syynth/brink/pull/305)) ([#306](https://github.com/Syynth/brink/pull/306))

### Other

- #312 + #313 (Track N core): shared INCLUDE-block detector + fold/auto-import cores ([#331](https://github.com/Syynth/brink/pull/331))
- *(brink-ide,brink-db)* regression coverage for shallower file-move outbound INCLUDE rewrite ([#325](https://github.com/Syynth/brink/pull/325))

## [0.0.7](https://github.com/Syynth/brink/compare/brink-ide-v0.0.6...brink-ide-v0.0.7) - 2026-06-20

### Added

- *(cli)* brink ide move-file / refactor * / actions ([#293](https://github.com/Syynth/brink/pull/293)) ([#300](https://github.com/Syynth/brink/pull/300))

### Fixed

- *(brink-ide)* fold same-file ref edits into promote/demote/move new_source ([#302](https://github.com/Syynth/brink/pull/302))

### Other

- welcoming README (playground/book links, overview, LLM-experiment disclaimer) ([#285](https://github.com/Syynth/brink/pull/285))

## [0.0.6](https://github.com/Syynth/brink/compare/brink-ide-v0.0.5...brink-ide-v0.0.6) - 2026-06-19

### Added

- *(studio)* host functions panel categories + search ([#210](https://github.com/Syynth/brink/pull/210)) ([#270](https://github.com/Syynth/brink/pull/270))

### Fixed

- *(ide)* don't highlight prose words that match ink keywords ([#275](https://github.com/Syynth/brink/pull/275)) ([#277](https://github.com/Syynth/brink/pull/277))

## [0.0.5](https://github.com/Syynth/brink/compare/brink-ide-v0.0.4...brink-ide-v0.0.5) - 2026-06-17

### Added

- *(ide)* file rename/move core (#164 Stage 3, PR A) ([#252](https://github.com/Syynth/brink/pull/252))
- *(brink-ide,web)* host-sourced value-lists in the call Form ([#237](https://github.com/Syynth/brink/pull/237))
- *(brink-ide,studio)* drive the call Form from signature metadata ([#233](https://github.com/Syynth/brink/pull/233))
- *(studio)* typed argument widgets in the call Form + live inter-arg context ([#223](https://github.com/Syynth/brink/pull/223))
- *(studio)* argument widgets stage 5 — arg-groups + inter-arg context + modal ([#222](https://github.com/Syynth/brink/pull/222))
- *(studio)* argument widgets stage 3 — the call Form + launchers ([#220](https://github.com/Syynth/brink/pull/220))
- *(studio)* argument widgets stage 2 — argument_widgets query + Fill ([#219](https://github.com/Syynth/brink/pull/219))
- *(studio)* argument widgets stage 1 — registry + light color picker ([#218](https://github.com/Syynth/brink/pull/218))

## [0.0.4](https://github.com/Syynth/brink/compare/brink-ide-v0.0.3...brink-ide-v0.0.4) - 2026-06-15

### Added

- *(ide,web)* host value push-cache transport for the argument picker ([#174](https://github.com/Syynth/brink/pull/174)) ([#205](https://github.com/Syynth/brink/pull/205))
- *(ide,studio)* arg-position value completion dropdown ([#175](https://github.com/Syynth/brink/pull/175)) ([#204](https://github.com/Syynth/brink/pull/204))
- *(manifest,ide)* static value-source + value-label inlay hints ([#174](https://github.com/Syynth/brink/pull/174)) ([#203](https://github.com/Syynth/brink/pull/203))

## [0.0.3](https://github.com/Syynth/brink/compare/brink-ide-v0.0.2...brink-ide-v0.0.3) - 2026-06-13

### Added

- *(ide,web)* story-graph extraction query, wasm-exposed ([#96](https://github.com/Syynth/brink/pull/96)) ([#139](https://github.com/Syynth/brink/pull/139))
- host capability manifest — Tier 1 + closed Tier 2 (Track B MVP) ([#74](https://github.com/Syynth/brink/pull/74))

### Fixed

- *(web)* attribute compile diagnostics to their own file (closes #43) ([#49](https://github.com/Syynth/brink/pull/49))

### Other

- Studio IDE: doc comments + type hints for all declarations (Track B integration) ([#101](https://github.com/Syynth/brink/pull/101))
