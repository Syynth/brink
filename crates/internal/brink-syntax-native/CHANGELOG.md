# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.12](https://github.com/Syynth/brink/compare/brink-syntax-native-v0.0.11...brink-syntax-native-v0.0.12) - 2026-08-15

### Added

- *(brink-ir)* @[convention(..., attach = StructName)] declared output schema
- *(brink-syntax-native)* accept a bare-integer value in an annotation clause
- *(brink-ir)* choice-guard `as` binding lowers for real ([#1508](https://github.com/Syynth/brink/pull/1508))
- *(brink-syntax-native,brink-ir)* implement `!name` sigil dispatch ([#2004](https://github.com/Syynth/brink/pull/2004))
- *(brink-syntax-native)* implement \! / \@ as line-start escapes (§8d.6, #1744)
- *(lang)* type-name conformance sweep — angle brackets + Uppercase non-primitives, Option<T>/Weighted<T> annotatable ([#1552](https://github.com/Syynth/brink/pull/1552))
- *(brink-syntax-native)* inline markup grammar — XML spans, escape set, nesting doctrine ([#1716](https://github.com/Syynth/brink/pull/1716))
- *(brink-ir,brink-syntax-native)* @[element]/@[style] annotation declaration surface ([#1719](https://github.com/Syynth/brink/pull/1719))
- *(native)* array/sequence literals `[1, 2, 3]` (NG-D, #1490)
- *(brink-syntax-native)* widen struct_field to the type_expr production ([#1505](https://github.com/Syynth/brink/pull/1505))
- *(native)* `: type` annotation grammar — params, bindings, returns (#1487, #1488, #1489)
- *(compiler)* B1 or-coalescing surface spelling on the native dialect ([#1460](https://github.com/Syynth/brink/pull/1460))
- *(brink-syntax-native)* unquoted `::`-path arg grammar for annotations
- *(brink-syntax-native, brink-ir)* body-dialect selectors (~{ }/>{ }) + fn code-ground default ([#1309](https://github.com/Syynth/brink/pull/1309))
- *(brink-syntax-native,brink-ir)* native return/break/continue, compound assign ([#1322](https://github.com/Syynth/brink/pull/1322))
- *(brink-syntax-native,brink-ir)* native code-ground control-flow (if/while/for/until) + statement lowering ([#1177](https://github.com/Syynth/brink/pull/1177))
- *(brink-syntax-native)* statement-grammar skeleton (parser only) — B0.8 Wave A

### Fixed

- *(brink-syntax-native)* repair stale test broken by attach's ident-value grammar
- *(brink-syntax-native)* apply PR #2065 review findings for #1883
- *(brink-syntax-native,brink-ir)* ~{ } logic block and ~ until (await) grammar at prose-body position
- *(prose-dialect-spec)* apply review findings for #1814 §4.7
- *(brink-syntax-native)* fix backslash-pair escape stripping to match markup::escape
- *(prose-dialect)* strip recognized escape's backslash in tag/cue-name/scene-title text ([#2045](https://github.com/Syynth/brink/pull/2045))
- *(brink-syntax-native)* apply review findings for PR #2042's #1738 audit
- *(brink-syntax-native,brink-ir)* \# escapes the tag/cue-name terminator ([#1738](https://github.com/Syynth/brink/pull/1738))
- *(brink-ir,brink-syntax-native)* apply review findings for #1992's prose-line escape (PR #2028)
- *(brink-syntax-native)* apply review findings for #1973's return-value grammar ([#2027](https://github.com/Syynth/brink/pull/2027))
- *(brink-ir)* escape leading `!name` on native emit, close #2004 review gaps
- *(brink-ir,brink-syntax-native,brink-respell)* apply PR #2015 review findings
- *(brink-syntax-native,brink-ir)* prose-body statement grammar for temp decl + emitter parity ([#1972](https://github.com/Syynth/brink/pull/1972))
- *(brink-syntax-native)* logic-line recovery stops at partial progress and block boundaries (#1991 review F2/F3)
- *(brink-syntax-native)* `~ stmt` content-ground line escape no longer swallowed as prose ([#1991](https://github.com/Syynth/brink/pull/1991))
- *(brink-syntax-native)* flush trivia before line-start ESCAPE node (PR #1978 review)
- *(brink-syntax-native)* address PR #1871 review findings
- use bitwise AND for even check instead of modulo
- *(brink-syntax-native)* escaped-backslash brace detection + depth-guarded colon stops
- *(brink-syntax-native)* cue_name() free-text scan tracks brace depth ([#1786](https://github.com/Syynth/brink/pull/1786))
- *(#1787)* rule content::tag() per-tag brace scoping, add e2e fixture
- *(#1552)* repair three more stale-casing parser test assertions
- *(review)* #1728 tag() brace-depth review fixes — escaped-brace exclusion + honest tradeoff docs
- *(brink-syntax-native)* tag() free-text scan tracks brace depth so an embedded interpolation/alternation brace can't fool the enclosing block's closer
- *(brink-syntax-native)* a documented second heading stays a flat sibling (#1715 review)
- *(brink-syntax-native)* require the declaration-header brace to trail the line (#1715 review)
- *(brink-syntax-native)* satisfy clippy::while_let_loop after the interpolation-loop shrink
- *(brink-syntax-native)* stop folding {…} interpolation into annotation values (#1724 review)
- *(ir)* repair the #1490/#1685 merge resolution that turned CI red
- *(native)* address review findings on array-literal PR ([#1687](https://github.com/Syynth/brink/pull/1687))
- *(brink-syntax-native/fuzz)* correct gitignore comment + fix self-contradicting doc
- *(brink-syntax-native)* diagnose leading `::` in use-decl instead of silent prose fallthrough
- *(brink-syntax-native)* tighten at_use_decl lookahead to reject leading ::
- *(review)* address PR #1525 review findings
- *(review)* merge main into B1b as-binding + apply reviewer findings ([#1475](https://github.com/Syynth/brink/pull/1475))
- *(review)* doc-ordering, fn(...) emission, and naming fixes for #1496
- *(analyzer)* surface coalescing mismatches as E066, fix eager-eval and precedence gaps (#1469 review)
- *(brink-syntax-native)* restore fn body-dialect fixtures broken by code-ground default
- *(brink-syntax-native)* brace_family proptest keyword filter missed while/for/in/until/as
- *(brink-syntax-native)* add `in` to proptest KEYWORDS lists (stops flake)
- *(brink-syntax-native)* prune dead use-tree bare-group branch ([#1277](https://github.com/Syynth/brink/pull/1277))
- *(brink-syntax-native)* `{|…}` is always a stopping-sequence (drop malformed-lambda heuristic)
- *(review)* restore the THREAD arm dropped by colon_body_line (#1263 regression)
- *(brink-syntax-native)* family.rs conditional-arm + alternation-marker fixes
- *(brink-syntax-native)* preserve inter-interpolation whitespace + choice trailing tags
- *(brink-syntax-native)* address review findings on trivia/proptest coverage
- *(brink-syntax-native)* review fixes for #1198 annotation coverage
- *(brink-syntax-native)* review fixes for #1197 brace-family coverage
- *(brink-syntax-native)* review fixes for #1195 choice test coverage
- *(brink-syntax-native)* review fixes for #1246 expression test coverage

### Other

- Merge remote-tracking branch 'origin/main' into train-fix
- sync prose-dialect/directive-annotations/compiler specs with the @[convention] split
- *(prose-dialect)* resolve #1883's remaining tag()/cue_name() asymmetries
- *(prose-dialect)* correct #2045 migration recipe and stale doc comments
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge pull request #2007 from Syynth/auto/issue-1996
- Merge pull request #1845 from Syynth/auto/issue-1838
- *(#1786)* guard cue_name over-correction, pin alternation parity, doc COLON residual
- *(#1787)* add compile-level tag-text assertion, fix mis-cited authority
- cargo fmt
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge remote-tracking branch 'origin/main' into fix-1685
- Merge origin/main into train-fix
- fix doc_markdown lint warning in at_use_decl comment
- apply rustfmt to match code style
- *(brink-syntax-native)* grammar-coverage completeness gate for SyntaxKind ([#1200](https://github.com/Syynth/brink/pull/1200))
- Merge remote-tracking branch 'origin/main' into train-fix
- merge main + address review: wire E084/E106 to native surface, name E138's key
- Merge remote-tracking branch 'origin/main' into train-fix
- gate fuzz-crate lockfiles against staleness (review finding, PR #1398)
- rustfmt the KEYWORDS lists after adding `in`
- Merge remote-tracking branch 'origin/main' into train-pr
- *(brink-syntax-native)* port fuzz setup from brink-syntax ([#1273](https://github.com/Syynth/brink/pull/1273))
- Merge pull request #1271 from Syynth/auto/issue-1261
- Merge pull request #1267 from Syynth/auto/issue-1251
- cargo fmt after merge conflict resolution
- Merge remote-tracking branch 'origin/main' into train-pr
- origin/main into train-fix
- Merge remote-tracking branch 'origin/main' into train-fix
- *(brink-syntax-native)* correct false #1199 ownership claim in content.rs tests
- *(brink-syntax-native)* reframe comment-absorption fixmes as open design question
- Merge origin/main into train-fix for PR #1248
- *(brink-syntax-native)* fix stale AT/ERROR_TOKEN doc comments
- Merge origin/main into train-fix
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge origin/main into train-fix
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge remote-tracking branch 'origin/main' into train-pr
- *(brink-syntax-native)* fix review-blocking gaps in #1192 declaration coverage
- *(brink-syntax-native)* exhaustive declaration-family parser coverage ([#1192](https://github.com/Syynth/brink/pull/1192))
