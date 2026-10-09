---
"@brink-lang/studio": patch
---

Every `brink.toml` warning now reaches Problems as a row on `brink.toml`
(#3671), not only an unresolvable `[dialogue]`: a host manifest that cannot
be loaded, an unknown key, an unknown lint code. They are warnings, coded
`config`; a `[dialogue]` refusal stays an error coded `dialogue:config`. They
still go to the Output log too.
