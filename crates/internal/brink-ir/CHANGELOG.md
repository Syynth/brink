# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.12](https://github.com/Syynth/brink/compare/brink-ir-v0.0.11...brink-ir-v0.0.12) - 2026-08-22

### Added

- *(brink-analyzer)* E188 warns when a STRUCT name collides with a reserved builtin/tower type
- *(brink-ir,brink-web)* the explain-match query (#2113, NS-T seam 3/6)
- *(brink-ir,brink-format,brink-web)* transport succession rows through the conventions projection ([#2115](https://github.com/Syynth/brink/pull/2115))
- *(brink-syntax-native,brink-ir)* pub visibility keyword ([#1582](https://github.com/Syynth/brink/pull/1582))
- *(brink-ir)* split @[element(claims=…)] into @[convention(claims=…, order=N)]
- *(brink-ir)* add enter_var_decl/enter_const_decl hooks to HirVisitor
- *(brink-ir)* choice-guard `as` binding lowers for real ([#1508](https://github.com/Syynth/brink/pull/1508))
- *(brink-ir)* built-in screenplay preset — cue/parenthetical claim dispatch ([#1720](https://github.com/Syynth/brink/pull/1720))
- *(brink-ir,brink-analyzer,brink-runtime)* block capture for @[element(..., block)] ([#1839](https://github.com/Syynth/brink/pull/1839))
- *(brink-syntax-native,brink-ir)* implement `!name` sigil dispatch ([#2004](https://github.com/Syynth/brink/pull/2004))
- *(compiler)* native bare-name fn values ([#1862](https://github.com/Syynth/brink/pull/1862))
- *(brink-ir)* @[element(…, block)] declaration surface (issue #1839)
- *(lang)* type-name conformance sweep — angle brackets + Uppercase non-primitives, Option<T>/Weighted<T> annotatable ([#1552](https://github.com/Syynth/brink/pull/1552))
- *(compiler)* manifest-validated markup vocabulary (§4.2 second half)
- *(brink-format,brink-ir)* LinePart::Span wire encoding, hash-transparent recognition ([#1716](https://github.com/Syynth/brink/pull/1716))
- *(brink-ir)* lower inline markup spans to ContentPart::Span ([#1716](https://github.com/Syynth/brink/pull/1716))
- *(brink-ir,brink-syntax-native)* @[element]/@[style] annotation declaration surface ([#1719](https://github.com/Syynth/brink/pull/1719))
- *(compiler)* lift lambdas to callable function values ([#1709](https://github.com/Syynth/brink/pull/1709))
- *(analyzer)* E157 lint for unnamed stateful choices/sequences
- *(analyzer)* dual-reading `use`/`IMPORT` trailing segments ([#1592](https://github.com/Syynth/brink/pull/1592))
- *(brink-ir)* close emitter construct-coverage gaps for issue #1335 (B0.8b)
- *(brink-ir)* @[allow(Exxx)] source-level diagnostic suppression ([#1161](https://github.com/Syynth/brink/pull/1161))
- *(brink-ir)* per-branch source spans on CondBranch/SequenceBranch ([#404](https://github.com/Syynth/brink/pull/404))
- *(brink-analyzer)* warn when a contains() needle is statically non-key-domain ([#582](https://github.com/Syynth/brink/pull/582))
- *(brink-ir)* widen hir::Stitch with return_type (issue #1509)
- *(stdlib)* rename seq remove-by-index to `remove_at` ([#1484](https://github.com/Syynth/brink/pull/1484)) ([#1501](https://github.com/Syynth/brink/pull/1501))
- *(native)* `: type` annotation grammar — params, bindings, returns (#1487, #1488, #1489)
- *(compiler)* B1 or-coalescing surface spelling on the native dialect ([#1460](https://github.com/Syynth/brink/pull/1460))
- *(brink-syntax-native)* unquoted `::`-path arg grammar for annotations
- *(brink-ir)* emit_native round-trips labeled gather/mid-flow lines ([#1335](https://github.com/Syynth/brink/pull/1335))
- *(brink-analyzer,brink-ir)* B0.9 native accept-list admission gate ([#1179](https://github.com/Syynth/brink/pull/1179))
- *(brink-syntax-native, brink-ir)* body-dialect selectors (~{ }/>{ }) + fn code-ground default ([#1309](https://github.com/Syynth/brink/pull/1309))
- *(brink-syntax-native,brink-ir)* native return/break/continue, compound assign ([#1322](https://github.com/Syynth/brink/pull/1322))
- *(brink-syntax-native,brink-ir)* native code-ground control-flow (if/while/for/until) + statement lowering ([#1177](https://github.com/Syynth/brink/pull/1177))
- *(lower_native)* implicit `-> DONE` for flows that fall off the end
- *(NS-A8)* protocol fence (E118), analyzer typing, tests, tier1 case, changeset (rebuild 2/3)
- *(NS-A5)* ranges as a real Value kind + the inhabited-range refinement ([#1111](https://github.com/Syynth/brink/pull/1111))
- *(NS-A6)* rng-as-cell — the RNG formalized, draws are writes, the rand verbs ([#1112](https://github.com/Syynth/brink/pull/1112))
- *(NS-A2)* effect-row extension — emits + tags + faults ([#1108](https://github.com/Syynth/brink/pull/1108))
- *(NS-A1)* Option[T] as the third parameterized builtin ([#1107](https://github.com/Syynth/brink/pull/1107))
- *(ir)* FS-3c per-await-site liveness → name-keyed frame shapes
- *(analyzer,db)* await-condition purity gate (E105) built on the effects machinery
- *(ir,analyzer)* await HIR lowering, strict-ink gate (E051), LIR fence (E052)
- *(t1e-2)* real MakeProjection/ProjRead/ProjWrite lowering + persistence
- *(compiler)* T1e-1 path-projection grammar + HIR + analyzer ([#831](https://github.com/Syynth/brink/pull/831))
- *(compiler,format,runtime)* M-3 renames — #@was, alias table, rehydration miss-path
- *(compiler)* M-2 visibility model + HIR imports + §7 diagnostics
- *(t1c)* T1c-2 — lower/execute/persist #fn function values ([#700](https://github.com/Syynth/brink/pull/700))
- *(analyzer)* Ty::Fn lattice point + fn(T…): R boundary annotations (E062 retired) ([#699](https://github.com/Syynth/brink/pull/699))
- *(analyzer)* #fn creation-site diagnostics E079/E080/E081 ([#699](https://github.com/Syynth/brink/pull/699))
- *(ir)* Expr::FnLiteral HIR + non-suppressible T1c-1 lowering fence ([#699](https://github.com/Syynth/brink/pull/699))
- *(runtime,analyzer)* int()/float()/string() conversion intrinsics ([#659](https://github.com/Syynth/brink/pull/659))
- *(ir,codegen,runtime)* TM-4c structs LIR + codegen ([#666](https://github.com/Syynth/brink/pull/666))
- *(syntax,ir,analyzer)* TM-4b structs grammar + HIR + analyzer, diagnostics-only ([#665](https://github.com/Syynth/brink/pull/665))
- *(analyzer,db)* TM-3 — types = strict policy, Unknown/Conflicted-escape, E063 wiring ([#619](https://github.com/Syynth/brink/pull/619))
- *(syntax,analyzer)* CONST declarations accept type annotations ([#641](https://github.com/Syynth/brink/pull/641))
- *(syntax)* TM-2 inline type annotation syntax — grammar/HIR/fmt/IDE, feeding signature() ([#618](https://github.com/Syynth/brink/pull/618))
- *(compiler,runtime)* T1b-3 — stdlib slice 1: len/keys/values/contains + push/insert/remove ([#571](https://github.com/Syynth/brink/pull/571))
- *(compiler,runtime)* T1b-2 — blocks, loops, collections, indexing go live ([#570](https://github.com/Syynth/brink/pull/570))
- *(compiler)* T1b-1 superset grammar + HIR + dialect gate ([#569](https://github.com/Syynth/brink/pull/569))

### Fixed

- *(analyzer)* teach ufcs::resolve to walk decl-default lambda bodies
- *(review)* address PR #2931 findings on #2264/E186
- *(compiler)* E186 — block + attach on one @[convention] handler is a hard error
- *(brink-ir)* spell synthesized choice segments c-{n}, never a legal identifier ([#2229](https://github.com/Syynth/brink/pull/2229))
- *(brink-ir)* file-qualify lower_knot_chunk's IdAllocator paths ([#2229](https://github.com/Syynth/brink/pull/2229))
- *(pr-2892)* address review findings — changeset + doc-comment fix
- *(brink-ir)* give lower_inline_block field-assignment/mutator dispatch parity with mod.rs
- *(lir)* extract block-scoped-temp-call refusal to satisfy too_many_lines
- *(lir)* correct E183 reachability, mirror E082 for block-scoped calls
- *(brink-ir)* replace test-helper panics with assert+expect — clippy::panic has no carve-out here
- *(brink-ir)* classify_node_compiled mirrors try_claim's attach-mode decline (#2351 review)
- *(brink-ir)* CUE/PARENTHETICAL tag extensions strip-then-match, uniformly
- *(brink-web,brink-ir)* close explain-match attach review gaps ([#2311](https://github.com/Syynth/brink/pull/2311))
- *(brink-ir,brink-web)* explain-match wasm DTOs carry mode/disposition/attach ([#2311](https://github.com/Syynth/brink/pull/2311))
- *(brink-ir,brink-web)* classify_line agrees with try_claim's sub-node selection ([#2351](https://github.com/Syynth/brink/pull/2351))
- *(brink-ir)* address #2344 review findings on heading slug/tag routing
- *(brink-ir)* slug-bearing headings strip structure, then match ([#2077](https://github.com/Syynth/brink/pull/2077))
- *(docs)* address #2315 review findings on NS-T prose drift sweep
- *(brink-ir,brink-web)* address #2309 review findings on explain-match
- *(brink-format,brink-ir)* strip transitions/templates from ConventionsProjectionDef
- *(brink-analyzer,brink-ir,brink-db)* exempt conventions injection from M-2 gate (#2297 review)
- *(brink-ir,brink-db,brink-analyzer)* conventions claiming reaches the whole project ([#2289](https://github.com/Syynth/brink/pull/2289))
- *(brink-syntax,brink-analyzer,brink-ir,brink-ide)* address PR #2271 review findings
- *(brink-ir,brink-analyzer)* register RefKind::Type for field/TM-2/temp annotations ([#2249](https://github.com/Syynth/brink/pull/2249))
- *(brink-ir)* address PR #2269 review findings on succession-row validation
- *(brink-ir)* remove unfulfilled too_many_lines expect on try_claim
- *(brink-analyzer)* resolve referrer_module by file, not the def's own id (#2252 review)
- *(brink-db,brink-ir)* correct stale story::std prose after peer-root ruling
- *(brink-db)* std:: mounts as a PEER ROOT of story::, not a child of it ([#2245](https://github.com/Syynth/brink/pull/2245))
- *(brink-ir)* address #2248 review findings on struct-shape resolution
- *(brink-ir)* LIR struct-shape resolution consumes analyzer scoping, closes fast-path std gate ([#2246](https://github.com/Syynth/brink/pull/2246))
- *(brink-ir)* correct doc claims + silent-drop comments on struct-shape resolve (#2239 review)
- *(brink-ir)* struct shapes get distinct ids + referrer-scoped resolution ([#2238](https://github.com/Syynth/brink/pull/2238))
- *(brink-ir)* file-scope lookup_address_id; reword untraced block-capture claim (#2226 review)
- *(brink-ir)* thread FileId through lambda-stamping's label lookup ([#2215](https://github.com/Syynth/brink/pull/2215))
- *(brink-ir,brink-analyzer)* close std-mount fallback gap; fix stale docs and vacuous test (#2197 review)
- *(brink-ir)* content::lower_inline_block also dispatches indexed assignment (PR #2211 review)
- *(brink-ir)* classic-line `~ a[i] = v` no longer silently drops ([#2174](https://github.com/Syynth/brink/pull/2174))
- *(brink-ir,brink-web)* address PR #2194 review findings on stale docs and missing #1582 e2e coverage
- *(brink-ir)* enforce E148 as-binding immutability on struct-field write/mutator
- *(brink-ir)* E179 groups by order instead of walking all pairs; names both handlers
- *(brink-test-harness,brink-ir)* apply PR #2150 review findings
- *(brink-ir)* wire native divert-target call args into DivertTarget::args ([#2136](https://github.com/Syynth/brink/pull/2136))
- *(brink-ir)* backtick DefinitionId in a test doc comment to satisfy clippy::doc_markdown
- *(brink-ir)* stop double-prefixing a nested lambda's fallback id (#1727 review)
- *(brink-ir)* HIR mints a lifted lambda's identity, LIR consumes it ([#1727](https://github.com/Syynth/brink/pull/1727))
- *(brink-ir)* address #2110 review findings for choice-guard `as` binding
- *(brink-ir)* correct the pre-fix symptom in the block-injection regression test (#2068 review)
- *(brink-ir,brink-analyzer)* carry block-capture flag across the cross-file injection join ([#2068](https://github.com/Syynth/brink/pull/2068))
- *(brink-ir)* eval_const_lambda's unreachable arm refuses loudly instead of a silent Null (#1774 review)
- *(brink-ir)* thread real UFCS/coalesce tables through decl-default lambda lowering (#1774 review)
- *(brink-ir,brink-analyzer)* apply PR #2081 review findings for #1720
- *(brink-ir)* capture_block must not fold element-level lines fused onto a CONTENT_LINE ([#1839](https://github.com/Syynth/brink/pull/1839))
- *(brink-ir)* apply review findings for #2063 (F4 re-anchor shape, fn-default e2e coverage, docs overclaim)
- *(brink-ir)* terminate whole-body code-ground `~{ }`/fn-default call output ([#2056](https://github.com/Syynth/brink/pull/2056))
- *(brink-ir)* apply review findings for #2055 (~{ } output boundary, code-ground return refusal, stale docs)
- *(brink-syntax-native,brink-ir)* ~{ } logic block and ~ until (await) grammar at prose-body position
- *(prose-dialect)* strip recognized escape's backslash in tag/cue-name/scene-title text ([#2045](https://github.com/Syynth/brink/pull/2045))
- *(brink-ir)* re-nest ink's IfElse conditional in the native emitter ([#1975](https://github.com/Syynth/brink/pull/1975))
- *(brink-ir,brink-syntax-native)* apply review findings for #1992's prose-line escape (PR #2028)
- *(brink-ir)* escape leading `!name` on native emit, close #2004 review gaps
- *(brink-ir)* correct false backward-compat claim + pin ty wire shape (#1997 review)
- *(brink-ir,brink-syntax-native,brink-respell)* apply PR #2015 review findings
- *(brink-syntax-native,brink-ir)* prose-body statement grammar for temp decl + emitter parity ([#1972](https://github.com/Syynth/brink/pull/1972))
- *(brink-ir)* assignment logic line keeps its EndOfLine on an emitting call (#1991 review F1/F1-minor)
- *(brink-syntax-native)* `~ stmt` content-ground line escape no longer swallowed as prose ([#1991](https://github.com/Syynth/brink/pull/1991))
- *(docs)* correct #1951 triage roll-up arithmetic, drop drifting line cites, add nesting-shape test
- *(brink-ir)* apply review findings for E172 directive-shaped-tag PR ([#1953](https://github.com/Syynth/brink/pull/1953))
- *(brink-ir)* add E172 for ink-dialect `#@…` directive-shaped tags in native ([#1835](https://github.com/Syynth/brink/pull/1835))
- *(brink-ir)* correct E171's PR #1898 review findings
- *(brink-ir)* sound E170 overlap heuristic, dispatch-order bug, test/doc gaps
- *(clippy)* merge identical match arms in generate_witness_from_hir
- *(brink-ir)* extract fold_path_ref to satisfy clippy::too_many_lines
- *(brink-ir)* native decl-initializer bare fn refs fold to FnRef, not DivertTarget
- *(brink-ir)* claim-module fence + duplicate-claim diagnostic (#1847, #1848)
- *(brink-ir,brink-ide)* apply PR #1845 review findings — claim-ref rename corruption, E166/E167 renumber, spec drift
- *(brink-ir,brink-compiler,docs)* address PR #1842 review findings on #1839 block-element surface
- *(#1552)* repair rebase fallout and finish the casing sweep
- *(brink-ir,wasm-types)* escape emitted markup text/attrs; TS Span types
- *(brink-ir)* admit a lone point-marker span to Template recognition
- clippy — disallowed_types allow for always-empty file_paths map, panic allow in test
- *(brink-ir)* parse_style checks !ok before emptiness, drop duplicate match arm (#1724 review)
- *(brink-ir)* element/style are recognized annotation names, not E111 (#1724 review)
- *(brink-ir,brink-analyzer,brink-db)* PR #1713 review findings for #1680 gap doc
- *(compiler)* lambda capture scan misses call-callee/UFCS/field reads; guard self-reference (PR #1710 review, issue #1709)
- *(compiler,analyzer,runtime,docs,wasm)* PR #1708 review findings for E157/anonymous-state ([#1674](https://github.com/Syynth/brink/pull/1674))
- *(brink-ir)* E156 must see an `if`/`while` `as` binding, not just enclosing ancestors
- *(ir)* repair the #1490/#1685 merge resolution that turned CI red
- *(brink-ir)* address PR #1694 review findings — inaccurate docs claims + missing cartesian-product test
- *(brink-ir)* qualify anonymous root-content scope paths by owning file ([#1504](https://github.com/Syynth/brink/pull/1504))
- *(analyzer)* address PR #1686 review findings on dual-reading use/IMPORT
- *(review)* address #1662 findings — vacuous overlay/projection guards, doc wording
- *(docs)* address review findings on root-content-identity-findings.md ([#1504](https://github.com/Syynth/brink/pull/1504))
- *(brink-ir)* address review findings on PR #1647 (issue #1335 round 2)
- *(brink-ir)* branchless-body first arm must not contain the else arm ([#404](https://github.com/Syynth/brink/pull/404))
- *(analyzer)* address review findings on import-alias precedence ([#1596](https://github.com/Syynth/brink/pull/1596))
- *(analyzer)* honor import aliases in resolution, not just E089 ([#1590](https://github.com/Syynth/brink/pull/1590))
- *(brink-ide)* narrow tail-segment + decl-initializer rename ranges ([#1571](https://github.com/Syynth/brink/pull/1571))
- *(brink-ir)* don't treat annotations on un-lowered nested fn/depth-3 flow as consumed
- *(brink-ir)* correct native body-annotation test to expect E112
- *(brink-ir,brink-analyzer)* address review findings on ResolvedRef range contract PR
- *(brink-ir)* rebase onto main, re-measure the oracle ratchet post-merge
- *(brink-analyzer)* E065/E066 return-escape check extends past is_function (E150)
- apply review findings for #1528 (assemble_analyzer_tables)
- *(review)* address PR #1536 review findings
- *(brink-ir)* correct stale doc refs in AnalyzerTables migration
- *(brink-ir)* thread CoalesceLookup through ufcs_auto_ref.rs's direct call
- *(brink-ir)* replace unbounded-recursion fallback with unreachable!
- review findings for PR #1524 (changeset wording + doc + e2e mirror)
- *(compiler)* refuse with E144 instead of silently dropping a drifted UFCS prelude call
- *(compiler)* recognize UFCS receiver-splice for statement-only mutators
- *(compiler)* wire LIR lowering to consume the ufcs_resolution verdict table
- apply review findings for PR #1500 (root final gather)
- *(lint)* collapse nested if in lower_ref_projection_arg's as-binding check
- *(review)* merge main into B1b as-binding + apply reviewer findings ([#1475](https://github.com/Syynth/brink/pull/1475))
- *(review)* doc-ordering, fn(...) emission, and naming fixes for #1496
- *(brink-ir)* take the native binding annotation by reference (clippy)
- *(analyzer)* surface coalescing mismatches as E066, fix eager-eval and precedence gaps (#1469 review)
- *(brink-ir)* lower the unquoted @[was(story::old::path)] arg into hir.module.was ([#1355](https://github.com/Syynth/brink/pull/1355))
- *(brink-ir)* guard emit_native against silent label drops in choice/gather emission
- *(brink-ir)* diagnose dropped divert-target call args, not silent
- *(lower_native)* bare return in a non-function container is a tunnel redirect
- *(ir)* FS-3c frame-shape liveness must read index/field target bases
- *(brink-ir)* dedup E046 on directives with dynamic content
- *(compiler)* reject direct calls through a computed fn-value callee instead of silently dropping them ([#869](https://github.com/Syynth/brink/pull/869))
- *(runtime,compiler)* map[newKey] = value inserts on assign ([#856](https://github.com/Syynth/brink/pull/856))
- *(analyzer)* escalate cross-declared-module duplicate names to E096 under brink dialect
- *(analyzer)* derive per-file module for symbol-less files; narrow E088 doc
- *(ir)* broaden E077 title to cover #fn bound value arguments
- *(ir)* E077 for bare VAR reference nested in a decl-default collection/#fn literal ([#743](https://github.com/Syynth/brink/pull/743))
- *(analyzer)* call()/bind() intrinsics statically check under strict mode ([#733](https://github.com/Syynth/brink/pull/733))
- *(brink-ir)* correct bind lowering comment on strict-mode static checking
- *(runtime)* CallVariable carries argc — direct-call arity mismatch faults cleanly ([#721](https://github.com/Syynth/brink/pull/721))
- *(ir)* E077 for non-constant array elements / map values in declaration defaults ([#673](https://github.com/Syynth/brink/pull/673))
- *(analyzer,db)* E063 error-severity under strict + void-assignment error E067 ([#619](https://github.com/Syynth/brink/pull/619))
- *(analyzer)* apply TM-2 review rulings — E063 opt-in ratified, strict-ink suppresses E061/E062 ([#618](https://github.com/Syynth/brink/pull/618))
- *(compiler,codegen)* real error paths for two debug_assert-guarded backstops (#585, #586)
- *(compiler)* break/continue outside a loop and mutator arity are E057/E058 compile errors (#577, #581)
- *(compiler)* route LogicBlock through real lowering in inline blocks ([#578](https://github.com/Syynth/brink/pull/578))
- *(compiler)* non-suppressible ICE backstop for residual T1b HIR nodes ([#572](https://github.com/Syynth/brink/pull/572))

### Other

- Address review findings: accept cascaded IDE snapshots, truthful docs, complete changeset
- conditional/sequence arm prose gets a real HIR Content span ([#981](https://github.com/Syynth/brink/pull/981))
- Fix pub_prefix rustdoc inaccuracies from review
- Emit `pub` for Some(Public) visibility in emit_native instead of refusing
- fix review findings on #2352 dispatch projection rows
- ConventionsProjection carries !name dispatch handler rows ([#2352](https://github.com/Syynth/brink/pull/2352))
- Address PR #2950 review findings for E187 (CONST reassignment)
- Reject CONST reassignment across every write channel (E187, issue #2201)
- fix coalesce misattribution; pin const-valued receiver in decl lambda (#2096 review)
- sort issue_2903 test-mod registration into place
- Fix #2903: index-operand postfix (a[0]++, m["k"]++) silently non-mutating
- Merge branch 'main' into auto/issue-2229
- WIP fix(brink-ir): file-qualify stamp_container_ids's per-knot interior scope ([#2229](https://github.com/Syynth/brink/pull/2229))
- Merge pull request #2899 from Syynth/auto/issue-2262
- Fix review findings: dedupe postfix lowering, document x++/x-- and E074
- bare-variable postfix x++/x-- inside a ~ { … } block never mutated
- Narrow #2185's E074 to accurate messages; fix the vacuous projection ground-truth test
- Merge branch 'main' into auto/issue-2185
- Apply #2866 review findings: missed builtin-hover copy, self-testing mutator test, stale docs
- unify reserved builtin/stdlib name lists into one canonical source
- Fix silent drop: author-declared symbols now shadow built-in names
- lir lower_call: refuse a non-callable resolved symbol kind (E183)
- Fix review findings on #2759 build-stamp freshness check
- Replace check-target-freshness.mjs's static heuristic with a real build stamp ([#2759](https://github.com/Syynth/brink/pull/2759))
- Fix bare-name-keyed locals shadowing hazard for lambda-descending analyzer walkers
- confirm no lambda-param generic-annotation gap ([#2775](https://github.com/Syynth/brink/pull/2775))
- garble SpanAttr provenance in seam test; document E173's whole-span ranging
- give SpanAttr per-attribute provenance so E165 doesn't collapse
- cargo fmt decl_attach's signature
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge branch 'main' into auto/issue-2179
- Merge origin/main into auto/issue-2077 (resolve #2079 compact-cue conflict)
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge origin/main into train-fix
- Merge remote-tracking branch 'origin/main' into auto/issue-2108
- checkpoint before merging origin/main (issue #2108)
- merge origin/main (#2239 struct-shape fix) so CI re-runs green
- Merge pull request #2203 from Syynth/auto/issue-2166
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge pull request #2194 from Syynth/auto/issue-1582
- Merge remote-tracking branch 'origin/main' into train-fix
- *(brink-ir)* fix stale fn conventions()/#1840 prose the #2164 split left behind
- Merge remote-tracking branch 'origin/main' into train-fix
- sync prose-dialect/directive-annotations/compiler specs with the @[convention] split
- *(brink-analyzer)* fix stale hand-walk prose after #2098 visitor migration
- Merge remote-tracking branch 'origin/main' into fix/2131-clippy
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge origin/main into train-fix for PR #2110
- *(brink-ir)* fix doc nits in external_conventions.rs (#2068 review)
- *(brink-ir,brink-compiler)* cover assemble_program's lifted-container append (#1774 review)
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge origin/main into train-fix
- *(brink-ir)* rustfmt issue #1974's emitter tests after the main merge
- Merge remote-tracking branch 'origin/main' into fix/2030-refresh
- Merge origin/main into train-fix for PR #2042
- Merge origin/main into auto/issue-1975 and apply review fixes for #1975
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge remote-tracking branch 'origin/main' into train-fix
- *(brink-ir)* rustfmt the renamed escape_leading_line_start_sigil call site
- Merge origin/main into auto/issue-1994
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge pull request #2007 from Syynth/auto/issue-1996
- *(brink-ir)* reclassify Assignment/ExprStmt prose-body refusal as emitter-only (#1991 review F4)
- Merge remote-tracking branch 'origin/main' into train-fix
- *(brink-ir)* re-triage #1951's six native grammar holes, correct three verdicts
- cargo fmt fixup for E172 tests ([#1835](https://github.com/Syynth/brink/pull/1835))
- E171 for a claiming handler's typed captured params ([#1849](https://github.com/Syynth/brink/pull/1849))
- *(brink-ir/brink-analyzer)* project-level injection point for an evaluated conventions registry ([#1863](https://github.com/Syynth/brink/pull/1863)) ([#1888](https://github.com/Syynth/brink/pull/1888))
- run cargo fmt
- Implement E170: detect non-identical overlapping claiming patterns
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge remote-tracking branch 'origin/main' into train-fix
- *(#1860)* fix E168 false positive, N>=3 double-report, and suppressibility
- Merge remote-tracking branch 'origin/main' into train-fix
- give SpanPart per-span provenance so E164/E165 don't collapse ([#1782](https://github.com/Syynth/brink/pull/1782))
- add LambdaBody::all_exprs for walkers that search the whole body
- record the Ty::Fn effect row and its open stratum question ([#1680](https://github.com/Syynth/brink/pull/1680))
- cargo fmt
- cargo fmt
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge origin/main into train-fix
- *(brink-db)* pin the lambda effect-row gap blocking #1680
- Merge remote-tracking branch 'origin/main' into train-fix
- cargo fmt
- *(brink-ir)* round-trip emit_native's new Expr::Lambda arm
- Merge remote-tracking branch 'origin/main' into auto/issue-1685
- Merge origin/main into train-fix for PR #1694
- Merge remote-tracking branch 'origin/main' into train-fix
- satisfy the implicit-hasher and disallowed-types lints on the #1504 seam
- Merge remote-tracking branch 'origin/main' into train-fix
- merge origin/main into train-fix for PR #1662
- *(brink-ir)* root-content DefinitionId identity findings + acceptance tests ([#1504](https://github.com/Syynth/brink/pull/1504))
- Merge remote-tracking branch 'origin/main' into train-fix
- *(#460)* write up the compile-time profile and defer the per-container split
- *(brink-db)* share one chunk-lowering context across knot memos ([#460](https://github.com/Syynth/brink/pull/460))
- merge origin/main + address review findings on #1615
- *(brink-ir)* cargo fmt issue_1161_allow_annotation.rs
- *(brink-ir)* pin @[allow] statement-position attachment
- *(brink-ir)* make the allow-scope-vs-expect test non-vacuous
- Merge remote-tracking branch 'origin/main' into train-pr
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge origin/main into train-fix for PR #1579
- Merge remote-tracking branch 'origin/main' into train-fix
- cargo fmt
- Merge remote-tracking branch 'origin/main' into train-fix
- *(brink-ir,brink-analyzer)* pin the call-path ResolvedRef range contract ([#1561](https://github.com/Syynth/brink/pull/1561))
- Merge origin/main into train-fix
- Merge remote-tracking branch 'origin/main' into train-fix
- cargo fmt
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge origin/main into train-fix
- update stale UFCS wiring prose now that brink-ide reads the table
- *(brink-ir)* bundle UfcsLookup/CoalesceLookup into AnalyzerTables<'a>
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge remote-tracking branch 'origin/main' into train-pr
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge origin/main into train-fix for PR #1500
- Merge remote-tracking branch 'origin/main' into train-fix
- fix stale claim in b2_for_two_binding.rs about the harness's call sequence
- merge main + address review: wire E084/E106 to native surface, name E138's key
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge remote-tracking branch 'origin/main' into train-pr
- Merge remote-tracking branch 'origin/main' into train-pr
- Merge origin/main into train-fix for PR #1341
- Merge origin/main into train-fix for PR #1331
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge remote-tracking branch 'origin/main' into train-pr
- respell-corpus vs oracle differential test + decision log
- re-respell tier1-brink-respell fixtures to the main-flow convention
- flow main() native story-entry convention
- first-class doc comments on the native surface ([#1208](https://github.com/Syynth/brink/pull/1208)) ([#1218](https://github.com/Syynth/brink/pull/1218))
- add `tail` to the shared HIR Block (expand phase) ([#1216](https://github.com/Syynth/brink/pull/1216))
- prose-dialect body lowering — the heart ([#1176](https://github.com/Syynth/brink/pull/1176)) ([#1215](https://github.com/Syynth/brink/pull/1215))
- native .brink declaration + module-skeleton lowering to HIR
- B0.6 prep: wire brink-syntax-native into brink-ir, reserve E129/E130
- B0.4 step 4: delete the hand-built manifest path — wire project_manifest in
- B0.4 step 2: add project_manifest — SymbolManifest as a pure projection of HIR
- B0.4 step 1: additive doc/visibility/was HIR fields (declaration nodes)
- reserve E121-E128 for the HIR admission validator
- explicit Return.kind — ReturnKind replaces ptr-presence semantics (Q7a, D5/F-I#6)
- Merge pull request #1156 from Syynth/auto/ns-a7
- analyzer wiring — Ty::Weighted, typing arms, effect rows, F29 discharge
- weighted/roll/heap lowering + codegen + E120 construction gate
- sort family in LIR/codegen/analyzer + E119 comparator-contract gate
- Merge origin/main (post-#1133) — E116+E117 coexist; A5 rows absorbed into the shared intrinsics table
- Merge origin/main (NS-A3 registry) into ns-a6
- warn on statically-visible non-key-domain map-literal keys (E106)
- Merge remote-tracking branch 'origin/main' into train-pr
- fmt, E052 fence doc, changeset, and exhaustive-match completion for await
- *(bevy-brink)* BH follow-up batch — manifest de-drift, reachability hardening, counter proof, baseline tripwire
- *(brink-ir)* cover #@was dynamic-content dedup + add web changeset
- Merge remote-tracking branch 'origin/main' into train-pr
- Merge remote-tracking branch 'origin/main' into train-pr
- *(brink-ir,brink-db)* FG-6 cleanup tail — audit lower_to_program/composed-equals-monolithic retirement ([#841](https://github.com/Syynth/brink/pull/841))
- *(brink-ir,brink-db)* own memo for the LIR prelude's decl collection
- Merge remote-tracking branch 'origin/main' into train-pr
- Merge pull request #820 from Syynth/docs/537-lru-ruling
- *(brink-ir)* FG-4c — per-container LIR chunks with symbolic refs ([#817](https://github.com/Syynth/brink/pull/817))
- Merge origin/main into train-pr
- origin/main into train-fix for PR #770
- M-2 clippy/fmt polish + @brink-lang/web changeset
- Merge remote-tracking branch 'origin/main' into train-pr
- Merge pull request #749 from Syynth/auto/issue-733
- Merge origin/main into train-fix for PR #730
- Merge pull request #729 from Syynth/auto/issue-680
- Merge remote-tracking branch 'origin/main' into train-pr
- Merge remote-tracking branch 'origin/main' into HEAD
- Merge remote-tracking branch 'origin/main' into train-pr
- sweep converter mentions after pipeline retirement ([#544](https://github.com/Syynth/brink/pull/544))
- salsa into brink-db — query-memoized pipeline stages (phase 0 slice B) ([#515](https://github.com/Syynth/brink/pull/515))
- #@local implies VISITS counting ([#496](https://github.com/Syynth/brink/pull/496)) ([#507](https://github.com/Syynth/brink/pull/507))

## [0.0.11](https://github.com/Syynth/brink/compare/brink-ir-v0.0.10...brink-ir-v0.0.11) - 2026-07-11

### Other

- Merge pull request #495 from Syynth/bronch/compiler-local-var-keyword-0fdbbc
- #@local directive — flow-private scope through HIR/LIR/codegen ([#473](https://github.com/Syynth/brink/pull/473))

## [0.0.10](https://github.com/Syynth/brink/compare/brink-ir-v0.0.9...brink-ir-v0.0.10) - 2026-07-10

### Other

- Story::new takes Arc<Program>, not &Program
- shared read-only HIR visitor + migrate 4 walkers ([#457](https://github.com/Syynth/brink/pull/457)) ([#464](https://github.com/Syynth/brink/pull/464))

## [0.0.9](https://github.com/Syynth/brink/compare/brink-ir-v0.0.8...brink-ir-v0.0.9) - 2026-07-06

### Added

- *(ide,editor,web)* fold kinds — structural/machinery/narrative + summary pills ([#365](https://github.com/Syynth/brink/pull/365)) ([#400](https://github.com/Syynth/brink/pull/400))
- *(ir,ide,web)* dialogue-dialect schema + Rust classification ([#368](https://github.com/Syynth/brink/pull/368)) ([#386](https://github.com/Syynth/brink/pull/386))

### Fixed

- *(ide,editor)* sigil-wins-chain + conditional scaffold classification ([#413](https://github.com/Syynth/brink/pull/413)) ([#425](https://github.com/Syynth/brink/pull/425))
- *(ir,editor)* reconcile at-cue Parenthetical content_group vs template round-trip ([#406](https://github.com/Syynth/brink/pull/406)) ([#424](https://github.com/Syynth/brink/pull/424))

## [0.0.8](https://github.com/Syynth/brink/compare/brink-ir-v0.0.7...brink-ir-v0.0.8) - 2026-07-01

### Other

- #312 + #313 (Track N core): shared INCLUDE-block detector + fold/auto-import cores ([#331](https://github.com/Syynth/brink/pull/331))

## [0.0.7](https://github.com/Syynth/brink/compare/brink-ir-v0.0.6...brink-ir-v0.0.7) - 2026-06-20

### Other

- welcoming README (playground/book links, overview, LLM-experiment disclaimer) ([#285](https://github.com/Syynth/brink/pull/285))

## [0.0.6](https://github.com/Syynth/brink/compare/brink-ir-v0.0.5...brink-ir-v0.0.6) - 2026-06-19

### Added

- *(studio)* host functions panel categories + search ([#210](https://github.com/Syynth/brink/pull/210)) ([#270](https://github.com/Syynth/brink/pull/270))

## [0.0.5](https://github.com/Syynth/brink/compare/brink-ir-v0.0.4...brink-ir-v0.0.5) - 2026-06-17

### Added

- *(studio)* argument widgets stage 5 — arg-groups + inter-arg context + modal ([#222](https://github.com/Syynth/brink/pull/222))
- *(studio)* argument widgets stage 2 — argument_widgets query + Fill ([#219](https://github.com/Syynth/brink/pull/219))

## [0.0.4](https://github.com/Syynth/brink/compare/brink-ir-v0.0.3...brink-ir-v0.0.4) - 2026-06-15

### Added

- *(manifest,ide)* static value-source + value-label inlay hints ([#174](https://github.com/Syynth/brink/pull/174)) ([#203](https://github.com/Syynth/brink/pull/203))

## [0.0.3](https://github.com/Syynth/brink/compare/brink-ir-v0.0.2...brink-ir-v0.0.3) - 2026-06-13

### Added

- host capability manifest — Tier 1 + closed Tier 2 (Track B MVP) ([#74](https://github.com/Syynth/brink/pull/74))

### Fixed

- *(syntax)* accept contextual keywords as EXTERNAL names and params ([#75](https://github.com/Syynth/brink/pull/75))
- *(compiler)* surface syntax errors + reject malformed inline conditionals (closes #44) ([#48](https://github.com/Syynth/brink/pull/48))

### Other

- Studio IDE: doc comments + type hints for all declarations (Track B integration) ([#101](https://github.com/Syynth/brink/pull/101))
