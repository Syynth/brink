# E176 — divert-with-args trim (issue #3428)

`accuse` declares one param; the divert over-supplies two
(`"Hastings"`, `"Poirot"`). Empirically (`brink play` against this exact
shape): the runtime binds the **trailing** supplied argument, printing
`I accuse Poirot!` — not `Hastings`, which a naive "trim the extra trailing
argument" reading would keep. See
`crates/internal/brink-ide/src/arity_trim_fix.rs`'s module doc. The `Safe`
trim removes the **leading** `"Hastings"` and keeps `"Poirot"`: both sides
print `I accuse Poirot!`.
