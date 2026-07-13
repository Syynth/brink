---
"@brink-lang/web": patch
---

TM-4b (#665): the struct compiler surface lands — grammar, HIR, and
analyzer, diagnostics-only (codegen lands with TM-4c, #666), per
`docs/typed-mode-spec.md` §6.

- **Grammar** (brink-syntax): `STRUCT Name = #{ field: type, … }`
  declarations (single-line or multi-line — the body mirrors the
  construction literal's shape); `Name#{field: expr, …}` construction
  literals in expression position; postfix `.field` access wherever the
  existing dotted-`PATH` grammar doesn't already cover it (a bare
  `ident.ident` chain still parses as one `PATH`, unchanged). All brink
  extension syntax — superset grammar, byte-identical CST for every
  non-struct program.
- **HIR** (brink-ir): `StructDecl`/`StructFieldDecl` items, `Expr::StructLiteral`,
  `Expr::FieldAccess`, `SymbolKind::Struct` manifest registration.
- **Analyzer** (brink-analyzer): resolution fallback for field access
  (static dotted paths like `knot.stitch`/`List.Item` resolve first and
  win; only a head resolving to a variable/temp/param makes `.field` a
  field access); dialect gate flags every new construct under strict-ink
  (`E051`); `Ty::Struct` nominal joins the TM-2 annotation grammar
  (declared struct names no longer trip `E061`); strict-mode-only
  construction checks naming the offending field — missing (`E069`), extra
  (`E070`), mistyped (`E071`); unresolved shape names (`E068`).
- **LIR**: struct constructs (construction literals, field access — both
  the new grammar and the ambiguous-path resolution-fallback case) reject
  with a real, non-suppressible `E072` diagnostic — the T1b-1 discipline
  (grammar/HIR/analyzer land before codegen) plus the E053-backstop lesson
  (a real diagnostic, not a `debug_assert!`-guarded silent drop).

Wasm-observable surface: the parser accepts the new grammar (new
`SyntaxKind`s reach `brink-ide`/`brink-web`'s CST-derived tooling); five new
diagnostic codes (`E068`-`E072`) can now be produced and surfaced through
`brink-web`'s diagnostics API; `editor_dto::symbol_kind_str` gains a
`"struct"` arm (was previously unreachable — a new `SymbolKind` variant);
the semantic-tokens legend gains a 13th token type, `"struct"` (existing
indices unchanged, purely additive).

Oracle corpus: unchanged, 5,577 passing episodes — no existing program uses
`STRUCT`/`Name#{…}`/the new field-access grammar, and LIR lowering rejects
every struct construct rather than emitting bytecode.
