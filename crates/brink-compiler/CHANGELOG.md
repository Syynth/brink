# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.17](https://github.com/Syynth/brink/compare/brink-compiler-v0.0.16...brink-compiler-v0.0.17) - 2026-09-04

### Added

- *(brink-ir)* add E195 warning for a completely empty choice ([#3365](https://github.com/Syynth/brink/pull/3365))
- *(compiler)* compat-deny diagnostic tier; E194 for knot temp read from stitch
- *(compiler,runtime)* E193 for undominated `~ temp` reads; resolve them to the slot (#3354, #3362)
- *(lir)* thread source provenance through lir::Expr (issue #3183 D5 remainder)
- *(lir)* thread source provenance through Container and Stmt ([#3183](https://github.com/Syynth/brink/pull/3183))

### Fixed

- *(brink-codegen-inkb)* a choice's display text keeps its whitespace runs ([#3508](https://github.com/Syynth/brink/pull/3508))
- *(brink-ir, brink-runtime)* the else-arm half of #3507 — spring in branch bodies, whitespace-only line refs pass the glue scan
- *(brink-ir)* lower the whitespace between an inline construct and <> to a Spring ([#3507](https://github.com/Syynth/brink/pull/3507))
- *(brink-ir)* lift hoists prefix interpolations ahead of the construct ([#3395](https://github.com/Syynth/brink/pull/3395))
- *(brink-ir)* a sequence cloned into a lift's branches shares one counter ([#3401](https://github.com/Syynth/brink/pull/3401))
- *(brink-analyzer)* E194 must fire on plain writes, not just reads
- *(brink-analyzer)* E193 speaks the author's own surface vocabulary
- *(brink-runtime)* GetTemp's uninitialized-temp check keys on written, not value
- *(3181)* apply adversarial review findings — span regression + wrong choice locations
- *(codegen)* thread real source_location through the EmitContent/ChoiceOutput flattening path ([#3181](https://github.com/Syynth/brink/pull/3181))
- `[lints] allow` suppresses the diagnostic, and advisory codes are overridable ([#3175](https://github.com/Syynth/brink/pull/3175))

### Other

- Merge remote-tracking branch 'origin/main' into wt-oracle-3401
- golden coverage for source_location values ([#3213](https://github.com/Syynth/brink/pull/3213))

## [0.0.15](https://github.com/Syynth/brink/compare/brink-compiler-v0.0.11...brink-compiler-v0.0.15) - 2026-08-23

### Added

- *(brink-ir)* choice-guard `as` binding lowers for real ([#1508](https://github.com/Syynth/brink/pull/1508))
- *(brink-runtime)* migrate output contract from Line to Step/OutputLine ([#1684](https://github.com/Syynth/brink/pull/1684))
- *(brink-ir)* a native var/const may hold a lambda literal ([#1774](https://github.com/Syynth/brink/pull/1774))
- *(brink-analyzer)* type native bare-name fn values with their target's effect row ([#1876](https://github.com/Syynth/brink/pull/1876))
- *(compiler)* native bare-name fn values ([#1862](https://github.com/Syynth/brink/pull/1862))
- *(lang)* type-name conformance sweep — angle brackets + Uppercase non-primitives, Option<T>/Weighted<T> annotatable ([#1552](https://github.com/Syynth/brink/pull/1552))
- *(compiler)* manifest-validated markup vocabulary (§4.2 second half)
- *(stdlib)* fn-value verb layer slice 2 — filter_map, each, map_each ([#1679](https://github.com/Syynth/brink/pull/1679))
- *(compiler)* lift lambdas to callable function values ([#1709](https://github.com/Syynth/brink/pull/1709))
- *(codegen)* assert no two containers share a DefinitionId ([#1673](https://github.com/Syynth/brink/pull/1673))
- *(diagnostics)* add Info/Hint severity tier below Warning ([#1162](https://github.com/Syynth/brink/pull/1162))
- *(brink-analyzer)* warn when a contains() needle is statically non-key-domain ([#582](https://github.com/Syynth/brink/pull/582))
- *(brink-analyzer)* E149 compile error for array-typed remove() calls
- *(analyzer)* record `or`-coalescing types for LIR lowering ([#1492](https://github.com/Syynth/brink/pull/1492))
- *(compiler)* B1b the `as` binding — one construct, both condition positions ([#1475](https://github.com/Syynth/brink/pull/1475))
- *(compiler)* B1 or-coalescing surface spelling on the native dialect ([#1460](https://github.com/Syynth/brink/pull/1460))
- *(brink-analyzer,brink-db)* B0.9 close — native strict-only enforcement point
- *(brink-driver,brink-compiler,brink-cli)* discover_native over SourceTree — RealFs walk + GitRev git-baseline
- *(NS-A9)* dialect-keyed type-policy default via resolve_type_policy seam
- *(NS-light)* F27 truthiness removal + paren-clause respell + wake-gate gap (#1120, #1128)
- *(NS-A6)* rng-as-cell — the RNG formalized, draws are writes, the rand verbs ([#1112](https://github.com/Syynth/brink/pull/1112))
- *(web)* FS-3w flow-addressed web surface — flow handles, Line::Suspended, wakeCheck ([#978](https://github.com/Syynth/brink/pull/978))
- *(analyzer,db)* await-condition purity gate (E105) built on the effects machinery
- *(t1e-2)* real MakeProjection/ProjRead/ProjWrite lowering + persistence
- *(compiler)* T1e-1 path-projection grammar + HIR + analyzer ([#831](https://github.com/Syynth/brink/pull/831))
- *(modules)* M-2b host semantic-access enforcement for #@private defs
- *(t1c)* T1c-2 — lower/execute/persist #fn function values ([#700](https://github.com/Syynth/brink/pull/700))
- *(analyzer)* dialect-gate #fn under strict-ink (E051) ([#699](https://github.com/Syynth/brink/pull/699))
- *(ir,codegen,runtime)* TM-4c structs LIR + codegen ([#666](https://github.com/Syynth/brink/pull/666))
- *(analyzer,db)* TM-3 — types = strict policy, Unknown/Conflicted-escape, E063 wiring ([#619](https://github.com/Syynth/brink/pull/619))
- *(compiler,runtime)* T1b-2 — blocks, loops, collections, indexing go live ([#570](https://github.com/Syynth/brink/pull/570))
- *(compiler)* T1b-1 superset grammar + HIR + dialect gate ([#569](https://github.com/Syynth/brink/pull/569))

### Fixed

- *(analyzer)* teach ufcs::resolve to walk decl-default lambda bodies
- *(analyzer)* check locals before globals at call sites (#2083 review)
- *(analyzer)* resolve a fn-valued CONST global's call site ([#2083](https://github.com/Syynth/brink/pull/2083))
- *(diagnostics)* correct E035.md param-shape overreach, spec drift, and #2867 link
- *(analyzer)* gate bare list-item lookup to #fn literal sites, not call sites
- *(brink-ir)* follow-up review fixes for E148 as-binding write (PR #2191)
- *(brink-ir)* enforce E148 as-binding immutability on struct-field write/mutator
- *(brink-compiler)* address #2172 review findings on the test-util fence
- *(brink-analyzer)* divert-with-args ref-position argument checking
- *(brink-ir)* address #2110 review findings for choice-guard `as` binding
- *(brink-runtime)* clippy fixes for Step migration (while_let_loop, doc backticks)
- *(brink-analyzer,brink-compiler)* apply review fixes for #2085/#1769
- *(brink-analyzer)* review fixes for #1840 Q4 registration slice
- *(brink-ir)* thread real UFCS/coalesce tables through decl-default lambda lowering (#1774 review)
- *(brink-analyzer)* guard native fn-value shadow check against bare-name list items ([#1901](https://github.com/Syynth/brink/pull/1901))
- *(brink-analyzer)* review fixes for #1900 dotted-field type check
- *(brink-analyzer)* apply string-numeric display-concat carve-out to `+=` too
- *(brink-compiler)* rework native driver strict sweep per review ([#1916](https://github.com/Syynth/brink/pull/1916))
- *(brink-analyzer)* resolve review findings on #1877's typed-assign checks
- *(brink-analyzer)* type native bare-name fn values in decl-initializer position ([#1895](https://github.com/Syynth/brink/pull/1895))
- *(brink-analyzer)* review fixes for #1864 direct-call arg-type PR
- *(brink-analyzer)* native fn-value E080 check must walk decl initializers too
- *(brink-ir,brink-compiler,docs)* address PR #1842 review findings on #1839 block-element surface
- *(test)* drop trailing semicolon so the E083 regression fixture matches the review's verified spelling
- *(review)* correct #1764 reachability overclaim + add E083/E106 regression test
- *(compiler)* lambda capture scan misses call-callee/UFCS/field reads; guard self-reference (PR #1710 review, issue #1709)
- *(compiler,analyzer,runtime,docs,wasm)* PR #1708 review findings for E157/anonymous-state ([#1674](https://github.com/Syynth/brink/pull/1674))
- *(brink-db,brink-compiler)* absolutize root_relative_key + doc/changeset review fixes ([#1706](https://github.com/Syynth/brink/pull/1706))
- *(stdlib)* review findings for PR #1707 — CI clippy fix + comparator/callback role split
- *(review)* apply #1693 review findings for #1504 root-content identity
- *(brink-ir)* qualify anonymous root-content scope paths by owning file ([#1504](https://github.com/Syynth/brink/pull/1504))
- *(brink-compiler)* use code-ground return in E150 stitch-fallthrough e2e test
- *(brink-analyzer)* keep E065/E066 escape reading the def's own body
- *(docs+tests)* scrub stale pre-#1530 E075 rationale flagged by review
- *(analyzer)* review findings on E151 — real control-flow terminator predicate + on-by-default wording (PR #1575)
- *(brink-analyzer)* restore void-annotation guard on return-escape check (PR #1556 review)
- *(review)* correct #1540 review-blocking issues on PR #1548
- *(brink-ir)* walk both operands of Coalesce in collect_counting_refs_expr
- *(compiler,runtime)* `or`-coalescing short-circuits, off analyzer types ([#1471](https://github.com/Syynth/brink/pull/1471))
- *(review)* merge main into B1b as-binding + apply reviewer findings ([#1475](https://github.com/Syynth/brink/pull/1475))
- *(analyzer)* surface coalescing mismatches as E066, fix eager-eval and precedence gaps (#1469 review)
- *(brink-compiler,brink-cli)* guard target/ discovery e2e; fix dropped doc separator
- *(brink-source-tree,brink-driver)* drop SourceTree::list's contradictory root param, guard discover_native against non-.brink keys
- *(brink-analyzer)* decouple T1b dialect gate from native .brink files
- *(brink-compiler)* pass HashMap by reference in compile_multifile test helper
- *(compile)* feed manifest external signatures to the compile-path strict pass (Closes #1004)
- *(brink-analyzer)* E105 await-purity recurses into struct-literal fields
- *(compiler)* reject direct calls through a computed fn-value callee instead of silently dropping them ([#869](https://github.com/Syynth/brink/pull/869))
- *(diagnostics)* retire unreachable codes E011/E013/E018/E019/E028/E053 ([#709](https://github.com/Syynth/brink/pull/709))
- *(analyzer,db)* E063 error-severity under strict + void-assignment error E067 ([#619](https://github.com/Syynth/brink/pull/619))
- *(compiler,codegen)* real error paths for two debug_assert-guarded backstops (#585, #586)
- *(compiler)* non-suppressible ICE backstop for residual T1b HIR nodes ([#572](https://github.com/Syynth/brink/pull/572))

### Other

- Address review: multiline_branch_body retry is dead code, pin honest tests
- Fix mid-line comment fragmenting inline-alternative branches ([#2976](https://github.com/Syynth/brink/pull/2976))
- Address review findings: honest divergence note, pin comment-before-bracket
- Fix mid-line comment fragmenting choice text ([#2960](https://github.com/Syynth/brink/pull/2960))
- Address PR #2950 review findings for E187 (CONST reassignment)
- Reject CONST reassignment across every write channel (E187, issue #2201)
- *(compiler)* pin the var-sibling fn-valued global call site end-to-end
- Fix E185 review findings: unresolved-RHS blind spot, spec drift, off-db test
- Add E185: unknown struct field on a plain assignment target ([#1944](https://github.com/Syynth/brink/pull/1944))
- Merge pull request #2879 from Syynth/auto/issue-2872
- *(diagnostics)* E063 does not fire under brink/gradual typing ([#2872](https://github.com/Syynth/brink/pull/2872))
- Apply PR #2871 review findings: inverted comment, E035.md spec drift, dedupe test
- *(analyzer)* pin locals-as-reserved-builtin-call-site gap (issue #2867)
- Merge origin/main into train-fix
- Apply #2866 review findings: missed builtin-hover copy, self-testing mutator test, stale docs
- Fix PR #2859 review findings: call-site guard, manifest-aware proptest filter, spec drift
- Fix silent drop: author-declared symbols now shadow built-in names
- *(brink-analyzer,brink-ir,brink-db,brink-format)* remove dissolved fn conventions()/register machinery
- *(brink-compiler)* gate closure/path compile entry points behind test-util feature ([#2168](https://github.com/Syynth/brink/pull/2168))
- Merge remote-tracking branch 'origin/main' into train-fix
- *(brink-compiler)* add the self-recursion regression test the PR body claimed already existed (#1774 review)
- *(brink-ir,brink-compiler)* cover assemble_program's lifted-container append (#1774 review)
- *(brink-analyzer)* fix stale #fn-only prose after native bare-name E119 support
- Merge remote-tracking branch 'origin/main' into train-fix
- *(brink-analyzer)* quote decision-log literally, name the lookup_by_name_direct trigger ([#1901](https://github.com/Syynth/brink/pull/1901))
- *(brink-analyzer)* close #1901 empirically — cross-file shadow can never legitimately compile
- Merge pull request #1946 from Syynth/auto/issue-1919
- *(brink-analyzer)* check plain struct-field assignment (~ p.x = expr) against declared field type ([#1900](https://github.com/Syynth/brink/pull/1900))
- *(brink-compiler)* pin E065 in the UFCS wrong-arity regression test
- Merge origin/main into train-fix
- Merge origin/main into auto/issue-1911 for review fixes
- *(brink-analyzer)* check UFCS-desugared call argument types ([#1881](https://github.com/Syynth/brink/pull/1881))
- Merge remote-tracking branch 'origin/main' into train-fix
- *(driver)* make ink native-guard regression test able to fail
- *(driver)* pin exact E063 code+message for the bare-name fn-value mismatch
- cargo fmt tm3_strict_policy.rs
- Merge remote-tracking branch 'origin/main' into train-fix
- cargo fmt e0xx_diagnostics.rs
- *(brink-compiler)* rewrite E166 test for content's resolvable Ty (#1846 review)
- Merge remote-tracking branch 'origin/main' into train-fix
- record the Ty::Fn effect row and its open stratum question ([#1680](https://github.com/Syynth/brink/pull/1680))
- *(brink-compiler)* each/map_each callback output reaches the transcript (#1679 review)
- Merge origin/main into train-fix
- Merge remote-tracking branch 'origin/main' into train-fix
- *(stdlib)* record the shipped trio + changeset; E119 prose across specs and book
- *(stdlib)* e2e fixture + compiler/runtime tests for map/filter/fold
- *(brink-compiler)* pin the lambda codegen fence's own message, not just its code
- Merge remote-tracking branch 'origin/main' into auto/issue-1685
- Merge remote-tracking branch 'origin/main' into train-fix
- *(corpus)* tier-1 case for root weave in entry + INCLUDEd file ([#1504](https://github.com/Syynth/brink/pull/1504))
- *(brink-ir)* root-content DefinitionId identity findings + acceptance tests ([#1504](https://github.com/Syynth/brink/pull/1504))
- merge origin/main + address review findings on #1615
- *(brink-compiler)* prove [lints] Info/Hint reaches ResolvedDiagnostic
- Merge origin/main into train-fix for PR #1585
- Merge origin/main into train-fix for PR #1579
- *(analyzer)* native asymmetric choice-branch dead-end (E151, issue #1219)
- *(analyzer)* cover the widened global type surface + refresh stale gap prose ([#1540](https://github.com/Syynth/brink/pull/1540))
- *(brink-compiler)* correct the coverage claim on the coalesce fall-through test
- *(lint)* clippy fixes for the coalesce threading
- rustfmt the E140 fixture's format! call
- *(B1b)* correct the Option chapter's stale `as` spelling; changeset ([#1475](https://github.com/Syynth/brink/pull/1475))
- F29(a) refined-faults carve-out + end-to-end ordering tests
- Merge origin/main (NS-A3 registry) into ns-a6
- *(brink-compiler)* cross-file diagnostic offset tracking with multi-byte UTF-8
- fmt, E052 fence doc, changeset, and exhaustive-match completion for await
- Merge remote-tracking branch 'origin/main' into train-pr
- *(brink-compiler)* route compile through the story_data query (FG-6 #841)
- Merge remote-tracking branch 'origin/main' into train-pr
- Merge remote-tracking branch 'origin/main' into train-pr
- reconcile e0xx audit with T1c-1 — E062 retired, E052 revived ([#699](https://github.com/Syynth/brink/pull/699))
- Merge remote-tracking branch 'origin/main' into feat/699-t1c1-fn-grammar-typing
- Merge remote-tracking branch 'origin/main' into HEAD
- *(compiler)* TM-5 corpus wing growth — annotations, structs, strict e2e ([#621](https://github.com/Syynth/brink/pull/621))
- sweep converter mentions after pipeline retirement ([#544](https://github.com/Syynth/brink/pull/544))
- retire the converter pipeline — remove brink-converter, brink-json, brink-codegen-json ([#544](https://github.com/Syynth/brink/pull/544))
- salsa into brink-db — query-memoized pipeline stages (phase 0 slice B) ([#515](https://github.com/Syynth/brink/pull/515))
- #@local implies VISITS counting ([#496](https://github.com/Syynth/brink/pull/496)) ([#507](https://github.com/Syynth/brink/pull/507))

## [0.0.11](https://github.com/Syynth/brink/compare/brink-compiler-v0.0.10...brink-compiler-v0.0.11) - 2026-07-11

### Other

- Merge pull request #495 from Syynth/bronch/compiler-local-var-keyword-0fdbbc
- #@local directive — flow-private scope through HIR/LIR/codegen ([#473](https://github.com/Syynth/brink/pull/473))

## [0.0.10](https://github.com/Syynth/brink/compare/brink-compiler-v0.0.9...brink-compiler-v0.0.10) - 2026-07-10

### Other

- Story::new takes Arc<Program>, not &Program
- *(runtime)* Program → Arc, delete <'p> lifetime (F1.1) ([#442](https://github.com/Syynth/brink/pull/442))

## [0.0.7](https://github.com/Syynth/brink/compare/brink-compiler-v0.0.6...brink-compiler-v0.0.7) - 2026-06-20

### Other

- welcoming README (playground/book links, overview, LLM-experiment disclaimer) ([#285](https://github.com/Syynth/brink/pull/285))

## [0.0.4](https://github.com/Syynth/brink/compare/brink-compiler-v0.0.3...brink-compiler-v0.0.4) - 2026-06-15

### Fixed

- *(#187)* tunnel calls aren't terminal (E033) + resolve diagnostics to paths (#190)

## [0.0.3](https://github.com/Syynth/brink/compare/brink-compiler-v0.0.2...brink-compiler-v0.0.3) - 2026-06-13

### Added

- host capability manifest — Tier 1 + closed Tier 2 (Track B MVP) ([#74](https://github.com/Syynth/brink/pull/74))

### Fixed

- *(compiler)* surface syntax errors + reject malformed inline conditionals (closes #44) ([#48](https://github.com/Syynth/brink/pull/48))
