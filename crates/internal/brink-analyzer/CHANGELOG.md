# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.12](https://github.com/Syynth/brink/compare/brink-analyzer-v0.0.11...brink-analyzer-v0.0.12) - 2026-08-15

### Added

- *(brink-analyzer)* E182 no-world-reads fence for @[convention] handlers ([#2179](https://github.com/Syynth/brink/pull/2179))
- *(brink-analyzer,brink-db,brink-ide,brink-lsp,brink-web)* wire the harvest index into cue-name completion ([#2134](https://github.com/Syynth/brink/pull/2134))
- *(brink-analyzer)* add screenplay to BUILTIN_ELEMENT_PRESETS now that #1720 shipped
- *(brink-analyzer)* validate `[project] elements` preset names against a closed set ([#1874](https://github.com/Syynth/brink/pull/1874))
- *(brink-ir,brink-analyzer,brink-runtime)* block capture for @[element(..., block)] ([#1839](https://github.com/Syynth/brink/pull/1839))
- *(brink-analyzer)* give `content` a resolvable Ty in the native type system
- *(lang)* type-name conformance sweep — angle brackets + Uppercase non-primitives, Option<T>/Weighted<T> annotatable ([#1552](https://github.com/Syynth/brink/pull/1552))
- *(compiler)* manifest-validated markup vocabulary (§4.2 second half)
- *(brink-analyzer)* §6.1 row variables on fn-typed params (part of #1680)
- *(brink-ir)* lower inline markup spans to ContentPart::Span ([#1716](https://github.com/Syynth/brink/pull/1716))
- *(stdlib)* the pure fn-value verb trio map/filter/fold (part of #1679)
- *(analyzer)* dual-reading `use`/`IMPORT` trailing segments ([#1592](https://github.com/Syynth/brink/pull/1592))
- *(brink-ir)* @[allow(Exxx)] source-level diagnostic suppression ([#1161](https://github.com/Syynth/brink/pull/1161))
- *(brink-ir)* per-branch source spans on CondBranch/SequenceBranch ([#404](https://github.com/Syynth/brink/pull/404))
- *(stdlib)* rename seq remove-by-index to `remove_at` ([#1484](https://github.com/Syynth/brink/pull/1484)) ([#1501](https://github.com/Syynth/brink/pull/1501))
- *(analyzer)* record `or`-coalescing types for LIR lowering ([#1492](https://github.com/Syynth/brink/pull/1492))
- *(analyzer)* B3a UFCS resolution — type-directed field-wins-else-free-fn ([#1482](https://github.com/Syynth/brink/pull/1482))
- *(brink-ir,brink-syntax-native)* B2 two-binding `for k, v` map iteration ([#1461](https://github.com/Syynth/brink/pull/1461))
- *(brink-cli,brink-analyzer,brink-environment)* CLI/API override tier for lint levels ([#1373](https://github.com/Syynth/brink/pull/1373))
- *(diagnostics)* brink.toml [lints] control plane — per-code severity + deny-warnings ([#1160](https://github.com/Syynth/brink/pull/1160))
- *(environment)* reify the compilation Environment + Project/SourceTree producer ([#1306](https://github.com/Syynth/brink/pull/1306))
- *(brink-analyzer,brink-db)* B0.9 close — native strict-only enforcement point
- *(brink-analyzer,brink-ir)* B0.9 native accept-list admission gate ([#1179](https://github.com/Syynth/brink/pull/1179))
- *(NS-A9)* dialect-keyed type-policy default via resolve_type_policy seam
- *(NS-A8)* protocol fence (E118), analyzer typing, tests, tier1 case, changeset (rebuild 2/3)
- *(NS-A5)* ranges as a real Value kind + the inhabited-range refinement ([#1111](https://github.com/Syynth/brink/pull/1111))
- *(NS-A6)* rng-as-cell — the RNG formalized, draws are writes, the rand verbs ([#1112](https://github.com/Syynth/brink/pull/1112))
- *(NS-A2)* effect-row extension — emits + tags + faults ([#1108](https://github.com/Syynth/brink/pull/1108))
- *(NS-A1)* Option[T] as the third parameterized builtin ([#1107](https://github.com/Syynth/brink/pull/1107))
- *(brink-analyzer)* T2 §8 precision rung — narrow effect rows at known indirect/value call sites ([#872](https://github.com/Syynth/brink/pull/872))
- *(analyzer,db)* await-condition purity gate (E105) built on the effects machinery
- *(ir,analyzer)* await HIR lowering, strict-ink gate (E051), LIR fence (E052)
- *(compiler)* T2-2 `#@effects(…)` assertion surface + exceedance error ([#861](https://github.com/Syynth/brink/pull/861))
- *(analyzer)* T2-1 effect-row inference substrate (advisory)
- *(t1e-2)* real MakeProjection/ProjRead/ProjWrite lowering + persistence
- *(compiler)* T1e-1 path-projection grammar + HIR + analyzer ([#831](https://github.com/Syynth/brink/pull/831))
- *(analyzer)* M-2d import-scoped resolution — relax the #784/#793 E096 stopgap ([#790](https://github.com/Syynth/brink/pull/790))
- *(compiler,format,runtime)* M-3 renames — #@was, alias table, rehydration miss-path
- *(analyzer)* M-2 import well-formedness + cross-module #@private enforcement
- *(compiler)* M-2 visibility model + HIR imports + §7 diagnostics
- *(compiler)* M-1 module name model — (module, name) DefinitionId, #@module directive ([#758](https://github.com/Syynth/brink/pull/758))
- *(t1c-3)* bind stdlib intrinsic, fn-value display form, structural equality
- *(t1c)* T1c-2 — lower/execute/persist #fn function values ([#700](https://github.com/Syynth/brink/pull/700))
- *(analyzer)* #fn typing consumes the bound prefix; strict call checking through fn values ([#699](https://github.com/Syynth/brink/pull/699))
- *(analyzer)* Ty::Fn lattice point + fn(T…): R boundary annotations (E062 retired) ([#699](https://github.com/Syynth/brink/pull/699))
- *(analyzer)* #fn creation-site diagnostics E079/E080/E081 ([#699](https://github.com/Syynth/brink/pull/699))
- *(analyzer)* dialect-gate #fn under strict-ink (E051) ([#699](https://github.com/Syynth/brink/pull/699))
- *(ir)* Expr::FnLiteral HIR + non-suppressible T1c-1 lowering fence ([#699](https://github.com/Syynth/brink/pull/699))
- *(runtime,analyzer)* int()/float()/string() conversion intrinsics ([#659](https://github.com/Syynth/brink/pull/659))
- *(syntax,ir,analyzer)* TM-4b structs grammar + HIR + analyzer, diagnostics-only ([#665](https://github.com/Syynth/brink/pull/665))
- *(analyzer,db)* FG-3 — decompose analysis_query into narrow cutoff-friendly projections ([#632](https://github.com/Syynth/brink/pull/632))
- *(analyzer,db)* TM-3 — types = strict policy, Unknown/Conflicted-escape, E063 wiring ([#619](https://github.com/Syynth/brink/pull/619))
- *(analyzer,db)* FG-2.1 — lazy per-reference globals + full dependency narrowing ([#638](https://github.com/Syynth/brink/pull/638))
- *(syntax,analyzer)* CONST declarations accept type annotations ([#641](https://github.com/Syynth/brink/pull/641))
- *(syntax)* TM-2 inline type annotation syntax — grammar/HIR/fmt/IDE, feeding signature() ([#618](https://github.com/Syynth/brink/pull/618))
- *(analyzer,db)* FG-2 — per-def/per-SCC inference decomposition ([#631](https://github.com/Syynth/brink/pull/631))
- *(analyzer)* TM-1 checker substrate — inference queries, mono-HM per SCC ([#617](https://github.com/Syynth/brink/pull/617))
- *(compiler,runtime)* T1b-3 — stdlib slice 1: len/keys/values/contains + push/insert/remove ([#571](https://github.com/Syynth/brink/pull/571))
- *(compiler,runtime)* T1b-2 — blocks, loops, collections, indexing go live ([#570](https://github.com/Syynth/brink/pull/570))
- *(compiler)* T1b-1 superset grammar + HIR + dialect gate ([#569](https://github.com/Syynth/brink/pull/569))

### Fixed

- *(brink-analyzer)* off-db conventions confinement drops files absent from modules
- *(brink-analyzer,brink-ide,brink-lsp)* off-db conventions confinement (E169)
- *(brink-web,brink-analyzer,brink-ide)* address #2327 review findings on is_all_native reachability and docs
- *(brink-analyzer,brink-ir,brink-db)* exempt conventions injection from M-2 gate (#2297 review)
- *(brink-analyzer)* correct remaining bare-access over-reads (#2296 review)
- *(brink-ir,brink-analyzer)* module-qualified divert resolution ([#2287](https://github.com/Syynth/brink/pull/2287))
- *(brink-analyzer)* repair main — is_std_shadowed_name still called the pre-#2251 name
- *(brink-syntax,brink-analyzer,brink-ir,brink-ide)* address PR #2271 review findings
- *(brink-ir,brink-analyzer)* register RefKind::Type for field/TM-2/temp annotations ([#2249](https://github.com/Syynth/brink/pull/2249))
- *(brink-ir,brink-db,brink-analyzer)* address #2266 review findings
- *(brink-analyzer)* route ShapeTable::resolve through lookup_by_name, not a bolt-on std gate
- *(brink-analyzer)* resolve referrer_module by file, not the def's own id (#2252 review)
- *(brink-analyzer)* thread a referrer-module hint into lookup_unique_by_name
- *(brink-db)* std:: mounts as a PEER ROOT of story::, not a child of it ([#2245](https://github.com/Syynth/brink/pull/2245))
- *(brink-analyzer)* correct #2216 review findings on reachability + doc inversions
- *(brink-analyzer)* unify lookup_unique_by_name with the std-invisibility gate ([#2216](https://github.com/Syynth/brink/pull/2216))
- *(brink-ir,brink-analyzer)* close std-mount fallback gap; fix stale docs and vacuous test (#2197 review)
- *(brink-project-config)* address PR review findings on #2180's rename sweep
- *(brink-ir)* arity-check return-redirect divert targets (E176 gap)
- *(brink-analyzer,brink-ir)* arity-check divert-with-args sites (E176)
- *(brink-analyzer)* apply review fixes for #1770's per-lambda escape frame
- *(brink-analyzer)* give lambda bodies a per-lambda strict-checked frame ([#1770](https://github.com/Syynth/brink/pull/1770))
- *(brink-ir)* stop double-prefixing a nested lambda's fallback id (#1727 review)
- *(brink-ir)* HIR mints a lifted lambda's identity, LIR consumes it ([#1727](https://github.com/Syynth/brink/pull/1727))
- *(brink-analyzer,brink-db)* apply PR #2107 review fixes for #1921
- *(brink-analyzer)* a UFCS call into an EXTERNAL is now argument-checked on the db-backed path ([#1921](https://github.com/Syynth/brink/pull/1921))
- *(brink-analyzer,brink-compiler)* apply review fixes for #2085/#1769
- *(brink-analyzer)* review fixes for #1840 Q4 registration slice
- *(brink-analyzer)* register writes the conventions registry cell (#1840 Q4)
- *(brink-analyzer,brink-db)* apply PR #2082 review findings for #1840 Q5
- *(brink-ir,brink-analyzer)* apply PR #2081 review findings for #1720
- *(brink-analyzer)* apply PR #2076 review findings for #1874
- *(brink-syntax-native)* value-carrying return <expr> at prose-body position ([#1973](https://github.com/Syynth/brink/pull/1973))
- *(brink-analyzer)* correct lambda param annotation widening + coverage + dedup (#1994 review)
- *(brink-analyzer)* check_direct_call_args reaches root_content facts (#2001 review)
- *(brink-analyzer)* #fn(target, args…) checks ref-bound arguments invariantly
- *(brink-analyzer)* apply review findings on #1995 ref-invariance fix
- *(brink-analyzer)* check ref parameter arguments invariantly ([#1995](https://github.com/Syynth/brink/pull/1995))
- *(brink-analyzer,brink-db)* apply review findings to #1942 handle-producer fixtures
- *(brink-analyzer)* guard native fn-value shadow check against bare-name list items ([#1901](https://github.com/Syynth/brink/pull/1901))
- *(brink-analyzer)* exclude body-rebound param names from #1941's lambda annotation seed
- *(brink-analyzer)* a lambda's value-position read of an annotated param exports its declared type ([#1941](https://github.com/Syynth/brink/pull/1941))
- *(brink-analyzer)* review fixes for #1900 dotted-field type check
- *(brink-analyzer)* merge #1928 onto main + apply review fixes for #1910
- *(brink-analyzer)* apply string-numeric display-concat carve-out to `+=` too
- *(brink-analyzer)* string + int/float display-concat no longer reports E066 ([#1911](https://github.com/Syynth/brink/pull/1911))
- *(brink-analyzer)* resolve review findings on #1877's typed-assign checks
- *(brink-analyzer)* type native bare-name fn values in decl-initializer position ([#1895](https://github.com/Syynth/brink/pull/1895))
- *(brink-analyzer)* review fixes for #1864 direct-call arg-type PR
- *(brink-analyzer)* native fn-value E080 check must walk decl initializers too
- address review findings on #1790 frame-scoped InferPass guard
- *(analyzer)* correct #1789 review findings — no-silence claim + third direction
- *(analyzer)* infer a block-bodied lambda's tail inside its own frame
- *(analyzer)* stop silently dropping markup diagnostics in choice display slots
- *(analyzer)* drop the unreachable tag descent in markup_check
- *(review)* correct #1764 reachability overclaim + add E083/E106 regression test
- *(brink-analyzer)* correct test comment + add Option/Weighted erasure coverage (#1758 review)
- *(brink-analyzer)* address review findings on #1763 doc PR
- *(brink-analyzer)* correct false soundness claim in declared_fn_type's comment
- *(effects-spec)* address review findings on the #1735 aliasing addendum
- *(brink-analyzer)* infer_lambda no longer leaks a lambda's own frame into the enclosing def (review findings on #1750)
- *(brink-analyzer)* infer_lambda absorbs a block-bodied lambda's stmts, not just its tail
- *(brink-analyzer)* make the conservative-total gate instantiation-aware for holed callees
- *(brink-analyzer)* set new Knot annotation fields in validate.rs test fixtures ([#1719](https://github.com/Syynth/brink/pull/1719))
- *(brink-ir,brink-analyzer,brink-db)* PR #1713 review findings for #1680 gap doc
- *(compiler,analyzer,runtime,docs,wasm)* PR #1708 review findings for E157/anonymous-state ([#1674](https://github.com/Syynth/brink/pull/1674))
- *(stdlib)* review findings for PR #1707 — CI clippy fix + comparator/callback role split
- *(compiler)* frame-local projection receivers are legal (issue #1531)
- *(analyzer)* pin + document the E088 known-modules widening (PR #1686 review)
- *(analyzer)* address PR #1686 review findings on dual-reading use/IMPORT
- *(review)* address #1662 findings — vacuous overlay/projection guards, doc wording
- *(analyzer)* thread native-awareness through the pure analysis path ([#1358](https://github.com/Syynth/brink/pull/1358))
- *(brink-analyzer)* PR #1652 review findings — real stitch-param coverage, §4.2 doc-stamp, qualify_label citation
- *(brink-analyzer)* E124 admission check descends into Param name ranges
- *(brink-analyzer,bevy-brink)* correct review findings on #1620
- *(brink-analyzer)* keep E065/E066 escape reading the def's own body
- *(lsp,analyzer,cli,wasm)* review fixes for #1417 lint-override tier
- *(analyzer)* address review findings on import-alias precedence ([#1596](https://github.com/Syynth/brink/pull/1596))
- *(analyzer)* honor import aliases in resolution, not just E089 ([#1590](https://github.com/Syynth/brink/pull/1590))
- *(brink-analyzer)* review fixes for #1587 — stop annotation-shadowed arg_tys from seeding sibling params in stdlib intrinsics
- *(brink-analyzer)* cover fall-through stitch returns in collect_void_defs
- *(brink-ide+brink-db+brink-analyzer)* address review findings on PR #1584 ([#530](https://github.com/Syynth/brink/pull/530))
- *(brink-db+brink-analyzer+brink-ide)* serve Param/Temp signatures via a per-file locals path ([#530](https://github.com/Syynth/brink/pull/530))
- *(analyzer)* review findings on E151 — real control-flow terminator predicate + on-by-default wording (PR #1575)
- *(lsp)* unpin M-2d native-homonym diagnostics from declared dialect; fix stale prose
- *(brink-ir,brink-analyzer)* address review findings on ResolvedRef range contract PR
- *(brink-analyzer)* restore void-annotation guard on return-escape check (PR #1556 review)
- *(brink-analyzer)* E065/E066 return-escape check extends past is_function (E150)
- *(review)* correct #1540 review-blocking issues on PR #1548
- *(brink-analyzer,brink-syntax)* review findings for #1509 stitch return-type
- *(brink-analyzer,brink-runtime)* review findings for #1542 coverage bundle
- *(brink-analyzer)* allow clippy::panic in coalesce test module
- *(review)* address PR #1536 review findings
- review findings for PR #1516 (coalesce RuntimeCheck reachability + doc gate)
- *(review)* merge main into B1b as-binding + apply reviewer findings ([#1475](https://github.com/Syynth/brink/pull/1475))
- *(analyzer)* UFCS review fixes — prelude fallback, cross-file receiver typing, Ty::display(), free-fn arity check ([#1482](https://github.com/Syynth/brink/pull/1482))
- *(analyzer)* fence the UFCS callee fallback to real call sites ([#1482](https://github.com/Syynth/brink/pull/1482))
- *(analyzer)* surface coalescing mismatches as E066, fix eager-eval and precedence gaps (#1469 review)
- *(brink-analyzer)* re-export iterate_val_ty (review finding)
- *(review)* correct stale merge-semantics docs + strengthen reapply test ([#1397](https://github.com/Syynth/brink/pull/1397))
- *(brink-analyzer)* apply_project_config replaces the [lints] policy, not merge
- *(brink-analyzer)* validate [lints] codes against the real DiagnosticCode set
- *(brink-analyzer)* exclude Param from value-call local narrowing (soundness)
- *(analyzer,ide)* semantic-type honesty for unregistered host types ([#1027](https://github.com/Syynth/brink/pull/1027))
- *(compile)* feed manifest external signatures to the compile-path strict pass (Closes #1004)
- *(brink-analyzer)* E105 await-purity recurses into struct-literal fields
- *(brink-analyzer)* record write atoms for T1b stdlib mutators + indexed-assignment targets ([#880](https://github.com/Syynth/brink/pull/880))
- *(brink-analyzer)* T2-1 effect rows write through ref params at the call site
- *(brink-analyzer)* update test helper for resolve() ImportScope param
- *(brink-analyzer)* ImportScope matches import_covers' (module,name) granularity
- *(analyzer)* E087 false positive on single-file declared-module self-reference ([#795](https://github.com/Syynth/brink/pull/795))
- *(compiler)* thread manifest handle-kind vocabulary into inference ([#774](https://github.com/Syynth/brink/pull/774))
- *(analyzer)* derive per-file module for symbol-less files; narrow E088 doc
- *(analyzer)* call()/bind() intrinsics statically check under strict mode ([#733](https://github.com/Syynth/brink/pull/733))
- *(analyzer)* declaration-derived signatures carry Ty::Fn for global VARs ([#712](https://github.com/Syynth/brink/pull/712))
- *(analyzer,db)* E063 error-severity under strict + void-assignment error E067 ([#619](https://github.com/Syynth/brink/pull/619))
- *(analyzer)* apply TM-2 review rulings — E063 opt-in ratified, strict-ink suppresses E061/E062 ([#618](https://github.com/Syynth/brink/pull/618))

### Other

- *(brink-analyzer)* share STORY_ROOT constant in duplicated native_module_path
- *(brink-analyzer)* hoist per-check maps in no_world_reads BFS; document two gaps
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge pull request #2254 from Syynth/auto/issue-2217
- *(brink-ir,brink-analyzer,brink-db)* generalize STD_ROOT/is_std_module to a set of reserved mount roots ([#2251](https://github.com/Syynth/brink/pull/2251))
- Merge remote-tracking branch 'origin/main' into auto/issue-2108
- checkpoint before merging origin/main (issue #2108)
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge remote-tracking branch 'origin/main' into train-fix
- rename [project] elements to [project] conventions with deprecated alias ([#2180](https://github.com/Syynth/brink/pull/2180))
- *(brink-analyzer)* update stale #2080 not-yet-mounted notes
- Merge remote-tracking branch 'origin/main' into train-fix
- *(brink-analyzer)* pin the Stitch arm of check_divert_arity's kind guard
- Merge remote-tracking branch 'origin/main' into train-fix
- *(brink-analyzer)* fix stale hand-walk prose after #2098 visitor migration
- *(brink-analyzer)* migrate 6 decl-initializer walks onto HirVisitor
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge origin/main into train-fix for PR #2110
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge pull request #2095 from Syynth/auto/issue-1840-registration
- *(brink-analyzer)* correct coalesce.rs prose #1774 falsifies (review finding)
- Merge origin/main into train-fix
- Merge origin/main into auto/issue-1994
- Merge remote-tracking branch 'origin/main' into train-fix
- *(brink-analyzer)* document #fn as a second DirectCallArgMismatch producer (#2001 review)
- *(brink-analyzer)* fix infer_fn_literal comment contradicting its own code (#2001 review)
- *(analyzer,brink-db)* replace opaque_handle scaffolding with registered handle producers ([#1942](https://github.com/Syynth/brink/pull/1942))
- *(brink-analyzer)* fix stale #fn-only prose after native bare-name E119 support
- Merge remote-tracking branch 'origin/main' into train-fix
- *(brink-analyzer)* close #1901 empirically — cross-file shadow can never legitimately compile
- Merge pull request #1937 from Syynth/auto/issue-1903
- Merge pull request #1946 from Syynth/auto/issue-1919
- Merge remote-tracking branch 'origin/main' into train-fix
- *(brink-analyzer)* check plain struct-field assignment (~ p.x = expr) against declared field type ([#1900](https://github.com/Syynth/brink/pull/1900))
- *(brink-analyzer)* document the FieldCall call-edge over-approximation
- Merge origin/main into train-fix
- *(brink-analyzer)* check UFCS-desugared call argument types ([#1881](https://github.com/Syynth/brink/pull/1881))
- Merge remote-tracking branch 'origin/main' into train-fix
- *(t1c-spec)* qualify the fn-value typing/lowering agreement claim
- Merge pull request #1886 from Syynth/auto/issue-1876
- *(brink-ir/brink-analyzer)* project-level injection point for an evaluated conventions registry ([#1863](https://github.com/Syynth/brink/pull/1863)) ([#1888](https://github.com/Syynth/brink/pull/1888))
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge remote-tracking branch 'origin/main' into train-fix
- cargo fmt conventions_confinement.rs and analysis.rs ([#1844](https://github.com/Syynth/brink/pull/1844))
- *(brink-ir/brink-analyzer/brink-db)* confine pattern-claiming handlers to the brink.toml-named conventions module ([#1844](https://github.com/Syynth/brink/pull/1844))
- *(brink-ir)* natural-notation @[element(claims)] dispatch, E166, docs, changeset ([#1838](https://github.com/Syynth/brink/pull/1838))
- *(brink-ir)* natural-notation @[element(claims)] handlers dispatch prose lines ([#1838](https://github.com/Syynth/brink/pull/1838))
- Merge remote-tracking branch 'origin/main' into train-pr
- give SpanPart per-span provenance so E164/E165 don't collapse ([#1782](https://github.com/Syynth/brink/pull/1782))
- *(#1808)* repoint dangling #1680 tracker refs, fix same-file contradiction
- Merge remote-tracking branch 'origin/main' into train-fix
- regression guard for frame-scoped InferPass fields (issue #1790)
- cargo fmt
- Merge origin/main into auto/issue-1779
- Merge remote-tracking branch 'origin/main' into train-fix
- merge origin/main into train-fix for review fixes
- Merge origin/main into train-fix for PR #1767
- *(brink-analyzer)* document why collect_temps skips Expr::Lambda ([#1763](https://github.com/Syynth/brink/pull/1763))
- name the Ty::Unknown poisoning exception on FnRow's doc + changeset
- *(brink-analyzer)* fix wrong diagnostic code in assignable's doc comment
- origin/main into train-fix for PR #1754
- Merge remote-tracking branch 'origin/main' into train-fix
- *(brink-analyzer)* drop the false "classifies to a creation site" claim on call_fn_args
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge origin/main into train-fix
- *(brink-db)* pin the lambda effect-row gap blocking #1680
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge remote-tracking branch 'origin/main' into auto/issue-1685
- Merge remote-tracking branch 'origin/main' into auto/issue-1436
- merge origin/main + address review findings on #1615
- Merge remote-tracking branch 'origin/main' into auto/issue-1591
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge origin/main into train-fix for PR #1585
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge origin/main into train-fix for PR #1579
- *(analyzer)* native asymmetric choice-branch dead-end (E151, issue #1219)
- *(brink-ir,brink-analyzer)* pin the call-path ResolvedRef range contract ([#1561](https://github.com/Syynth/brink/pull/1561))
- cargo fmt
- Merge pull request #1547 from Syynth/auto/issue-1526
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge origin/main into train-fix
- Merge remote-tracking branch 'origin/main' into train-fix
- *(brink-analyzer,brink-runtime)* prop_oneof! exhaustiveness sweep for RefKind/MapKey
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge origin/main into train-fix
- update stale UFCS wiring prose now that brink-ide reads the table
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge remote-tracking branch 'origin/main' into train-fix
- merge main + address review: wire E084/E106 to native surface, name E138's key
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge remote-tracking branch 'origin/main' into train-pr
- invert the brink-project-config → brink-analyzer edge ([#1234](https://github.com/Syynth/brink/pull/1234))
- add `tail` to the shared HIR Block (expand phase) ([#1216](https://github.com/Syynth/brink/pull/1216))
- B0.4 step 1: additive doc/visibility/was HIR fields (declaration nodes)
- corpus-wide admission-clean gate + NF-6 perf budget; fix O(n^2)
- one malformed-fixture test per admission E-code (#672-A posture)
- the HIR admission validator, wired at the lowered_query seam
- explicit Return.kind — ReturnKind replaces ptr-presence semantics (Q7a, D5/F-I#6)
- Merge pull request #1156 from Syynth/auto/ns-a7
- analyzer wiring — Ty::Weighted, typing arms, effect rows, F29 discharge
- F29(a) refined-faults carve-out + end-to-end ordering tests
- sort family in LIR/codegen/analyzer + E119 comparator-contract gate
- Merge origin/main (post-#1133) — E116+E117 coexist; A5 rows absorbed into the shared intrinsics table
- Merge origin/main (NS-A3 registry) into ns-a6
- Merge remote-tracking branch 'origin/main' into train-pr
- E078 classifies variable/call/index-valued int()/float() arguments ([#983](https://github.com/Syynth/brink/pull/983))
- Merge pull request #1015 from Syynth/auto/issue-994
- *(analyzer)* cover E071 stitch-scope dispatch + order-independence ([#670](https://github.com/Syynth/brink/pull/670))
- Merge origin/main into train-fix for PR #975
- E071 classifies variable/call/index-valued struct field initializers ([#670](https://github.com/Syynth/brink/pull/670))
- expose structs::declared_shapes/ShapeInfo as a public API ([#858](https://github.com/Syynth/brink/pull/858))
- Merge pull request #951 from Syynth/auto/issue-598
- merge origin/main into train-fix
- *(brink-analyzer)* unify order-independence law, incl. Conflicted absorption ([#746](https://github.com/Syynth/brink/pull/746))
- fmt, E052 fence doc, changeset, and exhaustive-match completion for await
- Merge remote-tracking branch 'origin/main' into train-pr
- Merge origin/main into train-fix
- Merge branch 'main' into auto/issue-801
- Merge remote-tracking branch 'origin/main' into train-pr
- Merge branch 'main' into auto/issue-784
- Merge pull request #800 from Syynth/fix/795-e087-self-reference
- Merge remote-tracking branch 'origin/main' into train-pr
- origin/main into feat/750-fg3-analysis-query-split — re-apply FG-3 completion onto the queries/ split
- origin/main into train-fix for PR #770
- M-2 clippy/fmt polish + @brink-lang/web changeset
- Merge pull request #749 from Syynth/auto/issue-733
- complete the E075->E078 renumber ([#659](https://github.com/Syynth/brink/pull/659))
- Merge remote-tracking branch 'origin/main' into auto/issue-641
- Merge remote-tracking branch 'origin/main' into auto/issue-641
- sweep converter mentions after pipeline retirement ([#544](https://github.com/Syynth/brink/pull/544))
- setSemanticTypeDiagnosticSeverity lever ([#532](https://github.com/Syynth/brink/pull/532))
- Merge remote-tracking branch 'origin/main' into train-pr
- split locals out of symbol_index (post-slice-B cutoff tightening)
- salsa into brink-db — query-memoized pipeline stages (phase 0 slice B) ([#515](https://github.com/Syynth/brink/pull/515))
- extract the symbol service from brink-analyzer (phase 0 slice A) ([#509](https://github.com/Syynth/brink/pull/509))

## [0.0.11](https://github.com/Syynth/brink/compare/brink-analyzer-v0.0.10...brink-analyzer-v0.0.11) - 2026-07-11

### Other

- Merge pull request #495 from Syynth/bronch/compiler-local-var-keyword-0fdbbc
- is_local on test Knot fixtures ([#473](https://github.com/Syynth/brink/pull/473))

## [0.0.10](https://github.com/Syynth/brink/compare/brink-analyzer-v0.0.9...brink-analyzer-v0.0.10) - 2026-07-10

### Other

- Story::new takes Arc<Program>, not &Program
- shared read-only HIR visitor + migrate 4 walkers ([#457](https://github.com/Syynth/brink/pull/457)) ([#464](https://github.com/Syynth/brink/pull/464))

## [0.0.7](https://github.com/Syynth/brink/compare/brink-analyzer-v0.0.6...brink-analyzer-v0.0.7) - 2026-06-20

### Other

- welcoming README (playground/book links, overview, LLM-experiment disclaimer) ([#285](https://github.com/Syynth/brink/pull/285))

## [0.0.6](https://github.com/Syynth/brink/compare/brink-analyzer-v0.0.5...brink-analyzer-v0.0.6) - 2026-06-19

### Added

- *(studio)* host functions panel categories + search ([#210](https://github.com/Syynth/brink/pull/210)) ([#270](https://github.com/Syynth/brink/pull/270))

## [0.0.5](https://github.com/Syynth/brink/compare/brink-analyzer-v0.0.4...brink-analyzer-v0.0.5) - 2026-06-17

### Added

- *(studio)* argument widgets stage 5 — arg-groups + inter-arg context + modal ([#222](https://github.com/Syynth/brink/pull/222))
- *(studio)* argument widgets stage 2 — argument_widgets query + Fill ([#219](https://github.com/Syynth/brink/pull/219))

## [0.0.4](https://github.com/Syynth/brink/compare/brink-analyzer-v0.0.3...brink-analyzer-v0.0.4) - 2026-06-15

### Added

- *(manifest,ide)* static value-source + value-label inlay hints ([#174](https://github.com/Syynth/brink/pull/174)) ([#203](https://github.com/Syynth/brink/pull/203))

### Fixed

- *(#187)* tunnel calls aren't terminal (E033) + resolve diagnostics to paths (#190)

## [0.0.3](https://github.com/Syynth/brink/compare/brink-analyzer-v0.0.2...brink-analyzer-v0.0.3) - 2026-06-13

### Added

- host capability manifest — Tier 1 + closed Tier 2 (Track B MVP) ([#74](https://github.com/Syynth/brink/pull/74))

### Other

- Studio IDE: doc comments + type hints for all declarations (Track B integration) ([#101](https://github.com/Syynth/brink/pull/101))
