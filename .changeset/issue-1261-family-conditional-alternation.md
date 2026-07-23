---
"@brink-lang/web": patch
---

Native parser (family.rs): four grammar fixes (ruled 2026-07-22) — flat `else if` chains; same-line colon-form `else:` recognition; alternation markers (`!`/`~`/`&`/`|`) win over interpolation with a `{(!x)}` paren escape and a malformed-alternation diagnostic for `{|x| x}`; and empty alternations `{~}`/`{&}` now emit a diagnostic. Observable through the web editor's parse/diagnostics for `.brink` files.
