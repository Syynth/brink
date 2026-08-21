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
