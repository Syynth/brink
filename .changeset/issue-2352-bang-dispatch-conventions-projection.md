---
"@brink-lang/web": patch
---

Analyzer/IDE: `ConventionsProjection` now carries a row for every declared
`!name` sigil-dispatch (`@[element(args = "…")]`) handler, not just
`@[convention]` claiming handlers (issue #2352).

`ConventionsProjection::from_decls` gains a second row source
(`dispatch_decls: &[DispatchHandlerDecl]`) and a new `dispatch` field,
populated by `brink_db::queries::analysis::conventions_projection_query`
from the conventions module's own `HirFile::dispatch_handlers`. Before this
fix, a project with only a `!name`-dispatched handler projected to a
completely empty `ConventionsProjection` — the handler was structurally
invisible to every consumer reading `EditorSession`'s live projection
(`self.session.db().conventions_projection()`), the same read
`explain_match_impl` uses.

Dispatch rows live in a separate `dispatch` list rather than merged into
`entries`: a `!name` handler has no real precedence to compare against a
claim handler's authored `order` (dispatch is an O(1) name-keyed lookup,
never a ranked walk over competing handlers), and every row's `attach` is
always `None` (`@[element]` has no `attach` clause at all). Wiring this row
into `classify_line`'s interactive matching walk (so `explainMatch` reports
`matched: true` for a real `!name` line) is left as follow-up work — this
slice only makes the row exist and be reachable, which is what issue #2352
asks for.

Each dispatch row also carries a `dispatch_name`, separate from `name`: the
handler's own function name is the declaration-site anchor, but the `!`
-sigil spelling an author actually writes is the `name = "…"` alias when one
is declared — a consumer matching a raw `!name` line against this
projection must key off `dispatch_name`, not `name`, or an aliased handler
becomes unfindable under its only author-writable spelling.

**Known limitation, left open pending a ruling on #2352**: `!name` dispatch
is file-local at the language level — a `!name` line only ever resolves
against handlers declared in the same file — but `dispatch` is populated
only from the ONE configured conventions-module file, the same scope
`entries` uses. A `!name` handler declared in an ordinary (non-conventions)
story file — the common case, since dispatch has no confinement rule the
way `@[convention]` does — contributes no row here at all. See
`ConventionsProjection::dispatch`'s own doc for the same limitation stated
in full.
