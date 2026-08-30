---
"@brink-lang/studio": patch
---

Settings chrome follows the theme, and the Project/App switch is legible in all of them

The scope switch drew its selected half in `--bs-panel-bg` — the same
surface the modal already sits on in most themes — so which scope you were
on came down to `--bs-fg` vs `--bs-fg-muted`. That step is only large
enough to see where a theme happens to make it large, which is why
inky-dark read fine and the rest did not. It is now accent-filled:
`--bs-on-accent` is defined as the colour legible on `--bs-accent`, so the
contrast comes from the token contract rather than from luck.

Underneath it, a chunk of the Settings UI could not respond to the theme at
all. `mocha` is the bare-class default, so it defines the raw Catppuccin
palette for every theme while the others override only the semantic
`--bs-*` layer — meaning `var(--ctp-base, …)` resolved to mocha's dark
blue-grey under all five, including the light ones. That pinned the
Settings rail and the toggle track. The same shape appeared as
`var(--bs-bg, #1e1e2e)`, where no theme defines `--bs-bg` at all and the
"fallback" was simply the value: that one put dark dropdowns in the middle
of latte's Settings.

Both are gone, along with `--bs-draft` and `--bs-accent-secondary`, which
were read but defined nowhere and so drew fixed Catppuccin peach and mauve
in every theme; each theme now names its own. Two guards keep the class
from returning: theme-agnostic chrome may not read a raw `--ctp-*` token,
and may not fall back to a literal colour for a token no theme defines.
