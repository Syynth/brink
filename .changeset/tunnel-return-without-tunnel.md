---
"@brink-lang/web": patch
---

A `->->` that runs with no tunnel to return from is now an error, as it is
in ink (#166): "Found tunnel onwards statement (->->), when expected end of
flow (-> END or choice)", or "…function return statement (~ return)" inside
a function. brink used to end the flow quietly. This comes up after a plain
divert or a host `choose_path_string` jump into a knot that ends with
`->->`. The line before the `->->` is still delivered first.
