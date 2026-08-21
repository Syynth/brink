# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.12](https://github.com/Syynth/brink/compare/brink-db-v0.0.11...brink-db-v0.0.12) - 2026-08-21

### Added

- *(brink-analyzer)* E188 warns when a STRUCT name collides with a reserved builtin/tower type
- *(brink-analyzer,brink-db,brink-ide,brink-lsp,brink-web)* wire the harvest index into cue-name completion ([#2134](https://github.com/Syynth/brink/pull/2134))
- *(brink-ir)* split @[element(claims=…)] into @[convention(claims=…, order=N)]
- *(brink-analyzer,brink-ir,brink-db)* register is a comptime-only intrinsic (#1840 Q5)
- *(brink-analyzer)* validate `[project] elements` preset names against a closed set ([#1874](https://github.com/Syynth/brink/pull/1874))
- *(brink-analyzer)* give `content` a resolvable Ty in the native type system
- *(lang)* type-name conformance sweep — angle brackets + Uppercase non-primitives, Option<T>/Weighted<T> annotatable ([#1552](https://github.com/Syynth/brink/pull/1552))
- *(compiler)* manifest-validated markup vocabulary (§4.2 second half)
- *(brink-analyzer)* §6.1 row variables on fn-typed params (part of #1680)
- *(brink-ir)* lower inline markup spans to ContentPart::Span ([#1716](https://github.com/Syynth/brink/pull/1716))
- *(brink-ir)* @[allow(Exxx)] source-level diagnostic suppression ([#1161](https://github.com/Syynth/brink/pull/1161))
- *(brink-ir)* per-branch source spans on CondBranch/SequenceBranch ([#404](https://github.com/Syynth/brink/pull/404))
- *(brink-ir)* lower native per-declaration `@[effects(…)]` annotations ([#1563](https://github.com/Syynth/brink/pull/1563))
- *(brink-analyzer)* E149 compile error for array-typed remove() calls
- *(compiler)* B1b the `as` binding — one construct, both condition positions ([#1475](https://github.com/Syynth/brink/pull/1475))
- *(brink-analyzer,brink-db)* B0.9 close — native strict-only enforcement point
- *(brink-analyzer,brink-ir)* B0.9 native accept-list admission gate ([#1179](https://github.com/Syynth/brink/pull/1179))
- *(brink-ir,brink-db)* native @[was] module-rename migration ([#1286](https://github.com/Syynth/brink/pull/1286))
- *(brink-db)* native codegen closure = the discovered module set ([#1296](https://github.com/Syynth/brink/pull/1296))
- *(brink-driver,brink-compiler,brink-cli)* discover_native over SourceTree — RealFs walk + GitRev git-baseline
- *(brink-db)* native module identity — path-derived, DefinitionId-qualifying
- *(brink-db,brink-driver)* add SourceTree seam (RealFs/GitRev/InMemory), unwired
- *(brink-db)* native .brink compile seam (B0.10a)
- *(NS-A9)* dialect-keyed type-policy default via resolve_type_policy seam
- *(NS-A8)* protocol fence (E118), analyzer typing, tests, tier1 case, changeset (rebuild 2/3)
- *(NS-A5)* ranges as a real Value kind + the inhabited-range refinement ([#1111](https://github.com/Syynth/brink/pull/1111))
- *(NS-A6)* rng-as-cell — the RNG formalized, draws are writes, the rand verbs ([#1112](https://github.com/Syynth/brink/pull/1112))
- *(NS-A2)* effect-row extension — emits + tags + faults ([#1108](https://github.com/Syynth/brink/pull/1108))
- *(NS-A1)* Option[T] as the third parameterized builtin ([#1107](https://github.com/Syynth/brink/pull/1107))
- *(analyzer,db)* await-condition purity gate (E105) built on the effects machinery
- *(ir,analyzer)* await HIR lowering, strict-ink gate (E051), LIR fence (E052)
- *(format)* T2-3 EffectRows emission — factored rows + DefinitionId→row table ([#862](https://github.com/Syynth/brink/pull/862))
- *(brink-db)* effects(def) salsa query beside signature(def) (advisory)
- *(analyzer)* M-2d import-scoped resolution — relax the #784/#793 E096 stopgap ([#790](https://github.com/Syynth/brink/pull/790))
- *(compiler,format,runtime)* M-3 renames — #@was, alias table, rehydration miss-path
- *(analyzer)* M-2 import well-formedness + cross-module #@private enforcement
- *(compiler)* M-1 module name model — (module, name) DefinitionId, #@module directive ([#758](https://github.com/Syynth/brink/pull/758))
- *(analyzer)* #fn typing consumes the bound prefix; strict call checking through fn values ([#699](https://github.com/Syynth/brink/pull/699))
- *(ir,codegen,runtime)* TM-4c structs LIR + codegen ([#666](https://github.com/Syynth/brink/pull/666))
- *(analyzer,db)* FG-3 — decompose analysis_query into narrow cutoff-friendly projections ([#632](https://github.com/Syynth/brink/pull/632))
- *(analyzer,db)* TM-3 — types = strict policy, Unknown/Conflicted-escape, E063 wiring ([#619](https://github.com/Syynth/brink/pull/619))
- *(analyzer,db)* FG-2.1 — lazy per-reference globals + full dependency narrowing ([#638](https://github.com/Syynth/brink/pull/638))
- *(analyzer,db)* FG-2 — per-def/per-SCC inference decomposition ([#631](https://github.com/Syynth/brink/pull/631))
- *(analyzer)* TM-1 checker substrate — inference queries, mono-HM per SCC ([#617](https://github.com/Syynth/brink/pull/617))
- *(compiler,runtime)* T1b-3 — stdlib slice 1: len/keys/values/contains + push/insert/remove ([#571](https://github.com/Syynth/brink/pull/571))

### Fixed

- *(analyzer)* resolve a fn-valued CONST global's call site ([#2083](https://github.com/Syynth/brink/pull/2083))
- *(brink-lsp)* route .brink documents through the native CST for semantic tokens, inlay hints, and code actions
- *(brink-analyzer)* resolve an unannotated ~ temp's shape from its initializer for E063/E185 ([#2906](https://github.com/Syynth/brink/pull/2906))
- *(brink-db)* address #2357 review findings on non-source-document gating
- *(brink-db)* is_source_file must not exclude extension-less pseudo-paths
- *(brink-db)* non-source documents never join parsing/symbol-index/diagnostics
- *(brink-ir)* address #2344 review findings on heading slug/tag routing
- *(brink-db)* brink.toml sharing a session no longer disqualifies is_all_native ([#2318](https://github.com/Syynth/brink/pull/2318))
- *(brink-analyzer,brink-ir,brink-db)* exempt conventions injection from M-2 gate (#2297 review)
- *(brink-ir,brink-db,brink-analyzer)* conventions claiming reaches the whole project ([#2289](https://github.com/Syynth/brink/pull/2289))
- *(brink-ir,brink-analyzer)* register RefKind::Type for field/TM-2/temp annotations ([#2249](https://github.com/Syynth/brink/pull/2249))
- *(brink-ir,brink-db,brink-analyzer)* address #2266 review findings
- *(brink-db,brink-ir)* correct stale story::std prose after peer-root ruling
- *(brink-db)* std:: mounts as a PEER ROOT of story::, not a child of it ([#2245](https://github.com/Syynth/brink/pull/2245))
- *(brink-db,brink-format,brink-ir)* close #2212 review findings
- *(brink-ir,brink-db,brink-format)* close #2111's three review findings
- *(brink-lsp)* one native ProjectDb per governing brink.toml ([#1580](https://github.com/Syynth/brink/pull/1580))
- *(brink-analyzer)* give lambda bodies a per-lambda strict-checked frame ([#1770](https://github.com/Syynth/brink/pull/1770))
- *(brink-ir)* HIR mints a lifted lambda's identity, LIR consumes it ([#1727](https://github.com/Syynth/brink/pull/1727))
- *(brink-analyzer,brink-db)* apply PR #2107 review fixes for #1921
- *(brink-analyzer)* a UFCS call into an EXTERNAL is now argument-checked on the db-backed path ([#1921](https://github.com/Syynth/brink/pull/1921))
- *(brink-analyzer)* register writes the conventions registry cell (#1840 Q4)
- *(brink-ir)* thread real UFCS/coalesce tables through decl-default lambda lowering (#1774 review)
- *(brink-analyzer,brink-db)* apply PR #2082 review findings for #1840 Q5
- *(brink-analyzer)* a lambda's written annotation now governs its type, with an eager E173 on disagreement ([#1994](https://github.com/Syynth/brink/pull/1994))
- *(brink-analyzer,brink-db)* apply review findings to #1942 handle-producer fixtures
- *(brink-analyzer)* E119's pure-callback gate recognizes native bare-name fn values
- *(brink-analyzer)* direct-call arguments checked against declared param types ([#1864](https://github.com/Syynth/brink/pull/1864))
- *(analyzer)* record the #fn-creation-site ref-param write ([#1755](https://github.com/Syynth/brink/pull/1755))
- *(brink-db)* satisfy clippy::explicit_iter_loop in new parity test
- *(brink-db)* address #1752 review findings on call-graph parity gate
- *(brink-analyzer)* infer_lambda no longer leaks a lambda's own frame into the enclosing def (review findings on #1750)
- *(brink-analyzer)* infer_lambda absorbs a block-bodied lambda's stmts, not just its tail
- *(brink-ir,brink-analyzer,brink-db)* PR #1713 review findings for #1680 gap doc
- *(brink-db,brink-compiler)* absolutize root_relative_key + doc/changeset review fixes ([#1706](https://github.com/Syynth/brink/pull/1706))
- *(stdlib)* review findings for PR #1707 — CI clippy fix + comparator/callback role split
- *(brink-ir)* qualify anonymous root-content scope paths by owning file ([#1504](https://github.com/Syynth/brink/pull/1504))
- *(analyzer)* thread native-awareness through the pure analysis path ([#1358](https://github.com/Syynth/brink/pull/1358))
- *(brink-db)* address review findings on PR #1653 (issue #460)
- *(brink-db)* bill HirFile::allow_scopes in heap_size estimator
- *(brink-ide+brink-db+brink-analyzer)* address review findings on PR #1584 ([#530](https://github.com/Syynth/brink/pull/530))
- *(brink-db+brink-analyzer+brink-ide)* serve Param/Temp signatures via a per-file locals path ([#530](https://github.com/Syynth/brink/pull/530))
- *(analyzer)* review findings on E151 — real control-flow terminator predicate + on-by-default wording (PR #1575)
- *(lsp)* unpin M-2d native-homonym diagnostics from declared dialect; fix stale prose
- *(ide,lsp,web)* propagate analysis options + module diagnostics to the IDE path ([#1553](https://github.com/Syynth/brink/pull/1553))
- *(brink-db,brink-ide)* address PR #1547 review findings
- *(review)* correct #1540 review-blocking issues on PR #1548
- *(brink-ide)* route def --at, find_references, and rename through the UFCS verdict table
- *(compiler,runtime)* `or`-coalescing short-circuits, off analyzer types ([#1471](https://github.com/Syynth/brink/pull/1471))
- *(compiler)* wire LIR lowering to consume the ufcs_resolution verdict table
- *(review)* merge main into B1b as-binding + apply reviewer findings ([#1475](https://github.com/Syynth/brink/pull/1475))
- *(brink-db)* heap_size accounts for ForStmt.val_name (review finding)
- *(brink-ir)* lower the unquoted @[was(story::old::path)] arg into hir.module.was ([#1355](https://github.com/Syynth/brink/pull/1355))
- *(brink-analyzer)* decouple T1b dialect gate from native .brink files
- *(brink-db,brink-driver)* cite the full decision-log heading for the SourceTree seam
- *(brink-syntax-native)* warn on `<-` outside a choice point ([#1263](https://github.com/Syynth/brink/pull/1263))
- *(brink-db)* scope compileProject's error gate to entry's INCLUDE closure
- *(brink-analyzer)* infer void return for functions with no value-returning path ([#1028](https://github.com/Syynth/brink/pull/1028))
- *(brink-analyzer)* preserve nominal LIST name through InferredType ([#628](https://github.com/Syynth/brink/pull/628))
- *(format,db)* wire #@private freeze semantics into T2-3 EffectRows emission ([#882](https://github.com/Syynth/brink/pull/882))
- *(brink-analyzer)* T2-1 effect rows write through ref params at the call site
- *(brink-db)* update topological_order call site for #815 signature change
- *(brink-db)* narrow topological_order to entry-reachable files ([#815](https://github.com/Syynth/brink/pull/815))
- *(brink-analyzer)* ImportScope matches import_covers' (module,name) granularity
- *(analyzer)* E087 false positive on single-file declared-module self-reference ([#795](https://github.com/Syynth/brink/pull/795))
- *(compiler)* thread manifest handle-kind vocabulary into inference ([#774](https://github.com/Syynth/brink/pull/774))
- *(analyzer)* derive per-file module for symbol-less files; narrow E088 doc
- *(analyzer)* declaration-derived signatures carry Ty::Fn for global VARs ([#712](https://github.com/Syynth/brink/pull/712))
- *(analyzer,db)* E063 error-severity under strict + void-assignment error E067 ([#619](https://github.com/Syynth/brink/pull/619))
- *(db)* narrow signature_query's per-file dependency + re-source inference inputs off resolution_index ([#630](https://github.com/Syynth/brink/pull/630)) ([#634](https://github.com/Syynth/brink/pull/634))
- *(compiler,codegen)* real error paths for two debug_assert-guarded backstops (#585, #586)
- *(compiler)* non-suppressible ICE backstop for residual T1b HIR nodes ([#572](https://github.com/Syynth/brink/pull/572))
- *(brink-db)* durable path->FileId identity stops permanent memo leak on remove/re-add ([#542](https://github.com/Syynth/brink/pull/542))

### Other

- fix review findings on #2352 dispatch projection rows
- ConventionsProjection carries !name dispatch handler rows ([#2352](https://github.com/Syynth/brink/pull/2352))
- Check argument types/arity for UFCS field calls (issue #1918)
- Merge remote-tracking branch 'origin/main' into auto/issue-2083
- *(brink-db)* fix stale incrementality-test comment (#2083 review)
- cargo fmt for #2083's analyzer fix and its new tests
- Rework #2320 per adversarial review: fix at the pointer's read site, not root_relative_key
- Fix #2320: resolve relative [project] conventions pointer against native_root, not cwd
- Add E185: unknown struct field on a plain assignment target ([#1944](https://github.com/Syynth/brink/pull/1944))
- give SpanAttr per-attribute provenance so E165 doesn't collapse
- *(brink-db,docs)* correct stale prose about conventions_confinement_diagnostics_query being the only seam
- *(brink-ir,brink-db)* name the story root literal STORY_ROOT
- *(brink-db)* extract for_each_source_file — the aggregator crossed clippy's line limit
- Merge branch 'main' into auto/issue-2329
- *(brink-db)* correct the harvest-completion projection's re-merge claim
- *(brink-ir,brink-analyzer,brink-db)* generalize STD_ROOT/is_std_module to a set of reserved mount roots ([#2251](https://github.com/Syynth/brink/pull/2251))
- Merge remote-tracking branch 'origin/main' into auto/issue-2108
- checkpoint before merging origin/main (issue #2108)
- merge origin/main into #2111 — reconcile with #2180's elements->conventions rename
- rename [project] elements to [project] conventions with deprecated alias ([#2180](https://github.com/Syynth/brink/pull/2180))
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge remote-tracking branch 'origin/main' into train-fix
- *(analyzer,brink-db)* replace opaque_handle scaffolding with registered handle producers ([#1942](https://github.com/Syynth/brink/pull/1942))
- *(brink-analyzer)* fix stale #fn-only prose after native bare-name E119 support
- Merge remote-tracking branch 'origin/main' into train-fix
- *(brink-analyzer)* check plain struct-field assignment (~ p.x = expr) against declared field type ([#1900](https://github.com/Syynth/brink/pull/1900))
- *(brink-analyzer)* check UFCS-desugared call argument types ([#1881](https://github.com/Syynth/brink/pull/1881))
- *(brink-analyzer)* check VAR/CONST/temp initializers and assignments against declared types ([#1877](https://github.com/Syynth/brink/pull/1877))
- Merge pull request #1886 from Syynth/auto/issue-1876
- *(brink-ir/brink-analyzer)* project-level injection point for an evaluated conventions registry ([#1863](https://github.com/Syynth/brink/pull/1863)) ([#1888](https://github.com/Syynth/brink/pull/1888))
- Merge remote-tracking branch 'origin/main' into train-fix
- *(brink-ir)* natural-notation @[element(claims)] handlers dispatch prose lines ([#1838](https://github.com/Syynth/brink/pull/1838))
- *(brink-db)* cover fn_row_heap's non-trivial branch
- origin/main into train-fix for PR #1754
- Merge remote-tracking branch 'origin/main' into train-fix
- *(effects-spec)* record §6.1b row variables; update the two floor tests
- Merge remote-tracking branch 'origin/main' into train-fix
- *(brink-db)* pin the lambda effect-row gap blocking #1680
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge remote-tracking branch 'origin/main' into train-fix
- satisfy the implicit-hasher and disallowed-types lints on the #1504 seam
- *(brink-db)* share one chunk-lowering context across knot memos ([#460](https://github.com/Syynth/brink/pull/460))
- merge origin/main + address review findings on #1615
- update stale suppression-channel prose for @[allow(...)]
- merge origin/main into train-fix
- *(analyzer)* native asymmetric choice-branch dead-end (E151, issue #1219)
- Merge remote-tracking branch 'origin/main' into train-fix
- *(brink-db)* prove the Stitch effects-assertion channel reaches the checker
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge origin/main into train-fix
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge remote-tracking branch 'origin/main' into train-fix
- *(brink-ir)* bundle UfcsLookup/CoalesceLookup into AnalyzerTables<'a>
- *(brink-db)* move UfcsResolution memoization doc onto the tracked query fn
- *(B1b)* correct the Option chapter's stale `as` spelling; changeset ([#1475](https://github.com/Syynth/brink/pull/1475))
- Merge remote-tracking branch 'origin/main' into auto/issue-1355
- *(brink-db)* extract SourceTree seam into leaf crate brink-source-tree
- add `tail` to the shared HIR Block (expand phase) ([#1216](https://github.com/Syynth/brink/pull/1216))
- B0.4 step 4: delete the hand-built manifest path — wire project_manifest in
- the HIR admission validator, wired at the lowered_query seam
- Merge pull request #1156 from Syynth/auto/ns-a7
- rename brink-db effects-assertion fixture off the new roll intrinsic name
- analyzer wiring — Ty::Weighted, typing arms, effect rows, F29 discharge
- register comparator_contract_diagnostics_query salsa ingredient
- sort family in LIR/codegen/analyzer + E119 comparator-contract gate
- *(NS-A9)* t2_2 + tm3_strict triage under the dialect-keyed strict default
- Merge origin/main (post-#1133) — E116+E117 coexist; A5 rows absorbed into the shared intrinsics table
- Merge remote-tracking branch 'origin/main' into train-pr
- Merge remote-tracking branch 'origin/main' into train-pr
- Merge remote-tracking branch 'origin/main' into train-pr
- *(brink-ir,brink-db)* FG-6 cleanup tail — audit lower_to_program/composed-equals-monolithic retirement ([#841](https://github.com/Syynth/brink/pull/841))
- *(brink-ir,brink-db)* own memo for the LIR prelude's decl collection
- Merge remote-tracking branch 'origin/main' into train-pr
- Merge remote-tracking branch 'origin/main' into train-pr
- Merge remote-tracking branch 'origin/main' into train-pr
- Merge branch 'main' into auto/issue-801
- Merge remote-tracking branch 'origin/main' into train-pr
- Merge branch 'main' into auto/issue-784
- Merge remote-tracking branch 'origin/main' into train-pr
- Merge pull request #800 from Syynth/fix/795-e087-self-reference
- Merge remote-tracking branch 'origin/main' into train-pr
- origin/main into feat/750-fg3-analysis-query-split — re-apply FG-3 completion onto the queries/ split
- origin/main into train-fix for PR #770
- M-2 clippy/fmt polish + @brink-lang/web changeset
- *(brink-db)* extract analysis_query family into queries/analysis submodule
- Merge pull request #706 from Syynth/test/672-lane-g-regression-sweep
- sweep converter mentions after pipeline retirement ([#544](https://github.com/Syynth/brink/pull/544))
- editor-session memory profiling harness ([#529](https://github.com/Syynth/brink/pull/529))
- split locals out of symbol_index (post-slice-B cutoff tightening)
- salsa into brink-db — query-memoized pipeline stages (phase 0 slice B) ([#515](https://github.com/Syynth/brink/pull/515))

## [0.0.10](https://github.com/Syynth/brink/compare/brink-db-v0.0.9...brink-db-v0.0.10) - 2026-07-10

### Other

- Story::new takes Arc<Program>, not &Program

## [0.0.8](https://github.com/Syynth/brink/compare/brink-db-v0.0.7...brink-db-v0.0.8) - 2026-07-01

### Other

- #312 + #313 (Track N core): shared INCLUDE-block detector + fold/auto-import cores ([#331](https://github.com/Syynth/brink/pull/331))
- *(brink-ide,brink-db)* regression coverage for shallower file-move outbound INCLUDE rewrite ([#325](https://github.com/Syynth/brink/pull/325))

## [0.0.7](https://github.com/Syynth/brink/compare/brink-db-v0.0.6...brink-db-v0.0.7) - 2026-06-20

### Other

- welcoming README (playground/book links, overview, LLM-experiment disclaimer) ([#285](https://github.com/Syynth/brink/pull/285))

## [0.0.5](https://github.com/Syynth/brink/compare/brink-db-v0.0.4...brink-db-v0.0.5) - 2026-06-17

### Added

- *(ide)* file rename/move core (#164 Stage 3, PR A) ([#252](https://github.com/Syynth/brink/pull/252))

## [0.0.3](https://github.com/Syynth/brink/compare/brink-db-v0.0.2...brink-db-v0.0.3) - 2026-06-13

### Fixed

- *(compiler)* surface syntax errors + reject malformed inline conditionals (closes #44) ([#48](https://github.com/Syynth/brink/pull/48))

### Other

- Studio IDE: doc comments + type hints for all declarations (Track B integration) ([#101](https://github.com/Syynth/brink/pull/101))
