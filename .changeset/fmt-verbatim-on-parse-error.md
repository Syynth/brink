---
"@brink-lang/web": patch
---

#603: formatting a T1b `~ { … }` multi-line logic block whose CST subtree
contains a parse error (mid-edit or otherwise malformed input) now bails to
byte-for-byte verbatim pass-through for that block only, instead of running
it through `render_logic_block`'s indentation-aware reindenting. #602's
reindenting assumed a well-formed subtree; on malformed input it could
corrupt the block (a trailing `//` comment between an `if` condition and its
`{` swallowed the brace onto the comment line; a multi-line call under a
parse error injected spurious blank lines and broke idempotence; a lone
`else`/brace line got mangled into mismatched braces). Well-formed `~ { … }`
blocks are unaffected and continue to reindent as before; everything outside
`~ { … }` blocks is untouched.

This is reachable from the web playground: `brink-web` depends on
`brink-ide`, which calls `brink_fmt::format` in `code_actions.rs` and
`formatting.rs`; `brink-web` exposes those as the `code_actions` /
`code_actions_doc` / `resolve_code_action` wasm-bindgen methods (the
"Format knot" / "Format stitch" code actions). Running a code action on a
knot/stitch containing a malformed `~ { … }` block (the normal state of a
block mid-edit) now leaves it untouched instead of corrupting it.
